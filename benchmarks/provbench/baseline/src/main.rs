//! `provbench-baseline` CLI entry point.
//!
//! Subcommands are skeletons; implementations land in subsequent tasks:
//!   - `sample` — Task 5 (deterministic mutation sampling)
//!   - `run`    — Task 8 (LLM invalidation pass over sampled mutations)
//!   - `score`  — Task 9 (three-way scoring vs labeler ground truth)

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "provbench-baseline",
    version,
    about = "Phase 0c LLM-as-invalidator baseline for ProvBench-CodeContext"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Deterministically sample mutations from labeler artifacts (Task 5).
    Sample(SampleArgs),
    /// Execute the LLM invalidator over sampled mutations (Task 8).
    Run(RunArgs),
    /// Score baseline decisions against labeler ground truth (Task 9).
    Score(ScoreArgs),
}

#[derive(Debug, clap::Args)]
struct SampleArgs {
    /// Path to the labeler corpus JSONL (e.g. `…/corpus/<run>.jsonl`).
    #[arg(long)]
    corpus: PathBuf,
    /// Path to the matching `*.facts.jsonl` artifact.
    #[arg(long)]
    facts: PathBuf,
    /// Path to the matching `*.diffs/` directory of per-commit JSON
    /// artifacts.
    #[arg(long)]
    diffs_dir: PathBuf,
    /// PRNG seed for the deterministic per-stratum draw.
    #[arg(long, default_value_t = provbench_baseline::constants::DEFAULT_SEED)]
    seed: u64,
    /// Operational budget ceiling in USD. Preflight refuses if the
    /// worst-case estimate exceeds this.
    #[arg(long, default_value_t = provbench_baseline::constants::DEFAULT_OPERATIONAL_BUDGET_USD)]
    budget_usd: f64,
    /// Path to write the resulting `manifest.json`.
    #[arg(long)]
    out: PathBuf,
}

#[derive(Debug, clap::Args)]
struct RunArgs {}

#[derive(Debug, clap::Args)]
struct ScoreArgs {}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Sample(args) => {
            let manifest = provbench_baseline::manifest::SampleManifest::from_corpus(
                &args.corpus,
                &args.facts,
                &args.diffs_dir,
                args.seed,
                provbench_baseline::sample::PerStratumTargets::default(),
                args.budget_usd,
            )?;
            manifest.save_atomic(&args.out)?;
            println!(
                "wrote manifest to {} (selected={}, excluded={})",
                args.out.display(),
                manifest.selected_count,
                manifest.excluded_count_by_reason.values().sum::<usize>()
            );
            Ok(())
        }
        Command::Run(_) => {
            anyhow::bail!("`run` not yet implemented (Task 8)")
        }
        Command::Score(_) => {
            anyhow::bail!("`score` not yet implemented (Task 9)")
        }
    }
}
