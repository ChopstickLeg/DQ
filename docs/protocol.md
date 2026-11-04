# DQ queue protocol

The gRPC protocol is defined in [`proto/dq.proto`](../proto/dq.proto), under
the `dq.v1` package. The service is `dq.v1.QueueService`.

## Enqueue

`Enqueue` accepts these fields:

| Field | Meaning |
| --- | --- |
| `service_name` | The service that owns the function. |
| `function_name` | The function the caller wants a worker to execute. |
| `submission_key` | A caller-supplied submission identifier, unique within a function name. |
| `arguments` | Opaque binary Protobuf application arguments. |
| `caller` | The logical caller name supplied in the request. |

The server stores these fields, generates a UUID job ID, and sets the job status
to `JOB_STATUS_PENDING`. A successful response contains the `job_id` only after
SQLite commits the insert. Generating an ID does not itself accept the job.

The server does not load application schemas, decode the argument bytes, or
validate function signatures. Callers encode and decode their application
messages. The queue returns the saved argument bytes unchanged.

The pair `(function_name, submission_key)` is unique across all callers. Reusing
that pair returns `ALREADY_EXISTS`, including when the caller or arguments differ.
The existing job is unchanged. The error does not include its job ID.

## GetJobStatus

`GetJobStatus` accepts a `job_id` and returns the persisted `status`,
`service_name`, `function_name`, `caller`, and `arguments`. A completed job also
includes `result`. A failed job includes `error`.

The job ID remains valid after a process restart when the server opens the same
database file. Callers retrieve jobs through gRPC. They do not read or poll
SQLite directly.

## Execution and completion

`SubscribeExecution` takes a service name and a list of up to 1,000 function
names. A job matches when both its service and function match the subscription.
The dispatcher assigns one job at a time across all workers. A worker receives
the job ID, function name, arguments, and a unique `attempt_id`. Jobs queued
before a worker connects are eligible for delivery.

The worker calls `CompleteExecution` with the job ID, attempt ID, and either
result bytes or an error string. An empty result is a valid success. The server
commits the outcome before acknowledging the worker or notifying listeners.
Retrying the same attempt with the same outcome returns success and does not
publish a second notification.

`SubscribeCompletion` uses the same service and function matching rules. Each
matching listener receives the job ID, service, function, terminal status, and
outcome for completions committed while its subscription is active. If a
listener's server buffer fills, the server closes that stream. Completion
streams have no replay. Listeners can recover missed outcomes for known job IDs
with `GetJobStatus`.

An assigned job stays pending until its outcome commits. If the worker
disconnects or the server restarts, the job is eligible for reassignment with a
new attempt ID. A late report from a superseded attempt is rejected. A connected
worker can hold the assignment until it reports an outcome or disconnects.
Worker side effects must tolerate repeat execution after a disconnect or restart.
Run one server process per queue database.

## Durable state transitions

| Event | Persisted state | RPC result |
| --- | --- | --- |
| A new submission commits | A new pending job exists. | `EnqueueResponse` with its job ID. |
| A submission fails to insert or commit | That submission creates no job. | A gRPC error. |
| A duplicate submission arrives | The existing job is unchanged. | `ALREADY_EXISTS`. |
| A worker reports an outcome for the active attempt. | The job becomes completed or failed. | `CompleteExecutionResponse`. |
| The server restarts | An acknowledged pending job remains pending. | Subsequent lookup returns the saved job. |

The status values have these meanings:

| Value | Name | Current behavior |
| --- | --- | --- |
| 0 | `JOB_STATUS_UNSPECIFIED` | Protobuf default. Not assigned to new jobs. |
| 1 | `JOB_STATUS_PENDING` | Queued or assigned to a worker. |
| 2 | `JOB_STATUS_COMPLETED` | The worker reported a result, and the server saved it. |
| 3 | `JOB_STATUS_ERROR` | The worker reported an error, and the server saved it. |

## Error responses

| RPC | Condition | gRPC status |
| --- | --- | --- |
| `Enqueue` | A job already has the same function name and submission key. | `ALREADY_EXISTS` |
| `Enqueue` | SQLite cannot insert or commit the job, or the pool is unavailable. | `INTERNAL` |
| `GetJobStatus` | No job has the requested ID. | `NOT_FOUND` |
| `GetJobStatus` | The database lookup fails. | `INTERNAL` |
| `CompleteExecution` | The job ID, attempt ID, or outcome is missing. | `INVALID_ARGUMENT` |
| `CompleteExecution` | The job ID is unknown. | `NOT_FOUND` |
| `CompleteExecution` | The attempt is stale or the retry supplies a different outcome. | `FAILED_PRECONDITION` |
| `CompleteExecution` | The database operation fails. | `INTERNAL` |
| `CompleteExecution` | The dispatcher is unavailable. | `UNAVAILABLE` |

Clients use the status code rather than parsing error text. A job enters
`JOB_STATUS_ERROR` when the server commits a worker-reported error.

A transport failure or timeout does not establish whether a submission committed.
The server may have committed before the response was lost. Retrying the same
function name and submission key then returns `ALREADY_EXISTS`. Lookup requires
the job ID, so a caller that lost the acceptance response cannot recover that ID
through the current API.

## SQLite durability settings

The server explicitly uses `journal_mode=WAL` and `synchronous=FULL` on its SQLite
connections. In WAL mode, commits append to the write-ahead log. SQLite can
recover those committed changes after the process exits without a clean shutdown.
FULL requests a log synchronization at each commit, subject to the filesystem
and storage device honoring synchronization requests.

Each enqueue uses one autocommitted insert. The handler awaits its completion
before returning success and propagates database errors instead of returning an
accepted job ID. SQLx performs blocking SQLite operations on dedicated worker
threads and communicates with the async runtime through channels.

The pool permits five connections and uses a five-second SQLite busy timeout.
The default database is `dq.db` in the working directory. `DATABASE_URL` selects
a different file. `sqlite::memory:` is temporary and does not preserve jobs after
a process restart.

The repeatable checks and runtime configuration are described in the
[`README`](../README.md).
