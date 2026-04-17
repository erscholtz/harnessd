//! Shared daemon state: cache, parser, and runtime configuration.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use tokio::sync::RwLock;

use crate::cache::ProposalCache;
use crate::parser::LanguageParsers;
use crate::rpc::{CacheStatus, DaemonMetricsSnapshot, RecentProposal, StatusResult};
use crate::runtime;

/// Shared state for the daemon, accessible from RPC handlers.
pub struct DaemonState {
    /// The proposal cache database.
    pub cache: ProposalCache,
    /// Tree-sitter parser registry for supported languages.
    pub parser: RwLock<LanguageParsers>,
    /// Runtime directory for sockets, etc.
    pub runtime_dir: PathBuf,
    cache_db_path: PathBuf,
    started_at: Instant,
    started_at_unix: u64,
    metrics: DaemonMetrics,
}

impl DaemonState {
    /// Create a new daemon state.
    pub fn new(runtime_dir: PathBuf) -> anyhow::Result<Arc<Self>> {
        let cache_path = runtime_dir.join("proposals.db");
        let cache = ProposalCache::open(&cache_path)?;
        let parser = LanguageParsers::new()?;

        tracing::info!(
            db_path = %cache_path.display(),
            "proposal cache opened"
        );

        Ok(Arc::new(Self {
            cache,
            parser: RwLock::new(parser),
            runtime_dir,
            cache_db_path: cache_path,
            started_at: Instant::now(),
            started_at_unix: unix_timestamp(),
            metrics: DaemonMetrics::default(),
        }))
    }

    /// Record an incoming JSON-RPC method call for diagnostics.
    pub fn record_request(&self, method: &str) {
        self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
        match method {
            "complete" => {
                self.metrics
                    .complete_requests
                    .fetch_add(1, Ordering::Relaxed);
            }
            "prefetch" => {
                self.metrics
                    .prefetch_requests
                    .fetch_add(1, Ordering::Relaxed);
            }
            "status" => {
                self.metrics.status_requests.fetch_add(1, Ordering::Relaxed);
            }
            "shutdown" => {
                self.metrics
                    .shutdown_requests
                    .fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
        self.metrics
            .last_request_at
            .store(unix_timestamp(), Ordering::Relaxed);
    }

    /// Build a dashboard-friendly snapshot of daemon state.
    pub async fn status_snapshot(&self) -> anyhow::Result<StatusResult> {
        let cache_stats = self.cache.stats().await?;
        let recent_proposals = self
            .cache
            .recent(5)
            .await?
            .into_iter()
            .map(|proposal| RecentProposal {
                label: proposal.label,
                file_path: proposal.file_path,
                byte_start: proposal.byte_start,
                byte_end: proposal.byte_end,
                created_at: proposal.created_at,
                snippet_bytes: proposal.snippet.len(),
                snippet_preview: summarize_snippet(&proposal.snippet),
            })
            .collect();
        let db_file_size_bytes = std::fs::metadata(&self.cache_db_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);

        Ok(StatusResult {
            pid: std::process::id(),
            runtime_dir: self.runtime_dir.display().to_string(),
            ipc_endpoint: ipc_endpoint(),
            cache_db_path: self.cache_db_path.display().to_string(),
            started_at: self.started_at_unix,
            uptime_secs: self.started_at.elapsed().as_secs(),
            metrics: DaemonMetricsSnapshot {
                total_requests: self.metrics.total_requests.load(Ordering::Relaxed),
                complete_requests: self.metrics.complete_requests.load(Ordering::Relaxed),
                prefetch_requests: self.metrics.prefetch_requests.load(Ordering::Relaxed),
                status_requests: self.metrics.status_requests.load(Ordering::Relaxed),
                shutdown_requests: self.metrics.shutdown_requests.load(Ordering::Relaxed),
                last_request_at: match self.metrics.last_request_at.load(Ordering::Relaxed) {
                    0 => None,
                    timestamp => Some(timestamp),
                },
            },
            cache: CacheStatus {
                total_proposals: cache_stats.total_proposals,
                total_bytes: cache_stats.total_bytes,
                db_file_size_bytes,
                oldest_timestamp: cache_stats.oldest_timestamp,
                newest_timestamp: cache_stats.newest_timestamp,
                max_lines: crate::cache::MAX_LINES,
                max_bytes: crate::cache::MAX_BYTES,
            },
            runtime: runtime::inspect(&self.runtime_dir, true),
            recent_proposals,
        })
    }
}

#[derive(Default)]
struct DaemonMetrics {
    total_requests: AtomicU64,
    complete_requests: AtomicU64,
    prefetch_requests: AtomicU64,
    status_requests: AtomicU64,
    shutdown_requests: AtomicU64,
    last_request_at: AtomicU64,
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn summarize_snippet(snippet: &str) -> String {
    let mut preview: String = snippet
        .chars()
        .map(|ch| {
            if matches!(ch, '\r' | '\n' | '\t') {
                ' '
            } else {
                ch
            }
        })
        .collect();
    preview.truncate(96);
    preview
}

fn ipc_endpoint() -> String {
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
