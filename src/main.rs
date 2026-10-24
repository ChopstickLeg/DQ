mod db;
mod dispatch;
mod grpc;
mod proto;
mod telemetry;

use grpc::QueueServiceHandler;
use proto::queue_service_server::QueueServiceServer;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let telemetry =
        telemetry::Telemetry::init().map_err(|error| -> Box<dyn std::error::Error> { error })?;
    let result = run(telemetry.instrumentation()).await;
    if let Err(error) = &result {
        tracing::error!(error = %error, "Service stopped");
    }
    telemetry.shutdown().await?;
    result
}

async fn run(
    instrumentation: Arc<telemetry::Instrumentation>,
) -> Result<(), Box<dyn std::error::Error>> {
    let address: std::net::SocketAddr = match std::env::var("LISTEN_ADDR") {
        Ok(address) => address,
        Err(std::env::VarError::NotPresent) => "127.0.0.1:50051".to_owned(),
        Err(error) => return Err(error.into()),
    }
    .parse()?;
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(std::env::VarError::NotPresent) => "sqlite://dq.db".to_owned(),
        Err(error) => return Err(error.into()),
    };
    let pool = db::connect(&database_url).await?;

    let listener = TcpListener::bind(address).await?;
    println!("Listening on {}", listener.local_addr()?);
    tracing::info!(address = %listener.local_addr()?, "gRPC server listening");

    let (tx, rx) = mpsc::channel(100);
    let mut dispatcher = tokio::spawn(dispatch::run(rx, Arc::clone(&instrumentation)));

    let server = Server::builder()
        .add_service(QueueServiceServer::new(QueueServiceHandler::new(
            pool,
            tx,
            instrumentation,
        )))
        .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::error!(error = %error, "Failed to listen for Ctrl+C");
            }
        });

    tokio::select! {
        result = &mut dispatcher => {
            result?;
            return Err(std::io::Error::other("Dispatcher stopped unexpectedly").into());
        },
        result = server => {
            // Dropping the completed server closes its sender. Drain notifications
            // before flushing telemetry so their spans and metrics are included.
            dispatcher.await?;
            result?;
        },
    }

    Ok(())
}
