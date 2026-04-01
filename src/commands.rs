use std::path::Path;

use crate::cli::Commands;

pub async fn run(command: Commands) -> anyhow::Result<()> {
    match command {
        Commands::Daemon => run_daemon().await,
        Commands::Research { query, manual } => run_research(&query, manual.as_deref()).await,
        Commands::ZedBridge {
            method,
            file,
            line,
            text,
            cursor,
        } => run_zed_bridge(&method, file.as_deref(), line, text.as_deref(), cursor.as_deref()).await,
    }
}

async fn run_daemon() -> anyhow::Result<()> {
    tracing::info!("daemon entry (socket + JSON-RPC not wired yet)");
    anyhow::bail!("daemon is not implemented yet");
}

async fn run_research(query: &str, manual: Option<&Path>) -> anyhow::Result<()> {
    tracing::info!(query, ?manual, "research entry (client not wired yet)");
    anyhow::bail!("research client is not implemented yet");
}

async fn run_zed_bridge(
    method: &str,
    file: Option<&Path>,
    line: Option<u32>,
    text: Option<&str>,
    cursor: Option<&str>,
) -> anyhow::Result<()> {
    tracing::info!(
        method,
        ?file,
        line,
        text_len = text.map(str::len),
        cursor,
        "zed-bridge entry (not wired yet)"
    );
    anyhow::bail!("zed-bridge is not implemented yet");
}
