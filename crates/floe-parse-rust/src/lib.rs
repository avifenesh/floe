//! Rust source parser — mirrors `floe-parse` but emits `floe_core::Graph`
//! nodes from Rust sources via `tree-sitter-rust`. RFC Appendix F
//! Phase A (Rust side).
//!
//! # Scope (phase A)
//!
//! - Walk every `*.rs` under the given root (respecting `.gitignore`).
//! - Extract:
//!   - `fn NAME(...) -> RET { ... }` → `NodeKind::Function`
//!   - `struct NAME { ... }` / `struct NAME(...);` / `struct NAME;` → `NodeKind::Type`
//!   - `enum NAME { ... }` → `NodeKind::Type`
//!   - `type NAME = ...;` → `NodeKind::Type`
//!   - `trait NAME { ... }` → `NodeKind::Type`
//! - Record byte span + file path + provenance on each node.
//!
//! Out of scope for phase A: call graph (comes in phase D via
//! rust-analyzer), `impl` block associations, macro bodies.

use std::path::Path;

use floe_core::graph::{Graph, Node, NodeId, NodeKind, Span};
use floe_core::provenance::Provenance;
use ignore::WalkBuilder;
use tree_sitter::{Parser, Tree};

const SOURCE: &str = "floe-parse-rust";
const VERSION: &str = "0.1.0";

/// Parse every Rust source file under `root`, returning an
/// `floe_core::Graph` with Function / Type nodes. Edges are empty in
/// phase A — they arrive via the rust-analyzer pass later.
pub fn parse_root(root: &Path) -> anyhow::Result<Graph> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_rust::language())?;

    let mut graph = Graph::default();
    let mut next_id: u32 = 0;

    let walk = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .build();
    for entry in walk.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        extract_nodes(&tree, &source, &rel, &mut graph, &mut next_id);
    }
    Ok(graph)
}

fn extract_nodes(
    tree: &Tree,
    source: &str,
    file: &str,
    graph: &mut Graph,
    next_id: &mut u32,
) {
    walk_nodes(tree.root_node(), source, file, graph, next_id);
}

fn walk_nodes(
    node: tree_sitter::Node,
    source: &str,
    file: &str,
    graph: &mut Graph,
    next_id: &mut u32,
) {
    let kind = node.kind();
    let pushed = match kind {
        "function_item" => function_node(node, source, file, graph, next_id),
        "struct_item" | "enum_item" | "trait_item" | "type_item" => {
            type_node(node, source, file, graph, next_id)
        }
        _ => false,
    };
    let _ = pushed;
    // Descend — nested fns / types are real code worth indexing.
    let mut walker = node.walk();
    for child in node.children(&mut walker) {
        walk_nodes(child, source, file, graph, next_id);
    }
}

fn function_node(
    node: tree_sitter::Node,
    source: &str,
    file: &str,
    graph: &mut Graph,
    next_id: &mut u32,
) -> bool {
    let Some(name) = child_field_text(node, "name", source) else {
        return false;
    };
    let signature = extract_fn_signature(node, source);
    let id = NodeId(*next_id);
    *next_id += 1;
    graph.nodes.push(Node {
        id,
        kind: NodeKind::Function {
            name: name.to_string(),
            signature,
        },
        file: file.to_string(),
        span: Span {
            start: node.start_byte() as u32,
            end: node.end_byte() as u32,
        },
        provenance: Provenance::new(SOURCE, VERSION, "fn", name.as_bytes()),
        package: None,
    });
    true
}

fn type_node(
    node: tree_sitter::Node,
    source: &str,
    file: &str,
    graph: &mut Graph,
    next_id: &mut u32,
) -> bool {
    let Some(name) = child_field_text(node, "name", source) else {
        return false;
    };
    let id = NodeId(*next_id);
    *next_id += 1;
    graph.nodes.push(Node {
        id,
        kind: NodeKind::Type {
            name: name.to_string(),
        },
        file: file.to_string(),
        span: Span {
            start: node.start_byte() as u32,
            end: node.end_byte() as u32,
        },
        provenance: Provenance::new(SOURCE, VERSION, "type", name.as_bytes()),
        package: None,
    });
    true
}

fn child_field_text<'a>(
    node: tree_sitter::Node<'a>,
    field: &str,
    source: &'a str,
) -> Option<String> {
    let child = node.child_by_field_name(field)?;
    let s = &source[child.start_byte()..child.end_byte()];
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// Extract a one-line function signature (fn header up to the body).
/// Everything before the first `{` or `;`, trimmed. Keeps generics,
/// params, return type — same shape TS's signature strings carry.
fn extract_fn_signature(node: tree_sitter::Node, source: &str) -> String {
    let text = &source[node.start_byte()..node.end_byte()];
    let head_end = text.find('{').or_else(|| text.find(';')).unwrap_or(text.len());
    text[..head_end].trim().replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_function_and_struct() {
        let tmp = std::env::temp_dir().join(format!("floe-parse-rust-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("a.rs"),
            "pub struct Job { pub id: u64 }\npub fn run(job: Job) -> u64 { job.id }\n",
        )
        .unwrap();
        let graph = parse_root(&tmp).unwrap();
        assert!(graph.nodes.iter().any(|n| matches!(
            &n.kind,
            NodeKind::Function { name, .. } if name == "run"
        )));
        assert!(graph.nodes.iter().any(|n| matches!(
            &n.kind,
            NodeKind::Type { name } if name == "Job"
        )));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
