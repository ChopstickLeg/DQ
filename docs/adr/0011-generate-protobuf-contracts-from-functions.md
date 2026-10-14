# Generate Protobuf contracts from exposed functions

## Status

Accepted. Exact generated API shape remains to be specified.

## Context

The author wants strong typing across C# and Python and binary application payloads from the outset. Manually maintained application .proto files and a JSON-first implementation do not meet that experience.

## Decision

Generate application .proto contracts from opted-in function declarations through the contract manifest. Use gRPC and binary Protobuf payloads. Use language code generation and SDK wrappers to provide typed queued calls and results. Keep the queue server's gRPC protocol fixed: generated SDKs serialize and deserialize application request and result messages, while the server stores and routes opaque binary payloads.

## Consequences

Generation must map supported language types to Protobuf types and define field numbers. Standard code generators provide message types; custom SDK wrappers implement job submission, handles, and completion semantics over the fixed queue protocol. The broker need not load application schemas or be rebuilt when functions are added. Strong typing applies at the SDK and worker boundaries; the broker does not validate application payload schemas. This does not introduce the deferred runtime versioning or signature compatibility feature.

References: [C# Protobuf generation](https://protobuf.dev/reference/csharp/csharp-generated/), [Python Protobuf generation](https://protobuf.dev/reference/python/python-generated/), [Python gRPC generation](https://grpc.io/docs/languages/python/generated-code/).
