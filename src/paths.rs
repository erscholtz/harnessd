//! Canonical runtime paths (see `AGENTS.md` / product plan).

use std::path::PathBuf;

/// User data directory for the daemon: `~/.local/share/harnessd` (Unix) or
/// `%LOCALAPPDATA%\harnessd` (Windows).
pub fn runtime_dir() -> PathBuf {
    #[cfg(windows)]
    {
        runtime_root()
    }
    #[cfg(unix)]
    {
        runtime_root()
    }
}

/// IPC socket path for Unix platforms.
pub fn socket_path() -> PathBuf {
    runtime_dir().join("daemon.sock")
}

/// IPC port file path for Windows loopback mode.
pub fn port_file() -> PathBuf {
    runtime_dir().join("daemon.port")
}

/// Recent project selections used by the TUI project picker.
pub fn recent_projects_path() -> PathBuf {
    runtime_dir().join("recent-projects.json")
}

/// Persistent Neovim line-thread anchors.
pub fn threads_path() -> PathBuf {
    runtime_dir().join("threads.json")
}

#[cfg(windows)]
fn runtime_root() -> PathBuf {
    dirs::data_local_dir()
        .expect("LOCALAPPDATA should be set on Windows")
        .join("harnessd")
}

#[cfg(unix)]
fn runtime_root() -> PathBuf {
    dirs::home_dir()
        .expect("HOME should be set")
        .join(".local/share/harnessd")
}
