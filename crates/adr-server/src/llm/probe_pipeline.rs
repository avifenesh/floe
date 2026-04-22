//! Orchestrates the probe pass — ensures a fresh baseline exists for
//! both the base and head snapshots of an artifact, runs the three
//! frozen probes on whichever side lacks one, and returns once the
//! store is current.
//!
//! The cost-delta computation itself lives in [`super::cost`] (next
//! milestone, rewriting `adr-cost`). This module only handles probe
//! execution + baseline storage.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use adr_core::{Artifact, Side};
use adr_probe::{
    aggregate, probe_set, AggregateBaseline, BaselineKey, BaselineStatus, BaselineStore, ProbeId,
    ProbeResult, ProbeSession, PROBE_SET_VERSION,
};
use anyhow::{Context, Result};
use serde_json::json;

use super::config::{LlmConfig, LlmProvider};
use super::glm_client::GlmClient;
use super::model_defaults;
use super::ollama_client::OllamaClient;
use super::probe_client::{spawn_probe_mcp, BackingClient, McpProbeClient};

/// Result of a full probe-pipeline run: baselines for both sides of the
/// artifact are on disk, with [`AggregateBaseline`] handles to feed the
/// downstream cost-delta computation.
pub struct PipelineOutcome {
    pub base: AggregateBaseline,
    pub head: AggregateBaseline,
}

pub struct ProbePipeline<'a> {
    pub probe_cfg: &'a LlmConfig,
    pub baseline_root: &'a Path,
    pub repo_root: &'a Path,
}

impl<'a> ProbePipeline<'a> {
    /// Drive both sides of the artifact through the probe. Each side's
    /// baseline is reused if fresh, otherwise recomputed and saved.
    pub async fn run(&self, artifact: &Artifact) -> Result<PipelineOutcome> {
        let repo_key = BaselineKey::repo_key_for(self.repo_root);
        let store = BaselineStore::new_at(self.baseline_root);
        let base = self
            .ensure_side(artifact, Side::Base, &store, &repo_key)
            .await
            .context("probe base side")?;
        let head = self
            .ensure_side(artifact, Side::Head, &store, &repo_key)
            .await
            .context("probe head side")?;
        Ok(PipelineOutcome { base, head })
    }

    async fn ensure_side(
        &self,
        artifact: &Artifact,
        side: Side,
        store: &BaselineStore,
        repo_key: &str,
    ) -> Result<AggregateBaseline> {
        let sha = artifact.snapshot_sha(side);
        let key = BaselineKey {
            repo_key: repo_key.to_string(),
            sha: sha.clone(),
            probe_model: self.probe_cfg.model.clone(),
            probe_set_version: PROBE_SET_VERSION.to_string(),
        };

        let status = store.status(&key).unwrap_or(BaselineStatus::Missing);
        if status == BaselineStatus::Fresh {
            tracing::info!(side = ?side, sha = %short(&sha), "baseline fresh — skipping probe");
            return store.load(&key);
        }
        tracing::info!(side = ?side, sha = %short(&sha), status = ?status, "running probes");
        let (aggregate_baseline, per_probe) = self.run_side_probes(artifact, side).await?;
        store.save(&key, &aggregate_baseline, &per_probe)?;
        Ok(aggregate_baseline)
    }

    async fn run_side_probes(
        &self,
        artifact: &Artifact,
        side: Side,
    ) -> Result<(AggregateBaseline, Vec<(ProbeId, ProbeResult)>)> {
        // 1. Persist the side-only artifact to a temp path the MCP child
        // can read.
        let side_only = artifact.side_only(side);
        let tmp_path = write_tmp_artifact(&side_only)?;
        let _guard = TempFile::new(tmp_path.clone());

        // 2. Spawn MCP child + grab the probe-safe tool list.
        let (mcp, tool_specs) = spawn_probe_mcp(&tmp_path).await?;

        // 3. Build the backing chat client (Ollama or GLM).
        let backing = build_backing_client(self.probe_cfg).await?;
        let client = McpProbeClient::new(
            backing,
            Arc::clone(&mcp),
            tool_specs,
            self.probe_cfg.model.clone(),
            match self.probe_cfg.provider {
                LlmProvider::Ollama => Some(self.probe_cfg.keep_alive.clone()),
                LlmProvider::Glm => None,
            },
            Some(json!({
                "num_ctx": self.probe_cfg.num_ctx,
                "num_predict": self.probe_cfg.num_predict,
                "temperature": self.probe_cfg.temperature,
            })),
        );

        // 4. Run each of the 3 probes in its own clean session.
        let mut per_probe: Vec<(ProbeId, ProbeResult)> = Vec::new();
        let mut results: Vec<ProbeResult> = Vec::new();
        for def in probe_set() {
            let session = ProbeSession::new(&def);
            let result = session.run(&client).await.with_context(|| {
                format!("probe `{}` ({})", def.id.as_str(), def.label)
            })?;
            tracing::info!(
                probe = def.id.as_str(),
                turns = result.turns,
                tool_calls = result.tool_calls,
                tokens_in = result.tokens_in,
                tokens_out = result.tokens_out,
                end_reason = %result.end_reason,
                "probe completed"
            );
            per_probe.push((def.id, result.clone()));
            results.push(result);
        }

        // 5. Shut the MCP child down; the child is per-side so each side
        // gets a fresh server state.
        let child = Arc::try_unwrap(mcp)
            .map(|m| m.into_inner())
            .ok();
        if let Some(c) = child {
            let _ = c.shutdown().await;
        }

        // 6. Aggregate.
        let aggregate_baseline = aggregate(&self.probe_cfg.model, &results);
        Ok((aggregate_baseline, per_probe))
    }
}

async fn build_backing_client(cfg: &LlmConfig) -> Result<BackingClient> {
    match cfg.provider {
        LlmProvider::Ollama => Ok(BackingClient::Ollama(OllamaClient::new(&cfg.base_url))),
        LlmProvider::Glm => {
            let key = cfg
                .api_key
                .clone()
                .context("glm provider requires ADR_GLM_API_KEY for the probe pass")?;
            Ok(BackingClient::Glm(GlmClient::new(&cfg.base_url, key)))
        }
    }
}

/// Write an artifact JSON to a temp file whose lifetime is bound by the
/// returned [`TempFile`] guard.
fn write_tmp_artifact(a: &Artifact) -> Result<PathBuf> {
    let dir = std::env::temp_dir();
    let fname = format!("adr-probe-{}.json", uuid::Uuid::new_v4());
    let path = dir.join(fname);
    let bytes = serde_json::to_vec(a)?;
    std::fs::write(&path, &bytes)
        .with_context(|| format!("writing side-only artifact to {}", path.display()))?;
    Ok(path)
}

struct TempFile(PathBuf);
impl TempFile {
    fn new(p: PathBuf) -> Self {
        Self(p)
    }
}
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Derive a per-model default [`LlmConfig`] for probe use. We reuse the
/// model-defaults table so the probe inherits the "generous max_tokens,
/// temperature ≈ 0.8" rule that synthesis already benefits from. The
/// shared [`LlmConfig::from_env_probe`] handles env overrides; this is
/// just a convenience for callers that want to inspect the effective
/// defaults.
pub fn effective_probe_config(cfg: &LlmConfig) -> model_defaults::ModelDefaults {
    model_defaults::defaults_for(cfg.provider, &cfg.model)
}

fn short(s: &str) -> String {
    s.chars().take(12).collect()
}

