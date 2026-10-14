# Retry interrupted work and recover completion notifications

## Status

Accepted. Retry limits, retention, and wire protocol remain undecided.

## Context

The worker can finish generating a voice clip and crash before reporting success. The caller can also restart before handling completion.

## Decision

Allow interrupted work to run again, including possible duplicate execution. Persist generation results and provide completion notifications that callers can resume and acknowledge. Keep Discord playback outside the queue's generation-completion contract. Do not expire pending work solely because it has become old.

## Consequences

Job identity and completion state must outlive client connections. Repeated execution must be tolerated by worker handlers. Repeated completion delivery does not guarantee exactly-once Discord playback. Queue admission, permanently failing jobs, and result retention still require explicit policies; eventual execution cannot imply guaranteed success for arbitrary handlers.
