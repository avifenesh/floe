//! TypeScript parsing → `adr-core` graph.
//!
//! Scope 1 coverage:
//! - `File` · `Function` · `Type` · `State` nodes from top-level declarations
//! - `Defines` edge from file → def for every emitted node
//! - `Exports` edge when the declaration is wrapped in `export_statement`
//! - `Calls` edges for within-file, bare-identifier callsites
//!
//! Cross-file call + method resolution awaits scip-typescript.

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
    /// Per-file map of exported Function name -> NodeId, populated while walking
    /// top-level defs. Used during the cross-file resolution pass.
    exports_by_file: HashMap<String, HashMap<String, NodeId>>,
    /// Per-file import bindings. `local_name` is what appears in call sites
    /// inside this file; `source_file` is the resolved absolute relative path
    /// (e.g. "src/queue.ts"); `imported_name` is the export's name at the source.
    imports_by_file: HashMap<String, Vec<ImportBinding>>,
    /// Call sites that didn't resolve to a same-file function. The cross-file
    /// pass walks these once every file is parsed.
    pending_calls: Vec<PendingCall>,
}

struct FnDef<'tree> {
    id: NodeId,
    name: String,
    body: TsNode<'tree>,
}

#[derive(Debug, Clone)]
struct ImportBinding {
    local_name: String,
    imported_name: String,
    source_file: String,
}

#[derive(Debug, Clone)]
struct PendingCall {
    caller: NodeId,
    caller_file: String,
    callee_name: String,
    provenance: Provenance,
}

impl Ingest {
    pub fn new(pass_id: impl Into<String>) -> Self {
        Self {
            graph: Graph::default(),
            next_node: 0,
            next_edge: 0,
            pass_id: pass_id.into(),
            exports_by_file: HashMap::new(),
            imports_by_file: HashMap::new(),
            pending_calls: Vec::new(),
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
            .filter(|p| {
                matches!(
                    p.extension().and_then(|x| x.to_str()),
                    Some("ts") | Some("tsx")
                )
            })
            .collect();
        paths.sort();

        let rels: Vec<String> = paths
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        let all_files: Vec<String> = rels.clone();
        for (path, rel) in paths.iter().zip(rels.iter()) {
            let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
            self.ingest_file(rel, &bytes, &all_files)?;
        }
        self.resolve_cross_file_calls();
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

    fn ingest_file(&mut self, rel_path: &str, source: &[u8], all_files: &[String]) -> Result<()> {
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

        let mut fns: Vec<FnDef<'_>> = Vec::new();
        let mut cursor = tree.root_node().walk();
        let root = tree.root_node();
        for child in root.named_children(&mut cursor) {
            match child.kind() {
                "import_statement" => {
                    self.collect_imports(child, source, rel_path, all_files);
                }
                _ => self.visit_top_level(child, source, rel_path, file_id, &mut fns),
            }
        }

        let name_to_id: HashMap<&str, NodeId> =
            fns.iter().map(|f| (f.name.as_str(), f.id)).collect();
        for f in &fns {
            self.resolve_calls_in(f.id, &f.name, f.body, source, rel_path, &name_to_id);
        }

        // Phase 3: classical state-machine transition detection. For every
        // State node in this file, scan function bodies for the idiom
        // `if (x === "a") return "b";` / `if (x === "a") x = "b";` where "a"
        // and "b" are declared variants. Emit Transitions edges (self-loop on
        // the State node) — the variant strings live in EdgeKind::Transitions.
        let states: Vec<(NodeId, Vec<String>)> = self
            .graph
            .nodes
            .iter()
            .filter(|n| n.file == rel_path)
            .filter_map(|n| match &n.kind {
                NodeKind::State { variants, .. } => Some((n.id, variants.clone())),
                _ => None,
            })
            .collect();
        if !states.is_empty() {
            for f in &fns {
                for (state_id, variants) in &states {
                    self.resolve_transitions_in(*state_id, variants, f.body, source);
                }
            }
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
        let (effective, exported) = if node.kind() == "export_statement" {
            (
                node.child_by_field_name("declaration").unwrap_or(node),
                true,
            )
        } else {
            (node, false)
        };
        match effective.kind() {
            "function_declaration" => {
                if let Some(name) = field_text(effective, "name", source) {
                    let sig = first_line(effective, source);
                    let id = self.emit_def(
                        NodeKind::Function {
                            name: name.clone(),
                            signature: sig,
                        },
                        effective,
                        source,
                        rel_path,
                        file_id,
                        exported,
                    );
                    fns.push(FnDef {
                        id,
                        name,
                        body: effective,
                    });
                }
            }
            "class_declaration" | "interface_declaration" => {
                if let Some(class_name) = field_text(effective, "name", source) {
                    self.emit_def(
                        NodeKind::Type {
                            name: class_name.clone(),
                        },
                        effective,
                        source,
                        rel_path,
                        file_id,
                        exported,
                    );
                    // Walk the class body for methods. Each method becomes a
                    // Function node named `ClassName.methodName` so hunk
                    // extractors can key by qualified name across sides.
                    if let Some(body) = effective.child_by_field_name("body") {
                        let mut bc = body.walk();
                        for member in body.named_children(&mut bc) {
                            match member.kind() {
                                "method_definition" | "method_signature" => {
                                    let Some(method_name) = field_text(member, "name", source)
                                    else {
                                        continue;
                                    };
                                    let qualified = format!("{class_name}.{method_name}");
                                    let sig = first_line(member, source);
                                    let id = self.emit_def(
                                        NodeKind::Function {
                                            name: qualified.clone(),
                                            signature: sig,
                                        },
                                        member,
                                        source,
                                        rel_path,
                                        file_id,
                                        // Methods inherit the class's export
                                        // status — re-exporting a class
                                        // exposes all public methods.
                                        exported,
                                    );
                                    // Include method bodies in the callsite
                                    // scan so calls inside methods contribute
                                    // to the call graph.
                                    if let Some(m_body) = member.child_by_field_name("body") {
                                        fns.push(FnDef {
                                            id,
                                            name: qualified,
                                            body: m_body,
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            "type_alias_declaration" => {
                if let Some(name) = field_text(effective, "name", source) {
                    let kind = match string_union_variants(effective, source) {
                        Some(variants) => NodeKind::State { name, variants },
                        None => NodeKind::Type { name },
                    };
                    self.emit_def(kind, effective, source, rel_path, file_id, exported);
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
                    let id = self.emit_def(
                        NodeKind::Function {
                            name: name.clone(),
                            signature: sig,
                        },
                        decl,
                        source,
                        rel_path,
                        file_id,
                        exported,
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

    fn emit_def(
        &mut self,
        kind: NodeKind,
        node: TsNode<'_>,
        source: &[u8],
        rel_path: &str,
        file_id: NodeId,
        exported: bool,
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
            provenance: prov.clone(),
        });

        if exported {
            let eid = self.fresh_edge();
            self.graph.edges.push(Edge {
                id: eid,
                from: file_id,
                to: def_id,
                kind: EdgeKind::Exports,
                provenance: prov,
            });
            if let NodeKind::Function { name, .. } = &self.graph.nodes[def_id.0 as usize].kind {
                self.exports_by_file
                    .entry(rel_path.to_string())
                    .or_default()
                    .insert(name.clone(), def_id);
            }
        }
        def_id
    }

    fn resolve_calls_in(
        &mut self,
        caller: NodeId,
        caller_name: &str,
        body: TsNode<'_>,
        source: &[u8],
        rel_path: &str,
        name_to_id: &HashMap<&str, NodeId>,
    ) {
        // If caller is a class method "Foo.bar", peel out "Foo" so we can
        // resolve `this.baz()` back to `Foo.baz` inside the same class.
        let enclosing_class: Option<&str> = caller_name.split_once('.').map(|(c, _)| c);

        let mut local_edges: Vec<(NodeId, Provenance)> = Vec::new();
        let mut cross_file: Vec<PendingCall> = Vec::new();
        walk_call_expressions(body, &mut |call| {
            let Some(fn_field) = call.child_by_field_name("function") else {
                return;
            };
            let resolved_name: Option<String> = match fn_field.kind() {
                "identifier" => fn_field.utf8_text(source).ok().map(ToString::to_string),
                "member_expression" => {
                    // Only `this.foo()` — other member calls (o.x, foo.bar.baz)
                    // require type info we don't have yet.
                    let object = fn_field.child_by_field_name("object");
                    let property = fn_field.child_by_field_name("property");
                    match (object.map(|o| o.kind()), property, enclosing_class) {
                        (Some("this"), Some(p), Some(cls))
                            if p.kind() == "property_identifier" =>
                        {
                            p.utf8_text(source).ok().map(|prop| format!("{cls}.{prop}"))
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            let Some(name) = resolved_name else {
                return;
            };
            let slice = &source[call.start_byte()..call.end_byte()];
            let prov = Provenance::new(PARSE_SOURCE, PARSE_VERSION, &self.pass_id, slice);
            if let Some(&callee) = name_to_id.get(name.as_str()) {
                if callee == caller {
                    return;
                }
                local_edges.push((callee, prov));
            } else {
                cross_file.push(PendingCall {
                    caller,
                    caller_file: rel_path.to_string(),
                    callee_name: name,
                    provenance: prov,
                });
            }
        });
        for (callee, prov) in local_edges {
            let id = self.fresh_edge();
            self.graph.edges.push(Edge {
                id,
                from: caller,
                to: callee,
                kind: EdgeKind::Calls,
                provenance: prov,
            });
        }
        self.pending_calls.extend(cross_file);
    }

    /// Parse named-imports (`import { a, b as c } from "./x"`) and record
    /// bindings keyed by the name used locally in this file. Default + namespace
    /// + side-effect imports are ignored — cross-file Calls edges only fire for
    ///   named function imports in v0.
    fn collect_imports(
        &mut self,
        node: TsNode<'_>,
        source: &[u8],
        rel_path: &str,
        all_files: &[String],
    ) {
        let Some(source_node) = node.child_by_field_name("source") else {
            return;
        };
        let Some(module) = string_literal_text(source_node, source) else {
            return;
        };
        let Some(resolved) = resolve_relative_module(&module, rel_path, all_files) else {
            return;
        };
        let Some(import_clause) = node
            .named_children(&mut node.walk())
            .find(|c| c.kind() == "import_clause")
        else {
            return;
        };
        let mut cursor = import_clause.walk();
        for child in import_clause.named_children(&mut cursor) {
            if child.kind() != "named_imports" {
                continue;
            }
            let mut c2 = child.walk();
            for specifier in child.named_children(&mut c2) {
                if specifier.kind() != "import_specifier" {
                    continue;
                }
                let name_node = specifier.child_by_field_name("name");
                let alias_node = specifier.child_by_field_name("alias");
                let imported = name_node.and_then(|n| n.utf8_text(source).ok());
                let local = alias_node
                    .or(name_node)
                    .and_then(|n| n.utf8_text(source).ok());
                if let (Some(imported), Some(local)) = (imported, local) {
                    self.imports_by_file
                        .entry(rel_path.to_string())
                        .or_default()
                        .push(ImportBinding {
                            local_name: local.to_string(),
                            imported_name: imported.to_string(),
                            source_file: resolved.clone(),
                        });
                }
            }
        }
    }

    /// Walk buffered pending calls, attempt to resolve each to an exported
    /// function in the imported source file, and emit cross-file Calls edges.
    fn resolve_cross_file_calls(&mut self) {
        let pending = std::mem::take(&mut self.pending_calls);
        for pc in pending {
            let Some(bindings) = self.imports_by_file.get(&pc.caller_file) else {
                continue;
            };
            let Some(binding) = bindings.iter().find(|b| b.local_name == pc.callee_name) else {
                continue;
            };
            let Some(exports) = self.exports_by_file.get(&binding.source_file) else {
                continue;
            };
            let Some(&callee) = exports.get(&binding.imported_name) else {
                continue;
            };
            let id = self.fresh_edge();
            self.graph.edges.push(Edge {
                id,
                from: pc.caller,
                to: callee,
                kind: EdgeKind::Calls,
                provenance: pc.provenance,
            });
        }
    }
}

/// Return `Some(variants)` iff the type alias RHS is a union of string literal
/// types (the classical state-machine idiom). Otherwise `None`.
fn string_union_variants(alias: TsNode<'_>, source: &[u8]) -> Option<Vec<String>> {
    let value = alias.child_by_field_name("value")?;
    if value.kind() != "union_type" {
        return None;
    }
    let mut out = Vec::new();
    flatten_union(value, source, &mut out)?;
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Union types nest left-associatively for 3+ variants
/// (`union_type(union_type(a, b), c)`). Walk recursively, collecting every
/// string-literal variant. Return `None` if any member is not a string literal.
fn flatten_union(node: TsNode<'_>, source: &[u8], out: &mut Vec<String>) -> Option<()> {
    let mut cursor = node.walk();
    for member in node.named_children(&mut cursor) {
        match member.kind() {
            "union_type" => flatten_union(member, source, out)?,
            "literal_type" => {
                let lit = member.named_child(0)?;
                if lit.kind() != "string" {
                    return None;
                }
                let text = lit.utf8_text(source).ok()?;
                out.push(text.trim_matches(|c| c == '"' || c == '\'').to_string());
            }
            _ => return None,
        }
    }
    Some(())
}

impl Ingest {
    fn resolve_transitions_in(
        &mut self,
        state_id: NodeId,
        variants: &[String],
        body: TsNode<'_>,
        source: &[u8],
    ) {
        walk_if_statements(body, &mut |iff| {
            let Some(condition) = iff.child_by_field_name("condition") else {
                return;
            };
            let from_variant = match extract_equality_literal(condition, source) {
                Some(s) if variants.iter().any(|v| v == &s) => s,
                _ => return,
            };
            let Some(cons) = iff.child_by_field_name("consequence") else {
                return;
            };
            let mut targets: Vec<String> = Vec::new();
            walk_target_literals(cons, source, &mut targets);
            for to_variant in targets {
                if to_variant == from_variant {
                    continue;
                }
                if !variants.iter().any(|v| v == &to_variant) {
                    continue;
                }
                let key = format!("{from_variant}→{to_variant}");
                let prov = self.prov(key.as_bytes());
                let id = self.fresh_edge();
                self.graph.edges.push(Edge {
                    id,
                    from: state_id,
                    to: state_id,
                    kind: EdgeKind::Transitions {
                        from: from_variant.clone(),
                        to: to_variant,
                    },
                    provenance: prov,
                });
            }
        });
    }
}

/// If `expr` is `x === "lit"` or `"lit" === x`, return "lit". Recurses through
/// `parenthesized_expression` so `(x === "lit")` works too.
fn extract_equality_literal(expr: TsNode<'_>, source: &[u8]) -> Option<String> {
    let e = if expr.kind() == "parenthesized_expression" {
        expr.named_child(0)?
    } else {
        expr
    };
    if e.kind() != "binary_expression" {
        return None;
    }
    let op = e.child_by_field_name("operator")?;
    let op_text = op.utf8_text(source).ok()?;
    if op_text != "===" && op_text != "==" {
        return None;
    }
    let left = e.child_by_field_name("left")?;
    let right = e.child_by_field_name("right")?;
    string_literal(left, source).or_else(|| string_literal(right, source))
}

fn string_literal(n: TsNode<'_>, source: &[u8]) -> Option<String> {
    if n.kind() != "string" {
        return None;
    }
    let t = n.utf8_text(source).ok()?;
    Some(t.trim_matches(|c| c == '"' || c == '\'').to_string())
}

/// Collect every string-literal value appearing as a return value or an
/// assignment RHS anywhere inside `n`. Recurses freely — this is a proxy
/// for "what values does this branch produce?", not a precise analysis.
fn walk_target_literals(n: TsNode<'_>, source: &[u8], out: &mut Vec<String>) {
    match n.kind() {
        "return_statement" => {
            if let Some(expr) = n.named_child(0) {
                if let Some(v) = string_literal(expr, source) {
                    out.push(v);
                }
            }
        }
        "assignment_expression" => {
            if let Some(right) = n.child_by_field_name("right") {
                if let Some(v) = string_literal(right, source) {
                    out.push(v);
                }
            }
        }
        _ => {}
    }
    let mut cursor = n.walk();
    for c in n.named_children(&mut cursor) {
        walk_target_literals(c, source, out);
    }
}

fn string_literal_text(n: TsNode<'_>, source: &[u8]) -> Option<String> {
    if n.kind() != "string" {
        return None;
    }
    let t = n.utf8_text(source).ok()?;
    Some(t.trim_matches(|c| c == '"' || c == '\'').to_string())
}

/// Resolve a relative module specifier to an actual file in `all_files`.
/// Tries, in order:
/// - `<spec>.ts`, `<spec>.tsx`, `<spec>/index.ts`, `<spec>/index.tsx`
/// - the spec as-is (when it already has an extension)
///
/// Non-relative specifiers (bare package names, `@scope/…`) return `None`:
/// v0 only tracks intra-repo imports.
fn resolve_relative_module(spec: &str, from_file: &str, all_files: &[String]) -> Option<String> {
    if !spec.starts_with('.') {
        return None;
    }
    let from_dir = std::path::Path::new(from_file)
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let joined = if from_dir.is_empty() {
        spec.trim_start_matches("./").to_string()
    } else {
        format!("{from_dir}/{}", spec.trim_start_matches("./"))
    };
    let norm = normalize_relative(&joined);
    let candidates = [
        format!("{norm}.ts"),
        format!("{norm}.tsx"),
        format!("{norm}/index.ts"),
        format!("{norm}/index.tsx"),
        norm.clone(),
    ];
    candidates
        .iter()
        .find(|c| all_files.iter().any(|f| f == *c))
        .cloned()
}

/// Collapse `a/b/../c` -> `a/c`. Preserves leading `../` runs.
fn normalize_relative(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => continue,
            ".." => {
                if matches!(out.last(), Some(&s) if s != "..") {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

fn walk_if_statements<'tree>(node: TsNode<'tree>, f: &mut dyn FnMut(TsNode<'tree>)) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "if_statement" {
            f(child);
        }
        walk_if_statements(child, f);
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

/// Capture the declaration signature: everything from the node's start up to
/// the body's start brace, whitespace-flattened. Falls back to the first
/// physical line when no body field is present.
///
/// Multi-line signatures (common in real TS: long parameter lists, nested
/// object types) get collapsed into one readable line so the API-hunk diff
/// shows the full shape, not just `foo(`.
fn first_line(node: TsNode<'_>, source: &[u8]) -> String {
    let start = node.start_byte();
    let end = match node.child_by_field_name("body") {
        Some(body) => body.start_byte(),
        None => node.end_byte(),
    };
    let slice = std::str::from_utf8(&source[start..end]).unwrap_or("");
    let collapsed: String = slice
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        slice.lines().next().unwrap_or("").trim().to_string()
    } else {
        collapsed
    }
}
