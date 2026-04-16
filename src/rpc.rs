//! JSON-RPC 2.0 types for the daemon protocol.

use serde::{Deserialize, Serialize};

/// A JSON-RPC 2.0 request.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
    pub id: Option<serde_json::Value>,
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: Option<serde_json::Value>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
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

    /// Convert to a JSON string.
    pub fn to_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"Internal error"},"id":null}"#
                .to_string()
        })
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

/// A completion suggestion.
#[derive(Debug, Clone, Serialize)]
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
