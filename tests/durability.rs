use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use prost::Message;
use sqlx::Connection;
use sqlx::sqlite::SqliteConnectOptions;
use tonic::Code;
use tonic::transport::{Channel, Endpoint};

mod proto {
    tonic::include_proto!("dq.v1");
}

use proto::queue_service_client::QueueServiceClient;
use proto::{EnqueueRequest, GetJobStatusRequest, GetJobStatusResponse, JobStatus};

#[derive(Clone, PartialEq, Message)]
struct SmokeArguments {
    #[prost(string, tag = "1")]
    message: String,
    #[prost(bytes = "vec", tag = "2")]
    binary: Vec<u8>,
}

struct ServerProcess {
    child: Child,
    endpoint: String,
}

impl ServerProcess {
    fn start(directory: &Path) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_dq"))
            .current_dir(directory)
            .env("DATABASE_URL", "sqlite://dq.db")
            .env("LISTEN_ADDR", "127.0.0.1:0")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start the Rust server");
        let mut server = Self {
            child,
            endpoint: String::new(),
        };
        let stdout = server.child.stdout.take().expect("capture server stdout");
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
            let _ = sender.send(result);
        });
        let line = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("server must report its listening address within 10 seconds")
            .expect("read server startup message");
        let address = line
            .trim()
            .strip_prefix("Listening on ")
            .expect("server must report its bound address after listening");
        server.endpoint = format!("http://{address}");
        server
    }

    async fn client(&self) -> QueueServiceClient<Channel> {
        let channel = Endpoint::from_shared(self.endpoint.clone())
            .expect("valid server endpoint")
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(5))
            .connect()
            .await
            .expect("connect to the Rust server");
        QueueServiceClient::new(channel)
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[tokio::test]
async fn independent_jobs_have_distinct_ids_and_preserve_their_arguments() {
    let directory = tempfile::tempdir().expect("create a temporary data directory");
    let server = ServerProcess::start(directory.path());
    let mut client = server.client().await;
    let requests = [
        EnqueueRequest {
            function_name: "smoke.echo".to_owned(),
            submission_key: "first-job".to_owned(),
            arguments: SmokeArguments {
                message: "first job".to_owned(),
                binary: vec![0, 128],
            }
            .encode_to_vec(),
            caller: "rust-smoke".to_owned(),
        },
        EnqueueRequest {
            function_name: "smoke.echo".to_owned(),
            submission_key: "second-job".to_owned(),
            arguments: SmokeArguments {
                message: "second job".to_owned(),
                binary: vec![255, 0],
            }
            .encode_to_vec(),
            caller: "rust-smoke".to_owned(),
        },
    ];
    let mut job_ids = Vec::new();
    for request in &requests {
        let job_id = client
            .enqueue(request.clone())
            .await
            .expect("accept each independent job")
            .into_inner()
            .job_id;
        job_ids.push(job_id);
    }
    assert_ne!(job_ids[0], job_ids[1]);

    for (job_id, request) in job_ids.into_iter().zip(requests) {
        let job = client
            .get_job_status(GetJobStatusRequest { job_id })
            .await
            .expect("retrieve each job by its own ID")
            .into_inner();
        assert_eq!(
            job,
            GetJobStatusResponse {
                status: JobStatus::Pending as i32,
                function_name: request.function_name,
                caller: request.caller,
                arguments: request.arguments,
            }
        );
    }
}

#[tokio::test]
async fn duplicate_submission_is_rejected_without_changing_the_original_job() {
    let directory = tempfile::tempdir().expect("create a temporary data directory");
    let server = ServerProcess::start(directory.path());
    let mut client = server.client().await;
    let request = EnqueueRequest {
        function_name: "smoke.echo".to_owned(),
        submission_key: "duplicate-job".to_owned(),
        arguments: SmokeArguments {
            message: "original job".to_owned(),
            binary: vec![0, 128, 255],
        }
        .encode_to_vec(),
        caller: "rust-smoke".to_owned(),
    };
    let job_id = client
        .enqueue(request.clone())
        .await
        .expect("accept the original job")
        .into_inner()
        .job_id;
    let duplicate = EnqueueRequest {
        arguments: SmokeArguments {
            message: "replacement arguments".to_owned(),
            binary: vec![255, 128, 0],
        }
        .encode_to_vec(),
        ..request.clone()
    };

    let error = client
        .enqueue(duplicate)
        .await
        .expect_err("reject the duplicate function name and submission key");
    assert_eq!(error.code(), Code::AlreadyExists);

    let job = client
        .get_job_status(GetJobStatusRequest { job_id })
        .await
        .expect("retrieve the unchanged original job")
        .into_inner();
    assert_eq!(
        job,
        GetJobStatusResponse {
            status: JobStatus::Pending as i32,
            function_name: request.function_name,
            caller: request.caller,
            arguments: request.arguments,
        }
    );
}

#[tokio::test]
async fn unknown_job_returns_not_found() {
    let directory = tempfile::tempdir().expect("create a temporary data directory");
    let server = ServerProcess::start(directory.path());
    let error = server
        .client()
        .await
        .get_job_status(GetJobStatusRequest {
            job_id: "00000000-0000-0000-0000-000000000000".to_owned(),
        })
        .await
        .expect_err("an unknown job ID must return a gRPC error");
    assert_eq!(error.code(), Code::NotFound);
}

#[tokio::test]
async fn acknowledged_job_survives_process_restart() {
    let directory = tempfile::tempdir().expect("create a temporary data directory");
    let arguments = SmokeArguments {
        message: "persist through restart".to_owned(),
        binary: vec![0, 128, 255],
    };
    let request = EnqueueRequest {
        function_name: "smoke.echo".to_owned(),
        submission_key: "restart-smoke".to_owned(),
        arguments: arguments.encode_to_vec(),
        caller: "rust-smoke".to_owned(),
    };

    let server = ServerProcess::start(directory.path());
    let mut client = server.client().await;
    let job_id = client
        .enqueue(request.clone())
        .await
        .expect("accept the job before restarting")
        .into_inner()
        .job_id;
    assert!(!job_id.is_empty());
    drop(client);
    drop(server);

    let restarted_server = ServerProcess::start(directory.path());
    let job = restarted_server
        .client()
        .await
        .get_job_status(GetJobStatusRequest { job_id })
        .await
        .expect("retrieve the acknowledged job after restarting")
        .into_inner();

    assert_eq!(
        job,
        GetJobStatusResponse {
            status: JobStatus::Pending as i32,
            function_name: request.function_name,
            caller: request.caller,
            arguments: request.arguments,
        }
    );
    assert_eq!(
        SmokeArguments::decode(job.arguments.as_slice()).expect("decode saved Protobuf arguments"),
        arguments
    );
}

#[tokio::test]
async fn failed_commit_is_not_acknowledged_and_submission_can_be_retried() {
    let directory = tempfile::tempdir().expect("create a temporary data directory");
    let server = ServerProcess::start(directory.path());
    let mut client = server.client().await;
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("dq.db"))
        .foreign_keys(true);
    let mut database = sqlx::SqliteConnection::connect_with(&options)
        .await
        .expect("connect to the temporary database for fault setup");

    // This constraint permits the INSERT inside a transaction, but rejects COMMIT.
    sqlx::raw_sql(
        "CREATE TABLE commit_failure_parent (id TEXT PRIMARY KEY);
         CREATE TABLE commit_failure_child (
             job_id TEXT PRIMARY KEY,
             parent_id TEXT NOT NULL REFERENCES commit_failure_parent(id)
                 DEFERRABLE INITIALLY DEFERRED
         );
         CREATE TRIGGER fail_job_commit AFTER INSERT ON jobs BEGIN
             INSERT INTO commit_failure_child (job_id, parent_id)
             VALUES (NEW.id, 'missing-parent');
         END;",
    )
    .execute(&mut database)
    .await
    .expect("install a deferred constraint that fails at commit");

    let mut transaction = database.begin().await.expect("begin a fault setup probe");
    sqlx::query(
        "INSERT INTO jobs (id, function_name, submission_key, arguments, caller)
         VALUES ('probe-id', 'smoke.probe', 'probe-key', X'0801', 'rust-smoke')",
    )
    .execute(&mut *transaction)
    .await
    .expect("the deferred constraint must permit INSERT before COMMIT");
    let error = transaction
        .commit()
        .await
        .expect_err("the deferred constraint must reject COMMIT");
    assert!(
        error
            .as_database_error()
            .is_some_and(|error| error.is_foreign_key_violation())
    );
    database.ping().await.expect("finish the probe rollback");

    let request = EnqueueRequest {
        function_name: "smoke.echo".to_owned(),
        submission_key: "failed-commit-smoke".to_owned(),
        arguments: SmokeArguments {
            message: "retry after failed commit".to_owned(),
            binary: vec![0, 128, 255],
        }
        .encode_to_vec(),
        caller: "rust-smoke".to_owned(),
    };
    let error = client
        .enqueue(request.clone())
        .await
        .expect_err("a failed commit must not return an accepted job ID");
    assert_eq!(error.code(), Code::Internal);

    sqlx::query("DROP TRIGGER fail_job_commit")
        .execute(&mut database)
        .await
        .expect("remove the commit failure");
    database
        .close()
        .await
        .expect("close the fault setup connection");

    // Reusing the same submission key proves the failed transaction left no job.
    let job_id = client
        .enqueue(request.clone())
        .await
        .expect("retry the same submission after removing the commit failure")
        .into_inner()
        .job_id;
    let job = client
        .get_job_status(GetJobStatusRequest { job_id })
        .await
        .expect("retrieve the successfully retried job")
        .into_inner();
    assert_eq!(
        job,
        GetJobStatusResponse {
            status: JobStatus::Pending as i32,
            function_name: request.function_name,
            caller: request.caller,
            arguments: request.arguments,
        }
    );
}
