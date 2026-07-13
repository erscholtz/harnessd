//! IPC server using named pipes (Windows) and Unix domain sockets (Unix).
//! Handles JSON-RPC 2.0 requests over async byte streams.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::acp::GenerationContext;
use crate::parser::{Anchor, AnchorKind, LanguageParsers, ParsedFile, SupportedLanguage};
use crate::rpc::{
    AnchorInfo, AnchorsParams, CodexSessionsParams, CompleteParams, CompletionSuggestion,
    GenerateParams, InlineFastParams, InlineParams, InlinePrepareParams, JsonRpcRequest,
    JsonRpcResponse, MarkCreateParams, MarkDeleteParams, MarkListParams, MarkStepParams,
    PrefetchParams, PrefetchResult, ScratchCreateParams, SettingsUpdateParams, StatusResult,
    ThreadAttachParams, ThreadCreateParams, ThreadDeleteParams, ThreadExampleCreateParams,
    ThreadLinkParams, ThreadListParams, ThreadResolveParams,
};
use crate::scratch::ScratchWriteOptions;
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
        "anchors" => handle_anchors(request, state).await,
        "generate" => handle_generate(request, state).await,
        "inline" => handle_inline(request, state).await,
        "inline.fast" => handle_inline_fast(request, state).await,
        "inline.prepare" => handle_inline_prepare(request, state).await,
        "scratch.create" => handle_scratch_create(request, state).await,
        "mark.create" => handle_mark_create(request, state).await,
        "mark.list" => handle_mark_list(request, state).await,
        "mark.delete" => handle_mark_delete(request, state).await,
        "mark.next" => handle_mark_next(request, state).await,
        "mark.prev" => handle_mark_prev(request, state).await,
        "settings.get" => handle_settings_get(request, state).await,
        "settings.update" => handle_settings_update(request, state).await,
        "codex.sessions" => handle_codex_sessions(request).await,
        "thread.create" => handle_thread_create(request, state).await,
        "thread.list" => handle_thread_list(request, state).await,
        "thread.link" => handle_thread_link(request, state).await,
        "thread.resolve" => handle_thread_resolve(request, state).await,
        "thread.attach" => handle_thread_attach(request, state).await,
        "thread.delete" => handle_thread_delete(request, state).await,
        "thread.example.create" => handle_thread_example_create(request, state).await,
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

async fn handle_codex_sessions(request: JsonRpcRequest) -> JsonRpcResponse {
    let params: CodexSessionsParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match codex_sessions(&params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => JsonRpcResponse::error(
            request.id,
            -32000,
            format!("Codex session listing failed: {e}"),
            None,
        ),
    }
}

async fn handle_thread_create(
    request: JsonRpcRequest,
    state: &Arc<DaemonState>,
) -> JsonRpcResponse {
    let params: ThreadCreateParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match thread_create(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => JsonRpcResponse::error(
            request.id,
            -32000,
            format!("Thread create failed: {e}"),
            None,
        ),
    }
}

async fn handle_thread_list(request: JsonRpcRequest, state: &Arc<DaemonState>) -> JsonRpcResponse {
    let params: ThreadListParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match thread_list(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => {
            JsonRpcResponse::error(request.id, -32000, format!("Thread list failed: {e}"), None)
        }
    }
}

async fn handle_thread_link(request: JsonRpcRequest, state: &Arc<DaemonState>) -> JsonRpcResponse {
    let params: ThreadLinkParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match thread_link(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => {
            JsonRpcResponse::error(request.id, -32000, format!("Thread link failed: {e}"), None)
        }
    }
}

async fn handle_thread_resolve(
    request: JsonRpcRequest,
    state: &Arc<DaemonState>,
) -> JsonRpcResponse {
    let params: ThreadResolveParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match thread_resolve(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => JsonRpcResponse::error(
            request.id,
            -32000,
            format!("Thread resolve failed: {e}"),
            None,
        ),
    }
}

async fn handle_thread_attach(
    request: JsonRpcRequest,
    state: &Arc<DaemonState>,
) -> JsonRpcResponse {
    let params: ThreadAttachParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match thread_attach(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => JsonRpcResponse::error(
            request.id,
            -32000,
            format!("Thread attach failed: {e}"),
            None,
        ),
    }
}

async fn handle_thread_delete(
    request: JsonRpcRequest,
    state: &Arc<DaemonState>,
) -> JsonRpcResponse {
    let params: ThreadDeleteParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match thread_delete(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => JsonRpcResponse::error(
            request.id,
            -32000,
            format!("Thread delete failed: {e}"),
            None,
        ),
    }
}

async fn handle_thread_example_create(
    request: JsonRpcRequest,
    state: &Arc<DaemonState>,
) -> JsonRpcResponse {
    let params: ThreadExampleCreateParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match thread_example_create(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => JsonRpcResponse::error(
            request.id,
            -32001,
            format!("Thread example create failed: {e}"),
            Some(serde_json::json!({ "retryable": true })),
        ),
    }
}

fn required_params<T>(request: &JsonRpcRequest) -> Result<T, Box<JsonRpcResponse>>
where
    T: serde::de::DeserializeOwned,
{
    match request.params.clone() {
        Some(p) => serde_json::from_value(p).map_err(|e| {
            Box::new(JsonRpcResponse::error(
                request.id.clone(),
                -32602,
                format!("Invalid params: {e}"),
                None,
            ))
        }),
        None => Err(Box::new(JsonRpcResponse::error(
            request.id.clone(),
            -32602,
            "Missing params".to_string(),
            None,
        ))),
    }
}

async fn handle_anchors(request: JsonRpcRequest, state: &Arc<DaemonState>) -> JsonRpcResponse {
    let params: AnchorsParams = match request.params {
        Some(p) => match serde_json::from_value(p) {
            Ok(params) => params,
            Err(e) => {
                return JsonRpcResponse::error(
                    request.id,
                    -32602,
                    format!("Invalid params: {e}"),
                    None,
                );
            }
        },
        None => {
            return JsonRpcResponse::error(request.id, -32602, "Missing params".to_string(), None);
        }
    };
    match anchors(state, &params).await {
        Ok(anchors) => {
            JsonRpcResponse::success(request.id, serde_json::json!({ "anchors": anchors }))
        }
        Err(e) => JsonRpcResponse::error(request.id, -32000, format!("Anchors failed: {e}"), None),
    }
}

async fn handle_generate(request: JsonRpcRequest, state: &Arc<DaemonState>) -> JsonRpcResponse {
    let params: GenerateParams = match request.params {
        Some(p) => match serde_json::from_value(p) {
            Ok(params) => params,
            Err(e) => {
                return JsonRpcResponse::error(
                    request.id,
                    -32602,
                    format!("Invalid params: {e}"),
                    None,
                );
            }
        },
        None => {
            return JsonRpcResponse::error(request.id, -32602, "Missing params".to_string(), None);
        }
    };
    match generate(state, &params).await {
        Ok(suggestion) => {
            JsonRpcResponse::success(request.id, serde_json::json!({ "suggestion": suggestion }))
        }
        Err(e) => JsonRpcResponse::error(
            request.id,
            -32001,
            format!("Generation failed: {e}"),
            Some(serde_json::json!({ "retryable": true })),
        ),
    }
}

async fn handle_inline(request: JsonRpcRequest, state: &Arc<DaemonState>) -> JsonRpcResponse {
    let params: InlineParams = match request.params {
        Some(p) => match serde_json::from_value(p) {
            Ok(params) => params,
            Err(e) => {
                return JsonRpcResponse::error(
                    request.id,
                    -32602,
                    format!("Invalid params: {e}"),
                    None,
                );
            }
        },
        None => {
            return JsonRpcResponse::error(request.id, -32602, "Missing params".to_string(), None);
        }
    };
    match crate::autocomplete::inline(state, &params).await {
        Ok(suggestion) => {
            JsonRpcResponse::success(request.id, serde_json::json!({ "suggestion": suggestion }))
        }
        Err(e) => JsonRpcResponse::error(
            request.id,
            -32001,
            format!("Inline generation failed: {e}"),
            Some(serde_json::json!({ "retryable": true })),
        ),
    }
}

async fn handle_inline_fast(request: JsonRpcRequest, state: &Arc<DaemonState>) -> JsonRpcResponse {
    let params: InlineFastParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match crate::autocomplete::inline_fast(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => JsonRpcResponse::error(
            request.id,
            -32001,
            format!("Fast inline failed: {e}"),
            Some(serde_json::json!({ "retryable": true })),
        ),
    }
}

async fn handle_inline_prepare(
    request: JsonRpcRequest,
    state: &Arc<DaemonState>,
) -> JsonRpcResponse {
    let params: InlinePrepareParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match crate::autocomplete::inline_prepare(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => JsonRpcResponse::error(
            request.id,
            -32001,
            format!("Inline prepare failed: {e}"),
            Some(serde_json::json!({ "retryable": true })),
        ),
    }
}

async fn handle_scratch_create(
    request: JsonRpcRequest,
    state: &Arc<DaemonState>,
) -> JsonRpcResponse {
    let params: ScratchCreateParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match scratch_create(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => JsonRpcResponse::error(
            request.id,
            -32001,
            format!("Scratch generation failed: {e}"),
            Some(serde_json::json!({ "retryable": true })),
        ),
    }
}

async fn handle_mark_create(request: JsonRpcRequest, state: &Arc<DaemonState>) -> JsonRpcResponse {
    let params: MarkCreateParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match mark_create(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => {
            JsonRpcResponse::error(request.id, -32000, format!("Mark create failed: {e}"), None)
        }
    }
}

async fn handle_mark_list(request: JsonRpcRequest, state: &Arc<DaemonState>) -> JsonRpcResponse {
    let params: MarkListParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match mark_list(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => {
            JsonRpcResponse::error(request.id, -32000, format!("Mark list failed: {e}"), None)
        }
    }
}

async fn handle_mark_delete(request: JsonRpcRequest, state: &Arc<DaemonState>) -> JsonRpcResponse {
    let params: MarkDeleteParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match mark_delete(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => {
            JsonRpcResponse::error(request.id, -32000, format!("Mark delete failed: {e}"), None)
        }
    }
}

async fn handle_mark_next(request: JsonRpcRequest, state: &Arc<DaemonState>) -> JsonRpcResponse {
    let params: MarkStepParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match mark_next(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => {
            JsonRpcResponse::error(request.id, -32000, format!("Mark next failed: {e}"), None)
        }
    }
}

async fn handle_mark_prev(request: JsonRpcRequest, state: &Arc<DaemonState>) -> JsonRpcResponse {
    let params: MarkStepParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match mark_prev(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => {
            JsonRpcResponse::error(request.id, -32000, format!("Mark prev failed: {e}"), None)
        }
    }
}

async fn handle_settings_get(request: JsonRpcRequest, state: &Arc<DaemonState>) -> JsonRpcResponse {
    match settings_get(state).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => JsonRpcResponse::error(
            request.id,
            -32000,
            format!("Settings get failed: {e}"),
            None,
        ),
    }
}

async fn handle_settings_update(
    request: JsonRpcRequest,
    state: &Arc<DaemonState>,
) -> JsonRpcResponse {
    let params: SettingsUpdateParams = match required_params(&request) {
        Ok(params) => params,
        Err(response) => return *response,
    };
    match settings_update(state, &params).await {
        Ok(result) => JsonRpcResponse::success(request.id, serde_json::json!(result)),
        Err(e) => JsonRpcResponse::error(
            request.id,
            -32000,
            format!("Settings update failed: {e}"),
            None,
        ),
    }
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

    let lookup = complete_from_content(
        state,
        &params.file,
        &content,
        params.offset,
        params.prefix.as_deref(),
    )
    .await?;
    Ok(lookup.suggestions)
}

pub(crate) struct CompleteLookup {
    pub(crate) suggestions: Vec<CompletionSuggestion>,
    pub(crate) source: &'static str,
}

pub(crate) async fn complete_from_content(
    state: &Arc<DaemonState>,
    file: &str,
    content: &str,
    offset: usize,
    prefix: Option<&str>,
) -> anyhow::Result<CompleteLookup> {
    let mut parser = state.parser.write().await;
    let parsed = parser.parse_file(Path::new(file), content)?;

    let Some(node) = parsed.node_at_offset(offset) else {
        return Ok(CompleteLookup {
            suggestions: vec![],
            source: "none",
        });
    };

    let content_hash = crate::parser::hash_node_region(content, node);
    let proposals = state
        .cache
        .lookup(file, node.start_byte(), node.end_byte(), &content_hash)
        .await
        .unwrap_or_default();

    let mut suggestions = proposals_to_suggestions(proposals, "cached");
    apply_prefix_filter(&mut suggestions, prefix);
    if !suggestions.is_empty() {
        return Ok(CompleteLookup {
            suggestions,
            source: "cache",
        });
    }

    for region in regions_for_anchors(&parsed, file)
        .into_iter()
        .filter(|region| offset >= region.context_start && offset <= region.context_end)
    {
        let proposals = state
            .cache
            .lookup(file, region.start, region.end, &region.content_hash)
            .await
            .unwrap_or_default();
        suggestions.extend(proposals_to_suggestions(proposals, "cached (anchor)"));
    }
    apply_prefix_filter(&mut suggestions, prefix);
    if !suggestions.is_empty() {
        return Ok(CompleteLookup {
            suggestions,
            source: "anchor-cache",
        });
    }

    // Preserve entries created by older function-scoped cache behavior as a fallback only.
    if let Some(function) = parsed.enclosing_function(offset) {
        let function_text = parsed.node_text(function);
        let function_hash = crate::cache::compute_hash(function_text);
        let function_proposals = state
            .cache
            .lookup(
                file,
                function.start_byte(),
                function.end_byte(),
                &function_hash,
            )
            .await
            .unwrap_or_default();
        suggestions = proposals_to_suggestions(function_proposals, "cached (function)");
    }

    apply_prefix_filter(&mut suggestions, prefix);
    let source = if suggestions.is_empty() {
        "none"
    } else {
        "function-cache"
    };
    Ok(CompleteLookup {
        suggestions,
        source,
    })
}

/// Inspect the anchors in one saved file without triggering generation.
pub async fn anchors(
    state: &Arc<DaemonState>,
    params: &AnchorsParams,
) -> anyhow::Result<Vec<AnchorInfo>> {
    let content = tokio::fs::read_to_string(&params.file)
        .await
        .with_context(|| format!("failed to read {}", params.file))?;
    let mut parser = state.parser.write().await;
    let parsed = parser.parse_file(Path::new(&params.file), &content)?;
    drop(parser);

    let regions = regions_for_anchors(&parsed, &params.file);
    let mut result = Vec::with_capacity(regions.len());
    for region in regions {
        let proposals = state
            .cache
            .lookup(&params.file, region.start, region.end, &region.content_hash)
            .await?;
        let status = if !proposals.is_empty() {
            "ready"
        } else if state.generation_failed(&region.key).await {
            "failed"
        } else {
            "candidate"
        };
        result.push(AnchorInfo {
            anchor_start: region.anchor.byte_range.start,
            anchor_end: region.anchor.byte_range.end,
            region_start: region.start,
            region_end: region.end,
            kind: kind_name(region.anchor.kind).to_string(),
            label: label_for_anchor(region.anchor.kind).to_string(),
            status: status.to_string(),
        });
    }
    Ok(result)
}

/// Generate a bounded proposal for the anchor region at a saved-file cursor location.
pub async fn generate(
    state: &Arc<DaemonState>,
    params: &GenerateParams,
) -> anyhow::Result<CompletionSuggestion> {
    let content = tokio::fs::read_to_string(&params.file)
        .await
        .with_context(|| format!("failed to read {}", params.file))?;
    let mut parser = state.parser.write().await;
    let parsed = parser.parse_file(Path::new(&params.file), &content)?;
    drop(parser);

    let region = regions_for_anchors(&parsed, &params.file)
        .into_iter()
        .find(|region| params.offset >= region.start && params.offset <= region.end)
        .context("cursor is not within an anchor-bearing region")?;

    if let Some(proposal) = state
        .cache
        .lookup(&params.file, region.start, region.end, &region.content_hash)
        .await?
        .into_iter()
        .next()
    {
        return Ok(proposal_to_suggestion(proposal, "cached"));
    }

    let context = GenerationContext {
        file: Path::new(&params.file),
        language: language_name(parsed.language),
        anchor_kind: kind_name(region.anchor.kind),
        anchor_text: &region.anchor.context,
        region_text: &region.text,
    };
    let snippet = match state.acp.generate(&context).await {
        Ok(snippet) => snippet,
        Err(error) => {
            state.mark_generation_failed(region.key).await;
            return Err(error);
        }
    };
    state
        .cache
        .store(
            &params.file,
            region.start,
            region.end,
            &region.content_hash,
            &snippet,
            label_for_anchor(region.anchor.kind),
        )
        .await?;
    state.clear_generation_failed(&region.key).await;
    Ok(CompletionSuggestion {
        label: label_for_anchor(region.anchor.kind).to_string(),
        insert_text: snippet,
        detail: Some("generated through ACP".to_string()),
        documentation: None,
    })
}

/// Create a saved scratch preview artifact using live-buffer context.
pub async fn scratch_create(
    state: &Arc<DaemonState>,
    params: &ScratchCreateParams,
) -> anyhow::Result<crate::rpc::ScratchCreateResult> {
    let settings = state.settings.load()?;
    crate::scratch::create(
        &state.scratch,
        params,
        &ScratchWriteOptions::new(settings.scratch_storage_mode, None),
    )
    .await
}

/// Create a persistent external source mark.
pub async fn mark_create(
    state: &Arc<DaemonState>,
    params: &MarkCreateParams,
) -> anyhow::Result<crate::marks::MarkCreateResult> {
    state.marks.create(params)
}

/// List persistent external source marks.
pub async fn mark_list(
    state: &Arc<DaemonState>,
    params: &MarkListParams,
) -> anyhow::Result<crate::marks::MarkListResult> {
    state.marks.list(params)
}

/// Delete an external source mark, and optionally its attached thread.
pub async fn mark_delete(
    state: &Arc<DaemonState>,
    params: &MarkDeleteParams,
) -> anyhow::Result<crate::marks::MarkDeleteResult> {
    let result = state.marks.delete(params)?;
    if params.delete_attached_thread
        && let Some(thread_id) = result.mark.as_ref().and_then(|mark| mark.thread_id.clone())
    {
        let deleted = thread_delete(
            state,
            &ThreadDeleteParams {
                thread_id: thread_id.clone(),
            },
        )
        .await?;
        if deleted.thread.is_none() {
            state.marks.unlink_thread(&thread_id)?;
        }
    }
    Ok(result)
}

/// Return the next mark in a file.
pub async fn mark_next(
    state: &Arc<DaemonState>,
    params: &MarkStepParams,
) -> anyhow::Result<crate::marks::MarkStepResult> {
    state.marks.next(params)
}

/// Return the previous mark in a file.
pub async fn mark_prev(
    state: &Arc<DaemonState>,
    params: &MarkStepParams,
) -> anyhow::Result<crate::marks::MarkStepResult> {
    state.marks.prev(params)
}

/// Return daemon settings.
pub async fn settings_get(
    state: &Arc<DaemonState>,
) -> anyhow::Result<crate::settings::SettingsResult> {
    state.settings.get()
}

/// Update daemon settings.
pub async fn settings_update(
    state: &Arc<DaemonState>,
    params: &SettingsUpdateParams,
) -> anyhow::Result<crate::settings::SettingsResult> {
    state.settings.update(params)
}

/// List Codex sessions for a workspace.
pub async fn codex_sessions(
    params: &CodexSessionsParams,
) -> anyhow::Result<crate::codex_sessions::CodexSessionsResult> {
    crate::codex_sessions::list_sessions(params)
}

/// Create a persistent source-line thread anchor.
pub async fn thread_create(
    state: &Arc<DaemonState>,
    params: &ThreadCreateParams,
) -> anyhow::Result<crate::threads::ThreadCreateResult> {
    let mark = state.marks.create_mark(
        &params.workspace,
        &params.file,
        params.offset,
        &params.content,
        None,
    )?;
    let created = state
        .threads
        .create_with_mark(params, Some(mark.mark_id.clone()))?;
    state
        .marks
        .link_thread(&mark.mark_id, &created.thread.thread_id)?;
    Ok(created)
}

/// List persistent source-line thread anchors.
pub async fn thread_list(
    state: &Arc<DaemonState>,
    params: &ThreadListParams,
) -> anyhow::Result<crate::threads::ThreadListResult> {
    state.threads.list(params)
}

/// Link a thread to a Codex session.
pub async fn thread_link(
    state: &Arc<DaemonState>,
    params: &ThreadLinkParams,
) -> anyhow::Result<crate::threads::ThreadLinkResult> {
    state.threads.link(params)
}

/// Resolve a newly launched thread to a saved Codex session.
pub async fn thread_resolve(
    state: &Arc<DaemonState>,
    params: &ThreadResolveParams,
) -> anyhow::Result<crate::threads::ThreadResolveResult> {
    state.threads.resolve(params)
}

/// Attach an existing Codex session to the current line.
pub async fn thread_attach(
    state: &Arc<DaemonState>,
    params: &ThreadAttachParams,
) -> anyhow::Result<crate::threads::ThreadLinkResult> {
    let mark = state.marks.create_mark(
        &params.workspace,
        &params.file,
        params.offset,
        &params.content,
        None,
    )?;
    let created = state.threads.create_with_mark(
        &ThreadCreateParams {
            workspace: params.workspace.clone(),
            file: params.file.clone(),
            offset: params.offset,
            content: params.content.clone(),
            prompt: format!("Attached Codex session {}", params.codex_session_id),
            selection_start: None,
            selection_end: None,
            model: None,
            reasoning_effort: None,
        },
        Some(mark.mark_id.clone()),
    )?;
    let linked = state.threads.link(&ThreadLinkParams {
        thread_id: created.thread.thread_id,
        codex_session_id: params.codex_session_id.clone(),
        codex_session_path: None,
    })?;
    state
        .marks
        .link_thread(&mark.mark_id, &linked.thread.thread_id)?;
    Ok(linked)
}

/// Delete a thread and its scratch artifacts.
pub async fn thread_delete(
    state: &Arc<DaemonState>,
    params: &ThreadDeleteParams,
) -> anyhow::Result<crate::threads::ThreadDeleteResult> {
    let deleted = state.threads.delete(params)?;
    if let Some(thread) = &deleted.thread {
        let settings = state.settings.load()?;
        for example in &thread.examples {
            crate::scratch::delete_artifact_scratch_dir(
                &state.runtime_dir,
                Path::new(&example.path),
            )?;
        }
        crate::scratch::delete_thread_scratch_dir(
            &state.runtime_dir,
            settings.scratch_storage_mode,
            Path::new(&thread.workspace),
            &thread.thread_id,
        )?;
        state.marks.unlink_thread(&thread.thread_id)?;
    }
    Ok(deleted)
}

/// Create a scratch example artifact and link it to a persistent thread.
pub async fn thread_example_create(
    state: &Arc<DaemonState>,
    params: &ThreadExampleCreateParams,
) -> anyhow::Result<crate::threads::ThreadExampleCreateResult> {
    let settings = state.settings.load()?;
    let scratch = crate::scratch::create(
        &state.scratch,
        &ScratchCreateParams {
            workspace: params.workspace.clone(),
            file: params.file.clone(),
            offset: params.offset,
            content: params.content.clone(),
            prompt: params.prompt.clone(),
            selection_start: params.selection_start,
            selection_end: params.selection_end,
            model: params.model.clone(),
            reasoning_effort: params.reasoning_effort.clone(),
        },
        &ScratchWriteOptions::new(
            settings.scratch_storage_mode,
            Some(params.thread_id.clone()),
        ),
    )
    .await?;
    state
        .threads
        .add_example(&params.thread_id, &scratch, &params.prompt)
        .map_err(|error| {
            anyhow::anyhow!(
                "created example at {}, but failed to link it to thread {}: {error}",
                scratch.path,
                params.thread_id
            )
        })
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

        let regions = regions_for_anchors(&parsed, &file_str);
        anchors_found += regions.len();

        for region in regions {
            let snippet = snippet_for_anchor(
                parsed.language,
                parsed.comment_prefix(),
                region.anchor.kind,
                &region.anchor.context,
            );
            match state
                .cache
                .store(
                    &file_str,
                    region.start,
                    region.end,
                    &region.content_hash,
                    &snippet,
                    label_for_anchor(region.anchor.kind),
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
        .map(|p| proposal_to_suggestion(p, detail))
        .collect()
}

fn proposal_to_suggestion(proposal: crate::cache::Proposal, detail: &str) -> CompletionSuggestion {
    CompletionSuggestion {
        label: proposal.label,
        insert_text: proposal.snippet,
        detail: Some(detail.to_string()),
        documentation: None,
    }
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

fn kind_name(kind: AnchorKind) -> &'static str {
    match kind {
        AnchorKind::TodoComment => "todo_comment",
        AnchorKind::FixmeComment => "fixme_comment",
        AnchorKind::TodoMacro => "todo_macro",
        AnchorKind::UnimplementedMacro => "unimplemented_macro",
        AnchorKind::EmptyFunctionBody => "empty_function_body",
    }
}

pub(crate) fn language_name(language: SupportedLanguage) -> &'static str {
    match language {
        SupportedLanguage::Rust => "Rust",
        SupportedLanguage::JavaScript => "JavaScript",
        SupportedLanguage::TypeScript => "TypeScript",
        SupportedLanguage::Tsx => "TSX",
        SupportedLanguage::Python => "Python",
        SupportedLanguage::Go => "Go",
    }
}

pub(crate) struct AnchorRegion {
    anchor: Anchor,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) context_start: usize,
    pub(crate) context_end: usize,
    content_hash: String,
    pub(crate) text: String,
    key: String,
}

pub(crate) fn regions_for_anchors(parsed: &ParsedFile, file: &str) -> Vec<AnchorRegion> {
    parsed
        .find_anchors()
        .into_iter()
        .map(|anchor| {
            let (context_start, context_end, text) =
                if let Some(function) = parsed.enclosing_function(anchor.byte_range.start) {
                    (
                        function.start_byte(),
                        function.end_byte(),
                        parsed.node_text(function).to_string(),
                    )
                } else {
                    (
                        anchor.byte_range.start,
                        anchor.byte_range.end,
                        anchor.context.clone(),
                    )
                };
            let content_hash = crate::cache::compute_hash(&text);
            let start = anchor.byte_range.start;
            let end = anchor.byte_range.end;
            let key = format!("{file}\0{start}\0{end}\0{content_hash}");
            AnchorRegion {
                anchor,
                start,
                end,
                context_start,
                context_end,
                content_hash,
                text,
                key,
            }
        })
        .collect()
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
