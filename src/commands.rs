use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::cli::Commands;
use crate::daemon_lock::{DaemonLock, read_daemon_pid};
use crate::paths;
use crate::rpc::{CompleteParams, JsonRpcRequest, PrefetchParams, PrefetchResult, StatusResult};
use crate::runtime;

pub async fn run(command: Commands) -> anyhow::Result<()> {
    match command {
        Commands::Daemon => run_daemon().await,
        Commands::Setup { path, no_tui } => run_setup(path.as_deref(), no_tui).await,
        Commands::Teardown => run_teardown().await,
        Commands::Doctor => run_doctor().await,
        Commands::Stop => run_stop().await,
        Commands::Tui => crate::tui::run().await,
        Commands::Complete {
            file,
            offset,
            prefix,
        } => run_complete(&file, offset, prefix.as_deref()).await,
        Commands::Prefetch { path } => run_prefetch(&path).await,
        Commands::Research { query, manual } => run_research(&query, manual.as_deref()).await,
        Commands::ZedBridge {
            method,
            file,
            line,
            text,
            cursor,
        } => {
            run_zed_bridge(
                &method,
                file.as_deref(),
                line,
                text.as_deref(),
                cursor.as_deref(),
            )
            .await
        }
    }
}

pub async fn teardown_runtime() -> anyhow::Result<()> {
    run_teardown().await
}

async fn run_daemon() -> anyhow::Result<()> {
    let dir = paths::runtime_dir();
    cleanup_stale_runtime_state(&dir).await?;
    let _lock = DaemonLock::acquire(&dir)
        .with_context(|| format!("could not acquire daemon lock under {}", dir.display()))?;

    tracing::info!(
        runtime_dir = %dir.display(),
        lock = %_lock.path().display(),
        pid = std::process::id(),
        "daemon started — exit with Ctrl+C, SIGTERM (`kill`), or `harnessd stop`"
    );

    // Initialize shared state (cache + parser)
    let state = crate::state::DaemonState::new(dir.clone())?;

    // Create shutdown channel for coordinated shutdown
    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
    let shutdown_tx_clone = shutdown_tx.clone();

    // Spawn shutdown signal handler
    let shutdown_handle = tokio::spawn(async move {
        crate::shutdown::wait_for_shutdown().await;
        let _ = shutdown_tx_clone.send(()).await;
    });

    // Start IPC server
    let ipc_handle = tokio::spawn(async move {
        if let Err(e) = crate::ipc::serve(state, shutdown_tx, shutdown_rx).await {
            tracing::error!(error = %e, "IPC server error");
        }
    });

    // Wait for either task to complete (shutdown signal or IPC error)
    tokio::select! {
        _ = shutdown_handle => {
            tracing::info!("shutdown signal received");
        }
        _ = ipc_handle => {
            tracing::info!("IPC server exited");
        }
    }

    tracing::info!("daemon shutting down cleanly");
    Ok(())
}

async fn run_setup(path: Option<&Path>, no_tui: bool) -> anyhow::Result<()> {
    let runtime_dir = paths::runtime_dir();
    tokio::fs::create_dir_all(&runtime_dir)
        .await
        .with_context(|| format!("failed to create runtime dir {}", runtime_dir.display()))?;
    cleanup_stale_runtime_state(&runtime_dir).await?;

    if daemon_ready().await {
        tracing::info!(runtime_dir = %runtime_dir.display(), "daemon already running");
    } else {
        tracing::info!(runtime_dir = %runtime_dir.display(), "starting daemon");
        start_daemon()?;
        wait_for_daemon_ready(Duration::from_secs(5))
            .await
            .with_context(|| runtime::render_report(&runtime::inspect(&runtime_dir, false)))?;
    }

    // `status` verifies that IPC is up and the daemon state opened the cache DB.
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "status".to_string(),
        params: None,
        id: Some(serde_json::json!(1)),
    };
    let status =
        read_status_response(&send_rpc_request_once(&serde_json::to_string(&request)?).await?)?;
    if let Some(path) = path {
        let prefetch = request_prefetch(path).await?;
        print_setup_summary(&status, Some((path, &prefetch)));
    } else {
        print_setup_summary(&status, None);
    }

    if !no_tui {
        crate::tui::run().await?;
    }

    Ok(())
}

/// Ask the running daemon to exit gracefully (SIGTERM on Unix; `taskkill` without `/F` on Windows).
async fn run_stop() -> anyhow::Result<()> {
    let dir = paths::runtime_dir();
    if request_daemon_shutdown().await? {
        wait_for_daemon_shutdown(Duration::from_secs(5)).await?;
        return Ok(());
    }
    let pid = read_daemon_pid(&dir)?;
    let lock_path = dir.join("daemon.lock");

    #[cfg(unix)]
    {
        let status = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .map_err(|e| {
                anyhow::anyhow!("failed to run `kill` (is coreutils/PATH available?): {e}")
            })?;

        if status.success() {
            tracing::info!(pid, "sent SIGTERM to daemon");
            wait_for_daemon_shutdown(Duration::from_secs(5)).await?;
            return Ok(());
        }

        // Often exit code 1 when the PID does not exist — treat as stale lock.
        if is_stale_kill_failure_unix(pid) {
            std::fs::remove_file(&lock_path).ok();
            anyhow::bail!("no process with pid {} (removed stale lock).", pid);
        }

        anyhow::bail!(
            "`kill -TERM {}` failed ({status}); daemon may still be running",
            pid
        );
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
            wait_for_daemon_shutdown(Duration::from_secs(5)).await?;
            return Ok(());
        }

        // 128 = not running (typical for stale pid file)
        std::fs::remove_file(&lock_path).ok();
        anyhow::bail!(
            "taskkill failed ({status}). The daemon may require forceful termination; stale lock file was removed if present."
        );
    }
}

async fn run_teardown() -> anyhow::Result<()> {
    let runtime_dir = paths::runtime_dir();

    if !runtime_dir.join("daemon.lock").exists() {
        cleanup_stale_runtime_state(&runtime_dir).await?;
        tracing::info!(runtime_dir = %runtime_dir.display(), "daemon is not running");
        return Ok(());
    }

    run_stop().await
}

async fn run_doctor() -> anyhow::Result<()> {
    let runtime_dir = paths::runtime_dir();
    let endpoint_reachable = daemon_ready().await;
    let health = runtime::inspect(&runtime_dir, endpoint_reachable);
    println!("{}", runtime::render_report(&health));
    Ok(())
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

async fn run_complete(file: &Path, offset: usize, prefix: Option<&str>) -> anyhow::Result<()> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "complete".to_string(),
        params: Some(serde_json::to_value(CompleteParams {
            file: file.canonicalize()?.to_string_lossy().to_string(),
            offset,
            prefix: prefix.map(str::to_string),
        })?),
        id: Some(serde_json::json!(1)),
    };
    println!("{}", send_rpc_request(&request).await?);
    Ok(())
}

async fn run_prefetch(path: &Path) -> anyhow::Result<()> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "prefetch".to_string(),
        params: Some(serde_json::to_value(PrefetchParams {
            path: path.canonicalize()?.to_string_lossy().to_string(),
        })?),
        id: Some(serde_json::json!(1)),
    };
    println!("{}", send_rpc_request(&request).await?);
    Ok(())
}

async fn run_zed_bridge(
    method: &str,
    file: Option<&Path>,
    line: Option<u32>,
    text: Option<&str>,
    cursor: Option<&str>,
) -> anyhow::Result<()> {
    let request = build_zed_request(method, file, line, text, cursor)?;
    println!("{}", send_rpc_request(&request).await?);
    Ok(())
}

fn build_zed_request(
    method: &str,
    file: Option<&Path>,
    line: Option<u32>,
    _text: Option<&str>,
    cursor: Option<&str>,
) -> anyhow::Result<JsonRpcRequest> {
    match method {
        "complete" => {
            let file = file.context("`--file` is required for `zed-bridge --method complete`")?;
            let cursor = cursor
                .context("`--cursor` is required for `zed-bridge --method complete`")?
                .parse::<usize>()
                .context("`--cursor` must be a byte offset for `complete`")?;
            Ok(JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                method: method.to_string(),
                params: Some(serde_json::to_value(CompleteParams {
                    file: file.canonicalize()?.to_string_lossy().to_string(),
                    offset: cursor,
                    prefix: None,
                })?),
                id: Some(serde_json::json!(1)),
            })
        }
        "prefetch" => {
            let file_or_dir =
                file.context("`--file` is required for `zed-bridge --method prefetch`")?;
            Ok(JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                method: method.to_string(),
                params: Some(serde_json::to_value(PrefetchParams {
                    path: file_or_dir.canonicalize()?.to_string_lossy().to_string(),
                })?),
                id: Some(serde_json::json!(1)),
            })
        }
        _ => {
            let mut data = serde_json::Map::new();
            if let Some(file) = file {
                data.insert(
                    "file".to_string(),
                    serde_json::Value::String(file.canonicalize()?.to_string_lossy().to_string()),
                );
            }
            if let Some(line) = line {
                data.insert("line".to_string(), serde_json::json!(line));
            }
            if let Some(cursor) = cursor {
                data.insert("cursor".to_string(), serde_json::json!(cursor));
            }
            Ok(JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                method: method.to_string(),
                params: Some(serde_json::Value::Object(data)),
                id: Some(serde_json::json!(1)),
            })
        }
    }
}

async fn send_rpc_request(request: &JsonRpcRequest) -> anyhow::Result<String> {
    let payload = serde_json::to_string(request)?;

    match send_rpc_request_once(&payload).await {
        Ok(response) => Ok(response),
        Err(first_error) => {
            tracing::info!(error = %first_error, "daemon unavailable, starting a new instance");
            cleanup_stale_runtime_state(&paths::runtime_dir()).await?;
            start_daemon()?;
            wait_for_daemon_ready(Duration::from_secs(5))
                .await
                .with_context(|| {
                    runtime::render_report(&runtime::inspect(&paths::runtime_dir(), false))
                })?;
            send_rpc_request_once(&payload)
                .await
                .with_context(|| format!("request failed after daemon startup: {first_error}"))
        }
    }
}

async fn send_rpc_request_once(payload: &str) -> anyhow::Result<String> {
    #[cfg(unix)]
    {
        let stream = tokio::net::UnixStream::connect(paths::socket_path())
            .await
            .context("failed to connect to daemon socket")?;
        send_payload(stream, payload).await
    }

    #[cfg(windows)]
    {
        let port = tokio::fs::read_to_string(paths::port_file())
            .await
            .context("failed to read daemon port file")?;
        let addr = format!("127.0.0.1:{}", port.trim());
        let stream = tokio::net::TcpStream::connect(&addr)
            .await
            .with_context(|| format!("failed to connect to daemon at {addr}"))?;
        send_payload(stream, payload).await
    }
}

fn read_status_response(response: &str) -> anyhow::Result<StatusResult> {
    let response: crate::rpc::JsonRpcResponse = serde_json::from_str(response)
        .with_context(|| format!("invalid status response: {response}"))?;
    if let Some(error) = response.error {
        anyhow::bail!("status RPC failed: {} ({})", error.message, error.code);
    }
    let result = response
        .result
        .context("status response was missing `result`")?;
    Ok(serde_json::from_value(result)?)
}

async fn request_prefetch(path: &Path) -> anyhow::Result<PrefetchResult> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "prefetch".to_string(),
        params: Some(serde_json::to_value(PrefetchParams {
            path: path.canonicalize()?.to_string_lossy().to_string(),
        })?),
        id: Some(serde_json::json!(1)),
    };
    let response = send_rpc_request(&request).await?;
    let response: crate::rpc::JsonRpcResponse = serde_json::from_str(&response)
        .with_context(|| format!("invalid prefetch response: {response}"))?;
    if let Some(error) = response.error {
        anyhow::bail!("prefetch RPC failed: {} ({})", error.message, error.code);
    }
    let result = response
        .result
        .context("prefetch response was missing `result`")?;
    Ok(serde_json::from_value(result)?)
}

fn print_setup_summary(status: &StatusResult, prefetch: Option<(&Path, &PrefetchResult)>) {
    println!(
        "daemon ready: pid {} at {}",
        status.pid, status.ipc_endpoint
    );
    println!(
        "cache db: {} ({} proposals)",
        status.cache_db_path, status.cache.total_proposals
    );
    if let Some((path, result)) = prefetch {
        println!(
            "prefetch: {} files, {} anchors, {} proposals from {}",
            result.scanned_files,
            result.anchors_found,
            result.proposals_stored,
            path.display()
        );
    }
    println!(
        "dashboard: {}",
        if prefetch.is_some() {
            "opening after prefetch"
        } else {
            "opening now"
        }
    );
}

async fn request_daemon_shutdown() -> anyhow::Result<bool> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "shutdown".to_string(),
        params: None,
        id: Some(serde_json::json!(1)),
    };
    let payload = serde_json::to_string(&request)?;
    match send_rpc_request_once(&payload).await {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

async fn send_payload<S>(stream: S, payload: &str) -> anyhow::Result<String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (reader, mut writer) = tokio::io::split(stream);
    writer.write_all(payload.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;

    let mut reader = BufReader::new(reader);
    let mut response = String::new();
    reader.read_line(&mut response).await?;
    Ok(response.trim().to_string())
}

fn start_daemon() -> anyhow::Result<()> {
    let current_exe = std::env::current_exe().context("failed to resolve current executable")?;
    let daemon_exe = daemon_executable_path(&current_exe)?;
    let mut cmd = Command::new(daemon_exe);
    cmd.arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn().context("failed to spawn daemon process")?;
    Ok(())
}

fn daemon_executable_path(current_exe: &Path) -> anyhow::Result<std::path::PathBuf> {
    #[cfg(windows)]
    {
        let runtime_dir = paths::runtime_dir();
        std::fs::create_dir_all(&runtime_dir)?;
        let daemon_copy = runtime_dir.join("harnessd-daemon.exe");
        std::fs::copy(current_exe, &daemon_copy).with_context(|| {
            format!(
                "failed to prepare daemon executable copy at {}",
                daemon_copy.display()
            )
        })?;
        Ok(daemon_copy)
    }

    #[cfg(not(windows))]
    {
        Ok(current_exe.to_path_buf())
    }
}

async fn wait_for_daemon_ready(timeout: Duration) -> anyhow::Result<()> {
    let start = Instant::now();
    loop {
        if daemon_ready().await {
            return Ok(());
        }
        if start.elapsed() >= timeout {
            anyhow::bail!("daemon did not become ready within {:?}", timeout);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_daemon_shutdown(timeout: Duration) -> anyhow::Result<()> {
    let start = Instant::now();
    loop {
        if !daemon_ready().await && !paths::runtime_dir().join("daemon.lock").exists() {
            return Ok(());
        }
        if start.elapsed() >= timeout {
            anyhow::bail!("daemon did not shut down within {:?}", timeout);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn daemon_ready() -> bool {
    #[cfg(unix)]
    {
        tokio::net::UnixStream::connect(paths::socket_path())
            .await
            .is_ok()
    }

    #[cfg(windows)]
    {
        match tokio::fs::read_to_string(paths::port_file()).await {
            Ok(port) => {
                let addr = format!("127.0.0.1:{}", port.trim());
                tokio::net::TcpStream::connect(addr).await.is_ok()
            }
            Err(_) => false,
        }
    }
}

async fn cleanup_stale_runtime_state(runtime_dir: &Path) -> anyhow::Result<()> {
    let initial = runtime::inspect(runtime_dir, daemon_ready().await);
    let cleaned = runtime::cleanup_stale_files(runtime_dir, initial.endpoint_reachable)?;
    if initial.stale_lock && !cleaned.lock_exists {
        tracing::warn!(runtime_dir = %runtime_dir.display(), "removed stale daemon lock");
    }
    #[cfg(windows)]
    if initial.stale_port_file && !cleaned.port_file_exists {
        tracing::warn!(runtime_dir = %runtime_dir.display(), "removed stale daemon port file");
    }

    Ok(())
}
