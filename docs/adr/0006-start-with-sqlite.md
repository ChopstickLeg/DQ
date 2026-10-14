# Start with embedded SQLite

## Status

Accepted. Durability settings and future backend design remain open.

## Context

The initial Parrot workload is not expected to exhaust SQLite write capacity. A custom storage engine would add a separate learning and correctness burden.

## Decision

Use embedded SQLite for initial persistent job state. Drive scheduling through notifications and timers rather than constant database polling. Consider a backend with greater concurrent write capacity later when measured workloads justify it; do not select that backend now.

## Consequences

SQLite permits one active writer per database file, not just one write per second. Queue correctness must rely on durable job state transitions rather than a database-specific API exposed to callers. Sync settings and acknowledgment timing need to implement the accepted durability contract. No throughput target or benchmark result is established.

Reference: [SQLite concurrency guidance](https://www.sqlite.org/whentouse.html).
