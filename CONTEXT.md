# Queued remote work

This project will implement a custom durable queue for background work between services, with an initial C# caller and Python worker use case. Its goals are learning, a performant C# implementation with few allocations, and replacing repeated database polling with a simpler developer experience. Outperforming RabbitMQ and product novelty are not goals.

## Language

**Caller**:
The application that requests remote work and consumes its completion. In Parrot, this is the C# Discord application.

**Worker**:
The application that exposes functions and executes accepted work within configured capacity. In Parrot, this is the Python service that generates voice clips with F5TTS.

**Job**:
A requested execution of an exposed worker function that must remain recoverable across disconnects and process crashes. Acceptance follows durable recording; interrupted execution can be retried and exhausted retries leave a retained failed job that supports manual retry.
_Avoid_: Local method call, transient RPC

**Concurrency limit**:
The maximum number of jobs allowed to execute concurrently within its configured scope. The agreed default scope is one worker process; shared resource limits are deferred until a real deployment needs them.

**Completion notification**:
A recoverable notification that a job has finished, which the caller can resume receiving after a restart and acknowledge. Discord playback is a separate application action, not part of generation completion.

**Contract manifest**:
A portable description of explicitly exposed worker functions, their supported argument and result types, and execution configuration. It is extracted from worker code and is the input to typed client generation.

**Execution attempt**:
One worker execution of a job, identified separately from the job and tracked by a renewable lease. A superseded attempt cannot overwrite the result of a newer attempt.

**Job handle**:
A typed client reference returned after durable job acceptance. It includes the job ID and supports waiting for completion and retrieving a result independently of the original submission call.

**Submission key**:
A client-generated idempotency key identifying one intended submission. Resubmission with the same key returns the existing job; reuse with different arguments fails.

**Completion subscription**:
A durable stream of completion notifications addressed to a stable logical caller name. Instances using that name compete for notifications; unacknowledged notifications can be delivered again.

**Dead-letter queue**:
Retained terminally failed jobs available for inspection and manual retry. Its default retention is thirty days, configurable to another duration or indefinite retention.

## Settled requirements

- Learning and solving the author's own service-to-service background work needs are the primary goals.
- Implement the queue itself as part of the learning project. RabbitMQ remains an alternative for Parrot if shipping that app takes priority.
- The initial example is Parrot: a C# Discord application submits work to a Python F5TTS worker, then receives completion and plays the generated clip in Discord.
- Version one has a single queue server with persistent local storage. It recovers after restarts, is unavailable while down, and does not protect against loss of its disk.
- Accepted work must survive disconnects and process crashes. Acceptance follows durable recording.
- Repeated database polling for work and completion is a problem this project should address.
- Concurrency limits default to per-process scope. No numeric default has been selected.
- Interrupted work may execute again, and duplicate execution is acceptable.
- Persist generation results and provide resumable, acknowledged completion notifications. Discord playback remains separate application logic.
- Accepted jobs remain pending until execution; automatic expiration due to age is not desired. New submissions may wait or fail explicitly when backlog or storage bounds are reached, with configurable submission timeouts.
- Interrupted executions and transient failures retry with backoff. A configurable attempt limit leads to retained terminal failure and supports manual retry.
- Worker functions are exposed automatically from code declarations. Users should not maintain .proto files for application functions.
- Large artifacts live outside the queue. Workers return references and metadata; the application manages artifact retention. Small return values may travel through the queue.
- Use SQLite initially. A future backend with greater write concurrency is desired if measured workloads justify it; no replacement or multi-backend architecture has been selected.
- Functions opt in through Python decorators or C# attributes. Signatures use explicit types from a portable subset: primitives, nullable values, lists, and structured records. Unsupported signatures fail registration.
- Extract a portable contract manifest and generate typed C# and Python clients from it. Generation can use a saved manifest without requiring a live worker.
- A disconnected worker may finish active work and reconnect to report. Execution leases permit retries after abandonment, and superseded attempt results cannot overwrite newer results. Active disconnected work still occupies a local concurrency slot.
- Explicit function versioning is deferred from version one. If added later, its behavior should be configurable by environment. No configuration mechanism or compatibility policy has been selected.
- Signature fingerprints, mismatch blocking, and compatibility management for queued jobs are also deferred. Registration-time supported-type validation remains in scope.
- Use client-generated submission keys to deduplicate admission. A retry with the same key returns the existing job, while different arguments under that key fail.
- Generated submission methods return typed job handles after durable acceptance. Provide convenience methods to submit and wait for completion, and local callbacks on job handles.
- Address durable completion subscriptions to stable logical caller names. Instances sharing a name compete for notifications, and unacknowledged notifications become available again. Independent copies require explicitly separate subscriber names.
- Applications register durable completion handlers at startup. Acknowledge notifications after handlers succeed; failures permit redelivery. Waiting for a result does not consume its durable subscription notification. Local callbacks do not survive process restarts.
- Use gRPC and binary Protobuf for application arguments and results. Generate application .proto files from opted-in function declarations through the manifest; do not use JSON application payloads as an initial shortcut.
- Enforce both a process-wide execution ceiling and per-function ceilings. Execution requires capacity under both limits.
- Job cancellation is deferred from version one. Stopping a caller's completion wait does not stop accepted work. A later cancellation feature should at least attempt cooperative interruption of running work.
- The queue server uses a fixed gRPC protocol and stores and routes opaque binary Protobuf application payloads. Generated SDKs serialize and deserialize typed function request and result messages; adding a function does not require rebuilding the queue server.
- Cleanup is configurable, including indefinite retention. Successful jobs default to deletion seven days after completion, with results preserved while needed by unacknowledged notifications. Failed jobs default to thirty days in the dead-letter queue. Retain submission keys while their job records remain; deleting those records ends deduplication.
- Schedule ready jobs FIFO within each function, and use round-robin scheduling across functions with available capacity. Delayed retries become eligible after backoff. Completion ordering is not guaranteed.
- Version one serves the author's trusted services in private deployments. Use a shared API token and TLS between machines. Multi-tenant access, per-function permissions, and user management are deferred.

## Implementation details to specify

- SQLite sync behavior and future storage evolution.
- Exact portable type mapping and client generation packaging.
- Retry and lease defaults, and coordination with external side effects.
- Cleanup representation for records protected by unacknowledged notifications.
- Submission key scope, retention, and cross-language payload comparison.
- Queue admission defaults and protocol limits.

These are implementation details within the accepted scope, not reasons to reopen deferred features. The author confirmed the final design summary on 2026-10-13, completing the grilling interview.
