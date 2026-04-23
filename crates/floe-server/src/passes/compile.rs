//! Compile-unit evidence pass — drive the TypeScript compiler on
//! base and head, diff the diagnostics, emit a [`CompileDelta`] plus
//! per-flow claims.
//!
//! Primary-source replacement for v1's signature-consistency
//! heuristics. `tsc` is the type-checker ground truth.
//!
//! # Format
//!
//! We invoke `tsc --noEmit --pretty false --incremental false`.
//! Diagnostic lines in that mode look like
//! `src/foo.ts(10,12): error TS2322: Type 'x' is not assignable…`.
//!
//! # Classification
//!
//! Keyed by `(file, line, column, code)`. Present on head but not on
//! base → `new_on_head`. Reverse → `resolved_on_head`. Both → `persistent`.
//!
//! # Gating
//!
//! Skipped when `FLOE_COMPILE_PASS=0`, or when either side has no
//! `tsconfig.json` at the root. Respects a per-run timeout (default
//! 90s via `FLOE_COMPILE_TIMEOUT_SECS`); a slow tsc on a mega-repo
//! reports `both_ran: false` and the UI surfaces a caveat.

use std::path::Path;
use std::time::Duration;

use floe_core::{
    Artifact, Claim, ClaimKind, CompileDelta, CompileDiagnostic, DiagnosticSeverity, SourceRef,
    SourceSide, Strength,
};
use floe_core::provenance::Provenance;
use anyhow::{anyhow, Context, Result};
use tokio::process::Command;

const DEFAULT_TIMEOUT_SECS: u64 = 90;

/// Run the full compile pass on both sides, diff, return delta. None
/// when the pass is disabled or skipped (no tsconfig).
pub async fn run(base: &Path, head: &Path) -> Option<CompileDelta> {
    if std::env::var("FLOE_COMPILE_PASS")
        .ok()
        .map(|v| v == "0" || v.eq_ignore_ascii_case("false"))
        .unwrap_or(false)
    {
        tracing::info!("compile pass disabled (FLOE_COMPILE_PASS=0)");
        return None;
    }
    if !base.join("tsconfig.json").is_file() || !head.join("tsconfig.json").is_file() {
        tracing::info!("compile pass skipped — missing tsconfig.json on at least one side");
        return None;
    }
    let timeout = Duration::from_secs(
        std::env::var("FLOE_COMPILE_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_TIMEOUT_SECS),
    );

    let base_task = tokio::time::timeout(timeout, run_tsc(base));
    let head_task = tokio::time::timeout(timeout, run_tsc(head));
    let (base_res, head_res) = tokio::join!(base_task, head_task);

    let err_delta = |version: String| CompileDelta {
        compiler_version: version,
        new_on_head: vec![],
        resolved_on_head: vec![],
        persistent: vec![],
        both_ran: false,
    };

    let (base_diags, base_version) = match base_res {
        Ok(Ok(r)) => (r.diagnostics, r.version),
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "compile pass: tsc base failed");
            return Some(err_delta(String::new()));
        }
        Err(_) => {
            tracing::warn!("compile pass: base timed out after {}s", timeout.as_secs());
            return Some(err_delta(String::new()));
        }
    };
    let (head_diags, head_version) = match head_res {
        Ok(Ok(r)) => (r.diagnostics, r.version),
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "compile pass: tsc head failed");
            return Some(err_delta(base_version));
        }
        Err(_) => {
            tracing::warn!("compile pass: head timed out after {}s", timeout.as_secs());
            return Some(err_delta(base_version));
        }
    };

    let delta = diff(base_diags, head_diags);
    Some(CompileDelta {
        compiler_version: if !head_version.is_empty() {
            head_version
        } else {
            base_version
        },
        ..delta
    })
}

/// Attach per-flow claims derived from the artifact's
/// `compile_diagnostics`. Call after `run` has placed the delta on
/// the artifact.
pub fn attach_claims(artifact: &mut Artifact) {
    let Some(delta) = artifact.compile_diagnostics.clone() else {
        return;
    };
    if !delta.both_ran {
        return;
    }
    let prov = Provenance {
        source: "floe-server::passes::compile".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        pass_id: "compile-delta".into(),
        hash: String::new(),
    };
    // Snapshot nodes so mutable iteration over flows doesn't clash
    // with the shared read of `artifact.{base,head}.nodes`.
    let all_nodes: Vec<&floe_core::graph::Node> = artifact
        .head
        .nodes
        .iter()
        .chain(artifact.base.nodes.iter())
        .collect();
    for flow in artifact.flows.iter_mut() {
        let files: std::collections::HashSet<&str> = flow
            .entities
            .iter()
            .chain(flow.extra_entities.iter())
            .filter_map(|name| {
                all_nodes
                    .iter()
                    .find(|n| match &n.kind {
                        floe_core::graph::NodeKind::Function { name: n_name, .. }
                        | floe_core::graph::NodeKind::Type { name: n_name }
                        | floe_core::graph::NodeKind::State { name: n_name, .. } => {
                            n_name == name
                        }
                        _ => false,
                    })
                    .map(|n| n.file.as_str())
            })
            .collect();
        if files.is_empty() {
            continue;
        }
        let new_for_flow: Vec<&CompileDiagnostic> = delta
            .new_on_head
            .iter()
            .filter(|d| files.contains(d.file.as_str()))
            .collect();
        let resolved_for_flow: Vec<&CompileDiagnostic> = delta
            .resolved_on_head
            .iter()
            .filter(|d| files.contains(d.file.as_str()))
            .collect();

        if !new_for_flow.is_empty() {
            let preview = preview_list(&new_for_flow);
            flow.evidence.push(Claim {
                id: claim_id("compile-new", &flow.id, &preview),
                text: format!(
                    "head introduces {} new type error{}: {}",
                    new_for_flow.len(),
                    if new_for_flow.len() == 1 { "" } else { "s" },
                    preview
                ),
                kind: ClaimKind::SignatureConsistency,
                strength: Strength::High,
                entities: flow.entities.clone(),
                provenance: prov.clone(),
                source_refs: new_for_flow
                    .iter()
                    .map(|d| refs_for(d, SourceSide::Head))
                    .collect(),
            });
        }
        if !resolved_for_flow.is_empty() {
            let preview = preview_list(&resolved_for_flow);
            flow.evidence.push(Claim {
                id: claim_id("compile-resolved", &flow.id, &preview),
                text: format!(
                    "head fixes {} pre-existing type error{}: {}",
                    resolved_for_flow.len(),
                    if resolved_for_flow.len() == 1 { "" } else { "s" },
                    preview
                ),
                kind: ClaimKind::SignatureConsistency,
                strength: Strength::Medium,
                entities: flow.entities.clone(),
                provenance: prov.clone(),
                source_refs: resolved_for_flow
                    .iter()
                    .map(|d| refs_for(d, SourceSide::Base))
                    .collect(),
            });
        }
    }
}

fn refs_for(d: &CompileDiagnostic, side: SourceSide) -> SourceRef {
    SourceRef {
        file: d.file.clone(),
        side,
        line: d.line,
        column: d.column,
        length: None,
    }
}

fn preview_list(ds: &[&CompileDiagnostic]) -> String {
    ds.iter()
        .take(3)
        .map(|d| format!("{}:{} {}", d.file, d.line, d.code))
        .collect::<Vec<_>>()
        .join("; ")
}

struct TscResult {
    diagnostics: Vec<CompileDiagnostic>,
    version: String,
}

async fn run_tsc(root: &Path) -> Result<TscResult> {
    let version = tsc_version(root).await.unwrap_or_default();
    // Prefer `npx tsc` so we hit the repo's local TypeScript install
    // when available; the `--no-install` flag stops npx from pulling
    // anything from the registry mid-analysis.
    let npx = which_bin("npx");
    let mut cmd = match npx {
        Some(path_to_npx) => {
            let mut c = Command::new(&path_to_npx);
            c.args(["--no-install", "tsc"]);
            c
        }
        None => Command::new(
            which_bin("tsc").ok_or_else(|| anyhow!("neither npx nor tsc on PATH"))?,
        ),
    };
    cmd.args(["--noEmit", "--pretty", "false", "--incremental", "false"]);
    cmd.current_dir(root);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let output = cmd.output().await.context("spawn tsc")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let diagnostics = parse_diagnostics(&stdout, &stderr);
    Ok(TscResult { diagnostics, version })
}

async fn tsc_version(root: &Path) -> Option<String> {
    let npx = which_bin("npx")?;
    let mut cmd = Command::new(&npx);
    cmd.args(["--no-install", "tsc", "--version"]);
    cmd.current_dir(root);
    let output = cmd.output().await.ok()?;
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Some(s.trim_start_matches("Version").trim().to_string())
}

fn parse_diagnostics(stdout: &str, stderr: &str) -> Vec<CompileDiagnostic> {
    let mut out = Vec::new();
    for chunk in [stdout, stderr] {
        for line in chunk.lines() {
            if let Some(d) = parse_one(line) {
                out.push(d);
            }
        }
    }
    out
}

fn parse_one(line: &str) -> Option<CompileDiagnostic> {
    let (head, tail) = line.split_once(": ")?;
    let (file, row_col) = head.rsplit_once('(')?;
    let row_col = row_col.strip_suffix(')')?;
    let (row_str, col_str) = row_col.split_once(',')?;
    let line_no: u32 = row_str.trim().parse().ok()?;
    let col: u32 = col_str.trim().parse().ok()?;
    let (severity_code, message) = tail.split_once(": ")?;
    let (severity_str, code) = severity_code.split_once(' ')?;
    let severity = match severity_str {
        "error" => DiagnosticSeverity::Error,
        "warning" => DiagnosticSeverity::Warning,
        _ => return None,
    };
    Some(CompileDiagnostic {
        file: file.replace('\\', "/"),
        line: line_no,
        column: col,
        code: code.trim().to_string(),
        severity,
        message: message.trim().to_string(),
    })
}

fn diff(
    base: Vec<CompileDiagnostic>,
    head: Vec<CompileDiagnostic>,
) -> CompileDelta {
    use std::collections::HashSet;
    let key = |d: &CompileDiagnostic| -> (String, u32, u32, String) {
        (d.file.clone(), d.line, d.column, d.code.clone())
    };
    let base_keys: HashSet<_> = base.iter().map(key).collect();
    let head_keys: HashSet<_> = head.iter().map(key).collect();
    let new_on_head: Vec<CompileDiagnostic> = head
        .iter()
        .filter(|d| !base_keys.contains(&key(d)))
        .cloned()
        .collect();
    let resolved_on_head: Vec<CompileDiagnostic> = base
        .iter()
        .filter(|d| !head_keys.contains(&key(d)))
        .cloned()
        .collect();
    let persistent: Vec<CompileDiagnostic> = head
        .iter()
        .filter(|d| base_keys.contains(&key(d)))
        .cloned()
        .collect();
    CompileDelta {
        compiler_version: String::new(),
        new_on_head,
        resolved_on_head,
        persistent,
        both_ran: true,
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

fn claim_id(bucket: &str, flow_id: &str, preview: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(bucket.as_bytes());
    h.update(b"|");
    h.update(flow_id.as_bytes());
    h.update(b"|");
    h.update(preview.as_bytes());
    format!("claim-{}", h.finalize().to_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tsc_line_unix_style() {
        let line = "src/foo.ts(10,12): error TS2322: Type 'string' is not assignable to type 'number'.";
        let d = parse_one(line).unwrap();
        assert_eq!(d.file, "src/foo.ts");
        assert_eq!(d.line, 10);
        assert_eq!(d.column, 12);
        assert_eq!(d.code, "TS2322");
        assert!(matches!(d.severity, DiagnosticSeverity::Error));
    }

    #[test]
    fn parse_tsc_line_windows_backslash() {
        let line = "src\\foo.ts(10,12): error TS2322: Boom.";
        let d = parse_one(line).unwrap();
        assert_eq!(d.file, "src/foo.ts");
    }

    #[test]
    fn diff_new_resolved_persistent() {
        let a = vec![
            CompileDiagnostic {
                file: "a.ts".into(),
                line: 1,
                column: 1,
                code: "TS1".into(),
                severity: DiagnosticSeverity::Error,
                message: "".into(),
            },
            CompileDiagnostic {
                file: "b.ts".into(),
                line: 2,
                column: 1,
                code: "TS2".into(),
                severity: DiagnosticSeverity::Error,
                message: "".into(),
            },
        ];
        let b = vec![
            CompileDiagnostic {
                file: "a.ts".into(),
                line: 1,
                column: 1,
                code: "TS1".into(),
                severity: DiagnosticSeverity::Error,
                message: "".into(),
            },
            CompileDiagnostic {
                file: "c.ts".into(),
                line: 3,
                column: 1,
                code: "TS3".into(),
                severity: DiagnosticSeverity::Error,
                message: "".into(),
            },
        ];
        let delta = diff(a, b);
        assert_eq!(delta.new_on_head.len(), 1);
        assert_eq!(delta.new_on_head[0].code, "TS3");
        assert_eq!(delta.resolved_on_head.len(), 1);
        assert_eq!(delta.resolved_on_head[0].code, "TS2");
        assert_eq!(delta.persistent.len(), 1);
        assert_eq!(delta.persistent[0].code, "TS1");
    }
}
