# Initial concurrency, callback, and cleanup boundaries

## Status

Accepted. Cleanup representation remains to be specified.

## Context

The initial queue needs execution limits and usable completion handling without expanding into job cancellation or retaining all terminal records forever.

## Decision

Enforce both process-wide and per-function concurrency ceilings. Offer local callbacks and startup-registered durable completion handlers, acknowledging notifications after successful handling. Result waits do not consume durable notifications. Defer job cancellation; stopping a wait does not cancel accepted work. Make cleanup configurable, including indefinite retention. Successful jobs default to seven days after completion, with results preserved while needed by unacknowledged notifications; failed jobs default to thirty days in a dead-letter queue. Retain submission keys while their job records exist.

## Consequences

Local callbacks disappear on restart, while durable handlers resume through subscriptions. Redelivery can repeat application side effects. Jobs continue after callers stop waiting. Cleanup cannot discard results needed for unacknowledged notifications. Deleting job records and submission keys ends deduplication, so a later submission with the same key can create new work.
