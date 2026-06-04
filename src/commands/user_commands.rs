use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::cli::{Commands, ThreadCommands};
use crate::daemon_lock::{DaemonLock, read_daemon_pid};
use crate::paths;
use crate::rpc::{
    AnchorsParams, CodexSessionsParams, CompleteParams, GenerateParams, InlineParams,
    JsonRpcRequest, PrefetchParams, PrefetchResult, ScratchCreateParams, StatusResult,
    ThreadAttachParams, ThreadCreateParams, ThreadLinkParams, ThreadListParams,
    ThreadResolveParams,
};
use crate::runtime;

/// Run the selected CLI subcommand.
pub async fn run(command: Commands) -> anyhow::Result<()> {
    match command {
        Commands::Daemon => run_daemon().await,
        Commands::Setup { path, no_tui } => run_setup(path.as_deref(), no_tui).await,
        Commands::Teardown => run_teardown().await,
        Commands::Doctor => run_doctor().await,
        Commands::Stop => run_stop().await,
        Commands::Tui => crate::tui::run(None).await,
        Commands::Lsp => crate::lsp::run_stdio().await,
        Commands::Complete {
            file,
            offset,
            prefix,
        } => run_complete(&file, offset, prefix.as_deref()).await,
        Commands::Inline {
            file,
            offset,
            prompt,
        } => run_inline(&file, offset, &prompt).await,
        Commands::Scratch {
            workspace,
            file,
            offset,
            prompt,
            selection_start,
            selection_end,
        } => {
            run_scratch(
                &workspace,
                &file,
                offset,
                &prompt,
                selection_start,
                selection_end,
            )
            .await
        }
        Commands::Prefetch { path } => run_prefetch(&path).await,
        Commands::CodexSessions {
            workspace,
            all,
            limit,
        } => run_codex_sessions(&workspace, all, limit).await,
        Commands::Thread { command } => run_thread(command).await,
        Commands::Research { query, manual } => run_research(&query, manual.as_deref()).await,
        Commands::Bridge {
            method,
            file,
            line,
            text,
            cursor,
        } => {
            run_bridge(
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

/// Tear down any running daemon and clean stale runtime files.
pub async fn teardown_runtime() -> anyhow::Result<()> {
    run_teardown().await
}

/// Warm the proposal cache for a path selected from the dashboard.
pub async fn prefetch_runtime_path(path: &Path) -> anyhow::Result<PrefetchResult> {
    request_prefetch(path).await
}

/// Ask the daemon for cached completions for a file and byte offset.
pub async fn complete_runtime_file(
    file: &Path,
    offset: usize,
    prefix: Option<&str>,
) -> anyhow::Result<Vec<crate::rpc::CompletionSuggestion>> {
    request_complete(file, offset, prefix).await
}

async fn run_daemon() -> anyhow::Result<()> {
    let dir = paths::runtime_dir();
    cleanup_stale_runtime_state(&dir).await?;
    let lock = DaemonLock::acquire(&dir)
        .with_context(|| format!("could not acquire daemon lock under {}", dir.display()))?;

    tracing::info!(
        runtime_dir = %dir.display(),
        lock = %lock.path().display(),
        pid = std::process::id(),
        "daemon started — exit with Ctrl+C, SIGTERM (`kill`), or `harnessd stop`"
    );

    // The daemon owns long-lived state so requests can stay on the local fast path.
    let state = crate::state::DaemonState::new(dir.clone())?;

    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
    let shutdown_tx_clone = shutdown_tx.clone();

    let shutdown_handle = tokio::spawn(async move {
        crate::shutdown::wait_for_shutdown().await;
        let _ = shutdown_tx_clone.send(()).await;
    });

    let ipc_handle = tokio::spawn(async move {
        if let Err(e) = crate::ipc::serve(state, shutdown_tx, shutdown_rx).await {
            tracing::error!(error = %e, "IPC server error");
        }
    });

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
    let request = rpc_request::<serde_json::Value>("status", None)?;
    let status =
        read_status_response(&send_rpc_request_once(&serde_json::to_string(&request)?).await?)?;
    if let Some(path) = path {
        let prefetch = request_prefetch(path).await?;
        print_setup_summary(&status, Some((path, &prefetch)));
    } else {
        print_setup_summary(&status, None);
    }

    if !no_tui {
        crate::tui::run(path.map(PathBuf::from)).await?;
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
    let request = rpc_request(
        "complete",
        Some(CompleteParams {
            file: canonicalize_rpc_path(file)?,
            offset,
            prefix: prefix.map(str::to_string),
        }),
    )?;
    println!("{}", send_rpc_request(&request).await?);
    Ok(())
}

async fn run_inline(file: &Path, offset: usize, prompt: &str) -> anyhow::Result<()> {
    let content = read_stdin_optional().await?;
    if content.is_empty() {
        anyhow::bail!("`inline` requires live buffer contents on stdin");
    }
    let request = build_inline_request(file, offset, prompt, content)?;
    println!("{}", send_rpc_request(&request).await?);
    Ok(())
}

async fn read_stdin_optional() -> anyhow::Result<String> {
    if std::io::stdin().is_terminal() {
        return Ok(String::new());
    }
    let mut content = String::new();
    tokio::io::stdin()
        .read_to_string(&mut content)
        .await
        .context("failed to read stdin")?;
    Ok(content)
}

async fn run_scratch(
    workspace: &Path,
    file: &Path,
    offset: usize,
    prompt: &str,
    selection_start: Option<usize>,
    selection_end: Option<usize>,
) -> anyhow::Result<()> {
    let content = read_stdin_optional().await?;
    if content.is_empty() {
        anyhow::bail!("`scratch` requires live buffer contents on stdin");
    }
    let request = build_scratch_request(
        workspace,
        file,
        offset,
        prompt,
        content,
        selection_start,
        selection_end,
    )?;
    println!("{}", send_rpc_request(&request).await?);
    Ok(())
}

fn build_inline_request(
    file: &Path,
    offset: usize,
    prompt: &str,
    content: String,
) -> anyhow::Result<JsonRpcRequest> {
    if prompt.trim().is_empty() {
        anyhow::bail!("`--prompt` must not be empty for `inline`");
    }
    if content.is_empty() {
        anyhow::bail!("`inline` requires live buffer contents on stdin");
    }
    rpc_request(
        "inline",
        Some(InlineParams {
            file: canonicalize_rpc_path(file)?,
            offset,
            content,
            prompt: prompt.to_string(),
        }),
    )
}

fn build_scratch_request(
    workspace: &Path,
    file: &Path,
    offset: usize,
    prompt: &str,
    content: String,
    selection_start: Option<usize>,
    selection_end: Option<usize>,
) -> anyhow::Result<JsonRpcRequest> {
    if prompt.trim().is_empty() {
        anyhow::bail!("`--prompt` must not be empty for `scratch`");
    }
    if content.is_empty() {
        anyhow::bail!("`scratch` requires live buffer contents on stdin");
    }
    rpc_request(
        "scratch.create",
        Some(ScratchCreateParams {
            workspace: canonicalize_rpc_path(workspace)?,
            file: canonicalize_rpc_path(file)?,
            offset,
            content,
            prompt: prompt.to_string(),
            selection_start,
            selection_end,
        }),
    )
}

async fn run_prefetch(path: &Path) -> anyhow::Result<()> {
    let request = rpc_request(
        "prefetch",
        Some(PrefetchParams {
            path: canonicalize_rpc_path(path)?,
        }),
    )?;
    println!("{}", send_rpc_request(&request).await?);
    Ok(())
}

async fn run_codex_sessions(workspace: &Path, all: bool, limit: usize) -> anyhow::Result<()> {
    let request = rpc_request(
        "codex.sessions",
        Some(CodexSessionsParams {
            workspace: canonicalize_rpc_path(workspace)?,
            all,
            limit: Some(limit),
        }),
    )?;
    println!("{}", send_rpc_request(&request).await?);
    Ok(())
}

async fn run_thread(command: ThreadCommands) -> anyhow::Result<()> {
    match command {
        ThreadCommands::Create {
            workspace,
            file,
            offset,
            prompt,
            selection_start,
            selection_end,
        } => {
            let content = read_stdin_optional().await?;
            if content.is_empty() {
                anyhow::bail!("`thread create` requires live buffer contents on stdin");
            }
            let request = rpc_request(
                "thread.create",
                Some(ThreadCreateParams {
                    workspace: canonicalize_rpc_path(&workspace)?,
                    file: canonicalize_rpc_path(&file)?,
                    offset,
                    content,
                    prompt,
                    selection_start,
                    selection_end,
                }),
            )?;
            println!("{}", send_rpc_request(&request).await?);
        }
        ThreadCommands::List { workspace, file } => {
            let content = read_stdin_optional().await?;
            let request = rpc_request(
                "thread.list",
                Some(ThreadListParams {
                    workspace: canonicalize_rpc_path(&workspace)?,
                    file: file.as_deref().map(canonicalize_rpc_path).transpose()?,
                    content: (!content.is_empty()).then_some(content),
                }),
            )?;
            println!("{}", send_rpc_request(&request).await?);
        }
        ThreadCommands::Link {
            thread_id,
            session_id,
            session_path,
        } => {
            let request = rpc_request(
                "thread.link",
                Some(ThreadLinkParams {
                    thread_id,
                    codex_session_id: session_id,
                    codex_session_path: session_path.map(|path| path.display().to_string()),
                }),
            )?;
            println!("{}", send_rpc_request(&request).await?);
        }
        ThreadCommands::Resolve {
            thread_id,
            workspace,
            started_after,
        } => {
            let request = rpc_request(
                "thread.resolve",
                Some(ThreadResolveParams {
                    thread_id,
                    workspace: canonicalize_rpc_path(&workspace)?,
                    started_after_unix: started_after,
                }),
            )?;
            println!("{}", send_rpc_request(&request).await?);
        }
        ThreadCommands::Attach {
            workspace,
            file,
            offset,
            session_id,
        } => {
            let content = read_stdin_optional().await?;
            if content.is_empty() {
                anyhow::bail!("`thread attach` requires live buffer contents on stdin");
            }
            let request = rpc_request(
                "thread.attach",
                Some(ThreadAttachParams {
                    workspace: canonicalize_rpc_path(&workspace)?,
                    file: canonicalize_rpc_path(&file)?,
                    offset,
                    content,
                    codex_session_id: session_id,
                }),
            )?;
            println!("{}", send_rpc_request(&request).await?);
        }
    }
    Ok(())
}

async fn run_bridge(
    method: &str,
    file: Option<&Path>,
    line: Option<u32>,
    text: Option<&str>,
    cursor: Option<&str>,
) -> anyhow::Result<()> {
    let request = build_bridge_request(method, file, line, text, cursor)?;
    println!("{}", send_rpc_request(&request).await?);
    Ok(())
}

fn build_bridge_request(
    method: &str,
    file: Option<&Path>,
    line: Option<u32>,
    _text: Option<&str>,
    cursor: Option<&str>,
) -> anyhow::Result<JsonRpcRequest> {
    match method {
        "complete" => {
            let file = file.context("`--file` is required for `bridge --method complete`")?;
            let cursor = cursor
                .context("`--cursor` is required for `bridge --method complete`")?
                .parse::<usize>()
                .context("`--cursor` must be a byte offset for `complete`")?;
            rpc_request(
                method,
                Some(CompleteParams {
                    file: canonicalize_rpc_path(file)?,
                    offset: cursor,
                    prefix: None,
                }),
            )
        }
        "prefetch" => {
            let file_or_dir =
                file.context("`--file` is required for `bridge --method prefetch`")?;
            rpc_request(
                method,
                Some(PrefetchParams {
                    path: canonicalize_rpc_path(file_or_dir)?,
                }),
            )
        }
        "anchors" => {
            let file = file.context("`--file` is required for `bridge --method anchors`")?;
            rpc_request(
                method,
                Some(AnchorsParams {
                    file: canonicalize_rpc_path(file)?,
                }),
            )
        }
        "generate" => {
            let file = file.context("`--file` is required for `bridge --method generate`")?;
            let offset = cursor
                .context("`--cursor` is required for `bridge --method generate`")?
                .parse::<usize>()
                .context("`--cursor` must be a byte offset for `generate`")?;
            rpc_request(
                method,
                Some(GenerateParams {
                    file: canonicalize_rpc_path(file)?,
                    offset,
                }),
            )
        }
        _ => {
            let mut data = serde_json::Map::new();
            if let Some(file) = file {
                data.insert(
                    "file".to_string(),
                    serde_json::Value::String(canonicalize_rpc_path(file)?),
                );
            }
            if let Some(line) = line {
                data.insert("line".to_string(), serde_json::json!(line));
            }
            if let Some(cursor) = cursor {
                data.insert("cursor".to_string(), serde_json::json!(cursor));
            }
            rpc_request(method, Some(serde_json::Value::Object(data)))
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
    let request = rpc_request(
        "prefetch",
        Some(PrefetchParams {
            path: canonicalize_rpc_path(path)?,
        }),
    )?;
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

async fn request_complete(
    file: &Path,
    offset: usize,
    prefix: Option<&str>,
) -> anyhow::Result<Vec<crate::rpc::CompletionSuggestion>> {
    let request = rpc_request(
        "complete",
        Some(CompleteParams {
            file: canonicalize_rpc_path(file)?,
            offset,
            prefix: prefix.map(str::to_string),
        }),
    )?;
    let response = send_rpc_request(&request).await?;
    let response: crate::rpc::JsonRpcResponse = serde_json::from_str(&response)
        .with_context(|| format!("invalid complete response: {response}"))?;
    if let Some(error) = response.error {
        anyhow::bail!("complete RPC failed: {} ({})", error.message, error.code);
    }
    let result = response
        .result
        .context("complete response was missing `result`")?;
    let suggestions = result
        .get("suggestions")
        .cloned()
        .context("complete response was missing `suggestions`")?;
    Ok(serde_json::from_value(suggestions)?)
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
    let request = rpc_request::<serde_json::Value>("shutdown", None)?;
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
        // Windows keeps the running executable locked, so the daemon starts
        // from a disposable copy inside the runtime directory.
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

fn rpc_request<T>(method: &str, params: Option<T>) -> anyhow::Result<JsonRpcRequest>
where
    T: serde::Serialize,
{
    Ok(JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        params: params.map(serde_json::to_value).transpose()?,
        id: Some(serde_json::json!(1)),
    })
}

fn canonicalize_rpc_path(path: &Path) -> anyhow::Result<String> {
    Ok(path.canonicalize()?.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::{build_bridge_request, build_inline_request, build_scratch_request};
    use crate::rpc::{
        CompleteParams, GenerateParams, InlineParams, PrefetchParams, ScratchCreateParams,
    };
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_file_path(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "harnessd_commands_test_{}_{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&base).expect("failed to create temp dir");
        let path = base.join(name);
        std::fs::write(&path, "fn demo() {}\n").expect("failed to write temp file");
        path
    }

    #[test]
    fn bridge_builds_complete_request() {
        let file = temp_file_path("fixture.rs");
        let request = build_bridge_request("complete", Some(&file), None, None, Some("7"))
            .expect("failed to build complete request");

        assert_eq!(request.method, "complete");
        let params: CompleteParams =
            serde_json::from_value(request.params.expect("missing params")).expect("bad params");
        assert_eq!(
            params.file,
            file.canonicalize()
                .expect("canonicalize failed")
                .to_string_lossy()
                .to_string()
        );
        assert_eq!(params.offset, 7);
        assert_eq!(params.prefix, None);

        std::fs::remove_file(&file).ok();
        std::fs::remove_dir_all(file.parent().expect("missing parent")).ok();
    }

    #[test]
    fn bridge_builds_prefetch_request() {
        let file = temp_file_path("fixture.rs");
        let request = build_bridge_request("prefetch", Some(&file), None, None, None)
            .expect("failed to build prefetch request");

        assert_eq!(request.method, "prefetch");
        let params: PrefetchParams =
            serde_json::from_value(request.params.expect("missing params")).expect("bad params");
        assert_eq!(
            params.path,
            file.canonicalize()
                .expect("canonicalize failed")
                .to_string_lossy()
                .to_string()
        );

        std::fs::remove_file(&file).ok();
        std::fs::remove_dir_all(file.parent().expect("missing parent")).ok();
    }

    #[test]
    fn bridge_builds_generate_request() {
        let file = temp_file_path("fixture.rs");
        let request = build_bridge_request("generate", Some(&file), None, None, Some("7"))
            .expect("failed to build generate request");

        assert_eq!(request.method, "generate");
        let params: GenerateParams =
            serde_json::from_value(request.params.expect("missing params")).expect("bad params");
        assert_eq!(params.offset, 7);

        std::fs::remove_file(&file).ok();
        std::fs::remove_dir_all(file.parent().expect("missing parent")).ok();
    }

    #[test]
    fn bridge_complete_requires_cursor() {
        let file = temp_file_path("fixture.rs");
        let error = build_bridge_request("complete", Some(&file), None, None, None)
            .expect_err("expected missing cursor to fail");

        assert!(
            error
                .to_string()
                .contains("`--cursor` is required for `bridge --method complete`")
        );

        std::fs::remove_file(&file).ok();
        std::fs::remove_dir_all(file.parent().expect("missing parent")).ok();
    }

    #[test]
    fn inline_builds_request_from_live_buffer_content() {
        let file = temp_file_path("fixture.rs");
        let request = build_inline_request(&file, 7, "insert a value", "fn unsaved() {}".into())
            .expect("failed to build inline request");
        assert_eq!(request.method, "inline");
        let params: InlineParams =
            serde_json::from_value(request.params.expect("missing params")).expect("bad params");
        assert_eq!(params.offset, 7);
        assert_eq!(params.prompt, "insert a value");
        assert_eq!(params.content, "fn unsaved() {}");

        std::fs::remove_file(&file).ok();
        std::fs::remove_dir_all(file.parent().expect("missing parent")).ok();
    }

    #[test]
    fn inline_rejects_empty_content_and_prompt() {
        let file = temp_file_path("fixture.rs");
        assert!(build_inline_request(&file, 0, "ask", String::new()).is_err());
        assert!(build_inline_request(&file, 0, " ", "fn x() {}".into()).is_err());

        std::fs::remove_file(&file).ok();
        std::fs::remove_dir_all(file.parent().expect("missing parent")).ok();
    }

    #[test]
    fn scratch_builds_request_from_live_buffer_content() {
        let file = temp_file_path("fixture.rs");
        let workspace = file.parent().unwrap().to_path_buf();
        let request = build_scratch_request(
            &workspace,
            &file,
            7,
            "sketch usage",
            "fn unsaved() {}".into(),
            Some(1),
            Some(5),
        )
        .expect("failed to build scratch request");
        assert_eq!(request.method, "scratch.create");
        let params: ScratchCreateParams =
            serde_json::from_value(request.params.expect("missing params")).expect("bad params");
        assert_eq!(params.offset, 7);
        assert_eq!(params.prompt, "sketch usage");
        assert_eq!(params.content, "fn unsaved() {}");
        assert_eq!(params.selection_start, Some(1));
        assert_eq!(params.selection_end, Some(5));

        std::fs::remove_file(&file).ok();
        std::fs::remove_dir_all(file.parent().expect("missing parent")).ok();
    }

    #[test]
    fn scratch_rejects_empty_content_and_prompt() {
        let file = temp_file_path("fixture.rs");
        let workspace = file.parent().unwrap().to_path_buf();
        assert!(
            build_scratch_request(&workspace, &file, 0, "ask", String::new(), None, None).is_err()
        );
        assert!(
            build_scratch_request(&workspace, &file, 0, " ", "fn x() {}".into(), None, None)
                .is_err()
        );

        std::fs::remove_file(&file).ok();
        std::fs::remove_dir_all(file.parent().expect("missing parent")).ok();
    }
}
