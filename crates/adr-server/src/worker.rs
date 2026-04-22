use std::path::{Path, PathBuf};
use std::sync::Arc;

use adr_core::{artifact::PrRef, Artifact, CostStatus, Flow, IntentInput, ProofStatus, SynthStatus};
use anyhow::{Context, Result};

use crate::cache::Cache;
use crate::db::{AnalysisRow, AnalysisStatus, DbStore};
use crate::job::{Job, JobStatus, ProgressEvent};
use crate::llm::intent_pipeline::{IntentPipeline, PerFlowResult};
use crate::llm::probe_pipeline::ProbePipeline;
use crate::llm::{LlmConfig, SynthesisOutcome};

/// Upstream-supplied PR context — populated by the URL-driven analyse
/// flow so the sidebar can show "owner/repo #N" immediately and the
/// artifact's [`PrRef`] carries the real identity instead of "unknown".
/// `None` for locally-pathed runs; the worker falls back to the tail
/// directory name.
#[derive(Debug, Clone, Default)]
pub struct PrContext {
    pub repo: Option<String>,
    pub pr_number: Option<i64>,
    pub user_id: Option<String>,
}

/// Everything needed to run one analysis. Bundled so [`run_pipeline`]
/// stays at one argument — the router builds this in place at the
/// call site, the worker destructures it into `run_inner` state.
pub struct PipelineRequest {
    pub job: Arc<Job>,
    pub base: PathBuf,
    pub head: PathBuf,
    pub cache: Arc<Cache>,
    pub db: DbStore,
    pub intent: Option<IntentInput>,
    pub notes: String,
    pub pr_ctx: PrContext,
}

pub async fn run_pipeline(req: PipelineRequest) {
    // We need `job` + `db` after the inner call to report errors, so
    // hold onto them before moving the rest of the request downstream.
    // `Arc<Job>` and `DbStore` both clone cheaply.
    let job = req.job.clone();
    let db = req.db.clone();
    match run_inner(req).await {
        Ok(()) => {}
        Err(e) => {
            let msg = format!("{e:#}");
            let _ = job.progress.send(ProgressEvent {
                stage: "error".into(),
                percent: 100,
                message: msg.clone(),
            });
            *job.status.write().await = JobStatus::Error { message: msg.clone() };
            // Best-effort DB write — don't propagate errors from the
            // write itself, the in-memory job state is already updated.
            let _ = upsert_errored_row(&db, &job.id, &msg).await;
        }
    }
}

/// Helper: on an error that happens before we've computed the cache
/// key, we still want a DB row so the landing-page history shows
/// the attempt. This writes a minimal errored row keyed by job id
/// only — no dedup against head_sha.
async fn upsert_errored_row(db: &DbStore, job_id: &uuid::Uuid, message: &str) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    db.upsert_analysis(&AnalysisRow {
        id: job_id.to_string(),
        user_id: None,
        repo: None,
        pr_number: None,
        head_sha: format!("job-{job_id}"),
        intent_fp: "pre-key".into(),
        llm_sig: "pre-key".into(),
        artifact_key: None,
        status: AnalysisStatus::Errored,
        message: Some(message.to_string()),
        created_at: now.clone(),
        updated_at: now,
    })
    .await
}

async fn run_inner(req: PipelineRequest) -> Result<()> {
    let PipelineRequest {
        job: owned_job,
        base: owned_base,
        head: owned_head,
        cache: owned_cache,
        db: owned_db,
        intent,
        notes,
        pr_ctx: owned_pr_ctx,
    } = req;
    // Re-bind as borrows so the rest of the body (which takes
    // references for a bunch of calls) doesn't care that we switched
    // to an owned-request input.
    let job = &owned_job;
    let base = owned_base.as_path();
    let head = owned_head.as_path();
    let cache = &owned_cache;
    let db = &owned_db;
    let pr_ctx = &owned_pr_ctx;
    let repo_label = pr_ctx
        .repo
        .clone()
        .or_else(|| infer_repo_label(head).map(str::to_owned));
    let pr_number = pr_ctx.pr_number;
    let user_id = pr_ctx.user_id.clone();
    let llm_cfg = LlmConfig::from_env();
    // Cache signature spans synthesis + proof so two runs with the
    // same synthesis model but different ADR_PROOF_LLM (or one with
    // proof, one without) don't collide on the same cache entry and
    // serve a stale artifact. Baseline pin (ArtifactBaseline) carries
    // the same (probe/synthesis/proof) tuple at the RFC v0.3 §9 level;
    // this line is the cache's side of the same invariant.
    let llm_sig = match &llm_cfg {
        Some(c) => {
            let synth = format!("{}:{}@{}", c.provider, c.model, c.prompt_version);
            let proof = LlmConfig::from_env_proof()
                .map(|p| format!("{}:{}", p.provider, p.model))
                .unwrap_or_else(|| "none".into());
            Some(format!("{synth}+proof={proof}"))
        }
        None => None,
    };
    let intent_fp = intent_fingerprint(intent.as_ref(), &notes);

    // Parse head first — it's cheap (10–30s) and gives us the
    // content-addressed `head_sha` we use for the cache key. Keying
    // on head content (not paths) means two users analysing the same
    // PR share cache hits regardless of where their worktrees live.
    emit(job, "parse-head", 15, "walking head tree").await;
    let head_graph = {
        let head = head.to_path_buf();
        tokio::task::spawn_blocking(move || adr_parse::Ingest::new("head").ingest_dir(&head))
            .await
            .context("parse-head join")??
    };

    // Build a minimal artifact probe just to compute the head_sha.
    let head_sha = {
        let mut probe = Artifact::new(PrRef {
            repo: "unknown".into(),
            base_sha: String::new(),
            head_sha: String::new(),
        });
        probe.head = head_graph.clone();
        probe.snapshot_sha(adr_core::Side::Head)
    };
    let key = cache.key(&head_sha, llm_sig.as_deref(), &intent_fp);
    let llm_sig_db = llm_sig.clone().unwrap_or_else(|| "structural".into());

    // Record a pending row before heavy work starts so the landing
    // page can show "running…" for this job as soon as it's accepted.
    let now = chrono::Utc::now().to_rfc3339();
    let _ = db
        .upsert_analysis(&AnalysisRow {
            id: job.id.to_string(),
            user_id: user_id.clone(),
            repo: repo_label.clone(),
            pr_number,
            head_sha: head_sha.clone(),
            intent_fp: intent_fp.clone(),
            llm_sig: llm_sig_db.clone(),
            artifact_key: None,
            status: AnalysisStatus::Pending,
            message: None,
            created_at: now.clone(),
            updated_at: now,
        })
        .await;

    // Cache hit → skip the rest of the pipeline, publish a single
    // "ready" event.
    if let Some(a) = cache.get(&key)? {
        *job.artifact.write().await = Some(a);
        *job.status.write().await = JobStatus::Ready;
        let _ = job.progress.send(ProgressEvent {
            stage: "ready".into(),
            percent: 100,
            message: "cached".into(),
        });
        // Upgrade DB row to ready + point at the cached artifact.
        let now = chrono::Utc::now().to_rfc3339();
        let _ = db
            .upsert_analysis(&AnalysisRow {
                id: job.id.to_string(),
                user_id: user_id.clone(),
                repo: repo_label.clone(),
                pr_number,
                head_sha: head_sha.clone(),
                intent_fp: intent_fp.clone(),
                llm_sig: llm_sig_db.clone(),
                artifact_key: Some(key.clone()),
                status: AnalysisStatus::Ready,
                message: Some("cache hit".into()),
                created_at: now.clone(),
                updated_at: now,
            })
            .await;
        return Ok(());
    }

    // Overlap parse-base with cfg-head — they're independent (cfg-head
    // only needs head_graph, which is already parsed). Saves ~parse-base
    // duration on a big repo (10–20s for glide-mq #181).
    emit(job, "parse-base", 30, "walking base tree").await;
    let parse_base_task = {
        let base = base.to_path_buf();
        tokio::task::spawn_blocking(move || adr_parse::Ingest::new("base").ingest_dir(&base))
    };
    emit(job, "cfg", 35, "building control-flow graphs").await;
    let cfg_head_task = {
        let g = head_graph.clone();
        let root = head.to_path_buf();
        tokio::task::spawn_blocking(move || adr_cfg::build_for_graph(&g, &root))
    };
    let base_graph = parse_base_task.await.context("parse-base join")??;
    let head_cfg = cfg_head_task.await.context("cfg-head join")??;

    // cfg-base needs base_graph; run now that parse-base finished.
    let base_cfg = {
        let g = base_graph.clone();
        let root = base.to_path_buf();
        tokio::task::spawn_blocking(move || adr_cfg::build_for_graph(&g, &root))
            .await
            .context("cfg-base join")??
    };

    emit(job, "hunks", 75, "extracting semantic hunks").await;
    let hunks = adr_hunks::extract_all(&base_graph, &head_graph);

    let mut artifact = Artifact::new(PrRef {
        repo: repo_label.clone().unwrap_or_else(|| "unknown".into()),
        base_sha: base.display().to_string(),
        head_sha: head.display().to_string(),
    });
    artifact.base = base_graph;
    artifact.head = head_graph;
    artifact.base_cfg = base_cfg;
    artifact.head_cfg = head_cfg;
    artifact.hunks = hunks;
    artifact.intent = intent;
    artifact.notes = notes;

    emit(job, "flows", 90, "structural flow clustering").await;
    artifact.flows = adr_flows::cluster(&artifact);

    // Evidence pass — runs against the structural flows first so we
    // can publish READY immediately. Claims are keyed on hunk_ids,
    // so when synth (background) later renames/splits flows, the
    // evidence stays attached correctly via a merge pass.
    emit(job, "evidence", 97, "collecting evidence").await;
    artifact = adr_evidence::collect(artifact);

    // Synth decision: `will_run_synth` controls whether we publish
    // READY with structural flows (true) and spawn synth in the
    // background, or wait for synth to finish before READY.
    // Background-synth is the new default (slice C — "publish what's
    // ready"); with no LLM config we stay on structural.
    let will_run_synth = llm_cfg.is_some();
    if will_run_synth {
        artifact.synth_status = SynthStatus::Analyzing;
    }
    // When synth runs in the background, we do NOT cache the
    // pre-synth artifact — the cache entry would then pin structural
    // flows under the LLM signature, causing the next run with the
    // same config to serve stale names. Only cache after synth's
    // writeback (below, in the background task) or when there's no
    // LLM configured at all.
    let llm_accepted = false;

    // Decide whether we'll run the probe pass after ready. Probe is
    // opt-in on the same env rules as synthesis; absence means the UI
    // shows `cost_status: not-run` and the heuristic cost stands.
    let probe_cfg = LlmConfig::from_env_probe();
    if probe_cfg.is_some() {
        artifact.cost_status = CostStatus::Analyzing;
    }

    // Intent + proof passes — gated on (a) intent supplied and (b)
    // a proof LLM configured. Per `feedback_proof_uses_glm.md`,
    // `from_env_proof` defaults to GLM-4.7 when `ADR_GLM_API_KEY`
    // is set. Analyzing → the UI shows the "intent + proof analysing"
    // state while the background task fans out GLM sessions per flow.
    let proof_cfg = LlmConfig::from_env_proof();
    let will_run_proof = artifact.intent.is_some() && proof_cfg.is_some();
    if will_run_proof {
        artifact.proof_status = ProofStatus::Analyzing;
    }

    // Only cache when the outcome matches the key. If synth runs in
    // the background we defer the cache write until it completes
    // (the background task calls `cache.put` itself). Otherwise —
    // no LLM, no synth pending — cache now.
    let _ = llm_accepted; // preserved for the unused-warning guard
    let should_cache = !will_run_synth;
    if should_cache {
        cache.put(&key, &artifact)?;
    }
    *job.artifact.write().await = Some(artifact.clone());
    *job.status.write().await = JobStatus::Ready;

    let _ = job.progress.send(ProgressEvent {
        stage: "ready".into(),
        percent: 100,
        message: "done".into(),
    });

    // Mark the DB row ready — landing page will surface this analysis
    // in its history list. Best-effort: a DB write failure must not
    // fail the whole pipeline when artifact is already cached on disk.
    let now = chrono::Utc::now().to_rfc3339();
    if let Err(e) = db
        .upsert_analysis(&AnalysisRow {
            id: job.id.to_string(),
            user_id: user_id.clone(),
            repo: repo_label.clone(),
            pr_number,
            head_sha: head_sha.clone(),
            intent_fp: intent_fp.clone(),
            llm_sig: llm_sig_db.clone(),
            artifact_key: if should_cache { Some(key.clone()) } else { None },
            status: AnalysisStatus::Ready,
            message: None,
            created_at: now.clone(),
            updated_at: now,
        })
        .await
    {
        tracing::warn!(error = %e, "db upsert (ready) failed — landing page may miss this row");
    }

    // Slice C — synth runs in the background after READY. The user
    // sees structural flows + evidence immediately; flow names
    // refresh in place as soon as parallel synth returns (~30–90s
    // typical). Cache write is deferred to synth's completion so we
    // don't pin structural names under an LLM signature.
    if will_run_synth {
        let job_bg = Arc::clone(job);
        let cache_bg = Arc::clone(cache);
        let key_bg = key.clone();
        let llm_cfg = llm_cfg.clone().expect("gated by will_run_synth");
        let artifact_for_synth = artifact.clone();
        tokio::spawn(async move {
            run_synth_pass(job_bg, cache_bg, key_bg, artifact_for_synth, llm_cfg).await;
        });
    }

    // Kick off the probe pass after the ready event — the UI can render
    // everything else while cost fills in. The task holds its own Arc
    // handles to the job + cache so the caller's stack frame exits
    // cleanly.
    if let Some(probe_cfg) = probe_cfg {
        let job_bg = Arc::clone(job);
        let cache_bg = Arc::clone(cache);
        let key_bg = key.clone();
        let cache_eligible = true;
        let base_path = base.to_path_buf();
        let head_path = head.to_path_buf();
        let artifact_for_probe = artifact.clone();
        tokio::spawn(async move {
            run_probe_pass(ProbePassInputs {
                job: job_bg,
                cache: cache_bg,
                cache_eligible,
                key: key_bg,
                artifact: artifact_for_probe,
                probe_cfg,
                base_path,
                head_path,
            })
            .await;
        });
    }

    // Intent + proof runs in parallel with probe — it touches a
    // different axis (per-flow IntentFit/Proof claims) and uses the
    // cloud LLM, not the local probe model. Skipped when there's no
    // intent on the artifact.
    if will_run_proof {
        let job_bg = Arc::clone(job);
        let cache_bg = Arc::clone(cache);
        let key_bg = key.clone();
        let cache_eligible = true;
        let head_path = head.to_path_buf();
        let proof_cfg = proof_cfg.expect("gated by will_run_proof");
        tokio::spawn(async move {
            run_intent_pass(
                job_bg,
                cache_bg,
                cache_eligible,
                key_bg,
                artifact,
                proof_cfg,
                head_path,
            )
            .await;
        });
    }

    Ok(())
}

/// Background synth pass. Fans out parallel per-cluster GLM calls,
/// merges the proposed names/rationales/splits into the live artifact
/// via `merge_into_artifact`, and caches the updated result so the
/// next run hits the LLM-stamped cache entry.
async fn run_synth_pass(
    job: Arc<Job>,
    cache: Arc<Cache>,
    key: String,
    snapshot: Artifact,
    cfg: LlmConfig,
) {
    let _ = job.progress.send(ProgressEvent {
        stage: "llm-synthesize".into(),
        percent: 0,
        message: "Naming flows".into(),
    });
    let outcome = crate::llm::synth_parallel::synthesize_parallel(&snapshot, &cfg).await;
    match outcome {
        SynthesisOutcome::Accepted(flows) => {
            tracing::info!(count = flows.len(), "parallel synth accepted");
            merge_into_artifact(&job, &cache, true, &key, |current| {
                // Merge names/rationale/source by matching flow ids;
                // split subflows (new ids) are appended. This is a
                // conservative rule — any flow whose id appears in the
                // synth output updates; ids that only appear in the
                // current live artifact stay put (they might be from a
                // splitting subflow elsewhere).
                let mut by_id: std::collections::HashMap<String, Flow> = flows
                    .iter()
                    .cloned()
                    .map(|f| (f.id.clone(), f))
                    .collect();
                for f in current.flows.iter_mut() {
                    if let Some(updated) = by_id.remove(&f.id) {
                        f.name = updated.name;
                        f.rationale = updated.rationale;
                        f.source = updated.source;
                    }
                }
                for (_, new_flow) in by_id {
                    // Any id unique to synth output (split subflow) gets
                    // appended.
                    current.flows.push(new_flow);
                }
                current.synth_status = SynthStatus::Ready;
                // Baseline.synthesis_model is plucked by adr-cost at
                // probe-finish time; synth runs in parallel and can
                // land AFTER probe, leaving the pin stale at `None`.
                // Refresh from the now-updated flows so the drift
                // banner doesn't falsely claim synthesis was skipped.
                if let Some(baseline) = current.baseline.as_mut() {
                    let synth_model = current.flows.iter().find_map(|f| match &f.source {
                        adr_core::FlowSource::Llm { model, .. } => Some(model.clone()),
                        adr_core::FlowSource::Structural => None,
                    });
                    baseline.synthesis_model = synth_model;
                }
            })
            .await;
            let _ = job.progress.send(ProgressEvent {
                stage: "llm-synthesize".into(),
                percent: 100,
                message: "flow names updated".into(),
            });
        }
        SynthesisOutcome::Rejected { rule, detail } => {
            tracing::warn!(rule = %rule, detail = %detail, "parallel synth rejected");
            merge_into_artifact(&job, &cache, true, &key, |current| {
                current.synth_status = SynthStatus::Errored;
            })
            .await;
            let _ = job.progress.send(ProgressEvent {
                stage: "llm-synthesize".into(),
                percent: 100,
                message: format!("kept structural names · {rule}"),
            });
        }
        SynthesisOutcome::NoFinalize | SynthesisOutcome::Errored(_) => {
            tracing::warn!("parallel synth did not complete");
            merge_into_artifact(&job, &cache, true, &key, |current| {
                current.synth_status = SynthStatus::Errored;
            })
            .await;
            let _ = job.progress.send(ProgressEvent {
                stage: "llm-synthesize".into(),
                percent: 100,
                message: "kept structural names".into(),
            });
        }
    }
}

/// Inputs for [`run_probe_pass`] — a spawned tokio task, so all args
/// are owned. Bundled as a struct so the spawn site reads field-by-
/// field instead of 8 positional args.
struct ProbePassInputs {
    job: Arc<Job>,
    cache: Arc<Cache>,
    cache_eligible: bool,
    key: String,
    artifact: Artifact,
    probe_cfg: LlmConfig,
    base_path: PathBuf,
    head_path: PathBuf,
}

/// Background task: drive the probe pass, update the artifact's
/// `cost_status`, and write the refreshed artifact back to cache.
async fn run_probe_pass(inputs: ProbePassInputs) {
    let ProbePassInputs {
        job,
        cache,
        cache_eligible,
        key,
        mut artifact,
        probe_cfg,
        base_path,
        head_path,
    } = inputs;
    let _ = job.progress.send(ProgressEvent {
        stage: "probe".into(),
        percent: 0,
        message: "Probing the repo for navigation cost (base + head)".into(),
    });
    // Baseline storage root: `<cache_dir>/../baseline`. Cache dir lives
    // at `.adr/cache` by default, so sibling `.adr/baseline` is the
    // natural home for probe output.
    let baseline_root = cache.root().parent().map(|p| p.join("baseline")).unwrap_or_else(|| PathBuf::from(".adr/baseline"));
    let pipeline = ProbePipeline {
        probe_cfg: &probe_cfg,
        baseline_root: &baseline_root,
        // Repo key is derived from the head path — per-machine stable,
        // doesn't depend on the cache key.
        repo_root: &head_path,
    };
    match pipeline.run(&artifact).await {
        Ok(outcome) => {
            tracing::info!(
                base_entities = outcome.base.per_entity.len(),
                head_entities = outcome.head.per_entity.len(),
                "probe pass ready — attributing cost"
            );
            artifact = adr_cost::attribute_from_baselines(artifact, &outcome.base, &outcome.head);
            artifact.cost_status = CostStatus::Ready;
            let net_summary: Vec<String> = artifact
                .flows
                .iter()
                .filter_map(|f| f.cost.as_ref().map(|c| format!("{}={:+}", f.name, c.net)))
                .collect();
            let _ = job.progress.send(ProgressEvent {
                stage: "probe".into(),
                percent: 100,
                message: format!(
                    "cost attributed — {}",
                    net_summary.join(" · ")
                ),
            });
        }
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "probe pass errored");
            artifact.cost_status = CostStatus::Errored;
            let _ = job.progress.send(ProgressEvent {
                stage: "probe".into(),
                percent: 100,
                message: format!("probe errored: {e:#}"),
            });
        }
    }

    // Drop unused params — they'll matter when the cost-delta
    // computation in the next milestone wants the original working
    // trees for `read` / `grep` fallback.
    let _ = base_path;

    // Writeback: merge probe-owned fields (cost_status, baseline,
    // per-flow cost) into the live artifact under the write lock so
    // the concurrent intent+proof pass can't clobber them with its
    // own stale clone.
    merge_into_artifact(&job, &cache, cache_eligible, &key, |current| {
        current.cost_status = artifact.cost_status;
        // Baseline rewrite — but preserve synthesis_model / proof_model
        // if they were already populated. Probe ran on a clone of the
        // pre-synth/pre-proof artifact, so its `attribute_from_baselines`
        // sees structural flows + no Proof → derives None for those
        // pins. Synth/proof may have set them on the live artifact
        // concurrently; don't clobber.
        let mut new_baseline = artifact.baseline.clone();
        if let (Some(nb), Some(prev)) = (new_baseline.as_mut(), current.baseline.as_ref()) {
            if nb.synthesis_model.is_none() && prev.synthesis_model.is_some() {
                nb.synthesis_model = prev.synthesis_model.clone();
            }
            if nb.proof_model.is_none() && prev.proof_model.is_some() {
                nb.proof_model = prev.proof_model.clone();
            }
        }
        // After merging, also pluck synthesis_model / proof_model from
        // the live flows in case synth/proof already landed on the
        // artifact without having set the baseline pin (pre-baseline
        // arrivals).
        if let Some(nb) = new_baseline.as_mut() {
            if nb.synthesis_model.is_none() {
                nb.synthesis_model = current.flows.iter().find_map(|f| match &f.source {
                    adr_core::FlowSource::Llm { model, .. } => Some(model.clone()),
                    adr_core::FlowSource::Structural => None,
                });
            }
            if nb.proof_model.is_none() {
                nb.proof_model = current
                    .flows
                    .iter()
                    .find_map(|f| f.proof.as_ref().map(|p| p.model.clone()));
            }
        }
        current.baseline = new_baseline;
        // Copy per-flow cost entries by flow_id.
        for src in &artifact.flows {
            if let Some(dst) = current.flows.iter_mut().find(|f| f.id == src.id) {
                dst.cost = src.cost.clone();
            }
        }
    })
    .await;
}

/// Best-effort label for the DB `repo` column when we only have a
/// local path — picks the final component (e.g. `/tmp/glide-mq-head-181`
/// → `glide-mq-head-181`). Later, when we add GitHub-sourced analyses,
/// the caller passes `owner/name` directly and this is skipped.
fn infer_repo_label(head: &Path) -> Option<&str> {
    head.file_name().and_then(|s| s.to_str())
}

async fn emit(job: &Arc<Job>, stage: &str, percent: u8, message: &str) {
    let _ = job.progress.send(ProgressEvent {
        stage: stage.into(),
        percent,
        message: message.into(),
    });
}

/// Background task: run intent-fit + proof-verification LLM passes
/// per flow. Mirrors `run_probe_pass`: stamps `proof_status`
/// accordingly, writes back to cache on completion.
async fn run_intent_pass(
    job: Arc<Job>,
    cache: Arc<Cache>,
    cache_eligible: bool,
    key: String,
    mut artifact: Artifact,
    proof_cfg: LlmConfig,
    head_path: PathBuf,
) {
    let _ = job.progress.send(ProgressEvent {
        stage: "proof".into(),
        percent: 0,
        message: "Matching flows to intent & hunting for proof".into(),
    });

    let pipeline = IntentPipeline {
        proof_cfg: &proof_cfg,
        repo_root: &head_path,
        intent_fit_version: "v0.1.0",
        proof_version: "v0.1.0",
    };

    match pipeline.run(&artifact).await {
        Ok(outcome) => {
            let mut errs: Vec<String> = Vec::new();
            if outcome.intent_summary.is_some() {
                artifact.intent_summary = outcome.intent_summary.clone();
            }
            merge_intent_outcome(&mut artifact, &outcome.per_flow, &mut errs);
            if errs.is_empty() {
                artifact.proof_status = ProofStatus::Ready;
            } else {
                // Partial outcome — some flows parsed, some didn't.
                // Surface as Errored so the UI's banner triggers, but
                // keep the partial claims in place so the reviewer
                // can still read the ones that worked.
                artifact.proof_status = ProofStatus::Errored;
                tracing::warn!(errors = ?errs, "intent + proof pass completed with per-flow failures");
            }
            let summary: Vec<String> = outcome
                .per_flow
                .iter()
                .filter_map(|r| {
                    let v = r.intent_fit.as_ref()?;
                    Some(format!("{}={:?}", r.flow_id, v.verdict))
                })
                .collect();
            let _ = job.progress.send(ProgressEvent {
                stage: "proof".into(),
                percent: 100,
                message: format!(
                    "intent + proof done · {}",
                    summary.join(" · ")
                ),
            });
        }
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "intent + proof pipeline errored");
            artifact.proof_status = ProofStatus::Errored;
            let _ = job.progress.send(ProgressEvent {
                stage: "proof".into(),
                percent: 100,
                message: format!("intent + proof errored: {e:#}"),
            });
        }
    }

    // Writeback: merge intent-owned fields (proof_status, per-flow
    // intent_fit + proof) into the live artifact under the write
    // lock so the concurrent probe pass can't clobber them.
    merge_into_artifact(&job, &cache, cache_eligible, &key, |current| {
        current.proof_status = artifact.proof_status;
        if artifact.intent_summary.is_some() {
            current.intent_summary = artifact.intent_summary.clone();
        }
        for src in &artifact.flows {
            if let Some(dst) = current.flows.iter_mut().find(|f| f.id == src.id) {
                dst.intent_fit = src.intent_fit.clone();
                dst.proof = src.proof.clone();
            }
        }
        // Refresh the baseline's proof_model pin. The probe pass
        // populated `current.baseline` but couldn't stamp proof_model
        // because proof hadn't run yet (both run as independent
        // background tasks). Without this refresh, re-baseline drift
        // detection can't tell two runs with different proof models
        // apart (RFC v0.3 §9). `None` when no flow received a Proof.
        if let Some(baseline) = current.baseline.as_mut() {
            let proof_model = current
                .flows
                .iter()
                .find_map(|f| f.proof.as_ref().map(|p| p.model.clone()));
            baseline.proof_model = proof_model;
        }
    })
    .await;
}

/// Atomically merge changes into the live job artifact and persist
/// to cache. Used by background tasks (probe, intent+proof) that run
/// concurrently with each other — without this, each task's final
/// writeback clobbers the other's (last-writer-wins race, seen in the
/// first live smoke where probe finished cost=ready then intent
/// wrote back with a stale cost_status=analyzing clone).
///
/// The mutation closure sees the current artifact from `job.artifact`
/// and mutates it in place. Held under the artifact write lock so no
/// other task can interleave.
async fn merge_into_artifact(
    job: &Arc<Job>,
    cache: &Arc<Cache>,
    cache_eligible: bool,
    key: &str,
    mutate: impl FnOnce(&mut Artifact),
) {
    let mut guard = job.artifact.write().await;
    let Some(current) = guard.as_mut() else {
        tracing::warn!(
            "merge_into_artifact: job.artifact missing — task raced with job teardown"
        );
        return;
    };
    mutate(current);
    let snapshot = current.clone();
    drop(guard);
    if cache_eligible {
        if let Err(e) = cache.put(key, &snapshot) {
            tracing::warn!(error = %e, "writeback to cache failed");
        }
    }
}

/// Merge `PerFlowResult`s into the artifact's flows by id. Records any
/// parse errors into `out_errors` (reported back to the UI via
/// `proof_status = Errored`).
fn merge_intent_outcome(
    artifact: &mut Artifact,
    per_flow: &[PerFlowResult],
    out_errors: &mut Vec<String>,
) {
    for r in per_flow {
        if let Some(flow) = artifact.flows.iter_mut().find(|f| f.id == r.flow_id) {
            flow.intent_fit = r.intent_fit.clone();
            flow.proof = r.proof.clone();
        } else {
            out_errors.push(format!("flow {} no longer in artifact", r.flow_id));
        }
        for e in &r.errors {
            out_errors.push(format!("{}: {}", r.flow_id, e));
        }
    }
}

/// Blake3 fingerprint of `(intent, notes)` — mixed into the cache key so
/// changing either invalidates the entry (different inputs → different
/// claims, different proof-axis fill). An empty input hashes to a
/// stable "no-intent" marker so repeated calls without intent hit the
/// same cache entry.
fn intent_fingerprint(intent: Option<&IntentInput>, notes: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"intent-v1|");
    match intent {
        None => h.update(b"none"),
        Some(i) => {
            let bytes = serde_json::to_vec(i).unwrap_or_default();
            h.update(&bytes)
        }
    };
    h.update(b"|notes|");
    h.update(notes.as_bytes());
    h.finalize().to_hex().to_string()
}

#[allow(dead_code)] // retained for the classic MCP-synth path if re-enabled
fn write_artifact_tmp(artifact: &Artifact) -> Result<PathBuf> {
    let dir = std::env::temp_dir();
    let fname = format!("adr-llm-{}.json", uuid::Uuid::new_v4());
    let path = dir.join(fname);
    let bytes = serde_json::to_vec(artifact)?;
    std::fs::write(&path, &bytes)
        .with_context(|| format!("writing artifact to {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use adr_core::intent::{Intent, IntentClaim, IntentInput};

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
    fn intent_fingerprint_is_deterministic() {
        let a = intent_fingerprint(Some(&structured("t")), "notes");
        let b = intent_fingerprint(Some(&structured("t")), "notes");
        assert_eq!(a, b);
    }

    #[test]
    fn intent_fingerprint_moves_on_intent_change() {
        let a = intent_fingerprint(Some(&structured("one")), "notes");
        let b = intent_fingerprint(Some(&structured("two")), "notes");
        assert_ne!(a, b, "different intent titles must change fingerprint");
    }

    #[test]
    fn intent_fingerprint_moves_on_notes_change() {
        let a = intent_fingerprint(Some(&structured("t")), "notes-a");
        let b = intent_fingerprint(Some(&structured("t")), "notes-b");
        assert_ne!(a, b, "reviewer notes are part of the cache key");
    }

    #[test]
    fn intent_fingerprint_none_and_empty_notes_is_stable() {
        // RFC v0.3: a no-intent / no-notes run should cache-hit on
        // repeated calls. The 'none' marker guarantees that.
        let a = intent_fingerprint(None, "");
        let b = intent_fingerprint(None, "");
        assert_eq!(a, b);
    }

    #[test]
    fn intent_fingerprint_none_differs_from_present_intent() {
        let a = intent_fingerprint(None, "");
        let b = intent_fingerprint(Some(&structured("t")), "");
        assert_ne!(a, b);
    }

    #[test]
    fn intent_fingerprint_raw_text_and_structured_differ() {
        let structured_fp = intent_fingerprint(Some(&structured("same-text")), "");
        let raw_fp =
            intent_fingerprint(Some(&IntentInput::RawText("same-text".into())), "");
        assert_ne!(
            structured_fp, raw_fp,
            "structured and raw intents aren't interchangeable from the cache's view"
        );
    }
}
