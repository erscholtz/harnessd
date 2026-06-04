//! Minimal stdio ACP client used for explicit inline generation requests.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};

use crate::cache::{MAX_BYTES, MAX_LINES};

const DEFAULT_GENERATION_TIMEOUT: Duration = Duration::from_secs(120);

/// Input context supplied to one ACP generation turn.
pub struct GenerationContext<'a> {
    /// Saved source file for workspace selection.
    pub file: &'a Path,
    /// Language identifier shown to the model.
    pub language: &'a str,
    /// Machine-readable marker kind.
    pub anchor_kind: &'a str,
    /// Source text of the marker.
    pub anchor_text: &'a str,
    /// Enclosing region supplied as the only code context.
    pub region_text: &'a str,
}

/// Input context supplied to an ephemeral freeform inline generation turn.
pub struct InlineContext<'a> {
    /// Source file used to choose the ACP workspace.
    pub file: &'a Path,
    /// Language identifier shown to the model.
    pub language: &'a str,
    /// User instruction for the insertion.
    pub prompt: &'a str,
    /// Bounded source context containing `<HARNESSD_CURSOR>`.
    pub cursor_context: &'a str,
}

/// Process-based ACP launcher.
#[derive(Debug, Clone)]
pub struct AcpClient {
    program: PathBuf,
    timeout: Duration,
}

impl AcpClient {
    /// Use `HARNESSD_ACP_COMMAND` for tests/overrides, or `codex-acp` from PATH.
    pub fn from_env() -> Self {
        Self {
            program: std::env::var_os("HARNESSD_ACP_COMMAND")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("codex-acp")),
            timeout: DEFAULT_GENERATION_TIMEOUT,
        }
    }

    /// Create an ACP client that launches the supplied executable.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            timeout: DEFAULT_GENERATION_TIMEOUT,
        }
    }

    /// Create an ACP client with a turn timeout, used by deterministic timeout tests.
    pub fn with_timeout(program: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            program: program.into(),
            timeout,
        }
    }

    /// Run one isolated prompt turn and return bounded insertion-only text.
    pub async fn generate(&self, context: &GenerationContext<'_>) -> anyhow::Result<String> {
        self.generate_with_prompt(context.file, generation_prompt(context))
            .await
    }

    /// Run one freeform insertion request and return bounded insertion-only text.
    pub async fn generate_inline(&self, context: &InlineContext<'_>) -> anyhow::Result<String> {
        self.generate_with_prompt(context.file, inline_prompt(context))
            .await
    }

    async fn generate_with_prompt(&self, file: &Path, prompt: String) -> anyhow::Result<String> {
        let cwd = file
            .parent()
            .context("source file has no parent directory for ACP session")?;
        let mut child = Command::new(&self.program)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to launch {}", self.program.display()))?;
        let mut stdin = child
            .stdin
            .take()
            .context("ACP child stdin is unavailable")?;
        let stdout = child
            .stdout
            .take()
            .context("ACP child stdout is unavailable")?;
        let mut reader = BufReader::new(stdout);

        let result = match tokio::time::timeout(
            self.timeout,
            run_turn(&mut stdin, &mut reader, cwd, &prompt),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(anyhow::anyhow!(
                "ACP generation timed out after {:?}",
                self.timeout
            )),
        };
        drop(stdin);
        drop(reader);
        terminate_child(&mut child).await;
        result.and_then(normalize_output)
    }
}

async fn run_turn<R>(
    stdin: &mut ChildStdin,
    reader: &mut BufReader<R>,
    cwd: &Path,
    prompt: &str,
) -> anyhow::Result<String>
where
    R: tokio::io::AsyncRead + Unpin,
{
    send_request(
        stdin,
        0,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": {},
            "clientInfo": {
                "name": "harnessd",
                "title": "harnessd inline completion",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
    .await?;
    wait_for_response(stdin, reader, 0, None).await?;

    send_request(
        stdin,
        1,
        "session/new",
        json!({
            "cwd": cwd.to_string_lossy(),
            "mcpServers": []
        }),
    )
    .await?;
    let session = wait_for_response(stdin, reader, 1, None).await?;
    let session_id = session
        .pointer("/result/sessionId")
        .and_then(Value::as_str)
        .context("ACP session/new response omitted sessionId")?;

    send_request(
        stdin,
        2,
        "session/prompt",
        json!({
            "sessionId": session_id,
            "prompt": [{
                "type": "text",
                "text": prompt
            }]
        }),
    )
    .await?;

    let mut output = String::new();
    wait_for_response(stdin, reader, 2, Some(&mut output)).await?;
    Ok(output)
}

fn generation_prompt(context: &GenerationContext<'_>) -> String {
    format!(
        "Generate inline completion insertion text for this {language} anchor.\n\
         Anchor kind: {kind}\n\
         Anchor text: {anchor}\n\n\
         Enclosing function/region context only:\n{region}\n\n\
         Return only code suitable for insertion at the cursor. Do not return markdown fences, \
         explanations, diffs, or file edits. Do not execute tools or read other files.",
        language = context.language,
        kind = context.anchor_kind,
        anchor = context.anchor_text,
        region = context.region_text
    )
}

fn inline_prompt(context: &InlineContext<'_>) -> String {
    format!(
        "Generate insertion text at <HARNESSD_CURSOR> for this {language} source file.\n\
         User request: {prompt}\n\n\
         Bounded source context:\n{source}\n\n\
         Return only text suitable for insertion at the cursor. Do not return markdown fences, \
         explanations, diffs, or file edits. Do not execute tools or read other files.",
        language = context.language,
        prompt = context.prompt,
        source = context.cursor_context
    )
}

async fn send_request(
    stdin: &mut ChildStdin,
    id: u64,
    method: &str,
    params: Value,
) -> anyhow::Result<()> {
    send_value(
        stdin,
        &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
    )
    .await
}

async fn send_value(stdin: &mut ChildStdin, message: &Value) -> anyhow::Result<()> {
    stdin
        .write_all(serde_json::to_string(message)?.as_bytes())
        .await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    Ok(())
}

async fn wait_for_response<R>(
    stdin: &mut ChildStdin,
    reader: &mut BufReader<R>,
    expected_id: u64,
    mut output: Option<&mut String>,
) -> anyhow::Result<Value>
where
    R: tokio::io::AsyncRead + Unpin,
{
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 {
            anyhow::bail!("ACP process exited before responding to request {expected_id}");
        }
        let message: Value =
            serde_json::from_str(line.trim()).context("ACP emitted invalid JSON-RPC")?;

        if message.get("method").and_then(Value::as_str) == Some("session/update") {
            if let Some(output) = output.as_deref_mut() {
                let update = &message["params"]["update"];
                if update["sessionUpdate"] == "agent_message_chunk"
                    && update["content"]["type"] == "text"
                    && let Some(text) = update["content"]["text"].as_str()
                {
                    output.push_str(text);
                }
                if update["sessionUpdate"] == "tool_call"
                    && matches!(
                        update["kind"].as_str(),
                        Some("edit" | "delete" | "move" | "execute")
                    )
                {
                    anyhow::bail!("ACP attempted a disallowed write or command tool call");
                }
            }
            continue;
        }

        if let Some(method) = message.get("method").and_then(Value::as_str) {
            if let Some(id) = message.get("id").cloned() {
                let result = if method == "session/request_permission" {
                    json!({"jsonrpc": "2.0", "id": id, "result": {
                        "outcome": {"outcome": "cancelled"}
                    }})
                } else {
                    json!({"jsonrpc": "2.0", "id": id, "error": {
                        "code": -32601, "message": "harnessd does not provide tools during generation"
                    }})
                };
                send_value(stdin, &result).await?;
                anyhow::bail!("ACP requested a disallowed client operation: {method}");
            }
            continue;
        }

        if message.get("id") == Some(&json!(expected_id)) {
            if let Some(error) = message.get("error") {
                anyhow::bail!("ACP request failed: {error}");
            }
            return Ok(message);
        }
    }
}

async fn terminate_child(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() {
        child.kill().await.ok();
    }
    child.wait().await.ok();
}

/// Remove one accidental code fence and enforce the autocomplete bounds.
pub fn normalize_output(output: String) -> anyhow::Result<String> {
    let mut snippet = output.trim().to_string();
    if snippet.starts_with("```")
        && snippet.ends_with("```")
        && let Some(first_newline) = snippet.find('\n')
    {
        snippet = snippet[first_newline + 1..snippet.len() - 3]
            .trim()
            .to_string();
    }
    if snippet.is_empty() {
        anyhow::bail!("ACP generated an empty suggestion");
    }
    if snippet.lines().count() > MAX_LINES {
        anyhow::bail!(
            "generated suggestion exceeds max lines ({} > {})",
            snippet.lines().count(),
            MAX_LINES
        );
    }
    if snippet.len() > MAX_BYTES {
        anyhow::bail!(
            "generated suggestion exceeds max bytes ({} > {})",
            snippet.len(),
            MAX_BYTES
        );
    }
    Ok(snippet)
}

#[cfg(test)]
mod tests {
    use super::normalize_output;
    use crate::cache::{MAX_BYTES, MAX_LINES};

    #[test]
    fn strips_one_code_fence() {
        assert_eq!(
            normalize_output("```rust\nlet x = 1;\n```".to_string()).unwrap(),
            "let x = 1;"
        );
    }

    #[test]
    fn rejects_empty_and_oversized_output() {
        assert!(normalize_output(" \n".to_string()).is_err());
        assert!(normalize_output("x\n".repeat(MAX_LINES + 1)).is_err());
        assert!(normalize_output("x".repeat(MAX_BYTES + 1)).is_err());
    }
}
