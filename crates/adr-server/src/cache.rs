use std::path::{Path, PathBuf};

use adr_core::Artifact;
use anyhow::{Context, Result};

/// Tool versions baked into the cache key. Bump any of these and all entries
/// silently invalidate.
const PIPELINE_VERSION: &str = "0.5.1";

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
    /// `Some("<provider>:<model>@<prompt-version>")` when it's on —
    /// changing any of those invalidates the entry rather than
    /// silently serving stale LLM-flavoured results.
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
