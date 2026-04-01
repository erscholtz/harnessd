# harnessd

`harnessd` is a Rust daemon + CLI that provides a **local “research harness”** via a small JSON-RPC API. A long-lived daemon owns all state, and multiple front-ends can talk to it (starting with a CLI client and a Zed stdio bridge).

## Goal (v0.0.1)

Prove the end-to-end architecture:

- **Daemon**: `harnessd --daemon`
- **Protocol**: JSON-RPC 2.0 over a local socket
- **CLI client**: `harnessd research "<query>"`
- **Zed bridge**: `harnessd zed-bridge ...` (one-shot request → stdout response)

For v0.0.1, `research` is a **stub** (no network): it should accept a query (and optionally sources) and return a structured JSON result.

## Architecture (current plan)

- **Single source of truth**: the daemon owns all state and long-lived connections.
- **Local RPC**: clients connect to a local socket and send JSON-RPC requests.
- **Two front-ends**:
  - **CLI**: runs commands like `research`; starts the daemon if needed.
  - **Zed stdio bridge**: Zed tasks can pipe a request to `harnessd`, which forwards it to the daemon and prints the response.

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

### Test

```bash
cargo test
```

## Project layout

- `src/main.rs`: binary entrypoint
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
- implement `zed-bridge`: forward one RPC and print JSON for Zed tasks

## Next edits (README)

As you refine requirements, we can fold them into:

- a crisp 1–2 sentence purpose statement
- a short feature list
- concrete usage examples
- a simple roadmap (if you want one)

