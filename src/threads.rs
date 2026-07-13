//! Persistent source-line anchors for Codex-backed ask threads.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const PROMPT_PREVIEW_MAX_CHARS: usize = 120;
const LINE_PREVIEW_MAX_CHARS: usize = 160;

static THREAD_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Parameters for creating an anchored thread.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThreadCreateParams {
    /// Workspace root.
    pub workspace: String,
    /// Source file path.
    pub file: String,
    /// Cursor byte offset in `content`.
    pub offset: usize,
    /// Live buffer content.
    pub content: String,
    /// User prompt.
    pub prompt: String,
    /// Optional selected start byte.
    #[serde(default)]
    pub selection_start: Option<usize>,
    /// Optional selected end byte.
    #[serde(default)]
    pub selection_end: Option<usize>,
    /// Optional model override for the launched Codex thread.
    #[serde(default)]
    pub model: Option<String>,
    /// Optional reasoning effort override for the launched Codex thread.
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

/// Parameters for listing anchored threads.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThreadListParams {
    /// Workspace root.
    pub workspace: String,
    /// Optional source file filter.
    #[serde(default)]
    pub file: Option<String>,
    /// Optional live buffer content used for reanchoring.
    #[serde(default)]
    pub content: Option<String>,
}

/// Parameters for linking a thread to a Codex session.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThreadLinkParams {
    /// Thread id.
    pub thread_id: String,
    /// Codex session UUID.
    pub codex_session_id: String,
    /// Optional JSONL path.
    #[serde(default)]
    pub codex_session_path: Option<String>,
}

/// Parameters for resolving a newly launched Codex session.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThreadResolveParams {
    /// Thread id.
    pub thread_id: String,
    /// Workspace root.
    pub workspace: String,
    /// Only consider Codex sessions modified after this Unix timestamp.
    pub started_after_unix: u64,
}

/// Parameters for attaching an existing Codex session to the current line.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThreadAttachParams {
    /// Workspace root.
    pub workspace: String,
    /// Source file path.
    pub file: String,
    /// Cursor byte offset in `content`.
    pub offset: usize,
    /// Live buffer content.
    pub content: String,
    /// Codex session UUID to attach.
    pub codex_session_id: String,
}

/// Parameters for deleting a thread and its scratch artifacts.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThreadDeleteParams {
    /// Thread id.
    pub thread_id: String,
}

/// A generated example artifact linked to a thread.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ThreadExample {
    /// Stable harnessd example id.
    pub example_id: String,
    /// Parent thread id.
    pub thread_id: String,
    /// Display title for the example.
    pub title: String,
    /// Absolute artifact path.
    pub path: String,
    /// Workspace-relative artifact path.
    pub relative_path: String,
    /// Full user prompt used to create the example.
    pub prompt: String,
    /// Bounded prompt preview.
    pub prompt_preview: String,
    /// Absolute source file path used as context.
    pub source_file: String,
    /// Number of bytes written.
    pub bytes: usize,
    /// Number of lines written.
    pub lines: usize,
    /// Creation timestamp as Unix seconds.
    pub created_at: u64,
}

/// Persistent thread anchor.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ThreadAnchor {
    /// Stable harnessd thread id.
    pub thread_id: String,
    /// Optional external mark id this thread is attached to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mark_id: Option<String>,
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
    /// Bounded prompt preview.
    pub prompt_preview: String,
    /// Full user prompt used for thread creation.
    pub prompt: String,
    /// Optional model override used when the thread was created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional reasoning effort override used when the thread was created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Optional Codex session id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_session_id: Option<String>,
    /// Optional Codex session JSONL path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_session_path: Option<String>,
    /// Example artifacts generated for this thread.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<ThreadExample>,
    /// `open`, `linked`, or `stale`.
    pub status: String,
    /// Creation timestamp as Unix seconds.
    pub created_at: u64,
    /// Update timestamp as Unix seconds.
    pub updated_at: u64,
}

/// Command launch specification returned to thin clients.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ThreadLaunch {
    /// Command argv.
    pub argv: Vec<String>,
    /// Working directory.
    pub cwd: String,
    /// Unix timestamp immediately before launch planning.
    pub started_after_unix: u64,
    /// Optional model override used for launch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional reasoning effort override used for launch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

/// Result of creating a thread.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThreadCreateResult {
    /// Created or reused thread.
    pub thread: ThreadAnchor,
    /// Launch command for Codex.
    pub launch: ThreadLaunch,
}

/// Result of listing threads.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThreadListResult {
    /// Thread anchors.
    pub threads: Vec<ThreadAnchor>,
}

/// Result of linking a thread.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThreadLinkResult {
    /// Updated thread.
    pub thread: ThreadAnchor,
}

/// Result of creating and linking a thread example.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThreadExampleCreateResult {
    /// Updated thread.
    pub thread: ThreadAnchor,
    /// Linked example artifact.
    pub example: ThreadExample,
}

/// Result of resolving a session link.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThreadResolveResult {
    /// Updated thread.
    pub thread: Option<ThreadAnchor>,
    /// Whether a matching Codex session was found.
    pub resolved: bool,
}

/// Result of deleting a thread.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThreadDeleteResult {
    /// Deleted thread, if it existed.
    pub thread: Option<ThreadAnchor>,
}

/// JSON-backed thread store.
#[derive(Debug, Clone)]
pub struct ThreadStore {
    path: PathBuf,
}

impl ThreadStore {
    /// Create a store at the supplied runtime path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Create an anchored thread.
    pub fn create(&self, params: &ThreadCreateParams) -> anyhow::Result<ThreadCreateResult> {
        self.create_with_mark(params, None)
    }

    /// Create an anchored thread attached to an optional external mark.
    pub fn create_with_mark(
        &self,
        params: &ThreadCreateParams,
        mark_id: Option<String>,
    ) -> anyhow::Result<ThreadCreateResult> {
        validate_offset(&params.content, params.offset)?;
        if params.prompt.trim().is_empty() {
            anyhow::bail!("thread prompt must not be empty");
        }
        let workspace = normalize_path(Path::new(&params.workspace));
        let file = normalize_path(Path::new(&params.file));
        let line = line_at_offset(&params.content, params.offset);
        let line_text = line_text_at(&params.content, line).unwrap_or_default();
        let now = unix_timestamp();
        let mut threads = self.load()?;
        let model = crate::models::normalize_model(params.model.clone())?;
        let reasoning_effort =
            crate::models::normalize_reasoning_effort(params.reasoning_effort.clone())?;
        let thread = ThreadAnchor {
            thread_id: new_thread_id(),
            mark_id,
            workspace: workspace.display().to_string(),
            file: file.display().to_string(),
            original_line: line,
            current_line: line,
            byte_offset: params.offset,
            line_hash: line_hash(line_text),
            line_preview: truncate(line_text.trim(), LINE_PREVIEW_MAX_CHARS),
            prompt_preview: truncate(params.prompt.trim(), PROMPT_PREVIEW_MAX_CHARS),
            prompt: params.prompt.trim().to_string(),
            model,
            reasoning_effort,
            codex_session_id: None,
            codex_session_path: None,
            examples: Vec::new(),
            status: "open".to_string(),
            created_at: now,
            updated_at: now,
        };
        threads.push(thread.clone());
        self.save(&threads)?;
        let launch = launch_for(
            &thread,
            &params.content,
            params.selection_start,
            params.selection_end,
        );
        Ok(ThreadCreateResult { thread, launch })
    }

    /// Link a generated scratch artifact to an existing thread.
    pub fn add_example(
        &self,
        thread_id: &str,
        scratch: &crate::rpc::ScratchCreateResult,
        prompt: &str,
    ) -> anyhow::Result<ThreadExampleCreateResult> {
        let mut threads = self.load()?;
        let Some(thread) = threads
            .iter_mut()
            .find(|thread| thread.thread_id == thread_id)
        else {
            anyhow::bail!("thread not found: {thread_id}");
        };
        let prompt = prompt.trim();
        let example = ThreadExample {
            example_id: new_example_id(),
            thread_id: thread_id.to_string(),
            title: title_for_example(prompt, &scratch.relative_path),
            path: scratch.path.clone(),
            relative_path: scratch.relative_path.clone(),
            prompt: prompt.to_string(),
            prompt_preview: truncate(prompt, PROMPT_PREVIEW_MAX_CHARS),
            source_file: scratch.source_file.clone(),
            bytes: scratch.bytes,
            lines: scratch.lines,
            created_at: scratch.created_at,
        };
        thread.examples.push(example.clone());
        thread.updated_at = unix_timestamp();
        let thread = thread.clone();
        self.save(&threads)?;
        Ok(ThreadExampleCreateResult { thread, example })
    }

    /// List threads, optionally filtered and reanchored against live content.
    pub fn list(&self, params: &ThreadListParams) -> anyhow::Result<ThreadListResult> {
        let workspace = normalize_path(Path::new(&params.workspace));
        let file = params
            .file
            .as_deref()
            .map(|file| normalize_path(Path::new(file)));
        let mut changed = false;
        let mut threads = self.load()?;
        for thread in &mut threads {
            if Path::new(&thread.workspace) != workspace {
                continue;
            }
            if let (Some(file), Some(content)) = (&file, params.content.as_deref())
                && Path::new(&thread.file) == file
            {
                changed |= reanchor(thread, content);
            }
        }
        if changed {
            self.save(&threads)?;
        }
        let threads = threads
            .into_iter()
            .filter(|thread| Path::new(&thread.workspace) == workspace)
            .filter(|thread| {
                file.as_ref()
                    .map(|file| Path::new(&thread.file) == file)
                    .unwrap_or(true)
            })
            .collect();
        Ok(ThreadListResult { threads })
    }

    /// Link a thread to a Codex session.
    pub fn link(&self, params: &ThreadLinkParams) -> anyhow::Result<ThreadLinkResult> {
        let mut threads = self.load()?;
        let Some(thread) = threads
            .iter_mut()
            .find(|thread| thread.thread_id == params.thread_id)
        else {
            anyhow::bail!("thread not found: {}", params.thread_id);
        };
        thread.codex_session_id = Some(params.codex_session_id.clone());
        thread.codex_session_path = params.codex_session_path.clone();
        thread.status = "linked".to_string();
        thread.updated_at = unix_timestamp();
        let thread = thread.clone();
        self.save(&threads)?;
        Ok(ThreadLinkResult { thread })
    }

    /// Attach an existing Codex session to a new current-line anchor.
    pub fn attach(&self, params: &ThreadAttachParams) -> anyhow::Result<ThreadLinkResult> {
        let created = self.create(&ThreadCreateParams {
            workspace: params.workspace.clone(),
            file: params.file.clone(),
            offset: params.offset,
            content: params.content.clone(),
            prompt: format!("Attached Codex session {}", params.codex_session_id),
            selection_start: None,
            selection_end: None,
            model: None,
            reasoning_effort: None,
        })?;
        self.link(&ThreadLinkParams {
            thread_id: created.thread.thread_id,
            codex_session_id: params.codex_session_id.clone(),
            codex_session_path: None,
        })
    }

    /// Resolve a newly launched Codex session by finding the newest project session.
    pub fn resolve(&self, params: &ThreadResolveParams) -> anyhow::Result<ThreadResolveResult> {
        let sessions =
            crate::codex_sessions::list_sessions(&crate::codex_sessions::CodexSessionsParams {
                workspace: params.workspace.clone(),
                all: false,
                limit: Some(10),
            })?;
        let Some(session) = sessions
            .sessions
            .into_iter()
            .find(|session| session.modified_at >= params.started_after_unix)
        else {
            return Ok(ThreadResolveResult {
                thread: None,
                resolved: false,
            });
        };
        let linked = self.link(&ThreadLinkParams {
            thread_id: params.thread_id.clone(),
            codex_session_id: session.id,
            codex_session_path: Some(session.path),
        })?;
        Ok(ThreadResolveResult {
            thread: Some(linked.thread),
            resolved: true,
        })
    }

    /// Delete a thread by id.
    pub fn delete(&self, params: &ThreadDeleteParams) -> anyhow::Result<ThreadDeleteResult> {
        let mut threads = self.load()?;
        let Some(index) = threads
            .iter()
            .position(|thread| thread.thread_id == params.thread_id)
        else {
            return Ok(ThreadDeleteResult { thread: None });
        };
        let thread = threads.remove(index);
        self.save(&threads)?;
        Ok(ThreadDeleteResult {
            thread: Some(thread),
        })
    }

    fn load(&self) -> anyhow::Result<Vec<ThreadAnchor>> {
        let Ok(contents) = std::fs::read_to_string(&self.path) else {
            return Ok(Vec::new());
        };
        Ok(serde_json::from_str(&contents).unwrap_or_default())
    }

    fn save(&self, threads: &[ThreadAnchor]) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, serde_json::to_string_pretty(threads)?)?;
        Ok(())
    }
}

fn launch_for(
    thread: &ThreadAnchor,
    content: &str,
    selection_start: Option<usize>,
    selection_end: Option<usize>,
) -> ThreadLaunch {
    let mut argv = vec!["codex".to_string(), "--no-alt-screen".to_string()];
    if let Some(session_id) = &thread.codex_session_id {
        argv.push("resume".to_string());
        argv.push(session_id.clone());
    } else {
        if let Some(model) = &thread.model {
            argv.push("--model".to_string());
            argv.push(model.clone());
        }
        if let Some(effort) = &thread.reasoning_effort {
            argv.push("-c".to_string());
            argv.push(crate::models::reasoning_effort_config_arg(effort));
        }
        argv.push("-C".to_string());
        argv.push(thread.workspace.clone());
    }
    argv.push(composed_prompt(
        thread,
        content,
        selection_start,
        selection_end,
    ));
    ThreadLaunch {
        argv,
        cwd: thread.workspace.clone(),
        started_after_unix: unix_timestamp(),
        model: thread.model.clone(),
        reasoning_effort: thread.reasoning_effort.clone(),
    }
}

fn composed_prompt(
    thread: &ThreadAnchor,
    content: &str,
    selection_start: Option<usize>,
    selection_end: Option<usize>,
) -> String {
    let mut prompt = format!(
        "You are working in {workspace}.\nFile: {file}\nLine: {line}\nUser ask: {ask}\n\n",
        workspace = thread.workspace,
        file = thread.file,
        line = thread.current_line,
        ask = thread.prompt
    );
    if let (Some(start), Some(end)) = (selection_start, selection_end)
        && start <= end
        && end <= content.len()
        && content.is_char_boundary(start)
        && content.is_char_boundary(end)
    {
        prompt.push_str("Selected code:\n");
        prompt.push_str(&cap_context(&content[start..end]));
        return prompt;
    }
    prompt.push_str("Nearby code context:\n");
    prompt.push_str(&cap_context(&nearby_lines(
        content,
        thread.current_line,
        80,
    )));
    prompt
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
    context.chars().take(16 * 1024).collect()
}

fn reanchor(thread: &mut ThreadAnchor, content: &str) -> bool {
    let matches: Vec<usize> = content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| (line_hash(line) == thread.line_hash).then_some(index + 1))
        .collect();
    let new_line = matches
        .iter()
        .copied()
        .min_by_key(|candidate| candidate.abs_diff(thread.current_line));
    let old_line = thread.current_line;
    let old_status = thread.status.clone();
    match new_line {
        Some(line) => {
            thread.current_line = line;
            if thread.status == "stale" {
                thread.status = if thread.codex_session_id.is_some() {
                    "linked".to_string()
                } else {
                    "open".to_string()
                };
            }
        }
        None => {
            thread.status = "stale".to_string();
        }
    }
    let changed = old_line != thread.current_line || old_status != thread.status;
    if changed {
        thread.updated_at = unix_timestamp();
    }
    changed
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

fn line_hash(line: &str) -> String {
    crate::cache::compute_hash(&normalize_line(line))
}

fn normalize_line(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut output: String = value.chars().take(max_chars).collect();
    if value.chars().count() > max_chars {
        output.push_str("...");
    }
    output
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn new_thread_id() -> String {
    format!(
        "thread-{}-{}-{}",
        unix_millis(),
        std::process::id(),
        THREAD_COUNTER.fetch_add(1, Ordering::SeqCst)
    )
}

fn new_example_id() -> String {
    format!(
        "example-{}-{}-{}",
        unix_millis(),
        std::process::id(),
        THREAD_COUNTER.fetch_add(1, Ordering::SeqCst)
    )
}

fn title_for_example(prompt: &str, relative_path: &str) -> String {
    let prompt = prompt.trim();
    if !prompt.is_empty() {
        return truncate(prompt, PROMPT_PREVIEW_MAX_CHARS);
    }
    Path::new(relative_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("example")
        .to_string()
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

/// Runtime store path.
pub fn store_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("threads.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn store() -> (ThreadStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "harnessd_threads_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        (ThreadStore::new(dir.join("threads.json")), dir)
    }

    #[test]
    fn persists_links_and_reanchors() {
        let (store, dir) = store();
        let file = dir.join("main.rs");
        let content = "fn a() {}\nlet target = 1;\nlet other = 2;\n";
        let created = store
            .create(&ThreadCreateParams {
                workspace: dir.display().to_string(),
                file: file.display().to_string(),
                offset: content.find("target").unwrap(),
                content: content.to_string(),
                prompt: "fix target".to_string(),
                selection_start: None,
                selection_end: None,
                model: None,
                reasoning_effort: None,
            })
            .unwrap();
        assert_eq!(created.thread.current_line, 2);
        let linked = store
            .link(&ThreadLinkParams {
                thread_id: created.thread.thread_id.clone(),
                codex_session_id: "abc".to_string(),
                codex_session_path: None,
            })
            .unwrap();
        assert_eq!(linked.thread.codex_session_id.as_deref(), Some("abc"));

        let shifted = "prep();\nfn a() {}\nlet target = 1;\nlet other = 2;\n";
        let listed = store
            .list(&ThreadListParams {
                workspace: dir.display().to_string(),
                file: Some(file.display().to_string()),
                content: Some(shifted.to_string()),
            })
            .unwrap();
        assert_eq!(listed.threads[0].current_line, 3);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn stale_when_anchor_line_disappears() {
        let (store, dir) = store();
        let file = dir.join("main.rs");
        let content = "a\nneedle\nc\n";
        let created = store
            .create(&ThreadCreateParams {
                workspace: dir.display().to_string(),
                file: file.display().to_string(),
                offset: content.find("needle").unwrap(),
                content: content.to_string(),
                prompt: "ask".to_string(),
                selection_start: None,
                selection_end: None,
                model: None,
                reasoning_effort: None,
            })
            .unwrap();
        let listed = store
            .list(&ThreadListParams {
                workspace: dir.display().to_string(),
                file: Some(file.display().to_string()),
                content: Some("a\nb\nc\n".to_string()),
            })
            .unwrap();
        assert_eq!(listed.threads[0].thread_id, created.thread.thread_id);
        assert_eq!(listed.threads[0].status, "stale");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn persists_examples_and_preserves_them_on_reanchor() {
        let (store, dir) = store();
        let file = dir.join("main.rs");
        let content = "fn a() {}\nlet target = 1;\n";
        let created = store
            .create(&ThreadCreateParams {
                workspace: dir.display().to_string(),
                file: file.display().to_string(),
                offset: content.find("target").unwrap(),
                content: content.to_string(),
                prompt: "explain target".to_string(),
                selection_start: None,
                selection_end: None,
                model: None,
                reasoning_effort: None,
            })
            .unwrap();
        let linked = store
            .add_example(
                &created.thread.thread_id,
                &crate::rpc::ScratchCreateResult {
                    path: dir
                        .join("scratch")
                        .join("hash")
                        .join("thread")
                        .join("demo.rs")
                        .display()
                        .to_string(),
                    relative_path: "scratch/hash/thread/demo.rs".to_string(),
                    bytes: 42,
                    lines: 3,
                    created_at: 2,
                    source_file: file.display().to_string(),
                    prompt_preview: "show usage".to_string(),
                },
                "show usage",
            )
            .unwrap();
        assert_eq!(linked.thread.examples.len(), 1);
        assert_eq!(linked.example.title, "show usage");

        let shifted = "prep();\nfn a() {}\nlet target = 1;\n";
        let listed = store
            .list(&ThreadListParams {
                workspace: dir.display().to_string(),
                file: Some(file.display().to_string()),
                content: Some(shifted.to_string()),
            })
            .unwrap();
        assert_eq!(listed.threads[0].current_line, 3);
        assert_eq!(listed.threads[0].examples.len(), 1);
        assert_eq!(
            listed.threads[0].examples[0].relative_path,
            "scratch/hash/thread/demo.rs"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn old_thread_json_without_examples_defaults_to_empty() {
        let thread: ThreadAnchor = serde_json::from_str(
            r#"{
                "thread_id":"thread-1",
                "workspace":"/workspace",
                "file":"/workspace/src/main.rs",
                "original_line":1,
                "current_line":1,
                "byte_offset":0,
                "line_hash":"hash",
                "line_preview":"fn main() {}",
                "prompt_preview":"ask",
                "prompt":"ask",
                "status":"open",
                "created_at":1,
                "updated_at":1
            }"#,
        )
        .unwrap();
        assert!(thread.examples.is_empty());
    }
}
