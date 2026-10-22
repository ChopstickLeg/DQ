# DQ queue protocol

The fixed gRPC protocol is defined in [`proto/dq.proto`](../proto/dq.proto), under
the `dq.v1` package. The service is `dq.v1.QueueService`.

## Enqueue

`Enqueue` accepts these fields:

| Field | Meaning |
| --- | --- |
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
`function_name`, `caller`, and `arguments`. Despite the RPC's name, its response
includes the saved job metadata and arguments as well as status.

The job ID remains valid after a process restart when the server opens the same
database file. Callers retrieve jobs through gRPC. They do not read or poll
SQLite directly.

## Durable state transitions

| Event | Persisted state | RPC result |
| --- | --- | --- |
| A new submission commits | A new pending job exists. | `EnqueueResponse` with its job ID. |
| A submission fails to insert or commit | That submission creates no job. | A gRPC error. |
| A duplicate submission arrives | The existing job is unchanged. | `ALREADY_EXISTS`. |
| The server restarts | An acknowledged pending job remains pending. | Subsequent lookup returns the saved job. |

The status values have these meanings:

| Value | Name | Current behavior |
| --- | --- | --- |
| 0 | `JOB_STATUS_UNSPECIFIED` | Protobuf default. Not assigned to new jobs. |
| 1 | `JOB_STATUS_PENDING` | Accepted and awaiting execution. The only status assigned by this server. |
| 2 | `JOB_STATUS_COMPLETED` | Reserved for successful execution. No transition is implemented yet. |
| 3 | `JOB_STATUS_ERROR` | Reserved for failed execution. No transition is implemented yet. |

Enqueue accepts work. It does not execute the function. Worker execution and
transitions out of pending are outside this first slice.

## Error responses

| RPC | Condition | gRPC status |
| --- | --- | --- |
| `Enqueue` | A job already has the same function name and submission key. | `ALREADY_EXISTS` |
| `Enqueue` | SQLite cannot insert or commit the job, or the pool is unavailable. | `INTERNAL` |
| `GetJobStatus` | No job has the requested ID. | `NOT_FOUND` |
| `GetJobStatus` | The database lookup fails. | `INTERNAL` |

Clients use the status code rather than parsing error text. `INTERNAL` is an RPC
failure, not the `JOB_STATUS_ERROR` execution state. No application-level request
validation is implemented yet. For example, an empty function name is not
currently rejected with `INVALID_ARGUMENT`.

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
