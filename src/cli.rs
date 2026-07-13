use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

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
        /// Optional model override for this request.
        #[arg(long)]
        model: Option<String>,
        /// Optional reasoning effort override for this request.
        #[arg(long)]
        reasoning_effort: Option<String>,
    },

    /// Generate a saved scratch preview file from live-buffer context.
    Scratch {
        /// Workspace root.
        #[arg(long)]
        workspace: PathBuf,
        /// Absolute or workspace-relative source file path.
        #[arg(long)]
        file: PathBuf,
        /// Cursor position as a byte offset in stdin content.
        #[arg(long)]
        offset: usize,
        /// User instruction for the scratch preview.
        #[arg(long)]
        prompt: String,
        /// Optional selected start byte.
        #[arg(long)]
        selection_start: Option<usize>,
        /// Optional selected end byte.
        #[arg(long)]
        selection_end: Option<usize>,
        /// Optional model override for this request.
        #[arg(long)]
        model: Option<String>,
        /// Optional reasoning effort override for this request.
        #[arg(long)]
        reasoning_effort: Option<String>,
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

    /// Manage external source marks.
    Mark {
        /// Mark management action.
        #[command(subcommand)]
        command: MarkCommands,
    },

    /// Manage daemon-owned settings.
    Settings {
        /// Settings action.
        #[command(subcommand)]
        command: SettingsCommands,
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
        #[arg(long)]
        /// Optional model override for bridge methods that support models.
        model: Option<String>,
        #[arg(long)]
        /// Optional reasoning effort override for bridge methods that support models.
        reasoning_effort: Option<String>,
        /// Disable background model refresh for bridge methods that support it.
        #[arg(long)]
        no_background_refresh: bool,
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
        /// Optional model override for the launched Codex thread.
        #[arg(long)]
        model: Option<String>,
        /// Optional reasoning effort override for the launched Codex thread.
        #[arg(long)]
        reasoning_effort: Option<String>,
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
    /// Generate a scratch example and link it to a thread.
    Example {
        /// Parent thread id.
        #[arg(long)]
        thread_id: String,
        /// Workspace root.
        #[arg(long)]
        workspace: PathBuf,
        /// Source file path.
        #[arg(long)]
        file: PathBuf,
        /// Cursor byte offset in stdin content.
        #[arg(long)]
        offset: usize,
        /// User prompt for the example.
        #[arg(long)]
        prompt: String,
        /// Optional selected start byte.
        #[arg(long)]
        selection_start: Option<usize>,
        /// Optional selected end byte.
        #[arg(long)]
        selection_end: Option<usize>,
        /// Optional model override for the example.
        #[arg(long)]
        model: Option<String>,
        /// Optional reasoning effort override for the example.
        #[arg(long)]
        reasoning_effort: Option<String>,
    },
    /// Delete a thread and its scratch artifacts.
    Delete {
        /// Thread id.
        #[arg(long)]
        thread_id: String,
    },
}

/// Subcommands for external source marks.
#[derive(Subcommand)]
pub enum MarkCommands {
    /// Create a new external source mark; live buffer content is read from stdin.
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
        /// Optional attached thread id.
        #[arg(long)]
        thread_id: Option<String>,
    },
    /// List marks; optional live buffer content is read from stdin when provided.
    List {
        /// Workspace root.
        #[arg(long)]
        workspace: PathBuf,
        /// Optional source file path.
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Delete a mark.
    Delete {
        /// Mark id.
        #[arg(long)]
        mark_id: String,
        /// Also delete an attached thread and its scratch files.
        #[arg(long, default_value_t = false)]
        delete_attached_thread: bool,
    },
    /// Return the next mark in a file, wrapping around.
    Next {
        /// Workspace root.
        #[arg(long)]
        workspace: PathBuf,
        /// Source file path.
        #[arg(long)]
        file: PathBuf,
        /// Current 1-based cursor line.
        #[arg(long)]
        current_line: usize,
    },
    /// Return the previous mark in a file, wrapping around.
    Prev {
        /// Workspace root.
        #[arg(long)]
        workspace: PathBuf,
        /// Source file path.
        #[arg(long)]
        file: PathBuf,
        /// Current 1-based cursor line.
        #[arg(long)]
        current_line: usize,
    },
}

/// Subcommands for daemon-owned settings.
#[derive(Subcommand)]
pub enum SettingsCommands {
    /// Print current settings.
    Get,
    /// Update settings.
    Update {
        /// Scratch storage mode.
        #[arg(long)]
        scratch_storage_mode: Option<ScratchStorageModeArg>,
        /// Read scope.
        #[arg(long)]
        read_scope: Option<ReadScopeArg>,
    },
}

/// CLI scratch storage mode values.
#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum ScratchStorageModeArg {
    /// Durable runtime-dir scratch storage.
    Runtime,
    /// Ephemeral OS temp-dir scratch storage.
    Temp,
}

/// CLI read scope values.
#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum ReadScopeArg {
    /// Current file/selection plus explicit context.
    CurrentContext,
}
