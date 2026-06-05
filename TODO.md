# TODO: Inline Autocomplete Refactor

Goal: make inline autocomplete behave like a fast ghost-text provider, similar to Kilo Code: editor requests must return quickly from local state/cache, while model work happens through prepared sessions and background refreshes.

## Current shape

- `inline` is an explicit slow model call in `src/ipc/methods.rs`.
- `inline.fast` is the right editor-facing direction: live buffer in, cache lookup first, optional background refresh on miss.
- `complete` and `inline.fast` currently share `complete_from_content`, which makes saved-file completion and live-buffer inline autocomplete harder to reason about separately.
- ACP inline generation is already bounded in `src/acp.rs`; keep that guardrail.

## Refactor plan

1. [x] Create a dedicated autocomplete module.
   - Add `src/autocomplete.rs`.
   - Move live-buffer autocomplete orchestration out of `src/ipc/methods.rs`.
   - Keep JSON-RPC request parsing in `ipc/methods.rs`; call into the new module for behavior.

2. [ ] Split saved-file completion from live-buffer inline autocomplete.
   - Keep `complete(file, offset, prefix)` as the saved-file/cache inspection path.
   - Make `inline.fast` use a live-buffer-specific lookup function.
   - Avoid coupling ghost-text behavior to disk reads.

3. [~] Define a single live autocomplete pipeline.
   - Validate cursor offset against live content.
   - Parse live content with tree-sitter.
   - Resolve a stable region around the cursor.
   - Look up bounded cached proposals by stable region key.
   - Apply prefix filtering.
   - Return the first usable suggestion immediately.
   - On miss, optionally queue a deduped background refresh.

4. [ ] Make region/key behavior explicit.
   - Prefer TODO/FIXME/comment anchors when the cursor is inside their context.
   - Fall back to enclosing function when available.
   - Fall back to a bounded cursor window only for refresh dedupe, not broad proposal reuse.
   - Include file path, byte range, content hash, prompt, model, and reasoning effort in refresh dedupe keys.

5. [x] Keep model generation off the hot path.
   - `inline.fast` must not await ACP/model generation.
   - `inline.prepare` should warm reusable ACP sessions for a workspace/file.
   - Background refresh stores capped snippets in the proposal cache for future requests.
   - Preserve max-lines and max-bytes enforcement before anything enters the cache.

6. [ ] Clarify method roles.
   - `inline.fast`: editor ghost-text provider.
   - `inline.prepare`: optional prewarm when an editor opens/focuses a file.
   - `inline`: explicit command/palette "ask at cursor", not keystroke autocomplete.
   - `complete`: saved-file/debug/cache completion path.

7. [ ] Update editor bridge docs after the code move.
   - Document that integrations should call `inline.fast` for inline autocomplete.
   - Document that live buffer content is required on stdin for bridge calls.
   - Warn users to disable competing inline providers if ghost text conflicts.

8. [ ] Add focused tests.
   - `inline.fast` returns cache hits without queueing refresh.
   - `inline.fast` queues at most one equivalent refresh on repeated misses.
   - Refresh keys change when region content changes.
   - Prefix filtering applies to live autocomplete.
   - Oversized ACP output is rejected and never cached.
   - `inline` remains usable as an explicit slow command.

## Suggested implementation order

1. [x] Add `src/autocomplete.rs` and move helper functions with no behavior change.
2. [ ] Add tests around existing behavior before changing contracts.
3. [ ] Rename internal functions to distinguish saved-file completion from live autocomplete.
4. [x] Route `inline.fast` through the new module.
5. [ ] Tighten docs and integration examples.
6. [x] Run `cargo test`.

## Progress notes

- 2026-06-05: Added `src/autocomplete.rs` and moved `inline`, `inline.fast`, `inline.prepare`, cursor context construction, live-buffer validation, and background inline refresh orchestration out of `src/ipc/methods.rs`.
- 2026-06-05: `ipc/methods.rs` now remains responsible for JSON-RPC handling plus shared cache/anchor helpers; `inline.fast` behavior is still intentionally unchanged.
- 2026-06-05: `cargo check` passes after the module split.
- 2026-06-05: `cargo fmt --check` and `cargo test` pass.

## Non-goals for this slice

- Do not add research, vector indexing, or tmux workflows.
- Do not make autocomplete wait on network/model generation.
- Do not return uncapped model output as ghost text.
- Do not refactor unrelated CLI, TUI, scratch, or agent features.
