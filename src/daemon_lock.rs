//! Single-instance lock file containing the daemon PID (for `harnessd stop`).

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Exclusive `daemon.lock` under the runtime dir; removed when the guard is dropped
/// (normal exit, panic, or after dropping following shutdown).
pub struct DaemonLock {
    path: PathBuf,
}

impl DaemonLock {
    /// Creates `runtime_dir` if needed and acquires an exclusive lock file.
    pub fn acquire(runtime_dir: &Path) -> anyhow::Result<Self> {
        fs::create_dir_all(runtime_dir)?;
        let path = runtime_dir.join("daemon.lock");

        match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(mut file) => {
                writeln!(file, "{}", std::process::id())?;
                file.sync_all()?;
                Ok(Self { path })
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                let existing = read_pid(&path).unwrap_or(0);
                anyhow::bail!(
                    "another daemon instance may be running (lock {:?}, pid {}). \
                     Stop it with `harnessd stop` or remove the lock if the process is gone.",
                    path,
                    existing
                );
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Path to the lock file (for diagnostics).
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Remove a stale daemon lock if its pid no longer exists.
pub fn remove_stale_lock(runtime_dir: &Path) -> anyhow::Result<bool> {
    let path = runtime_dir.join("daemon.lock");
    if !path.exists() {
        return Ok(false);
    }

    let Some(pid) = read_pid(&path) else {
        fs::remove_file(&path)?;
        return Ok(true);
    };

    if process_is_running(pid) {
        return Ok(false);
    }

    fs::remove_file(&path)?;
    Ok(true)
}

impl Drop for DaemonLock {
    fn drop(&mut self) {
        if let Err(e) = fs::remove_file(&self.path) {
            tracing::warn!(path = %self.path.display(), error = %e, "failed to remove daemon lock file");
        }
    }
}

/// Read a PID from a lock file, returning `None` for missing or invalid files.
pub fn read_pid(path: &Path) -> Option<u32> {
    let s = fs::read_to_string(path).ok()?;
    s.trim().parse().ok()
}

/// Read PID from an existing lock file (for `stop`).
pub fn read_daemon_pid(runtime_dir: &Path) -> anyhow::Result<u32> {
    let path = runtime_dir.join("daemon.lock");
    let s = fs::read_to_string(&path).map_err(|_| {
        anyhow::anyhow!(
            "no daemon lock at {}; is the daemon running?",
            path.display()
        )
    })?;
    s.trim()
        .parse::<u32>()
        .map_err(|_| anyhow::anyhow!("invalid pid in {}", path.display()))
}

#[cfg(unix)]
/// Whether a process with this PID appears to be alive on Unix.
pub fn process_is_running(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
/// Whether a process with this PID appears to be alive on Windows.
pub fn process_is_running(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}")])
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
        })
        .unwrap_or(false)
}
