//! Snapshot collection for the terminal dashboard.

use std::path::{Path, PathBuf};

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::cache::ProposalCache;
use crate::rpc::{CacheStatus, JsonRpcRequest, JsonRpcResponse, RuntimeHealth, StatusResult};

/// Combined local and remote status data rendered by the TUI.
#[derive(Debug, Clone)]
pub struct DashboardSnapshot {
    /// Time the snapshot was collected, as Unix seconds.
    pub collected_at: u64,
    /// Runtime directory path.
    pub runtime_dir: PathBuf,
    /// Lock file path.
    pub lock_path: PathBuf,
    /// Whether the daemon lock file exists.
    pub lock_exists: bool,
    /// PID parsed from the lock file, if any.
    pub daemon_pid: Option<u32>,
    /// IPC endpoint path or address.
    pub ipc_endpoint: String,
    /// Whether the IPC endpoint accepted a connection.
    pub ipc_ready: bool,
    /// Local cache database path.
    pub cache_db_path: PathBuf,
    /// Whether the cache database exists.
    pub cache_db_exists: bool,
    /// Local cache database file size in bytes.
    pub cache_db_size_bytes: u64,
    /// Cache statistics obtained locally if the database exists.
    pub local_cache: Option<CacheStatus>,
    /// Daemon-reported status if the daemon answered an RPC request.
    pub remote_status: Option<StatusResult>,
    /// Most recent collection error, typically an RPC/connect issue.
    pub error: Option<String>,
    /// Local runtime health for stale-state reporting.
    pub runtime_health: RuntimeHealth,
}

/// Gather a fresh dashboard snapshot.
pub async fn collect() -> DashboardSnapshot {
    let runtime_dir = crate::paths::runtime_dir();
    let lock_path = runtime_dir.join("daemon.lock");
    let cache_db_path = runtime_dir.join("proposals.db");
    let lock_exists = lock_path.exists();
    let daemon_pid = read_pid(&lock_path);
    let (cache_db_exists, cache_db_size_bytes) = match std::fs::metadata(&cache_db_path) {
        Ok(metadata) => (true, metadata.len()),
        Err(_) => (false, 0),
    };

    let local_cache = if cache_db_exists {
        load_local_cache_status(&cache_db_path, cache_db_size_bytes)
            .await
            .ok()
    } else {
        None
    };

    let (remote_status, error) = match request_remote_status().await {
        Ok(status) => (Some(status), None),
        Err(error) => (None, Some(error.to_string())),
    };
    let ipc_endpoint = remote_status
        .as_ref()
        .map(|status| status.ipc_endpoint.clone())
        .unwrap_or_else(local_ipc_endpoint);
    let ipc_ready = remote_status.is_some() || can_connect().await;
    let runtime_health = remote_status
        .as_ref()
        .map(|status| status.runtime.clone())
        .unwrap_or_else(|| crate::runtime::inspect(&runtime_dir, ipc_ready));

    DashboardSnapshot {
        collected_at: unix_timestamp(),
        runtime_dir,
        lock_path,
        lock_exists,
        daemon_pid,
        ipc_endpoint,
        ipc_ready,
        cache_db_path,
        cache_db_exists,
        cache_db_size_bytes,
        local_cache,
        remote_status,
        error,
        runtime_health,
    }
}

async fn load_local_cache_status(
    cache_db_path: &Path,
    db_file_size_bytes: u64,
) -> anyhow::Result<CacheStatus> {
    let cache = ProposalCache::open(cache_db_path)?;
    let stats = cache.stats().await?;
    Ok(CacheStatus {
        total_proposals: stats.total_proposals,
        total_bytes: stats.total_bytes,
        db_file_size_bytes,
        oldest_timestamp: stats.oldest_timestamp,
        newest_timestamp: stats.newest_timestamp,
        max_lines: crate::cache::MAX_LINES,
        max_bytes: crate::cache::MAX_BYTES,
    })
}

async fn request_remote_status() -> anyhow::Result<StatusResult> {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "status".to_string(),
        params: None,
        id: Some(serde_json::json!(1)),
    };
    let payload = serde_json::to_string(&request)?;
    let response = send_rpc_request_once(&payload).await?;
    let parsed: JsonRpcResponse = serde_json::from_str(&response)
        .with_context(|| format!("invalid status response: {response}"))?;

    if let Some(error) = parsed.error {
        anyhow::bail!("daemon returned {} ({})", error.message, error.code);
    }

    let result = parsed
        .result
        .context("status response was missing `result`")?;
    Ok(serde_json::from_value(result)?)
}

async fn send_rpc_request_once(payload: &str) -> anyhow::Result<String> {
    #[cfg(unix)]
    {
        let stream = tokio::net::UnixStream::connect(crate::paths::socket_path())
            .await
            .context("failed to connect to daemon socket")?;
        send_payload(stream, payload).await
    }

    #[cfg(windows)]
    {
        let port = tokio::fs::read_to_string(crate::paths::port_file())
            .await
            .context("failed to read daemon port file")?;
        let addr = format!("127.0.0.1:{}", port.trim());
        let stream = tokio::net::TcpStream::connect(&addr)
            .await
            .with_context(|| format!("failed to connect to daemon at {addr}"))?;
        send_payload(stream, payload).await
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

async fn can_connect() -> bool {
    #[cfg(unix)]
    {
        tokio::net::UnixStream::connect(crate::paths::socket_path())
            .await
            .is_ok()
    }

    #[cfg(windows)]
    {
        match tokio::fs::read_to_string(crate::paths::port_file()).await {
            Ok(port) => {
                let addr = format!("127.0.0.1:{}", port.trim());
                tokio::net::TcpStream::connect(addr).await.is_ok()
            }
            Err(_) => false,
        }
    }
}

fn read_pid(lock_path: &Path) -> Option<u32> {
    let content = std::fs::read_to_string(lock_path).ok()?;
    content.trim().parse::<u32>().ok()
}

fn local_ipc_endpoint() -> String {
    #[cfg(unix)]
    {
        crate::paths::socket_path().display().to_string()
    }

    #[cfg(windows)]
    {
        match std::fs::read_to_string(crate::paths::port_file()) {
            Ok(port) => format!("127.0.0.1:{}", port.trim()),
            Err(_) => crate::paths::port_file().display().to_string(),
        }
    }
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
