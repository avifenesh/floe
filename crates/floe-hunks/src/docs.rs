//! Docs-drift hunk extractor — find functions in the head tree whose
//! JSDoc / Rustdoc `@param` list disagrees with the signature's actual
//! parameter names, and emit a `Docs` hunk.
//!
//! This is a head-only observation: we don't care how the drift came
//! to be, only that the docs now lie about the code. Reviewers who see
//! a Docs hunk know to either update the comment or the function.
//!
//! Detection is regex-light:
//! - TS: `/** ... */\n[export ]function NAME(params) { ... }`. `@param X`
//!   lines in the comment vs the param list in `(...)`.
//! - Rust: `/// line(s)\npub? fn NAME(args) { ... }`. `/// * `arg` ...`
//!   or inline arg mentions.
//!
//! Rust scanning is skipped for this first pass — rustdoc parameter
//! conventions are softer than JSDoc, so false positives would be high.

use std::collections::BTreeSet;
use std::path::Path;

use floe_core::hunks::{Hunk, HunkKind};
use floe_core::provenance::Provenance;

const SOURCE: &str = "floe-hunks/docs";
const VERSION: &str = "0.1.0";

pub fn extract_docs_hunks(head_root: &Path) -> Vec<Hunk> {
    let mut out = Vec::new();
    visit(head_root, head_root, &mut out);
    out
}

fn visit(root: &Path, dir: &Path, out: &mut Vec<Hunk>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if path.is_dir() {
            if matches!(
                name,
                "node_modules" | ".git" | "dist" | "build" | "target" | "coverage" | ".next"
            ) {
                continue;
            }
            visit(root, &path, out);
            continue;
        }
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if !matches!(ext, "ts" | "tsx" | "mts" | "cts") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| path.to_string_lossy().replace('\\', "/"));
        for (target, drift_kind) in scan_ts(&text) {
            let id_payload =
                serde_json::to_vec(&(&rel, &target, &drift_kind)).unwrap_or_default();
            out.push(Hunk {
                id: format!("docs-{}", blake3::hash(&id_payload).to_hex()),
                kind: HunkKind::Docs {
                    file: rel.clone(),
                    target,
                    drift_kind,
                },
                provenance: Provenance::new(SOURCE, VERSION, "hunks", &id_payload),
            });
        }
    }
}

/// Walk a TS source file and yield `(function-name, drift-kind)` for
/// every function whose JSDoc `@param` list doesn't match its
/// signature.
fn scan_ts(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = text[cursor..].find("/**") {
        let start = cursor + rel;
        let Some(end_rel) = text[start..].find("*/") else {
            break;
        };
        let end = start + end_rel + 2;
        let comment = &text[start..end];
        cursor = end;

        // Look at the next non-whitespace, non-export keyword token for
        // `function NAME(params)`.
        let after = text[end..].trim_start();
        let Some((fn_name, params_raw)) = parse_ts_fn_header(after) else {
            continue;
        };
        let doc_params = extract_doc_params(comment);
        if doc_params.is_empty() {
            continue;
        }
        let sig_params = extract_sig_params(&params_raw);
        let drift = classify_drift(&doc_params, &sig_params);
        if let Some(kind) = drift {
            out.push((fn_name, kind.to_string()));
        }
    }
    out
}

fn parse_ts_fn_header(s: &str) -> Option<(String, String)> {
    let s = s.trim_start();
    let after = s.strip_prefix("export ").unwrap_or(s).trim_start();
    let after = after.strip_prefix("async ").unwrap_or(after).trim_start();
    let after = after.strip_prefix("function ")?;
    let after = after.trim_start();
    // Strip generics.
    let name_end = after
        .find(|c: char| c == '(' || c == '<' || c.is_whitespace())
        .unwrap_or(after.len());
    let name = after[..name_end].trim().to_string();
    if name.is_empty() {
        return None;
    }
    let paren_start = after.find('(')?;
    let mut depth = 0i32;
    let mut end = 0usize;
    for (i, c) in after[paren_start..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = paren_start + i;
                    break;
                }
            }
            _ => {}
        }
    }
    if end <= paren_start {
        return None;
    }
    Some((name, after[paren_start + 1..end].to_string()))
}

fn extract_doc_params(comment: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in comment.lines() {
        let l = line.trim_start_matches(|c: char| c == '/' || c == '*' || c.is_whitespace());
        if let Some(rest) = l.strip_prefix("@param") {
            let rest = rest.trim_start();
            // Forms: `@param {T} name desc`, `@param name desc`.
            let rest = if rest.starts_with('{') {
                let Some(close) = rest.find('}') else { continue };
                rest[close + 1..].trim_start()
            } else {
                rest
            };
            let name_end = rest.find(|c: char| c.is_whitespace() || c == '-' || c == ':').unwrap_or(rest.len());
            let name = rest[..name_end].trim_end_matches('?').trim_start_matches('[').trim_end_matches(']');
            let name = name.split('=').next().unwrap_or("");
            if !name.is_empty() {
                out.push(name.to_string());
            }
        }
    }
    out
}

fn extract_sig_params(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let push = |frag: &str, out: &mut Vec<String>| {
        let t = frag.trim().trim_start_matches("...");
        if t.is_empty() {
            return;
        }
        let name_end = t
            .find(|c: char| c == ':' || c == '=' || c == '?' || c.is_whitespace())
            .unwrap_or(t.len());
        let name = t[..name_end].trim();
        if !name.is_empty()
            && name
                .chars()
                .next()
                .map(|c| c.is_ascii_alphabetic() || c == '_' || c == '{')
                .unwrap_or(false)
        {
            // Destructured params (`{ a, b }`) aren't supported by doc
            // `@param` conventions — skip them entirely.
            if name.starts_with('{') || name.starts_with('[') {
                return;
            }
            out.push(name.to_string());
        }
    };
    for (i, c) in raw.char_indices() {
        match c {
            '(' | '{' | '[' | '<' => depth += 1,
            ')' | '}' | ']' | '>' => depth -= 1,
            ',' if depth == 0 => {
                push(&raw[start..i], &mut out);
                start = i + 1;
            }
            _ => {}
        }
    }
    push(&raw[start..], &mut out);
    out
}

fn classify_drift(doc: &[String], sig: &[String]) -> Option<&'static str> {
    if doc.is_empty() || sig.is_empty() {
        return None;
    }
    if doc.len() != sig.len() {
        return Some("param-count");
    }
    let doc_set: BTreeSet<_> = doc.iter().collect();
    let sig_set: BTreeSet<_> = sig.iter().collect();
    if doc_set != sig_set {
        return Some("param-names");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_param_count_mismatch() {
        let src = r#"
/**
 * Runs the thing.
 * @param a the first
 */
export function run(a: number, b: string) {}
"#;
        let drifts = scan_ts(src);
        assert_eq!(drifts, vec![("run".to_string(), "param-count".to_string())]);
    }

    #[test]
    fn flags_param_name_mismatch() {
        let src = r#"
/**
 * @param a foo
 * @param b bar
 */
export function run(a: number, c: string) {}
"#;
        let drifts = scan_ts(src);
        assert_eq!(drifts, vec![("run".to_string(), "param-names".to_string())]);
    }

    #[test]
    fn no_drift_when_aligned() {
        let src = r#"
/**
 * @param a foo
 * @param b bar
 */
export function run(a: number, b: string) {}
"#;
        let drifts = scan_ts(src);
        assert!(drifts.is_empty());
    }
}
