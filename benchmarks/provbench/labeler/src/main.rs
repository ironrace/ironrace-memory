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
    /// Sample 200 rows from a corpus JSONL for human spot-check review.
    Spotcheck {
        #[arg(long)]
        corpus: std::path::PathBuf,
        #[arg(long)]
        out: std::path::PathBuf,
        #[arg(long, default_value_t = 200)]
        n: usize,
    },
    /// Read a filled spot-check CSV, compute the agreement rate, print
    /// the Wilson 95% report.
    Report {
        #[arg(long)]
        csv: std::path::PathBuf,
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
        Some(Cmd::Spotcheck { corpus, out, n }) => {
            let content = std::fs::read_to_string(&corpus)?;
            let rows: Vec<provbench_labeler::output::OutputRow> = content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| {
                    serde_json::from_str::<provbench_labeler::output::OutputRow>(l)
                        .map_err(|e| anyhow::anyhow!("failed to parse JSONL line: {e}"))
                })
                .collect::<anyhow::Result<_>>()?;
            let samples = provbench_labeler::spotcheck::sample(&rows, n);
            provbench_labeler::spotcheck::write_csv(&out, &samples)?;
            println!("wrote {} samples to {}", samples.len(), out.display());
            Ok(())
        }
        Some(Cmd::Report { csv }) => {
            let content = std::fs::read_to_string(&csv)?;
            let mut lines = content.lines();
            // Skip header row.
            lines.next();
            let mut total: u32 = 0;
            let mut agree: u32 = 0;
            for line in lines {
                if line.trim().is_empty() {
                    continue;
                }
                let fields = split_csv_line(line);
                // Columns: fact_id(0), commit_sha(1), bucket(2), predicted_label(3), human_label(4), disagreement_notes(5)
                let human = fields.get(4).map(|s| s.as_str()).unwrap_or("").trim();
                if human.is_empty() {
                    continue;
                }
                total += 1;
                let predicted = fields.get(3).map(|s| s.as_str()).unwrap_or("").trim();
                if human == predicted {
                    agree += 1;
                }
            }
            let r = provbench_labeler::spotcheck::report(agree, total);
            println!("Total reviewed: {}", r.total);
            println!("Agreements: {}", r.agree);
            println!("Point estimate: {:.2}%", r.point_estimate * 100.0);
            println!("Wilson 95% lower bound: {:.2}%", r.wilson_lower_95 * 100.0);
            println!(
                "Gate (\u{2265}95% and n\u{2265}200): {}",
                if r.gate_passed { "PASS" } else { "FAIL" }
            );
            Ok(())
        }
    }
}

/// Split a single CSV line respecting double-quote escaping.
fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    // Escaped quote inside quoted field.
                    chars.next();
                    current.push('"');
                } else {
                    in_quotes = false;
                }
            }
            '"' => {
                in_quotes = true;
            }
            ',' if !in_quotes => {
                fields.push(current.clone());
                current.clear();
            }
            other => {
                current.push(other);
            }
        }
    }
    fields.push(current);
    fields
}
