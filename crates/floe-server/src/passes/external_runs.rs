//! External-runs evidence pass — run the reviewer-configured test or
//! bench command on base and head, capture the outcome, emit a
//! [`ExternalRunDelta`] plus per-flow claims.
//!
//! RFC Appendix F upgrade #4. Closes the PERF (bench) and LOCK
//! (tests) evidence classes from RFC §8.
//!
//! # Configuration
//!
//! - `FLOE_TEST_CMD` — shell-free command string split on whitespace,
//!   e.g. `vitest run` or `cargo test --no-run`. Set `FLOE_TEST_CMD=`
//!   (empty) to skip.
//! - `FLOE_BENCH_CMD` — same shape.
//! - `FLOE_EXTERNAL_TIMEOUT_SECS` — per-side wall-clock cap (default 180).
//! - `FLOE_EXTERNAL_MAX_BYTES` — stdout/stderr truncation threshold
//!   (default 65536).
//!
//! We deliberately don't shell-interpret the command — `bash -c foo |
//! bar` is a no-go. Runners that need pipelines should use wrapper
//! scripts. This keeps the attack surface narrow (no shell injection
//! even if a sample or URL-driven repo defines an unwanted command).

use std::path::Path;
use std::time::{Duration, Instant};

use floe_core::{
    Artifact, Claim, ClaimKind, ExternalRunDelta, ExternalRunOutcome, Strength,
};
use floe_core::provenance::Provenance;
use tokio::process::Command;

const DEFAULT_TIMEOUT_SECS: u64 = 180;
const DEFAULT_MAX_BYTES: usize = 64 * 1024;

/// Which of the two user-configurable runners we're executing.
#[derive(Clone, Copy, Debug)]
pub enum RunKind {
    Tests,
    Bench,
}

impl RunKind {
    fn env_key(self) -> &'static str {
        match self {
            RunKind::Tests => "FLOE_TEST_CMD",
            RunKind::Bench => "FLOE_BENCH_CMD",
        }
    }
    fn label(self) -> &'static str {
        match self {
            RunKind::Tests => "tests",
            RunKind::Bench => "bench",
        }
    }
}

pub async fn run(kind: RunKind, base: &Path, head: &Path) -> Option<ExternalRunDelta> {
    let cmd_str = std::env::var(kind.env_key())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;
    let timeout = Duration::from_secs(
        std::env::var("FLOE_EXTERNAL_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_TIMEOUT_SECS),
    );
    let max_bytes: usize = std::env::var("FLOE_EXTERNAL_MAX_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MAX_BYTES);

    let parts: Vec<String> = cmd_str
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();
    if parts.is_empty() {
        return None;
    }

    let base_task = tokio::time::timeout(timeout, run_once(base, &parts, max_bytes));
    let head_task = tokio::time::timeout(timeout, run_once(head, &parts, max_bytes));
    let (base_res, head_res) = tokio::join!(base_task, head_task);

    let base_outcome = match base_res {
        Ok(Ok(o)) => Some(o),
        _ => None,
    };
    let head_outcome = match head_res {
        Ok(Ok(o)) => Some(o),
        _ => None,
    };
    let both_ran = base_outcome.is_some() && head_outcome.is_some();
    tracing::info!(
        kind = kind.label(),
        both_ran,
        base_exit = base_outcome.as_ref().map(|o| o.exit_code),
        head_exit = head_outcome.as_ref().map(|o| o.exit_code),
        "external run finished"
    );
    Some(ExternalRunDelta {
        command: cmd_str,
        base: base_outcome,
        head: head_outcome,
        both_ran,
    })
}

async fn run_once(
    cwd: &Path,
    parts: &[String],
    max_bytes: usize,
) -> anyhow::Result<ExternalRunOutcome> {
    let program = which_bin(&parts[0])
        .ok_or_else(|| anyhow::anyhow!("{} not on PATH", parts[0]))?;
    let mut cmd = Command::new(&program);
    if parts.len() > 1 {
        cmd.args(&parts[1..]);
    }
    cmd.current_dir(cwd);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let start = Instant::now();
    let output = cmd.output().await?;
    let duration_ms = start.elapsed().as_millis() as u64;
    let (stdout, stdout_truncated) = truncate(&output.stdout, max_bytes);
    let (stderr, stderr_truncated) = truncate(&output.stderr, max_bytes);
    Ok(ExternalRunOutcome {
        exit_code: output.status.code().unwrap_or(-1),
        duration_ms,
        stdout,
        stderr,
        truncated: stdout_truncated || stderr_truncated,
    })
}

fn truncate(raw: &[u8], max: usize) -> (String, bool) {
    if raw.len() <= max {
        (String::from_utf8_lossy(raw).into_owned(), false)
    } else {
        (String::from_utf8_lossy(&raw[..max]).into_owned(), true)
    }
}

fn which_bin(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: &[&str] = if cfg!(windows) {
        &[".cmd", ".bat", ".exe", ""]
    } else {
        &[""]
    };
    for dir in std::env::split_paths(&path) {
        for ext in exts {
            let candidate = if ext.is_empty() {
                dir.join(name)
            } else {
                dir.join(format!("{name}{ext}"))
            };
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Attach per-flow claims based on test/bench deltas.
///
/// Tests: a transition from pass→fail between base and head is a
/// high-strength `TestCoverage` claim; fail→pass is a medium positive.
/// Identical outcomes on both sides aren't claims (nothing changed
/// from the perspective of the test command).
///
/// Bench: we don't claim anything yet — structured bench comparison
/// needs a runner-specific parser. The raw outcome is persisted on
/// the artifact so the proof pass can mine it.
pub fn attach_claims(artifact: &mut Artifact) {
    let Some(test_delta) = artifact.test_run.clone() else {
        return;
    };
    if !test_delta.both_ran {
        return;
    }
    let (Some(base), Some(head)) = (&test_delta.base, &test_delta.head) else {
        return;
    };
    if base.exit_code == head.exit_code {
        return;
    }
    let prov = Provenance {
        source: "floe-server::passes::external_runs".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        pass_id: "test-run-delta".into(),
        hash: String::new(),
    };
    let (kind, strength, text) = if base.exit_code == 0 && head.exit_code != 0 {
        (
            "tests broken by head",
            Strength::High,
            format!(
                "tests pass on base (0) but fail on head ({}). command: `{}`",
                head.exit_code, test_delta.command
            ),
        )
    } else if base.exit_code != 0 && head.exit_code == 0 {
        (
            "tests fixed by head",
            Strength::Medium,
            format!(
                "tests fail on base ({}) but pass on head (0). command: `{}`",
                base.exit_code, test_delta.command
            ),
        )
    } else {
        return;
    };
    // Parse failing-test files from the head run's stdout so we can
    // bucket claims per flow when possible. Falls back to PR-wide
    // attribution when parsing yields nothing.
    let failing_files: Vec<String> = if head.exit_code != 0 {
        parse_failing_test_files(&head.stdout)
            .into_iter()
            .chain(parse_failing_test_files(&head.stderr))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    } else {
        Vec::new()
    };
    let attr_mode = !failing_files.is_empty();
    for flow in artifact.flows.iter_mut() {
        if attr_mode {
            let flow_touches_failing = flow
                .entities
                .iter()
                .chain(flow.extra_entities.iter())
                .any(|e| {
                    artifact.head.nodes.iter().any(|n| match &n.kind {
                        floe_core::NodeKind::Function { name, .. }
                        | floe_core::NodeKind::Type { name }
                        | floe_core::NodeKind::State { name, .. } => {
                            name == e && failing_files.iter().any(|f| &n.file == f)
                        }
                        _ => false,
                    })
                });
            if !flow_touches_failing {
                continue;
            }
        }
        flow.evidence.push(Claim {
            id: claim_id(kind, &flow.id),
            text: text.clone(),
            kind: ClaimKind::TestCoverage,
            strength,
            entities: flow.entities.clone(),
            provenance: prov.clone(),
            // Bench / test-level claim is command-global — no
            // source coordinates to anchor. Future B2.1 (parse
            // vitest/jest output) can fill these with failing-test
            // file/line.
            source_refs: Vec::new(),
        });
    }
}

/// Extract failing-test file paths from vitest / jest stdout.
///
/// vitest: lines like ` FAIL  src/foo.test.ts > describe > it` or
///   `❯ src/foo.test.ts (5 tests | 2 failed)`.
/// jest: lines starting with `FAIL ` followed by a path.
///
/// Heuristic only — noise lines that don't resolve to a real file
/// are harmless since downstream code only matches against
/// `Graph.head.nodes[*].file`.
pub fn parse_failing_test_files(output: &str) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    for raw in output.lines() {
        let line = raw.trim_start();
        // Strip ANSI color codes cheaply — `\x1b[...m`.
        let line = strip_ansi(line);
        let line = line.trim_start();
        for tok in ["FAIL  ", "FAIL ", "❯ ", "× "] {
            if let Some(rest) = line.strip_prefix(tok) {
                if let Some(path) = extract_path(rest) {
                    if path.contains(".test.")
                        || path.contains(".spec.")
                        || path.ends_with(".test.ts")
                        || path.ends_with(".test.tsx")
                        || path.ends_with(".spec.ts")
                        || path.ends_with(".spec.tsx")
                    {
                        out.insert(path.replace('\\', "/"));
                    }
                }
            }
        }
    }
    out.into_iter().collect()
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            while let Some(&nc) = chars.peek() {
                chars.next();
                if nc.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn extract_path(rest: &str) -> Option<String> {
    let end = rest.find([' ', '>', '(']).unwrap_or(rest.len());
    let candidate = rest[..end].trim();
    if candidate.is_empty() {
        None
    } else {
        Some(candidate.to_string())
    }
}

fn claim_id(kind: &str, flow_id: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(kind.as_bytes());
    h.update(b"|");
    h.update(flow_id.as_bytes());
    format!("claim-{}", h.finalize().to_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vitest_fail_lines() {
        let out = " FAIL  src/foo.test.ts > describe > it\n❯ src/bar.spec.ts (3 tests | 1 failed)\n";
        let files = parse_failing_test_files(out);
        assert!(files.contains(&"src/foo.test.ts".to_string()));
        assert!(files.contains(&"src/bar.spec.ts".to_string()));
    }

    #[test]
    fn parses_jest_fail_lines() {
        let out = "FAIL packages/ui/Button.test.tsx\nPASS packages/ui/Label.test.tsx\n";
        let files = parse_failing_test_files(out);
        assert_eq!(files, vec!["packages/ui/Button.test.tsx".to_string()]);
    }

    #[test]
    fn truncate_bounds_output() {
        let raw = vec![b'x'; 100_000];
        let (s, tr) = truncate(&raw, 1024);
        assert!(tr);
        assert_eq!(s.len(), 1024);
        let short = b"hello".to_vec();
        let (s, tr) = truncate(&short, 1024);
        assert!(!tr);
        assert_eq!(s, "hello");
    }
}
