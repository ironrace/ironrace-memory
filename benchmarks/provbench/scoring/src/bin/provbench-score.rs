use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "provbench-score", version, about = "ProvBench shared scorer")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Score a Phase 0c LLM-baseline run directory.
    Baseline {
        #[arg(long)]
        run: PathBuf,
    },
    /// Side-by-side comparison (LLM baseline + candidate). Filled in Task 4.
    Compare {
        #[arg(long = "baseline-run")]
        baseline_run: PathBuf,
        #[arg(long = "candidate-run")]
        candidate_run: PathBuf,
        #[arg(long = "candidate-name")]
        candidate_name: String,
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Baseline { run } => provbench_scoring::report::score_llm_baseline_run(&run),
        Cmd::Compare { .. } => anyhow::bail!("compare: implemented in Task 4"),
    }
}
