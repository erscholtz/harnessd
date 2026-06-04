//! Local research harness: CLI, long-lived daemon, and editor bridge.
//!
//! This crate provides an autocomplete-first daemon that uses tree-sitter
//! to understand code structure and provides cached completion suggestions.

#![warn(missing_docs)]

/// ACP client wrapper used for explicit generation requests.
pub mod acp;
/// SQLite-backed proposal cache.
pub mod cache;
/// Command-line argument definitions.
pub mod cli;
/// Saved Codex session scanner.
pub mod codex_sessions;
/// CLI command execution.
pub mod commands;
/// Daemon lockfile handling.
pub mod daemon_lock;
/// Dashboard status collection.
pub mod dashboard;
/// Local JSON-RPC IPC server and method implementations.
pub mod ipc;
/// Editor-facing LSP adapter.
pub mod lsp;
/// Tree-sitter parsing and anchor detection.
pub mod parser;
/// Runtime path helpers.
pub mod paths;
/// JSON-RPC protocol types.
pub mod rpc;
/// Runtime health inspection.
pub mod runtime;
/// Scratch preview artifact generation.
pub mod scratch;
/// Shutdown signal handling.
pub mod shutdown;
/// Shared daemon state.
pub mod state;
/// Persistent Codex thread anchors.
pub mod threads;
/// Terminal dashboard UI.
pub mod tui;
