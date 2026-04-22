use std::path::PathBuf;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Load `.env` from the workspace root so secrets like
    // `ADR_GLM_API_KEY` don't have to be exported in every shell that
    // runs `cargo run -p adr-server`. Silent no-op when the file is
    // missing — production env is injected differently.
    let _ = dotenvy::from_path(".env");
    let _ = dotenvy::dotenv(); // fallback: default search upward

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

    // DB selection — `ADR_DB=:memory:` (or CLI `--in-memory` via the
    // same env) uses a throwaway store; anything else is a file path
    // (default `.adr/adr.db`). Persistent index survives restart so
    // the landing page can list prior analyses.
    let db = match std::env::var("ADR_DB").ok().as_deref() {
        Some(":memory:") => {
            tracing::info!("opening in-memory db (ADR_DB=:memory:)");
            adr_server::DbStore::open_in_memory().await?
        }
        Some(path) => {
            let p = PathBuf::from(path);
            tracing::info!(db = %p.display(), "opening db");
            adr_server::DbStore::open_file(&p).await?
        }
        None => {
            let default = cache_dir
                .parent()
                .map(|p| p.join("adr.db"))
                .unwrap_or_else(|| PathBuf::from(".adr/adr.db"));
            tracing::info!(db = %default.display(), "opening db (default path)");
            adr_server::DbStore::open_file(&default).await?
        }
    };

    // Stale-pending sweep — any `pending` row older than 5 minutes
    // is from a dead worker (server restart/crash); flip to errored
    // so the sidebar stops misleading the reviewer.
    match db.sweep_stale_pending(5).await {
        Ok(0) => {}
        Ok(n) => tracing::info!(n, "swept stale pending analyses on boot"),
        Err(e) => tracing::warn!(error = %e, "sweep_stale_pending failed"),
    }

    let auth = adr_server::AuthConfig::from_env()?;
    if let Some(cfg) = auth.as_ref() {
        tracing::info!(
            github = cfg.github.is_some(),
            frontend = %cfg.frontend_url,
            "auth config loaded"
        );
    }
    let state = adr_server::AppState::new(cache_dir.clone(), db, auth)?;
    let app = adr_server::build_router(state);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!(%addr, cache=%cache_dir.display(), "adr-server listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
