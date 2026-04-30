//! Minimal Language Server Protocol adapter for editor autocomplete.
//!
//! This adapter speaks LSP over stdio and forwards completion/prefetch work to
//! the long-lived daemon. It does not own parser or proposal-cache state.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use crate::rpc::{CompleteParams, CompletionSuggestion, PrefetchParams};
use crate::state::DaemonState;

/// Run the LSP adapter over process stdin/stdout.
pub async fn run_stdio() -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    run_with_io(stdin, stdout).await
}

/// Run the LSP adapter over arbitrary async I/O streams.
pub async fn run_with_io<R, W>(reader: R, mut writer: W) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut session = LspSession::new();

    while let Some(message) = read_lsp_message(&mut reader).await? {
        if let Some(response) = session.handle_json_message(message).await {
            write_lsp_message(&mut writer, &response).await?;
        }
    }

    Ok(())
}

/// Stateful LSP request handler used by stdio and tests.
#[derive(Default)]
pub struct LspSession {
    documents: HashMap<String, String>,
    daemon_state: Option<Arc<DaemonState>>,
}

impl LspSession {
    /// Create an empty LSP session.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an LSP session backed by an in-process daemon state.
    pub fn with_daemon_state(daemon_state: Arc<DaemonState>) -> Self {
        Self {
            documents: HashMap::new(),
            daemon_state: Some(daemon_state),
        }
    }

    /// Handle one decoded LSP JSON-RPC message.
    pub async fn handle_json_message(&mut self, message: Value) -> Option<Value> {
        let method = message.get("method").and_then(Value::as_str)?;
        let id = message.get("id").cloned();

        match method {
            "initialize" => id.map(|id| success_response(id, initialize_result())),
            "shutdown" => id.map(|id| success_response(id, Value::Null)),
            "textDocument/didOpen" => {
                self.handle_did_open(message.get("params")).await;
                None
            }
            "textDocument/didChange" => {
                self.handle_did_change(message.get("params"));
                None
            }
            "textDocument/didSave" => {
                self.handle_did_save(message.get("params")).await;
                None
            }
            "textDocument/completion" => {
                let id = id?;
                match self.handle_completion(message.get("params")).await {
                    Ok(items) => Some(success_response(id, Value::Array(items))),
                    Err(error) => Some(error_response(id, -32000, error.to_string())),
                }
            }
            _ => id.map(|id| error_response(id, -32601, format!("Method not found: {method}"))),
        }
    }

    async fn handle_did_open(&mut self, params: Option<&Value>) {
        let Some(text_document) = params.and_then(|p| p.get("textDocument")) else {
            return;
        };
        let Some(uri) = text_document.get("uri").and_then(Value::as_str) else {
            return;
        };
        let Some(text) = text_document.get("text").and_then(Value::as_str) else {
            return;
        };

        self.documents.insert(uri.to_string(), text.to_string());
        self.prefetch_uri(uri).await;
    }

    fn handle_did_change(&mut self, params: Option<&Value>) {
        let Some(uri) = params
            .and_then(|p| p.get("textDocument"))
            .and_then(|d| d.get("uri"))
            .and_then(Value::as_str)
        else {
            return;
        };
        let Some(text) = params
            .and_then(|p| p.get("contentChanges"))
            .and_then(Value::as_array)
            .and_then(|changes| changes.last())
            .and_then(|change| change.get("text"))
            .and_then(Value::as_str)
        else {
            return;
        };

        self.documents.insert(uri.to_string(), text.to_string());
    }

    async fn handle_did_save(&self, params: Option<&Value>) {
        let Some(uri) = params
            .and_then(|p| p.get("textDocument"))
            .and_then(|d| d.get("uri"))
            .and_then(Value::as_str)
        else {
            return;
        };

        self.prefetch_uri(uri).await;
    }

    async fn handle_completion(&self, params: Option<&Value>) -> anyhow::Result<Vec<Value>> {
        let params = params.context("completion params are required")?;
        let uri = params
            .get("textDocument")
            .and_then(|d| d.get("uri"))
            .and_then(Value::as_str)
            .context("completion textDocument.uri is required")?;
        let line = params
            .get("position")
            .and_then(|p| p.get("line"))
            .and_then(Value::as_u64)
            .context("completion position.line is required")? as usize;
        let character = params
            .get("position")
            .and_then(|p| p.get("character"))
            .and_then(Value::as_u64)
            .context("completion position.character is required")? as usize;

        let path = file_uri_to_path(uri)?;
        let content = match self.documents.get(uri) {
            Some(content) => content.clone(),
            None => tokio::fs::read_to_string(&path)
                .await
                .with_context(|| format!("failed to read {}", path.display()))?,
        };
        let offset = byte_offset_for_lsp_position(&content, line, character)
            .context("completion position is outside document")?;
        let suggestions = if let Some(state) = &self.daemon_state {
            crate::ipc::complete(
                state,
                &CompleteParams {
                    file: path.to_string_lossy().to_string(),
                    offset,
                    prefix: None,
                },
            )
            .await?
        } else {
            crate::commands::complete_runtime_file(&path, offset, None).await?
        };

        Ok(completion_items_from_suggestions(&suggestions))
    }

    async fn prefetch_uri(&self, uri: &str) {
        let Ok(path) = file_uri_to_path(uri) else {
            return;
        };

        if let Some(state) = &self.daemon_state {
            let _ = crate::ipc::prefetch(
                state,
                &PrefetchParams {
                    path: path.to_string_lossy().to_string(),
                },
            )
            .await;
        } else {
            prefetch_path_in_background(path);
        }
    }
}

/// Convert LSP zero-based line/UTF-16 character coordinates to a byte offset.
pub fn byte_offset_for_lsp_position(text: &str, line: usize, character: usize) -> Option<usize> {
    let mut current_line = 0usize;
    let mut line_start = 0usize;

    for (idx, ch) in text.char_indices() {
        if current_line == line {
            return byte_offset_in_line(&text[line_start..], character)
                .map(|offset| line_start + offset);
        }
        if ch == '\n' {
            current_line += 1;
            line_start = idx + ch.len_utf8();
        }
    }

    if current_line == line {
        return byte_offset_in_line(&text[line_start..], character)
            .map(|offset| line_start + offset);
    }

    None
}

/// Convert daemon completion suggestions into LSP completion items.
pub fn completion_items_from_suggestions(suggestions: &[CompletionSuggestion]) -> Vec<Value> {
    suggestions
        .iter()
        .map(|suggestion| {
            let mut item = serde_json::Map::new();
            item.insert("label".to_string(), json!(suggestion.label));
            item.insert("kind".to_string(), json!(15));
            item.insert("insertText".to_string(), json!(suggestion.insert_text));
            if let Some(detail) = &suggestion.detail {
                item.insert("detail".to_string(), json!(detail));
            }
            if let Some(documentation) = &suggestion.documentation {
                item.insert("documentation".to_string(), json!(documentation));
            }
            Value::Object(item)
        })
        .collect()
}

fn byte_offset_in_line(line_text: &str, character: usize) -> Option<usize> {
    let mut utf16_units = 0usize;
    for (idx, ch) in line_text.char_indices() {
        if ch == '\r' || ch == '\n' {
            break;
        }
        if utf16_units == character {
            return Some(idx);
        }
        utf16_units += ch.len_utf16();
        if utf16_units > character {
            return None;
        }
    }

    if utf16_units == character {
        Some(line_text.find(['\r', '\n']).unwrap_or(line_text.len()))
    } else {
        None
    }
}

fn initialize_result() -> Value {
    json!({
        "capabilities": {
            "textDocumentSync": 1,
            "completionProvider": {
                "triggerCharacters": [".", ":", "_"]
            }
        },
        "serverInfo": {
            "name": "harnessd",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn error_response(id: Value, code: i32, message: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

fn prefetch_path_in_background(path: PathBuf) {
    tokio::spawn(async move {
        let _ = crate::commands::prefetch_runtime_path(&path).await;
    });
}

fn file_uri_to_path(uri: &str) -> anyhow::Result<PathBuf> {
    let path = uri
        .strip_prefix("file://")
        .with_context(|| format!("unsupported URI scheme: {uri}"))?;
    let decoded = percent_decode(path)?;

    #[cfg(windows)]
    {
        let normalized = decoded.replace('/', "\\");
        let without_leading_slash = if normalized.len() >= 4
            && normalized.as_bytes()[0] == b'\\'
            && normalized.as_bytes()[2] == b':'
        {
            &normalized[1..]
        } else {
            normalized.as_str()
        };
        Ok(PathBuf::from(without_leading_slash))
    }

    #[cfg(not(windows))]
    {
        Ok(PathBuf::from(decoded))
    }
}

fn percent_decode(input: &str) -> anyhow::Result<String> {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut idx = 0usize;

    while idx < bytes.len() {
        if bytes[idx] == b'%' {
            let hex = input
                .get(idx + 1..idx + 3)
                .with_context(|| format!("invalid percent escape in URI: {input}"))?;
            let value = u8::from_str_radix(hex, 16)
                .with_context(|| format!("invalid percent escape in URI: %{hex}"))?;
            decoded.push(value);
            idx += 3;
        } else {
            decoded.push(bytes[idx]);
            idx += 1;
        }
    }

    Ok(String::from_utf8(decoded)?)
}

async fn read_lsp_message<R>(reader: &mut BufReader<R>) -> anyhow::Result<Option<Value>>
where
    R: AsyncRead + Unpin,
{
    let mut content_length = None;

    loop {
        let mut header = String::new();
        let bytes = reader.read_line(&mut header).await?;
        if bytes == 0 {
            return Ok(None);
        }
        let header = header.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }

        if let Some(value) = header.strip_prefix("Content-Length:") {
            content_length = Some(value.trim().parse::<usize>()?);
        }
    }

    let content_length = content_length.context("missing Content-Length header")?;
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).await?;
    Ok(Some(serde_json::from_slice(&body)?))
}

async fn write_lsp_message<W>(writer: &mut W, message: &Value) -> anyhow::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let body = serde_json::to_vec(message)?;
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await?;
    writer.write_all(&body).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{byte_offset_for_lsp_position, completion_items_from_suggestions};
    use crate::rpc::CompletionSuggestion;

    #[test]
    fn converts_lsp_position_to_byte_offset_for_ascii() {
        let text = "fn main() {\n    todo!();\n}\n";
        assert_eq!(byte_offset_for_lsp_position(text, 1, 4), Some(16));
    }

    #[test]
    fn converts_lsp_position_to_byte_offset_for_utf8_and_utf16() {
        let text = "let city = \"Toronto\";\nlet emoji = \"😀\";\n";
        let emoji_offset = text.find('😀').expect("missing emoji");
        assert_eq!(
            byte_offset_for_lsp_position(text, 1, 12),
            Some(emoji_offset - 1)
        );
        assert_eq!(
            byte_offset_for_lsp_position(text, 1, 13),
            Some(emoji_offset)
        );
        assert_eq!(
            byte_offset_for_lsp_position(text, 1, 15),
            Some(emoji_offset + 4)
        );
        assert_eq!(byte_offset_for_lsp_position(text, 1, 14), None);
    }

    #[test]
    fn maps_suggestions_to_completion_items() {
        let items = completion_items_from_suggestions(&[CompletionSuggestion {
            label: "Implement TODO".to_string(),
            insert_text: "todo!(\"demo\");".to_string(),
            detail: Some("cached".to_string()),
            documentation: Some("Generated by harnessd".to_string()),
        }]);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["label"], "Implement TODO");
        assert_eq!(items[0]["insertText"], "todo!(\"demo\");");
        assert_eq!(items[0]["detail"], "cached");
    }
}
