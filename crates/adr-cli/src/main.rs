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
            let artifact = Artifact::new(pr);
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
