# Durable queued RPC: version-one design

Accepted by the author on 2026-10-13. The grilling interview is complete. This is a scope agreement, not a complete implementation specification.

## Purpose

Build a custom queue in performant C# for learning and personal service-to-service background work. Parrot is the first representative workload: a C# Discord application submits voice generation to a Python F5TTS worker and handles completion. Market novelty and outperforming RabbitMQ are not objectives.

## Architecture

- One C# queue server, with embedded SQLite on persistent local storage. Server downtime stops service; disk loss is outside the recovery guarantee.
- C# and Python SDKs. Explicit attributes or decorators expose worker functions from a supported portable type subset.
- Function declarations produce a manifest, generated application .proto files, and typed clients. Saved manifests support offline generation.
- A fixed internal gRPC protocol transports binary Protobuf application payloads. SDKs own their types and codecs; the broker stores and routes bytes without application-schema validation.
- Large artifacts live outside the queue. The application owns their storage and lifetime; jobs return references and metadata.

## Acceptance and execution

- A submission is accepted only after durable recording. Bounded admission can wait or fail explicitly; accepted work does not expire because it is old.
- Client-generated submission keys deduplicate admission. Conflicting reuse fails; repeated submission returns the same job.
- Scheduling is FIFO among ready jobs within a function, and round-robin across eligible functions.
- Execution requires capacity under both process-wide and per-function limits. Shared limits across processes are deferred.
- Renewable leases identify execution attempts. Disconnected workers can finish and reconnect; abandoned work retries, and superseded results cannot overwrite newer attempts.
- Duplicate execution is acceptable. Transient failures retry with backoff and a configurable attempt limit. Terminal failures enter a retained dead-letter queue and can be retried manually.

## Caller experience

- Generated submission methods return typed job handles after durable acceptance.
- Convenience APIs support submission followed by waiting, plus local callbacks. Neither a live wait nor an in-memory callback survives a caller process restart.
- Durable completion subscriptions use stable logical caller names. Instances sharing a name compete for notifications; separate subscriber names can receive independent copies.
- Applications register durable completion handlers at startup. Successful handling acknowledges notifications; unacknowledged notifications can be redelivered.
- Waiting for a result does not consume the durable notification. Discord playback is application logic, with no exactly-once side-effect guarantee.
- Stopping a wait does not cancel accepted work. Job cancellation is deferred.

## Retention and deployment

- Successful jobs default to cleanup seven days after completion, while preserving results needed by unacknowledged notifications.
- Failed jobs default to thirty-day dead-letter retention. Retention is configurable, including indefinite retention.
- Submission keys remain while job records exist; record deletion ends deduplication.
- Initial deployments serve trusted private services using a shared API token and TLS between machines.

## Deferred features

- Explicit function versions, signature fingerprints, and queued-contract compatibility checks. Future versioning should be configurable by environment.
- Job cancellation, shared resource limits, replication, automatic failover, multi-tenant permissions, priorities, and strict global ordering.
- A replacement database or custom storage engine unless measurements justify it.

## Implementation discipline

Prove durable state transitions with crash and reconnect scenarios. Measure throughput, tail latency, allocations per operation, and retained memory using representative payloads. Do not claim low allocation or high throughput without measurements. Numeric limits, lease and retry timings, exact type mappings, and packaging remain specification details.

See [CONTEXT.md](../CONTEXT.md), [interview notes](design-interview.md), and [ADRs](adr/).
