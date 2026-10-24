use std::sync::Arc;

use tokio::sync::mpsc;

use crate::telemetry::{DispatchJob, Instrumentation};

pub async fn run(mut rx: mpsc::Receiver<DispatchJob>, instrumentation: Arc<Instrumentation>) {
    while let Some(job) = rx.recv().await {
        instrumentation.received(job).await;
    }
}
