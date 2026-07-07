use clap::Parser;
use qa::{run_qa, QaRunOptions};
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = "qa", about = "Run data quality checks over pipeline Parquet outputs")]
struct Cli {
    /// Exit non-zero when checks regress vs committed baseline
    #[arg(long)]
    strict: bool,

    /// Rewrite checks_baseline.parquet from current run (after review)
    #[arg(long)]
    update_baseline: bool,

    /// Run only checks whose id starts with this prefix (e.g. vote, graph)
    #[arg(long)]
    tier: Option<String>,

    /// Run a single check by full id
    #[arg(long)]
    check: Option<String>,
}

fn main() -> ExitCode {
    dotenvy::dotenv().ok();

    let cli = Cli::parse();
    match run_qa(&QaRunOptions {
        strict: cli.strict,
        update_baseline: cli.update_baseline,
        tier_filter: cli.tier,
        check_filter: cli.check,
    }) {
        Ok(result) => {
            if result.strict_failed {
                eprintln!("[qa] strict mode: failing due to baseline regression");
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("[qa] error: {e}");
            ExitCode::from(2)
        }
    }
}
