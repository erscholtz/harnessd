# Integrations

Editor and IDE glue lives here.

The daemon remains the single long-lived process; integrations in this folder
should stay thin and call into `harnessd complete`, `harnessd prefetch`, or
`harnessd bridge` rather than reimplementing daemon behavior.

Current integrations:

- `nvim/` for Neovim helpers
- `zed/` for Zed bridge wrappers

Planned future integrations can be added here without mixing editor-specific
files into `src/`.
