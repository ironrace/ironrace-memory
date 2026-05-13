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
        /// Skip commit-tree symbol resolution (unit-test mode).
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
        /// RNG seed for stratified sampling. Accepts decimal (`42`) or
        /// hex with `0x`/`0X` prefix (`0xC0DEBABEDEADBEEF`) so the
        /// value the CLI echoes can be pasted back as an argument.
        /// Omit to use the historical default (`DEFAULT_SEED`) so
        /// reviewers can resume an in-progress CSV. Supply a fresh
        /// value only for post-merge / anti-tuning validation runs.
        #[arg(long, value_parser = parse_seed_arg)]
        seed: Option<u64>,
    },
    /// Read a filled spot-check CSV, compute the agreement rate, print
    /// the Wilson 95% report.
    Report {
        #[arg(long)]
        csv: std::path::PathBuf,
    },
}

/// Parse the `--seed` value from a clap argument string. Accepts
/// decimal (`42`, `12345678901234567890`) and hex with the standard
/// `0x` / `0X` prefix (`0xC0DEBABEDEADBEEF`) so the seed the CLI
/// echoes on success (`seed=0x…`) can be pasted back verbatim. Returns
/// a human-readable error string on parse failure; clap renders it to
/// stderr.
fn parse_seed_arg(s: &str) -> Result<u64, String> {
    let trimmed = s.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16)
            .map_err(|e| format!("invalid hex seed `{trimmed}`: {e} (expected u64)"))
    } else {
        trimmed
            .parse::<u64>()
            .map_err(|e| format!("invalid decimal seed `{trimmed}`: {e} (expected u64)"))
    }
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
            let sha = provbench_labeler::labeler_stamp();
            provbench_labeler::output::write_jsonl(&out, &rows, &sha)?;
            println!("wrote {} rows to {}", rows.len(), out.display());
            Ok(())
        }
        Some(Cmd::Spotcheck {
            corpus,
            out,
            n,
            seed,
        }) => {
            let content = std::fs::read_to_string(&corpus)?;
            let rows: Vec<provbench_labeler::output::OutputRow> = content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| {
                    serde_json::from_str::<provbench_labeler::output::OutputRow>(l)
                        .map_err(|e| anyhow::anyhow!("failed to parse JSONL line: {e}"))
                })
                .collect::<anyhow::Result<_>>()?;
            let resolved_seed = seed.unwrap_or(provbench_labeler::spotcheck::DEFAULT_SEED);
            let samples = provbench_labeler::spotcheck::sample(&rows, n, resolved_seed);
            provbench_labeler::spotcheck::write_csv(&out, &samples)?;
            let meta = provbench_labeler::spotcheck::SpotCheckMeta {
                corpus: corpus.display().to_string(),
                seed: resolved_seed,
                n,
                labeler_git_sha: provbench_labeler::labeler_stamp(),
            };
            provbench_labeler::spotcheck::write_meta_sidecar(&out, &meta)?;
            println!(
                "wrote {} samples to {} (seed=0x{:016x})",
                samples.len(),
                out.display(),
                resolved_seed
            );
            Ok(())
        }
        Some(Cmd::Report { csv }) => {
            let (agree, total) = provbench_labeler::spotcheck::read_report_counts(&csv)?;
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

#[cfg(test)]
mod tests {
    use super::parse_seed_arg;

    #[test]
    fn parses_decimal_seed() {
        assert_eq!(parse_seed_arg("0").unwrap(), 0);
        assert_eq!(parse_seed_arg("42").unwrap(), 42);
        assert_eq!(parse_seed_arg("18446744073709551615").unwrap(), u64::MAX);
    }

    #[test]
    fn parses_hex_seed_with_either_prefix_case() {
        assert_eq!(parse_seed_arg("0x0").unwrap(), 0);
        assert_eq!(
            parse_seed_arg("0xC0DEBABEDEADBEEF").unwrap(),
            0xC0DE_BABE_DEAD_BEEF
        );
        // Uppercase prefix is also accepted so the CLI's lowercase
        // echo and an upper-case paste both work.
        assert_eq!(
            parse_seed_arg("0XC0DEBABEDEADBEEF").unwrap(),
            0xC0DE_BABE_DEAD_BEEF
        );
    }

    #[test]
    fn rejects_garbage_seed() {
        assert!(parse_seed_arg("not-a-number").is_err());
        assert!(parse_seed_arg("0xZZZ").is_err());
        // u64 overflow surfaces as an error rather than silently
        // wrapping.
        assert!(parse_seed_arg("18446744073709551616").is_err());
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(parse_seed_arg("  42  ").unwrap(), 42);
        assert_eq!(parse_seed_arg(" 0xff ").unwrap(), 0xff);
    }
}
