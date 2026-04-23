//! File-access tools for the proof LLM pass: `read_file`, `grep`, `glob`.
//!
//! This module is a thin shim over the [`harness-tools`] crate — Avi's
//! benchmarked agent-tool implementations. The shim handles three
//! concerns: build a per-session config (cwd + permission policy
//! pinned to the tools root), await the async harness entry point,
//! and serialize the discriminated-union result back to a `Value`
//! for the MCP wire format.
//!
//! The tools are still surfaced only when the MCP server runs with
//! `--proof` — probe sessions keep a tight navigation-only toolbox.
//!
//! The LLM-facing schemas follow harness-tools' contract (richer than
//! the previous ad-hoc shapes): `pattern`, `path`, `glob`,
//! `output_mode`, `head_limit`, `offset`, `context`,
//! `case_insensitive`, `multiline` for grep; `file_path`, `offset`,
//! `limit` for read; `pattern`, `path`, `head_limit`, `offset` for
//! glob. Per-call parsing is delegated to harness — it returns
//! explicit error results with alias hints when the LLM sends the
//! wrong field name, which lands back to the model as a useful
//! correction rather than a cryptic 400.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use harness_tools::core::PermissionPolicy;
use harness_tools::glob::{glob as harness_glob_run, GlobSessionConfig};
use harness_tools::grep::{grep as harness_grep_run, GrepSessionConfig};
use harness_tools::read::{read as harness_read_run, ReadSessionConfig};
use serde_json::Value;

/// Per-session root the fs tools operate against. Every path the LLM
/// sends resolves underneath this root via the harness
/// [`PermissionPolicy`].
#[derive(Debug, Clone)]
pub struct ToolsRoot {
    root: PathBuf,
    cwd: String,
}

impl ToolsRoot {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root
            .into()
            .canonicalize()
            .map_err(|e| anyhow!("canonicalize tools root: {e}"))?;
        // harness-tools 0.1.1's fence tolerates `/` or `\\` as a
        // separator, but the root must be in the same *canonical form*
        // as the path its internal resolver produces. On Windows that
        // form keeps the `\\?\` UNC prefix + backslashes — so we pass
        // the raw canonicalised string through verbatim. On POSIX
        // canonicalize returns plain absolute paths; same story.
        let cwd = root.to_string_lossy().into_owned();
        Ok(Self { root, cwd })
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    fn permissions(&self) -> PermissionPolicy {
        // harness-tools 0.1.1+ accepts either `/` or `\\` as a
        // separator in its workspace fence, so no Windows bypass is
        // needed — the session cwd (canonicalised, normalised to
        // forward slashes) is the one source of truth for the
        // allowed root.
        PermissionPolicy::new([self.cwd.clone()])
    }

    fn read_session(&self) -> ReadSessionConfig {
        ReadSessionConfig::new(self.cwd.clone(), self.permissions())
    }

    fn grep_session(&self) -> GrepSessionConfig {
        GrepSessionConfig::new(self.cwd.clone(), self.permissions())
    }

    fn glob_session(&self) -> GlobSessionConfig {
        GlobSessionConfig::new(self.cwd.clone(), self.permissions())
    }
}

// ─────────────────────────────────────────────────────────────────────
// read_file / grep / glob — all forward to harness-tools
// ─────────────────────────────────────────────────────────────────────

/// Read a file. Accepts harness `ReadParams` as JSON (`file_path`,
/// `offset`, `limit`). Returns harness `ReadResult` serialized to
/// `Value` (discriminated union with `kind = text | directory |
/// attachment | error`).
pub async fn read_file(root: &ToolsRoot, args: Value) -> Value {
    let result = harness_read_run(args, &root.read_session()).await;
    serde_json::to_value(result).unwrap_or(Value::Null)
}

/// Grep across files. Harness `GrepParams` shape: `pattern` (required),
/// `path`, `glob`, `type`, `output_mode`, `case_insensitive`,
/// `multiline`, `context`, `head_limit`, `offset`. Discriminated
/// result with `kind = files_with_matches | content | count | error`.
pub async fn grep(root: &ToolsRoot, args: Value) -> Value {
    let result = harness_grep_run(args, &root.grep_session()).await;
    serde_json::to_value(result).unwrap_or(Value::Null)
}

/// Glob filenames. Harness `GlobParams`: `pattern` (required),
/// `path`, `head_limit`, `offset`. Returns `kind = paths | error`.
pub async fn glob(root: &ToolsRoot, args: Value) -> Value {
    let result = harness_glob_run(args, &root.glob_session()).await;
    serde_json::to_value(result).unwrap_or(Value::Null)
}

// ─────────────────────────────────────────────────────────────────────
// JSON-Schema descriptors — fed to the LLM as tool definitions
// ─────────────────────────────────────────────────────────────────────

/// Tool descriptors advertised to the LLM. Mirrors the harness-tools
/// wire contract so prompts can rely on its richer parameter set
/// (alias hints, offset pagination, content/count/files output modes).
pub fn tool_schemas() -> serde_json::Value {
    serde_json::json!({
        "floe.read_file": {
            "type": "object",
            "description": "Read a file with 1-indexed pagination. Returns either text (with numbered lines), a directory listing, an attachment, or an error with fuzzy sibling suggestions when the path is wrong.",
            "properties": {
                "path": {"type": "string", "description": "Path relative to the session root, or absolute inside the root."},
                "offset": {"type": "integer", "minimum": 1, "description": "1-indexed start line. Default 1."},
                "limit": {"type": "integer", "minimum": 1, "description": "Max lines to return. Default set by session."}
            },
            "required": ["path"],
            "additionalProperties": false
        },
        "floe.grep": {
            "type": "object",
            "description": "Search files with a Rust-regex pattern (ripgrep-backed, respects .gitignore). output_mode chooses between content (matched lines with context), count (matches per file), or files_with_matches (file paths only).",
            "properties": {
                "pattern": {"type": "string", "description": "Regex pattern."},
                "path": {"type": "string", "description": "Optional subpath to scope the search."},
                "glob": {"type": "string", "description": "Optional glob (e.g. '*.ts')."},
                "type": {"type": "string", "description": "Optional ripgrep file type (e.g. 'ts', 'rust')."},
                "output_mode": {"type": "string", "enum": ["content", "count", "files_with_matches"], "description": "Result shape. Default files_with_matches."},
                "case_insensitive": {"type": "boolean"},
                "multiline": {"type": "boolean"},
                "context": {"type": "integer", "minimum": 0, "description": "Lines of context around each match."},
                "context_before": {"type": "integer", "minimum": 0},
                "context_after": {"type": "integer", "minimum": 0},
                "head_limit": {"type": "integer", "minimum": 1, "description": "Cap on results."},
                "offset": {"type": "integer", "minimum": 0}
            },
            "required": ["pattern"],
            "additionalProperties": false
        },
        "floe.glob": {
            "type": "object",
            "description": "List files matching a glob pattern (respects .gitignore). Paths are returned relative to the session root.",
            "properties": {
                "pattern": {"type": "string", "description": "Glob pattern (e.g. 'examples/**/*.ts')."},
                "path": {"type": "string", "description": "Optional subpath to scope the walk."},
                "head_limit": {"type": "integer", "minimum": 1, "description": "Max paths returned."},
                "offset": {"type": "integer", "minimum": 0}
            },
            "required": ["pattern"],
            "additionalProperties": false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (TempDir, ToolsRoot) {
        let dir = TempDir::new().unwrap();
        let r = ToolsRoot::new(dir.path()).unwrap();
        (dir, r)
    }

    #[tokio::test]
    async fn read_file_returns_text_result() {
        let (dir, root) = setup();
        fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
        let out = read_file(&root, json!({ "path": "a.txt" })).await;
        let kind = out.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(kind, "text", "got {out}");
        let body = out.get("output").and_then(|v| v.as_str()).unwrap_or("");
        assert!(body.contains("one"));
        assert!(body.contains("two"));
        assert!(body.contains("three"));
    }

    #[tokio::test]
    async fn read_file_rejects_escape() {
        let (_dir, root) = setup();
        let out = read_file(&root, json!({ "path": "../../etc/passwd" })).await;
        let kind = out.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(kind, "error", "escape must error: {out}");
    }

    #[tokio::test]
    async fn grep_finds_content() {
        let (dir, root) = setup();
        fs::write(dir.path().join("a.ts"), "foo\nbar\n").unwrap();
        fs::write(dir.path().join("b.ts"), "baz\nfoo extra\n").unwrap();
        let out = grep(
            &root,
            json!({ "pattern": "foo", "output_mode": "content" }),
        )
        .await;
        let kind = out.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(kind, "content", "got {out}");
        let body = out.get("output").and_then(|v| v.as_str()).unwrap_or("");
        assert!(body.contains("foo"));
    }

    #[tokio::test]
    async fn grep_filters_by_glob() {
        let (dir, root) = setup();
        fs::write(dir.path().join("a.ts"), "hit\n").unwrap();
        fs::write(dir.path().join("a.md"), "hit\n").unwrap();
        let out = grep(
            &root,
            json!({ "pattern": "hit", "glob": "*.ts", "output_mode": "files_with_matches" }),
        )
        .await;
        let paths = out
            .get("paths")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(paths.iter().any(|p| p.as_str().unwrap_or("").ends_with("a.ts")));
        assert!(!paths.iter().any(|p| p.as_str().unwrap_or("").ends_with("a.md")));
    }

    #[tokio::test]
    async fn glob_matches_pattern() {
        let (dir, root) = setup();
        fs::create_dir(dir.path().join("examples")).unwrap();
        fs::write(dir.path().join("examples/stream.ts"), "").unwrap();
        fs::write(dir.path().join("examples/other.md"), "").unwrap();
        fs::write(dir.path().join("src.ts"), "").unwrap();
        let out = glob(&root, json!({ "pattern": "examples/**/*.ts" })).await;
        let kind = out.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(kind, "paths", "got {out}");
        let paths = out
            .get("paths")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(paths
            .iter()
            .any(|p| p.as_str().unwrap_or("").ends_with("examples/stream.ts") || p.as_str().unwrap_or("").ends_with("examples\\stream.ts")));
    }
}
