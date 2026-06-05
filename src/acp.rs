//! Minimal stdio ACP client used for explicit inline generation requests.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use crate::cache::{MAX_BYTES, MAX_LINES};

const DEFAULT_GENERATION_TIMEOUT: Duration = Duration::from_secs(120);
const ANCHOR_REASONING_EFFORT: &str = "high";
const INLINE_REASONING_EFFORT: &str = "low";

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
    reusable_sessions: Arc<Mutex<HashMap<ReusableSessionKey, ReusableSession>>>,
}

impl AcpClient {
    /// Use `HARNESSD_ACP_COMMAND` for tests/overrides, or `codex-acp` from PATH.
    pub fn from_env() -> Self {
        Self {
            program: std::env::var_os("HARNESSD_ACP_COMMAND")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("codex-acp")),
            timeout: DEFAULT_GENERATION_TIMEOUT,
            reusable_sessions: Arc::default(),
        }
    }

    /// Create an ACP client that launches the supplied executable.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            timeout: DEFAULT_GENERATION_TIMEOUT,
            reusable_sessions: Arc::default(),
        }
    }

    /// Create an ACP client with a turn timeout, used by deterministic timeout tests.
    pub fn with_timeout(program: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            program: program.into(),
            timeout,
            reusable_sessions: Arc::default(),
        }
    }

    /// Run one isolated prompt turn and return bounded insertion-only text.
    pub async fn generate(&self, context: &GenerationContext<'_>) -> anyhow::Result<String> {
        self.generate_with_prompt(
            context.file,
            generation_prompt(context),
            ANCHOR_REASONING_EFFORT,
        )
        .await
    }

    /// Run one freeform insertion request and return bounded insertion-only text.
    pub async fn generate_inline(
        &self,
        context: &InlineContext<'_>,
        model: Option<&str>,
        reasoning_effort: Option<&str>,
    ) -> anyhow::Result<String> {
        let reasoning_effort = reasoning_effort.unwrap_or(INLINE_REASONING_EFFORT);
        self.generate_with_reusable_prompt(
            context.file,
            inline_prompt(context),
            reasoning_effort,
            model,
        )
        .await
    }

    /// Start the reusable inline ACP session for this file's workspace before a prompt arrives.
    pub async fn prepare_inline(
        &self,
        file: &Path,
        model: Option<&str>,
        reasoning_effort: Option<&str>,
    ) -> anyhow::Result<()> {
        let reasoning_effort = reasoning_effort.unwrap_or(INLINE_REASONING_EFFORT);
        self.prepare_reusable_session(file, reasoning_effort, model)
            .await
    }

    /// Terminate reusable ACP sessions owned by this client.
    pub async fn shutdown(&self) {
        let mut sessions = self.reusable_sessions.lock().await;
        for (_, mut session) in sessions.drain() {
            session.terminate().await;
        }
    }

    async fn generate_with_prompt(
        &self,
        file: &Path,
        prompt: String,
        reasoning_effort: &str,
    ) -> anyhow::Result<String> {
        let cwd = file
            .parent()
            .context("source file has no parent directory for ACP session")?;
        let mut command = Command::new(&self.program);
        command
            .arg("-c")
            .arg(reasoning_config_arg(reasoning_effort))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command
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

    async fn generate_with_reusable_prompt(
        &self,
        file: &Path,
        prompt: String,
        reasoning_effort: &str,
        model: Option<&str>,
    ) -> anyhow::Result<String> {
        let cwd = file
            .parent()
            .context("source file has no parent directory for ACP session")?;
        let model = crate::models::normalize_model(model.map(str::to_string))?;
        let key = ReusableSessionKey::new(cwd, reasoning_effort, model.as_deref());
        let mut sessions = self.reusable_sessions.lock().await;
        let result = match tokio::time::timeout(
            self.timeout,
            run_reusable_turn(
                &mut sessions,
                &self.program,
                cwd,
                reasoning_effort,
                model.as_deref(),
                &prompt,
            ),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(anyhow::anyhow!(
                "ACP generation timed out after {:?}",
                self.timeout
            )),
        };

        if result.is_err() {
            if let Some(mut session) = sessions.remove(&key) {
                session.terminate().await;
            }
        } else if let Some(session) = sessions.get_mut(&key)
            && session.exited()
        {
            sessions.remove(&key);
        }

        result.and_then(normalize_output)
    }

    async fn prepare_reusable_session(
        &self,
        file: &Path,
        reasoning_effort: &str,
        model: Option<&str>,
    ) -> anyhow::Result<()> {
        let cwd = file
            .parent()
            .context("source file has no parent directory for ACP session")?;
        let model = crate::models::normalize_model(model.map(str::to_string))?;
        let key = ReusableSessionKey::new(cwd, reasoning_effort, model.as_deref());
        let mut sessions = self.reusable_sessions.lock().await;
        let result = match tokio::time::timeout(
            self.timeout,
            ensure_reusable_session(
                &mut sessions,
                &self.program,
                cwd,
                reasoning_effort,
                model.as_deref(),
            ),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(anyhow::anyhow!(
                "ACP generation timed out after {:?}",
                self.timeout
            )),
        };

        if result.is_err() {
            if let Some(mut session) = sessions.remove(&key) {
                session.terminate().await;
            }
        }
        result.map(|_| ())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReusableSessionKey {
    cwd: PathBuf,
    reasoning_effort: String,
    model: Option<String>,
}

impl ReusableSessionKey {
    fn new(cwd: &Path, reasoning_effort: &str, model: Option<&str>) -> Self {
        Self {
            cwd: cwd.to_path_buf(),
            reasoning_effort: reasoning_effort.to_string(),
            model: model.map(str::to_string),
        }
    }
}

#[derive(Debug)]
struct ReusableSession {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    session_id: String,
    next_id: u64,
}

impl ReusableSession {
    async fn start(
        program: &Path,
        cwd: &Path,
        reasoning_effort: &str,
        model: Option<&str>,
    ) -> anyhow::Result<Self> {
        let mut command = Command::new(program);
        command
            .arg("-c")
            .arg(reasoning_config_arg(reasoning_effort));
        if let Some(model) = model {
            command
                .arg("-c")
                .arg(crate::models::model_config_arg(model));
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to launch {}", program.display()))?;
        let setup = async {
            let mut stdin = child
                .stdin
                .take()
                .context("ACP child stdin is unavailable")?;
            let stdout = child
                .stdout
                .take()
                .context("ACP child stdout is unavailable")?;
            let mut reader = BufReader::new(stdout);

            send_request(
                &mut stdin,
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
            wait_for_response(&mut stdin, &mut reader, 0, None).await?;

            send_request(
                &mut stdin,
                1,
                "session/new",
                json!({
                    "cwd": cwd.to_string_lossy(),
                    "mcpServers": []
                }),
            )
            .await?;
            let session = wait_for_response(&mut stdin, &mut reader, 1, None).await?;
            let session_id = session
                .pointer("/result/sessionId")
                .and_then(Value::as_str)
                .context("ACP session/new response omitted sessionId")?
                .to_string();

            Ok::<_, anyhow::Error>((stdin, reader, session_id))
        }
        .await;

        let (stdin, reader, session_id) = match setup {
            Ok(session) => session,
            Err(error) => {
                terminate_child(&mut child).await;
                return Err(error);
            }
        };

        Ok(Self {
            child,
            stdin,
            reader,
            session_id,
            next_id: 2,
        })
    }

    async fn prompt(&mut self, prompt: &str) -> anyhow::Result<String> {
        let id = self.next_id;
        self.next_id += 1;
        send_request(
            &mut self.stdin,
            id,
            "session/prompt",
            json!({
                "sessionId": self.session_id,
                "prompt": [{
                    "type": "text",
                    "text": prompt
                }]
            }),
        )
        .await?;

        let mut output = String::new();
        wait_for_response(&mut self.stdin, &mut self.reader, id, Some(&mut output)).await?;
        Ok(output)
    }

    fn exited(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_some()
    }

    async fn terminate(&mut self) {
        terminate_child(&mut self.child).await;
    }
}

async fn run_reusable_turn(
    sessions: &mut HashMap<ReusableSessionKey, ReusableSession>,
    program: &Path,
    cwd: &Path,
    reasoning_effort: &str,
    model: Option<&str>,
    prompt: &str,
) -> anyhow::Result<String> {
    let key = ensure_reusable_session(sessions, program, cwd, reasoning_effort, model).await?;
    sessions
        .get_mut(&key)
        .context("reusable ACP session was not created")?
        .prompt(prompt)
        .await
}

async fn ensure_reusable_session(
    sessions: &mut HashMap<ReusableSessionKey, ReusableSession>,
    program: &Path,
    cwd: &Path,
    reasoning_effort: &str,
    model: Option<&str>,
) -> anyhow::Result<ReusableSessionKey> {
    let key = ReusableSessionKey::new(cwd, reasoning_effort, model);
    let should_start = match sessions.get_mut(&key) {
        Some(session) => session.exited(),
        None => true,
    };
    if should_start {
        sessions.remove(&key);
        sessions.insert(
            key.clone(),
            ReusableSession::start(program, cwd, reasoning_effort, model).await?,
        );
    }
    Ok(key)
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
         explanations, diffs, or file edits. Prefer the smallest valid insertion; for completion \
         requests, return one line unless correctness requires more. Do not execute tools or read \
         other files.",
        language = context.language,
        prompt = context.prompt,
        source = context.cursor_context
    )
}

fn reasoning_config_arg(effort: &str) -> String {
    crate::models::reasoning_effort_config_arg(effort)
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
    use super::{normalize_output, reasoning_config_arg};
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

    #[test]
    fn reasoning_effort_is_passed_as_codex_config_override() {
        assert_eq!(
            reasoning_config_arg("low"),
            "model_reasoning_effort=\"low\""
        );
        assert_eq!(
            reasoning_config_arg("high"),
            "model_reasoning_effort=\"high\""
        );
    }
}
