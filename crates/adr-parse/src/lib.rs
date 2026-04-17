//! TypeScript parsing → `adr-core` graph.
//!
//! Day-2: walk a directory, parse each `.ts` / `.tsx`, emit `File` + top-level
//! `Function` / `Type` nodes with `Defines` edges.
//! Day-3: second pass per file resolving `call_expression` → `Calls` edge when the
//! callee is a bare identifier bound to a same-file function. Cross-file + method
//! call resolution lands with scip-typescript.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use adr_core::graph::{Edge, EdgeId, EdgeKind, Graph, Node, NodeId, NodeKind, Span};
use adr_core::provenance::Provenance;
use anyhow::{Context, Result};
use tree_sitter::{Node as TsNode, Parser};
use walkdir::WalkDir;

const PARSE_SOURCE: &str = "tree-sitter-typescript";
const PARSE_VERSION: &str = "0.21.2";

pub struct Ingest {
    graph: Graph,
    next_node: u32,
    next_edge: u32,
    pass_id: String,
}

/// Function definition captured during phase 1 so phase 2 can resolve its
/// callsites without re-walking the whole tree.
struct FnDef<'tree> {
    id: NodeId,
    name: String,
    body: TsNode<'tree>,
}

impl Ingest {
    pub fn new(pass_id: impl Into<String>) -> Self {
        Self {
            graph: Graph::default(),
            next_node: 0,
            next_edge: 0,
            pass_id: pass_id.into(),
        }
    }

    pub fn ingest_dir(mut self, root: &Path) -> Result<Graph> {
        let mut paths: Vec<PathBuf> = WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let n = e.file_name().to_string_lossy();
                !(n == "node_modules" || n == "dist" || n == "target" || n == ".git")
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .filter(|p| matches!(p.extension().and_then(|x| x.to_str()), Some("ts") | Some("tsx")))
            .collect();
        paths.sort();

        for path in paths {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
            self.ingest_file(&rel, &bytes)?;
        }
        Ok(self.graph)
    }

    fn fresh_node(&mut self) -> NodeId {
        let id = NodeId(self.next_node);
        self.next_node += 1;
        id
    }

    fn fresh_edge(&mut self) -> EdgeId {
        let id = EdgeId(self.next_edge);
        self.next_edge += 1;
        id
    }

    fn prov(&self, bytes: &[u8]) -> Provenance {
        Provenance::new(PARSE_SOURCE, PARSE_VERSION, &self.pass_id, bytes)
    }

    fn ingest_file(&mut self, rel_path: &str, source: &[u8]) -> Result<()> {
        let tsx = rel_path.ends_with(".tsx");
        let mut parser = Parser::new();
        let lang = if tsx {
            tree_sitter_typescript::language_tsx()
        } else {
            tree_sitter_typescript::language_typescript()
        };
        parser
            .set_language(&lang)
            .context("load tree-sitter-typescript grammar")?;
        let tree = parser
            .parse(source, None)
            .with_context(|| format!("parse {rel_path}"))?;

        let file_id = self.fresh_node();
        let file_prov = self.prov(source);
        self.graph.nodes.push(Node {
            id: file_id,
            kind: NodeKind::File {
                path: rel_path.to_string(),
            },
            file: rel_path.to_string(),
            span: Span {
                start: 0,
                end: source.len() as u32,
            },
            provenance: file_prov,
        });

        // Phase 1: emit top-level defs, remember function bodies for phase 2.
        let mut fns: Vec<FnDef<'_>> = Vec::new();
        let mut cursor = tree.root_node().walk();
        let root = tree.root_node();
        for child in root.named_children(&mut cursor) {
            self.visit_top_level(child, source, rel_path, file_id, &mut fns);
        }

        // Phase 2: within-file callsite resolution. Callee names resolve against
        // other functions in the same file only; cross-file awaits scip.
        let name_to_id: HashMap<&str, NodeId> =
            fns.iter().map(|f| (f.name.as_str(), f.id)).collect();
        for f in &fns {
            self.resolve_calls_in(f.id, f.body, source, &name_to_id);
        }

        Ok(())
    }

    fn visit_top_level<'tree>(
        &mut self,
        node: TsNode<'tree>,
        source: &[u8],
        rel_path: &str,
        file_id: NodeId,
        fns: &mut Vec<FnDef<'tree>>,
    ) {
        let effective = if node.kind() == "export_statement" {
            node.child_by_field_name("declaration").unwrap_or(node)
        } else {
            node
        };
        match effective.kind() {
            "function_declaration" => {
                if let Some(name) = field_text(effective, "name", source) {
                    let sig = first_line(effective, source);
                    let id = self.emit_definition(
                        NodeKind::Function {
                            name: name.clone(),
                            signature: sig,
                        },
                        effective,
                        source,
                        rel_path,
                        file_id,
                    );
                    fns.push(FnDef {
                        id,
                        name,
                        body: effective,
                    });
                }
            }
            "class_declaration" | "interface_declaration" => {
                if let Some(name) = field_text(effective, "name", source) {
                    self.emit_definition(
                        NodeKind::Type { name },
                        effective,
                        source,
                        rel_path,
                        file_id,
                    );
                }
            }
            "type_alias_declaration" => {
                if let Some(name) = field_text(effective, "name", source) {
                    self.emit_definition(
                        NodeKind::Type { name },
                        effective,
                        source,
                        rel_path,
                        file_id,
                    );
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                let mut c = effective.walk();
                for decl in effective.named_children(&mut c) {
                    if decl.kind() != "variable_declarator" {
                        continue;
                    }
                    let Some(value) = decl.child_by_field_name("value") else {
                        continue;
                    };
                    if !matches!(value.kind(), "arrow_function" | "function_expression") {
                        continue;
                    }
                    let Some(name) = field_text(decl, "name", source) else {
                        continue;
                    };
                    let sig = first_line(decl, source);
                    let id = self.emit_definition(
                        NodeKind::Function {
                            name: name.clone(),
                            signature: sig,
                        },
                        decl,
                        source,
                        rel_path,
                        file_id,
                    );
                    fns.push(FnDef {
                        id,
                        name,
                        body: value,
                    });
                }
            }
            _ => {}
        }
    }

    fn emit_definition(
        &mut self,
        kind: NodeKind,
        node: TsNode<'_>,
        source: &[u8],
        rel_path: &str,
        file_id: NodeId,
    ) -> NodeId {
        let span = Span {
            start: node.start_byte() as u32,
            end: node.end_byte() as u32,
        };
        let slice = &source[node.start_byte()..node.end_byte()];
        let prov = self.prov(slice);

        let def_id = self.fresh_node();
        self.graph.nodes.push(Node {
            id: def_id,
            kind,
            file: rel_path.to_string(),
            span,
            provenance: prov.clone(),
        });

        let edge_id = self.fresh_edge();
        self.graph.edges.push(Edge {
            id: edge_id,
            from: file_id,
            to: def_id,
            kind: EdgeKind::Defines,
            provenance: prov,
        });
        def_id
    }

    fn resolve_calls_in(
        &mut self,
        caller: NodeId,
        body: TsNode<'_>,
        source: &[u8],
        name_to_id: &HashMap<&str, NodeId>,
    ) {
        walk_call_expressions(body, &mut |call| {
            let Some(fn_field) = call.child_by_field_name("function") else {
                return;
            };
            if fn_field.kind() != "identifier" {
                return;
            }
            let Ok(name) = fn_field.utf8_text(source) else {
                return;
            };
            if let Some(&callee) = name_to_id.get(name) {
                if callee == caller {
                    return;
                }
                let slice = &source[call.start_byte()..call.end_byte()];
                let prov = self.prov(slice);
                let id = self.fresh_edge();
                self.graph.edges.push(Edge {
                    id,
                    from: caller,
                    to: callee,
                    kind: EdgeKind::Calls,
                    provenance: prov,
                });
            }
        });
    }
}

fn walk_call_expressions<'tree>(node: TsNode<'tree>, f: &mut dyn FnMut(TsNode<'tree>)) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "call_expression" {
            f(child);
        }
        walk_call_expressions(child, f);
    }
}

fn field_text(node: TsNode<'_>, field: &str, source: &[u8]) -> Option<String> {
    node.child_by_field_name(field)
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())
}

fn first_line(node: TsNode<'_>, source: &[u8]) -> String {
    node.utf8_text(source)
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}
