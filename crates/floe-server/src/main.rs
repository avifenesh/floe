use std::path::PathBuf;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Load `.env` from the workspace root so secrets like
    // `FLOE_GLM_API_KEY` don't have to be exported in every shell that
    // runs `cargo run -p floe-server`. Silent no-op when the file is
    // missing — production env is injected differently.
    let _ = dotenvy::from_path(".env");
    let _ = dotenvy::dotenv(); // fallback: default search upward

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cache_dir = std::env::var("FLOE_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".floe/cache"));
    let port: u16 = std::env::var("FLOE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8787);

    // DB selection — `FLOE_DB=:memory:` (or CLI `--in-memory` via the
    // same env) uses a throwaway store; anything else is a file path
    // (default `.floe/adr.db`). Persistent index survives restart so
    // the landing page can list prior analyses.
    // Postgres via `FLOE_DATABASE_URL` / `DATABASE_URL` takes precedence
    // over the local libsql file (see `project_postgres_migration`).
    // Falls through to SQLite when neither URL is set.
    let pg_url = std::env::var("FLOE_DATABASE_URL")
        .ok()
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .filter(|s| s.starts_with("postgres://") || s.starts_with("postgresql://"));

    let db = if let Some(url) = pg_url {
        tracing::info!(
            url = %redact_pg_url(&url),
            "opening postgres db"
        );
        floe_server::DbStore::open_postgres(&url).await?
    } else {
        match std::env::var("FLOE_DB").ok().as_deref() {
            Some(":memory:") => {
                tracing::info!("opening in-memory db (FLOE_DB=:memory:)");
                floe_server::DbStore::open_in_memory().await?
            }
            Some(path) => {
                let p = PathBuf::from(path);
                tracing::info!(db = %p.display(), "opening db");
                floe_server::DbStore::open_file(&p).await?
            }
            None => {
                let default = cache_dir
                    .parent()
                    .map(|p| p.join("floe.db"))
                    .unwrap_or_else(|| PathBuf::from(".floe/adr.db"));
                tracing::info!(db = %default.display(), "opening db (default path)");
                floe_server::DbStore::open_file(&default).await?
            }
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

    let auth = floe_server::AuthConfig::from_env()?;
    if let Some(cfg) = auth.as_ref() {
        tracing::info!(
            github = cfg.github.is_some(),
            frontend = %cfg.frontend_url,
            "auth config loaded"
        );
    }
    // Fixtures root: `FLOE_SAMPLES_ROOT` overrides. Default is
    // `<workspace>/fixtures` resolved relative to the current dir
    // (which is how `cargo run` launches us, so a dev checkout
    // "just works"). Self-hosters without a fixtures dir get an
    // empty gallery — the landing hides itself gracefully.
    let samples_root = std::env::var("FLOE_SAMPLES_ROOT")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            let default = std::path::PathBuf::from("fixtures");
            default.exists().then_some(default)
        });
    let state = floe_server::AppState::new(
        cache_dir.clone(),
        db,
        auth,
        samples_root.as_deref(),
    )?;
    let app = floe_server::build_router(state);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!(%addr, cache=%cache_dir.display(), "floe-server listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Redact password from a postgres URL for logs. Returns the original
/// string when it doesn't parse as a URL.
fn redact_pg_url(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(mut u) => {
            if u.password().is_some() {
                let _ = u.set_password(Some("***"));
            }
            u.to_string()
        }
        Err(_) => "<unparseable>".to_string(),
    }
}
