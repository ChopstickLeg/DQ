# Single server with durable admission and bounded submissions

## Status

Accepted. Storage implementation and numeric limits remain open.

## Context

The project needs crash recovery and preservation of accepted work, but finite storage prevents unlimited acceptance. Replicated operation would expand the initial implementation substantially.

## Decision

Version one runs a single queue server with persistent local storage. Acceptance follows durable recording. Preserve accepted pending jobs without age-based expiration, and apply backpressure or explicit rejection to new submissions when capacity bounds are reached. Submission timeouts are configurable. Retry interrupted work and transient failures with backoff; after a configurable attempt limit, retain failure state and allow manual retry.

## Consequences

The service is unavailable while the server is down and cannot recover from loss of its disk. Preserving accepted jobs does not guarantee successful execution of arbitrary functions. An ambiguous submission response still needs a deduplication policy. Storage implementation, ordering, and numeric limits remain undecided.
