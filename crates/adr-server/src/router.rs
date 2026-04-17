use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{sse::Event, Sse},
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
    let job = Job::new();
    let id = job.id;
    state.jobs.insert(id, job.clone());
    let cache = state.cache.clone();
    let base = req.base_path;
    let head = req.head_path;
    tokio::spawn(async move {
        run_pipeline(job, base, head, cache).await;
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
