# Expose application functions automatically from code

## Status

Accepted. Discovery, supported types, generation, and transport remain open.

## Context

The author prefers declaring worker functions in code over maintaining application .proto definitions, while the initial workload crosses C# and Python.

## Decision

Derive exposed application functions from code declarations rather than requiring user-authored .proto contracts. Keep large generated artifacts outside the queue, returning references and metadata instead. Applications own artifact retention.

## Consequences

The libraries must extract and communicate function contracts across languages. Automatic exposure does not eliminate the need for supported types and versioning. A fixed internal gRPC protocol remains a candidate; this decision does not select it. The queue's completion record does not guarantee continued availability of externally stored artifacts.
