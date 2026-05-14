use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "provbench-phase1",
    version,
    about = "Phase 1 rules-based invalidator"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Score a baseline run's eval subset with the structural rule chain.
    Score {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        t0: String,
        #[arg(long)]
        facts: PathBuf,
        #[arg(long = "diffs-dir")]
        diffs_dir: PathBuf,
        #[arg(long = "baseline-run")]
        baseline_run: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Score { .. } => anyhow::bail!("score: implemented in Task 3 + Task 4"),
    }
}
