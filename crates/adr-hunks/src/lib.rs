//! Semantic hunk extractors. Scope 1: `call` · `state` · `api`.
//!
//! Every extractor takes base + head [`adr_core::Graph`] and returns zero or more
//! [`adr_core::Hunk`]. Identity keys project away graph-local NodeIds and EdgeIds
//! so base/head can be compared.

mod call;
mod api;
mod state;

pub use api::extract_api_hunks;
pub use call::extract_call_hunk;
pub use state::extract_state_hunks;

use adr_core::graph::Graph;
use adr_core::hunks::Hunk;

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
