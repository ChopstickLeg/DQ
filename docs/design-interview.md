# Queued RPC design interview

Interview notes. Accepted requirements live in [CONTEXT.md](../CONTEXT.md); architectural decisions live in [docs/adr](adr/). Recommendations below are not accepted decisions.

## Stated intent

Explore an app or package that exposes registered functions to remote callers through a client library. Calls wait for execution capacity, then report completion through a callback or tracked status. The implementation is an opportunity to write high-performance C# with few allocations. gRPC is a candidate, not a decision.

## Repository baseline

On 2026-10-13, the workspace was empty. There were no existing source files, domain glossary, agent instructions, or ADRs.

## Existing systems

- [RabbitMQ RPC tutorial for .NET](https://www.rabbitmq.com/tutorials/tutorial-six-dotnet): request and reply over a message broker. A method-call API does not remove the underlying messages.
- [Hangfire background methods](https://docs.hangfire.io/en/latest/background-methods/index.html): background jobs expressed as method calls.
- [Hangfire concurrency and rate limiting](https://docs.hangfire.io/en/latest/background-processing/throttling.html): per-job concurrency controls through Hangfire.Ace, with documented best-effort guarantees.
- [NATS request-reply](https://docs.nats.io/concepts/request-reply): requests distributed among responders in a queue group. This does not establish durable job execution.
- [Temporal .NET SDK](https://docs.temporal.io/develop/dotnet): Activities, Workers, and Workflows. Compare further if durable execution becomes a requirement.
- [gRPC core concepts](https://grpc.io/docs/what-is-grpc/core-concepts/): generated client APIs, remote calls, streaming, and deadlines. Durable scheduling would require application logic beyond the transport.

## Initial assessment

The generic pattern alone is not a demonstrated product advantage. A learning project is worthwhile without novelty. A narrow library for bounded queued calls is a candidate to investigate, not an agreed scope or an established market gap.

## First-round answers

1. Success means learning and solving personal service-to-service background work needs. Hangfire's database polling and paid Redis package are objections.
2. Parrot is the representative example: C# Discord request handling, Python F5TTS generation, completion notification, and Discord playback. Runtime, argument size, and burst rates are still unspecified.
3. Work must survive disconnects and crashes.
4. Limits default to per-process scope. Configurable shared limits are a possibility, not a settled requirement.

## Updated assessment

The use case justifies a project for personal utility and learning. Its job semantics now matter more than the RPC-shaped API. After the second round, the author selected implementing a custom queue because building infrastructure is itself a learning goal. Product novelty and outperforming RabbitMQ are not goals.

[JetStream consumers](https://docs.nats.io/nats-concepts/jetstream/consumers) support durable consumer state, acknowledgments, and redelivery. [RabbitMQ work queues](https://www.rabbitmq.com/tutorials/tutorial-two-python) provide a Python example of acknowledgments and durable work. These are relevant comparisons to polling a jobs table; ordinary transient request/reply is insufficient on its own.

The F5TTS example does not yet establish a throughput or allocation bottleneck in the queue layer. Performance goals need a representative benchmark rather than assuming job dispatch dominates voice generation.

## Second-round answers

1. Implement the queue itself. RabbitMQ remains a fallback for shipping Parrot.
2. Interrupted work can run again; duplicate execution is acceptable.
3. Adopt persisted results, resumable and acknowledged completion notifications, and separate application logic for Discord playback.
4. Keep per-process limits as the default and defer shared resource limits until needed.
5. Every request should eventually run. Do not adopt age-based expiration. This answer does not yet settle whether every submission must be accepted or how permanent failures behave.

## Third-round answers

1. Adopt one queue server with persistent local storage and recovery after restart. Replication and protection from disk loss are outside version one.
2. Adopt durable acceptance, bounded admission, and submission backpressure or explicit failure. Already accepted jobs remain queued.
3. Adopt backoff, configurable attempt limits, retained failed jobs, and manual retry.
4. Prefer automatic function exposure from code over user-authored .proto contracts. The exact extraction and client generation design remains open.
5. Keep large artifacts outside the queue; return references and metadata. Applications own artifact lifetimes.

## Fourth-round answers

1. Adopt explicit decorators or attributes, required type declarations, and a small portable type subset.
2. Adopt manifest extraction and generated typed clients for C# and Python. The author wants an explanation of the concrete workflow.
3. Start with SQLite. Consider a more concurrent write backend later if needed; no specific replacement is chosen.
4. Adopt renewable leases, attempt IDs, completion after reconnect, and prevention of superseded result overwrites.
5. Defer explicit function versions from the initial implementation. If implemented later, versioning should be configurable by environment. Handling incompatible signatures without versions remains open.

## Fifth-round answers

1. Defer signature compatibility checks as well as explicit function versions. The author does not want this feature initially.
2. Adopt client-generated idempotency keys: repeat submissions return the existing job; conflicting reuse fails.
3. Adopt typed job handles and submit-and-wait convenience methods. Callback support is desired; concrete lifecycle semantics remain open.
4. Adopt stable caller names and durable competing completion subscriptions, with independent copies only for explicit separate subscribers.

## Sixth-round answers

1. Adopt local callbacks and durable registered handlers, acknowledging after successful handling. Result waits do not consume subscription notifications.
2. Use gRPC with generated application .proto contracts and binary Protobuf payloads from the outset. Reject the JSON-first proposal.
3. Enforce process-wide and per-function limits together.
4. Defer job cancellation. A later feature should attempt to stop running jobs cooperatively. Stopping a wait leaves accepted work intact.
5. Make retention configurable. Completed jobs default to seven days; failed jobs remain in a dead-letter queue for a configurable duration. Exact failed retention and unacknowledged cleanup interactions remain open.

## Seventh-round answers

1. Keep the queue server protocol fixed and transport opaque binary Protobuf payloads. Generated SDKs own typed application serialization and deserialization.
2. Adopt configurable retention, including indefinite retention: seven days for successful jobs after completion, protecting results needed by unacknowledged notifications; thirty days for failed jobs. Retain submission keys while job records exist.
3. Adopt FIFO within each function and round-robin selection across eligible functions. No completion-order guarantee.
4. Scope version one to trusted private services, with a shared API token and TLS between machines. Defer multi-tenant permissions and user management.

## Interview state

Complete. The author explicitly confirmed the [design summary](design-summary.md) on 2026-10-13. The major scope and behavior frontier is settled, domain terminology is recorded in CONTEXT.md, and architectural decisions are recorded in ADRs. Numeric tuning, exact schema mapping, and packaging can be specified within this scope without reopening deferred features. No further interview question blocks specification; implementation has not begun.
