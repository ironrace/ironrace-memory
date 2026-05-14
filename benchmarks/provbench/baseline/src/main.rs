//! `provbench-baseline` CLI entry point.
//!
//! Subcommands are skeletons; implementations land in subsequent tasks:
//!   - `sample` — Task 5 (deterministic mutation sampling)
//!   - `run`    — Task 8 (LLM invalidation pass over sampled mutations)
//!   - `score`  — Task 9 (three-way scoring vs labeler ground truth)

use anyhow::Result;
use clap::{Parser, Subcommand};

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
struct SampleArgs {}

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
        Command::Sample(_) => {
            anyhow::bail!("`sample` not yet implemented (Task 5)")
        }
        Command::Run(_) => {
            anyhow::bail!("`run` not yet implemented (Task 8)")
        }
        Command::Score(_) => {
            anyhow::bail!("`score` not yet implemented (Task 9)")
        }
    }
}
