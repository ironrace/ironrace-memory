use anyhow::Result;
use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "provbench-phase1", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
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
        #[arg(long, default_value = "v1.0")]
        rule_set_version: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Score {
            repo,
            t0,
            facts,
            diffs_dir,
            baseline_run,
            out,
            rule_set_version,
        } => {
            fs::create_dir_all(&out)?;
            let db = provbench_phase1::storage::open(&out.join("phase1.sqlite"))?;
            provbench_phase1::facts::ingest(&db, &facts)?;
            provbench_phase1::diffs::ingest(&db, &diffs_dir)?;
            provbench_phase1::baseline_run::ingest(&db, &baseline_run.join("predictions.jsonl"))?;
            let repo = provbench_phase1::repo::Repo::open(&repo)?;
            let stats = provbench_phase1::runner::run(provbench_phase1::runner::RunnerOpts {
                db: &db,
                repo: &repo,
                t0: &t0,
                rule_set_version: &rule_set_version,
                out_predictions: &out.join("predictions.jsonl"),
                out_traces: &out.join("rule_traces.jsonl"),
            })?;
            eprintln!("phase1 done: {:?}", stats);
            Ok(())
        }
    }
}
