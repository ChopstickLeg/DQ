# Address completion subscriptions to stable caller names

## Status

Accepted. Retention remains open.

## Context

Completion handling must resume after caller restarts, and multiple caller instances may connect at the same time. Connection IDs cannot provide stable ownership.

## Decision

Use stable logical caller names for durable completion subscriptions. Instances sharing a name compete to handle each notification, and unacknowledged notifications become available for redelivery. Independent copies use explicitly separate subscriber names. Register durable handlers at application startup and acknowledge notifications after successful handling. Result waits do not consume subscription notifications.

## Consequences

Caller identities and subscription state outlive connections. Redelivery means completion handlers must tolerate duplicates. Notification acknowledgment does not provide exactly-once external side effects. Local callbacks are separate convenience APIs that disappear on process restart. Retention and authorization remain to be specified.
