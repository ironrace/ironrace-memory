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
    }
}
