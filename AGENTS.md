# Agent implementation guidelines — harnessd

Use this when changing code or planning work. Detailed roadmap lives in **`priv/plan.txt`** and **`priv/TODO.md`** (local-only if `/priv/` stays ignored; keep this file accurate either way).

## North star

- **Autocomplete first**: daemon + **tree-sitter** + **proposal cache** + fast **`complete(file, cursor, …)`**. Cursor path = cache lookup when possible, not a full model round-trip every time.
- **Background work** warms the cache (TODO/FIXME / `todo!()` / `unimplemented!()` first). **Research**, tmux, vector DB, Kimi/SSE, and heavy **index** are **later**; do not block autocomplete milestones on them.

## Architecture rules

- **Single long-lived process** owns parse cache, proposal store, and (later) model clients. CLI and Zed bridge are **thin clients** over JSON-RPC (or equivalent) on IPC.
- **IPC**: Unix domain socket first; Windows may use named pipe or TCP loopback — document quirks, avoid breaking Unix behavior.
- **Runtime dir**: `~/.local/share/harnessd/` (Unix) or `%LOCALAPPDATA%\harnessd\` (Windows). Lockfile / single-instance behavior must fail fast with a clear error if another daemon holds the endpoint.
- **Daemon shutdown**: support **easy, non-zombie teardown** — Tokio waits for tasks to finish on graceful exit; the daemon listens for **Ctrl+C**, **SIGTERM** (Unix), and **`harnessd stop`** (SIGTERM / graceful `taskkill`). The **`daemon.lock`** file contains the PID and is removed when the process exits cleanly (including after signals, via `Drop`). Avoid `SIGKILL` in docs unless stuck.
- **JSON-RPC 2.0**: preserve `id`, return structured errors for malformed JSON and unknown methods.

### Subprocesses and zombies

- If you **spawn child processes** (workers, compilers, etc.), **await** or **wait** on them, or use **`tokio::process`** and drive completion explicitly. Unreaped terminated children become **zombies** on Unix.
- When the CLI **auto-spawns** the daemon, avoid patterns where a short-lived parent exits without reaping an intermediate child; prefer a single spawn with a well-defined parent, or documented **double-fork** / **detach** behavior. On Windows, **`taskkill /T`** stops a process **tree** so stray children are less likely when you add them later.

## Proposal cache and guards

- Every stored “agentic” snippet must be **bounded**: enforce **max lines** and **max bytes** per proposal; reject or truncate anything over cap — completions must **feel like autocomplete**, not thousand-line diffs.
- Keys should tie to **stable identity** for a region: e.g. file path + byte range or node fingerprint + content hash; **invalidate** when that region’s content changes.
- **`complete`**: resolve cursor to enclosing/relevant **tree-sitter** node (or comment anchor), then **lookup cache**; only then consider slower paths. Stub/no-network path should remain testable.

## Code change discipline

- **Minimal diffs**: match existing style in `src/` (modules, error handling with `anyhow` if that’s what the crate uses, `tracing` patterns). No drive-by refactors or unrelated formatting sweeps.
- **Dependencies**: add only what the current milestone needs; prefer crates already aligned with the stack (e.g. `tokio`, `serde`).
- **Tests**: add focused tests for RPC shape, cache keys, cap enforcement, and cursor→node resolution where feasible without a full editor.

## Order of implementation (when unsure)

1. Daemon skeleton: runtime dir, lock, IPC, JSON-RPC router.  
2. Tree-sitter integration + at least one grammar needed for dogfooding.  
3. Proposal store (SQLite or similar).  
4. **`complete`** with cache hit/miss and stubs.  
5. Background TODO/anchor scanner + job queue (placeholder generator before real LLM).  
6. **`zed-bridge`** for `complete`.  
7. Prefetch + priority; metrics/logging for latency and cache hits.  
8. Later: `research`, `inline`, `index`, vector/crawl, Kimi pool — per plan.

## Files to read before large edits

- `priv/plan.txt` — architecture and rationale.  
- `priv/TODO.md` — checklist and acceptance for the current slice.  
- `src/main.rs`, `src/commands.rs` (and any new modules) — current entrypoints and CLI shape.

## Do not

- Implement full research/tmux/vector pipelines **before** `complete` + cache + TODO pipeline are demonstrably working.  
- Return uncapped model output directly as completion text.  
- Introduce vendored `vendor/` trees unless the user explicitly asks for vendoring again.
