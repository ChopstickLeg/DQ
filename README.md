# dq

Rust gRPC queue service with SQLite storage through SQLx. See the
[protocol reference](docs/protocol.md) for job fields, acceptance guarantees,
state transitions, error responses, and durability settings.

## Build and run the service

Install Rust through rustup and install the Protocol Buffers compiler (`protoc`).
The repository's `rust-toolchain.toml` selects Rust 1.99.1, rustfmt, and Clippy.
Keep `Cargo.lock` in version control to preserve dependency resolution.

The `sqlx::query!` call requires a SQLite database
with the current schema at compile time. Set `DATABASE_URL` to that database
before building, or supply it in `.env` for the SQLx macros. Offline query
metadata is not included yet. Runtime startup migrations cannot supply a schema
for compile-time query checks.

Run the service with the locked dependency versions:

```powershell
cargo run --locked
```

By default, the server listens on `127.0.0.1:50051`, creates `dq.db` in the working
directory, and applies the embedded migrations. It prints `Listening on` followed
by the bound address after opening the listening socket.

`Enqueue` stores a function name, submission key, binary Protobuf arguments, and
logical caller. It returns a generated job ID after the pending job commits.
Workers receive jobs through `SubscribeExecution` and report the outcome with
`CompleteExecution`. Clients can read outcomes through `SubscribeCompletion` or
look up a known job ID with `GetJobStatus`.

## Configure the server

Set runtime configuration through environment variables:

| Variable | Default | Meaning |
| --- | --- | --- |
| `DATABASE_URL` | `sqlite://dq.db` | SQLite connection URL. The file's parent directory must exist. |
| `LISTEN_ADDR` | `127.0.0.1:50051` | IP address and port for gRPC. Port `0` requests an available port. |

For example, run on a different port with a different database:

```powershell
$env:DATABASE_URL = 'sqlite://queue.db'
$env:LISTEN_ADDR = '127.0.0.1:50052'
cargo run --locked
```

The server does not load `.env` at runtime. SQLx macros can read `.env` during
compilation, but that does not configure the running process. For a temporary
database, use `sqlite::memory:`. Jobs in that database do not survive a restart.

## Run the integration checks

See [telemetry configuration](docs/telemetry.md) to export OpenTelemetry logs,
traces, and metrics. Export is disabled until an OTLP endpoint is configured.

Run the gRPC integration checks:

```powershell
cargo test --locked --test durability
```

The checks launch the actual Rust executable with temporary data directories and
ports assigned by the operating system:

- `independent_jobs_have_distinct_ids_and_preserve_their_arguments` submits two
  jobs with different submission keys and argument bytes. It verifies distinct
  IDs and retrieves the correct saved job for each ID.
- `duplicate_submission_is_rejected_without_changing_the_original_job` submits
  the same function name and submission key with different arguments. It checks
  `ALREADY_EXISTS` and verifies that the original job remains unchanged.
- `unknown_job_returns_not_found` checks that lookup of an unknown job ID returns
  `NOT_FOUND`.
- `acknowledged_job_survives_process_restart` submits an explicitly defined
  Protobuf message, kills the server process, and launches a new process with the
  same database. It retrieves the acknowledged job over gRPC and verifies pending
  status, metadata, exact argument bytes, and the decoded application message.
- `failed_commit_is_not_acknowledged_and_submission_can_be_retried` installs a
  temporary deferred SQLite constraint. It confirms that an insert inside a
  transaction succeeds but its commit fails. It then verifies that gRPC rejects
  the submission with `INTERNAL`. After removing the failure, it submits the same
  request again and retrieves the pending job, proving no partial job blocked
  the retry.
- `pending_job_is_reassigned_and_its_committed_result_is_broadcast` checks
  pending work after restart, worker reassignment, stale attempt rejection,
  fan-out to two listeners, and completion retry after restart.

The failure check accesses SQLite only to arrange and remove the fault. Job
submission and retrieval assertions use gRPC. The
[C# SDK](sdks/csharp/README.md) and [Python SDK](sdks/python/dq_python/README.md)
also have integration tests that start the Rust server.

See [CI and releases](docs/ci-release.md) for the required GitHub Actions check,
release tags, and registry setup.

Run the remaining checks with:

```powershell
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

## Change the schema

Add a numbered SQL file to `migrations/`, for example `0003_add_priority.sql`.
Rebuild and restart the service to apply it. Keep existing migrations unchanged
once applied, because SQLx checks their checksums on startup.

