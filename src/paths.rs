//! Canonical runtime paths (see `AGENTS.md` / product plan).

use std::path::PathBuf;

/// User data directory for the daemon: `~/.local/share/harnessd` (Unix) or
/// `%LOCALAPPDATA%\harnessd` (Windows).
pub fn runtime_dir() -> PathBuf {
    #[cfg(windows)]
    {
        dirs::data_local_dir()
            .expect("LOCALAPPDATA should be set on Windows")
            .join("harnessd")
    }
    #[cfg(unix)]
    {
        dirs::home_dir()
            .expect("HOME should be set")
            .join(".local/share/harnessd")
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
