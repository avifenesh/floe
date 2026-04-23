//! Data-shape hunk extractor — compare the set of fields declared on
//! each serializable type between base and head source trees, and emit
//! a `Data` hunk when the field set differs.
//!
//! Detection is regex-light: for each `.ts` / `.tsx` / `.rs` file we
//! find:
//!   - TS: `interface NAME { ... }` and `type NAME = { ... }` blocks;
//!     `z.object({ ... })` bound to `const NAME`.
//!   - Rust: `struct NAME { ... }` blocks.
//!
//! For each block we extract the set of field names (left side of
//! `name?: type` / `name: type,`). Field-set diff drives the hunk.
//!
//! This is heuristic — nested generics, multi-line types, and exotic
//! syntaxes can confuse the parser. The worst case is a missed hunk
//! (quiet) or a noisy one (a false rename). Good enough for a "this
//! PR changed a payload shape" signal; an AST-based rewrite can follow
//! in a later pass.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use floe_core::hunks::{Hunk, HunkKind};
use floe_core::provenance::Provenance;

const SOURCE: &str = "floe-hunks/data";
const VERSION: &str = "0.1.0";

pub fn extract_data_hunks(base_root: &Path, head_root: &Path) -> Vec<Hunk> {
    let base = scan_root(base_root);
    let head = scan_root(head_root);

    let mut keys: BTreeSet<(String, String)> = BTreeSet::new();
    keys.extend(base.keys().cloned());
    keys.extend(head.keys().cloned());

    let mut out = Vec::new();
    for (file, type_name) in keys {
        let key = (file.clone(), type_name.clone());
        let before = base.get(&key);
        let after = head.get(&key);
        let (added, removed) = match (before, after) {
            (Some(b), Some(a)) => {
                let added: Vec<String> =
                    a.iter().filter(|f| !b.contains(*f)).cloned().collect();
                let removed: Vec<String> =
                    b.iter().filter(|f| !a.contains(*f)).cloned().collect();
                if added.is_empty() && removed.is_empty() {
                    continue;
                }
                (added, removed)
            }
            (None, Some(a)) => (a.iter().cloned().collect(), Vec::new()),
            (Some(b), None) => (Vec::new(), b.iter().cloned().collect()),
            (None, None) => continue,
        };
        // Rename heuristic: exactly one added and one removed → pair.
        let renamed: Vec<(String, String)> = if added.len() == 1 && removed.len() == 1 {
            vec![(removed[0].clone(), added[0].clone())]
        } else {
            Vec::new()
        };
        let (added_fields, removed_fields) = if renamed.is_empty() {
            (added, removed)
        } else {
            (Vec::new(), Vec::new())
        };
        let id_payload = serde_json::to_vec(&(
            &file,
            &type_name,
            &added_fields,
            &removed_fields,
            &renamed,
        ))
        .unwrap_or_default();
        out.push(Hunk {
            id: format!("data-{}", blake3::hash(&id_payload).to_hex()),
            kind: HunkKind::Data {
                file,
                type_name,
                added_fields,
                removed_fields,
                renamed_fields: renamed,
            },
            provenance: Provenance::new(SOURCE, VERSION, "hunks", &id_payload),
        });
    }
    out
}

fn scan_root(root: &Path) -> BTreeMap<(String, String), BTreeSet<String>> {
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}

fn visit(
    root: &Path,
    dir: &Path,
    out: &mut BTreeMap<(String, String), BTreeSet<String>>,
) {
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
        let is_ts = matches!(ext, "ts" | "tsx" | "mts" | "cts");
        let is_rs = ext == "rs";
        if !is_ts && !is_rs {
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
        let blocks = if is_ts {
            scan_ts(&text)
        } else {
            scan_rust(&text)
        };
        for (type_name, fields) in blocks {
            out.insert((rel.clone(), type_name), fields);
        }
    }
}

/// Find TS `interface X { ... }` / `type X = { ... }` / `const X = z.object({ ... })`
/// blocks and return `(type_name, field-set)` for each.
fn scan_ts(text: &str) -> Vec<(String, BTreeSet<String>)> {
    let mut out = Vec::new();
    // Pattern: interface NAME [extends ...] { ... }
    scan_blocks(text, &["interface "], |header| {
        // header is after the keyword; strip generics / extends.
        let name = ident_prefix(header)?;
        Some(name.to_string())
    })
    .into_iter()
    .for_each(|(n, body)| out.push((n, extract_ts_fields(&body))));

    scan_blocks(text, &["type "], |header| {
        let name = ident_prefix(header)?;
        // Only take `type X = { ... }` forms — skip unions / aliases.
        let after_name = &header[name.len()..];
        if !after_name.trim_start().starts_with('=') {
            return None;
        }
        Some(name.to_string())
    })
    .into_iter()
    .for_each(|(n, body)| out.push((n, extract_ts_fields(&body))));

    // z.object({ ... }) bound to `const NAME` (or `export const NAME`).
    for (i, _) in text.match_indices("z.object(") {
        let before = &text[..i];
        let Some(name) = find_const_binding(before) else {
            continue;
        };
        let after = &text[i + "z.object(".len()..];
        let Some(body) = take_balanced(after, '(', ')') else {
            continue;
        };
        let body = body.trim();
        let body = body.strip_prefix('{').unwrap_or(body);
        let body = body.strip_suffix('}').unwrap_or(body);
        out.push((name, extract_ts_fields(body)));
    }
    out
}

fn scan_rust(text: &str) -> Vec<(String, BTreeSet<String>)> {
    scan_blocks(text, &["struct "], |header| ident_prefix(header).map(|s| s.to_string()))
        .into_iter()
        .map(|(n, body)| (n, extract_rust_fields(&body)))
        .collect()
}

/// For each start-keyword found, find the matching `{ ... }` block and
/// return `(header-derived-name, body-text-between-braces)`.
fn scan_blocks(
    text: &str,
    keywords: &[&str],
    header_to_name: impl Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for kw in keywords {
        let mut cursor = 0;
        while let Some(rel) = text[cursor..].find(kw) {
            let start = cursor + rel;
            // Require the keyword at a word boundary.
            let prev = start
                .checked_sub(1)
                .and_then(|i| text.as_bytes().get(i));
            if prev
                .map(|b| (*b as char).is_alphanumeric() || *b == b'_')
                .unwrap_or(false)
            {
                cursor = start + kw.len();
                continue;
            }
            let after = start + kw.len();
            let Some(brace) = text[after..].find('{') else {
                break;
            };
            let header = &text[after..after + brace];
            cursor = after + brace + 1;
            let Some(body) = take_balanced(&text[after + brace..], '{', '}') else {
                continue;
            };
            let body = body
                .strip_prefix('{')
                .and_then(|s| s.strip_suffix('}'))
                .unwrap_or(&body);
            if let Some(name) = header_to_name(header.trim()) {
                out.push((name, body.to_string()));
            }
        }
    }
    out
}

/// Consume from `s` starting at `open`, returning the inclusive
/// balanced block ending at the matching `close`. `None` if unbalanced.
fn take_balanced(s: &str, open: char, close: char) -> Option<String> {
    let mut depth: i32 = 0;
    let mut started = false;
    let mut end = 0;
    for (i, c) in s.char_indices() {
        if c == open {
            depth += 1;
            started = true;
        } else if c == close {
            depth -= 1;
            if started && depth == 0 {
                end = i + c.len_utf8();
                return Some(s[..end].to_string());
            }
        }
    }
    let _ = end;
    None
}

fn ident_prefix(s: &str) -> Option<&str> {
    let s = s.trim_start();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    if i == 0 {
        None
    } else {
        Some(&s[..i])
    }
}

fn find_const_binding(text_before: &str) -> Option<String> {
    // Look back for `const NAME = ` (with optional export).
    let trimmed = text_before.trim_end();
    let eq_idx = trimmed.rfind('=')?;
    let before_eq = trimmed[..eq_idx].trim_end();
    let name = ident_suffix(before_eq)?;
    let before_name = before_eq[..before_eq.len() - name.len()].trim_end();
    if !(before_name.ends_with("const") || before_name.ends_with("let") || before_name.ends_with("var")) {
        return None;
    }
    Some(name.to_string())
}

fn ident_suffix(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let mut i = bytes.len();
    while i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
        i -= 1;
    }
    if i == bytes.len() {
        None
    } else {
        Some(&s[i..])
    }
}

fn extract_ts_fields(body: &str) -> BTreeSet<String> {
    // Split into top-level field declarations by scanning, tracking
    // brace / paren depth so nested types don't split.
    let mut fields = BTreeSet::new();
    let mut depth: i32 = 0;
    let mut start = 0usize;
    let bytes = body.as_bytes();
    for (i, c) in body.char_indices() {
        match c {
            '{' | '(' | '[' | '<' => depth += 1,
            '}' | ')' | ']' | '>' => depth -= 1,
            ';' | ',' | '\n' if depth == 0 => {
                if let Some(name) = parse_field_name(&body[start..i]) {
                    fields.insert(name);
                }
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    if start < bytes.len() {
        if let Some(name) = parse_field_name(&body[start..]) {
            fields.insert(name);
        }
    }
    fields
}

fn extract_rust_fields(body: &str) -> BTreeSet<String> {
    let mut fields = BTreeSet::new();
    let mut depth: i32 = 0;
    let mut start = 0usize;
    for (i, c) in body.char_indices() {
        match c {
            '{' | '(' | '[' | '<' => depth += 1,
            '}' | ')' | ']' | '>' => depth -= 1,
            ',' | '\n' if depth == 0 => {
                if let Some(name) = parse_field_name(&body[start..i]) {
                    fields.insert(name);
                }
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    if let Some(name) = parse_field_name(&body[start..]) {
        fields.insert(name);
    }
    fields
}

fn parse_field_name(frag: &str) -> Option<String> {
    // Strip comments + leading whitespace + attributes (`#[...]`) + pub.
    let mut line = frag.trim().to_string();
    if line.starts_with("//") || line.starts_with("/*") || line.starts_with("#[") {
        return None;
    }
    if let Some(stripped) = line.strip_prefix("pub ") {
        line = stripped.trim().to_string();
    }
    if let Some(stripped) = line.strip_prefix("readonly ") {
        line = stripped.trim().to_string();
    }
    let stop = line
        .find(|c: char| c == ':' || c.is_whitespace())
        .unwrap_or(line.len());
    let name_raw = &line[..stop];
    let name = name_raw.trim_end_matches('?');
    if name.is_empty()
        || !name
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false)
    {
        return None;
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    // Reserved words that shouldn't be treated as fields.
    if matches!(name, "pub" | "fn" | "impl" | "where" | "for" | "export" | "import") {
        return None;
    }
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn extracts_ts_interface_fields() {
        let src = "export interface Payload {\n  id: string;\n  name?: string;\n  nested: { a: number };\n}\n";
        let blocks = scan_ts(src);
        let (_, fields) = blocks.iter().find(|(n, _)| n == "Payload").unwrap();
        assert!(fields.contains("id"));
        assert!(fields.contains("name"));
        assert!(fields.contains("nested"));
    }

    #[test]
    fn extracts_rust_struct_fields() {
        let src = "pub struct Event {\n  pub id: u64,\n  pub name: String,\n}\n";
        let blocks = scan_rust(src);
        let (_, fields) = blocks.iter().find(|(n, _)| n == "Event").unwrap();
        assert!(fields.contains("id"));
        assert!(fields.contains("name"));
    }

    #[test]
    fn detects_added_field() {
        let tmp = std::env::temp_dir().join(format!("floe-data-{}", std::process::id()));
        let base = tmp.join("base");
        let head = tmp.join("head");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&head).unwrap();
        fs::write(
            base.join("p.ts"),
            "interface P { id: string; }\n",
        )
        .unwrap();
        fs::write(
            head.join("p.ts"),
            "interface P { id: string; extra: number; }\n",
        )
        .unwrap();
        let hunks = extract_data_hunks(&base, &head);
        assert!(hunks.iter().any(|h| matches!(
            &h.kind,
            HunkKind::Data { added_fields, .. } if added_fields.iter().any(|f| f == "extra")
        )));
        let _ = fs::remove_dir_all(&tmp);
    }
}
