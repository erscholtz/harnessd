//! Local JSON-RPC IPC server and daemon method entrypoints.

mod methods;

pub use methods::{
    anchors, codex_sessions, complete, generate, inline, prefetch, scratch_create, serve, status,
    thread_attach, thread_create, thread_link, thread_list, thread_resolve,
};
