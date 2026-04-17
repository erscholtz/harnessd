//! IPC server using named pipes (Windows) and Unix domain sockets (Unix).
//! Handles JSON-RPC 2.0 requests over async byte streams.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::parser::{AnchorKind, LanguageParsers, SupportedLanguage};
use crate::rpc::{
    CompleteParams, CompletionSuggestion, JsonRpcRequest, JsonRpcResponse, PrefetchParams,
    PrefetchResult, StatusResult,
};
use crate::state::DaemonState;

/// Start the IPC server and listen for JSON-RPC connections.
pub async fn serve(
    state: Arc<DaemonState>,
    shutdown_tx: mpsc::Sender<()>,
    mut shutdown_rx: mpsc::Receiver<()>,
) -> anyhow::Result<()> {
    let socket_path = state.runtime_dir.join("daemon.sock");

    // Remove stale socket file if it exists
    if socket_path.exists() {
        tokio::fs::remove_file(&socket_path)
            .await
            .context("failed to remove stale socket file")?;
    }

    #[cfg(unix)]
    {
        use tokio::net::UnixListener;
        let listener = UnixListener::bind(&socket_path)
            .with_context(|| format!("failed to bind Unix socket at {}", socket_path.display()))?;

        tracing::info!(socket = %socket_path.display(), "IPC server listening");

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _)) => {
                            let state_clone = Arc::clone(&state);
                            let shutdown_tx_clone = shutdown_tx.clone();
                            tokio::spawn(handle_connection(stream, state_clone, shutdown_tx_clone));
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "failed to accept connection");
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    tracing::info!("IPC server shutting down");
                    break;
                }
            }
        }
    }

    #[cfg(windows)]
    {
        use tokio::net::TcpListener;

        // On Windows, use TCP loopback. The port is chosen dynamically.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("failed to bind TCP listener")?;
        let addr = listener.local_addr()?;

        // Write the port to a file so clients can find it
        let port_file = state.runtime_dir.join("daemon.port");
        tokio::fs::write(&port_file, addr.port().to_string())
            .await
            .context("failed to write port file")?;

        tracing::info!(addr = %addr, "IPC server listening on TCP");

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _)) => {
                            let state_clone = Arc::clone(&state);
                            let shutdown_tx_clone = shutdown_tx.clone();
                            tokio::spawn(handle_connection(stream, state_clone, shutdown_tx_clone));
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "failed to accept connection");
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    tracing::info!("IPC server shutting down");
                    break;
                }
            }
        }

        // Cleanup port file
        tokio::fs::remove_file(&port_file).await.ok();
    }

    // Cleanup
    #[cfg(unix)]
    {
        tokio::fs::remove_file(&socket_path).await.ok();
    }

    Ok(())
}

/// Handle a single JSON-RPC connection.
async fn handle_connection<S>(stream: S, state: Arc<DaemonState>, shutdown_tx: mpsc::Sender<()>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                let response = process_request(&line, &state, &shutdown_tx).await;
                if let Err(e) = writer.write_all(response.as_bytes()).await {
                    tracing::warn!(error = %e, "failed to write response");
                    break;
                }
                if let Err(e) = writer.write_all(b"\n").await {
                    tracing::warn!(error = %e, "failed to write newline");
                    break;
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to read from connection");
                break;
            }
        }
    }
}

/// Process a single JSON-RPC request and return the response as a string.
async fn process_request(
    line: &str,
    state: &Arc<DaemonState>,
    shutdown_tx: &mpsc::Sender<()>,
) -> String {
    let request: JsonRpcRequest = match serde_json::from_str(line.trim()) {
        Ok(req) => req,
        Err(e) => {
            return JsonRpcResponse::error(None, -32700, format!("Parse error: {}", e), None)
                .to_string();
        }
    };

    let id = request.id.clone();
    state.record_request(&request.method);

    match request.method.as_str() {
        "complete" => handle_complete(request, state).await,
        "prefetch" => handle_prefetch(request, state).await,
        "status" => handle_status(request, state).await,
        "shutdown" => handle_shutdown(request, shutdown_tx).await,
        _ => JsonRpcResponse::error(
            id,
            -32601,
            format!("Method not found: {}", request.method),
            None,
        ),
    }
    .to_string()
}

async fn handle_complete(request: JsonRpcRequest, state: &Arc<DaemonState>) -> JsonRpcResponse {
    let params: CompleteParams = match request.params {
        Some(p) => match serde_json::from_value(p) {
            Ok(params) => params,
            Err(e) => {
                return JsonRpcResponse::error(
                    request.id,
                    -32602,
                    format!("Invalid params: {}", e),
                    None,
                );
            }
        },
        None => {
            return JsonRpcResponse::error(request.id, -32602, "Missing params".to_string(), None);
        }
    };

    match complete(state, &params).await {
        Ok(suggestions) => JsonRpcResponse::success(
            request.id,
            serde_json::json!({ "suggestions": suggestions }),
        ),
        Err(e) => JsonRpcResponse::error(request.id, -32000, format!("Complete failed: {e}"), None),
    }
}

async fn handle_prefetch(request: JsonRpcRequest, state: &Arc<DaemonState>) -> JsonRpcResponse {
    let params: PrefetchParams = match request.params {
        Some(p) => match serde_json::from_value(p) {
            Ok(params) => params,
            Err(e) => {
                return JsonRpcResponse::error(
                    request.id,
                    -32602,
                    format!("Invalid params: {}", e),
                    None,
                );
            }
        },
        None => {
            return JsonRpcResponse::error(request.id, -32602, "Missing params".to_string(), None);
        }
    };

    match prefetch(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => JsonRpcResponse::error(request.id, -32000, format!("Prefetch failed: {e}"), None),
    }
}

async fn handle_status(request: JsonRpcRequest, state: &Arc<DaemonState>) -> JsonRpcResponse {
    match status(state).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => JsonRpcResponse::error(request.id, -32000, format!("Status failed: {e}"), None),
    }
}

async fn handle_shutdown(
    request: JsonRpcRequest,
    shutdown_tx: &mpsc::Sender<()>,
) -> JsonRpcResponse {
    let _ = shutdown_tx.send(()).await;
    JsonRpcResponse::success(request.id, serde_json::json!({ "ok": true }))
}

/// Compute suggestions for a file and byte offset using the cached fast path.
pub async fn complete(
    state: &Arc<DaemonState>,
    params: &CompleteParams,
) -> anyhow::Result<Vec<CompletionSuggestion>> {
    tracing::debug!(file = %params.file, offset = params.offset, "complete request");

    let content = tokio::fs::read_to_string(&params.file)
        .await
        .with_context(|| format!("failed to read {}", params.file))?;

    let mut parser = state.parser.write().await;
    let parsed = parser.parse_file(Path::new(&params.file), &content)?;

    let Some(node) = parsed.node_at_offset(params.offset) else {
        return Ok(vec![]);
    };

    let content_hash = crate::parser::hash_node_region(&content, node);
    let proposals = state
        .cache
        .lookup(
            &params.file,
            node.start_byte(),
            node.end_byte(),
            &content_hash,
        )
        .await
        .unwrap_or_default();

    let mut suggestions = proposals_to_suggestions(proposals, "cached");
    if !suggestions.is_empty() {
        apply_prefix_filter(&mut suggestions, params.prefix.as_deref());
        return Ok(suggestions);
    }

    if let Some(function) = parsed.enclosing_function(params.offset) {
        let function_text = parsed.node_text(function);
        let function_hash = crate::cache::compute_hash(function_text);
        let function_proposals = state
            .cache
            .lookup(
                &params.file,
                function.start_byte(),
                function.end_byte(),
                &function_hash,
            )
            .await
            .unwrap_or_default();
        suggestions = proposals_to_suggestions(function_proposals, "cached (function)");
    }

    apply_prefix_filter(&mut suggestions, params.prefix.as_deref());
    Ok(suggestions)
}

/// Scan a file or workspace path, find anchors, and populate the proposal cache.
pub async fn prefetch(
    state: &Arc<DaemonState>,
    params: &PrefetchParams,
) -> anyhow::Result<PrefetchResult> {
    let files = collect_supported_files(Path::new(&params.path))?;
    let mut scanned_files = 0usize;
    let mut anchors_found = 0usize;
    let mut proposals_stored = 0usize;

    for file in files {
        let file_str = file.to_string_lossy().to_string();
        let content = match tokio::fs::read_to_string(&file).await {
            Ok(content) => content,
            Err(e) => {
                tracing::warn!(file = %file.display(), error = %e, "skipping unreadable file");
                continue;
            }
        };

        scanned_files += 1;
        let mut parser = state.parser.write().await;
        let parsed = match parser.parse_file(&file, &content) {
            Ok(parsed) => parsed,
            Err(e) => {
                tracing::warn!(file = %file.display(), error = %e, "skipping unparsable file");
                continue;
            }
        };

        let anchors = parsed.find_anchors();
        anchors_found += anchors.len();

        for anchor in anchors {
            let (byte_start, byte_end, content_hash, label) =
                if let Some(function) = parsed.enclosing_function(anchor.byte_range.start) {
                    let function_text = parsed.node_text(function);
                    (
                        function.start_byte(),
                        function.end_byte(),
                        crate::cache::compute_hash(function_text),
                        label_for_anchor(anchor.kind),
                    )
                } else {
                    (
                        anchor.byte_range.start,
                        anchor.byte_range.end,
                        crate::cache::compute_hash(&anchor.context),
                        label_for_anchor(anchor.kind),
                    )
                };

            let snippet = snippet_for_anchor(
                parsed.language,
                parsed.comment_prefix(),
                anchor.kind,
                &anchor.context,
            );
            match state
                .cache
                .store(
                    &file_str,
                    byte_start,
                    byte_end,
                    &content_hash,
                    &snippet,
                    label,
                )
                .await
            {
                Ok(_) => proposals_stored += 1,
                Err(e) => tracing::warn!(
                    file = %file.display(),
                    error = %e,
                    "failed to store prefetched proposal"
                ),
            }
        }
    }

    Ok(PrefetchResult {
        scanned_files,
        anchors_found,
        proposals_stored,
    })
}

/// Return a snapshot of daemon status for dashboards and diagnostics.
pub async fn status(state: &Arc<DaemonState>) -> anyhow::Result<StatusResult> {
    state.status_snapshot().await
}

fn proposals_to_suggestions(
    proposals: Vec<crate::cache::Proposal>,
    detail: &str,
) -> Vec<CompletionSuggestion> {
    proposals
        .into_iter()
        .map(|p| CompletionSuggestion {
            label: p.label,
            insert_text: p.snippet,
            detail: Some(detail.to_string()),
            documentation: None,
        })
        .collect()
}

fn apply_prefix_filter(suggestions: &mut Vec<CompletionSuggestion>, prefix: Option<&str>) {
    let Some(prefix) = prefix.filter(|prefix| !prefix.is_empty()) else {
        return;
    };
    suggestions.retain(|suggestion| {
        suggestion.label.contains(prefix) || suggestion.insert_text.contains(prefix)
    });
}

fn collect_supported_files(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(if LanguageParsers::supports_path(path) {
            vec![path.to_path_buf()]
        } else {
            Vec::new()
        });
    }

    if !path.is_dir() {
        anyhow::bail!("path does not exist: {}", path.display());
    }

    let mut files = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry.file_type()?.is_dir() {
            files.extend(collect_supported_files(&entry_path)?);
        } else if LanguageParsers::supports_path(&entry_path) {
            files.push(entry_path);
        }
    }
    Ok(files)
}

fn label_for_anchor(kind: AnchorKind) -> &'static str {
    match kind {
        AnchorKind::TodoComment => "Implement TODO",
        AnchorKind::FixmeComment => "Fix FIXME",
        AnchorKind::TodoMacro => "Replace todo!()",
        AnchorKind::UnimplementedMacro => "Replace unimplemented!()",
        AnchorKind::EmptyFunctionBody => "Fill empty function",
    }
}

fn snippet_for_anchor(
    language: SupportedLanguage,
    comment_prefix: &str,
    kind: AnchorKind,
    context: &str,
) -> String {
    match kind {
        AnchorKind::TodoComment | AnchorKind::FixmeComment => {
            format!(
                "{comment_prefix} {}\n{comment_prefix} Placeholder generated by harnessd.\n{}",
                context.trim(),
                placeholder_statement(language, summarize_context(context))
            )
        }
        AnchorKind::TodoMacro | AnchorKind::UnimplementedMacro => {
            placeholder_statement(language, summarize_context(context))
        }
        AnchorKind::EmptyFunctionBody => {
            placeholder_statement(language, "implement function body".to_string())
        }
    }
}

fn placeholder_statement(language: SupportedLanguage, summary: String) -> String {
    match language {
        SupportedLanguage::Rust => format!("todo!(\"{summary}\");"),
        SupportedLanguage::Python => format!("raise NotImplementedError(\"{summary}\")"),
        SupportedLanguage::JavaScript | SupportedLanguage::TypeScript | SupportedLanguage::Tsx => {
            format!("throw new Error(\"{summary}\");")
        }
        SupportedLanguage::Go => format!("panic(\"{summary}\")"),
    }
}

fn summarize_context(context: &str) -> String {
    let summary: String = context
        .chars()
        .filter(|ch| !matches!(ch, '\r' | '\n' | '"'))
        .take(80)
        .collect();
    if summary.is_empty() {
        "implement".to_string()
    } else {
        summary
    }
}
