# Zed

`harnessd` already exposes a thin `bridge` command. This folder keeps the
editor-facing wrappers next to the rest of the integration assets instead of
burying them inside `src/`.

Files:

- `complete.ps1`: one-shot completion bridge for Windows/Zed tasks
- `prefetch.ps1`: cache warmup bridge for Windows/Zed tasks
- `extension/`: local Zed dev extension that launches `harnessd lsp` for Rust autocomplete

Both wrappers keep the editor side thin and forward straight to:

- `harnessd bridge --method complete ...`
- `harnessd bridge --method prefetch ...`

The PowerShell wrappers accept explicit arguments so you can wire them into
whatever Zed task shape you prefer without changing daemon code.

## Autocomplete

Build `harnessd`, then install `extension/` with `zed: install dev extension`.
The extension registers a Rust language server that runs:

```powershell
harnessd lsp
```

The LSP adapter stores open document text, prefetches on open/save, and returns
daemon-backed completion items through Zed's normal autocomplete popup.
