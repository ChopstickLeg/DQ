# Lease execution attempts and reject superseded results

## Status

Accepted. Lease durations, renewal protocol, and late-result boundaries remain open.

## Context

A worker can lose its connection while continuing to generate audio. The queue must recover abandoned jobs without requiring immediate termination of disconnected work.

## Decision

Allow a disconnected worker to finish active work and reconnect to report. Use renewable execution leases and separate attempt identities. Abandoned jobs become retryable; a superseded attempt cannot overwrite the result of a newer attempt. The worker keeps counting active work against local concurrency until execution actually ends.

## Consequences

Partitions and lease expiry can produce duplicate computation. A lease does not forcibly stop the old execution. The server must distinguish valid completion from superseded results and persist the relevant state transitions. Behavior when a lease expires but no successor exists remains to be specified.
