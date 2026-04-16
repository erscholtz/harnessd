//! Wait for a graceful shutdown signal (no SIGKILL; daemon exits normally).

/// Blocks until the process should shut down: **Ctrl+C** everywhere; **SIGTERM**
/// on Unix (`kill <pid>`). On Windows, use **`harnessd stop`** or Task Manager
/// / `taskkill` (graceful flags) — `taskkill` without `/F` asks nicely first.
pub async fn wait_for_shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "SIGTERM not available; use Ctrl+C or `harnessd stop`");
                tokio::signal::ctrl_c().await.ok();
                return;
            }
        };

        tokio::select! {
            _ = sigterm.recv() => {
                tracing::info!("received SIGTERM");
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("received Ctrl+C (SIGINT)");
            }
        }
    }

    #[cfg(not(unix))]
    {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!(error = %e, "Ctrl+C handler failed");
            return;
        }
        tracing::info!("received Ctrl+C");
    }
}
