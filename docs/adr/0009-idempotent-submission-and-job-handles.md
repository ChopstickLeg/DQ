# Idempotent submission with typed job handles

## Status

Accepted. Key retention, key scope, payload comparison, and callback lifecycle remain open.

## Context

The server can record a job but lose its connection before the caller receives confirmation. Callers also need convenient typed APIs while preserving job identity across restarts.

## Decision

Use client-generated idempotency keys for submission. A resubmission with the same key returns the original job, while reuse with different arguments fails. Generated submission methods return typed job handles after durable acceptance; handles support completion waiting and result retrieval. Provide a submit-and-wait convenience method. Callback support is desired but its semantics have not yet been selected.

## Consequences

Deduplicating admission does not prevent duplicate execution attempts. Key-to-job associations need persistent state and a retention policy. Handles can be reconstructed using job identity; in-memory awaiters do not survive process restarts. Explicit versions and signature compatibility checks are outside the initial scope.
