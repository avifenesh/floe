//! Lock-primitive hunk extractor — scan TypeScript (and Rust) source
//! files on base and head for synchronization primitives, then emit a
//! `Lock` hunk per (file, primitive) where the presence or class
//! changed.
//!
//! Detection is pattern-based, not AST-based: a file matches the
//! primitive if a regex hits any line in the file. Good enough for the
//! reviewer-facing signal "this PR adds/removes a mutex/semaphore/etc."
//! without pulling another parser into the pipeline.
//!
//! TS primitives covered: `Mutex`, `Semaphore` (async-mutex);
//! `pLimit` (p-limit); `PQueue` (p-queue); `AsyncLock` (async-lock);
//! `Atomics.` (builtin).
//! Rust primitives covered: `Mutex`, `RwLock`, `AtomicBool`,
//! `AtomicUsize`, `AtomicU32`, `AtomicI32`, `OnceCell`, `OnceLock`,
//! `Arc`, `parking_lot::Mutex`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use floe_core::hunks::{Hunk, HunkKind};
use floe_core::provenance::Provenance;

const SOURCE: &str = "floe-hunks/lock";
const VERSION: &str = "0.1.0";

/// Each primitive is a (display_name, needle) pair. We look for the
/// needle as a substring — cheap, and the set is narrow enough that
/// false positives are acceptable (the reviewer sees a lock claim on a
/// file that mentions "Mutex" in a comment; low cost).
const TS_PRIMITIVES: &[(&str, &str)] = &[
    ("async-mutex.Mutex", "new Mutex("),
    ("async-mutex.Semaphore", "new Semaphore("),
    ("p-limit", "pLimit("),
    ("p-queue.PQueue", "new PQueue("),
    ("async-lock.AsyncLock", "new AsyncLock("),
    ("Atomics", "Atomics."),
];

const RUST_PRIMITIVES: &[(&str, &str)] = &[
    ("Mutex", "Mutex::new"),
    ("RwLock", "RwLock::new"),
    ("parking_lot.Mutex", "parking_lot::Mutex"),
    ("AtomicBool", "AtomicBool::new"),
    ("AtomicUsize", "AtomicUsize::new"),
    ("AtomicU32", "AtomicU32::new"),
    ("AtomicI32", "AtomicI32::new"),
    ("OnceCell", "OnceCell::"),
    ("OnceLock", "OnceLock::"),
];

/// Walk `base_root` and `head_root`, find every (file, primitive) pair,
/// and emit hunks where the presence changed. `file` in the emitted
/// hunk is the root-relative path (head-side when present, base-side
/// otherwise) with forward slashes.
pub fn extract_lock_hunks(base_root: &Path, head_root: &Path) -> Vec<Hunk> {
    let base_set = scan_root(base_root);
    let head_set = scan_root(head_root);

    let mut all_keys: BTreeSet<(String, String)> = BTreeSet::new();
    all_keys.extend(base_set.keys().cloned());
    all_keys.extend(head_set.keys().cloned());

    let mut out = Vec::new();
    for (file, primitive) in all_keys {
        let before = base_set
            .get(&(file.clone(), primitive.clone()))
            .cloned();
        let after = head_set
            .get(&(file.clone(), primitive.clone()))
            .cloned();
        if before == after {
            continue;
        }
        let id_payload =
            serde_json::to_vec(&(&file, &primitive, &before, &after)).unwrap_or_default();
        out.push(Hunk {
            id: format!("lock-{}", blake3::hash(&id_payload).to_hex()),
            kind: HunkKind::Lock {
                file,
                primitive,
                before,
                after,
            },
            provenance: Provenance::new(SOURCE, VERSION, "hunks", &id_payload),
        });
    }
    out
}

/// For every scannable file under `root`, record each (file, primitive)
/// pair that appears, with the value being the same primitive display
/// name (so before/after in the hunk are `Some(name)` or `None`).
fn scan_root(root: &Path) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}

fn visit(root: &Path, dir: &Path, out: &mut BTreeMap<(String, String), String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if path.is_dir() {
            // Skip common noisy dirs.
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
        let primitives: &[(&str, &str)] = match ext {
            "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" => TS_PRIMITIVES,
            "rs" => RUST_PRIMITIVES,
            _ => continue,
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| path.to_string_lossy().replace('\\', "/"));
        for (display, needle) in primitives {
            if text.contains(needle) {
                out.insert((rel.clone(), display.to_string()), display.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detects_added_lock_in_head() {
        let tmp = std::env::temp_dir().join(format!("floe-lock-test-{}", std::process::id()));
        let base = tmp.join("base");
        let head = tmp.join("head");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&head).unwrap();
        fs::write(base.join("worker.ts"), "export function run() { return 1; }\n").unwrap();
        fs::write(
            head.join("worker.ts"),
            "import { Mutex } from 'async-mutex';\nconst m = new Mutex();\n",
        )
        .unwrap();

        let hunks = extract_lock_hunks(&base, &head);
        assert!(hunks.iter().any(|h| matches!(
            &h.kind,
            HunkKind::Lock { primitive, before: None, after: Some(_), .. }
                if primitive == "async-mutex.Mutex"
        )));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn no_hunk_when_unchanged() {
        let tmp = std::env::temp_dir().join(format!("floe-lock-test2-{}", std::process::id()));
        let base = tmp.join("base");
        let head = tmp.join("head");
        fs::create_dir_all(&base).unwrap();
        fs::create_dir_all(&head).unwrap();
        let src = "import { Mutex } from 'async-mutex';\nconst m = new Mutex();\n";
        fs::write(base.join("a.ts"), src).unwrap();
        fs::write(head.join("a.ts"), src).unwrap();
        let hunks = extract_lock_hunks(&base, &head);
        assert!(hunks.is_empty());
        let _ = fs::remove_dir_all(&tmp);
    }
}
