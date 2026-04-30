//! Local research harness: CLI, long-lived daemon, and editor bridge.
//!
//! This crate provides an autocomplete-first daemon that uses tree-sitter
//! to understand code structure and provides cached completion suggestions.

#![warn(missing_docs)]

pub mod cache;
pub mod cli;
pub mod commands;
pub mod daemon_lock;
pub mod dashboard;
pub mod ipc;
pub mod lsp;
pub mod parser;
pub mod paths;
pub mod rpc;
pub mod runtime;
pub mod shutdown;
pub mod state;
pub mod tui;
