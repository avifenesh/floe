use std::path::PathBuf;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cache_dir = std::env::var("ADR_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".adr/cache"));
    let port: u16 = std::env::var("ADR_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8787);

    let state = adr_server::AppState::new(cache_dir.clone())?;
    let app = adr_server::build_router(state);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!(%addr, cache=%cache_dir.display(), "adr-server listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
