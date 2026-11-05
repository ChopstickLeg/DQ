use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    sync::Arc,
    time::Duration,
};

use opentelemetry::Context;
use sqlx::SqlitePool;
use tokio::sync::{mpsc, oneshot};
use tonic::Status;
use uuid::Uuid;

use crate::db;
use crate::proto::complete_execution_request::Outcome;
use crate::proto::{
    EnqueueRequest, EnqueueResponse, ExecuteRequest, ExecuteResponse, JobStatus, SubscribeRequest,
};
use crate::telemetry::Instrumentation;

struct Subscription<T> {
    request: SubscribeRequest,
    tx: mpsc::Sender<Result<T, Status>>,
}

struct Assignment {
    worker_id: Uuid,
    job_id: String,
    attempt_id: String,
    service_name: String,
    function_name: String,
}

pub struct Dispatcher {
    pool: SqlitePool,
    workers: VecDeque<(Uuid, Subscription<ExecuteRequest>)>,
    listeners: HashMap<Uuid, Subscription<ExecuteResponse>>,
    active: Option<Assignment>,
    instrumentation: Arc<Instrumentation>,
}

pub enum DispatchMessage {
    Enqueue {
        request: EnqueueRequest,
        response: EnqueueResponse,
        trace_context: Option<Context>,
    },
    SubscribeExecution {
        id: Uuid,
        request: SubscribeRequest,
        tx: mpsc::Sender<Result<ExecuteRequest, Status>>,
    },
    SubscribeCompletion {
        id: Uuid,
        request: SubscribeRequest,
        tx: mpsc::Sender<Result<ExecuteResponse, Status>>,
    },
    CompleteExecution {
        job_id: String,
        attempt_id: String,
        outcome: Outcome,
        reply: oneshot::Sender<Result<(), Status>>,
    },
}

impl Dispatcher {
    pub fn new(pool: SqlitePool, instrumentation: Arc<Instrumentation>) -> Self {
        Self {
            pool,
            workers: VecDeque::new(),
            listeners: HashMap::new(),
            active: None,
            instrumentation,
        }
    }

    pub async fn run(
        mut self,
        mut rx: mpsc::Receiver<DispatchMessage>,
        shutdown: impl Future<Output = ()>,
    ) {
        tokio::pin!(shutdown);
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut shutting_down = false;
        loop {
            tokio::select! {
                _ = &mut shutdown, if !shutting_down => {
                    shutting_down = true;
                    self.workers.clear();
                    self.listeners.clear();
                }
                message = rx.recv() => {
                    let Some(message) = message else { break };
                    self.handle(message, shutting_down).await;
                }
                _ = tick.tick(), if !shutting_down => {}
            }

            if !shutting_down && let Err(error) = self.dispatch_next().await {
                tracing::error!(%error, "Failed to read pending jobs");
            }
        }
    }

    async fn handle(&mut self, message: DispatchMessage, shutting_down: bool) {
        match message {
            DispatchMessage::Enqueue {
                request,
                response,
                trace_context,
            } => {
                tracing::debug!(target: "dq::dispatch", service_name = %request.service_name, function_name = %request.function_name, "Enqueue request received");
                self.instrumentation
                    .received(&response.job_id, trace_context)
                    .await;
            }
            DispatchMessage::SubscribeExecution { id, request, tx } if !shutting_down => {
                self.workers.push_back((id, Subscription { request, tx }));
            }
            DispatchMessage::SubscribeCompletion { id, request, tx } if !shutting_down => {
                self.listeners.insert(id, Subscription { request, tx });
            }
            DispatchMessage::SubscribeExecution { .. }
            | DispatchMessage::SubscribeCompletion { .. } => {}
            DispatchMessage::CompleteExecution {
                job_id,
                attempt_id,
                outcome,
                reply,
            } => {
                let result = self.complete(job_id, &attempt_id, outcome).await;
                let _ = reply.send(result);
            }
        }
    }

    async fn dispatch_next(&mut self) -> Result<(), sqlx::Error> {
        self.workers.retain(|(_, worker)| !worker.tx.is_closed());
        self.listeners
            .retain(|_, listener| !listener.tx.is_closed());
        if let Some(active) = &self.active {
            if self.workers.iter().any(|(id, _)| *id == active.worker_id) {
                return Ok(());
            }
            self.active = None;
        }

        for (index, (worker_id, worker)) in self.workers.iter().enumerate() {
            let Some(job) = db::next_pending_job(
                &self.pool,
                &worker.request.service_name,
                &worker.request.function_names,
            )
            .await?
            else {
                continue;
            };
            let assignment = Assignment {
                worker_id: *worker_id,
                job_id: job.id.clone(),
                attempt_id: Uuid::new_v4().to_string(),
                service_name: worker.request.service_name.clone(),
                function_name: job.function_name.clone(),
            };
            if worker
                .tx
                .try_send(Ok(ExecuteRequest {
                    job_id: job.id,
                    function_name: job.function_name,
                    arguments: job.arguments,
                    attempt_id: assignment.attempt_id.clone(),
                }))
                .is_ok()
            {
                self.active = Some(assignment);
                self.workers.rotate_left(index + 1);
                break;
            }
        }
        Ok(())
    }

    async fn complete(
        &mut self,
        job_id: String,
        attempt_id: &str,
        outcome: Outcome,
    ) -> Result<(), Status> {
        let (status, result, error) = match outcome {
            Outcome::Result(result) => (JobStatus::Completed, Some(result), None),
            Outcome::Error(error) => (JobStatus::Error, None, Some(error)),
        };
        let Some(active) = self
            .active
            .as_ref()
            .filter(|active| active.job_id == job_id && active.attempt_id == attempt_id)
        else {
            return match db::completion_matches(
                &self.pool,
                &job_id,
                attempt_id,
                result.as_deref(),
                error.as_deref(),
            )
            .await
            .map_err(completion_storage_error)?
            {
                Some(true) => Ok(()),
                Some(false) => Err(Status::failed_precondition(
                    "Execution attempt is no longer active or has a different outcome",
                )),
                None => Err(Status::not_found("Job not found")),
            };
        };

        let completion = ExecuteResponse {
            job_id,
            service_name: active.service_name.clone(),
            function_name: active.function_name.clone(),
            status: status as i32,
            result,
            error,
        };
        let saved = db::save_completion(&self.pool, &completion, attempt_id)
            .await
            .map_err(completion_storage_error)?;
        self.active = None;
        if !saved {
            return Err(Status::failed_precondition("Job is no longer pending"));
        }
        self.listeners.retain(|_, listener| {
            if listener.request.service_name == completion.service_name
                && listener
                    .request
                    .function_names
                    .contains(&completion.function_name)
            {
                // End a lagging stream so it can reconnect and recover via GetJobStatus.
                return listener.tx.try_send(Ok(completion.clone())).is_ok();
            }
            true
        });
        Ok(())
    }
}

fn completion_storage_error(error: sqlx::Error) -> Status {
    tracing::error!(%error, "Failed to access the job outcome");
    Status::internal("Failed to access the job outcome")
}
