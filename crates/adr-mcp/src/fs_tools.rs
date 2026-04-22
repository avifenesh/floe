//! File-access tools for the proof LLM pass: `read_file`, `grep`, `glob`.
//!
//! These three are surfaced only when the MCP server runs with `--proof`
//! — probe sessions keep their tight navigation-only toolbox. The proof
//! pass needs unstructured access to the repo (examples, test files,
//! bench scripts) so it can verify *"does `examples/stream-backpressure.ts`
//! actually exercise the 64 KB window claim?"*.
//!
//! ## Attribution
//!
//! `read_file` is lifted from OpenAI's Codex CLI (Apache-2.0,
//! <https://github.com/openai/codex>, file `codex-rs/core/src/tools/
//! handlers/read_file.rs` at SHA 504aeb0e09bb…). Significantly slimmed:
//! the upstream offers a dual-mode (`slice` + `indentation`) reader
//! with anchor-expansion rules; we ship the slice mode only for v1.
//! See `feedback_reuse_codex_tools.md` in memory for why we lift rather
//! than reinvent.
//!
//! `grep` is built on the ripgrep crate family (`grep-searcher`,
//! `grep-regex`, `ignore`) directly rather than shelling out to the
//! `rg` binary — our MCP server is long-lived so per-call process
//! spawn cost is unacceptable. `glob` is local (no Codex equivalent),
//! `ignore::WalkBuilder` + `globset::Glob`.
//!
//! ## Path safety
//!
//! All three tools resolve their path arguments against the
//! `ToolsRoot` passed at construction time and reject paths that
//! escape it (symlink traversal, `..` tricks, absolute paths pointing
//! outside). The proof LLM *must not* read outside the repo it's
//! analysing.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use globset::{Glob, GlobSetBuilder};
use grep_regex::RegexMatcher;
use grep_searcher::{sinks::UTF8, SearcherBuilder};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

/// Per-session root the fs tools operate against. Every path
/// argument resolves to a canonical path underneath this root.
#[derive(Debug, Clone)]
pub struct ToolsRoot {
    root: PathBuf,
}

impl ToolsRoot {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root
            .into()
            .canonicalize()
            .map_err(|e| anyhow!("canonicalize tools root: {e}"))?;
        Ok(Self { root })
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    /// Resolve a user-supplied path argument (absolute or relative)
    /// against the root. Rejects anything that escapes.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf> {
        let p = Path::new(rel);
        let joined = if p.is_absolute() { p.to_path_buf() } else { self.root.join(p) };
        let canonical = joined
            .canonicalize()
            .map_err(|e| anyhow!("canonicalize {}: {e}", joined.display()))?;
        if !canonical.starts_with(&self.root) {
            return Err(anyhow!(
                "path escapes session root: {}",
                canonical.display()
            ));
        }
        Ok(canonical)
    }
}

// ─────────────────────────────────────────────────────────────────────
// read_file
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ReadFileInput {
    /// File path relative to the session root (absolute paths also
    /// accepted but must canonicalize underneath the root).
    pub file_path: String,
    /// 1-indexed start line. Default 1.
    #[serde(default)]
    pub offset: Option<u64>,
    /// Max lines to return from `offset`. Default [`READ_FILE_DEFAULT_LIMIT`].
    #[serde(default)]
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadFileOutput {
    /// Newline-joined lines with `L{n}: ` prefix — same shape Codex
    /// emits so prompts lifted from their ecosystem work unchanged.
    pub content: String,
    /// Total lines in the file — lets the model know when to paginate.
    pub total_lines: u64,
}

pub const READ_FILE_DEFAULT_LIMIT: u64 = 2000;
/// Cap on how many bytes we'll include for one line before truncating;
/// minified JS/CSS can produce 1 MB-on-one-line files that would
/// otherwise blow the model's context in a single read.
pub const READ_FILE_MAX_LINE_BYTES: usize = 500;

pub fn read_file(root: &ToolsRoot, input: ReadFileInput) -> Result<ReadFileOutput> {
    let path = root.resolve(&input.file_path)?;
    let offset = input.offset.unwrap_or(1);
    let limit = input.limit.unwrap_or(READ_FILE_DEFAULT_LIMIT);
    if offset == 0 {
        return Err(anyhow!("offset must be >= 1 (1-indexed)"));
    }
    if limit == 0 {
        return Err(anyhow!("limit must be >= 1"));
    }
    let bytes = std::fs::read(&path)
        .map_err(|e| anyhow!("read {}: {e}", path.display()))?;
    // Treat as UTF-8 best-effort; binary files get lossy decoding so
    // the model sees "� … " rather than a tool error. The proof pass
    // rarely reads binaries; when it does, the lossy view is enough
    // to recognise "yes, this is a PNG, skip it".
    let text = String::from_utf8_lossy(&bytes);
    let total_lines: u64 = text.split('\n').count() as u64;
    let mut out = String::new();
    let mut emitted: u64 = 0;
    for (idx, raw_line) in text.split('\n').enumerate() {
        let lineno = (idx as u64) + 1;
        if lineno < offset {
            continue;
        }
        if emitted >= limit {
            break;
        }
        let stripped = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let capped = truncate_at_char_boundary(stripped, READ_FILE_MAX_LINE_BYTES);
        out.push_str(&format!("L{lineno}: {capped}\n"));
        emitted += 1;
    }
    Ok(ReadFileOutput { content: out, total_lines })
}

/// Truncate a string at a character boundary — don't split a UTF-8
/// codepoint when hitting the byte cap. Lifted from Codex's
/// `codex_utils_string::take_bytes_at_char_boundary`.
fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = s[..end].to_string();
    out.push_str(" …");
    out
}

// ─────────────────────────────────────────────────────────────────────
// grep — ripgrep crates directly (library, not shell-out)
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct GrepInput {
    /// Regex pattern. Ripgrep flavour (Rust `regex` crate) — no PCRE.
    pub pattern: String,
    /// Optional subpath to scope the search (relative to session root).
    #[serde(default)]
    pub path: Option<String>,
    /// Optional glob to filter files (e.g. `"*.ts"`, `"src/**/*.rs"`).
    #[serde(default)]
    pub glob: Option<String>,
    /// Max match lines to return. Default [`GREP_DEFAULT_LIMIT`],
    /// hard-capped at [`GREP_MAX_LIMIT`].
    #[serde(default)]
    pub limit: Option<usize>,
    /// Case-insensitive match. Default false.
    #[serde(default)]
    pub case_insensitive: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GrepOutput {
    pub matches: Vec<GrepMatch>,
    /// `true` when we hit the limit — model knows to narrow the pattern.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GrepMatch {
    /// File path relative to the session root (not absolute — easier
    /// for the LLM to cite back).
    pub path: String,
    pub line: u64,
    pub text: String,
}

pub const GREP_DEFAULT_LIMIT: usize = 100;
pub const GREP_MAX_LIMIT: usize = 2000;

pub fn grep(root: &ToolsRoot, input: GrepInput) -> Result<GrepOutput> {
    let limit = input
        .limit
        .unwrap_or(GREP_DEFAULT_LIMIT)
        .min(GREP_MAX_LIMIT)
        .max(1);
    let scope = match input.path.as_deref() {
        Some(p) => root.resolve(p)?,
        None => root.path().to_path_buf(),
    };
    let matcher = RegexMatcher::new_line_matcher(&input.pattern)
        .map_err(|e| anyhow!("compile pattern: {e}"))?;
    // Re-do with case flag if needed — RegexMatcherBuilder gives us
    // that knob; constructing via the builder keeps case-insensitive
    // opt-in rather than forcing `(?i)` into the pattern.
    let matcher = if input.case_insensitive {
        grep_regex::RegexMatcherBuilder::new()
            .case_insensitive(true)
            .build(&input.pattern)
            .map_err(|e| anyhow!("compile case-insensitive pattern: {e}"))?
    } else {
        matcher
    };

    let globset = match input.glob.as_deref() {
        Some(g) => {
            let mut b = GlobSetBuilder::new();
            b.add(Glob::new(g).map_err(|e| anyhow!("bad glob: {e}"))?);
            Some(b.build().map_err(|e| anyhow!("build globset: {e}"))?)
        }
        None => None,
    };

    let mut out: Vec<GrepMatch> = Vec::new();
    let mut truncated = false;
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .build();

    let walker = WalkBuilder::new(&scope)
        .standard_filters(true) // gitignore + hidden + .ignore
        .build();
    'outer: for entry in walker.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let entry_path = entry.path();
        // Apply glob filter against the path relative to scope.
        if let Some(set) = &globset {
            let rel = entry_path
                .strip_prefix(&scope)
                .unwrap_or(entry_path);
            if !set.is_match(rel) {
                continue;
            }
        }
        let rel_for_report = entry_path
            .strip_prefix(root.path())
            .unwrap_or(entry_path)
            .to_string_lossy()
            .replace('\\', "/");
        let result = searcher.search_path(
            &matcher,
            entry_path,
            UTF8(|lineno, text| {
                if out.len() >= limit {
                    truncated = true;
                    return Ok(false); // stop this file
                }
                out.push(GrepMatch {
                    path: rel_for_report.clone(),
                    line: lineno,
                    text: truncate_at_char_boundary(
                        text.strip_suffix('\n').unwrap_or(text),
                        READ_FILE_MAX_LINE_BYTES,
                    ),
                });
                Ok(true)
            }),
        );
        if let Err(e) = result {
            tracing::debug!(path = %entry_path.display(), error = %e, "grep skipped file");
        }
        if truncated {
            break 'outer;
        }
    }

    Ok(GrepOutput { matches: out, truncated })
}

// ─────────────────────────────────────────────────────────────────────
// glob — WalkBuilder + GlobSet (no Codex equivalent to lift)
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct GlobInput {
    /// Glob pattern (e.g. `"examples/**/*.ts"`, `"**/*.bench.*"`).
    pub pattern: String,
    /// Optional subpath to scope the walk (relative to session root).
    #[serde(default)]
    pub path: Option<String>,
    /// Max paths returned. Default [`GLOB_DEFAULT_LIMIT`].
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GlobOutput {
    pub paths: Vec<String>,
    pub truncated: bool,
}

pub const GLOB_DEFAULT_LIMIT: usize = 500;

pub fn glob(root: &ToolsRoot, input: GlobInput) -> Result<GlobOutput> {
    let limit = input.limit.unwrap_or(GLOB_DEFAULT_LIMIT).max(1);
    let scope = match input.path.as_deref() {
        Some(p) => root.resolve(p)?,
        None => root.path().to_path_buf(),
    };
    let g = Glob::new(&input.pattern)
        .map_err(|e| anyhow!("bad glob: {e}"))?
        .compile_matcher();
    let mut paths: Vec<String> = Vec::new();
    let mut truncated = false;
    let walker = WalkBuilder::new(&scope)
        .standard_filters(true)
        .build();
    for entry in walker.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let entry_path = entry.path();
        let rel = entry_path
            .strip_prefix(&scope)
            .unwrap_or(entry_path);
        if !g.is_match(rel) {
            continue;
        }
        if paths.len() >= limit {
            truncated = true;
            break;
        }
        let rel_for_report = entry_path
            .strip_prefix(root.path())
            .unwrap_or(entry_path)
            .to_string_lossy()
            .replace('\\', "/");
        paths.push(rel_for_report);
    }
    Ok(GlobOutput { paths, truncated })
}

// ─────────────────────────────────────────────────────────────────────
// JSON-Schema descriptors — fed to the LLM as tool definitions
// ─────────────────────────────────────────────────────────────────────

/// Tool descriptor the MCP server advertises for each fs tool.
///
/// Mirrors the shape Codex publishes so prompts lifted from their
/// ecosystem continue to work verbatim.
pub fn tool_schemas() -> serde_json::Value {
    serde_json::json!({
        "adr.read_file": {
            "type": "object",
            "description": "Read a file by path. Returns numbered lines (L{n}: prefix). Respect offset/limit to paginate large files.",
            "properties": {
                "file_path": {"type": "string", "description": "Path relative to the session root, or absolute inside the root."},
                "offset": {"type": "integer", "minimum": 1, "description": "1-indexed start line. Default 1."},
                "limit": {"type": "integer", "minimum": 1, "description": "Max lines to return. Default 2000."}
            },
            "required": ["file_path"],
            "additionalProperties": false
        },
        "adr.grep": {
            "type": "object",
            "description": "Search files with a Rust-regex pattern (ripgrep-backed). Respects .gitignore. Returns matching line/path/text.",
            "properties": {
                "pattern": {"type": "string", "description": "Regex pattern."},
                "path": {"type": "string", "description": "Optional subpath to scope the search."},
                "glob": {"type": "string", "description": "Optional glob (e.g. '*.ts')."},
                "limit": {"type": "integer", "minimum": 1, "maximum": 2000, "description": "Max matches. Default 100."},
                "case_insensitive": {"type": "boolean", "description": "Case-insensitive match. Default false."}
            },
            "required": ["pattern"],
            "additionalProperties": false
        },
        "adr.glob": {
            "type": "object",
            "description": "List files matching a glob pattern, respecting .gitignore. Returns paths relative to the session root.",
            "properties": {
                "pattern": {"type": "string", "description": "Glob pattern (e.g. 'examples/**/*.ts')."},
                "path": {"type": "string", "description": "Optional subpath to scope the walk."},
                "limit": {"type": "integer", "minimum": 1, "description": "Max paths returned. Default 500."}
            },
            "required": ["pattern"],
            "additionalProperties": false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (TempDir, ToolsRoot) {
        let dir = TempDir::new().unwrap();
        let r = ToolsRoot::new(dir.path()).unwrap();
        (dir, r)
    }

    #[test]
    fn read_file_returns_numbered_lines() {
        let (dir, root) = setup();
        fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
        let out = read_file(
            &root,
            ReadFileInput { file_path: "a.txt".into(), offset: None, limit: None },
        )
        .unwrap();
        assert!(out.content.contains("L1: one"));
        assert!(out.content.contains("L2: two"));
        assert!(out.content.contains("L3: three"));
    }

    #[test]
    fn read_file_honors_offset_and_limit() {
        let (dir, root) = setup();
        fs::write(dir.path().join("a.txt"), "1\n2\n3\n4\n5\n").unwrap();
        let out = read_file(
            &root,
            ReadFileInput { file_path: "a.txt".into(), offset: Some(2), limit: Some(2) },
        )
        .unwrap();
        assert!(out.content.contains("L2: 2"));
        assert!(out.content.contains("L3: 3"));
        assert!(!out.content.contains("L1:"));
        assert!(!out.content.contains("L4:"));
    }

    #[test]
    fn read_file_rejects_escape() {
        let (_dir, root) = setup();
        assert!(read_file(
            &root,
            ReadFileInput { file_path: "../../etc/passwd".into(), offset: None, limit: None },
        )
        .is_err());
    }

    #[test]
    fn grep_finds_matches_across_files() {
        let (dir, root) = setup();
        fs::write(dir.path().join("a.ts"), "foo\nbar\n").unwrap();
        fs::write(dir.path().join("b.ts"), "baz\nfoo extra\n").unwrap();
        let out = grep(
            &root,
            GrepInput {
                pattern: "foo".into(),
                path: None,
                glob: None,
                limit: None,
                case_insensitive: false,
            },
        )
        .unwrap();
        assert_eq!(out.matches.len(), 2);
        assert!(!out.truncated);
    }

    #[test]
    fn grep_filters_by_glob() {
        let (dir, root) = setup();
        fs::write(dir.path().join("a.ts"), "hit\n").unwrap();
        fs::write(dir.path().join("a.md"), "hit\n").unwrap();
        let out = grep(
            &root,
            GrepInput {
                pattern: "hit".into(),
                path: None,
                glob: Some("*.ts".into()),
                limit: None,
                case_insensitive: false,
            },
        )
        .unwrap();
        assert_eq!(out.matches.len(), 1);
        assert!(out.matches[0].path.ends_with("a.ts"));
    }

    #[test]
    fn glob_matches_pattern() {
        let (dir, root) = setup();
        fs::create_dir(dir.path().join("examples")).unwrap();
        fs::write(dir.path().join("examples/stream.ts"), "").unwrap();
        fs::write(dir.path().join("examples/other.md"), "").unwrap();
        fs::write(dir.path().join("src.ts"), "").unwrap();
        let out = glob(
            &root,
            GlobInput {
                pattern: "examples/**/*.ts".into(),
                path: None,
                limit: None,
            },
        )
        .unwrap();
        assert_eq!(out.paths.len(), 1);
        assert!(out.paths[0].ends_with("examples/stream.ts"));
    }
}
