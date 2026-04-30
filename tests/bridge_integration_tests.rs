//! Integration tests for the bridge-facing JSON-RPC endpoints over IPC.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use harnessd::ipc;
use harnessd::rpc::{CompleteParams, JsonRpcRequest, JsonRpcResponse, PrefetchParams};
use harnessd::state::DaemonState;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_runtime_dir() -> PathBuf {
    let temp_dir = std::env::temp_dir();
    let unique_name = format!(
        "harnessd_bridge_test_{}_{}",
        std::process::id(),
        TEST_COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    temp_dir.join(unique_name)
}

async fn wait_for_endpoint(runtime_dir: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        #[cfg(unix)]
        if runtime_dir.join("daemon.sock").exists() {
            return;
        }

        #[cfg(windows)]
        if tokio::fs::read_to_string(runtime_dir.join("daemon.port"))
            .await
            .map(|port| !port.trim().is_empty())
            .unwrap_or(false)
        {
            return;
        }

        assert!(Instant::now() < deadline, "daemon endpoint was not created");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn send_request(runtime_dir: &Path, request: &JsonRpcRequest) -> JsonRpcResponse {
    let payload = serde_json::to_string(request).expect("failed to serialize request");

    #[cfg(unix)]
    let stream = tokio::net::UnixStream::connect(runtime_dir.join("daemon.sock"))
        .await
        .expect("failed to connect to daemon socket");

    #[cfg(windows)]
    let stream = {
        let port = tokio::fs::read_to_string(runtime_dir.join("daemon.port"))
            .await
            .expect("failed to read daemon port");
        tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port.trim()))
            .await
            .expect("failed to connect to daemon tcp endpoint")
    };

    let (reader, mut writer) = tokio::io::split(stream);
    writer
        .write_all(payload.as_bytes())
        .await
        .expect("failed to write request");
    writer
        .write_all(b"\n")
        .await
        .expect("failed to write newline");
    writer.flush().await.expect("failed to flush request");

    let mut response = String::new();
    let mut reader = BufReader::new(reader);
    reader
        .read_line(&mut response)
        .await
        .expect("failed to read response");

    serde_json::from_str(response.trim()).expect("failed to parse response")
}

#[tokio::test]
async fn ipc_serves_prefetch_and_complete_for_bridge_clients() {
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
    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
    let server = tokio::spawn(ipc::serve(state, shutdown_tx.clone(), shutdown_rx));

    wait_for_endpoint(&runtime_dir).await;

    let canonical_file = file_path
        .canonicalize()
        .expect("failed to canonicalize file")
        .to_string_lossy()
        .to_string();

    let prefetch_response = send_request(
        &runtime_dir,
        &JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "prefetch".to_string(),
            params: Some(
                serde_json::to_value(PrefetchParams {
                    path: canonical_file.clone(),
                })
                .expect("failed to serialize prefetch params"),
            ),
            id: Some(serde_json::json!(1)),
        },
    )
    .await;

    assert!(prefetch_response.error.is_none(), "{prefetch_response:?}");
    let prefetch_result = prefetch_response.result.expect("missing prefetch result");
    assert_eq!(prefetch_result["scanned_files"], 1);
    assert_eq!(prefetch_result["anchors_found"], 1);
    assert_eq!(prefetch_result["proposals_stored"], 1);

    let offset = source.find("value").expect("missing value offset");
    let complete_response = send_request(
        &runtime_dir,
        &JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "complete".to_string(),
            params: Some(
                serde_json::to_value(CompleteParams {
                    file: canonical_file,
                    offset,
                    prefix: None,
                })
                .expect("failed to serialize complete params"),
            ),
            id: Some(serde_json::json!(2)),
        },
    )
    .await;

    assert!(complete_response.error.is_none(), "{complete_response:?}");
    let suggestions = complete_response.result.expect("missing complete result")["suggestions"]
        .as_array()
        .expect("suggestions should be an array")
        .clone();
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0]["label"], "Implement TODO");
    assert!(
        suggestions[0]["insert_text"]
            .as_str()
            .expect("insert_text should be a string")
            .contains("todo!")
    );

    shutdown_tx
        .send(())
        .await
        .expect("failed to request shutdown");
    server
        .await
        .expect("server task panicked")
        .expect("server returned an error");

    std::fs::remove_dir_all(&runtime_dir).ok();
}
