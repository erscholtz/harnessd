//! Integration tests for the RPC module.

use harnessd::rpc::{
    CacheStatus, CompleteParams, DaemonMetricsSnapshot, JsonRpcRequest, JsonRpcResponse,
    PrefetchParams, RecentProposal, StatusResult,
};

#[test]
fn test_json_rpc_request_deserialization() {
    let json = r#"{
        "jsonrpc": "2.0",
        "method": "complete",
        "params": {
            "file": "/test/file.rs",
            "offset": 100
        },
        "id": 1
    }"#;

    let request: JsonRpcRequest = serde_json::from_str(json).expect("failed to deserialize");
    assert_eq!(request.jsonrpc, "2.0");
    assert_eq!(request.method, "complete");
    assert!(request.params.is_some());
    assert_eq!(request.id, Some(serde_json::json!(1)));
}

#[test]
fn test_json_rpc_request_without_params() {
    let json = r#"{
        "jsonrpc": "2.0",
        "method": "prefetch",
        "id": "abc"
    }"#;

    let request: JsonRpcRequest = serde_json::from_str(json).expect("failed to deserialize");
    assert_eq!(request.method, "prefetch");
    assert!(request.params.is_none());
    assert_eq!(request.id, Some(serde_json::json!("abc")));
}

#[test]
fn test_json_rpc_request_null_id() {
    let json = r#"{
        "jsonrpc": "2.0",
        "method": "complete",
        "id": null
    }"#;

    let request: JsonRpcRequest = serde_json::from_str(json).expect("failed to deserialize");
    // When id is null, it's deserialized as None
    assert_eq!(request.id, None);
}

#[test]
fn test_complete_params_deserialization() {
    let json = r#"{
        "file": "/test/file.rs",
        "offset": 150,
        "prefix": "test"
    }"#;

    let params: CompleteParams = serde_json::from_str(json).expect("failed to deserialize");
    assert_eq!(params.file, "/test/file.rs");
    assert_eq!(params.offset, 150);
    assert_eq!(params.prefix, Some("test".to_string()));
}

#[test]
fn test_complete_params_without_prefix() {
    let json = r#"{
        "file": "/test/file.rs",
        "offset": 100
    }"#;

    let params: CompleteParams = serde_json::from_str(json).expect("failed to deserialize");
    assert_eq!(params.file, "/test/file.rs");
    assert_eq!(params.offset, 100);
    assert_eq!(params.prefix, None);
}

#[test]
fn test_prefetch_params_deserialization() {
    let json = r#"{
        "path": "/workspace/project"
    }"#;

    let params: PrefetchParams = serde_json::from_str(json).expect("failed to deserialize");
    assert_eq!(params.path, "/workspace/project");
}

#[test]
fn test_json_rpc_response_success() {
    let response = JsonRpcResponse::success(
        Some(serde_json::json!(1)),
        serde_json::json!({"suggestions": []}),
    );

    assert_eq!(response.jsonrpc, "2.0");
    assert!(response.result.is_some());
    assert!(response.error.is_none());
    assert_eq!(response.id, Some(serde_json::json!(1)));

    let json = response.to_string();
    assert!(json.contains("\"jsonrpc\":\"2.0\""));
    assert!(json.contains("\"result\""));
    assert!(json.contains("\"suggestions\""));
}

#[test]
fn test_json_rpc_response_error() {
    let response = JsonRpcResponse::error(
        Some(serde_json::json!(1)),
        -32601,
        "Method not found".to_string(),
        None,
    );

    assert_eq!(response.jsonrpc, "2.0");
    assert!(response.result.is_none());
    assert!(response.error.is_some());

    let error = response.error.as_ref().unwrap();
    assert_eq!(error.code, -32601);
    assert_eq!(error.message, "Method not found");
    assert!(error.data.is_none());

    let json = response.to_string();
    assert!(json.contains("\"error\""));
    assert!(json.contains("-32601"));
    assert!(json.contains("Method not found"));
}

#[test]
fn test_json_rpc_response_with_data() {
    let response = JsonRpcResponse::error(
        Some(serde_json::json!(1)),
        -32000,
        "Server error".to_string(),
        Some(serde_json::json!({"details": "additional info"})),
    );

    let error = response.error.as_ref().unwrap();
    assert!(error.data.is_some());
    assert_eq!(error.data.as_ref().unwrap()["details"], "additional info");
}

#[test]
fn test_json_rpc_response_null_id() {
    let response = JsonRpcResponse::success(None, serde_json::json!("test"));

    assert_eq!(response.id, None);

    let json = response.to_string();
    assert!(json.contains("\"id\":null") || !json.contains("\"id\""));
}

#[test]
fn test_json_rpc_response_serialization_roundtrip() {
    let original = JsonRpcResponse::success(
        Some(serde_json::json!(42)),
        serde_json::json!({
            "suggestions": [
                {"label": "test", "insert_text": "test()"}
            ]
        }),
    );

    let json = original.to_string();
    let deserialized: JsonRpcResponse = serde_json::from_str(&json).expect("failed to deserialize");

    assert_eq!(deserialized.jsonrpc, original.jsonrpc);
    assert_eq!(deserialized.id, original.id);
    assert!(deserialized.result.is_some());
}

#[test]
fn test_complete_params_serialization() {
    let params = CompleteParams {
        file: "/test/file.rs".to_string(),
        offset: 100,
        prefix: Some("func".to_string()),
    };

    let json = serde_json::to_string(&params).expect("failed to serialize");
    assert!(json.contains("/test/file.rs"));
    assert!(json.contains("100"));
    assert!(json.contains("func"));
}

#[test]
fn test_prefetch_params_serialization() {
    let params = PrefetchParams {
        path: "/workspace".to_string(),
    };

    let json = serde_json::to_string(&params).expect("failed to serialize");
    assert!(json.contains("/workspace"));
}

#[test]
fn test_status_result_serialization() {
    let status = StatusResult {
        pid: 42,
        runtime_dir: "/tmp/harnessd".to_string(),
        ipc_endpoint: "/tmp/harnessd/daemon.sock".to_string(),
        cache_db_path: "/tmp/harnessd/proposals.db".to_string(),
        started_at: 1,
        uptime_secs: 2,
        metrics: DaemonMetricsSnapshot {
            total_requests: 3,
            complete_requests: 1,
            prefetch_requests: 1,
            status_requests: 1,
            shutdown_requests: 0,
            last_request_at: Some(4),
        },
        cache: CacheStatus {
            total_proposals: 5,
            total_bytes: 128,
            db_file_size_bytes: 4096,
            oldest_timestamp: Some(10),
            newest_timestamp: Some(20),
            max_lines: 20,
            max_bytes: 2048,
        },
        runtime: harnessd::runtime::RuntimeHealth {
            runtime_dir: "/tmp/harnessd".to_string(),
            lock_path: "/tmp/harnessd/daemon.lock".to_string(),
            lock_exists: true,
            lock_pid: Some(42),
            stale_lock: false,
            port_file_path: "/tmp/harnessd/daemon.port".to_string(),
            port_file_exists: false,
            stale_port_file: false,
            endpoint_reachable: true,
            warnings: vec![],
        },
        recent_proposals: vec![RecentProposal {
            label: "Implement TODO".to_string(),
            file_path: "/tmp/harnessd/src/main.rs".to_string(),
            byte_start: 10,
            byte_end: 20,
            created_at: 30,
            snippet_preview: "todo!(\"demo\");".to_string(),
            snippet_bytes: 15,
        }],
    };

    let json = serde_json::to_string(&status).expect("failed to serialize status");
    assert!(json.contains("\"pid\":42"));
    assert!(json.contains("\"total_proposals\":5"));
    assert!(json.contains("\"runtime\""));
    assert!(json.contains("\"recent_proposals\""));
}
