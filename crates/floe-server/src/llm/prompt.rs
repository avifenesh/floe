//! Load the flow-synthesis prompt from disk and expand `{{placeholders}}`.
//!
//! Prompt lives at `prompts/flow_synthesis/<version>/flow_synthesis.md`
//! relative to the repo root. The repo root is discovered by walking up
//! from the current working directory until a `Cargo.toml` with a
//! `[workspace]` section is found; override via `FLOE_REPO_ROOT`.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

pub struct RenderedPrompt {
    pub version: String,
    pub body: String,
}

/// Placeholders the prompt accepts. Keep in lockstep with the
/// `flow_synthesis.about.md` declaration.
#[derive(Debug, Clone)]
pub struct PromptInputs<'a> {
    pub version: &'a str,
    pub hunk_count: usize,
    pub initial_cluster_count: usize,
    pub max_tool_calls: u32,
}

pub fn render(inputs: PromptInputs<'_>) -> Result<RenderedPrompt> {
    let root = repo_root()?;
    let path = root
        .join("prompts")
        .join("flow_synthesis")
        .join(inputs.version)
        .join("flow_synthesis.md");
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading prompt {}", path.display()))?;
    let body = raw
        .replace("{{hunk_count}}", &inputs.hunk_count.to_string())
        .replace(
            "{{initial_cluster_count}}",
            &inputs.initial_cluster_count.to_string(),
        )
        .replace("{{max_tool_calls}}", &inputs.max_tool_calls.to_string());
    Ok(RenderedPrompt {
        version: inputs.version.to_string(),
        body,
    })
}

fn repo_root() -> Result<PathBuf> {
    if let Ok(r) = std::env::var("FLOE_REPO_ROOT") {
        return Ok(PathBuf::from(r));
    }
    let start = std::env::current_dir()?;
    let mut cur: &Path = &start;
    loop {
        let candidate = cur.join("Cargo.toml");
        if candidate.is_file() {
            if let Ok(s) = std::fs::read_to_string(&candidate) {
                if s.contains("[workspace]") {
                    return Ok(cur.to_path_buf());
                }
            }
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => {
                return Err(anyhow!(
                    "could not locate repo root (workspace Cargo.toml) from {}",
                    start.display()
                ));
            }
        }
    }
}
