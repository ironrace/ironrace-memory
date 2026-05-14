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
    /// Side-by-side comparison (LLM baseline + candidate).
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
        Cmd::Compare {
            baseline_run,
            candidate_run,
            candidate_name,
            out,
        } => {
            let report =
                provbench_scoring::compare::run(&baseline_run, &candidate_run, &candidate_name)?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let bytes = serde_json::to_vec_pretty(&report)?;
            std::fs::write(&out, bytes)?;
            Ok(())
        }
    }
}
