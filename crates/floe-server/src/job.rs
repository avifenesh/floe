use std::path::PathBuf;
use std::sync::Arc;

use floe_core::Artifact;
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
#[derive(Clone, serde::Serialize)]
pub struct TurnProgress {
    pub current: u32,
    pub max: u32,
    /// Unix-ms timestamp of last turn advance. Lets the UI show
    /// "stuck for Xs" when the turn doesn't advance within expected
    /// latency — distinguishes "still chugging" from "frozen".
    pub updated_at: u64,
}

#[derive(Debug, Default)]
pub struct TurnProgressMap(pub std::sync::RwLock<std::collections::HashMap<String, TurnProgress>>);

impl TurnProgressMap {
    /// Advance the named pass to `turn` of `max`. Stamps `updated_at`.
    /// Safe to call from any LLM-loop turn boundary.
    pub fn mark(&self, pass: &str, turn: u32, max: u32) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        if let Ok(mut g) = self.0.write() {
            g.insert(
                pass.to_string(),
                TurnProgress {
                    current: turn,
                    max,
                    updated_at: now,
                },
            );
        }
    }
    pub fn snapshot(&self) -> std::collections::HashMap<String, TurnProgress> {
        self.0.read().map(|g| g.clone()).unwrap_or_default()
    }
}

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
    /// Per-pass turn counters. Keys: `proof:<flow>`, `intent-fit:<flow>`,
    /// `membership:<flow>`, `probe:<probe>`, `synth`. The UI polls
    /// this to render a 0-100 progress bar per pass — each turn
    /// advances one decile; between turns the FE interpolates.
    /// `Arc` so spawned pipeline tasks can share the same live map
    /// as the job owner.
    pub turn_progress: Arc<TurnProgressMap>,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job")
            .field("id", &self.id)
            .field("base_root", &self.base_root)
            .field("head_root", &self.head_root)
            .finish()
    }
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
            turn_progress: Arc::new(TurnProgressMap::default()),
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
            turn_progress: Arc::new(TurnProgressMap::default()),
        })
    }
}
