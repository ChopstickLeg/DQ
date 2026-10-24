# Telemetry configuration

DQ exports logs, traces, and metrics over OTLP to an OpenTelemetry Collector or
another compatible receiver. Export is opt-in. The app does not contact a default
localhost collector when endpoints are absent.

## Endpoints

| Variable | Behavior |
| --- | --- |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Shared endpoint for all three signals. |
| `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` | Trace endpoint. Overrides the shared endpoint. |
| `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT` | Log endpoint. Overrides the shared endpoint. |
| `OTEL_EXPORTER_OTLP_METRICS_ENDPOINT` | Metric endpoint. Overrides the shared endpoint. |
| `OTEL_SDK_DISABLED=true` | Disables all telemetry, even when endpoints are configured. |

A signal-specific variable takes precedence when present. An empty or
whitespace-only value explicitly disables that signal, including when a shared
endpoint is configured. An absent variable falls back to the shared endpoint.
Without either endpoint, that signal is disabled.

Endpoint configuration is read at startup. Restart the app to apply changes.
Set environment variables in the shell or process configuration. The app does not
load `.env` at runtime.

## Transport and identity

The default transport is `http/protobuf`. The SDK appends `/v1/traces`, `/v1/logs`,
and `/v1/metrics` to a shared HTTP endpoint. Signal-specific HTTP endpoints are
complete URLs and must include the receiver's signal path.

Set `OTEL_EXPORTER_OTLP_PROTOCOL=grpc` for gRPC. A gRPC endpoint normally uses
port `4317` and has no signal path. HTTP normally uses port `4318`. DQ supports
HTTPS with system trust roots for both transports.

| Variable | Default or purpose |
| --- | --- |
| `OTEL_SERVICE_NAME` | `dq`. Identifies this service in the backend. |
| `OTEL_RESOURCE_ATTRIBUTES` | Additional resource attributes, such as `deployment.environment.name=development`. |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `http/protobuf` or `grpc`. |
| `OTEL_EXPORTER_OTLP_HEADERS` | Export headers, for example `authorization=Bearer%20TOKEN`. |
| `OTEL_EXPORTER_OTLP_TIMEOUT` | SDK export timeout in milliseconds. |
| `OTEL_METRIC_EXPORT_INTERVAL` | Metric export interval in milliseconds. SDK default is 60000. |
| `OTEL_TRACES_SAMPLER` | SDK trace sampler. Default is `parentbased_always_on`. |
| `OTEL_TRACES_SAMPLER_ARG` | Sampling ratio when a ratio-based sampler is selected. |
| `RUST_LOG` | Span and event filter. Default is `info`. Does not enable disabled exporters. |

The SDK also supports signal-specific protocol, header, and timeout variables,
such as `OTEL_EXPORTER_OTLP_TRACES_HEADERS`. Those override their shared
equivalents. The app does not log export credentials or job argument bytes.

For a collector receiving OTLP over HTTP:

```powershell
$env:OTEL_EXPORTER_OTLP_ENDPOINT = 'http://localhost:4318'
$env:OTEL_SERVICE_NAME = 'dq'
cargo run --locked
```

For a collector receiving OTLP over gRPC:

```powershell
$env:OTEL_EXPORTER_OTLP_ENDPOINT = 'http://localhost:4317'
$env:OTEL_EXPORTER_OTLP_PROTOCOL = 'grpc'
cargo run --locked
```

To export only traces, leave the shared endpoint unset and configure:

```powershell
$env:OTEL_EXPORTER_OTLP_TRACES_ENDPOINT = 'http://localhost:4318/v1/traces'
cargo run --locked
```

The collector forwards telemetry to your chosen backend. DQ does not require a
Datadog-specific exporter. The collector itself does not provide trace storage or
a visualization UI.

## Recorded data

Traces cover both gRPC methods and dispatcher notification receipt. The gRPC
boundary extracts W3C `traceparent` and `tracestate` metadata. Dispatch messages
carry trace context so their spans share the originating request's trace.
Trace context is currently in memory and does not survive a process restart.

Logs include startup, job acceptance, dispatcher notification receipt, and RPC
failures. Logs emitted inside a recorded span include its trace and span IDs.
Only application events and spans with the `dq` target or its module targets are exported,
which prevents exporter diagnostics from recursively producing OTLP logs.
Telemetry does not automatically instrument every dependency or SQL statement.

| Metric | Type | Attributes |
| --- | --- | --- |
| `dq.rpc.requests` | Counter of completed requests | `rpc.method`, `rpc.grpc.status_code` |
| `dq.rpc.duration` | Histogram in seconds | `rpc.method`, `rpc.grpc.status_code` |
| `dq.dispatch.notifications` | Counter of received notifications | None |

Job IDs appear on relevant logs and dispatch spans, but never as metric
attributes. Request metrics include successful and failed requests.
The dispatcher currently receives notifications and leaves jobs pending.
Consumer delivery and execution are not implemented.

## Disabled behavior and shutdown

Without configured endpoints, DQ creates no telemetry providers, exporters,
recording subscriber, metric instruments, or export workers. A no-op subscriber
also disables tracing's legacy logging fallback. Instrumentation skips timing,
metric attributes, trace-context extraction, and trace-context capture.
Disabled tracing macros do not evaluate their fields or allocate recorded spans.
Small runtime checks remain; the telemetry dependencies are still compiled into
the executable.

When only logs are enabled, events are recorded but span recording stays
disabled. When only metrics are enabled, application spans and events stay disabled,
and there is no trace context collection. `RUST_LOG=trace` does not override these
signal gates. Whenever any exporter is enabled, SDK warnings and errors go to
stderr through an event-only diagnostic layer. They are never exported as OTLP logs.

Export uses background batching and periodic metric collection. Collector
availability does not determine whether a job is accepted. Export buffers are
bounded, so telemetry can be dropped during an extended outage.

Ctrl+C stops gRPC intake, drains dispatcher notifications, and shuts down the
providers while the Tokio runtime is still available. Shutdown attempts to flush
all three signals. A forced kill cannot flush pending telemetry. Shutdown export
failures are reported on standard error.

## Verification

Run the OTLP integration checks:

```powershell
cargo test --locked --test telemetry
```

The checks use a local HTTP receiver and decode actual OTLP Protobuf payloads.
They verify signal-specific exports, endpoint overrides, W3C context propagation
through the handler and Tokio channel, log correlation, metric values, and
shutdown flushing. Separate processes verify that disabled span and event fields
are not evaluated, including logs-only and metrics-only configurations. A rejected
log batch verifies that background export failures remain visible on stderr.
