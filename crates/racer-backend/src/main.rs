use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    // Host names are resolved, so `localhost:8080` works as well as `127.0.0.1:8080`.
    let addr = std::env::var("RACER_BACKEND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    match serve(&addr).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("racer-backend: {addr}: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve(addr: &str) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("racer-backend listening on {}", listener.local_addr()?);
    axum::serve(listener, racer_backend::router())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
}
