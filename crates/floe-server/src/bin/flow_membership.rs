//! `floe-flow-membership` — experimental probe binary.
//!
//! Runs the flow-membership LLM session once against a cached
//! artifact. Prints the raw model response to stdout; logs every
//! prompt / tool call / tool response / final content to stderr.
//!
//! Usage:
//!
//! ```text
//! floe-flow-membership \
//!     --artifact path/to/artifact.json \
//!     --head path/to/head/snapshot \
//!     --flow-id flow-abcdef...
//! ```
//!
//! Requires `FLOE_GLM_API_KEY` in the environment (or `.env`). The
//! model defaults to `glm-4.7` regardless of other `FLOE_*_LLM`
//! knobs — this probe is specifically measuring 4.7.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use clap::Parser;

use floe_core::Artifact;
use floe_server::llm::config::{LlmConfig, LlmProvider};
use floe_server::llm::flow_membership;

#[derive(Parser)]
#[command(
    name = "floe-flow-membership",
    about = "Experimental — probe GLM-4.7 for flow-membership curation on one flow."
)]
struct Cli {
    /// Path to a cached artifact JSON (typically under `.floe/cache/`).
    #[arg(long)]
    artifact: PathBuf,
    /// Head-snapshot filesystem root — the MCP fs tools resolve paths
    /// against this. Usually `<cache>/git/head/<sha>/`.
    #[arg(long)]
    head: PathBuf,
    /// Which flow to curate. Pass the flow id as emitted in the
    /// artifact. When omitted, the first flow is used.
    #[arg(long)]
    flow_id: Option<String>,
    /// Override the GLM model tag. Defaults to glm-4.7.
    #[arg(long, default_value = "glm-4.7")]
    model: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::from_path(".env");
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new(
                    "flow_membership=info,floe_server=info,warn",
                )
            }),
        )
        .init();

    let cli = Cli::parse();

    let bytes = std::fs::read(&cli.artifact)
        .with_context(|| format!("reading {}", cli.artifact.display()))?;
    let artifact: Artifact =
        serde_json::from_slice(&bytes).context("parsing artifact JSON")?;

    let flow_id = match cli.flow_id.as_deref() {
        Some(id) => id.to_string(),
        None => artifact
            .flows
            .first()
            .map(|f| f.id.clone())
            .ok_or_else(|| anyhow!("artifact has no flows"))?,
    };

    let api_key = std::env::var("FLOE_GLM_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("FLOE_GLM_API_KEY must be set"))?;

    let base_url = std::env::var("FLOE_GLM_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            floe_server::llm::glm_client::default_base_url().to_string()
        });

    // Minimal LlmConfig — we only need fields the probe reads.
    let llm_cfg = LlmConfig {
        provider: LlmProvider::Glm,
        model: cli.model,
        base_url,
        api_key: Some(api_key),
        prompt_version: "0.3.0".to_string(),
        num_ctx: 131072,
        num_predict: 4096,
        temperature: 0.3,
        keep_alive: "10m".into(),
    };

    let raw = flow_membership::probe(
        &artifact,
        &flow_id,
        &llm_cfg,
        &cli.head,
        &cli.artifact,
        None,
    )
    .await?;

    println!("---- raw ----");
    println!("{raw}");

    match flow_membership::parse_response(&raw) {
        Ok(parsed) => {
            let sanitized = flow_membership::sanitize(parsed);
            println!("---- parsed + sanitized ----");
            let pretty = serde_json::to_string_pretty(&sanitized)?;
            println!("{pretty}");
        }
        Err(e) => {
            eprintln!("parse failed: {e:#}");
        }
    }
    Ok(())
}
