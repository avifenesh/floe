use std::path::{Path, PathBuf};

use adr_core::Artifact;
use anyhow::{Context, Result};

/// Tool versions baked into the cache key. Bump any of these and all entries
/// silently invalidate.
const PIPELINE_VERSION: &str = "0.2.0";

pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        Ok(Self { dir })
    }

    pub fn key(&self, base: &Path, head: &Path) -> String {
        let mut h = blake3::Hasher::new();
        h.update(PIPELINE_VERSION.as_bytes());
        h.update(b"|");
        h.update(base.to_string_lossy().as_bytes());
        h.update(b"|");
        h.update(head.to_string_lossy().as_bytes());
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
}
