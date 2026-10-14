# Implement a custom queue for learning and personal use

## Status

Accepted.

## Context

Existing brokers can solve Parrot's integration problem. The author also wants to learn by implementing queue infrastructure in performant C#.

## Decision

Build the queue itself rather than only a client abstraction over an existing broker. Evaluate performance and allocations against the project's own measured goals. Competing with RabbitMQ on speed or market novelty is not a goal.

## Consequences

Durable state, scheduling, and recovery are part of the implementation work. Parrot can use RabbitMQ independently if shipping it becomes the priority. Existing broker designs remain useful references, without becoming a requirement to match their feature sets.
