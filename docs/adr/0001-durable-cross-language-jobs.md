# Durable jobs across C# and Python services

## Status

Accepted for project scope. Storage, transport, and failure guarantees remain open.

## Context

Parrot receives Discord requests in C#, validates them, and sends voice generation work to a Python F5TTS worker. The current implementation repeatedly polls a database for work and completion. The author wants to replace that integration pattern and requires work to survive disconnects and crashes.

## Decision

Design for durable jobs and cross-language clients from the outset. An in-memory queue tied to a live RPC connection is insufficient for the accepted requirements. Concurrency limits default to a worker process; a shared limit has not been accepted.

## Consequences

Acceptance, recovery, and completion need persistent state somewhere, whether in an existing broker or a custom service. Cross-language contracts are part of the initial scope. A pending C# Task or an in-memory callback alone cannot recover a result after the caller restarts. Durability does not by itself guarantee exactly-once execution of worker functions or external side effects.
