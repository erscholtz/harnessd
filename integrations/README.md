# Integrations

Editor glue lives here. Integrations should stay thin and call the daemon for
marks, optional threads, settings, scratch metadata, and cleanup operations.

## Direction

Neovim is the primary UI for the scratchpad/whiteboard workflow:

- external source marks
- optional threads attached to marks
- scratch artifacts stored outside the project tree
- model and scratch-storage settings
- mark browsing, cycling, and source jumps

Future integrations can be added here without moving editor-specific files into
`src/`.

## Current Folders

- `nvim/`: primary Neovim helper for marks, threads, scratch views, settings,
  and source jumps.
- `zed/`: legacy Zed completion assets kept only until the old completion path
  is removed.

## Legacy Note

Older integration paths for inline completion, cached completion, prefetching,
and bridge-based completion are not part of the new product direction. Avoid
building new work on them.
