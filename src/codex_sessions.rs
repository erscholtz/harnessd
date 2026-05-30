//! Scanner for saved Codex CLI sessions under `~/.codex/sessions`.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const PREVIEW_MAX_CHARS: usize = 160;

/// Parameters for listing Codex sessions.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CodexSessionsParams {
    /// Workspace used for project-first ordering.
    pub workspace: String,
    /// Include sessions from all workspaces before project filtering.
    #[serde(default)]
    pub all: bool,
    /// Maximum number of sessions to return.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// One saved Codex session summary.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CodexSessionInfo {
    /// Codex session UUID.
    pub id: String,
    /// Absolute JSONL path.
    pub path: String,
    /// Session working directory from `session_meta`.
    pub cwd: String,
    /// Session timestamp from `session_meta`.
    pub timestamp: String,
    /// Originating client, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub originator: Option<String>,
    /// Source integration, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Codex CLI version, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_version: Option<String>,
    /// Model provider, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_provider: Option<String>,
    /// Bounded first user-message preview.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// File modification timestamp as Unix seconds.
    pub modified_at: u64,
    /// Whether this session cwd matches the requested workspace.
    pub project_match: bool,
}

/// Result for a Codex session listing.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CodexSessionsResult {
    /// Returned session summaries.
    pub sessions: Vec<CodexSessionInfo>,
}

/// Locate Codex home from `CODEX_HOME` or `~/.codex`.
pub fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".codex")
        })
}

/// List saved Codex sessions using the real Codex on-disk session store.
pub fn list_sessions(params: &CodexSessionsParams) -> anyhow::Result<CodexSessionsResult> {
    list_sessions_from_home(&codex_home(), params)
}

/// List saved Codex sessions from an explicit home, used by tests.
pub fn list_sessions_from_home(
    home: &Path,
    params: &CodexSessionsParams,
) -> anyhow::Result<CodexSessionsResult> {
    let workspace = normalize_path(Path::new(&params.workspace));
    let mut sessions = Vec::new();
    collect_jsonl_files(&home.join("sessions"), &mut sessions)?;

    let mut sessions: Vec<CodexSessionInfo> = sessions
        .into_iter()
        .filter_map(|path| parse_session_file(&path, &workspace).ok().flatten())
        .filter(|session| params.all || session.project_match)
        .collect();

    sessions.sort_by(|left, right| {
        right
            .project_match
            .cmp(&left.project_match)
            .then_with(|| right.modified_at.cmp(&left.modified_at))
            .then_with(|| right.timestamp.cmp(&left.timestamp))
    });
    sessions.truncate(params.limit.unwrap_or(50));
    Ok(CodexSessionsResult { sessions })
}

fn collect_jsonl_files(root: &Path, files: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_jsonl_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
    Ok(())
}

fn parse_session_file(path: &Path, workspace: &Path) -> anyhow::Result<Option<CodexSessionInfo>> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut meta = None;
    let mut preview = None;

    for line in reader.lines().take(256) {
        let line = line?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("session_meta") => meta = value.get("payload").cloned(),
            Some("user_message") | Some("response_item") if preview.is_none() => {
                preview = first_user_preview(&value);
            }
            _ => {}
        }
        if meta.is_some() && preview.is_some() {
            break;
        }
    }

    let Some(meta) = meta else {
        return Ok(None);
    };
    let id = meta.get("id").and_then(Value::as_str).unwrap_or_default();
    let cwd = meta.get("cwd").and_then(Value::as_str).unwrap_or_default();
    if id.is_empty() || cwd.is_empty() {
        return Ok(None);
    }
    let normalized_cwd = normalize_path(Path::new(cwd));
    let modified_at = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    Ok(Some(CodexSessionInfo {
        id: id.to_string(),
        path: path.display().to_string(),
        cwd: normalized_cwd.display().to_string(),
        timestamp: meta
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        originator: string_field(&meta, "originator"),
        source: string_field(&meta, "source"),
        cli_version: string_field(&meta, "cli_version"),
        model_provider: string_field(&meta, "model_provider"),
        preview,
        modified_at,
        project_match: same_path(&normalized_cwd, workspace),
    }))
}

fn first_user_preview(value: &Value) -> Option<String> {
    let text = match value.get("type").and_then(Value::as_str) {
        Some("user_message") => value
            .pointer("/payload/message")
            .or_else(|| value.pointer("/payload/text"))
            .and_then(Value::as_str)
            .map(str::to_string),
        Some("response_item") => {
            if value.pointer("/payload/type").and_then(Value::as_str) != Some("message")
                || value.pointer("/payload/role").and_then(Value::as_str) != Some("user")
            {
                return None;
            }
            value
                .pointer("/payload/content")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items.iter().find_map(|item| {
                        item.get("text")
                            .or_else(|| item.get("input_text"))
                            .and_then(Value::as_str)
                    })
                })
                .map(str::to_string)
        }
        _ => None,
    }?;
    Some(truncate_preview(&text))
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn truncate_preview(value: &str) -> String {
    let mut preview: String = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(PREVIEW_MAX_CHARS)
        .collect();
    if value.chars().count() > PREVIEW_MAX_CHARS {
        preview.push_str("...");
    }
    preview
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn same_path(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "harnessd_codex_sessions_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scans_project_first_and_ignores_bad_lines() {
        let home = temp_dir();
        let project = temp_dir();
        let other = temp_dir();
        let day = home.join("sessions/2026/05/29");
        fs::create_dir_all(&day).unwrap();
        fs::write(
            day.join("project.jsonl"),
            format!(
                "not json\n{}\n{}\n",
                serde_json::json!({"type":"session_meta","payload":{"id":"project","cwd":project,"timestamp":"2026-05-29T00:00:00Z"}}),
                serde_json::json!({"type":"user_message","payload":{"message":"hello from project session with a long enough prompt"}})
            ),
        )
        .unwrap();
        fs::write(
            day.join("other.jsonl"),
            format!(
                "{}\n",
                serde_json::json!({"type":"session_meta","payload":{"id":"other","cwd":other,"timestamp":"2026-05-29T00:00:01Z"}})
            ),
        )
        .unwrap();

        let result = list_sessions_from_home(
            &home,
            &CodexSessionsParams {
                workspace: project.display().to_string(),
                all: true,
                limit: Some(10),
            },
        )
        .unwrap();
        assert_eq!(result.sessions[0].id, "project");
        assert!(result.sessions[0].project_match);
        assert!(
            result.sessions[0]
                .preview
                .as_ref()
                .unwrap()
                .contains("hello")
        );

        let filtered = list_sessions_from_home(
            &home,
            &CodexSessionsParams {
                workspace: project.display().to_string(),
                all: false,
                limit: Some(10),
            },
        )
        .unwrap();
        assert_eq!(filtered.sessions.len(), 1);
        assert_eq!(filtered.sessions[0].id, "project");

        fs::remove_dir_all(home).ok();
        fs::remove_dir_all(project).ok();
        fs::remove_dir_all(other).ok();
    }
}
