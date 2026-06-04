//! JSON-RPC 2.0 types for the daemon protocol.

use std::fmt;

use serde::{Deserialize, Serialize};

pub use crate::codex_sessions::{CodexSessionInfo, CodexSessionsParams, CodexSessionsResult};
pub use crate::threads::{
    ThreadAnchor, ThreadAttachParams, ThreadCreateParams, ThreadCreateResult, ThreadLaunch,
    ThreadLinkParams, ThreadLinkResult, ThreadListParams, ThreadListResult, ThreadResolveParams,
    ThreadResolveResult,
};

/// A JSON-RPC 2.0 request.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcRequest {
    /// Protocol version; always `2.0`.
    pub jsonrpc: String,
    /// Method name.
    pub method: String,
    /// Optional method parameters.
    #[serde(default)]
    pub params: Option<serde_json::Value>,
    /// Request id preserved in the response.
    pub id: Option<serde_json::Value>,
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcResponse {
    /// Protocol version; always `2.0`.
    pub jsonrpc: String,
    /// Successful result payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error payload for failed requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    /// Request id copied from the request.
    pub id: Option<serde_json::Value>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcError {
    /// JSON-RPC error code.
    pub code: i32,
    /// Human-readable error message.
    pub message: String,
    /// Optional structured error details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    /// Create a successful response.
    pub fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Create an error response.
    pub fn error(
        id: Option<serde_json::Value>,
        code: i32,
        message: String,
        data: Option<serde_json::Value>,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data,
            }),
            id,
        }
    }
}

impl fmt::Display for JsonRpcResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"Internal error"},"id":null}"#
                .to_string()
        });
        f.write_str(&value)
    }
}

/// Parameters for the `complete` method.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompleteParams {
    /// Absolute path to the file.
    pub file: String,
    /// Cursor position as byte offset.
    pub offset: usize,
    /// Optional prefix to filter suggestions.
    #[serde(default)]
    pub prefix: Option<String>,
}

/// Parameters for the `anchors` method.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnchorsParams {
    /// Absolute path to one saved source file.
    pub file: String,
}

/// Parameters for the `generate` method.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GenerateParams {
    /// Absolute path to one saved source file.
    pub file: String,
    /// Cursor position as a byte offset within an anchor marker.
    pub offset: usize,
}

/// Parameters for an ephemeral freeform inline generation request.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InlineParams {
    /// Absolute source file path used for language and workspace selection.
    pub file: String,
    /// Cursor position as a byte offset into `content`.
    pub offset: usize,
    /// Current editor buffer contents, including unsaved edits.
    pub content: String,
    /// User instruction for text to insert at the cursor.
    pub prompt: String,
}

/// An anchor reported for an editor buffer.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnchorInfo {
    /// First byte of the TODO or empty-function marker.
    pub anchor_start: usize,
    /// End byte of the TODO or empty-function marker.
    pub anchor_end: usize,
    /// First byte of the proposal cache region.
    pub region_start: usize,
    /// End byte of the proposal cache region.
    pub region_end: usize,
    /// Machine-readable anchor kind.
    pub kind: String,
    /// Label shown to an editor user.
    pub label: String,
    /// Current cache/generation state: `candidate`, `ready`, or `failed`.
    pub status: String,
}

/// A completion suggestion.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompletionSuggestion {
    /// Label shown in the UI.
    pub label: String,
    /// Text to insert.
    pub insert_text: String,
    /// Optional detail/description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Optional documentation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
}

/// Parameters for the `prefetch` method.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PrefetchParams {
    /// File or directory to prefetch.
    pub path: String,
}

/// Result metadata for a `prefetch` request.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PrefetchResult {
    /// Number of files scanned for anchors.
    pub scanned_files: usize,
    /// Number of anchors found across scanned files.
    pub anchors_found: usize,
    /// Number of proposals successfully stored.
    pub proposals_stored: usize,
}

/// Result returned by the `status` method.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StatusResult {
    /// Daemon process id.
    pub pid: u32,
    /// Runtime directory path.
    pub runtime_dir: String,
    /// IPC endpoint path or address.
    pub ipc_endpoint: String,
    /// Cache database path.
    pub cache_db_path: String,
    /// Process start time as Unix seconds.
    pub started_at: u64,
    /// Process uptime in seconds.
    pub uptime_secs: u64,
    /// Request counters and timestamps.
    pub metrics: DaemonMetricsSnapshot,
    /// Cache statistics.
    pub cache: CacheStatus,
    /// Runtime lifecycle health.
    pub runtime: RuntimeHealth,
    /// Most recent cached proposals.
    pub recent_proposals: Vec<RecentProposal>,
}

/// Runtime lifecycle health reported through status and doctor.
pub type RuntimeHealth = crate::runtime::RuntimeHealth;

/// Request counter snapshot for the daemon.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DaemonMetricsSnapshot {
    /// Total number of JSON-RPC requests processed.
    pub total_requests: u64,
    /// Number of `complete` requests processed.
    pub complete_requests: u64,
    /// Number of `prefetch` requests processed.
    pub prefetch_requests: u64,
    /// Number of `status` requests processed.
    pub status_requests: u64,
    /// Number of `shutdown` requests processed.
    pub shutdown_requests: u64,
    /// Last processed request time as Unix seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_request_at: Option<u64>,
}

/// Cache statistics rendered in status responses.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CacheStatus {
    /// Number of cached proposals.
    pub total_proposals: usize,
    /// Total proposal bytes currently stored.
    pub total_bytes: usize,
    /// Database file size in bytes, if present on disk.
    pub db_file_size_bytes: u64,
    /// Oldest cached entry timestamp as Unix seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oldest_timestamp: Option<i64>,
    /// Newest cached entry timestamp as Unix seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub newest_timestamp: Option<i64>,
    /// Enforced maximum lines per proposal.
    pub max_lines: usize,
    /// Enforced maximum bytes per proposal.
    pub max_bytes: usize,
}

/// Summary of a recent proposal for dashboard rendering.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecentProposal {
    /// Cached proposal label.
    pub label: String,
    /// Absolute source file path.
    pub file_path: String,
    /// Region start byte.
    pub byte_start: usize,
    /// Region end byte.
    pub byte_end: usize,
    /// Proposal creation time as Unix seconds.
    pub created_at: i64,
    /// Truncated single-line snippet preview.
    pub snippet_preview: String,
    /// Snippet size in bytes.
    pub snippet_bytes: usize,
}
