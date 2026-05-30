# Neovim

This folder contains a minimal Neovim-side adapter for the current
autocomplete-first `harnessd` daemon.

Files:

- `lua/harnessd.lua`: thin Lua wrapper around the `harnessd` CLI

The module currently exposes:

- `setup(opts)` to register user commands
- `complete(opts, callback)` to request completions for a file + byte offset
- `thread_ask()` to create or reopen a line-anchored Codex thread
- `inline_ask()` to ask for ephemeral ACP insertion text using the live buffer
- `sidebar_toggle()` to open the Codex thread/session sidebar
- `preview_complete()` to render the first saved-file cache hit as ghost text
- `accept()` and `dismiss()` to manage the active ghost preview
- `prefetch(path, callback)` to warm the proposal cache

Minimal setup:

```lua
vim.opt.rtp:append([[D:/School + Work/dev/harnessd/integrations/nvim]])
require("harnessd").setup()
```

Config options:

```lua
require("harnessd").setup({
  command = "harnessd",
  codex_command = "codex",
  sidebar_width = 72,
  session_limit = 50,
  thread_sign_text = "H",
})
```

Registered commands:

- `:HarnessdPrefetch [path]`
- `:HarnessdCompleteDebug`
- `:HarnessdAsk`
- `:HarnessdInline`
- `:HarnessdThreads`
- `:HarnessdThreadOpen`
- `:HarnessdThreadAttach`
- `:HarnessdComplete`
- `:HarnessdAccept`
- `:HarnessdDismiss`

`HarnessdAsk` opens a native floating prompt, creates a persistent line anchor,
marks the source line with `H`, and launches a real
`codex --no-alt-screen -C <workspace> ...` session in a right sidebar. Saved
Codex sessions are read from `~/.codex/sessions` through the harnessd CLI and
shown project-first.

`HarnessdInline` is the former `HarnessdAsk` ghost-text insertion flow. It
sends the current buffer, including unsaved edits, through `harnessd inline`.
Its answer is rendered as virtual text and is inserted only with
`HarnessdAccept`. `HarnessdComplete` uses the same preview surface for existing
cached completions, and requires the buffer to be saved first.

`HarnessdThreads` toggles the sidebar. Inside the sidebar, `<CR>` opens the
selected linked thread or saved Codex session, `r` refreshes, and `q` closes the
sidebar. `HarnessdThreadAttach` opens the sidebar in attach mode so pressing
`<CR>` on a saved session links it to the current line.

The adapter defines `<Plug>` mappings without assigning user keys:

```lua
vim.keymap.set({ "n", "i" }, "<C-k>", "<Plug>(HarnessdAsk)")
vim.keymap.set({ "n", "i" }, "<C-i>", "<Plug>(HarnessdInline)")
vim.keymap.set({ "n", "i" }, "<C-l>", "<Plug>(HarnessdComplete)")
vim.keymap.set({ "n", "i" }, "<C-y>", "<Plug>(HarnessdAccept)")
vim.keymap.set({ "n", "i" }, "<C-e>", "<Plug>(HarnessdDismiss)")
vim.keymap.set("n", "<leader>ht", "<Plug>(HarnessdThreads)")
```

Headless UI test:

```bash
nvim --headless -u NONE -l integrations/nvim/tests/headless.lua
```
