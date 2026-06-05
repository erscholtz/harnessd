//! Local JSON-RPC IPC server and daemon method entrypoints.

pub(crate) mod methods;

pub use crate::autocomplete::{inline, inline_fast, inline_prepare};
pub use methods::{
    anchors, codex_sessions, complete, generate, prefetch, scratch_create, serve, status,
    thread_attach, thread_create, thread_link, thread_list, thread_resolve,
};
