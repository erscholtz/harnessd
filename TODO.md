# TODO: Non-Interfering Scratchpad And Whiteboard

Goal: make `harnessd` a Neovim-first scratchpad and text whiteboard that can
anchor notes and optional threads to source locations without letting Codex
write to the project directory.

## Product Contract

- Codex must not edit or write inside the project tree.
- Codex receives only current context by default: current file/selection plus
  explicitly requested context.
- Scratch artifacts are written outside the project tree.
- Marks are external metadata and can exist without threads.
- Threads attach to marks and own their scratch artifacts.
- Deleting a thread deletes its scratch files.
- Deleting a mark with an attached thread prompts before deleting both.

## Roadmap

1. [x] Documentation pivot.
   - Rewrite README, TODO, AGENTS, and integration docs around the new
     scratchpad/whiteboard direction.
   - Mark legacy autocomplete and completion surfaces as removal targets.

2. [x] Move scratch storage outside the project tree.
   - Default to `~/.local/share/harnessd/scratch/<workspace-hash>/<thread-id>/`
     on Unix.
   - Default to `%LOCALAPPDATA%\harnessd\scratch\<workspace-hash>\<thread-id>\`
     on Windows.
   - Stop writing new artifacts under workspace-local scratch directories.

3. [x] Add a settings-page toggle for scratch storage.
   - Support `runtime` durable storage as the default.
   - Support `temp` storage under the OS temp directory.
   - Persist the selected storage mode through daemon-owned settings.

4. [x] Add an independent external mark store.
   - Introduce `Mark` records with `mark_id`, workspace, file, current line,
     byte offset, line hash, optional `thread_id`, and status.
   - Reanchor marks from line hashes and live buffer content.
   - Keep marks out of source files.

5. [x] Attach threads to marks.
   - Convert thread creation to attach to an existing or newly created mark.
   - Let marks exist without threads.
   - Keep thread metadata separate from mark identity.

6. [~] Build the mark/thread menu and mark cycling.
   - List marks and show attached thread state.
   - Cycle previous/next marks from the source buffer.
   - Jump from panel items back to source.
   - RPC, CLI, Neovim mark browsing, and Neovim mark cycling exist.
   - The panel still needs a fuller mark-first scratchpad view instead of the
     transitional thread-first behavior.

7. [~] Add thread deletion with guarded scratch cleanup.
   - Delete a thread's scratch directory when the thread is removed.
   - Prompt before deleting a mark that has an attached thread.
   - Guard recursive deletion so only configured harnessd scratch roots can be
     removed.
   - Backend deletion and guard are implemented; explicit Neovim deletion UI
     confirmation still needs wiring.

8. [ ] Remove legacy code-assist surfaces.
   - Remove autocomplete, ghost-text preview, inline completion, ACP generation,
     LSP completion, Zed autocomplete, and proposal cache product paths.
   - Keep compatibility shims only when needed for a short migration window.

9. [~] Add focused tests.
   - Read-only/current-context request shape.
   - Scratch path safety outside the project tree.
   - Runtime-vs-temp scratch storage selection.
   - Thread deletion and scratch cleanup.
   - Mark creation, reanchoring, cycling, and thread attachment.
   - Backend and Neovim headless coverage exists for the implemented pieces.

## Target API And CLI Direction

These names describe implementation direction, not a guarantee that the current
binary already exposes them:

- `mark.create`, `mark.list`, `mark.delete`, `mark.next`, `mark.prev`
- `thread.create`, `thread.list`, `thread.delete`, `thread.attach`
- `scratch.create`, `scratch.list`, `scratch.delete`
- `settings.get`, `settings.update`

## Non-Goals

- Do not reintroduce AI writes to project files.
- Do not add new autocomplete, ghost-text, inline, or LSP completion work.
- Do not make Zed autocomplete part of the new product direction.
- Do not insert marker comments into source files.
- Do not document workspace-local scratch directories as target behavior.
