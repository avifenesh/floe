//! Baseline on-disk layout.
//!
//! Local layout (self-hosted tester path):
//!
//! ```text
//! <root>/<repo_key>/<sha>/<probe_model>/
//!     probe-api-surface.json
//!     probe-external-boundaries.json
//!     probe-type-callsites.json
//!     aggregate.json
//! ```
//!
//! `<root>` defaults to `.adr/baseline` at the repo root; override via
//! [`BaselineStore::new_at`].
//!
//! Hosted / S3 backing (v1+, sketched in the plan doc) keeps the same
//! schema but swaps the filesystem for S3. The `BaselineStore` trait
//! boundary stays the same; only the impl changes.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};

use crate::aggregate::AggregateBaseline;
use crate::probes::ProbeId;
use crate::session::ProbeResult;

/// Default TTL before a baseline is considered stale and the probe
/// re-runs — even if the `(sha, model, version)` key still matches. Set
/// so a year-old cache doesn't silently drift against a rehosted model
/// that shares the tag but changed behaviour.
pub const DEFAULT_BASELINE_TTL: Duration = Duration::from_secs(60 * 24 * 60 * 60);

/// Composite key for a baseline: identifies one run of the probe set
/// against one snapshot of one repo under one model.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BaselineKey {
    /// Stable hash of the repo root's canonical path — NOT the repo
    /// contents. We want the same repo on the same machine to map to
    /// the same key.
    pub repo_key: String,
    pub sha: String,
    pub probe_model: String,
    pub probe_set_version: String,
}

impl BaselineKey {
    /// Build a repo key from a root path. Uses the canonical path's
    /// blake3 so the same repo from a different CWD still maps through.
    pub fn repo_key_for(root: &Path) -> String {
        let canon = root
            .canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| root.to_string_lossy().to_string());
        format!("repo-{}", blake3::hash(canon.as_bytes()).to_hex())
    }
}

/// What the store reports when asked whether a baseline exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselineStatus {
    /// Fresh baseline on disk, no need to re-probe.
    Fresh,
    /// Nothing for this key yet.
    Missing,
    /// On disk but stale (TTL expired). Caller should re-probe and
    /// overwrite.
    Stale,
}

pub struct BaselineStore {
    root: PathBuf,
    ttl: Duration,
}

impl BaselineStore {
    /// Store rooted at `<repo_root>/.adr/baseline`.
    pub fn at_repo_root(repo_root: &Path) -> Self {
        Self {
            root: repo_root.join(".adr").join("baseline"),
            ttl: DEFAULT_BASELINE_TTL,
        }
    }

    /// Store rooted at an explicit path. Useful for tests and for the
    /// hosted tier where the baseline directory sits outside the repo.
    pub fn new_at(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            ttl: DEFAULT_BASELINE_TTL,
        }
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    pub fn status(&self, key: &BaselineKey) -> Result<BaselineStatus> {
        let path = self.aggregate_path(key);
        if !path.exists() {
            return Ok(BaselineStatus::Missing);
        }
        let meta = std::fs::metadata(&path).with_context(|| format!("stat {}", path.display()))?;
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let age = SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default();
        Ok(if age > self.ttl {
            BaselineStatus::Stale
        } else {
            BaselineStatus::Fresh
        })
    }

    pub fn load(&self, key: &BaselineKey) -> Result<AggregateBaseline> {
        let path = self.aggregate_path(key);
        let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let agg: AggregateBaseline = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse aggregate at {}", path.display()))?;
        Ok(agg)
    }

    /// Write an aggregate baseline + each per-probe result. Overwrites
    /// any existing files under the key.
    pub fn save(
        &self,
        key: &BaselineKey,
        aggregate: &AggregateBaseline,
        probes: &[(ProbeId, ProbeResult)],
    ) -> Result<()> {
        let dir = self.key_dir(key);
        std::fs::create_dir_all(&dir).with_context(|| format!("mkdir {}", dir.display()))?;
        for (id, r) in probes {
            let p = dir.join(format!("{}.json", id.as_str()));
            let bytes = serde_json::to_vec_pretty(r)?;
            std::fs::write(&p, bytes).with_context(|| format!("write {}", p.display()))?;
        }
        let agg_path = self.aggregate_path(key);
        std::fs::write(&agg_path, serde_json::to_vec_pretty(aggregate)?)
            .with_context(|| format!("write {}", agg_path.display()))?;
        Ok(())
    }

    fn key_dir(&self, key: &BaselineKey) -> PathBuf {
        // Include probe_set_version in the path so bumping it cleanly
        // coexists with the prior version's files.
        self.root
            .join(&key.repo_key)
            .join(&key.sha)
            .join(sanitize(&key.probe_model))
            .join(&key.probe_set_version)
    }

    fn aggregate_path(&self, key: &BaselineKey) -> PathBuf {
        self.key_dir(key).join("aggregate.json")
    }
}

/// Model tags contain `:` (`qwen3.5:27b-q4_K_M`) which is invalid in
/// filenames on Windows. Substitute to `__` for the directory name.
fn sanitize(s: &str) -> String {
    s.replace([':', '/', '\\'], "__")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::{AggregateTotals, EntityCost};
    use crate::probes::PROBE_SET_VERSION;

    fn key(root: &Path, sha: &str) -> BaselineKey {
        BaselineKey {
            repo_key: BaselineKey::repo_key_for(root),
            sha: sha.into(),
            probe_model: "qwen3.5:27b-q4_K_M".into(),
            probe_set_version: PROBE_SET_VERSION.into(),
        }
    }

    fn empty_agg() -> AggregateBaseline {
        AggregateBaseline {
            schema_version: "0.1.0".into(),
            probe_set_version: PROBE_SET_VERSION.into(),
            probe_model: "qwen3.5:27b-q4_K_M".into(),
            per_entity: std::collections::HashMap::from([(
                "A.b".into(),
                EntityCost {
                    visits: 3,
                    tokens: 120,
                    sessions_present: 2,
                    cost: 7.12,
                },
            )]),
            per_probe_entity_cost: std::collections::HashMap::new(),
            per_probe: std::collections::HashMap::new(),
            totals: AggregateTotals {
                entities: 1,
                tokens: 120,
                tool_calls: 3,
                turns: 2,
                duration_ms: 1000,
            },
        }
    }

    #[test]
    fn save_then_load_roundtrip() {
        // Use a unique subdir under the OS temp dir so parallel tests
        // don't collide. No `tempdir` crate dependency needed.
        let root = std::env::temp_dir().join(format!(
            "adr-probe-baseline-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = BaselineStore::new_at(root.clone());
        let k = key(&root, "abcdef");
        assert_eq!(store.status(&k).unwrap(), BaselineStatus::Missing);
        store.save(&k, &empty_agg(), &[]).unwrap();
        assert_eq!(store.status(&k).unwrap(), BaselineStatus::Fresh);
        let loaded = store.load(&k).unwrap();
        assert_eq!(loaded.per_entity.get("A.b").unwrap().visits, 3);
        let _ = std::fs::remove_dir_all(&root);
    }
}
