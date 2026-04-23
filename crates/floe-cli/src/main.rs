mod calibrate;
mod intent_source;

use std::path::PathBuf;

use floe_core::{Artifact, artifact::PrRef};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "floe", version, about = "Architectural PR review CLI")]
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
        /// Path to a structured intent JSON (matches `floe_core::Intent`)
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
    /// Reveal the baseline pin on an artifact, or compare two pins to
    /// check whether `floe calibrate` is apples-to-apples (RFC v0.3 §9).
    /// Exits non-zero on mismatch.
    Baseline {
        /// First artifact JSON. With `--against`, this is the reference
        /// pin; without, just prints its pin.
        artifact: PathBuf,
        /// Optional second artifact — compares pins against the first.
        #[arg(long)]
        against: Option<PathBuf>,
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
            artifact.base = floe_parse::Ingest::new("base").ingest_dir(&base)?;
            artifact.head = floe_parse::Ingest::new("head").ingest_dir(&head)?;
            artifact.base_cfg = floe_cfg::build_for_graph(&artifact.base, &base)?;
            artifact.head_cfg = floe_cfg::build_for_graph(&artifact.head, &head)?;
            artifact.hunks = floe_hunks::extract_all(&artifact.base, &artifact.head);
            artifact.hunks.extend(floe_hunks::extract_lock_hunks(&base, &head));
            artifact.hunks.extend(floe_hunks::extract_data_hunks(&base, &head));
            artifact.hunks.extend(floe_hunks::extract_docs_hunks(&head));
            artifact.hunks.extend(floe_hunks::extract_deletion_hunks(
                &artifact.base,
                &artifact.head,
            ));
            artifact.flows = floe_flows::cluster(&artifact);
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
        Cmd::Baseline { artifact, against } => {
            let a_art: Artifact = serde_json::from_slice(
                &std::fs::read(&artifact)
                    .with_context(|| format!("reading {}", artifact.display()))?,
            )
            .context("parsing artifact")?;
            let a_pin = a_art
                .baseline
                .as_ref()
                .context("artifact has no baseline — probe pass has not run")?;
            println!("artifact: {}", artifact.display());
            print_pin(a_pin);
            if let Some(b_path) = against {
                let b_art: Artifact = serde_json::from_slice(
                    &std::fs::read(&b_path)
                        .with_context(|| format!("reading {}", b_path.display()))?,
                )
                .context("parsing --against artifact")?;
                let b_pin = b_art
                    .baseline
                    .as_ref()
                    .context("--against artifact has no baseline")?;
                println!("\n--against: {}", b_path.display());
                print_pin(b_pin);
                println!();
                if a_pin.pin_matches(b_pin) {
                    println!("pin: MATCHES — comparison is apples-to-apples.");
                } else {
                    println!("pin: MISMATCH — re-baseline required (RFC v0.3 §9).");
                    std::process::exit(2);
                }
            }
        }
    }
    Ok(())
}

fn print_pin(b: &floe_core::ArtifactBaseline) {
    println!("  probe_model:         {}", b.probe_model);
    println!("  probe_set_version:   {}", b.probe_set_version);
    println!(
        "  synthesis_model:     {}",
        b.synthesis_model.as_deref().unwrap_or("<none — structural only>")
    );
    println!(
        "  proof_model:         {}",
        b.proof_model.as_deref().unwrap_or("<none — proof skipped>")
    );
}
