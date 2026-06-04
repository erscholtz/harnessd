//! Integration tests for end-to-end prefetch and complete behavior.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use harnessd::acp::AcpClient;
use harnessd::ipc;
use harnessd::rpc::{
    AnchorsParams, CompleteParams, GenerateParams, InlineParams, PrefetchParams,
    ScratchCreateParams, ThreadCreateParams, ThreadLinkParams, ThreadListParams,
};
use harnessd::scratch::ScratchClient;
use harnessd::state::DaemonState;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_runtime_dir() -> PathBuf {
    let temp_dir = std::env::temp_dir();
    let unique_name = format!(
        "harnessd_ipc_test_{}_{}",
        std::process::id(),
        TEST_COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    temp_dir.join(unique_name)
}

#[cfg(unix)]
fn fake_acp(runtime_dir: &std::path::Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = runtime_dir.join("fake-acp.sh");
    std::fs::write(&path, body).expect("failed to write fake ACP executable");
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
}

#[cfg(unix)]
fn fake_codex(runtime_dir: &std::path::Path, body: &str) -> PathBuf {
    fake_acp(runtime_dir, body)
}

#[tokio::test]
async fn prefetch_populates_cache_for_complete() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).expect("failed to create runtime dir");

    let file_path = runtime_dir.join("fixture.rs");
    let source = r#"fn demo() {
    // TODO: implement demo
    let value = 42;
}
"#;
    std::fs::write(&file_path, source).expect("failed to write fixture");

    let state = DaemonState::new(runtime_dir.clone()).expect("failed to create daemon state");
    let prefetch = ipc::prefetch(
        &state,
        &PrefetchParams {
            path: file_path.to_string_lossy().to_string(),
        },
    )
    .await
    .expect("prefetch failed");

    assert_eq!(prefetch.scanned_files, 1);
    assert_eq!(prefetch.anchors_found, 1);
    assert_eq!(prefetch.proposals_stored, 1);

    let offset = source.find("value").expect("missing value offset");
    let suggestions = ipc::complete(
        &state,
        &CompleteParams {
            file: file_path.to_string_lossy().to_string(),
            offset,
            prefix: None,
        },
    )
    .await
    .expect("complete failed");

    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].label, "Implement TODO");
    assert!(suggestions[0].insert_text.contains("todo!"));

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[tokio::test]
async fn prefetch_populates_cache_for_python_complete() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).expect("failed to create runtime dir");

    let file_path = runtime_dir.join("fixture.py");
    let source = r#"def demo():
    # TODO: implement demo
    value = 42
"#;
    std::fs::write(&file_path, source).expect("failed to write fixture");

    let state = DaemonState::new(runtime_dir.clone()).expect("failed to create daemon state");
    let prefetch = ipc::prefetch(
        &state,
        &PrefetchParams {
            path: file_path.to_string_lossy().to_string(),
        },
    )
    .await
    .expect("prefetch failed");

    assert_eq!(prefetch.scanned_files, 1);
    assert_eq!(prefetch.anchors_found, 1);
    assert_eq!(prefetch.proposals_stored, 1);

    let offset = source.find("value").expect("missing value offset");
    let suggestions = ipc::complete(
        &state,
        &CompleteParams {
            file: file_path.to_string_lossy().to_string(),
            offset,
            prefix: None,
        },
    )
    .await
    .expect("complete failed");

    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].label, "Implement TODO");
    assert!(suggestions[0].insert_text.contains("NotImplementedError"));

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[tokio::test]
async fn status_reports_cache_and_request_metrics() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).expect("failed to create runtime dir");

    let file_path = runtime_dir.join("fixture.rs");
    let source = r#"fn demo() {
    // TODO: implement demo
}
"#;
    std::fs::write(&file_path, source).expect("failed to write fixture");

    let state = DaemonState::new(runtime_dir.clone()).expect("failed to create daemon state");

    ipc::prefetch(
        &state,
        &PrefetchParams {
            path: file_path.to_string_lossy().to_string(),
        },
    )
    .await
    .expect("prefetch failed");
    state.record_request("prefetch");

    let status = ipc::status(&state).await.expect("status failed");

    assert_eq!(status.pid, std::process::id());
    assert_eq!(status.cache.total_proposals, 1);
    assert_eq!(status.metrics.prefetch_requests, 1);
    assert!(!status.runtime.stale_lock);
    assert!(status.runtime.warnings.is_empty());
    assert!(!status.recent_proposals.is_empty());
    assert_eq!(status.recent_proposals[0].label, "Implement TODO");

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[tokio::test]
async fn thread_rpc_helpers_create_list_and_link() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).expect("failed to create runtime dir");

    let file_path = runtime_dir.join("fixture.rs");
    let source = "fn demo() {\n    let value = 1;\n}\n";
    std::fs::write(&file_path, source).expect("failed to write fixture");
    let state = DaemonState::new(runtime_dir.clone()).expect("failed to create daemon state");

    let created = ipc::thread_create(
        &state,
        &ThreadCreateParams {
            workspace: runtime_dir.to_string_lossy().to_string(),
            file: file_path.to_string_lossy().to_string(),
            offset: source.find("value").unwrap(),
            content: source.to_string(),
            prompt: "explain value".to_string(),
            selection_start: None,
            selection_end: None,
        },
    )
    .await
    .expect("thread create failed");
    assert_eq!(created.thread.current_line, 2);
    assert_eq!(created.launch.argv[0], "codex");

    let linked = ipc::thread_link(
        &state,
        &ThreadLinkParams {
            thread_id: created.thread.thread_id.clone(),
            codex_session_id: "session-1".to_string(),
            codex_session_path: None,
        },
    )
    .await
    .expect("thread link failed");
    assert_eq!(linked.thread.codex_session_id.as_deref(), Some("session-1"));

    let shifted = "fn prelude() {}\nfn demo() {\n    let value = 1;\n}\n";
    let listed = ipc::thread_list(
        &state,
        &ThreadListParams {
            workspace: runtime_dir.to_string_lossy().to_string(),
            file: Some(file_path.to_string_lossy().to_string()),
            content: Some(shifted.to_string()),
        },
    )
    .await
    .expect("thread list failed");
    assert_eq!(listed.threads.len(), 1);
    assert_eq!(listed.threads[0].current_line, 3);

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[tokio::test]
async fn anchors_report_supported_anchor_kinds_and_candidate_state() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).expect("failed to create runtime dir");
    let file_path = runtime_dir.join("anchors.rs");
    std::fs::write(
        &file_path,
        "fn a() { // TODO: one\n let x = 1; }\nfn b() { // FIXME: two\n let y = 1; }\nfn c() { todo!(); }\nfn d() { unimplemented!(); }\nfn empty() {}\n",
    )
    .unwrap();
    let state = DaemonState::new(runtime_dir.clone()).unwrap();

    let anchors = ipc::anchors(
        &state,
        &AnchorsParams {
            file: file_path.to_string_lossy().to_string(),
        },
    )
    .await
    .unwrap();
    let kinds: Vec<_> = anchors.iter().map(|anchor| anchor.kind.as_str()).collect();
    assert!(kinds.contains(&"todo_comment"));
    assert!(kinds.contains(&"fixme_comment"));
    assert!(kinds.contains(&"todo_macro"));
    assert!(kinds.contains(&"unimplemented_macro"));
    assert!(kinds.contains(&"empty_function_body"));
    assert!(anchors.iter().all(|anchor| anchor.status == "candidate"));

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[cfg(unix)]
#[tokio::test]
async fn generate_via_acp_is_bounded_cached_and_deduplicated() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let agent = fake_acp(
        &runtime_dir,
        r##"#!/bin/sh
read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1}}'
read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"fake"}}'
read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"```rust\nlet value = "}}}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"42;\n```"}}}}'
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"stopReason":"end_turn"}}'
"##,
    );
    let file_path = runtime_dir.join("fixture.rs");
    let source = "fn demo() {\n    // TODO: value\n}\n";
    std::fs::write(&file_path, source).unwrap();
    let state = DaemonState::new_with_acp(runtime_dir.clone(), AcpClient::new(agent)).unwrap();
    let params = GenerateParams {
        file: file_path.to_string_lossy().to_string(),
        offset: source.find("TODO").unwrap(),
    };

    let first = ipc::generate(&state, &params).await.unwrap();
    let second = ipc::generate(&state, &params).await.unwrap();
    assert_eq!(first.insert_text, "let value = 42;");
    assert_eq!(second.insert_text, first.insert_text);
    assert_eq!(state.cache.stats().await.unwrap().total_proposals, 1);
    let anchors = ipc::anchors(
        &state,
        &AnchorsParams {
            file: params.file.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(anchors[0].status, "ready");

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[cfg(unix)]
#[tokio::test]
async fn generate_keeps_separate_proposals_for_anchors_in_one_function() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let agent = fake_acp(
        &runtime_dir,
        r##"#!/bin/sh
read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1}}'
read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"fake"}}'
read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"let generated = true;"}}}}'
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"stopReason":"end_turn"}}'
"##,
    );
    let file_path = runtime_dir.join("fixture.rs");
    let source = "fn demo() {\n    // TODO: first\n    let value = 0;\n    // TODO: second\n}\n";
    std::fs::write(&file_path, source).unwrap();
    let state = DaemonState::new_with_acp(runtime_dir.clone(), AcpClient::new(agent)).unwrap();

    for offset in [
        source.find("first").unwrap(),
        source.find("second").unwrap(),
    ] {
        ipc::generate(
            &state,
            &GenerateParams {
                file: file_path.to_string_lossy().to_string(),
                offset,
            },
        )
        .await
        .unwrap();
    }

    assert_eq!(state.cache.stats().await.unwrap().total_proposals, 2);
    let suggestions = ipc::complete(
        &state,
        &CompleteParams {
            file: file_path.to_string_lossy().to_string(),
            offset: source.find("value").unwrap(),
            prefix: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(suggestions.len(), 2);

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[cfg(unix)]
#[tokio::test]
async fn denied_acp_permission_marks_anchor_failed() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let agent = fake_acp(
        &runtime_dir,
        r##"#!/bin/sh
read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1}}'
read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"fake"}}'
read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":9,"method":"session/request_permission","params":{"sessionId":"fake","toolCall":{"toolCallId":"write"},"options":[]}}'
read -r denied
"##,
    );
    let file_path = runtime_dir.join("fixture.rs");
    let source = "fn demo() {\n    todo!();\n}\n";
    std::fs::write(&file_path, source).unwrap();
    let state = DaemonState::new_with_acp(runtime_dir.clone(), AcpClient::new(agent)).unwrap();

    let error = ipc::generate(
        &state,
        &GenerateParams {
            file: file_path.to_string_lossy().to_string(),
            offset: source.find("todo").unwrap(),
        },
    )
    .await
    .expect_err("permission request should fail generation");
    assert!(error.to_string().contains("disallowed"));
    let anchors = ipc::anchors(
        &state,
        &AnchorsParams {
            file: file_path.to_string_lossy().to_string(),
        },
    )
    .await
    .unwrap();
    assert_eq!(anchors[0].status, "failed");

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[cfg(unix)]
#[tokio::test]
async fn timed_out_acp_generation_marks_anchor_failed() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let agent = fake_acp(
        &runtime_dir,
        r##"#!/bin/sh
while :; do :; done
"##,
    );
    let file_path = runtime_dir.join("fixture.rs");
    let source = "fn demo() {\n    todo!();\n}\n";
    std::fs::write(&file_path, source).unwrap();
    let state = DaemonState::new_with_acp(
        runtime_dir.clone(),
        AcpClient::with_timeout(agent, Duration::from_millis(10)),
    )
    .unwrap();

    let error = ipc::generate(
        &state,
        &GenerateParams {
            file: file_path.to_string_lossy().to_string(),
            offset: source.find("todo").unwrap(),
        },
    )
    .await
    .expect_err("timed out ACP request should fail generation");
    assert!(error.to_string().contains("timed out"));
    let anchors = ipc::anchors(
        &state,
        &AnchorsParams {
            file: file_path.to_string_lossy().to_string(),
        },
    )
    .await
    .unwrap();
    assert_eq!(anchors[0].status, "failed");

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[cfg(unix)]
#[tokio::test]
async fn inline_uses_live_buffer_content_without_caching() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let agent = fake_acp(
        &runtime_dir,
        r##"#!/bin/sh
read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1}}'
read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"fake"}}'
read -r prompt
case "$prompt" in
  *HARNESSD_CURSOR*unsaved_value*) ;;
  *) exit 7 ;;
esac
printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"let inserted = true;"}}}}'
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"stopReason":"end_turn"}}'
"##,
    );
    let file_path = runtime_dir.join("fixture.rs");
    std::fs::write(&file_path, "fn saved() {}\n").unwrap();
    let content = "fn demo() {\n    let unsaved_value = 1;\n}\n";
    let state = DaemonState::new_with_acp(runtime_dir.clone(), AcpClient::new(agent)).unwrap();

    let suggestion = ipc::inline(
        &state,
        &InlineParams {
            file: file_path.to_string_lossy().to_string(),
            offset: content.find("unsaved_value").unwrap(),
            content: content.to_string(),
            prompt: "insert handling".to_string(),
        },
    )
    .await
    .unwrap();
    assert_eq!(suggestion.label, "Inline ask");
    assert_eq!(suggestion.insert_text, "let inserted = true;");
    assert_eq!(state.cache.stats().await.unwrap().total_proposals, 0);

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[tokio::test]
async fn inline_validates_prompt_and_cursor_before_generation() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let file_path = runtime_dir.join("fixture.rs");
    std::fs::write(&file_path, "fn saved() {}\n").unwrap();
    let state = DaemonState::new(runtime_dir.clone()).unwrap();

    for params in [
        InlineParams {
            file: file_path.to_string_lossy().to_string(),
            offset: 0,
            content: "fn x() {}".to_string(),
            prompt: "  ".to_string(),
        },
        InlineParams {
            file: file_path.to_string_lossy().to_string(),
            offset: 99,
            content: "fn x() {}".to_string(),
            prompt: "ask".to_string(),
        },
        InlineParams {
            file: file_path.to_string_lossy().to_string(),
            offset: 1,
            content: "é".to_string(),
            prompt: "ask".to_string(),
        },
    ] {
        assert!(ipc::inline(&state, &params).await.is_err());
    }
    assert_eq!(state.cache.stats().await.unwrap().total_proposals, 0);

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[cfg(unix)]
#[tokio::test]
async fn inline_rejects_disallowed_acp_tool_requests() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let agent = fake_acp(
        &runtime_dir,
        r##"#!/bin/sh
read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1}}'
read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"fake"}}'
read -r ignored
printf '%s\n' '{"jsonrpc":"2.0","id":9,"method":"session/request_permission","params":{"sessionId":"fake","toolCall":{"toolCallId":"write"},"options":[]}}'
read -r denied
"##,
    );
    let file_path = runtime_dir.join("fixture.rs");
    std::fs::write(&file_path, "fn demo() {}\n").unwrap();
    let state = DaemonState::new_with_acp(runtime_dir.clone(), AcpClient::new(agent)).unwrap();
    let error = ipc::inline(
        &state,
        &InlineParams {
            file: file_path.to_string_lossy().to_string(),
            offset: 10,
            content: "fn demo() {}\n".to_string(),
            prompt: "fill it".to_string(),
        },
    )
    .await
    .expect_err("permission request should fail inline generation");
    assert!(error.to_string().contains("disallowed"));

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[cfg(unix)]
#[tokio::test]
async fn scratch_create_launches_read_only_codex_and_writes_artifact() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let args_log = runtime_dir.join("codex-args.txt");
    let prompt_log = runtime_dir.join("codex-prompt.txt");
    let fake = fake_codex(
        &runtime_dir,
        &format!(
            r##"#!/bin/sh
printf '%s\n' "$@" > "{args_log}"
cat > "{prompt_log}"
out=""
next=0
for arg in "$@"; do
  if [ "$next" = "1" ]; then
    out="$arg"
    next=0
  elif [ "$arg" = "--output-last-message" ]; then
    next=1
  fi
done
printf '%s\n' '{{"title":"Usage sketch","body":"fn main() {{\n    println!(\"scratch\");\n}}"}}' > "$out"
"##,
            args_log = args_log.display(),
            prompt_log = prompt_log.display()
        ),
    );
    let workspace = runtime_dir.join("workspace");
    std::fs::create_dir_all(workspace.join("src")).unwrap();
    let file_path = workspace.join("src").join("main.rs");
    let source = "fn demo() {\n    let unsaved_value = 1;\n}\n";
    std::fs::write(&file_path, "fn saved() {}\n").unwrap();
    let state = DaemonState::new_with_clients(
        runtime_dir.clone(),
        AcpClient::new(runtime_dir.join("unused-acp")),
        ScratchClient::new(fake, runtime_dir.clone()),
    )
    .unwrap();

    let result = ipc::scratch_create(
        &state,
        &ScratchCreateParams {
            workspace: workspace.to_string_lossy().to_string(),
            file: file_path.to_string_lossy().to_string(),
            offset: source.find("unsaved_value").unwrap(),
            content: source.to_string(),
            prompt: "sketch usage".to_string(),
            selection_start: None,
            selection_end: None,
        },
    )
    .await
    .unwrap();

    let args = std::fs::read_to_string(args_log).unwrap();
    assert!(args.contains("--ask-for-approval\nnever"));
    assert!(args.contains("--sandbox\nread-only"));
    assert!(args.contains("--cd\n"));
    assert!(args.contains(&workspace.to_string_lossy().to_string()));
    let prompt = std::fs::read_to_string(prompt_log).unwrap();
    assert!(prompt.contains("unsaved_value"));
    assert!(prompt.contains("must not edit files"));
    assert!(result.relative_path.starts_with("scratch/harnessd/"));
    assert!(result.relative_path.ends_with(".rs"));
    let written = std::fs::read_to_string(&result.path).unwrap();
    assert!(written.contains("harnessd scratch preview"));
    assert!(written.contains("println!(\"scratch\")"));
    assert_eq!(
        std::fs::read_to_string(&file_path).unwrap(),
        "fn saved() {}\n"
    );
    assert_eq!(state.cache.stats().await.unwrap().total_proposals, 0);

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[cfg(unix)]
#[tokio::test]
async fn scratch_rejects_malformed_and_oversized_codex_output_without_writing() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let malformed = fake_codex(
        &runtime_dir,
        r##"#!/bin/sh
out=""
next=0
for arg in "$@"; do
  if [ "$next" = "1" ]; then out="$arg"; next=0; elif [ "$arg" = "--output-last-message" ]; then next=1; fi
done
printf '%s\n' 'not json' > "$out"
"##,
    );
    let workspace = runtime_dir.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let file_path = workspace.join("main.rs");
    std::fs::write(&file_path, "fn saved() {}\n").unwrap();
    let params = ScratchCreateParams {
        workspace: workspace.to_string_lossy().to_string(),
        file: file_path.to_string_lossy().to_string(),
        offset: 0,
        content: "fn live() {}\n".to_string(),
        prompt: "sketch usage".to_string(),
        selection_start: None,
        selection_end: None,
    };
    let state = DaemonState::new_with_clients(
        runtime_dir.clone(),
        AcpClient::new(runtime_dir.join("unused-acp")),
        ScratchClient::new(malformed, runtime_dir.clone()),
    )
    .unwrap();
    assert!(ipc::scratch_create(&state, &params).await.is_err());
    assert!(!workspace.join("scratch").exists());

    let oversized = fake_codex(
        &runtime_dir,
        r##"#!/bin/sh
out=""
next=0
for arg in "$@"; do
  if [ "$next" = "1" ]; then out="$arg"; next=0; elif [ "$arg" = "--output-last-message" ]; then next=1; fi
done
{
  printf '{"title":"Big","body":"'
  i=0
  while [ "$i" -lt 401 ]; do
    printf 'line\\n'
    i=$((i + 1))
  done
  printf '"}\n'
} > "$out"
"##,
    );
    let state = DaemonState::new_with_clients(
        runtime_dir.clone(),
        AcpClient::new(runtime_dir.join("unused-acp")),
        ScratchClient::new(oversized, runtime_dir.clone()),
    )
    .unwrap();
    assert!(ipc::scratch_create(&state, &params).await.is_err());
    assert!(!workspace.join("scratch").exists());

    std::fs::remove_dir_all(&runtime_dir).ok();
}

#[cfg(unix)]
#[tokio::test]
async fn scratch_uses_create_new_and_does_not_overwrite_existing_artifact() {
    let runtime_dir = temp_runtime_dir();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let fake = fake_codex(
        &runtime_dir,
        r##"#!/bin/sh
out=""
next=0
for arg in "$@"; do
  if [ "$next" = "1" ]; then out="$arg"; next=0; elif [ "$arg" = "--output-last-message" ]; then next=1; fi
done
printf '%s\n' '{"title":"Demo","body":"fn main() {}"}' > "$out"
"##,
    );
    let workspace = runtime_dir.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let file_path = workspace.join("main.rs");
    std::fs::write(&file_path, "fn saved() {}\n").unwrap();
    let state = DaemonState::new_with_clients(
        runtime_dir.clone(),
        AcpClient::new(runtime_dir.join("unused-acp")),
        ScratchClient::new(fake, runtime_dir.clone()),
    )
    .unwrap();
    let params = ScratchCreateParams {
        workspace: workspace.to_string_lossy().to_string(),
        file: file_path.to_string_lossy().to_string(),
        offset: 0,
        content: "fn live() {}\n".to_string(),
        prompt: "same prompt".to_string(),
        selection_start: None,
        selection_end: None,
    };

    let first = ipc::scratch_create(&state, &params).await.unwrap();
    let second = ipc::scratch_create(&state, &params).await.unwrap();
    assert_ne!(first.path, second.path);
    assert!(std::path::Path::new(&first.path).exists());
    assert!(std::path::Path::new(&second.path).exists());

    std::fs::remove_dir_all(&runtime_dir).ok();
}
