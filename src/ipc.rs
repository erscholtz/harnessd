//! Local JSON-RPC IPC server and daemon method entrypoints.

pub(crate) mod methods;

pub use crate::autocomplete::{inline, inline_fast, inline_prepare};
pub use methods::{
    anchors, codex_sessions, complete, generate, mark_create, mark_delete, mark_list, mark_next,
    mark_prev, prefetch, scratch_create, serve, settings_get, settings_update, status,
    thread_attach, thread_create, thread_delete, thread_example_create, thread_link, thread_list,
    thread_resolve,
};
