use std::{process::ExitCode, sync::Arc};

use racer_backend::{Backend, LocalFleet, router, spawn_ticker};

#[tokio::main]
async fn main() -> ExitCode {
    // Host names are resolved, so `localhost:8080` works as well as `127.0.0.1:8080`.
    let addr = std::env::var("RACER_BACKEND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let fleet = match LocalFleet::from_env() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("racer-backend: {e}");
            return ExitCode::FAILURE;
        }
    };
    let state = std::env::var_os("RACER_BACKEND_STATE").map(Into::into);
    let backend = Backend::new(Arc::new(fleet), state);
    match serve(&addr, backend).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("racer-backend: {addr}: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve(addr: &str, backend: Backend) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("racer-backend listening on {}", listener.local_addr()?);
    spawn_ticker(backend.clone());
    axum::serve(listener, router(backend))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
}
