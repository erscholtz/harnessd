//! Scratch preview generation through read-only Codex exec.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::Deserialize;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};

use crate::rpc::{ScratchCreateParams, ScratchCreateResult};

const CONTEXT_MAX_BYTES: usize = 16 * 1024;
const PROMPT_PREVIEW_MAX_CHARS: usize = 120;
const SLUG_MAX_CHARS: usize = 48;
const MAX_LINES: usize = 400;
const MAX_BYTES: usize = 64 * 1024;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(180);

/// Process-based scratch generator.
#[derive(Debug, Clone)]
pub struct ScratchClient {
    program: PathBuf,
    timeout: Duration,
    runtime_dir: PathBuf,
}

impl ScratchClient {
    /// Use `HARNESSD_CODEX_COMMAND` for overrides, or `codex` from PATH.
    pub fn from_env(runtime_dir: PathBuf) -> Self {
        Self {
            program: std::env::var_os("HARNESSD_CODEX_COMMAND")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("codex")),
            timeout: DEFAULT_TIMEOUT,
            runtime_dir,
        }
    }

    /// Create a scratch client that launches the supplied executable.
    pub fn new(program: impl Into<PathBuf>, runtime_dir: PathBuf) -> Self {
        Self {
            program: program.into(),
            timeout: DEFAULT_TIMEOUT,
            runtime_dir,
        }
    }

    /// Create a scratch client with a turn timeout, used by deterministic tests.
    pub fn with_timeout(
        program: impl Into<PathBuf>,
        runtime_dir: PathBuf,
        timeout: Duration,
    ) -> Self {
        Self {
            program: program.into(),
            timeout,
            runtime_dir,
        }
    }

    /// Generate one scratch artifact and return the created path metadata.
    pub async fn create(
        &self,
        params: &ScratchCreateParams,
    ) -> anyhow::Result<ScratchCreateResult> {
        validate_params(params)?;
        let prompt = scratch_prompt(params);
        let generated = self.run_codex(params, &prompt).await?;
        let artifact = build_artifact(params, &generated)?;
        write_artifact(params, &artifact)
    }

    async fn run_codex(
        &self,
        params: &ScratchCreateParams,
        prompt: &str,
    ) -> anyhow::Result<GeneratedScratch> {
        std::fs::create_dir_all(&self.runtime_dir)?;
        let schema_path = self.runtime_dir.join("scratch-output.schema.json");
        let output_path = self.runtime_dir.join(format!(
            "scratch-output-{}-{}.json",
            std::process::id(),
            unix_millis()
        ));
        std::fs::write(&schema_path, scratch_schema())?;
        std::fs::remove_file(&output_path).ok();

        let model = crate::models::normalize_model(params.model.clone())?;
        let mut command = Command::new(&self.program);
        if let Some(model) = model {
            command.arg("--model").arg(model);
        }
        if let Some(effort) =
            crate::models::normalize_reasoning_effort(params.reasoning_effort.clone())?
        {
            command
                .arg("-c")
                .arg(crate::models::reasoning_effort_config_arg(&effort));
        }
        let mut child = command
            .arg("--ask-for-approval")
            .arg("never")
            .arg("--sandbox")
            .arg("read-only")
            .arg("exec")
            .arg("--cd")
            .arg(&params.workspace)
            .arg("--output-schema")
            .arg(&schema_path)
            .arg("--output-last-message")
            .arg(&output_path)
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to launch {}", self.program.display()))?;

        let mut stdin = child
            .stdin
            .take()
            .context("Codex child stdin is unavailable")?;
        stdin.write_all(prompt.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        drop(stdin);

        let status = match tokio::time::timeout(self.timeout, child.wait()).await {
            Ok(result) => result.context("failed to wait for Codex process")?,
            Err(_) => {
                terminate_child(&mut child).await;
                anyhow::bail!(
                    "Codex scratch generation timed out after {:?}",
                    self.timeout
                );
            }
        };
        if !status.success() {
            anyhow::bail!("Codex scratch generation failed with status {status}");
        }

        let output = tokio::fs::read_to_string(&output_path)
            .await
            .with_context(|| {
                format!(
                    "Codex did not write structured scratch output at {}",
                    output_path.display()
                )
            })?;
        let generated: GeneratedScratch =
            serde_json::from_str(&output).context("Codex returned malformed scratch JSON")?;
        if generated.body.trim().is_empty() {
            anyhow::bail!("Codex returned an empty scratch body");
        }
        Ok(generated)
    }
}

/// Create a scratch preview through the daemon-owned client.
pub async fn create(
    client: &ScratchClient,
    params: &ScratchCreateParams,
) -> anyhow::Result<ScratchCreateResult> {
    client.create(params).await
}

#[derive(Debug, Deserialize)]
struct GeneratedScratch {
    title: String,
    body: String,
}

struct Artifact {
    path: PathBuf,
    relative_path: PathBuf,
    text: String,
}

fn validate_params(params: &ScratchCreateParams) -> anyhow::Result<()> {
    if params.prompt.trim().is_empty() {
        anyhow::bail!("scratch prompt must not be empty");
    }
    if params.content.is_empty() {
        anyhow::bail!("scratch requires live buffer contents on stdin");
    }
    validate_offset(&params.content, params.offset)?;
    validate_selection(
        &params.content,
        params.selection_start,
        params.selection_end,
    )?;
    Ok(())
}

fn validate_offset(content: &str, offset: usize) -> anyhow::Result<()> {
    if offset > content.len() {
        anyhow::bail!("cursor offset is outside buffer content");
    }
    if !content.is_char_boundary(offset) {
        anyhow::bail!("cursor offset is not a UTF-8 character boundary");
    }
    Ok(())
}

fn validate_selection(
    content: &str,
    start: Option<usize>,
    end: Option<usize>,
) -> anyhow::Result<()> {
    match (start, end) {
        (Some(start), Some(end)) => {
            if start > end || end > content.len() {
                anyhow::bail!("selection range is outside buffer content");
            }
            if !content.is_char_boundary(start) || !content.is_char_boundary(end) {
                anyhow::bail!("selection range is not on UTF-8 character boundaries");
            }
        }
        (None, None) => {}
        _ => anyhow::bail!("selection_start and selection_end must be provided together"),
    }
    Ok(())
}

fn scratch_prompt(params: &ScratchCreateParams) -> String {
    let line = line_at_offset(&params.content, params.offset);
    let context = match (params.selection_start, params.selection_end) {
        (Some(start), Some(end)) => {
            format!(
                "Selected code:\n{}",
                cap_context(&params.content[start..end])
            )
        }
        _ => format!(
            "Nearby live buffer context:\n{}",
            cap_context(&nearby_lines(&params.content, line, 80))
        ),
    };

    format!(
        "You are creating one scratch preview file for a developer.\n\
         Workspace: {workspace}\n\
         Source file: {file}\n\
         Cursor line: {line}\n\
         User request: {ask}\n\n\
         You may inspect the repository for context, but you must not edit files, run write commands, \
         or rely on network access. Return only JSON matching the provided schema. The `body` field \
         must be the complete contents of one useful preview/MVP/example file, with no markdown fences.\n\n\
         {context}\n",
        workspace = params.workspace,
        file = params.file,
        line = line,
        ask = params.prompt.trim(),
        context = context
    )
}

fn build_artifact(
    params: &ScratchCreateParams,
    generated: &GeneratedScratch,
) -> anyhow::Result<Artifact> {
    let mut body = generated.body.trim().to_string();
    if body.starts_with("```")
        && body.ends_with("```")
        && let Some(first_newline) = body.find('\n')
    {
        body = body[first_newline + 1..body.len() - 3].trim().to_string();
    }

    let header = header_for(params, &generated.title);
    let text = format!("{header}{body}\n");
    enforce_caps(&text)?;

    let workspace = Path::new(&params.workspace);
    let scratch_dir = workspace.join("scratch").join("harnessd");
    let extension = Path::new(&params.file)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty())
        .unwrap_or("md");
    let basename = format!(
        "{}-{}",
        timestamp_for_filename(unix_timestamp()),
        slug(&params.prompt)
    );
    let path = unique_path(&scratch_dir, &basename, extension);
    let relative_path = path.strip_prefix(workspace).unwrap_or(&path).to_path_buf();

    Ok(Artifact {
        path,
        relative_path,
        text,
    })
}

fn write_artifact(
    params: &ScratchCreateParams,
    artifact: &Artifact,
) -> anyhow::Result<ScratchCreateResult> {
    if let Some(parent) = artifact.path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&artifact.path)
        .with_context(|| format!("failed to create {}", artifact.path.display()))?;
    file.write_all(artifact.text.as_bytes())?;

    let created_at = unix_timestamp();
    let result = ScratchCreateResult {
        path: artifact.path.display().to_string(),
        relative_path: artifact.relative_path.display().to_string(),
        bytes: artifact.text.len(),
        lines: artifact.text.lines().count(),
        created_at,
        source_file: params.file.clone(),
        prompt_preview: truncate(params.prompt.trim(), PROMPT_PREVIEW_MAX_CHARS),
    };
    tracing::info!(
        path = %result.path,
        bytes = result.bytes,
        lines = result.lines,
        "scratch preview written"
    );
    Ok(result)
}

fn enforce_caps(text: &str) -> anyhow::Result<()> {
    let lines = text.lines().count();
    if lines > MAX_LINES {
        anyhow::bail!("scratch output exceeds max lines ({lines} > {MAX_LINES})");
    }
    if text.len() > MAX_BYTES {
        anyhow::bail!(
            "scratch output exceeds max bytes ({} > {MAX_BYTES})",
            text.len()
        );
    }
    Ok(())
}

fn header_for(params: &ScratchCreateParams, title: &str) -> String {
    let prefix = comment_prefix(Path::new(&params.file));
    let mut lines = vec![
        format!("{prefix} harnessd scratch preview"),
        format!("{prefix} source: {}", params.file),
        format!(
            "{prefix} prompt: {}",
            truncate(params.prompt.trim(), PROMPT_PREVIEW_MAX_CHARS)
        ),
    ];
    if !title.trim().is_empty() {
        lines.push(format!("{prefix} title: {}", truncate(title.trim(), 80)));
    }
    lines.join("\n") + "\n\n"
}

fn comment_prefix(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("py") | Some("sh") | Some("rb") | Some("toml") | Some("yaml") | Some("yml")
        | Some("md") => "#",
        _ => "//",
    }
}

fn unique_path(dir: &Path, basename: &str, extension: &str) -> PathBuf {
    let first = dir.join(format!("{basename}.{extension}"));
    if !first.exists() {
        return first;
    }
    for index in 2usize.. {
        let candidate = dir.join(format!("{basename}-{index}.{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn slug(prompt: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in prompt.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
        if out.len() >= SLUG_MAX_CHARS {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "scratch".to_string()
    } else {
        out
    }
}

fn line_at_offset(content: &str, offset: usize) -> usize {
    content[..offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn nearby_lines(content: &str, line: usize, radius: usize) -> String {
    let start = line.saturating_sub(radius).max(1);
    let end = line.saturating_add(radius);
    content
        .lines()
        .enumerate()
        .filter_map(|(index, text)| {
            let number = index + 1;
            (number >= start && number <= end).then(|| format!("{number}: {text}"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn cap_context(context: &str) -> String {
    if context.len() <= CONTEXT_MAX_BYTES {
        return context.to_string();
    }
    let mut end = CONTEXT_MAX_BYTES;
    while end > 0 && !context.is_char_boundary(end) {
        end -= 1;
    }
    context[..end].to_string()
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut output: String = value.chars().take(max_chars).collect();
    if value.chars().count() > max_chars {
        output.push_str("...");
    }
    output
}

fn scratch_schema() -> String {
    serde_json::to_string_pretty(&json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["title", "body"],
        "properties": {
            "title": {
                "type": "string",
                "minLength": 1,
                "maxLength": 120
            },
            "body": {
                "type": "string",
                "minLength": 1
            }
        }
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

async fn terminate_child(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() {
        child.kill().await.ok();
    }
    child.wait().await.ok();
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn timestamp_for_filename(timestamp: u64) -> String {
    let days = (timestamp / 86_400) as i64;
    let seconds = timestamp % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds / 3_600;
    let minute = (seconds % 3_600) / 60;
    let second = seconds % 60;
    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

// Howard Hinnant's civil calendar conversion, using Unix epoch days.
fn civil_from_days(days: i64) -> (i64, u64, u64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m as u64, d as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_offsets_and_selection() {
        let base = ScratchCreateParams {
            workspace: "/tmp".to_string(),
            file: "/tmp/demo.rs".to_string(),
            offset: 0,
            content: "fn main() {}\n".to_string(),
            prompt: "demo".to_string(),
            selection_start: None,
            selection_end: None,
            model: None,
            reasoning_effort: None,
        };
        assert!(validate_params(&base).is_ok());

        let mut empty_prompt = base.clone();
        empty_prompt.prompt = " ".to_string();
        assert!(validate_params(&empty_prompt).is_err());

        let mut bad_offset = base.clone();
        bad_offset.offset = 99;
        assert!(validate_params(&bad_offset).is_err());

        let mut bad_selection = base.clone();
        bad_selection.selection_start = Some(5);
        bad_selection.selection_end = Some(3);
        assert!(validate_params(&bad_selection).is_err());

        let mut non_boundary = base;
        non_boundary.content = "é".to_string();
        non_boundary.offset = 1;
        assert!(validate_params(&non_boundary).is_err());
    }

    #[test]
    fn timestamp_uses_utc_filename_format() {
        assert_eq!(timestamp_for_filename(0), "19700101-000000");
        assert_eq!(timestamp_for_filename(1_780_574_462), "20260604-120102");
    }
}
