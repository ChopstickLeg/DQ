use std::env;
use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use opentelemetry::metrics::{Counter, Histogram, MeterProvider as _};
use opentelemetry::propagation::Extractor;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{Context, KeyValue};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tonic::metadata::{KeyRef, MetadataMap};
use tonic::{Code, Status};
use tracing::{Instrument, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::filter::{EnvFilter, filter_fn};
use tracing_subscriber::prelude::*;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// Owns exporters so they can be flushed before the Tokio runtime stops.
#[derive(Default)]
pub struct Telemetry {
    traces: Option<SdkTracerProvider>,
    logs: Option<SdkLoggerProvider>,
    metrics: Option<SdkMeterProvider>,
    instrumentation: Arc<Instrumentation>,
}

impl Telemetry {
    pub fn init() -> Result<Self, Error> {
        Self::from_config(Config::from_env()?)
    }

    fn from_config(config: Config) -> Result<Self, Error> {
        let mut telemetry = Self::default();
        if !config.traces && !config.logs && !config.metrics {
            // Mark tracing as configured even in the disabled case. Otherwise its
            // legacy `log` compatibility path can still evaluate span fields.
            tracing::subscriber::set_global_default(tracing::subscriber::NoSubscriber::default())?;
            return Ok(telemetry);
        }

        // The SDK's resource detector honors OTEL_SERVICE_NAME and OTEL_RESOURCE_ATTRIBUTES.
        let resource = Resource::builder()
            .with_service_name(env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "dq".into()))
            .with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")))
            .build();

        if config.traces {
            telemetry.traces = Some(
                SdkTracerProvider::builder()
                    .with_resource(resource.clone())
                    .with_batch_exporter(opentelemetry_otlp::SpanExporter::builder().build()?)
                    .build(),
            );
        }
        if config.logs {
            telemetry.logs = Some(
                SdkLoggerProvider::builder()
                    .with_resource(resource.clone())
                    .with_batch_exporter(opentelemetry_otlp::LogExporter::builder().build()?)
                    .build(),
            );
        }
        if config.metrics {
            telemetry.metrics = Some(
                SdkMeterProvider::builder()
                    .with_resource(resource)
                    .with_periodic_exporter(opentelemetry_otlp::MetricExporter::builder().build()?)
                    .build(),
            );
        }

        {
            let level = if config.traces || config.logs {
                EnvFilter::try_from_default_env().or_else(|error| {
                    if env::var_os("RUST_LOG").is_none() {
                        Ok(EnvFilter::new("info"))
                    } else {
                        Err(error)
                    }
                })?
            } else {
                EnvFilter::new("off")
            };
            let traces = telemetry.traces.as_ref().map(|provider| {
                tracing_opentelemetry::layer()
                    .with_tracer(provider.tracer("dq"))
                    .with_filter(filter_fn(is_application))
                    .with_filter(level.clone())
            });
            let logs = telemetry.logs.as_ref().map(|provider| {
                OpenTelemetryTracingBridge::new(provider)
                    .with_filter(filter_fn(is_application))
                    .with_filter(level)
            });
            // A log exporter must not enable span allocation when traces are disabled.
            // Exporter diagnostics go only to stderr, never back through OTLP.
            let enabled = filter_fn(move |metadata| {
                is_export_diagnostic(metadata)
                    || (is_application(metadata)
                        && (config.traces || (config.logs && metadata.is_event())))
            });
            let diagnostics = tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(false)
                .with_filter(filter_fn(is_export_diagnostic));
            tracing_subscriber::registry()
                .with(enabled)
                .with(traces)
                .with(logs)
                .with(diagnostics)
                .try_init()?;
        }

        telemetry.instrumentation = Arc::new(Instrumentation {
            traces: config.traces,
            metrics: telemetry.metrics.as_ref().map(QueueMetrics::new),
        });
        Ok(telemetry)
    }

    pub fn instrumentation(&self) -> Arc<Instrumentation> {
        Arc::clone(&self.instrumentation)
    }

    pub async fn shutdown(self) -> Result<(), tokio::task::JoinError> {
        if self.traces.is_none() && self.logs.is_none() && self.metrics.is_none() {
            return Ok(());
        }
        // Provider shutdown waits for worker threads. Keep the async runtime available
        // while gRPC exporters finish their pending requests.
        tokio::task::spawn_blocking(move || drop(self)).await
    }
}

fn is_application(metadata: &tracing::Metadata<'_>) -> bool {
    metadata.target() == "dq" || metadata.target().starts_with("dq::")
}

fn is_export_diagnostic(metadata: &tracing::Metadata<'_>) -> bool {
    metadata.is_event()
        && metadata.target().starts_with("opentelemetry")
        && metadata.level() <= &tracing::Level::WARN
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        if let Some(provider) = &self.traces
            && let Err(error) = provider.shutdown()
        {
            eprintln!("Failed to flush OTEL traces: {error}");
        }
        if let Some(provider) = &self.logs
            && let Err(error) = provider.shutdown()
        {
            eprintln!("Failed to flush OTEL logs: {error}");
        }
        if let Some(provider) = &self.metrics
            && let Err(error) = provider.shutdown()
        {
            eprintln!("Failed to flush OTEL metrics: {error}");
        }
    }
}

struct Config {
    traces: bool,
    logs: bool,
    metrics: bool,
}

impl Config {
    fn from_env() -> Result<Self, env::VarError> {
        let disabled = env::var("OTEL_SDK_DISABLED")
            .map(|value| value.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if disabled {
            return Ok(Self {
                traces: false,
                logs: false,
                metrics: false,
            });
        }
        let common = match env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
            Ok(value) => !value.trim().is_empty(),
            Err(env::VarError::NotPresent) => false,
            Err(error) => return Err(error),
        };
        let enabled = |signal| match env::var(format!("OTEL_EXPORTER_OTLP_{signal}_ENDPOINT")) {
            Ok(value) => Ok(!value.trim().is_empty()),
            Err(env::VarError::NotPresent) => Ok(common),
            Err(error) => Err(error),
        };
        Ok(Self {
            traces: enabled("TRACES")?,
            logs: enabled("LOGS")?,
            metrics: enabled("METRICS")?,
        })
    }
}

#[derive(Default)]
pub struct Instrumentation {
    traces: bool,
    metrics: Option<QueueMetrics>,
}

pub struct DispatchJob {
    pub job_id: String,
    trace_context: Option<Context>,
}

impl Instrumentation {
    /// Measures an RPC without timers, attribute allocation, or context extraction
    /// for signals that are disabled.
    pub async fn rpc<T>(
        &self,
        method: &'static str,
        metadata: &MetadataMap,
        operation: impl Future<Output = Result<T, Status>>,
    ) -> Result<T, Status> {
        let started = self.metrics.as_ref().map(|_| Instant::now());
        let span = if self.traces {
            let span = tracing::info_span!(
                target: "dq::rpc", "rpc", rpc.system = "grpc", rpc.service = "dq.v1.QueueService",
                rpc.method = method, otel.kind = "server",
                otel.status_code = tracing::field::Empty,
            );
            let parent = opentelemetry::propagation::TextMapPropagator::extract(
                &TraceContextPropagator::new(),
                &MetadataExtractor(metadata),
            );
            let _ = span.set_parent(parent);
            span
        } else {
            Span::none()
        };
        let observed = async {
            let result = operation.await;
            if let Err(error) = &result {
                if self.traces {
                    Span::current().record("otel.status_code", "ERROR");
                }
                tracing::warn!(target: "dq::rpc", rpc_method = method, grpc_code = ?error.code(), "RPC failed");
            }
            result
        };
        let result = if self.traces {
            observed.instrument(span).await
        } else {
            observed.await
        };
        if let Some(metrics) = &self.metrics {
            let code = result.as_ref().err().map_or(Code::Ok, Status::code);
            let attributes = [
                KeyValue::new("rpc.method", method),
                KeyValue::new("rpc.grpc.status_code", code as i64),
            ];
            metrics.requests.add(1, &attributes);
            if let Some(started) = started {
                metrics
                    .duration
                    .record(started.elapsed().as_secs_f64(), &attributes);
            }
        }
        result
    }

    pub fn dispatch_job(&self, job_id: String) -> DispatchJob {
        DispatchJob {
            job_id,
            trace_context: self.traces.then(|| Span::current().context()),
        }
    }

    pub async fn received(&self, job: DispatchJob) {
        let operation = async {
            tracing::info!(target: "dq::dispatch", job_id = %job.job_id, "Job notification received; consumer delivery is not implemented");
            if let Some(metrics) = &self.metrics {
                metrics.notifications.add(1, &[]);
            }
        };
        if let Some(context) = job.trace_context {
            let span = tracing::info_span!(target: "dq::dispatch", "dispatch.receive", job_id = %job.job_id);
            let _ = span.set_parent(context);
            operation.instrument(span).await;
        } else {
            operation.await;
        }
    }
}

struct QueueMetrics {
    requests: Counter<u64>,
    duration: Histogram<f64>,
    notifications: Counter<u64>,
}

impl QueueMetrics {
    fn new(provider: &SdkMeterProvider) -> Self {
        let meter = provider.meter("dq");
        Self {
            requests: meter
                .u64_counter("dq.rpc.requests")
                .with_description("Completed gRPC requests")
                .build(),
            duration: meter
                .f64_histogram("dq.rpc.duration")
                .with_unit("s")
                .with_description("gRPC request duration")
                .build(),
            notifications: meter
                .u64_counter("dq.dispatch.notifications")
                .with_description("Job notifications received by the dispatcher")
                .build(),
        }
    }
}

struct MetadataExtractor<'a>(&'a MetadataMap);

impl Extractor for MetadataExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key)?.to_str().ok()
    }

    fn keys(&self) -> Vec<&str> {
        self.0
            .keys()
            .filter_map(|key| match key {
                KeyRef::Ascii(key) => Some(key.as_str()),
                KeyRef::Binary(_) => None,
            })
            .collect()
    }
}
