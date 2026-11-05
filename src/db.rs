use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};

use crate::proto::{ExecuteResponse, JobStatus};

pub async fn connect(database_url: &str) -> Result<SqlitePool, Box<dyn std::error::Error>> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}

#[derive(sqlx::FromRow)]
pub struct PendingJob {
    pub id: String,
    pub function_name: String,
    pub arguments: Vec<u8>,
}

pub async fn next_pending_job(
    pool: &SqlitePool,
    service_name: &str,
    function_names: &[String],
) -> Result<Option<PendingJob>, sqlx::Error> {
    if function_names.is_empty() {
        return Ok(None);
    }
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT id, function_name, arguments FROM jobs WHERE status = 1 AND service_name = ",
    );
    query
        .push_bind(service_name)
        .push(" AND function_name IN (");
    let mut names = query.separated(", ");
    for name in function_names {
        names.push_bind(name);
    }
    query.push(") ORDER BY id LIMIT 1");
    query
        .build_query_as()
        .persistent(false)
        .fetch_optional(pool)
        .await
}

pub async fn save_completion(
    pool: &SqlitePool,
    completion: &ExecuteResponse,
    attempt_id: &str,
) -> Result<bool, sqlx::Error> {
    let update = sqlx::query(
        "UPDATE jobs SET status = ?, result = ?, error = ?, completed_attempt_id = ?,
                         completed_at = CURRENT_TIMESTAMP WHERE id = ? AND status = ?",
    )
    .bind(completion.status)
    .bind(completion.result.as_deref())
    .bind(completion.error.as_deref())
    .bind(attempt_id)
    .bind(&completion.job_id)
    .bind(JobStatus::Pending as i32)
    .execute(pool)
    .await?;
    Ok(update.rows_affected() == 1)
}

pub async fn completion_matches(
    pool: &SqlitePool,
    job_id: &str,
    attempt_id: &str,
    result: Option<&[u8]>,
    error: Option<&str>,
) -> Result<Option<bool>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT completed_attempt_id IS ? AND result IS ? AND error IS ? FROM jobs WHERE id = ?",
    )
    .bind(attempt_id)
    .bind(result)
    .bind(error)
    .bind(job_id)
    .fetch_optional(pool)
    .await
}
