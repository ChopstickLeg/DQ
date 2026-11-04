use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::dispatch::DispatchMessage;
use crate::proto::queue_service_server::QueueService;
use crate::proto::{
    CompleteExecutionRequest, CompleteExecutionResponse, EnqueueRequest, EnqueueResponse,
    ExecuteRequest, ExecuteResponse, GetJobStatusRequest, GetJobStatusResponse, SubscribeRequest,
};
use crate::telemetry::Instrumentation;

pub struct QueueServiceHandler {
    pool: SqlitePool,
    dispatch_tx: mpsc::Sender<DispatchMessage>,
    instrumentation: Arc<Instrumentation>,
}

impl QueueServiceHandler {
    pub fn new(
        pool: SqlitePool,
        dispatch_tx: mpsc::Sender<DispatchMessage>,
        instrumentation: Arc<Instrumentation>,
    ) -> Self {
        Self {
            pool,
            dispatch_tx,
            instrumentation,
        }
    }
}

#[tonic::async_trait]
impl QueueService for QueueServiceHandler {
    type SubscribeExecutionStream = ReceiverStream<Result<ExecuteRequest, Status>>;
    type SubscribeCompletionStream = ReceiverStream<Result<ExecuteResponse, Status>>;

    async fn enqueue(
        &self,
        request: Request<EnqueueRequest>,
    ) -> Result<Response<EnqueueResponse>, Status> {
        let (metadata, _, req) = request.into_parts();
        self.instrumentation
            .rpc("Enqueue", &metadata, self.enqueue_job(req))
            .await
    }

    async fn get_job_status(
        &self,
        request: Request<GetJobStatusRequest>,
    ) -> Result<Response<GetJobStatusResponse>, Status> {
        let (metadata, _, req) = request.into_parts();
        self.instrumentation
            .rpc("GetJobStatus", &metadata, self.fetch_job_status(req))
            .await
    }

    async fn subscribe_execution(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeExecutionStream>, Status> {
        let (metadata, _, req) = request.into_parts();
        self.instrumentation
            .rpc(
                "SubscribeExecution",
                &metadata,
                self.subscribe_execution(req),
            )
            .await
    }

    async fn subscribe_completion(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeCompletionStream>, Status> {
        let (metadata, _, req) = request.into_parts();
        self.instrumentation
            .rpc(
                "SubscribeCompletion",
                &metadata,
                self.subscribe_completion(req),
            )
            .await
    }

    async fn complete_execution(
        &self,
        request: Request<CompleteExecutionRequest>,
    ) -> Result<Response<CompleteExecutionResponse>, Status> {
        let (metadata, _, req) = request.into_parts();
        self.instrumentation
            .rpc("CompleteExecution", &metadata, self.report_completion(req))
            .await
    }
}

impl QueueServiceHandler {
    async fn enqueue_job(&self, req: EnqueueRequest) -> Result<Response<EnqueueResponse>, Status> {
        let id = Uuid::now_v7().to_string();
        sqlx::query!(
            "INSERT INTO jobs (id, function_name, service_name, submission_key, arguments, caller)
                VALUES (?, ?, ?, ?, ?, ?)",
            id,
            req.function_name,
            req.service_name,
            req.submission_key,
            req.arguments,
            req.caller,
        )
        .execute(&self.pool)
        .await
        .map_err(|error| match error {
            sqlx::Error::Database(db_error) => {
                if db_error.is_unique_violation() {
                    Status::already_exists(
                        "Job with the same function_name and submission_key already exists",
                    )
                } else {
                    Status::internal(format!("Database error: {}", db_error))
                }
            }
            _ => Status::internal(format!("Failed to enqueue job: {}", error)),
        })?;

        let response = EnqueueResponse { job_id: id };
        if let Err(error) = self
            .dispatch_tx
            .send(DispatchMessage::Enqueue {
                request: req,
                response: response.clone(),
                trace_context: self.instrumentation.dispatch_context(),
            })
            .await
        {
            tracing::error!(target: "dq::grpc", job_id = %response.job_id, error = %error, "Job saved, but dispatcher notification failed");
            return Err(Status::internal(format!(
                "Failed to send job to dispatcher: {}",
                error
            )));
        }

        tracing::info!(target: "dq::grpc", job_id = %response.job_id, "Job accepted");

        Ok(Response::new(response))
    }

    async fn fetch_job_status(
        &self,
        req: GetJobStatusRequest,
    ) -> Result<Response<GetJobStatusResponse>, Status> {
        let (status, function_name, service_name, caller, arguments, result, error) =
            sqlx::query_as(
                "SELECT status, function_name, service_name, caller, arguments, result, error
             FROM jobs WHERE id = ?",
            )
            .bind(req.job_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|error| match error {
                sqlx::Error::RowNotFound => Status::not_found("Job not found"),
                error => Status::internal(format!("Failed to fetch job status: {error}")),
            })?;

        Ok(Response::new(GetJobStatusResponse {
            status,
            function_name,
            service_name,
            caller,
            arguments,
            result,
            error,
        }))
    }

    async fn report_completion(
        &self,
        req: CompleteExecutionRequest,
    ) -> Result<Response<CompleteExecutionResponse>, Status> {
        if req.job_id.is_empty() || req.attempt_id.is_empty() {
            return Err(Status::invalid_argument(
                "Job ID and attempt ID are required",
            ));
        }
        let outcome = req
            .outcome
            .ok_or_else(|| Status::invalid_argument("Execution outcome is required"))?;
        let (reply, received) = oneshot::channel();
        self.dispatch_tx
            .send(DispatchMessage::CompleteExecution {
                job_id: req.job_id,
                attempt_id: req.attempt_id,
                outcome,
                reply,
            })
            .await
            .map_err(|_| Status::unavailable("Dispatcher is unavailable"))?;
        received
            .await
            .map_err(|_| Status::unavailable("Dispatcher is unavailable"))??;
        Ok(Response::new(CompleteExecutionResponse {}))
    }

    async fn subscribe_execution(
        &self,
        req: SubscribeRequest,
    ) -> Result<Response<<QueueServiceHandler as QueueService>::SubscribeExecutionStream>, Status>
    {
        if req.function_names.len() > 1000 {
            return Err(Status::invalid_argument(
                "SubscribeExecution accepts up to 1000 function names",
            ));
        }
        let (tx, rx) = mpsc::channel(1);
        let connect_message = DispatchMessage::SubscribeExecution {
            id: Uuid::now_v7(),
            request: req,
            tx,
        };
        if let Err(error) = self.dispatch_tx.send(connect_message).await {
            tracing::error!(target: "dq::grpc", error = %error, "Failed to send connection message");
            return Err(Status::internal("Failed to subscribe to execution updates"));
        }
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn subscribe_completion(
        &self,
        req: SubscribeRequest,
    ) -> Result<Response<<QueueServiceHandler as QueueService>::SubscribeCompletionStream>, Status>
    {
        let (tx, rx) = mpsc::channel(32);
        let connect_message = DispatchMessage::SubscribeCompletion {
            id: Uuid::now_v7(),
            request: req,
            tx,
        };
        if let Err(error) = self.dispatch_tx.send(connect_message).await {
            tracing::error!(target: "dq::grpc", error = %error, "Failed to send connection message");
            return Err(Status::internal(
                "Failed to subscribe to completion updates",
            ));
        }
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}
