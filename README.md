# harnessd

`harnessd` is a Rust daemon + CLI that provides a **local “research harness”** via a small JSON-RPC API. A long-lived daemon owns all state, and multiple front-ends can talk to it (starting with a CLI client and an editor stdio bridge).

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

## JSON-RPC (planned surface)

Requests are JSON-RPC 2.0. Planned methods:

- `research(query, sources[], depth)` → result (later: streaming handle)
- `inline(context, query)` → markdown response
- `complete(file, cursor_pos, prefix)` → suggestions
- `index(path_or_url)` → start background indexing
- `stream_next(handle)` → chunk / EOF (later)

In v0.0.1, only `research` is required; other methods may return “method not found”.

## Platform notes

v0.0.1 uses a **cross-platform local socket** transport:

- **Windows**: named pipes
- **Linux/macOS**: Unix domain sockets

This keeps IPC local (no TCP ports) while allowing native Windows development.

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

- Rust toolchain (stable) via `rustup`

### Build

```bash
cargo build
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

- `integrations/nvim/`: Neovim Lua wrapper around `harnessd complete` and `harnessd prefetch`
- `integrations/zed/`: Zed wrapper scripts and a local dev extension for Rust autocomplete

To test Zed autocomplete locally:

```powershell
cargo build
```

Then install `integrations/zed/extension/` with `zed: install dev extension`.
The extension launches `harnessd lsp`, which prefetches Rust files on open/save
and returns cached harnessd proposals through Zed's normal completion popup.

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

- tmux panes / research window layout
- vector DB, crawl cache, embeddings
- model client (e.g. Kimi), SSE pool, streaming (`stream_next`)
- full `inline`, `complete`, `index` behavior

## Next edits (from `priv/TODO.md`)

- dependencies are in place; next is wiring the daemon/client/bridge around them
- implement daemon: runtime dir + single-instance lock + socket listener + JSON-RPC parsing/dispatch
- implement CLI client: connect/send/print; spawn daemon if missing
- implement `bridge`: forward one RPC and print JSON for editor tasks

## Next edits (README)

As you refine requirements, we can fold them into:

- a crisp 1–2 sentence purpose statement
- a short feature list
- concrete usage examples
- a simple roadmap (if you want one)
