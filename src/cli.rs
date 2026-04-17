use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "harnessd",
    version,
    about = "Local research harness (daemon + CLI + Zed bridge)"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run the long-lived daemon (local socket + JSON-RPC).
    Daemon,

    /// Initialize the runtime stack in order and verify the daemon is ready.
    Setup {
        /// Optional file or directory to prefetch once the daemon is up.
        #[arg(long)]
        path: Option<PathBuf>,
        /// Skip launching the dashboard after setup completes.
        #[arg(long, default_value_t = false)]
        no_tui: bool,
    },

    /// Tear the runtime stack down gracefully and wait for shutdown.
    Teardown,

    /// Inspect runtime state and explain common startup/shutdown issues.
    Doctor,

    /// Stop the running daemon (SIGTERM on Unix; graceful `taskkill` on Windows).
    Stop,

    /// Open a live terminal dashboard for daemon and cache status.
    Tui,

    /// Debug the autocomplete path by asking the daemon for completions.
    Complete {
        /// Absolute or workspace-relative file path.
        #[arg(long)]
        file: PathBuf,
        /// Cursor position as a byte offset.
        #[arg(long)]
        offset: usize,
        /// Optional prefix to filter suggestions.
        #[arg(long)]
        prefix: Option<String>,
    },

    /// Warm the proposal cache for a file or workspace path.
    Prefetch {
        /// File or directory to scan for anchors.
        #[arg(long)]
        path: PathBuf,
    },

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
