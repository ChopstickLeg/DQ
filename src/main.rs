mod db;
mod dispatch;
mod grpc;
mod proto;

use grpc::QueueServiceHandler;
use proto::queue_service_server::QueueServiceServer;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    let (tx, rx) = mpsc::channel::<String>(100);
    let mut dispatcher = tokio::spawn(dispatch::run(rx));

    let listener = TcpListener::bind(address).await?;
    println!("Listening on {}", listener.local_addr()?);

    let server = Server::builder()
        .add_service(QueueServiceServer::new(QueueServiceHandler::new(pool, tx)))
        .serve_with_incoming(TcpListenerStream::new(listener));

    tokio::select! {
        result = &mut dispatcher => {
            eprintln!("Dispatcher task exited with error: {:?}", result);
        },
        result = server => {
            eprintln!("gRPC server exited with error: {:?}", result);
        },
    }

    Ok(())
}
