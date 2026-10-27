use std::net::SocketAddr;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr: SocketAddr = std::env::var("RACER_BACKEND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8080".into())
        .parse()
        .expect("RACER_BACKEND_ADDR must be a socket address");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("racer-backend listening on {}", listener.local_addr()?);
    axum::serve(listener, racer_backend::router())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
}
