# Neovim

This folder contains the primary `harnessd` UI. The target experience is a
pinned scratchpad and text-whiteboard panel for external source marks, optional
threads, linked scratch artifacts, and source jumps.

Files:

- `lua/harnessd.lua`: compatibility entrypoint for the Neovim plugin.
- `lua/harnessd/init.lua`: main Lua implementation.

## Minimal Setup

```lua
vim.opt.rtp:append([[D:/School + Work/dev/harnessd/integrations/nvim]])
require("harnessd").setup()
```

## Target Workflow

- Create or reuse an external mark at the cursor.
- Open `:HarnessdPanel` to show the scratchpad/whiteboard panel for the current
  mark or attached thread.
- Use `:HarnessdThreads` to browse marks and see which marks have attached
  threads.
- Cycle marks from the source buffer and show attached thread state when
  present.
- Add scratch notes or linked artifacts without editing the source buffer.
- Open linked scratch files from the panel.
- Jump from a panel item back to the marked source location.

Marks can exist without threads. A plain mark is still a useful source-location
bookmark or review note.

## Commands

Current command names may be transitional while behavior moves toward
mark/scratchpad semantics.

- `:HarnessdPanel`: opens the scratchpad/whiteboard panel for the current mark
  or attached thread.
- `:HarnessdPanelFlip`: cycles panel views such as notes, scratch artifacts, and
  mark/thread browse.
- `:HarnessdThreads`: opens the mark/thread browser.
- `:HarnessdMarks`: opens the same mark/thread browser.
- `:HarnessdMarkNext`: jumps to the next external mark in the current file.
- `:HarnessdMarkPrev`: jumps to the previous external mark in the current file.
- `:HarnessdExample`: creates a linked scratch artifact for the active thread
  or marked location.
- `:HarnessdThreadOpen`: opens the thread attached to the current mark when one
  exists.
- `:HarnessdThreadAttach`: attaches an existing thread/session to the current
  mark.
- `:HarnessdSettings`: opens model and scratch-storage settings.

`HarnessdAsk` may remain as a temporary compatibility alias for
`HarnessdPanel`.

## Panel Keys

Target panel behavior:

- `<Tab>` flips panel mode.
- `e` creates a linked scratch artifact.
- `g` jumps to the active source mark.
- `<CR>` opens the selected mark, thread, scratch artifact, or session.
- `r` refreshes.
- `q` closes the panel.

## Settings

The settings panel includes:

- model selection
- scratch storage mode:
  - `runtime`: default durable storage under the harnessd runtime dir
  - `temp`: ephemeral storage under the OS temp dir
- read-only project access with current-context read scope

Target config shape:

```lua
require("harnessd").setup({
  command = "harnessd",
  codex_command = "codex",
  sidebar_width = 72,
  thread_sign_text = "H",
  scratch_storage_mode = "runtime",
  read_scope = "current_context",
  model_roles = {
    ask = { model = nil, reasoning_effort = nil },
    scratch = { model = nil, reasoning_effort = nil },
  },
})
```

## Scratch Storage

Linked scratch files should live outside the workspace by default:

- Unix: `~/.local/share/harnessd/scratch/<workspace-hash>/<thread-id>/`
- Windows: `%LOCALAPPDATA%\harnessd\scratch\<workspace-hash>\<thread-id>\`

The `temp` settings toggle should redirect new scratch files to a harnessd-owned
directory under the OS temp dir. Deleting a thread deletes its scratch files.
Deleting a mark with an attached thread prompts before removing both the thread
and its scratch files.

## Legacy Note

The previous autocomplete, ghost-text, inline completion, LSP completion, and
Zed completion surfaces are not part of the new Neovim product direction.
Existing commands or tests may remain temporarily while the implementation is
being cleaned up, but new work should target marks, scratch artifacts, settings,
and cleanup.

## Headless UI Test

```bash
nvim --headless -u NONE -l integrations/nvim/tests/headless.lua
```
