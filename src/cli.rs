use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "harnessd",
    version,
    about = "Local research harness (daemon + CLI + editor bridge)"
)]
/// Parsed command-line interface for `harnessd`.
pub struct Cli {
    /// Selected subcommand.
    #[command(subcommand)]
    pub command: Commands,
}

/// Top-level CLI subcommands.
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

    /// Run the editor-facing Language Server Protocol adapter over stdio.
    Lsp,

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

    /// Ask ACP for ephemeral insertion text at a live-buffer cursor location.
    Inline {
        /// Absolute or workspace-relative source file path.
        #[arg(long)]
        file: PathBuf,
        /// Cursor position as a byte offset in stdin content.
        #[arg(long)]
        offset: usize,
        /// User instruction for generated insertion text.
        #[arg(long)]
        prompt: String,
    },

    /// Warm the proposal cache for a file or workspace path.
    Prefetch {
        /// File or directory to scan for anchors.
        #[arg(long)]
        path: PathBuf,
    },

    /// List saved Codex sessions for a workspace.
    CodexSessions {
        /// Workspace root for project-first filtering.
        #[arg(long)]
        workspace: PathBuf,
        /// Include all saved Codex sessions.
        #[arg(long, default_value_t = false)]
        all: bool,
        /// Maximum number of sessions to return.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },

    /// Manage Neovim line-anchored Codex threads.
    Thread {
        /// Thread management action.
        #[command(subcommand)]
        command: ThreadCommands,
    },

    /// Send a research request to the daemon (starts daemon if needed).
    Research {
        /// Search query
        query: String,
        /// Optional manual path (reserved for future indexing)
        #[arg(long)]
        manual: Option<PathBuf>,
    },

    /// One-shot RPC bridge for editor integrations (stdout is the JSON-RPC response).
    Bridge {
        /// JSON-RPC method name (e.g. inline, complete)
        #[arg(long)]
        method: String,
        #[arg(long)]
        /// Optional file path.
        file: Option<PathBuf>,
        #[arg(long)]
        /// Optional line number.
        line: Option<u32>,
        #[arg(long)]
        /// Optional text payload.
        text: Option<String>,
        #[arg(long)]
        /// Optional cursor payload.
        cursor: Option<String>,
    },
}

/// Subcommands for persistent Codex thread anchors.
#[derive(Subcommand)]
pub enum ThreadCommands {
    /// Create a new line-anchored thread; live buffer content is read from stdin.
    Create {
        /// Workspace root.
        #[arg(long)]
        workspace: PathBuf,
        /// Source file path.
        #[arg(long)]
        file: PathBuf,
        /// Cursor byte offset in stdin content.
        #[arg(long)]
        offset: usize,
        /// User prompt.
        #[arg(long)]
        prompt: String,
        /// Optional selected start byte.
        #[arg(long)]
        selection_start: Option<usize>,
        /// Optional selected end byte.
        #[arg(long)]
        selection_end: Option<usize>,
    },
    /// List anchored threads; optional live buffer content is read from stdin when provided.
    List {
        /// Workspace root.
        #[arg(long)]
        workspace: PathBuf,
        /// Optional source file path.
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Link a thread to a Codex session.
    Link {
        /// Thread id.
        #[arg(long)]
        thread_id: String,
        /// Codex session UUID.
        #[arg(long)]
        session_id: String,
        /// Optional Codex JSONL path.
        #[arg(long)]
        session_path: Option<PathBuf>,
    },
    /// Resolve a newly launched thread to a Codex session.
    Resolve {
        /// Thread id.
        #[arg(long)]
        thread_id: String,
        /// Workspace root.
        #[arg(long)]
        workspace: PathBuf,
        /// Unix timestamp captured before launching Codex.
        #[arg(long)]
        started_after: u64,
    },
    /// Attach an existing Codex session to the current source line.
    Attach {
        /// Workspace root.
        #[arg(long)]
        workspace: PathBuf,
        /// Source file path.
        #[arg(long)]
        file: PathBuf,
        /// Cursor byte offset in stdin content.
        #[arg(long)]
        offset: usize,
        /// Codex session UUID.
        #[arg(long)]
        session_id: String,
    },
}
