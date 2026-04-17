//! Runtime state diagnostics shared by setup, status, and the TUI.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::daemon_lock;

/// Summary of local runtime health.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuntimeHealth {
    /// Runtime directory path.
    pub runtime_dir: String,
    /// Lock file path.
    pub lock_path: String,
    /// Whether the lock file exists.
    pub lock_exists: bool,
    /// PID parsed from the lock file, if any.
    pub lock_pid: Option<u32>,
    /// Whether the lock file points at a missing process or contains invalid data.
    pub stale_lock: bool,
    /// Port file path on Windows loopback mode.
    pub port_file_path: String,
    /// Whether the port file exists.
    pub port_file_exists: bool,
    /// Whether the port file exists but the endpoint is not reachable.
    pub stale_port_file: bool,
    /// Whether the IPC endpoint accepted a connection.
    pub endpoint_reachable: bool,
    /// Human-readable warnings about stale or inconsistent runtime state.
    pub warnings: Vec<String>,
}

/// Inspect the runtime directory and current IPC reachability.
pub fn inspect(runtime_dir: &Path, endpoint_reachable: bool) -> RuntimeHealth {
    let lock_path = runtime_dir.join("daemon.lock");
    let lock_exists = lock_path.exists();
    let lock_pid = daemon_lock::read_pid(&lock_path);
    let stale_lock = lock_exists
        && match lock_pid {
            Some(pid) => !daemon_lock::process_is_running(pid),
            None => true,
        };

    let port_file = crate::paths::port_file();
    let port_file_exists = port_file.exists();
    #[cfg(windows)]
    let stale_port_file = port_file_exists && !endpoint_reachable;
    #[cfg(not(windows))]
    let stale_port_file = false;

    let mut warnings = Vec::new();
    if stale_lock {
        warnings.push(match lock_pid {
            Some(pid) => format!("stale daemon lock: pid {pid} is not running"),
            None => "stale daemon lock: pid is missing or invalid".to_string(),
        });
    }
    if stale_port_file {
        warnings.push("stale daemon port file: endpoint is not reachable".to_string());
    }
    if lock_exists && !endpoint_reachable && !stale_lock {
        warnings.push("daemon lock exists but IPC endpoint is not reachable".to_string());
    }

    RuntimeHealth {
        runtime_dir: runtime_dir.display().to_string(),
        lock_path: lock_path.display().to_string(),
        lock_exists,
        lock_pid,
        stale_lock,
        port_file_path: port_file.display().to_string(),
        port_file_exists,
        stale_port_file,
        endpoint_reachable,
        warnings,
    }
}

/// Remove stale lifecycle files that point at no running daemon.
pub fn cleanup_stale_files(
    runtime_dir: &Path,
    endpoint_reachable: bool,
) -> anyhow::Result<RuntimeHealth> {
    let mut health = inspect(runtime_dir, endpoint_reachable);

    if health.stale_lock {
        std::fs::remove_file(runtime_dir.join("daemon.lock")).ok();
    }

    #[cfg(windows)]
    if health.stale_port_file || (health.stale_lock && health.port_file_exists) {
        std::fs::remove_file(crate::paths::port_file()).ok();
    }

    health = inspect(runtime_dir, endpoint_reachable);
    Ok(health)
}

/// Format a concise diagnostics block for CLI output.
pub fn render_report(health: &RuntimeHealth) -> String {
    let mut lines = vec![
        format!("runtime_dir: {}", health.runtime_dir),
        format!("endpoint_reachable: {}", yes_no(health.endpoint_reachable)),
        format!("lock_exists: {}", yes_no(health.lock_exists)),
        format!(
            "lock_pid: {}",
            health
                .lock_pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "none".to_string())
        ),
        format!("stale_lock: {}", yes_no(health.stale_lock)),
    ];

    #[cfg(windows)]
    {
        lines.push(format!(
            "port_file_exists: {}",
            yes_no(health.port_file_exists)
        ));
        lines.push(format!(
            "stale_port_file: {}",
            yes_no(health.stale_port_file)
        ));
    }

    if health.warnings.is_empty() {
        lines.push("warnings: none".to_string());
    } else {
        lines.push("warnings:".to_string());
        for warning in &health.warnings {
            lines.push(format!("- {warning}"));
        }
    }

    lines.join("\n")
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
