//! Orchestrator HTTP surface. Three endpoints:
//!
//! - `POST /analyze` · body `{ base_path, head_path }` → `{ job_id }`
//! - `GET /analyze/:id` · `{ status, artifact? }`
//! - `GET /analyze/:id/stream` · Server-Sent Events, one line per stage
//!
//! State is in-memory (jobs keyed by UUID). Finished artifacts persist to
//! `<cache_dir>/<blake3(base_path,head_path,tool-versions)>.json` so identical
//! re-analysis returns instantly. A finished job remains retrievable until the
//! server restarts; the cache file survives across restarts.

pub mod cache;
pub mod job;
pub mod router;
pub mod worker;

pub use router::{build_router, AppState};
