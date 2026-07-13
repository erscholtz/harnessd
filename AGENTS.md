# Agent implementation guidelines - harnessd

Use this when changing code or planning work. `harnessd` is pivoting to a
Neovim-first scratchpad and text whiteboard with external source marks. Keep
this file accurate as the implementation catches up.

## North Star

- **Non-interfering scratchpad first**: `harnessd` should help the user think
  beside code without letting Codex edit or write to the project directory.
- **External marks are primary**: source locations are tracked as daemon-owned
  metadata. Marks can exist without threads and must not insert source comments.
- **Threads are optional attachments**: a thread may attach to a mark, and the
  thread owns its scratch artifacts.
- **Scratch lives outside projects**: generated scratch files default to the
  runtime dir and may use the OS temp dir when the settings toggle selects it.
- **No new completion work**: do not add new autocomplete, ghost-text, inline,
  LSP completion, or Zed completion behavior.

## Architecture Rules

- **Single long-lived process** owns runtime state: marks, optional threads,
  scratch artifact metadata, settings, cleanup state, and any future model
  clients. CLI and editor integrations are thin clients over JSON-RPC or an
  equivalent local IPC protocol.
- **IPC**: Unix domain socket first; Windows may use named pipe or TCP loopback.
  Document platform quirks without breaking Unix behavior.
- **Runtime dir**: `~/.local/share/harnessd/` on Unix or
  `%LOCALAPPDATA%\harnessd\` on Windows. Lockfile and single-instance behavior
  must fail fast with a clear error if another daemon holds the endpoint.
- **Scratch roots**:
  - runtime mode: `runtime_dir/scratch/<workspace-hash>/<thread-id>/`
  - temp mode: OS temp dir under a harnessd-owned scratch root
- **Daemon shutdown**: support easy, non-zombie teardown. Tokio waits for tasks
  to finish on graceful exit; the daemon listens for Ctrl+C, SIGTERM on Unix,
  and `harnessd stop` through SIGTERM or graceful `taskkill`. The `daemon.lock`
  file contains the PID and is removed when the process exits cleanly, including
  after signals through `Drop`. Avoid `SIGKILL` in docs unless stuck.
- **JSON-RPC 2.0**: preserve `id`, return structured errors for malformed JSON
  and unknown methods.

### Subprocesses And Zombies

- If you spawn child processes, await or wait on them, or use `tokio::process`
  and drive completion explicitly. Unreaped terminated children become zombies
  on Unix.
- When the CLI auto-spawns the daemon, avoid patterns where a short-lived
  parent exits without reaping an intermediate child. Prefer a single spawn
  with a well-defined parent, or documented double-fork/detach behavior. On
  Windows, `taskkill /T` stops a process tree so stray children are less likely.

## Scratch And Cleanup Guards

- Never document or implement project-tree scratch storage as the target.
- Codex has read-only project access and may read only current context by
  default: current file/selection plus explicitly requested context.
- Every generated artifact must be bounded: enforce max lines and max bytes;
  reject or truncate output over cap.
- Scratch paths must be rooted under the configured runtime or temp scratch
  root. Do not accept arbitrary deletion paths from client input.
- Recursive cleanup must verify the path is inside a configured harnessd
  scratch root before deleting anything.
- Deleting a thread deletes its scratch artifacts.
- Deleting a mark with an attached thread must prompt before deleting both.

## Code Change Discipline

- **Minimal diffs**: match existing style in `src/` for modules, error handling,
  and tracing. Avoid drive-by refactors or unrelated formatting sweeps.
- **Dependencies**: add only what the current milestone needs; prefer crates
  already aligned with the stack, such as `tokio` and `serde`.
- **Tests**: add focused tests for read-only context shape, scratch path safety,
  cleanup guards, mark reanchoring, mark cycling, and thread attachment.

## Order Of Implementation When Unsure

1. Update docs and remove old product framing.
2. Move scratch storage outside the project tree.
3. Add daemon-owned settings for model choice and scratch storage mode.
4. Introduce external marks independent of threads.
5. Attach threads to marks and link scratch artifacts to threads.
6. Build Neovim mark browsing, mark cycling, and source jumps.
7. Add guarded thread deletion and scratch cleanup.
8. Remove autocomplete, inline, ACP generation, LSP completion, Zed completion,
   and proposal cache product paths.

## Files To Read Before Large Edits

- `README.md` - user-facing product direction.
- `TODO.md` - current roadmap and acceptance checklist.
- `src/main.rs`, `src/commands.rs`, and new modules touched by the work.
- Neovim integration files under `integrations/nvim/` before UI changes.

## Do Not

- Do not reintroduce AI writes to project files.
- Do not add new autocomplete, ghost-text, inline, LSP completion, or Zed
  completion surfaces.
- Do not insert marker comments into source files.
- Do not treat workspace-local scratch directories as the target.
- Do not recursively delete paths unless they are verified under a configured
  harnessd scratch root.
- Do not introduce tmux as a required process manager for the panel workflow.
- Do not introduce vendored `vendor/` trees unless the user explicitly asks for
  vendoring again.
