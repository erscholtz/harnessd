# Zed

This folder contains legacy Zed completion assets from the previous product
direction.

The new `harnessd` direction is Neovim-first marks, scratchpad/whiteboard
views, external scratch files, and guarded cleanup. Zed completion support is
not part of that direction, and normal setup should not install the Zed dev
extension.

Files currently kept for removal or migration:

- `complete.ps1`: legacy one-shot completion bridge.
- `prefetch.ps1`: legacy cache warmup bridge.
- `extension/`: legacy local dev extension that launched `harnessd lsp`.

Avoid adding new behavior here unless it directly supports removing or
migrating the legacy Zed autocomplete path.
