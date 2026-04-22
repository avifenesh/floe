use std::path::{Path, PathBuf};

use adr_core::{Artifact, SCHEMA_VERSION};
use anyhow::{Context, Result};

/// Pipeline revision baked into the cache key. Bump when the worker
/// pipeline changes in a way that's not reflected in `SCHEMA_VERSION`
/// (e.g. a pass's internal behaviour shifts but the artifact shape
/// stays the same).
///
/// `SCHEMA_VERSION` (from `adr_core`) is mixed into the key alongside
/// this, so a bump of either invalidates all cache entries and a dev
/// editing `artifact.rs` doesn't have to remember to bump cache too.
///
/// 0.5.2 (2026-04-22): `llm_signature` now carries the proof model
/// suffix so `ADR_PROOF_LLM` drift invalidates the entry (it used to
/// collide with synthesis-only signatures and serve stale proof
/// claims). Old entries become dead weight; they disappear on the
/// next run that builds a new key.
const PIPELINE_VERSION: &str = "0.5.2";

pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        Ok(Self { dir })
    }

    /// Cache root on disk. Also used as the parent for adjacent
    /// state like git checkouts (see `git_sync::repos_root`).
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Build the cache key from the content-addressed head snapshot
    /// plus the intent + LLM regime. We deliberately do **not** mix
    /// paths into the key — two users analysing the same PR head
    /// should share cache hits regardless of where the worktree
    /// lives. `head_sha` comes from [`adr_core::Artifact::snapshot_sha`]
    /// (blake3 over qualified-name + provenance-hash rows).
    ///
    /// `llm_signature` = `None` when LLM synthesis is disabled and
    /// `Some("<synth-provider>:<synth-model>@<prompt-version>+proof=<proof-provider>:<proof-model>|none")`
    /// when it's on. The proof suffix is load-bearing: without it, two
    /// runs with identical synthesis config but different
    /// `ADR_PROOF_LLM` would collide on the same cache entry and a
    /// cached artifact could serve stale proof claims. Changing any
    /// pin field invalidates the entry rather than silently serving a
    /// stale mix. Mirrors the RFC v0.3 §9 baseline pin at the cache
    /// layer — see `worker.rs` where the signature is composed.
    ///
    /// `intent_fingerprint` = blake3 over caller intent + notes;
    /// supplying different intent or notes changes the output so
    /// the key has to change.
    pub fn key(
        &self,
        head_sha: &str,
        llm_signature: Option<&str>,
        intent_fingerprint: &str,
    ) -> String {
        let mut h = blake3::Hasher::new();
        h.update(PIPELINE_VERSION.as_bytes());
        h.update(b"|");
        // Any Artifact-shape change bumps SCHEMA_VERSION (by convention
        // in adr-core/src/lib.rs); mixing it in here means we don't
        // need to remember to also bump PIPELINE_VERSION when editing
        // artifact.rs. Load-bearing — without it, an edit that adds a
        // #[serde(default)] field would silently deserialize old
        // cached JSONs with the default value.
        h.update(SCHEMA_VERSION.as_bytes());
        h.update(b"|");
        h.update(head_sha.as_bytes());
        h.update(b"|");
        h.update(llm_signature.unwrap_or("structural").as_bytes());
        h.update(b"|");
        h.update(intent_fingerprint.as_bytes());
        h.finalize().to_hex().to_string()
    }

    pub fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    pub fn get(&self, key: &str) -> Result<Option<Artifact>> {
        let p = self.path_for(key);
        if !p.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&p).with_context(|| format!("read {}", p.display()))?;
        let a: Artifact = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse cached artifact {}", p.display()))?;
        Ok(Some(a))
    }

    pub fn put(&self, key: &str, artifact: &Artifact) -> Result<()> {
        let p = self.path_for(key);
        let bytes = serde_json::to_vec_pretty(artifact)?;
        std::fs::write(&p, bytes).with_context(|| format!("write {}", p.display()))?;
        Ok(())
    }

    /// The on-disk root this cache writes to. Used by the probe pass to
    /// locate the sibling `baseline/` directory.
    pub fn root(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_cache() -> (Cache, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let c = Cache::new(dir.path()).unwrap();
        (c, dir)
    }

    #[test]
    fn head_sha_moves_the_key() {
        let (c, _tmp) = tmp_cache();
        let a = c.key("sha-a", Some("glm:glm-4.7@v0.3.1+proof=glm:glm-4.7"), "intent-fp");
        let b = c.key("sha-b", Some("glm:glm-4.7@v0.3.1+proof=glm:glm-4.7"), "intent-fp");
        assert_ne!(a, b);
    }

    #[test]
    fn llm_signature_moves_the_key() {
        let (c, _tmp) = tmp_cache();
        let a = c.key("sha", Some("glm:glm-4.7@v0.3.1+proof=glm:glm-4.7"), "fp");
        let b = c.key("sha", Some("ollama:qwen3.5:27b-q4_K_M@v0.3.1+proof=glm:glm-4.7"), "fp");
        assert_ne!(a, b, "synthesis model drift must invalidate cache");
    }

    #[test]
    fn proof_suffix_moves_the_key() {
        let (c, _tmp) = tmp_cache();
        let a = c.key("sha", Some("glm:glm-4.7@v0.3.1+proof=glm:glm-4.7"), "fp");
        let b = c.key("sha", Some("glm:glm-4.7@v0.3.1+proof=none"), "fp");
        assert_ne!(a, b, "proof model drift (or proof skipped) must invalidate cache");
    }

    #[test]
    fn intent_fingerprint_moves_the_key() {
        let (c, _tmp) = tmp_cache();
        let a = c.key("sha", Some("glm:glm-4.7@v0.3.1+proof=glm:glm-4.7"), "intent-a");
        let b = c.key("sha", Some("glm:glm-4.7@v0.3.1+proof=glm:glm-4.7"), "intent-b");
        assert_ne!(a, b);
    }

    #[test]
    fn structural_and_none_llm_map_to_same_key() {
        // Documented behaviour: `None` llm_signature → "structural". Two
        // structural runs on the same head + intent should cache-hit.
        let (c, _tmp) = tmp_cache();
        let a = c.key("sha", None, "fp");
        let b = c.key("sha", Some("structural"), "fp");
        assert_eq!(a, b);
    }

    #[test]
    fn same_inputs_produce_identical_keys() {
        let (c, _tmp) = tmp_cache();
        let a = c.key("sha", Some("glm:glm-4.7"), "fp");
        let b = c.key("sha", Some("glm:glm-4.7"), "fp");
        assert_eq!(a, b, "key must be deterministic");
    }
}
