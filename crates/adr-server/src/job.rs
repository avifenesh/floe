use std::path::PathBuf;
use std::sync::Arc;

use adr_core::Artifact;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum JobStatus {
    Pending,
    Ready,
    Error { message: String },
}

/// A single progress event. Stages mirror the pipeline the worker walks:
/// `parse-base · parse-head · cfg · hunks · ready`. `percent` is coarse — 0..100
/// — so the UI skeleton can animate. `message` is human-readable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub stage: String,
    pub percent: u8,
    pub message: String,
}

#[derive(Debug)]
pub struct Job {
    pub id: uuid::Uuid,
    pub status: RwLock<JobStatus>,
    pub artifact: RwLock<Option<Artifact>>,
    /// Root directories for the two sides, canonicalized. Used by the file
    /// endpoint to serve source bytes with path-traversal protection.
    pub base_root: PathBuf,
    pub head_root: PathBuf,
    /// Broadcast channel — SSE subscribers receive every event fired after they
    /// subscribe. Events from before a subscription are not replayed (we emit
    /// a terminal `ready`/`error` event so a late subscriber still learns the
    /// outcome).
    pub progress: broadcast::Sender<ProgressEvent>,
}

impl Job {
    pub fn new(base_root: PathBuf, head_root: PathBuf) -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(64);
        Arc::new(Self {
            id: uuid::Uuid::new_v4(),
            status: RwLock::new(JobStatus::Pending),
            artifact: RwLock::new(None),
            base_root,
            head_root,
            progress: tx,
        })
    }

    /// Build a ready-state job pinned to an existing id + artifact.
    /// Used by the router when a cached artifact is loaded after a
    /// server restart so the file endpoint can still serve source
    /// bytes. Progress channel is empty — the work is long done.
    pub fn rehydrated(
        id: uuid::Uuid,
        base_root: PathBuf,
        head_root: PathBuf,
        artifact: Artifact,
    ) -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(64);
        Arc::new(Self {
            id,
            status: RwLock::new(JobStatus::Ready),
            artifact: RwLock::new(Some(artifact)),
            base_root,
            head_root,
            progress: tx,
        })
    }
}
