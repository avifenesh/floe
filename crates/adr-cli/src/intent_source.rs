//! Resolve a CLI-supplied intent into an [`adr_core::IntentInput`].
//!
//! Three sources, in order of preference:
//!
//! 1. `--intent-file <path>` — either a structured `Intent` JSON (loaded
//!    via `IntentInput::Structured`) or a plain-text PR description
//!    (loaded via `IntentInput::RawText`). Detected by extension; any
//!    non-`.json` extension is treated as raw text.
//! 2. `--intent-pr <n> [--intent-repo owner/name]` — shells out to
//!    `gh pr view <n> --json title,body,url --repo <repo>` and folds the
//!    body into raw-text intent. Title becomes the first line so the
//!    structuring pass has something to latch onto.
//! 3. Nothing — returns `Ok(None)`, downstream passes emit a
//!    "no-intent" claim and the proof axis stays zero.

use std::path::Path;
use std::process::Command;

use adr_core::{Intent, IntentInput};
use anyhow::{bail, Context, Result};

pub fn resolve(
    file: Option<&Path>,
    pr: Option<u64>,
    repo: Option<&str>,
) -> Result<Option<IntentInput>> {
    if let Some(path) = file {
        return Ok(Some(from_file(path)?));
    }
    if let Some(number) = pr {
        return Ok(Some(from_gh(number, repo)?));
    }
    Ok(None)
}

fn from_file(path: &Path) -> Result<IntentInput> {
    let is_json = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.eq_ignore_ascii_case("json"))
        .unwrap_or(false);
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading intent file {}", path.display()))?;
    if is_json {
        let parsed: Intent = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing intent JSON {}", path.display()))?;
        Ok(IntentInput::Structured(parsed))
    } else {
        let text = String::from_utf8(bytes)
            .with_context(|| format!("intent file {} is not UTF-8", path.display()))?;
        Ok(IntentInput::RawText(text))
    }
}

fn from_gh(number: u64, repo: Option<&str>) -> Result<IntentInput> {
    let mut cmd = Command::new("gh");
    cmd.arg("pr").arg("view").arg(number.to_string());
    cmd.arg("--json").arg("title,body,url");
    if let Some(r) = repo {
        cmd.arg("--repo").arg(r);
    }
    let output = cmd
        .output()
        .context("running `gh pr view` — is the GitHub CLI installed and authenticated?")?;
    if !output.status.success() {
        bail!(
            "`gh pr view {number}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let v: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("parsing `gh pr view` JSON output")?;
    let title = v.get("title").and_then(|s| s.as_str()).unwrap_or("").trim();
    let body = v.get("body").and_then(|s| s.as_str()).unwrap_or("").trim();
    let url = v.get("url").and_then(|s| s.as_str()).unwrap_or("").trim();
    let mut text = String::new();
    if !title.is_empty() {
        text.push_str(&format!("# {title}\n\n"));
    }
    if !url.is_empty() {
        text.push_str(&format!("<{url}>\n\n"));
    }
    text.push_str(body);
    if text.trim().is_empty() {
        bail!("`gh pr view {number}` returned no title/body/url — nothing to feed as intent");
    }
    Ok(IntentInput::RawText(text))
}
