use std::sync::Arc;

use opentelemetry_otlp::tonic_types::metadata;
use sqlx::{Execute, SqlitePool};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::proto::queue_service_server::QueueService;
use crate::proto::{
    EnqueueRequest, EnqueueResponse, ExecuteRequest, GetJobStatusRequest, GetJobStatusResponse,
    SubscribeRequest,
};
use crate::telemetry::{DispatchJob, Instrumentation};

pub struct QueueServiceHandler {
    pool: SqlitePool,
    dispatch_tx: tokio::sync::mpsc::Sender<DispatchJob>,
    instrumentation: Arc<Instrumentation>,
}

impl QueueServiceHandler {
    pub fn new(
        pool: SqlitePool,
        dispatch_tx: tokio::sync::mpsc::Sender<DispatchJob>,
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
    type SubscribeStream = ReceiverStream<Result<ExecuteRequest, Status>>;

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

    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let (metadata, _, req) = request.into_parts();
        self.instrumentation
            .rpc("Subscribe", &metadata, self.subscribe(req))
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

        if let Err(error) = self
            .dispatch_tx
            .send(self.instrumentation.dispatch_job(id.clone()))
            .await
        {
            tracing::error!(target: "dq::grpc", job_id = %id, error = %error, "Job saved, but dispatcher notification failed");
            return Err(Status::internal(format!(
                "Failed to send job to dispatcher: {}",
                error
            )));
        }

        tracing::info!(target: "dq::grpc", job_id = %id, "Job accepted");
        let response = EnqueueResponse { job_id: id };

        Ok(Response::new(response))
    }

    async fn fetch_job_status(
        &self,
        req: GetJobStatusRequest,
    ) -> Result<Response<GetJobStatusResponse>, Status> {
        let result = sqlx::query_as!(
            GetJobStatusResponse,
            r#"
                SELECT
                    status AS "status: i32",
                    function_name,
                    service_name,
                    caller,
                    arguments
                FROM jobs
                WHERE id = ?
                "#,
            req.job_id
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|error| match error {
            sqlx::Error::RowNotFound => Status::not_found("Job not found"),
            error => Status::internal(format!("Failed to fetch job status: {error}")),
        })?;

        Ok(Response::new(result))
    }

    async fn subscribe(
        &self,
        req: SubscribeRequest,
    ) -> Result<Response<<QueueServiceHandler as QueueService>::SubscribeStream>, Status> {
        let (tx, rx) = mpsc::channel(32);
    }
}
