//! Semantic hunk extractors. Scope 1: `call` · `state` · `api`.
//!
//! Every extractor takes base + head [`floe_core::Graph`] and returns zero or more
//! [`floe_core::Hunk`]. Identity keys project away graph-local NodeIds and EdgeIds
//! so base/head can be compared.

mod call;
mod api;
mod data;
mod deletion;
mod docs;
mod lock;
mod state;

pub use api::extract_api_hunks;
pub use call::extract_call_hunk;
pub use data::extract_data_hunks;
pub use deletion::extract_deletion_hunks;
pub use docs::extract_docs_hunks;
pub use lock::extract_lock_hunks;
pub use state::extract_state_hunks;

use floe_core::graph::Graph;
use floe_core::hunks::Hunk;

/// Run every scope-1 extractor and return the concatenated hunks.
pub fn extract_all(base: &Graph, head: &Graph) -> Vec<Hunk> {
    let mut out = Vec::new();
    if let Some(h) = extract_call_hunk(base, head) {
        out.push(h);
    }
    out.extend(extract_state_hunks(base, head));
    out.extend(extract_api_hunks(base, head));
    out
}
