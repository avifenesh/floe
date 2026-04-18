use std::path::PathBuf;

use adr_core::{Artifact, artifact::PrRef};
use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "adr", version, about = "Architectural delta review CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Emit a JSON artifact for the diff between two working trees.
    Diff {
        /// Base working tree (pre-change snapshot).
        base: PathBuf,
        /// Head working tree (post-change snapshot).
        head: PathBuf,
    },
    /// Print the graph schema (v0.1) as JSON Schema.
    Schema,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Diff { base, head } => {
            let pr = PrRef {
                repo: "unknown".into(),
                base_sha: base.display().to_string(),
                head_sha: head.display().to_string(),
            };
            let mut artifact = Artifact::new(pr);
            artifact.base = adr_parse::Ingest::new("base").ingest_dir(&base)?;
            artifact.head = adr_parse::Ingest::new("head").ingest_dir(&head)?;
            artifact.base_cfg = adr_cfg::build_for_graph(&artifact.base, &base)?;
            artifact.head_cfg = adr_cfg::build_for_graph(&artifact.head, &head)?;
            artifact.hunks = adr_hunks::extract_all(&artifact.base, &artifact.head);
            artifact.flows = adr_flows::cluster(&artifact);
            let out = serde_json::to_string_pretty(&artifact)?;
            println!("{out}");
        }
        Cmd::Schema => {
            let schema = schemars::schema_for!(Artifact);
            println!("{}", serde_json::to_string_pretty(&schema)?);
        }
    }
    Ok(())
}
