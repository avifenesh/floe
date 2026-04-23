//! Graph enrichment: replace tree-sitter's syntactic call edges with
//! LSP-derived semantic ones.
//!
//! Input: a `Graph` produced by `floe-parse` + the workspace root.
//! Output: the same graph with `EdgeKind::Calls` edges rewritten from
//! `textDocument/prepareCallHierarchy` + `callHierarchy/outgoingCalls`.
//! All other edges (defines, exports, transitions) are preserved.
//!
//! The tree-sitter floor stays as-is; if the LSP session fails (binary
//! missing, init timeout, unrecoverable protocol error) we return the
//! caller's original graph unchanged and log the failure. Downstream
//! passes that were operating on tree-sitter edges keep working.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use floe_core::graph::{Edge, EdgeId, EdgeKind, Graph, NodeId, NodeKind};
use floe_core::provenance::Provenance;
use anyhow::Result;

use crate::TsLspClient;

/// Drive the LSP pass against `graph` rooted at `workspace_root`.
///
/// Best-effort: individual per-node failures (no hit on
/// prepareCallHierarchy, file that tsserver refuses to open) are
/// swallowed and logged; the function still returns a usable graph.
/// Fatal LSP failures (binary missing, init error) return the input
/// graph unchanged with a warning so the caller's pipeline can
/// continue on tree-sitter edges.
pub async fn enrich_graph(graph: Graph, workspace_root: &Path) -> Graph {
    match enrich_graph_fallible(graph.clone(), workspace_root).await {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "floe-lsp enrichment failed — falling back to tree-sitter call edges"
            );
            graph
        }
    }
}

async fn enrich_graph_fallible(mut graph: Graph, workspace_root: &Path) -> Result<Graph> {
    let mut client = TsLspClient::start(workspace_root).await?;

    // Group function / method nodes by file so we can read each file
    // once and open it once on the LSP side. State / type / api /
    // file nodes aren't callable and skip this pass.
    let mut by_file: HashMap<String, Vec<NodeId>> = HashMap::new();
    for node in &graph.nodes {
        if matches!(node.kind, NodeKind::Function { .. }) {
            by_file.entry(node.file.clone()).or_default().push(node.id);
        }
    }

    // Cache file contents per path — one read each.
    let mut file_cache: HashMap<PathBuf, String> = HashMap::new();

    // Walk the file list in a stable order for reproducibility.
    let mut files: Vec<&String> = by_file.keys().collect();
    files.sort();

    // Accumulate the new call edges; we replace every EdgeKind::Calls
    // in one shot at the end.
    let mut new_calls: Vec<(NodeId, NodeId)> = Vec::new();

    for rel_path in files {
        let abs_path = workspace_root.join(rel_path);
        let text = match file_cache.get(&abs_path) {
            Some(t) => t.clone(),
            None => match std::fs::read_to_string(&abs_path) {
                Ok(t) => {
                    file_cache.insert(abs_path.clone(), t.clone());
                    t
                }
                Err(e) => {
                    tracing::debug!(path = %abs_path.display(), error = %e, "skip file, unreadable");
                    continue;
                }
            },
        };
        if let Err(e) = client.open_file(&abs_path, &text).await {
            tracing::debug!(path = %abs_path.display(), error = %e, "skip file, open failed");
            continue;
        }

        let node_ids = by_file.get(rel_path).cloned().unwrap_or_default();
        for node_id in node_ids {
            // Re-fetch the node (stable indexing isn't guaranteed; use id lookup).
            let Some(node) = graph.nodes.iter().find(|n| n.id == node_id) else {
                continue;
            };
            let name = match &node.kind {
                NodeKind::Function { name, .. } => name.clone(),
                _ => continue,
            };
            // Strip a class prefix if present — tree-sitter emits
            // `Class.method` but the LSP sees just `method` in the
            // source. Fall back to the full name if there's no dot.
            let short_name = name.rsplit_once('.').map(|(_, s)| s).unwrap_or(&name);
            let Some((line, character)) =
                find_name_position(&text, short_name, node.span.start as usize, node.span.end as usize)
            else {
                continue;
            };
            let items = match client.prepare_call_hierarchy(&abs_path, line, character).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::debug!(file = %rel_path, name = %short_name, error = %e, "prepare_call_hierarchy failed");
                    continue;
                }
            };
            if items.is_empty() {
                continue;
            }
            let outgoing = match client.outgoing_calls(&items[0]).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::debug!(name = %short_name, error = %e, "outgoing_calls failed");
                    continue;
                }
            };
            for out in outgoing {
                // Map the target file + name back to a node in the graph.
                let target_path = uri_to_rel(&out.to.uri, workspace_root);
                let target_name = &out.to.name;
                if let Some(target_id) = find_function_node(&graph, target_path.as_deref(), target_name) {
                    if target_id != node_id {
                        new_calls.push((node_id, target_id));
                    }
                }
            }
        }
    }

    // Replace every EdgeKind::Calls in the original graph with the
    // LSP-derived set. Defines / Exports / Transitions stay.
    graph.edges.retain(|e| !matches!(e.kind, EdgeKind::Calls));
    let mut next_edge_id = graph
        .edges
        .iter()
        .map(|e| e.id.0)
        .max()
        .map(|m| m + 1)
        .unwrap_or(0);
    let provenance = Provenance {
        source: "floe-lsp".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        pass_id: "lsp-call-hierarchy".into(),
        hash: String::new(),
    };
    // Dedupe (from, to) pairs — LSP sometimes reports the same edge
    // twice if a call appears in both the receiver context and a
    // propagation context.
    let mut seen = std::collections::HashSet::new();
    for (from, to) in new_calls {
        if !seen.insert((from, to)) {
            continue;
        }
        graph.edges.push(Edge {
            id: EdgeId(next_edge_id),
            from,
            to,
            kind: EdgeKind::Calls,
            provenance: provenance.clone(),
        });
        next_edge_id += 1;
    }

    client.shutdown().await?;
    Ok(graph)
}

/// Find (line, utf16_col) of `name` inside `text` between `start..end`
/// byte offsets. Returns None if the name isn't found in that range.
fn find_name_position(
    text: &str,
    name: &str,
    start: usize,
    end: usize,
) -> Option<(u32, u32)> {
    let end = end.min(text.len());
    let start = start.min(end);
    let slice = text.get(start..end)?;
    let rel_offset = slice.find(name)?;
    let abs_offset = start + rel_offset;
    // Count newlines up to abs_offset to get the line; character is
    // the UTF-16 column of the identifier start on that line.
    let mut line: u32 = 0;
    let mut line_start_byte: usize = 0;
    for (i, b) in text.as_bytes().iter().enumerate().take(abs_offset) {
        if *b == b'\n' {
            line += 1;
            line_start_byte = i + 1;
        }
    }
    let col_bytes = &text[line_start_byte..abs_offset];
    let character: u32 = col_bytes.encode_utf16().count() as u32;
    Some((line, character))
}

/// Map an LSP file URI back to a graph-relative path string (the same
/// format `floe-parse` stores in `Node.file`). Returns None if the URI
/// isn't a file under `root`.
fn uri_to_rel(uri: &async_lsp::lsp_types::Url, root: &Path) -> Option<String> {
    let path = uri.to_file_path().ok()?;
    let canonical_root = root.canonicalize().ok()?;
    let canonical_path = path.canonicalize().ok()?;
    let rel = canonical_path.strip_prefix(&canonical_root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// Resolve an LSP outgoing-call target (file + name) to a graph node.
/// Picks the first Function node on the target file whose `name`
/// ends with `target_name` (handles both bare-name LSP returns and
/// tree-sitter's `Class.method` form).
fn find_function_node(graph: &Graph, target_path: Option<&str>, target_name: &str) -> Option<NodeId> {
    let path = target_path?;
    for n in &graph.nodes {
        if n.file != path {
            continue;
        }
        let NodeKind::Function { name, .. } = &n.kind else {
            continue;
        };
        let short = name.rsplit_once('.').map(|(_, s)| s).unwrap_or(name);
        if short == target_name {
            return Some(n.id);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_name_position_computes_line_col() {
        let text = "function foo() {}\nfunction bar() {\n  foo();\n}\n";
        // Function `bar` — between bytes 18..43 approximately.
        let (line, col) = find_name_position(text, "bar", 18, 43).unwrap();
        assert_eq!(line, 1);
        // "function " is 9 UTF-16 units.
        assert_eq!(col, 9);
    }

    #[test]
    fn find_name_position_misses_outside_range() {
        let text = "function foo() {}\n";
        assert!(find_name_position(text, "foo", 100, 200).is_none());
    }
}
