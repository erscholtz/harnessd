//! External source marks that can optionally own attached threads.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};

const LINE_PREVIEW_MAX_CHARS: usize = 160;

static MARK_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Parameters for creating an external mark.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MarkCreateParams {
    /// Workspace root.
    pub workspace: String,
    /// Source file path.
    pub file: String,
    /// Cursor byte offset in `content`.
    pub offset: usize,
    /// Live buffer content.
    pub content: String,
    /// Optional attached thread id.
    #[serde(default)]
    pub thread_id: Option<String>,
}

/// Parameters for listing external marks.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MarkListParams {
    /// Workspace root.
    pub workspace: String,
    /// Optional source file filter.
    #[serde(default)]
    pub file: Option<String>,
    /// Optional live buffer content used for reanchoring.
    #[serde(default)]
    pub content: Option<String>,
}

/// Parameters for deleting an external mark.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MarkDeleteParams {
    /// Mark id.
    pub mark_id: String,
    /// Whether an attached thread may be deleted with the mark.
    #[serde(default)]
    pub delete_attached_thread: bool,
}

/// Parameters for moving to the next or previous mark in one file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MarkStepParams {
    /// Workspace root.
    pub workspace: String,
    /// Source file path.
    pub file: String,
    /// Current 1-based cursor line.
    pub current_line: usize,
    /// Optional live buffer content used for reanchoring before selection.
    #[serde(default)]
    pub content: Option<String>,
}

/// External source mark.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct MarkAnchor {
    /// Stable harnessd mark id.
    pub mark_id: String,
    /// Workspace root.
    pub workspace: String,
    /// Source file path.
    pub file: String,
    /// Original 1-based line.
    pub original_line: usize,
    /// Current 1-based line after reanchor.
    pub current_line: usize,
    /// Original byte offset.
    pub byte_offset: usize,
    /// Hash of normalized anchored line.
    pub line_hash: String,
    /// Bounded anchored line preview.
    pub line_preview: String,
    /// Optional attached thread id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// `open`, `linked`, or `stale`.
    pub status: String,
    /// Creation timestamp as Unix seconds.
    pub created_at: u64,
    /// Update timestamp as Unix seconds.
    pub updated_at: u64,
}

/// Result of creating an external mark.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MarkCreateResult {
    /// Created mark.
    pub mark: MarkAnchor,
}

/// Result of listing external marks.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MarkListResult {
    /// Matching marks.
    pub marks: Vec<MarkAnchor>,
}

/// Result of deleting an external mark.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MarkDeleteResult {
    /// Deleted mark, if it existed.
    pub mark: Option<MarkAnchor>,
    /// Whether an attached thread was requested and deleted by the caller.
    pub deleted_thread: bool,
}

/// Result of moving to a next or previous mark.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MarkStepResult {
    /// Selected mark, if one exists.
    pub mark: Option<MarkAnchor>,
}

/// JSON-backed mark store.
#[derive(Debug, Clone)]
pub struct MarkStore {
    path: PathBuf,
}

impl MarkStore {
    /// Create a store at the supplied runtime path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Create a mark from RPC parameters.
    pub fn create(&self, params: &MarkCreateParams) -> anyhow::Result<MarkCreateResult> {
        let mark = self.create_mark(
            &params.workspace,
            &params.file,
            params.offset,
            &params.content,
            params.thread_id.clone(),
        )?;
        Ok(MarkCreateResult { mark })
    }

    /// Create a mark with explicit parts, used when creating a thread.
    pub fn create_mark(
        &self,
        workspace: &str,
        file: &str,
        offset: usize,
        content: &str,
        thread_id: Option<String>,
    ) -> anyhow::Result<MarkAnchor> {
        validate_offset(content, offset)?;
        let workspace = normalize_path(Path::new(workspace));
        let file = normalize_path(Path::new(file));
        let line = line_at_offset(content, offset);
        let line_text = line_text_at(content, line).unwrap_or_default();
        let now = unix_timestamp();
        let mark = MarkAnchor {
            mark_id: new_mark_id(),
            workspace: workspace.display().to_string(),
            file: file.display().to_string(),
            original_line: line,
            current_line: line,
            byte_offset: offset,
            line_hash: line_hash(line_text),
            line_preview: truncate(line_text.trim(), LINE_PREVIEW_MAX_CHARS),
            thread_id,
            status: "open".to_string(),
            created_at: now,
            updated_at: now,
        };

        let mut marks = self.load()?;
        marks.push(mark.clone());
        self.save(&marks)?;
        Ok(mark)
    }

    /// List marks, optionally filtered and reanchored against live content.
    pub fn list(&self, params: &MarkListParams) -> anyhow::Result<MarkListResult> {
        let workspace = normalize_path(Path::new(&params.workspace));
        let file_filter = params
            .file
            .as_deref()
            .map(|file| normalize_path(Path::new(file)));
        let mut marks = self.load()?;
        let mut changed = false;

        if let (Some(file), Some(content)) = (&file_filter, &params.content) {
            for mark in &mut marks {
                if Path::new(&mark.workspace) == workspace && Path::new(&mark.file) == file {
                    changed |= reanchor(mark, content);
                }
            }
        }
        if changed {
            self.save(&marks)?;
        }

        let marks = marks
            .into_iter()
            .filter(|mark| Path::new(&mark.workspace) == workspace)
            .filter(|mark| {
                file_filter
                    .as_ref()
                    .map(|file| Path::new(&mark.file) == file)
                    .unwrap_or(true)
            })
            .collect();
        Ok(MarkListResult { marks })
    }

    /// Delete a mark by id.
    pub fn delete(&self, params: &MarkDeleteParams) -> anyhow::Result<MarkDeleteResult> {
        let mut marks = self.load()?;
        let Some(index) = marks.iter().position(|mark| mark.mark_id == params.mark_id) else {
            return Ok(MarkDeleteResult {
                mark: None,
                deleted_thread: false,
            });
        };
        if marks[index].thread_id.is_some() && !params.delete_attached_thread {
            anyhow::bail!(
                "mark {} has an attached thread; pass delete_attached_thread to remove both",
                params.mark_id
            );
        }
        let deleted = marks.remove(index);
        self.save(&marks)?;
        Ok(MarkDeleteResult {
            deleted_thread: deleted.thread_id.is_some() && params.delete_attached_thread,
            mark: Some(deleted),
        })
    }

    /// Attach a thread id to an existing mark.
    pub fn link_thread(
        &self,
        mark_id: &str,
        thread_id: &str,
    ) -> anyhow::Result<Option<MarkAnchor>> {
        let mut marks = self.load()?;
        let Some(mark) = marks.iter_mut().find(|mark| mark.mark_id == mark_id) else {
            return Ok(None);
        };
        mark.thread_id = Some(thread_id.to_string());
        mark.status = "linked".to_string();
        mark.updated_at = unix_timestamp();
        let mark = mark.clone();
        self.save(&marks)?;
        Ok(Some(mark))
    }

    /// Clear any thread attachment from matching marks.
    pub fn unlink_thread(&self, thread_id: &str) -> anyhow::Result<Vec<MarkAnchor>> {
        let mut marks = self.load()?;
        let mut changed = Vec::new();
        for mark in &mut marks {
            if mark.thread_id.as_deref() == Some(thread_id) {
                mark.thread_id = None;
                mark.status = if mark.status == "stale" {
                    "stale".to_string()
                } else {
                    "open".to_string()
                };
                mark.updated_at = unix_timestamp();
                changed.push(mark.clone());
            }
        }
        if !changed.is_empty() {
            self.save(&marks)?;
        }
        Ok(changed)
    }

    /// Return the next mark after `current_line`, wrapping within the file.
    pub fn next(&self, params: &MarkStepParams) -> anyhow::Result<MarkStepResult> {
        self.step(params, StepDirection::Next)
    }

    /// Return the previous mark before `current_line`, wrapping within the file.
    pub fn prev(&self, params: &MarkStepParams) -> anyhow::Result<MarkStepResult> {
        self.step(params, StepDirection::Prev)
    }

    fn step(
        &self,
        params: &MarkStepParams,
        direction: StepDirection,
    ) -> anyhow::Result<MarkStepResult> {
        let list = self.list(&MarkListParams {
            workspace: params.workspace.clone(),
            file: Some(params.file.clone()),
            content: params.content.clone(),
        })?;
        let mut marks = list.marks;
        marks.sort_by_key(|mark| mark.current_line);
        let mark = match direction {
            StepDirection::Next => marks
                .iter()
                .find(|mark| mark.current_line > params.current_line)
                .or_else(|| marks.first())
                .cloned(),
            StepDirection::Prev => marks
                .iter()
                .rev()
                .find(|mark| mark.current_line < params.current_line)
                .or_else(|| marks.last())
                .cloned(),
        };
        Ok(MarkStepResult { mark })
    }

    fn load(&self) -> anyhow::Result<Vec<MarkAnchor>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", self.path.display()))
    }

    fn save(&self, marks: &[MarkAnchor]) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::write(&self.path, serde_json::to_string_pretty(marks)?)
            .with_context(|| format!("failed to write {}", self.path.display()))
    }
}

#[derive(Debug, Clone, Copy)]
enum StepDirection {
    Next,
    Prev,
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

fn line_at_offset(content: &str, offset: usize) -> usize {
    content[..offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn line_text_at(content: &str, line: usize) -> Option<&str> {
    content.lines().nth(line.saturating_sub(1))
}

fn reanchor(mark: &mut MarkAnchor, content: &str) -> bool {
    let best = content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| (line_hash(line) == mark.line_hash).then_some(index + 1))
        .min_by_key(|candidate| candidate.abs_diff(mark.current_line));
    let old_line = mark.current_line;
    let old_status = mark.status.clone();
    match best {
        Some(line) => {
            mark.current_line = line;
            if mark.status == "stale" {
                mark.status = if mark.thread_id.is_some() {
                    "linked".to_string()
                } else {
                    "open".to_string()
                };
            }
        }
        None => {
            mark.status = "stale".to_string();
        }
    }
    let changed = old_line != mark.current_line || old_status != mark.status;
    if changed {
        mark.updated_at = unix_timestamp();
    }
    changed
}

fn line_hash(line: &str) -> String {
    crate::cache::compute_hash(normalized_line(line).as_ref())
}

fn normalized_line(line: &str) -> String {
    line.trim().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn new_mark_id() -> String {
    format!(
        "mark-{}-{}-{}",
        unix_timestamp(),
        std::process::id(),
        MARK_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Store path for marks under the runtime directory.
pub fn store_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("marks.json")
}
