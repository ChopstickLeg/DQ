# FIFO scheduling within functions and private deployment

## Status

Accepted.

## Context

Multiple exposed functions share worker capacity, and the initial users are the author's own trusted services.

## Decision

Schedule ready jobs FIFO within each function. Select among eligible functions in round-robin order, subject to process and function capacity ceilings. Delayed retries become eligible after backoff. Deploy privately using a shared API token and TLS between machines. Defer priorities, strict global ordering, multiple tenants, per-function permissions, and user management.

## Consequences

Concurrent execution and retries can change completion order. Scheduling gives eligible functions turns without promising equal execution time. The initial access model trusts connected services rather than isolating unrelated users.
