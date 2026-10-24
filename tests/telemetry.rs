use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{OriginalUri, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value};
use opentelemetry_proto::tonic::metrics::v1::{metric, number_data_point};
use prost::Message;
use tokio::net::TcpListener;
use tonic::Request;

#[path = "../src/db.rs"]
mod db;
#[path = "../src/dispatch.rs"]
mod dispatch;
#[path = "../src/grpc.rs"]
mod grpc;
#[path = "../src/proto.rs"]
mod proto;
#[path = "../src/telemetry.rs"]
mod telemetry;

use proto::queue_service_server::QueueService;

struct Export {
    path: String,
    bytes: Vec<u8>,
    has_header: bool,
}

type Captured = Arc<Mutex<Vec<Export>>>;

async fn collect(
    State(captured): State<Captured>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, [(&'static str, &'static str); 1], Vec<u8>) {
    let status = if uri.path() == "/reject-logs" {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    captured.lock().unwrap().push(Export {
        path: uri.path().to_owned(),
        bytes: body.to_vec(),
        has_header: headers
            .get("x-dq-test")
            .is_some_and(|value| value == "present"),
    });
    // An empty protobuf message is a valid successful OTLP export response.
    (
        status,
        [("content-type", "application/x-protobuf")],
        Vec::new(),
    )
}

// Runs in a fresh subprocess because tracing's subscriber is process-global and
// changing environment variables in a multithreaded Rust process is unsafe.
#[test]
fn export_probe() {
    let Ok(expected) = std::env::var("DQ_TEST_SIGNALS") else {
        return;
    };
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let telemetry = telemetry::Telemetry::init().expect("initialize telemetry");
        let instrumentation = telemetry.instrumentation();

        let span_fields = AtomicUsize::new(0);
        let span = tracing::info_span!(
            target: "dq::probe", "probe",
            computed = span_fields.fetch_add(1, Ordering::Relaxed),
        );
        drop(span);
        assert_eq!(span_fields.load(Ordering::Relaxed), usize::from(expected.contains('t')));
        let event_fields = AtomicUsize::new(0);
        tracing::info!(target: "dq::probe", computed = event_fields.fetch_add(1, Ordering::Relaxed), "probe log");
        assert_eq!(event_fields.load(Ordering::Relaxed), usize::from(expected.contains('t') || expected.contains('l')));

        let pool = db::connect("sqlite::memory:").await.unwrap();
        let (tx, rx) = tokio::sync::mpsc::channel(10);
        let dispatcher = tokio::spawn(dispatch::run(rx, Arc::clone(&instrumentation)));
        let handler = grpc::QueueServiceHandler::new(pool, tx, instrumentation);
        let mut request = Request::new(proto::EnqueueRequest {
            function_name: "telemetry.probe".into(),
            submission_key: "probe".into(),
            arguments: vec![1, 2, 3],
            caller: "telemetry-test".into(),
        });
        request.metadata_mut().insert("traceparent", "00-11111111111111111111111111111111-2222222222222222-01".parse().unwrap());
        handler.enqueue(request).await.unwrap();
        assert!(handler.get_job_status(Request::new(proto::GetJobStatusRequest { job_id: "missing".into() })).await.is_err());
        drop(handler);
        dispatcher.await.unwrap();
        if std::env::var_os("DQ_TEST_REJECTED").is_some() {
            // Exercise a background batch failure before shutdown flushes anything.
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
        telemetry.shutdown().await.unwrap();
    });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exports_configured_signals_and_skips_disabled_instrumentation() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let captured = Captured::default();
    let app = axum::Router::new()
        .route("/{*path}", post(collect))
        .with_state(Arc::clone(&captured));
    let collector = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    for (case, expected) in [
        ("all", "tlm"),
        ("traces", "t"),
        ("logs", "l"),
        ("metrics", "m"),
        ("rejected-logs", "l"),
        ("absent", ""),
        ("blank", ""),
        ("sdk-disabled", ""),
    ] {
        captured.lock().unwrap().clear();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "export_probe", "--nocapture"]);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("OTEL_") || key == "RUST_LOG" {
                command.env_remove(key);
            }
        }
        command
            .env("DQ_TEST_SIGNALS", expected)
            .env("RUST_LOG", "trace")
            .env("OTEL_SERVICE_NAME", "dq-test")
            .env("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf")
            .env("OTEL_EXPORTER_OTLP_HEADERS", "x-dq-test=present")
            .env("OTEL_EXPORTER_OTLP_TIMEOUT", "1000")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match case {
            "all" => {
                command.env("OTEL_EXPORTER_OTLP_ENDPOINT", &endpoint);
                command.env(
                    "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
                    format!("{endpoint}/override-traces"),
                );
            }
            "traces" | "logs" | "metrics" => {
                command.env(
                    format!("OTEL_EXPORTER_OTLP_{}_ENDPOINT", case.to_uppercase()),
                    format!("{endpoint}/v1/{case}"),
                );
            }
            "rejected-logs" => {
                command
                    .env(
                        "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
                        format!("{endpoint}/reject-logs"),
                    )
                    .env("OTEL_BLRP_SCHEDULE_DELAY", "10")
                    .env("DQ_TEST_REJECTED", "true");
            }
            "blank" => {
                command.env("OTEL_EXPORTER_OTLP_ENDPOINT", &endpoint);
                for signal in ["TRACES", "LOGS", "METRICS"] {
                    command.env(format!("OTEL_EXPORTER_OTLP_{signal}_ENDPOINT"), "   ");
                }
            }
            "sdk-disabled" => {
                command
                    .env("OTEL_EXPORTER_OTLP_ENDPOINT", &endpoint)
                    .env("OTEL_SDK_DISABLED", "true");
            }
            _ => {}
        }
        let mut child = command.spawn().unwrap();
        let status = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        if status.is_err() {
            child.kill().unwrap();
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            status.is_ok() && output.status.success(),
            "{case}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if case == "rejected-logs" {
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("BatchLogProcessor.ExportError"),
                "report background export failures on stderr"
            );
        }

        let exports = captured.lock().unwrap();
        assert_eq!(
            exports.iter().any(|export| export.path.contains("traces")),
            expected.contains('t'),
            "{case}"
        );
        assert_eq!(
            exports.iter().any(|export| export.path.contains("logs")),
            expected.contains('l'),
            "{case}"
        );
        assert_eq!(
            exports.iter().any(|export| export.path.contains("metrics")),
            expected.contains('m'),
            "{case}"
        );
        assert!(
            exports.iter().all(|export| export.has_header),
            "export configured auth headers"
        );
        if case == "all" {
            verify_payloads(&exports);
        }
    }
    collector.abort();
}

fn string_value(value: &str) -> Option<AnyValue> {
    Some(AnyValue {
        value: Some(any_value::Value::StringValue(value.into())),
    })
}

fn verify_payloads(exports: &[Export]) {
    let mut spans = Vec::new();
    let mut logs = Vec::new();
    let mut metrics = Vec::new();
    for export in exports {
        let resource = if export.path == "/override-traces" {
            let request = ExportTraceServiceRequest::decode(export.bytes.as_slice()).unwrap();
            let resource = request.resource_spans[0].resource.clone();
            spans.extend(
                request
                    .resource_spans
                    .into_iter()
                    .flat_map(|r| r.scope_spans)
                    .flat_map(|s| s.spans),
            );
            resource
        } else if export.path == "/v1/logs" {
            let request = ExportLogsServiceRequest::decode(export.bytes.as_slice()).unwrap();
            let resource = request.resource_logs[0].resource.clone();
            logs.extend(
                request
                    .resource_logs
                    .into_iter()
                    .flat_map(|r| r.scope_logs)
                    .flat_map(|s| s.log_records),
            );
            resource
        } else {
            assert_eq!(export.path, "/v1/metrics");
            let request = ExportMetricsServiceRequest::decode(export.bytes.as_slice()).unwrap();
            let resource = request.resource_metrics[0].resource.clone();
            metrics.extend(
                request
                    .resource_metrics
                    .into_iter()
                    .flat_map(|r| r.scope_metrics)
                    .flat_map(|s| s.metrics),
            );
            resource
        };
        assert!(resource.unwrap().attributes.contains(&KeyValue {
            key: "service.name".into(),
            value: string_value("dq-test"),
            ..Default::default()
        }));
    }
    let enqueue = spans
        .iter()
        .find(|span| {
            span.attributes.contains(&KeyValue {
                key: "rpc.method".into(),
                value: string_value("Enqueue"),
                ..Default::default()
            })
        })
        .unwrap();
    assert_eq!(
        enqueue.trace_id,
        vec![0x11; 16],
        "extract incoming W3C trace context"
    );
    assert_eq!(enqueue.parent_span_id, vec![0x22; 8]);
    let dispatch = spans
        .iter()
        .find(|span| span.name == "dispatch.receive")
        .unwrap();
    assert_eq!(
        dispatch.trace_id, enqueue.trace_id,
        "carry context across the Tokio channel"
    );
    assert_eq!(dispatch.parent_span_id, enqueue.span_id);
    let accepted = logs
        .iter()
        .find(|log| log.body == string_value("Job accepted"))
        .unwrap();
    assert_eq!(
        accepted.trace_id, enqueue.trace_id,
        "correlate exported logs with traces"
    );
    let requests = metrics
        .iter()
        .find(|metric| metric.name == "dq.rpc.requests")
        .unwrap();
    let Some(metric::Data::Sum(sum)) = &requests.data else {
        panic!("request counter must be an OTLP sum")
    };
    let count: i64 = sum
        .data_points
        .iter()
        .map(|point| match point.value {
            Some(number_data_point::Value::AsInt(value)) => value,
            _ => 0,
        })
        .sum();
    assert_eq!(count, 2, "count successful and failed requests");
    assert!(
        metrics
            .iter()
            .any(|metric| metric.name == "dq.rpc.duration")
    );
    assert!(
        metrics
            .iter()
            .any(|metric| metric.name == "dq.dispatch.notifications")
    );
}
