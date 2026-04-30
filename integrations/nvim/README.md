# Neovim

This folder contains a minimal Neovim-side adapter for the current
autocomplete-first `harnessd` daemon.

Files:

- `lua/harnessd.lua`: thin Lua wrapper around the `harnessd` CLI

The module currently exposes:

- `setup(opts)` to register user commands
- `complete(opts, callback)` to request completions for a file + byte offset
- `prefetch(path, callback)` to warm the proposal cache

Minimal setup:

```lua
vim.opt.rtp:append([[D:/School + Work/dev/harnessd/integrations/nvim]])
require("harnessd").setup()
```

Registered commands:

- `:HarnessdPrefetch [path]`
- `:HarnessdCompleteDebug`

This is intentionally small. A future `nvim-cmp` source or native completion
bridge can sit on top of this module once the daemon protocol settles.
