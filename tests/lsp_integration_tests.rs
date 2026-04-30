//! Integration tests for the LSP adapter completion path.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use harnessd::lsp::LspSession;
use harnessd::state::DaemonState;
use serde_json::{Value, json};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_runtime_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "harnessd_lsp_test_{}_{}",
        std::process::id(),
        TEST_COUNTER.fetch_add(1, Ordering::SeqCst)
    ))
}

fn file_uri(path: &std::path::Path) -> String {
    let path = path
        .canonicalize()
        .expect("failed to canonicalize path")
        .to_string_lossy()
        .replace('\\', "/");

    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    }
}

#[tokio::test]
async fn lsp_completion_returns_cached_harnessd_items() {
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
    let mut session = LspSession::with_daemon_state(state);
    let uri = file_uri(&file_path);

    session
        .handle_json_message(json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "rust",
                    "version": 1,
                    "text": source
                }
            }
        }))
        .await;

    let response = session
        .handle_json_message(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "textDocument/completion",
            "params": {
                "textDocument": {
                    "uri": uri
                },
                "position": {
                    "line": 2,
                    "character": 9
                }
            }
        }))
        .await
        .expect("completion should produce a response");

    assert!(response.get("error").is_none(), "{response:?}");
    let items = response["result"]
        .as_array()
        .expect("completion result should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]["label"],
        Value::String("Implement TODO".to_string())
    );
    assert!(
        items[0]["insertText"]
            .as_str()
            .expect("insertText should be a string")
            .contains("todo!")
    );

    std::fs::remove_dir_all(&runtime_dir).ok();
}
