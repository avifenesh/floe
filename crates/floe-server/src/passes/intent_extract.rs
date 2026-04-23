//! Intent extraction pass — when the reviewer hasn't supplied an
//! `intent.json` and no PR body was pulled, synthesise a structured
//! [`Intent`] from whatever signal we have about the PR: commit
//! messages, README, the changed filenames themselves.
//!
//! RFC Appendix F upgrade #5. Closes the "author-file-burden" gap —
//! most PRs ship without an intent file and we'd been leaving their
//! proof verdicts at `no-intent`. After this pass, every PR has
//! something for the proof pipeline to chew on.
//!
//! # Sources (in priority order)
//!
//! 1. **Git commit messages between base and head.** Richest signal
//!    when present — the author already described each step.
//! 2. **Repo README top 40 lines** — gives the model the project's
//!    own vocabulary so the extracted claims use the right words.
//! 3. **Changed filenames** — fallback shape when the repo is
//!    README-less and there's no git ancestry.
//!
//! We feed those three as a concatenated prompt to a local Qwen (by
//! default, per the Phase F pinning); GLM cloud is used only when
//! `FLOE_INTENT_LLM=glm:…` is set. Output is parsed as JSON into
//! [`Intent`].
//!
//! # When it doesn't run
//!
//! - `FLOE_INTENT_EXTRACT=0` → disabled.
//! - Artifact already has non-empty intent → nothing to do; we don't
//!   overwrite a reviewer-supplied intent.
//! - No model reachable → log a single warning, skip.

use std::path::Path;

use floe_core::intent::{Intent, IntentClaim, IntentInput, EvidenceType};
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::json;
use tokio::process::Command;

use crate::llm::config::{LlmConfig, LlmProvider};
use crate::llm::glm_client::GlmClient;
use crate::llm::ollama_client::{ChatMessage, ChatRequest, OllamaClient};

const SYSTEM: &str = "You read pull request signals and produce a structured \
intent. The reviewer uses this to verify whether the PR delivered what it \
claimed. Be faithful: do NOT invent features the signals don't mention. \
Strong preference for short, concrete, testable claims over ambitious prose. \
\
Return JSON ONLY in this exact shape:
{
  \"title\": \"<≤80 chars, imperative mood, describes the change>\",
  \"summary\": \"<1-3 sentences, plain English>\",
  \"claims\": [
    { \"statement\": \"<one thing the PR claims to do>\",
      \"evidence_type\": \"bench\" | \"example\" | \"test\" | \"observation\",
      \"detail\": \"<optional: where to look for proof>\" }
  ]
}

If you can't find enough signal to claim anything honestly, return \
claims:[] rather than fabricate.";

#[derive(Debug, Deserialize)]
struct ExtractedIntent {
    title: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    claims: Vec<ExtractedClaim>,
}

#[derive(Debug, Deserialize)]
struct ExtractedClaim {
    statement: String,
    #[serde(default)]
    evidence_type: Option<String>,
    #[serde(default)]
    detail: Option<String>,
}

pub async fn run(
    cfg: &LlmConfig,
    base: &Path,
    head: &Path,
) -> Option<IntentInput> {
    if std::env::var("FLOE_INTENT_EXTRACT")
        .ok()
        .map(|v| v == "0" || v.eq_ignore_ascii_case("false"))
        .unwrap_or(false)
    {
        tracing::info!("intent extraction disabled (FLOE_INTENT_EXTRACT=0)");
        return None;
    }

    let commit_log = gather_commit_log(base, head).await;
    let readme = gather_readme(head).await;
    let changed_files = gather_changed_files(base, head).await;
    if commit_log.is_none() && readme.is_none() && changed_files.is_empty() {
        tracing::info!("intent extraction: no signal available, skipping");
        return None;
    }

    let user = format_prompt(commit_log.as_deref(), readme.as_deref(), &changed_files);
    match extract(cfg, &user).await {
        Ok(intent) => Some(IntentInput::Structured(intent)),
        Err(e) => {
            tracing::warn!(error = %e, "intent extraction pass failed — leaving intent absent");
            None
        }
    }
}

async fn extract(cfg: &LlmConfig, user: &str) -> Result<Intent> {
    let req = ChatRequest {
        model: cfg.model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".into(),
                content: SYSTEM.into(),
                tool_calls: Vec::new(),
                tool_name: None,
            },
            ChatMessage {
                role: "user".into(),
                content: user.to_string(),
                tool_calls: Vec::new(),
                tool_name: None,
            },
        ],
        tools: Vec::new(),
        stream: false,
        options: Some(json!({
            "temperature": cfg.temperature,
            "num_predict": cfg.num_predict,
            "num_ctx": cfg.num_ctx,
        })),
        keep_alive: Some(cfg.keep_alive.clone()),
    };
    let content = match cfg.provider {
        LlmProvider::Glm => {
            let key = cfg
                .api_key
                .clone()
                .ok_or_else(|| anyhow!("FLOE_GLM_API_KEY required for intent extraction on GLM"))?;
            let client = GlmClient::new(cfg.base_url.clone(), key);
            client
                .chat(req)
                .await
                .context("glm intent extract")?
                .message
                .content
        }
        LlmProvider::Ollama => {
            let client = OllamaClient::new(cfg.base_url.clone());
            client
                .chat(req)
                .await
                .context("ollama intent extract")?
                .message
                .content
        }
    };
    let body = extract_json_object(content.trim())
        .ok_or_else(|| anyhow!("intent-extract response had no JSON: {content}"))?;
    let raw: ExtractedIntent =
        serde_json::from_str(body).with_context(|| format!("intent parse: {body}"))?;
    let claims: Vec<IntentClaim> = raw
        .claims
        .into_iter()
        .filter_map(|c| {
            if c.statement.trim().is_empty() {
                return None;
            }
            let evidence_type = match c
                .evidence_type
                .as_deref()
                .map(|s| s.to_ascii_lowercase())
                .as_deref()
            {
                Some("bench") => EvidenceType::Bench,
                Some("example") => EvidenceType::Example,
                Some("test") => EvidenceType::Test,
                Some("observation") | None | Some("") => EvidenceType::Observation,
                _ => EvidenceType::Observation,
            };
            Some(IntentClaim {
                statement: c.statement,
                evidence_type,
                detail: c.detail.unwrap_or_default(),
            })
        })
        .collect();
    Ok(Intent {
        title: raw.title,
        summary: raw.summary,
        claims,
    })
}

fn format_prompt(
    commits: Option<&str>,
    readme: Option<&str>,
    changed: &[String],
) -> String {
    let mut out = String::new();
    if let Some(c) = commits {
        out.push_str("Commit messages between base and head:\n");
        out.push_str(c);
        out.push_str("\n\n");
    }
    if let Some(r) = readme {
        out.push_str("README excerpt (for project vocabulary):\n");
        out.push_str(r);
        out.push_str("\n\n");
    }
    if !changed.is_empty() {
        out.push_str("Changed files:\n");
        for (i, f) in changed.iter().take(60).enumerate() {
            out.push_str(&format!("  {}. {}\n", i + 1, f));
        }
        if changed.len() > 60 {
            out.push_str(&format!("  … ({} more)\n", changed.len() - 60));
        }
    }
    out
}

async fn gather_commit_log(base: &Path, head: &Path) -> Option<String> {
    // Only meaningful when both sides are under the same git repo and
    // we can resolve a merge-base between them. Use git `rev-list` at
    // `head` side; the `base` path might be a detached worktree.
    let head_sha = git_head_sha(head).await?;
    let base_sha = git_head_sha(base).await?;
    let git = which_bin("git")?;
    let mut cmd = Command::new(&git);
    cmd.args(["log", "--format=%B", &format!("{base_sha}..{head_sha}")]);
    cmd.current_dir(head);
    let output = cmd.output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    let log = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if log.is_empty() {
        return None;
    }
    Some(truncate_to(&log, 8_000))
}

async fn git_head_sha(dir: &Path) -> Option<String> {
    let git = which_bin("git")?;
    let mut cmd = Command::new(&git);
    cmd.args(["rev-parse", "HEAD"]);
    cmd.current_dir(dir);
    let output = cmd.output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string(),
    )
}

async fn gather_readme(dir: &Path) -> Option<String> {
    for name in ["README.md", "README", "readme.md", "Readme.md"] {
        let path = dir.join(name);
        if let Ok(text) = tokio::fs::read_to_string(&path).await {
            let lines: Vec<&str> = text.lines().take(40).collect();
            return Some(lines.join("\n"));
        }
    }
    None
}

async fn gather_changed_files(base: &Path, head: &Path) -> Vec<String> {
    let (Some(base_sha), Some(head_sha)) = (git_head_sha(base).await, git_head_sha(head).await)
    else {
        return Vec::new();
    };
    let Some(git) = which_bin("git") else {
        return Vec::new();
    };
    let mut cmd = Command::new(&git);
    cmd.args(["diff", "--name-only", &format!("{base_sha}..{head_sha}")]);
    cmd.current_dir(head);
    let Ok(output) = cmd.output().await else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn truncate_to(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect()
}

fn extract_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    let bytes = s.as_bytes();
    for i in start..bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_format_omits_empty_sections() {
        let s = format_prompt(None, None, &[]);
        assert_eq!(s, "");
    }

    #[test]
    fn prompt_format_lists_changed_files() {
        let s = format_prompt(
            None,
            None,
            &["src/a.ts".into(), "src/b.ts".into()],
        );
        assert!(s.contains("Changed files:"));
        assert!(s.contains("src/a.ts"));
        assert!(s.contains("src/b.ts"));
    }

    #[test]
    fn extract_json_tolerates_prose_wrapping() {
        let r = extract_json_object("here you go:\n```\n{\"x\": 1}\n```");
        assert_eq!(r, Some("{\"x\": 1}"));
    }
}
