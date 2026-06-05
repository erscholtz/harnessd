//! Live-buffer inline autocomplete orchestration.

use std::path::Path;
use std::sync::Arc;

use crate::acp::InlineContext;
use crate::ipc::methods;
use crate::parser::ParsedFile;
use crate::rpc::{
    CompletionSuggestion, InlineFastParams, InlineFastResult, InlineParams, InlinePrepareParams,
    InlinePrepareResult,
};
use crate::state::DaemonState;

const INLINE_CONTEXT_MAX_BYTES: usize = 4_096;
const CURSOR_MARKER: &str = "<HARNESSD_CURSOR>";

/// Generate an ephemeral bounded proposal at a live-buffer cursor location.
pub async fn inline(
    state: &Arc<DaemonState>,
    params: &InlineParams,
) -> anyhow::Result<CompletionSuggestion> {
    if params.prompt.trim().is_empty() {
        anyhow::bail!("inline prompt must not be empty");
    }
    validate_live_buffer(params.offset, &params.content)?;

    let mut parser = state.parser.write().await;
    let parsed = parser.parse_file(Path::new(&params.file), &params.content)?;
    drop(parser);
    let cursor_context = inline_cursor_context(&parsed, params.offset);
    let context = InlineContext {
        file: Path::new(&params.file),
        language: methods::language_name(parsed.language),
        prompt: params.prompt.trim(),
        cursor_context: &cursor_context,
    };
    let model = crate::models::normalize_model(params.model.clone())?;
    let reasoning_effort =
        crate::models::normalize_reasoning_effort(params.reasoning_effort.clone())?;
    let snippet = state
        .acp
        .generate_inline(&context, model.as_deref(), reasoning_effort.as_deref())
        .await?;
    Ok(CompletionSuggestion {
        label: "Inline ask".to_string(),
        insert_text: snippet,
        detail: Some("generated through ACP inline ask".to_string()),
        documentation: None,
    })
}

/// Return an immediate local inline suggestion and optionally queue a slow refresh.
pub async fn inline_fast(
    state: &Arc<DaemonState>,
    params: &InlineFastParams,
) -> anyhow::Result<InlineFastResult> {
    validate_live_buffer(params.offset, &params.content)?;

    let lookup = methods::complete_from_content(
        state,
        &params.file,
        &params.content,
        params.offset,
        params.prefix.as_deref(),
    )
    .await?;
    if let Some(suggestion) = lookup.suggestions.into_iter().next() {
        state.record_inline_fast_cache_hit();
        return Ok(InlineFastResult {
            suggestion: Some(suggestion),
            source: lookup.source.to_string(),
            refresh_queued: false,
        });
    }

    let refresh_queued = if params.allow_background_refresh {
        queue_inline_refresh(state, params).await?
    } else {
        false
    };

    Ok(InlineFastResult {
        suggestion: None,
        source: "none".to_string(),
        refresh_queued,
    })
}

/// Warm the daemon-owned inline generation session for a source file.
pub async fn inline_prepare(
    state: &Arc<DaemonState>,
    params: &InlinePrepareParams,
) -> anyhow::Result<InlinePrepareResult> {
    let model = crate::models::normalize_model(params.model.clone())?;
    let reasoning_effort =
        crate::models::normalize_reasoning_effort(params.reasoning_effort.clone())?;
    state
        .acp
        .prepare_inline(
            Path::new(&params.file),
            model.as_deref(),
            reasoning_effort.as_deref(),
        )
        .await?;
    Ok(InlinePrepareResult { prepared: true })
}

async fn queue_inline_refresh(
    state: &Arc<DaemonState>,
    params: &InlineFastParams,
) -> anyhow::Result<bool> {
    let prompt = params
        .prompt
        .as_deref()
        .unwrap_or("Complete the code at the cursor with the smallest useful insertion.")
        .trim();
    if prompt.is_empty() {
        return Ok(false);
    }
    let model = crate::models::normalize_model(params.model.clone())?;
    let reasoning_effort =
        crate::models::normalize_reasoning_effort(params.reasoning_effort.clone())?;
    let key = inline_refresh_key(
        state,
        &params.file,
        &params.content,
        params.offset,
        prompt,
        model.as_deref(),
        reasoning_effort.as_deref(),
    )
    .await?;
    if !state.start_inline_refresh(key.clone()).await {
        return Ok(false);
    }

    state.record_inline_fast_refresh_queued();
    let state = Arc::clone(state);
    let file = params.file.clone();
    let content = params.content.clone();
    let prompt = prompt.to_string();
    let offset = params.offset;
    tokio::spawn(async move {
        if let Err(error) = run_inline_refresh(
            &state,
            &file,
            &content,
            params_offset_saturating(&content, offset),
            &prompt,
            model,
            reasoning_effort,
        )
        .await
        {
            tracing::debug!(file = %file, error = %error, "background inline refresh failed");
        } else {
            state.record_inline_fast_refresh_completed();
        }
        state.finish_inline_refresh(&key).await;
    });
    Ok(true)
}

fn params_offset_saturating(content: &str, offset: usize) -> usize {
    offset.min(content.len())
}

async fn inline_refresh_key(
    state: &Arc<DaemonState>,
    file: &str,
    content: &str,
    offset: usize,
    prompt: &str,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
) -> anyhow::Result<String> {
    let mut parser = state.parser.write().await;
    let parsed = parser.parse_file(Path::new(file), content)?;
    drop(parser);
    let (start, end, text) = stable_refresh_region(&parsed, offset).unwrap_or_else(|| {
        (
            offset,
            offset,
            content.get(offset..offset).unwrap_or("").to_string(),
        )
    });
    let content_hash = crate::cache::compute_hash(&text);
    Ok(format!(
        "{file}\0{start}\0{end}\0{content_hash}\0{}\0{}\0{}",
        model.unwrap_or(""),
        reasoning_effort.unwrap_or(""),
        prompt
    ))
}

async fn run_inline_refresh(
    state: &Arc<DaemonState>,
    file: &str,
    content: &str,
    offset: usize,
    prompt: &str,
    model: Option<String>,
    reasoning_effort: Option<String>,
) -> anyhow::Result<()> {
    let mut parser = state.parser.write().await;
    let parsed = parser.parse_file(Path::new(file), content)?;
    drop(parser);
    let cursor_context = inline_cursor_context(&parsed, offset);
    let context = InlineContext {
        file: Path::new(file),
        language: methods::language_name(parsed.language),
        prompt,
        cursor_context: &cursor_context,
    };
    let snippet = state
        .acp
        .generate_inline(&context, model.as_deref(), reasoning_effort.as_deref())
        .await?;

    if let Some((start, end, text)) = stable_refresh_region(&parsed, offset) {
        state
            .cache
            .store(
                file,
                start,
                end,
                &crate::cache::compute_hash(&text),
                &snippet,
                "Inline refresh",
            )
            .await?;
    }
    Ok(())
}

fn stable_refresh_region(parsed: &ParsedFile, offset: usize) -> Option<(usize, usize, String)> {
    for region in methods::regions_for_anchors(parsed, "") {
        if offset >= region.context_start && offset <= region.context_end {
            return Some((region.start, region.end, region.text));
        }
    }
    parsed.enclosing_function(offset).map(|function| {
        (
            function.start_byte(),
            function.end_byte(),
            parsed.node_text(function).to_string(),
        )
    })
}

fn validate_live_buffer(offset: usize, content: &str) -> anyhow::Result<()> {
    if offset > content.len() {
        anyhow::bail!("cursor offset is outside buffer content");
    }
    if !content.is_char_boundary(offset) {
        anyhow::bail!("cursor offset is not a UTF-8 character boundary");
    }
    Ok(())
}

fn inline_cursor_context(parsed: &ParsedFile, offset: usize) -> String {
    if let Some(function) = parsed.enclosing_function(offset) {
        let text = parsed.node_text(function);
        if text.len() <= INLINE_CONTEXT_MAX_BYTES {
            let relative_offset = offset.saturating_sub(function.start_byte());
            return insert_cursor_marker(text, relative_offset);
        }
    }

    let source = parsed.source.as_str();
    let half = INLINE_CONTEXT_MAX_BYTES / 2;
    let mut start = offset.saturating_sub(half);
    let mut end = (start + INLINE_CONTEXT_MAX_BYTES).min(source.len());
    start = previous_char_boundary(source, start);
    end = previous_char_boundary(source, end);
    if end < offset {
        end = offset;
    }
    if end - start < INLINE_CONTEXT_MAX_BYTES && end == source.len() {
        start = previous_char_boundary(source, end.saturating_sub(INLINE_CONTEXT_MAX_BYTES));
    }
    insert_cursor_marker(&source[start..end], offset - start)
}

fn insert_cursor_marker(text: &str, offset: usize) -> String {
    let mut context = String::with_capacity(text.len() + CURSOR_MARKER.len());
    context.push_str(&text[..offset]);
    context.push_str(CURSOR_MARKER);
    context.push_str(&text[offset..]);
    context
}

fn previous_char_boundary(text: &str, mut offset: usize) -> usize {
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}
