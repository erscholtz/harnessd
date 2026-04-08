use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "harnessd", version, about = "Local research harness (daemon + CLI + Zed bridge)")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run the long-lived daemon (local socket + JSON-RPC).
    Daemon,

    /// Stop the running daemon (SIGTERM on Unix; graceful `taskkill` on Windows).
    Stop,

    /// Send a research request to the daemon (starts daemon if needed).
    Research {
        /// Search query
        query: String,
        /// Optional manual path (reserved for future indexing)
        #[arg(long)]
        manual: Option<PathBuf>,
    },

    /// One-shot RPC bridge for Zed tasks (stdout is the JSON-RPC response).
    ZedBridge {
        /// JSON-RPC method name (e.g. inline, complete)
        #[arg(long)]
        method: String,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        line: Option<u32>,
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        cursor: Option<String>,
    },
}
