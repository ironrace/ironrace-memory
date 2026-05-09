use clap::Parser;

#[derive(Parser)]
#[command(
    name = "provbench-labeler",
    version,
    about = "ProvBench Phase 0b labeler"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(clap::Subcommand)]
enum Cmd {
    /// Print the labeler git SHA stamp used for output rows.
    Stamp,
    /// Verify pinned external tools match SPEC §13.1 content hashes.
    VerifyTooling,
    /// Run the labeler over a pilot repo and write JSONL.
    Run {
        /// Local path to the cloned pilot repo.
        #[arg(long)]
        repo: std::path::PathBuf,
        /// T₀ commit SHA.
        #[arg(long)]
        t0: String,
        /// Output JSONL path.
        #[arg(long)]
        out: std::path::PathBuf,
        /// Skip rust-analyzer symbol resolution (unit-test mode).
        #[arg(long, default_value_t = false)]
        skip_symbol_resolution: bool,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();
    match cli.command {
        None => Ok(()),
        Some(Cmd::Stamp) => {
            println!("{}", provbench_labeler::labeler_stamp());
            Ok(())
        }
        Some(Cmd::VerifyTooling) => {
            let resolved = provbench_labeler::tooling::resolve_from_env()?;
            println!("rust-analyzer: {}", resolved.rust_analyzer.display());
            println!("tree-sitter:  {}", resolved.tree_sitter.display());
            Ok(())
        }
        Some(Cmd::Run {
            repo,
            t0,
            out,
            skip_symbol_resolution,
        }) => {
            let cfg = provbench_labeler::replay::ReplayConfig {
                repo_path: repo,
                t0_sha: t0,
                skip_symbol_resolution,
            };
            let rows: Vec<provbench_labeler::output::OutputRow> =
                provbench_labeler::replay::Replay::run(&cfg)?
                    .into_iter()
                    .map(|r| provbench_labeler::output::OutputRow {
                        fact_id: r.fact_id,
                        commit_sha: r.commit_sha,
                        label: r.label,
                    })
                    .collect();
            let sha = provbench_labeler::output::current_labeler_sha()?;
            provbench_labeler::output::write_jsonl(&out, &rows, &sha)?;
            println!("wrote {} rows to {}", rows.len(), out.display());
            Ok(())
        }
    }
}
