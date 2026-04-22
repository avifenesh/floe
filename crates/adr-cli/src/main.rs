mod calibrate;
mod intent_source;

use std::path::PathBuf;

use adr_core::{Artifact, artifact::PrRef};
use anyhow::{Context, Result};
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
        /// Path to a structured intent JSON (matches `adr_core::Intent`)
        /// or a raw text file. `.json` is parsed as structured; anything
        /// else is treated as raw PR description text.
        #[arg(long)]
        intent_file: Option<PathBuf>,
        /// Resolve intent via `gh pr view <number> --repo <repo>` — the
        /// GitHub CLI's JSON output is piped through a normaliser into
        /// the raw-text form. Mutually exclusive with `--intent-file`.
        #[arg(long)]
        intent_pr: Option<u64>,
        /// When using `--intent-pr`, the GitHub repo in `owner/name`
        /// form. Falls back to `gh`'s auto-detection when omitted.
        #[arg(long)]
        intent_repo: Option<String>,
        /// Path to a plain-text notes file — reviewer-pasted benchmark
        /// output, staging logs, observations that corroborate claims.
        /// Read verbatim into `artifact.notes`.
        #[arg(long)]
        notes_file: Option<PathBuf>,
    },
    /// Print the graph schema (v0.1) as JSON Schema.
    Schema,
    /// Compare two artifacts' flow assignments side-by-side. Useful for
    /// calibrating LLM runs (same PR, different model/prompt version)
    /// against the structural floor or against each other.
    Calibrate {
        /// First artifact JSON (commonly the baseline).
        a: PathBuf,
        /// Second artifact JSON (the one under test).
        b: PathBuf,
        /// Emit a machine-readable JSON report instead of the human one.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Diff {
            base,
            head,
            intent_file,
            intent_pr,
            intent_repo,
            notes_file,
        } => {
            if intent_file.is_some() && intent_pr.is_some() {
                anyhow::bail!(
                    "--intent-file and --intent-pr are mutually exclusive; pick one"
                );
            }
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
            artifact.intent = intent_source::resolve(
                intent_file.as_deref(),
                intent_pr,
                intent_repo.as_deref(),
            )?;
            if let Some(p) = notes_file.as_deref() {
                artifact.notes = std::fs::read_to_string(p)
                    .with_context(|| format!("reading notes file {}", p.display()))?;
            }
            let out = serde_json::to_string_pretty(&artifact)?;
            println!("{out}");
        }
        Cmd::Schema => {
            let schema = schemars::schema_for!(Artifact);
            println!("{}", serde_json::to_string_pretty(&schema)?);
        }
        Cmd::Calibrate { a, b, json } => {
            let a_art: Artifact = serde_json::from_slice(
                &std::fs::read(&a).with_context(|| format!("reading {}", a.display()))?,
            )
            .context("parsing artifact A")?;
            let b_art: Artifact = serde_json::from_slice(
                &std::fs::read(&b).with_context(|| format!("reading {}", b.display()))?,
            )
            .context("parsing artifact B")?;
            let report = calibrate::compare(&a_art, &b_art);
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("A: {}\nB: {}\n", a.display(), b.display());
                print!("{}", calibrate::format_text(&report));
            }
        }
    }
    Ok(())
}
