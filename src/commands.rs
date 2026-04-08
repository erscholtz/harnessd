use std::path::Path;
use std::process::Command;

use anyhow::Context;

use crate::cli::Commands;
use crate::daemon_lock::{read_daemon_pid, DaemonLock};
use crate::paths;

pub async fn run(command: Commands) -> anyhow::Result<()> {
    match command {
        Commands::Daemon => run_daemon().await,
        Commands::Stop => run_stop(),
        Commands::Research { query, manual } => run_research(&query, manual.as_deref()).await,
        Commands::ZedBridge {
            method,
            file,
            line,
            text,
            cursor,
        } => run_zed_bridge(&method, file.as_deref(), line, text.as_deref(), cursor.as_deref()).await,
    }
}

async fn run_daemon() -> anyhow::Result<()> {
    let dir = paths::runtime_dir();
    let _lock = DaemonLock::acquire(&dir)
        .with_context(|| format!("could not acquire daemon lock under {}", dir.display()))?;

    tracing::info!(
        runtime_dir = %dir.display(),
        lock = %_lock.path().display(),
        pid = std::process::id(),
        "daemon started — exit with Ctrl+C, SIGTERM (`kill`), or `harnessd stop`"
    );

    crate::shutdown::wait_for_shutdown().await;
    tracing::info!("daemon shutting down cleanly");
    Ok(())
}

/// Ask the running daemon to exit gracefully (SIGTERM on Unix; `taskkill` without `/F` on Windows).
fn run_stop() -> anyhow::Result<()> {
    let dir = paths::runtime_dir();
    let pid = read_daemon_pid(&dir)?;
    let lock_path = dir.join("daemon.lock");

    #[cfg(unix)]
    {
        let status = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .map_err(|e| anyhow::anyhow!("failed to run `kill` (is coreutils/PATH available?): {e}"))?;

        if status.success() {
            tracing::info!(pid, "sent SIGTERM to daemon");
            return Ok(());
        }

        // Often exit code 1 when the PID does not exist — treat as stale lock.
        if is_stale_kill_failure_unix(pid) {
            std::fs::remove_file(&lock_path).ok();
            anyhow::bail!(
                "no process with pid {} (removed stale lock).",
                pid
            );
        }

        anyhow::bail!("`kill -TERM {}` failed ({status}); daemon may still be running", pid);
    }

    #[cfg(windows)]
    {
        // `/T` terminates the process tree so child processes are not left behind.
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T"])
            .status()
            .map_err(|e| anyhow::anyhow!("failed to run `taskkill`: {e}"))?;

        if status.success() {
            tracing::info!(pid, "requested graceful stop via taskkill");
            return Ok(());
        }

        // 128 = not running (typical for stale pid file)
        std::fs::remove_file(&lock_path).ok();
        anyhow::bail!(
            "taskkill failed ({status}). If the daemon is not running, stale lock file was removed if present."
        );
    }
}

#[cfg(unix)]
fn is_stale_kill_failure_unix(pid: u32) -> bool {
    // `kill -0` checks existence without sending a signal.
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
}

async fn run_research(query: &str, manual: Option<&Path>) -> anyhow::Result<()> {
    tracing::info!(query, ?manual, "research entry (client not wired yet)");
    anyhow::bail!("research client is not implemented yet");
}

async fn run_zed_bridge(
    method: &str,
    file: Option<&Path>,
    line: Option<u32>,
    text: Option<&str>,
    cursor: Option<&str>,
) -> anyhow::Result<()> {
    tracing::info!(
        method,
        ?file,
        line,
        text_len = text.map(str::len),
        cursor,
        "zed-bridge entry (not wired yet)"
    );
    anyhow::bail!("zed-bridge is not implemented yet");
}
