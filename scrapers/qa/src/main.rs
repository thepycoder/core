use clap::Parser;
use qa::{QaRunOptions, run_qa};
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "qa",
    about = "Run data quality checks over pipeline Parquet outputs"
)]
struct Cli {
    /// Exit non-zero when findings regress relative to the committed baseline.
    #[arg(long)]
    strict: bool,

    /// Replace committed baselines after findings have been reviewed.
    #[arg(long)]
    update_baseline: bool,
}

fn main() -> ExitCode {
    dotenvy::dotenv().ok();

    let cli = Cli::parse();
    match run_qa(&QaRunOptions {
        strict: cli.strict,
        update_baseline: cli.update_baseline,
    }) {
        Ok(result) if result.strict_failed => {
            eprintln!("[qa] strict mode: failing due to baseline regression");
            ExitCode::from(1)
        }
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[qa] error: {e}");
            ExitCode::from(2)
        }
    }
}
