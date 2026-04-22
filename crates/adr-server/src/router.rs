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
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::auth::{self, AuthConfig, Session};
use crate::cache::Cache;
use crate::db::DbStore;
use crate::git_sync;
use crate::job::{Job, JobStatus, ProgressEvent};
use crate::llm::LlmConfig;
use crate::samples::{SampleView, Samples};
use crate::worker::{run_pipeline, PipelineRequest, PrContext};
use axum_extra::extract::cookie::SignedCookieJar;

#[derive(Clone)]
pub struct AppState {
    pub jobs: Arc<DashMap<uuid::Uuid, Arc<Job>>>,
    pub cache: Arc<Cache>,
    /// Persistent analysis index — survives restarts. In-memory DB
    /// when launched with `--in-memory`. See [`crate::db`].
    pub db: DbStore,
    /// In-flight job dedupe: cache-key-ish → job_id. If a second
    /// identical request arrives while the first is still running,
    /// we return the existing `job_id` instead of spawning a new
    /// pipeline (probe alone burns 20 min on a big repo — we don't
    /// want to run it twice on a double-click).
    pub inflight: Arc<DashMap<String, uuid::Uuid>>,
    /// OAuth + signed-cookie config. `None` when `ADR_SESSION_SECRET`
    /// is unset — auth routes 404 and the FE never sees Sign-in.
    pub auth: Option<Arc<AuthConfig>>,
    /// Demo PRs the landing page offers. Populated once at startup
    /// from the fixtures root; empty when that root is absent.
    pub samples: Arc<Samples>,
}

impl AppState {
    pub fn new(
        cache_dir: PathBuf,
        db: DbStore,
        auth: Option<AuthConfig>,
        samples_root: Option<&Path>,
    ) -> anyhow::Result<Self> {
        let samples = samples_root
            .map(Samples::load)
            .unwrap_or_default();
        Ok(Self {
            jobs: Arc::new(DashMap::new()),
            cache: Arc::new(Cache::new(cache_dir)?),
            db,
            inflight: Arc::new(DashMap::new()),
            auth: auth.map(Arc::new),
            samples: Arc::new(samples),
        })
    }
}

// Substate extractors so auth handlers can pull only what they need
// without taking the whole AppState. The `FromRef` impls let
// `State<Arc<AuthConfig>>` and `State<DbStore>` resolve against
// `AppState`.

impl axum::extract::FromRef<AppState> for DbStore {
    fn from_ref(s: &AppState) -> Self {
        s.db.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<AuthConfig> {
    fn from_ref(s: &AppState) -> Self {
        s.auth
            .clone()
            .expect("auth routes must not be mounted without AuthConfig")
    }
}

impl axum::extract::FromRef<AppState> for axum_extra::extract::cookie::Key {
    fn from_ref(s: &AppState) -> Self {
        s.auth
            .as_ref()
            .expect("auth routes require AuthConfig")
            .session_key
            .clone()
    }
}

pub fn build_router(state: AppState) -> Router {
    let mut r = Router::new()
        .route("/analyze", post(analyze))
        .route("/analyze/url", post(analyze_url))
        .route("/analyze/:id", get(get_job))
        .route("/analyze/:id/stream", get(stream_job))
        .route("/analyze/:id/file", get(get_file))
        .route("/analyses", get(list_pr_analyses))
        .route("/analyses/:id", axum::routing::delete(delete_analysis))
        .route("/llm-config", get(get_llm_config))
        .route("/samples", get(list_samples))
        .route("/analyze/sample/:id", post(analyze_sample))
        .route("/health", get(|| async { "ok" }));

    if state.auth.is_some() {
        r = r
            .route("/auth/github", get(auth::start_github))
            .route("/auth/github/callback", get(auth::github_callback))
            .route("/me", get(auth::me))
            .route("/auth/logout", post(auth::logout));
        if std::env::var("ADR_ALLOW_DEV_LOGIN").ok().as_deref() == Some("1") {
            tracing::warn!("ADR_ALLOW_DEV_LOGIN=1 — mounting POST /auth/dev/login (DO NOT USE IN PROD)");
            r = r.route("/auth/dev/login", post(auth::dev_login));
        }
    }

    r.with_state(state).layer(cors_layer())
}

/// CORS for dev. The FE runs on :5173 (Vite) and the backend on
/// :8787 — a different origin in browser eyes, so credentialed
/// requests (`credentials: "include"`) need an explicit allowlist
/// AND `Access-Control-Allow-Credentials: true`. `permissive()`'s
/// wildcard `*` violates the spec when paired with credentials, and
/// browsers silently drop the cookie on those responses — which is
/// exactly why `/me` returned 401 after OAuth.
///
/// We mirror the request origin instead of hard-coding :5173, so a
/// prod build served from the same origin as the backend keeps
/// working without config.
fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::mirror_request())
        .allow_credentials(true)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            axum::http::header::ACCEPT,
        ])
}

/// GET /pr_analyses — landing-page history list. Newest first, capped
/// at `limit` (default 50). Scoped to `user_id` when auth lands
/// (slice 2); today returns the global list.
async fn list_pr_analyses(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<crate::db::AnalysisRow>>, (StatusCode, String)> {
    let limit = q.limit.unwrap_or(50).min(200);
    let rows = state
        .db
        .list_recent(q.user_id.as_deref(), limit)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("list: {e:#}")))?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// DELETE /analyses/:id — drop a row from the sidebar. Idempotent
/// (200 with `removed=0` when the id wasn't there). Also evicts the
/// in-memory job + inflight-dedupe entry so a re-analyze of the same
/// inputs gets a fresh run.
async fn delete_analysis(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let removed = state
        .db
        .delete_analysis(&id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("delete: {e:#}")))?;
    // Also evict in-memory if present.
    if let Ok(uuid) = id.parse::<uuid::Uuid>() {
        state.jobs.remove(&uuid);
    }
    Ok(Json(serde_json::json!({ "removed": removed })))
}

#[derive(Debug, Deserialize)]
pub struct AnalyzeRequest {
    pub base_path: PathBuf,
    pub head_path: PathBuf,
    /// Optional PR intent — structured `Intent` or raw PR-description
    /// text. Consumed by the intent-fit + proof-verification LLM passes.
    /// `None` means those passes emit a "no-intent" claim and skip the
    /// proof axis.
    #[serde(default)]
    pub intent: Option<adr_core::IntentInput>,
    /// Optional reviewer-supplied side-channel notes — pasted benchmark
    /// output, staging logs, corroborating observations. Read by the
    /// proof-verification pass alongside the code.
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AnalyzeResponse {
    pub job_id: uuid::Uuid,
}

async fn analyze(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Json(req): Json<AnalyzeRequest>,
) -> Result<Json<AnalyzeResponse>, (StatusCode, String)> {
    let user_id = Session::from_jar(&jar).map(|s| s.user_id);
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
    let intent = req.intent;
    let notes = req.notes.unwrap_or_default();

    // Dedupe: if an identical job is already running, return its id.
    // Key on (base, head, intent, notes) — same inputs, same work.
    let dedupe_key = request_dedupe_key(&base_root, &head_root, intent.as_ref(), &notes);
    if let Some(entry) = state.inflight.get(&dedupe_key) {
        let existing = *entry.value();
        // Only return the cached id if the Job is still pending; a
        // completed job should fall through to a fresh spawn (the
        // cache layer will serve it in ~0ms).
        if let Some(job_ref) = state.jobs.get(&existing) {
            let status = job_ref.status.read().await.clone();
            if matches!(status, crate::job::JobStatus::Pending) {
                tracing::info!(job_id = %existing, "dedupe: returning in-flight job");
                return Ok(Json(AnalyzeResponse { job_id: existing }));
            }
        }
    }

    let job = Job::new(base_root.clone(), head_root.clone());
    let id = job.id;
    state.jobs.insert(id, job.clone());
    state.inflight.insert(dedupe_key.clone(), id);
    let cache = state.cache.clone();
    let inflight = state.inflight.clone();
    let db = state.db.clone();
    let pr_ctx = PrContext { repo: None, pr_number: None, user_id };
    tokio::spawn(async move {
        run_pipeline(PipelineRequest {
            job,
            base: base_root,
            head: head_root,
            cache,
            db,
            intent,
            notes,
            pr_ctx,
        })
        .await;
        // Clear the inflight entry so a subsequent identical request
        // can re-trigger (cache will serve it instantly if the
        // artifact landed). Only remove when the id still matches —
        // a later re-trigger with a different id shouldn't be clobbered.
        inflight.remove_if(&dedupe_key, |_, v| *v == id);
    });
    Ok(Json(AnalyzeResponse { job_id: id }))
}

#[derive(Debug, Deserialize)]
pub struct AnalyzeUrlRequest {
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct AnalyzeUrlResponse {
    pub job_id: uuid::Uuid,
    pub repo: String,
    pub pr_number: u64,
    pub base_sha: String,
    pub head_sha: String,
}

/// POST /analyze/url — analyse a GitHub PR by URL. Auth-required: we
/// need the user's stored access token to hit the GitHub API + clone
/// private repos. Clones base + head at the PR's resolved SHAs into
/// `<cache>/repos/<owner>-<repo>/<sha>`, then kicks off the same
/// pipeline `POST /analyze` runs. PR body becomes the raw-text intent.
async fn analyze_url(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Json(req): Json<AnalyzeUrlRequest>,
) -> Result<Json<AnalyzeUrlResponse>, (StatusCode, String)> {
    // Must be signed in — both for the GitHub API token and because
    // PR-URL flow is a "signed-in user" feature (anonymous users use
    // local paths on the landing page's Try button).
    let session = Session::from_jar(&jar)
        .ok_or((StatusCode::UNAUTHORIZED, "sign in to analyse by URL".into()))?;
    let token = state
        .db
        .find_access_token(&session.user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("find_access_token: {e:#}")))?
        .ok_or((StatusCode::FORBIDDEN, "no GitHub access token on file — sign in again".into()))?;

    let (owner, repo, number) = auth::parse_github_pr_url(&req.url)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("{e:#}")))?;
    let pr = auth::fetch_github_pr(&token, &owner, &repo, number)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("github PR fetch: {e:#}")))?;

    // Materialise base + head checkouts in parallel. Both go under
    // the cache dir so they survive restart.
    let cache_dir = state.cache.dir().to_path_buf();
    let base_path = git_sync::checkout_path(&cache_dir, &owner, &repo, &pr.base.sha);
    let head_path = git_sync::checkout_path(&cache_dir, &owner, &repo, &pr.head.sha);
    let (br, hr) = tokio::join!(
        git_sync::clone_sha(&owner, &repo, &pr.base.sha, Some(&token), &base_path),
        git_sync::clone_sha(&owner, &repo, &pr.head.sha, Some(&token), &head_path),
    );
    br.map_err(|e| (StatusCode::BAD_GATEWAY, format!("git clone base: {e:#}")))?;
    hr.map_err(|e| (StatusCode::BAD_GATEWAY, format!("git clone head: {e:#}")))?;

    let base_root = base_path
        .canonicalize()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("canonicalize base: {e}")))?;
    let head_root = head_path
        .canonicalize()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("canonicalize head: {e}")))?;

    let intent = pr
        .body
        .as_ref()
        .filter(|b| !b.trim().is_empty())
        .map(|b| adr_core::IntentInput::RawText(b.clone()));
    let notes = String::new();

    // Same dedupe + spawn path as `analyze()` so a double-click on
    // "Analyse" doesn't run the pipeline twice.
    let dedupe_key = request_dedupe_key(&base_root, &head_root, intent.as_ref(), &notes);
    if let Some(entry) = state.inflight.get(&dedupe_key) {
        let existing = *entry.value();
        if let Some(job_ref) = state.jobs.get(&existing) {
            let status = job_ref.status.read().await.clone();
            if matches!(status, crate::job::JobStatus::Pending) {
                tracing::info!(job_id = %existing, "dedupe: returning in-flight url job");
                return Ok(Json(AnalyzeUrlResponse {
                    job_id: existing,
                    repo: format!("{owner}/{repo}"),
                    pr_number: number,
                    base_sha: pr.base.sha,
                    head_sha: pr.head.sha,
                }));
            }
        }
    }

    let job = Job::new(base_root.clone(), head_root.clone());
    let id = job.id;
    state.jobs.insert(id, job.clone());
    state.inflight.insert(dedupe_key.clone(), id);
    let cache = state.cache.clone();
    let inflight = state.inflight.clone();
    let db = state.db.clone();
    let pr_ctx = PrContext {
        repo: Some(format!("{owner}/{repo} #{number}")),
        pr_number: Some(number as i64),
        user_id: Some(session.user_id.clone()),
    };
    tokio::spawn(async move {
        run_pipeline(PipelineRequest {
            job,
            base: base_root,
            head: head_root,
            cache,
            db,
            intent,
            notes,
            pr_ctx,
        })
        .await;
        inflight.remove_if(&dedupe_key, |_, v| *v == id);
    });
    Ok(Json(AnalyzeUrlResponse {
        job_id: id,
        repo: format!("{owner}/{repo}"),
        pr_number: number,
        base_sha: pr.base.sha,
        head_sha: pr.head.sha,
    }))
}

/// Key used to dedupe concurrent analyze requests. Matches what the
/// cache layer mixes in for its own key; exact collision isn't
/// required — we only need "same inputs → same inflight job".
fn request_dedupe_key(
    base: &Path,
    head: &Path,
    intent: Option<&adr_core::IntentInput>,
    notes: &str,
) -> String {
    let mut h = blake3::Hasher::new();
    h.update(base.to_string_lossy().as_bytes());
    h.update(b"|");
    h.update(head.to_string_lossy().as_bytes());
    h.update(b"|");
    if let Some(i) = intent {
        let bytes = serde_json::to_vec(i).unwrap_or_default();
        h.update(&bytes);
    }
    h.update(b"|");
    h.update(notes.as_bytes());
    h.finalize().to_hex().to_string()
}

/// Read the cached artifact for a job id by joining the DB row's
/// `artifact_key` to the cache. Returns `None` when no row exists or
/// the row's status isn't ready / lacks a cache key.
async fn load_cached_artifact(
    state: &AppState,
    id: &uuid::Uuid,
) -> Result<Option<adr_core::Artifact>, anyhow::Error> {
    let rows = state.db.list_recent(None, 200).await?;
    let row = match rows.into_iter().find(|r| r.id == id.to_string()) {
        Some(r) => r,
        None => return Ok(None),
    };
    let key = match row.artifact_key {
        Some(k) => k,
        None => return Ok(None),
    };
    state.cache.get(&key)
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
    // In-memory `Job` map is cleared on server restart. The DB row +
    // cached artifact survive though, so when we miss in-memory we
    // rehydrate a minimal Job with base/head roots so subsequent
    // `/file` requests can still serve source bytes. Without this,
    // the Source view + NodeDetailPanel 404 on every post-restart
    // visit.
    if !state.jobs.contains_key(&id) {
        if let Ok(Some(artifact)) = load_cached_artifact(&state, &id).await {
            if let Some((base_root, head_root)) =
                resolve_job_roots(&state, &id, &artifact).await
            {
                let job = Job::rehydrated(id, base_root, head_root, artifact.clone());
                state.jobs.insert(id, job);
            }
            return Ok(Json(JobView {
                status: JobStatus::Ready,
                artifact: Some(artifact),
            }));
        }
        return Err((StatusCode::NOT_FOUND, "unknown job".into()));
    }
    let job = state
        .jobs
        .get(&id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "unknown job".into()))?
        .clone();
    let status = job.status.read().await.clone();
    let artifact = job.artifact.read().await.clone();
    Ok(Json(JobView { status, artifact }))
}

/// Look up where this job's base/head snapshots live on disk,
/// given a cache-hydrated artifact. Three sources:
///
/// 1. **Sample-run** (`pr.repo == "sample/<id>"`) — resolve via
///    the in-memory samples table.
/// 2. **GitHub URL-run** — reconstruct the git_sync checkout path
///    from `(owner, name, sha)` in the artifact's pr_ref.
/// 3. **Path-driven** — no persisted paths; returns `None` and the
///    file endpoint will 404 cleanly.
async fn resolve_job_roots(
    state: &AppState,
    _id: &uuid::Uuid,
    artifact: &adr_core::Artifact,
) -> Option<(PathBuf, PathBuf)> {
    // Sample-run path.
    if let Some(sample_id) = artifact.pr.repo.strip_prefix("sample/") {
        if let Some(sample) = state.samples.get(sample_id) {
            let base = sample.base.canonicalize().ok()?;
            let head = sample.head.canonicalize().ok()?;
            return Some((base, head));
        }
    }
    // GitHub URL-run path. `pr.repo` is `owner/name`; base_sha +
    // head_sha are the two commits git_sync checked out.
    if let Some((owner, name)) = artifact.pr.repo.split_once('/') {
        let cache_root = state.cache.root();
        let base = git_sync::checkout_path(cache_root, owner, name, &artifact.pr.base_sha);
        let head = git_sync::checkout_path(cache_root, owner, name, &artifact.pr.head_sha);
        if base.is_dir() && head.is_dir() {
            let base = base.canonicalize().ok()?;
            let head = head.canonicalize().ok()?;
            return Some((base, head));
        }
    }
    None
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

/// Response of `GET /llm-config` — the server's current LLM regime
/// as resolved from env (`ADR_LLM`, `ADR_PROBE_LLM`, `ADR_PROOF_LLM`
/// plus provider defaults). The frontend compares this to the
/// `artifact.baseline` pin and renders a "re-baseline required"
/// banner per RFC v0.3 §9 when the user's current config would
/// produce different numbers than the displayed artifact.
///
/// Fields are `None` when the corresponding pass wouldn't run:
/// synthesis disabled (no `ADR_LLM` or `ADR_GLM_API_KEY`), probe
/// defaults to the main config, proof defaults to GLM-4.7 when the
/// key is set but is `None` otherwise.
///
/// Model names only — deliberately no provider / URL / API-key
/// exposure. Everything else leaks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmConfigView {
    /// The flow-synthesis model (`ADR_LLM`-resolved). `None` when
    /// synthesis is disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthesis_model: Option<String>,
    /// The probe model (`ADR_PROBE_LLM`, falls back to synthesis).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe_model: Option<String>,
    /// The proof model (`ADR_PROOF_LLM`, defaults to `glm-4.7` when
    /// `ADR_GLM_API_KEY` is set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_model: Option<String>,
}

impl LlmConfigView {
    /// Compose from ambient env. Swallows auth-gated errors in the
    /// proof path — if `from_env_proof` can't engage we report
    /// `proof_model = None`, which is the honest signal.
    pub fn from_env() -> Self {
        Self {
            synthesis_model: LlmConfig::from_env().map(|c| c.model),
            probe_model: LlmConfig::from_env_probe().map(|c| c.model),
            proof_model: LlmConfig::from_env_proof().map(|c| c.model),
        }
    }
}

async fn get_llm_config() -> Json<LlmConfigView> {
    Json(LlmConfigView::from_env())
}

/// `GET /samples` — the landing-page demo gallery. Model names only,
/// no on-disk paths (see [`SampleView`]). Empty when the server was
/// started without a fixtures root.
async fn list_samples(State(state): State<AppState>) -> Json<Vec<SampleView>> {
    Json(state.samples.view())
}

/// `POST /analyze/sample/:id` — kick off the same pipeline `/analyze`
/// runs, but against a built-in sample the server resolves
/// internally. Frontend never sees the on-disk paths.
///
/// Mirrors `/analyze` semantics for dedupe + spawn + cache behaviour
/// so a click on the gallery card behaves identically to a `POST
/// /analyze` with the sample's paths. When the sample dir includes
/// an `intent.json` next to `base/`/`head/`, it's loaded and passed
/// through — so intent-fit + proof passes actually run on demos
/// instead of always skipping. Notes aren't supported on sample
/// runs (they're reviewer side-channel input).
async fn analyze_sample(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<AnalyzeResponse>, (StatusCode, String)> {
    let sample = state
        .samples
        .get(&id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("unknown sample id: {id}")))?
        .clone();
    let user_id = Session::from_jar(&jar).map(|s| s.user_id);
    let base_root = sample
        .base
        .canonicalize()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("canonicalize sample base: {e}")))?;
    let head_root = sample
        .head
        .canonicalize()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("canonicalize sample head: {e}")))?;
    // Optional intent.json alongside the sample — enables intent-fit
    // + proof on demos. Malformed JSON fails the analyse request
    // loudly (better than silently dropping intent).
    let intent = match sample.intent.as_ref() {
        None => None,
        Some(p) => {
            let bytes = std::fs::read(p).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("read sample intent {}: {e}", p.display()),
                )
            })?;
            let parsed: adr_core::IntentInput = serde_json::from_slice(&bytes).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("parse sample intent {}: {e}", p.display()),
                )
            })?;
            Some(parsed)
        }
    };

    let dedupe_key = request_dedupe_key(&base_root, &head_root, intent.as_ref(), "");
    if let Some(entry) = state.inflight.get(&dedupe_key) {
        let existing = *entry.value();
        if let Some(job_ref) = state.jobs.get(&existing) {
            let status = job_ref.status.read().await.clone();
            if matches!(status, crate::job::JobStatus::Pending) {
                return Ok(Json(AnalyzeResponse { job_id: existing }));
            }
        }
    }

    let job = Job::new(base_root.clone(), head_root.clone());
    let job_id = job.id;
    state.jobs.insert(job_id, job.clone());
    state.inflight.insert(dedupe_key.clone(), job_id);
    let cache = state.cache.clone();
    let inflight = state.inflight.clone();
    let db = state.db.clone();
    // The sample id is the "repo" label so the sidebar can show it
    // cleanly; pr_number stays None (these aren't real PRs).
    let pr_ctx = PrContext {
        repo: Some(format!("sample/{}", sample.id)),
        pr_number: None,
        user_id,
    };
    tokio::spawn(async move {
        run_pipeline(PipelineRequest {
            job,
            base: base_root,
            head: head_root,
            cache,
            db,
            intent,
            notes: String::new(),
            pr_ctx,
        })
        .await;
        inflight.remove_if(&dedupe_key, |_, v| *v == job_id);
    });
    Ok(Json(AnalyzeResponse { job_id }))
}

#[cfg(test)]
mod tests {
    //! Security + dedupe tests for the router-local pure helpers.
    //! `resolve_inside` is the gatekeeper for `/analyze/:id/file` —
    //! a bug here is a path-traversal exposure. `request_dedupe_key`
    //! decides when two concurrent analyses get coalesced.

    use super::*;
    use adr_core::intent::{Intent, IntentClaim, IntentInput};

    // ---- request_dedupe_key -------------------------------------------

    fn structured(title: &str) -> IntentInput {
        IntentInput::Structured(Intent {
            title: title.into(),
            summary: "".into(),
            claims: vec![IntentClaim {
                statement: "c".into(),
                evidence_type: adr_core::intent::EvidenceType::Observation,
                detail: "".into(),
            }],
        })
    }

    #[test]
    fn dedupe_key_is_deterministic() {
        let a = request_dedupe_key(Path::new("/a"), Path::new("/b"), None, "");
        let b = request_dedupe_key(Path::new("/a"), Path::new("/b"), None, "");
        assert_eq!(a, b);
    }

    #[test]
    fn dedupe_key_splits_on_head_path() {
        let a = request_dedupe_key(Path::new("/a"), Path::new("/b"), None, "");
        let b = request_dedupe_key(Path::new("/a"), Path::new("/c"), None, "");
        assert_ne!(a, b);
    }

    #[test]
    fn dedupe_key_splits_on_intent() {
        let a =
            request_dedupe_key(Path::new("/a"), Path::new("/b"), Some(&structured("t1")), "");
        let b =
            request_dedupe_key(Path::new("/a"), Path::new("/b"), Some(&structured("t2")), "");
        assert_ne!(a, b, "two users analysing the same PR with different intents must not coalesce");
    }

    #[test]
    fn dedupe_key_splits_on_notes() {
        let a = request_dedupe_key(Path::new("/a"), Path::new("/b"), None, "notes-a");
        let b = request_dedupe_key(Path::new("/a"), Path::new("/b"), None, "notes-b");
        assert_ne!(a, b);
    }

    // ---- resolve_inside -----------------------------------------------

    fn tmp_root() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        // Create a safe inner file so canonicalize has something to land on.
        std::fs::write(root.join("inner.ts"), "ok").unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/nested.ts"), "ok").unwrap();
        (dir, root)
    }

    #[test]
    fn resolve_inside_accepts_relative_file() {
        let (_tmp, root) = tmp_root();
        let p = resolve_inside(&root, "inner.ts").unwrap();
        assert_eq!(p, root.join("inner.ts"));
    }

    #[test]
    fn resolve_inside_accepts_nested_relative_file() {
        let (_tmp, root) = tmp_root();
        let p = resolve_inside(&root, "sub/nested.ts").unwrap();
        assert_eq!(p, root.join("sub").join("nested.ts"));
    }

    #[test]
    fn resolve_inside_rejects_absolute_unix_path() {
        let (_tmp, root) = tmp_root();
        // On Windows, Path::is_absolute rejects this (no drive), so
        // canonicalize fails; the starts_with check catches any
        // accidental traversal. Either way: error.
        assert!(resolve_inside(&root, "/etc/passwd").is_err());
    }

    #[test]
    fn resolve_inside_rejects_windows_drive_paths() {
        let (_tmp, root) = tmp_root();
        // `C:\Windows\notepad.exe` — the `:` check catches this even on
        // non-Windows hosts (defence in depth against cross-platform
        // path strings landing on a Windows server).
        assert!(resolve_inside(&root, "C:\\Windows\\notepad.exe").is_err());
    }

    #[test]
    fn resolve_inside_rejects_backslash_root() {
        let (_tmp, root) = tmp_root();
        assert!(resolve_inside(&root, "\\\\server\\share").is_err());
    }

    #[test]
    fn resolve_inside_rejects_dotdot_escape() {
        let (_tmp, root) = tmp_root();
        // Create a sibling file outside root that ../ would reach.
        let outside = root.parent().unwrap().join("outside.ts");
        let _ = std::fs::write(&outside, "leak").ok();
        let res = resolve_inside(&root, "../outside.ts");
        // Either canonicalize fails (file absent on this platform) or
        // the starts_with check rejects. Both end as errors.
        assert!(res.is_err(), "dotdot escape must be rejected");
        let _ = std::fs::remove_file(&outside);
    }
}
