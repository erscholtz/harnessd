//! Integration tests for end-to-end prefetch and complete behavior.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use harnessd::ipc;
use harnessd::rpc::{CompleteParams, PrefetchParams};
use harnessd::state::DaemonState;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_runtime_dir() -> PathBuf {
    let temp_dir = std::env::temp_dir();
    let unique_name = format!(
        "harnessd_ipc_test_{}_{}",
        std::process::id(),
        TEST_COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    temp_dir.join(unique_name)
}

#[tokio::test]
async fn prefetch_populates_cache_for_complete() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).expect("failed to create runtime dir");

    let file_path = runtime_dir.join("fixture.rs");
    let source = r#"fn demo() {
    // TODO: implement demo
    let value = 42;
}
"#;
    std::fs::write(&file_path, source).expect("failed to write fixture");

    let state = DaemonState::new(runtime_dir.clone()).expect("failed to create daemon state");
    let prefetch = ipc::prefetch(
        &state,
        &PrefetchParams {
            path: file_path.to_string_lossy().to_string(),
        },
    )
    .await
    .expect("prefetch failed");

    assert_eq!(prefetch.scanned_files, 1);
    assert_eq!(prefetch.anchors_found, 1);
    assert_eq!(prefetch.proposals_stored, 1);

    let offset = source.find("value").expect("missing value offset");
    let suggestions = ipc::complete(
        &state,
        &CompleteParams {
            file: file_path.to_string_lossy().to_string(),
            offset,
            prefix: None,
        },
    )
    .await
    .expect("complete failed");

    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].label, "Implement TODO");
    assert!(suggestions[0].insert_text.contains("todo!"));

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[tokio::test]
async fn prefetch_populates_cache_for_python_complete() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).expect("failed to create runtime dir");

    let file_path = runtime_dir.join("fixture.py");
    let source = r#"def demo():
    # TODO: implement demo
    value = 42
"#;
    std::fs::write(&file_path, source).expect("failed to write fixture");

    let state = DaemonState::new(runtime_dir.clone()).expect("failed to create daemon state");
    let prefetch = ipc::prefetch(
        &state,
        &PrefetchParams {
            path: file_path.to_string_lossy().to_string(),
        },
    )
    .await
    .expect("prefetch failed");

    assert_eq!(prefetch.scanned_files, 1);
    assert_eq!(prefetch.anchors_found, 1);
    assert_eq!(prefetch.proposals_stored, 1);

    let offset = source.find("value").expect("missing value offset");
    let suggestions = ipc::complete(
        &state,
        &CompleteParams {
            file: file_path.to_string_lossy().to_string(),
            offset,
            prefix: None,
        },
    )
    .await
    .expect("complete failed");

    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].label, "Implement TODO");
    assert!(suggestions[0].insert_text.contains("NotImplementedError"));

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[tokio::test]
async fn status_reports_cache_and_request_metrics() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).expect("failed to create runtime dir");

    let file_path = runtime_dir.join("fixture.rs");
    let source = r#"fn demo() {
    // TODO: implement demo
}
"#;
    std::fs::write(&file_path, source).expect("failed to write fixture");

    let state = DaemonState::new(runtime_dir.clone()).expect("failed to create daemon state");

    ipc::prefetch(
        &state,
        &PrefetchParams {
            path: file_path.to_string_lossy().to_string(),
        },
    )
    .await
    .expect("prefetch failed");
    state.record_request("prefetch");

    let status = ipc::status(&state).await.expect("status failed");

    assert_eq!(status.pid, std::process::id());
    assert_eq!(status.cache.total_proposals, 1);
    assert_eq!(status.metrics.prefetch_requests, 1);
    assert!(!status.runtime.stale_lock);
    assert!(status.runtime.warnings.is_empty());
    assert!(!status.recent_proposals.is_empty());
    assert_eq!(status.recent_proposals[0].label, "Implement TODO");

    std::fs::remove_dir_all(&runtime_dir).ok();
}
