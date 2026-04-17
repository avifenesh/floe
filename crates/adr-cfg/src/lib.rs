//! Per-function control-flow graph over tree-sitter-typescript ASTs.
//!
//! The CFG is not SSA and not path-sensitive; it's a cheap structural skeleton
//! that:
//! - names every branch, loop, try, throw, return, and async yield point;
//! - preserves topological order so the flow view can animate packet hops;
//! - costs next to nothing to build, so it can run per PR on every request.
//!
//! Precision gaps we accept: no exception flow tracking into callers, no
//! switch-case expansion (wrapped as Branch), no closure/lambda bodies (their
//! own Function nodes get their own CFGs when the parser emits them).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use adr_core::cfg::{Cfg, CfgEdge, CfgEntry, CfgMap, CfgNode, CfgNodeId, CfgNodeKind};
use adr_core::graph::{Graph, Node, NodeKind, Span};
use anyhow::{Context, Result};
use tree_sitter::{Node as TsNode, Parser};

/// Build a [`CfgMap`] covering every Function node in `graph`, re-parsing each
/// function's source file (graph spans reference byte ranges in the file bytes).
///
/// `root` is the workspace root the graph was ingested from.
pub fn build_for_graph(graph: &Graph, root: &Path) -> Result<CfgMap> {
    // Group functions by file so each file is parsed once.
    let mut by_file: HashMap<String, Vec<&Node>> = HashMap::new();
    for n in &graph.nodes {
        if matches!(n.kind, NodeKind::Function { .. }) {
            by_file.entry(n.file.clone()).or_default().push(n);
        }
    }
    let mut out: CfgMap = Vec::new();
    for (file_rel, fns) in by_file {
        let path = root.join(&file_rel);
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let tsx = file_rel.ends_with(".tsx");
        let mut parser = Parser::new();
        let lang = if tsx {
            tree_sitter_typescript::language_tsx()
        } else {
            tree_sitter_typescript::language_typescript()
        };
        parser
            .set_language(&lang)
            .context("load tree-sitter-typescript")?;
        let tree = parser
            .parse(&bytes, None)
            .with_context(|| format!("parse {file_rel}"))?;
        let root_node = tree.root_node();

        for fn_node in fns {
            if let Some(body_ts) = find_function_body(root_node, fn_node.span) {
                let cfg = Builder::new().build(body_ts);
                out.push(CfgEntry {
                    function: fn_node.id,
                    cfg,
                });
            }
        }
    }
    // Deterministic order for snapshot stability.
    out.sort_by_key(|e| e.function);
    Ok(out)
}

/// Locate the function-like node whose span matches the emitted Function's
/// byte range, then return its body (statement_block or expression body).
fn find_function_body(root: TsNode<'_>, span: Span) -> Option<TsNode<'_>> {
    fn walk<'a>(n: TsNode<'a>, span: Span, best: &mut Option<TsNode<'a>>) {
        if n.start_byte() as u32 == span.start && n.end_byte() as u32 == span.end {
            match n.kind() {
                "function_declaration"
                | "method_definition"
                | "arrow_function"
                | "function_expression" => {
                    if let Some(body) = n.child_by_field_name("body") {
                        *best = Some(body);
                        return;
                    }
                }
                "variable_declarator" => {
                    if let Some(value) = n.child_by_field_name("value") {
                        if let Some(body) = value.child_by_field_name("body") {
                            *best = Some(body);
                            return;
                        }
                    }
                }
                _ => {}
            }
        }
        let mut cursor = n.walk();
        for child in n.named_children(&mut cursor) {
            if best.is_some() {
                return;
            }
            walk(child, span, best);
        }
    }
    let mut best = None;
    walk(root, span, &mut best);
    best
}

struct Builder {
    nodes: Vec<CfgNode>,
    edges: Vec<CfgEdge>,
}

impl Builder {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    fn build(mut self, body: TsNode<'_>) -> Cfg {
        let entry = self.fresh(
            CfgNodeKind::Entry,
            Span {
                start: body.start_byte() as u32,
                end: body.start_byte() as u32,
            },
        );
        let exit = self.fresh(
            CfgNodeKind::Exit,
            Span {
                start: body.end_byte() as u32,
                end: body.end_byte() as u32,
            },
        );
        let end = self.walk_any(body, entry, exit);
        self.add_edge(end, exit);
        Cfg {
            nodes: self.nodes,
            edges: self.edges,
        }
    }

    fn fresh(&mut self, kind: CfgNodeKind, span: Span) -> CfgNodeId {
        let id = CfgNodeId(self.nodes.len() as u32);
        self.nodes.push(CfgNode { id, kind, span });
        id
    }

    fn add_edge(&mut self, from: CfgNodeId, to: CfgNodeId) {
        self.edges.push(CfgEdge { from, to });
    }

    fn walk_any(&mut self, node: TsNode<'_>, current: CfgNodeId, exit: CfgNodeId) -> CfgNodeId {
        if node.kind() == "statement_block" {
            self.walk_block(node, current, exit)
        } else {
            self.walk_stmt(node, current, exit)
        }
    }

    fn walk_block(&mut self, block: TsNode<'_>, mut current: CfgNodeId, exit: CfgNodeId) -> CfgNodeId {
        let mut cursor = block.walk();
        for stmt in block.named_children(&mut cursor) {
            current = self.walk_stmt(stmt, current, exit);
        }
        current
    }

    fn walk_stmt(&mut self, stmt: TsNode<'_>, current: CfgNodeId, exit: CfgNodeId) -> CfgNodeId {
        let span = Span {
            start: stmt.start_byte() as u32,
            end: stmt.end_byte() as u32,
        };
        match stmt.kind() {
            "if_statement" => {
                let branch = self.fresh(CfgNodeKind::Branch, span);
                self.add_edge(current, branch);
                let merge = self.fresh(CfgNodeKind::Seq, span);
                if let Some(cons) = stmt.child_by_field_name("consequence") {
                    let end = self.walk_any(cons, branch, exit);
                    self.add_edge(end, merge);
                }
                if let Some(alt) = stmt.child_by_field_name("alternative") {
                    let end = self.walk_any(alt, branch, exit);
                    self.add_edge(end, merge);
                } else {
                    self.add_edge(branch, merge);
                }
                merge
            }
            "for_statement"
            | "while_statement"
            | "do_statement"
            | "for_in_statement"
            | "for_of_statement" => {
                let loop_n = self.fresh(CfgNodeKind::Loop, span);
                self.add_edge(current, loop_n);
                if let Some(body) = stmt.child_by_field_name("body") {
                    let end = self.walk_any(body, loop_n, exit);
                    self.add_edge(end, loop_n);
                }
                loop_n
            }
            "try_statement" => {
                let try_n = self.fresh(CfgNodeKind::Try, span);
                self.add_edge(current, try_n);
                let body_end = stmt
                    .child_by_field_name("body")
                    .map(|b| self.walk_any(b, try_n, exit))
                    .unwrap_or(try_n);
                if let Some(handler) = stmt.child_by_field_name("handler") {
                    let h_end = self.walk_any(handler, try_n, exit);
                    let merge = self.fresh(CfgNodeKind::Seq, span);
                    self.add_edge(body_end, merge);
                    self.add_edge(h_end, merge);
                    merge
                } else {
                    body_end
                }
            }
            "return_statement" => {
                let r = self.fresh(CfgNodeKind::Return, span);
                self.add_edge(current, r);
                self.add_edge(r, exit);
                r
            }
            "throw_statement" => {
                let t = self.fresh(CfgNodeKind::Throw, span);
                self.add_edge(current, t);
                self.add_edge(t, exit);
                t
            }
            _ => {
                let kind = if contains_await(stmt) {
                    CfgNodeKind::AsyncBoundary
                } else {
                    CfgNodeKind::Seq
                };
                let n = self.fresh(kind, span);
                self.add_edge(current, n);
                n
            }
        }
    }
}

fn contains_await(n: TsNode<'_>) -> bool {
    if n.kind() == "await_expression" {
        return true;
    }
    let mut cursor = n.walk();
    for c in n.named_children(&mut cursor) {
        if contains_await(c) {
            return true;
        }
    }
    false
}
