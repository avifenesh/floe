//! Claim-anchoring pass — fill `Claim.source_refs` on every claim
//! that carries entity names but no explicit source coordinates yet.
//!
//! RFC Appendix F upgrade #6. The UI renders a "jump to source"
//! affordance for every claim that has at least one `SourceRef`, so
//! anchoring closes the "where did this come from?" loop for the
//! structural-evidence claims emitted by `floe-evidence` before the
//! LSP pass runs.
//!
//! # Two tiers
//!
//! 1. **Synchronous declaration anchoring** ([`attach`]) — fast,
//!    graph-only. Resolves each claim's entity name to the matching
//!    Function/Type/State node and takes `(file, span.start)` →
//!    `(line, UTF-16 column)` from on-disk source. Runs right after
//!    `floe_evidence::collect`; no external dependencies.
//! 2. **Async LSP reference fan-out** ([`fan_out_refs`]) — richer.
//!    For every claim that already carries a declaration anchor, queries
//!    `textDocument/references` (no declaration, to avoid duplicating
//!    the anchor already present) on each side and appends up to 10
//!    additional `SourceRef`s per claim. Backgrounded so the reviewer
//!    sees the artifact READY first, with richer anchors filling in.
//!
//! Keeping the two tiers separate lets the sync pass stay pure (no
//! LSP dep, easy to test) while still allowing the worker to schedule
//! the fan-out alongside the other LLM passes.

use floe_core::graph::NodeKind;
use floe_core::{Artifact, SourceRef, SourceSide};

/// Walk every flow's evidence and attach source refs where missing.
/// Safe to call multiple times — claims that already carry refs are
/// left alone.
pub fn attach(artifact: &mut Artifact) {
    let head_nodes = snapshot_nodes(&artifact.head);
    let base_nodes = snapshot_nodes(&artifact.base);
    for flow in artifact.flows.iter_mut() {
        // Claims emitted by floe-evidence often leave `claim.entities`
        // empty — their natural anchor set is the flow's own entity
        // list. Fall back to it so structural claims still pick up a
        // "→ source" jump target rather than landing cold.
        let flow_entities: Vec<String> = flow
            .entities
            .iter()
            .chain(flow.extra_entities.iter())
            .cloned()
            .collect();
        for claim in flow.evidence.iter_mut() {
            if !claim.source_refs.is_empty() {
                continue;
            }
            let names: &[String] = if !claim.entities.is_empty() {
                &claim.entities
            } else {
                &flow_entities
            };
            for name in names.iter().take(6) {
                // Head first — fresh code is the usual jump target.
                if let Some((file, line, col)) = resolve_name(&head_nodes, name) {
                    claim.source_refs.push(SourceRef {
                        file,
                        side: SourceSide::Head,
                        line,
                        column: col,
                        length: Some(name_len_hint(name)),
                    });
                } else if let Some((file, line, col)) = resolve_name(&base_nodes, name) {
                    claim.source_refs.push(SourceRef {
                        file,
                        side: SourceSide::Base,
                        line,
                        column: col,
                        length: Some(name_len_hint(name)),
                    });
                }
            }
        }
    }
}

/// Snapshot (name, file, span.start) of every Function/Type/State
/// node. Cheap — graph is small even on big repos.
fn snapshot_nodes(graph: &floe_core::Graph) -> Vec<NodeEntry> {
    graph
        .nodes
        .iter()
        .filter_map(|n| match &n.kind {
            NodeKind::Function { name, .. }
            | NodeKind::Type { name }
            | NodeKind::State { name, .. } => Some(NodeEntry {
                name: name.clone(),
                file: n.file.clone(),
                // Need the source text to convert byte span → line/col;
                // for now we emit byte span as the column fallback and
                // line=1 so the UI at least has the file + length. A
                // later pass can refine by reading the file.
                byte_offset: n.span.start,
            }),
            _ => None,
        })
        .collect()
}

struct NodeEntry {
    name: String,
    file: String,
    byte_offset: u32,
}

fn resolve_name(entries: &[NodeEntry], name: &str) -> Option<(String, u32, u32)> {
    // Prefer exact match; fall back to suffix match so
    // `ClassName.method` can resolve when the claim cites the bare
    // method name (rare but the safe fallback).
    let exact = entries.iter().find(|e| e.name == name);
    let fuzzy = exact.or_else(|| {
        entries.iter().find(|e| {
            e.name.rsplit_once('.').map(|(_, s)| s == name).unwrap_or(false)
        })
    })?;
    // Resolve (line, column) from a byte offset by reading the file.
    // Best-effort — if the file isn't present, emit (1, 1) as a
    // fallback; UI at least lands on the file top.
    let (line, col) = byte_to_line_col(&fuzzy.file, fuzzy.byte_offset).unwrap_or((1, 1));
    Some((fuzzy.file.clone(), line, col))
}

fn byte_to_line_col(file: &str, byte_offset: u32) -> Option<(u32, u32)> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut line: u32 = 1;
    let mut line_start: usize = 0;
    let target = byte_offset as usize;
    for (i, b) in text.as_bytes().iter().enumerate() {
        if i >= target {
            break;
        }
        if *b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let col_bytes = &text.as_bytes()[line_start..target.min(text.len())];
    // UTF-16 column (what LSP uses) — iterate chars and count surrogates.
    let col = std::str::from_utf8(col_bytes)
        .ok()?
        .encode_utf16()
        .count() as u32;
    Some((line, col.saturating_add(1)))
}

/// Heuristic highlight span — length of the bare identifier. Claims
/// cite entities like `ClassName.method`; the UI should highlight
/// just `method` when it lands, so we hand back the trailing segment
/// length. Falls back to full length for dot-free names.
fn name_len_hint(qname: &str) -> u32 {
    let tail = qname.rsplit('.').next().unwrap_or(qname);
    tail.encode_utf16().count() as u32
}

/// Max additional SourceRefs appended per claim by the LSP fan-out.
/// Bounded to keep the artifact JSON tight on large PRs.
const MAX_REFS_PER_CLAIM: usize = 10;

/// Fan out `source_refs` on every claim using LSP `references`.
/// Called from the worker as a background pass; returns when either
/// side of the artifact has been enriched or the LSP session fails
/// (no error — caller keeps the sync anchors).
///
/// Paths on `Claim.source_refs` are relative (the graph's `Node.file`);
/// we resolve to absolute by joining `base_root` / `head_root` per
/// `SourceSide` before passing to LSP. Files that aren't readable
/// are skipped quietly.
pub async fn fan_out_refs(
    artifact: &mut floe_core::Artifact,
    base_root: &std::path::Path,
    head_root: &std::path::Path,
) {
    // Head-side session first — reviewers land there most often.
    if let Err(e) = fan_out_side(artifact, head_root, SourceSide::Head).await {
        tracing::warn!(error = %e, "claim-anchor fan-out (head) failed");
    }
    if let Err(e) = fan_out_side(artifact, base_root, SourceSide::Base).await {
        tracing::warn!(error = %e, "claim-anchor fan-out (base) failed");
    }
}

async fn fan_out_side(
    artifact: &mut floe_core::Artifact,
    root: &std::path::Path,
    side: SourceSide,
) -> anyhow::Result<()> {
    let mut client = floe_lsp::TsLspClient::start(root).await?;
    // Pre-open every file the claim set references so we pay one
    // tsserver open per file, not one per claim.
    let relevant_files: std::collections::BTreeSet<String> = artifact
        .flows
        .iter()
        .flat_map(|f| f.evidence.iter())
        .flat_map(|c| c.source_refs.iter())
        .filter(|r| r.side == side)
        .map(|r| r.file.clone())
        .collect();
    for rel in &relevant_files {
        let abs = root.join(rel);
        if let Ok(text) = tokio::fs::read_to_string(&abs).await {
            let _ = client.open_file(&abs, &text).await;
        }
    }

    // Iterate flows; expand each claim's refs with references to the
    // same declaration. Avoid the decl itself (include_declaration:false).
    let mut pending: Vec<(usize, usize, Vec<SourceRef>)> = Vec::new();
    for (fi, flow) in artifact.flows.iter().enumerate() {
        for (ci, claim) in flow.evidence.iter().enumerate() {
            if claim.source_refs.len() >= MAX_REFS_PER_CLAIM {
                continue;
            }
            let seed = match claim.source_refs.iter().find(|r| r.side == side) {
                Some(r) => r,
                None => continue,
            };
            // Line/col in SourceRef are 1-indexed by our convention;
            // LSP expects 0-indexed.
            let abs = root.join(&seed.file);
            let line0 = seed.line.saturating_sub(1);
            let col0 = seed.column.saturating_sub(1);
            let refs = match client
                .references(&abs, line0, col0, false)
                .await
            {
                Ok(r) => r,
                Err(_) => continue,
            };
            let budget = MAX_REFS_PER_CLAIM.saturating_sub(claim.source_refs.len());
            let new_refs: Vec<SourceRef> = refs
                .into_iter()
                .take(budget)
                .filter_map(|loc| {
                    let rel = uri_to_rel(&loc.uri, root)?;
                    Some(SourceRef {
                        file: rel,
                        side,
                        line: loc.range.start.line + 1,
                        column: loc.range.start.character + 1,
                        length: Some(
                            loc.range.end.character.saturating_sub(loc.range.start.character),
                        ),
                    })
                })
                .collect();
            if !new_refs.is_empty() {
                pending.push((fi, ci, new_refs));
            }
        }
    }

    // Apply pending merges — skip any that duplicate an existing ref.
    for (fi, ci, new_refs) in pending {
        let Some(flow) = artifact.flows.get_mut(fi) else {
            continue;
        };
        let Some(claim) = flow.evidence.get_mut(ci) else {
            continue;
        };
        for r in new_refs {
            let dupe = claim
                .source_refs
                .iter()
                .any(|e| e.side == r.side && e.file == r.file && e.line == r.line);
            if !dupe {
                claim.source_refs.push(r);
            }
        }
    }

    let _ = client.shutdown().await;
    Ok(())
}

fn uri_to_rel(
    uri: &floe_lsp::Url,
    root: &std::path::Path,
) -> Option<String> {
    let path = uri.to_file_path().ok()?;
    let canonical_root = root.canonicalize().ok()?;
    let canonical_path = path.canonicalize().ok()?;
    let rel = canonical_path.strip_prefix(&canonical_root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_len_hint_uses_last_segment() {
        assert_eq!(name_len_hint("Queue.enqueue"), 7);
        assert_eq!(name_len_hint("plainFn"), 7);
    }

    #[test]
    fn byte_to_line_col_maps_first_line() {
        let path = std::env::temp_dir().join("floe_anchors_test.ts");
        std::fs::write(&path, "const x = 1;\nexport function foo() {}\n").unwrap();
        let (line, col) = byte_to_line_col(path.to_str().unwrap(), 0).unwrap();
        assert_eq!(line, 1);
        assert_eq!(col, 1);
        // "export function " = 16 chars; foo at offset 16+13 = 29
        let (line, _col) = byte_to_line_col(path.to_str().unwrap(), 29).unwrap();
        assert_eq!(line, 2);
    }
}
