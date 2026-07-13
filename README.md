# harnessd

`harnessd` is a local Neovim scratchpad and text-first whiteboard with
external source marks. It is meant to help you think beside a project without
letting Codex write to that project.

## Goal

The new product goal is a non-interfering workspace for notes, sketches, and
threaded scratch work:

- create an external mark at a source location
- attach an optional thread to that mark
- open a pinned panel for notes, scratch files, and thread history
- cycle marked locations from Neovim
- jump from a note or thread back to the marked source line
- keep generated scratch artifacts outside the project tree

`harnessd` should feel like a durable notebook attached to your editor, not an
autocomplete engine and not an agent that edits code.

## Non-Interference Contract

The target contract is precise and deliberately conservative:

- Codex has read-only project access: it must not edit or write inside the
  project directory.
- Codex receives only current context by default: the current file or
  selection, plus context the user explicitly asks to include.
- Scratch output is written outside the project tree by default.
- Marks are external metadata. They do not insert comments or markers into
  source files.
- Generated scratch files are linked back to marks and threads, but source
  buffers change only when the user manually copies or applies content.

This is a project-isolation contract, not yet a claim of a complete security
boundary. Implementation work should enforce it before the docs present it as
fully shipped behavior.

## Workflow

The intended Neovim workflow is:

1. Place the cursor on code and create or reuse an external mark.
2. Open `:HarnessdPanel` to show the scratchpad/whiteboard panel for that mark.
3. Add plain notes, attach a thread, or create a linked scratch artifact.
4. Use the mark/thread browser to scroll through project marks.
5. Cycle marks directly from the source buffer and see whether a thread is
   attached.
6. Open linked scratch files from the panel.
7. Jump from the panel back to the marked source location.

Marks can exist without threads. A plain mark is still useful as a bookmark,
review note, or source-location anchor.

## Storage And Cleanup

Scratch output should live outside the workspace.

Default durable scratch roots:

- Unix: `~/.local/share/harnessd/scratch/<workspace-hash>/<thread-id>/`
- Windows: `%LOCALAPPDATA%\harnessd\scratch\<workspace-hash>\<thread-id>\`

The settings page should expose a scratch storage toggle:

- `runtime`: default durable storage under the harnessd runtime directory.
- `temp`: ephemeral storage under the operating system temp directory.

Threads own their associated scratch artifacts. Deleting a thread deletes its
scratch files. Deleting a mark that has an attached thread should prompt before
deleting both the thread and its scratch files. Recursive cleanup must be
guarded so it can only remove configured harnessd scratch roots.

## Architecture Target

The daemon remains the single long-lived process and owns runtime state:

- external marks
- optional threads attached to marks
- scratch artifact index and cleanup metadata
- settings, including model selection and scratch storage mode
- local IPC endpoint and lifecycle state

Neovim and CLI surfaces should stay thin. They should call the daemon for mark,
thread, scratch, and settings operations rather than owning state themselves.

Target concepts:

- `Mark`: external source location with `mark_id`, `workspace`, `file`,
  `current_line`, `byte_offset`, `line_hash`, optional `thread_id`, and status.
- `Thread`: optional conversation attached to a mark with `thread_id`,
  `mark_id`, title or prompt, storage mode, scratch root, and created/updated
  timestamps.
- `ScratchArtifact`: external file with `artifact_id`, optional `thread_id`,
  absolute path, display path, title, source reference, bytes, lines, and
  created timestamp.
- `Settings`: model selection, `scratch_storage_mode = runtime | temp`, and
  `read_scope = current_context`.

## Neovim Target Workflow

Neovim is the primary UI.

- `:HarnessdPanel` opens the scratchpad/whiteboard panel for the current mark
  or attached thread.
- `:HarnessdThreads` opens the mark/thread browser.
- Mark cycling moves through external marks and shows whether each mark has an
  attached thread.
- `:HarnessdSettings` controls model selection and scratch storage mode.
- Linked scratch files open from the panel and live outside the workspace by
  default.
- Deleting a mark with an attached thread prompts before removing both.

Some current command names still reflect the previous thread-first design. They
may remain temporarily while behavior moves toward marks and scratchpad
semantics.

## Platform Notes

The daemon uses a local IPC transport:

- Windows: TCP loopback with the selected port recorded in `daemon.port`.
- Linux/macOS: Unix domain sockets.

Windows named pipes remain a future transport option. Unix socket behavior
should stay stable.

## Dependencies

The crate currently depends on:

- `tokio`: async runtime
- `interprocess`: local sockets
- `tokio-util` and `tokio-serde`: framed JSON over stream transport
- `serde` and `serde_json`: JSON-RPC message types and serialization
- `clap`: CLI argument parsing
- `anyhow`: error handling
- `tracing` and `tracing-subscriber`: structured logging
- `dirs`: platform directory resolution

## Getting Started

### Prerequisites

- Nix with flakes enabled, or a stable Rust toolchain.
- Authenticated `codex` on `PATH` only for scratch/thread features that invoke
  Codex.

### Build

```bash
cargo build
```

On NixOS:

```bash
nix develop
cargo test
nix build
```

### Run

```bash
cargo run
```

### Dashboard

```bash
cargo run -- tui
```

The dashboard currently reports daemon health, IPC state, cache/runtime data,
and recent activity. Its target role is to report marks, scratch storage,
settings, and cleanup state.

### Lifecycle

```bash
cargo run -- setup --path .
cargo run -- setup --no-tui
cargo run -- teardown
cargo run -- doctor
```

`setup` creates the runtime dir, starts the daemon if needed, waits for IPC
readiness, verifies state through `status`, and optionally opens the dashboard.
`teardown` stops the daemon and waits for the lock and IPC endpoint to
disappear cleanly. `doctor` reports stale lock/port files and common runtime
mismatches.

### Test

```bash
cargo test
```

## Project Layout

- `src/main.rs`: binary entrypoint
- `src/scratch.rs`: scratch artifact generation and writing
- `src/threads.rs`: current persistent anchored thread store
- `integrations/`: editor integration assets
- `Cargo.toml`: crate metadata and dependencies
- `Cargo.lock`: resolved dependency graph

## Legacy Removal

Autocomplete, ghost-text preview, inline completion, LSP completion, Zed
autocomplete, proposal cache behavior, and ACP generation are no longer product
goals. Existing code may keep those surfaces temporarily during the transition,
but new work should move toward marks, scratch artifacts, settings, and guarded
cleanup.

## Current Status

The repository still contains legacy autocomplete APIs, LSP/Zed support, ACP
generation paths, proposal cache code, and command names from the earlier
code-thread sidecar design.

The backend now writes scratch artifacts outside the workspace, persists
daemon-owned scratch storage settings, stores independent external marks,
attaches newly created threads to marks, exposes mark cycling through RPC/CLI,
and deletes thread-owned scratch directories through `thread.delete`. The
Neovim UI can browse and cycle marks and can toggle scratch storage mode, but
the main panel still needs a fuller mark-first scratchpad view instead of the
transitional thread-first behavior.

No `priv/` roadmap files are present in this checkout. `TODO.md` is the current
public roadmap for the pivot.
