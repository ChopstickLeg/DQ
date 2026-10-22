use sqlx::SqlitePool;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::proto::queue_service_server::QueueService;
use crate::proto::{EnqueueRequest, EnqueueResponse, GetJobStatusRequest, GetJobStatusResponse};

#[derive(Debug)]
pub struct QueueServiceHandler {
    pool: SqlitePool,
}

impl QueueServiceHandler {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[tonic::async_trait]
impl QueueService for QueueServiceHandler {
    async fn enqueue(
        &self,
        request: Request<EnqueueRequest>,
    ) -> Result<Response<EnqueueResponse>, Status> {
        let req = request.into_inner();
        let id = Uuid::now_v7().to_string();
        sqlx::query!(
            "INSERT INTO jobs (id, function_name, submission_key, arguments, caller)
                VALUES (?, ?, ?, ?, ?)",
            id,
            req.function_name,
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

        Ok(Response::new(response))
    }

    async fn get_job_status(
        &self,
        request: Request<GetJobStatusRequest>,
    ) -> Result<Response<GetJobStatusResponse>, Status> {
        let req = request.into_inner();
        let result = sqlx::query_as!(
            GetJobStatusResponse,
            r#"
                SELECT
                    status AS "status: i32",
                    function_name,
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
}
