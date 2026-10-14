# Generate typed clients from opted-in functions

## Status

Accepted. Exact schema, portable type mapping, and generator packaging remain open.

## Context

Worker functions should be defined in ordinary code without user-authored application .proto files. Callers in C# and Python still need a shared description of their signatures.

## Decision

Expose explicitly marked functions using Python decorators or C# attributes. Require supported type declarations, initially limited to primitives, nullable values, lists, and structured records. Extract a contract manifest and generate typed C# and Python clients from it. Generation must also work from a saved manifest without a running worker. Reject unsupported signatures at registration.

## Consequences

The worker code is the contract source. Manifest extraction and client generation are separate tooling responsibilities. Generated clients can provide typed method calls, but SDK return behavior, serialization, and compatibility handling remain open. Explicit function versioning is deferred from version one; a future versioning feature should be configurable by environment.
