use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path as AxumPath, Query, State},
    http::{header, StatusCode},
    response::{sse::Event, IntoResponse, Response, Sse},
    routing::{get, post},
    Json, Router,
};
use dashmap::DashMap;
use futures::{stream::Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::BroadcastStream;
use tower_http::cors::CorsLayer;

use crate::cache::Cache;
use crate::job::{Job, JobStatus, ProgressEvent};
use crate::worker::run_pipeline;

#[derive(Clone)]
pub struct AppState {
    pub jobs: Arc<DashMap<uuid::Uuid, Arc<Job>>>,
    pub cache: Arc<Cache>,
}

impl AppState {
    pub fn new(cache_dir: PathBuf) -> anyhow::Result<Self> {
        Ok(Self {
            jobs: Arc::new(DashMap::new()),
            cache: Arc::new(Cache::new(cache_dir)?),
        })
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/analyze", post(analyze))
        .route("/analyze/:id", get(get_job))
        .route("/analyze/:id/stream", get(stream_job))
        .route("/analyze/:id/file", get(get_file))
        .route("/health", get(|| async { "ok" }))
        .with_state(state)
        .layer(CorsLayer::permissive())
}

#[derive(Debug, Deserialize)]
pub struct AnalyzeRequest {
    pub base_path: PathBuf,
    pub head_path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct AnalyzeResponse {
    pub job_id: uuid::Uuid,
}

async fn analyze(
    State(state): State<AppState>,
    Json(req): Json<AnalyzeRequest>,
) -> Result<Json<AnalyzeResponse>, (StatusCode, String)> {
    if !req.base_path.exists() {
        return Err((StatusCode::BAD_REQUEST, format!("base_path missing: {}", req.base_path.display())));
    }
    if !req.head_path.exists() {
        return Err((StatusCode::BAD_REQUEST, format!("head_path missing: {}", req.head_path.display())));
    }
    let base_root = req
        .base_path
        .canonicalize()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("canonicalize base: {e}")))?;
    let head_root = req
        .head_path
        .canonicalize()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("canonicalize head: {e}")))?;
    let job = Job::new(base_root.clone(), head_root.clone());
    let id = job.id;
    state.jobs.insert(id, job.clone());
    let cache = state.cache.clone();
    tokio::spawn(async move {
        run_pipeline(job, base_root, head_root, cache).await;
    });
    Ok(Json(AnalyzeResponse { job_id: id }))
}

#[derive(Debug, Serialize)]
pub struct JobView {
    #[serde(flatten)]
    pub status: JobStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact: Option<adr_core::Artifact>,
}

async fn get_job(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<uuid::Uuid>,
) -> Result<Json<JobView>, (StatusCode, String)> {
    let job = state
        .jobs
        .get(&id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "unknown job".into()))?
        .clone();
    let status = job.status.read().await.clone();
    let artifact = job.artifact.read().await.clone();
    Ok(Json(JobView { status, artifact }))
}

#[derive(Debug, serde::Deserialize)]
pub struct FileQuery {
    pub side: Side,
    pub path: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    Base,
    Head,
}

/// GET /analyze/:id/file?side=base|head&path=<relative>
///
/// Returns the file bytes as `text/plain; charset=utf-8`. Path is joined
/// against the job's canonicalized root and the result must stay inside it
/// (reject any `..` escape). Binary files return 415 — v0 only serves text.
async fn get_file(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<uuid::Uuid>,
    Query(q): Query<FileQuery>,
) -> Result<Response, (StatusCode, String)> {
    let job = state
        .jobs
        .get(&id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "unknown job".into()))?
        .clone();
    let root = match q.side {
        Side::Base => &job.base_root,
        Side::Head => &job.head_root,
    };
    let resolved = resolve_inside(root, &q.path)
        .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
    let bytes = tokio::fs::read(&resolved)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, format!("read {}: {e}", resolved.display())))?;
    let Ok(text) = String::from_utf8(bytes) else {
        return Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "binary file; only text served".into(),
        ));
    };
    Ok((
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        text,
    )
        .into_response())
}

/// Join `rel` onto `root` and refuse the result if it escapes `root`.
fn resolve_inside(root: &Path, rel: &str) -> Result<PathBuf, String> {
    // Reject absolute + drive-relative inputs up front — they bypass the join.
    let p = Path::new(rel);
    if p.is_absolute() || rel.starts_with('\\') || rel.contains(':') {
        return Err("absolute paths not allowed".into());
    }
    let joined = root.join(p);
    let canonical = joined
        .canonicalize()
        .map_err(|e| format!("canonicalize: {e}"))?;
    if !canonical.starts_with(root) {
        return Err("path escapes job root".into());
    }
    Ok(canonical)
}

async fn stream_job(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<uuid::Uuid>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)> {
    let job = state
        .jobs
        .get(&id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "unknown job".into()))?
        .clone();
    let rx = job.progress.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|r| async move {
        let ev: ProgressEvent = r.ok()?;
        let data = serde_json::to_string(&ev).ok()?;
        Some(Ok(Event::default().event(ev.stage.clone()).data(data)))
    });
    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new().interval(Duration::from_secs(15)),
    ))
}
