//! Local research harness: CLI, long-lived daemon, and editor bridge (`harnessd --help`).
#![warn(missing_docs)]

use anyhow::Context;
use clap::Parser;

use harnessd::cli::Cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let cli = Cli::parse();
    harnessd::commands::run(cli.command)
        .await
        .context("harnessd command failed")?;

    Ok(())
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
