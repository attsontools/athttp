use athttp_service::{parse_service_args, server::ServiceServer, ServiceError};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{}: {}", error.code, error.message);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), ServiceError> {
    let options = parse_service_args(std::env::args_os().skip(1))?;
    let server = ServiceServer::bind(
        options.endpoint_path,
        options.data_directory.to_string_lossy(),
        env!("CARGO_PKG_VERSION"),
    )
    .await?;
    let stop = server.stop_handle();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            stop.notify_one();
        }
    });
    server.run().await
}
