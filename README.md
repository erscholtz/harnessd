# harnessd

`harnessd` is a Rust daemon + CLI for saved-file, anchor-driven inline
completions. A long-lived daemon owns parse and proposal state; clients can
inspect TODO/FIXME-style anchors cheaply and request ACP generation explicitly.

## Goal (v0.0.1)

Prove the end-to-end architecture:

- **Daemon**: `harnessd --daemon`
- **Protocol**: JSON-RPC 2.0 over a local socket
- **CLI client**: `harnessd research "<query>"`
- **Editor bridge**: `harnessd bridge ...` (one-shot request → stdout response)

For v0.0.1, `research` is a **stub** (no network): it should accept a query (and optionally sources) and return a structured JSON result.

## Architecture (current plan)

- **Single source of truth**: the daemon owns all state and long-lived connections.
- **Local RPC**: clients connect to a local socket and send JSON-RPC requests.
- **Two front-ends**:
  - **CLI**: runs commands like `research`; starts the daemon if needed.
  - **Editor stdio bridge**: editor tasks can pipe a request to `harnessd`, which forwards it to the daemon and prints the response.

Planned daemon “context” (not all in v0.0.1):

- crawl cache of fetched docs
- sessions/history
- vector DB for indexed docs/chunks
- (later) model client/connection pool, streaming responses

## JSON-RPC Surface

Requests are JSON-RPC 2.0. Implemented methods:

- `anchors({ file })` returns saved-file anchor ranges and `candidate`, `ready`,
  or `failed` state without starting generation.
- `generate({ file, offset })` returns one bounded insertion suggestion when
  `offset` is within an anchor marker. An uncached request launches `codex-acp`
  over stdio ACP.
- `inline({ file, offset, content, prompt })` returns ephemeral bounded ACP
  insertion text using the editor's live buffer; it is not written to cache.
- `complete({ file, offset, prefix })` remains a cache-only lookup.
- `prefetch({ path })` remains a debug/cache warming path.

## Platform notes

v0.0.1 uses a local IPC transport:

- **Windows**: TCP loopback with the selected port recorded in `daemon.port`
- **Linux/macOS**: Unix domain sockets

Windows named pipes remain a future transport option; Unix socket behavior is
unchanged.

## Dependencies (current)

The crate currently depends on:

- `tokio`: async runtime
- `interprocess`: local sockets (named pipes / Unix domain sockets)
- `tokio-util` + `tokio-serde`: framed JSON over a stream transport
- `serde` + `serde_json`: JSON-RPC message types and serialization
- `clap`: CLI argument parsing
- `anyhow`: error handling
- `tracing` + `tracing-subscriber`: structured logging
- `dirs`: platform directory resolution

## Getting started

### Prerequisites

- Nix with flakes enabled, or a stable Rust toolchain
- `codex-acp` and authenticated `codex` on `PATH` for uncached generation

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

This opens a live terminal dashboard that polls the daemon for health, cache,
IPC, and recent proposal information. It also shows local runtime/DB state when
the daemon is offline. Inside the dashboard, press `p` to open the project
picker, select a recent path from the dropdown-style list, or choose
`Browse...` to walk directories and pick the root to prefetch.

### Lifecycle

```bash
cargo run -- setup --path .
cargo run -- setup --no-tui
cargo run -- teardown
cargo run -- doctor
```

`setup` creates the runtime dir, starts the daemon if needed, waits for IPC
readiness, verifies the cache/database state through `status`, and optionally
warms the cache with `prefetch`, then opens the dashboard by default. Use
`--no-tui` when you only want the bootstrap step. `teardown` stops the daemon
and waits for the lock and IPC endpoint to disappear cleanly. `doctor` reports
stale lock/port files and other common runtime mismatches in a user-facing
format.

## Editor integrations

Editor-specific adapters live under `integrations/`.

- `integrations/nvim/`: Neovim Lua UI for anchored Codex threads, `inline`, cached `complete`, and `prefetch`
- `integrations/zed/`: Zed wrapper scripts and a local dev extension for Rust autocomplete

To test Zed autocomplete locally:

```powershell
cargo build
```

Then install `integrations/zed/extension/` with `zed: install dev extension`.
The extension launches `harnessd lsp`, which prefetches Rust files on open/save
and returns cached harnessd proposals through Zed's normal completion popup.

Neovim supports anchored Codex threads in a right sidebar. `:HarnessdAsk`
prompts for a task, stores a line anchor in `~/.local/share/harnessd/threads.json`,
shows an `H` marker on the source line, and launches a real
`codex --no-alt-screen` terminal session in the sidebar. The sidebar lists
project-first saved sessions from `~/.codex/sessions` and can reopen linked
threads later.

The older ghost-text insertion flow is still available as `:HarnessdInline`,
which sends live buffer contents through:

```bash
printf '%s' "$BUFFER_CONTENT" | harnessd inline --file src/main.rs --offset 10 --prompt "insert validation"
```

Use `:HarnessdThreads` to toggle the Codex thread sidebar. Use
`:HarnessdComplete` to preview a saved-file cache hit, then
`:HarnessdAccept` or `:HarnessdDismiss` to resolve either preview.

### Test

```bash
cargo test
```

## Project layout

- `src/main.rs`: binary entrypoint
- `integrations/`: editor and IDE integration assets
- `Cargo.toml`: crate metadata + dependencies
- `Cargo.lock`: resolved dependency graph (checked in)
- `.gitignore`: ignores `target/`, build noise (`*.pdb`), IDE/OS cruft, env files, and `priv/local/` for machine-specific notes; `priv/plan.txt` and `priv/TODO.md` are meant to be committable

## What’s explicitly not in v0.0.1

- tmux panes / research window layout; the current Codex-session UI is the
  Neovim sidebar
- vector DB, crawl cache, embeddings
- model client (e.g. Kimi), SSE pool, streaming (`stream_next`)
- full `research` and `index` behavior

## Current scope

- daemon lifecycle, IPC, tree-sitter parsing, proposal cache, and cached
  `complete` are in place
- `anchors` inspection and explicit bounded `generate` through ACP are daemon
  APIs; Neovim also exposes ephemeral freeform `inline` asks with ghost-text
  preview and guarded acceptance
- Neovim can create persistent line-anchored Codex threads and reopen saved
  Codex CLI sessions without owning Codex auth or model state
- background cache warming, priority, and latency/cache-hit metrics remain
  autocomplete follow-up work
