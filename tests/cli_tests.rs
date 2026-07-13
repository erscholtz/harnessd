use clap::Parser;
use harnessd::cli::{Cli, Commands, MarkCommands, SettingsCommands, ThreadCommands};

#[test]
fn parses_setup_with_prefetch_path() {
    let cli = Cli::parse_from(["harnessd", "setup", "--path", "."]);

    match cli.command {
        Commands::Setup { path, no_tui } => {
            assert_eq!(path.as_deref(), Some(std::path::Path::new(".")));
            assert!(!no_tui);
        }
        _ => panic!("expected setup command"),
    }
}

#[test]
fn parses_setup_with_no_tui() {
    let cli = Cli::parse_from(["harnessd", "setup", "--no-tui"]);

    match cli.command {
        Commands::Setup { path, no_tui } => {
            assert!(path.is_none());
            assert!(no_tui);
        }
        _ => panic!("expected setup command"),
    }
}

#[test]
fn parses_teardown() {
    let cli = Cli::parse_from(["harnessd", "teardown"]);

    match cli.command {
        Commands::Teardown => {}
        _ => panic!("expected teardown command"),
    }
}

#[test]
fn parses_doctor() {
    let cli = Cli::parse_from(["harnessd", "doctor"]);

    match cli.command {
        Commands::Doctor => {}
        _ => panic!("expected doctor command"),
    }
}

#[test]
fn parses_lsp() {
    let cli = Cli::parse_from(["harnessd", "lsp"]);

    match cli.command {
        Commands::Lsp => {}
        _ => panic!("expected lsp command"),
    }
}

#[test]
fn parses_bridge_complete_flags() {
    let cli = Cli::parse_from([
        "harnessd",
        "bridge",
        "--method",
        "complete",
        "--file",
        "src/main.rs",
        "--cursor",
        "12",
        "--model",
        "gpt-5.4-mini",
        "--reasoning-effort",
        "low",
    ]);

    match cli.command {
        Commands::Bridge {
            method,
            file,
            line,
            text,
            cursor,
            model,
            reasoning_effort,
            no_background_refresh,
        } => {
            assert_eq!(method, "complete");
            assert_eq!(file.as_deref(), Some(std::path::Path::new("src/main.rs")));
            assert_eq!(line, None);
            assert_eq!(text, None);
            assert_eq!(cursor.as_deref(), Some("12"));
            assert_eq!(model.as_deref(), Some("gpt-5.4-mini"));
            assert_eq!(reasoning_effort.as_deref(), Some("low"));
            assert!(!no_background_refresh);
        }
        _ => panic!("expected bridge command"),
    }
}

#[test]
fn parses_inline_flags() {
    let cli = Cli::parse_from([
        "harnessd",
        "inline",
        "--file",
        "src/main.rs",
        "--offset",
        "12",
        "--prompt",
        "insert validation",
        "--model",
        "gpt-5.4-mini",
        "--reasoning-effort",
        "low",
    ]);

    match cli.command {
        Commands::Inline {
            file,
            offset,
            prompt,
            model,
            reasoning_effort,
        } => {
            assert_eq!(file, std::path::PathBuf::from("src/main.rs"));
            assert_eq!(offset, 12);
            assert_eq!(prompt, "insert validation");
            assert_eq!(model.as_deref(), Some("gpt-5.4-mini"));
            assert_eq!(reasoning_effort.as_deref(), Some("low"));
        }
        _ => panic!("expected inline command"),
    }
}

#[test]
fn parses_scratch_flags() {
    let cli = Cli::parse_from([
        "harnessd",
        "scratch",
        "--workspace",
        ".",
        "--file",
        "src/main.rs",
        "--offset",
        "12",
        "--prompt",
        "sketch usage",
        "--selection-start",
        "1",
        "--selection-end",
        "5",
        "--model",
        "gpt-5.4-mini",
        "--reasoning-effort",
        "low",
    ]);

    match cli.command {
        Commands::Scratch {
            workspace,
            file,
            offset,
            prompt,
            selection_start,
            selection_end,
            model,
            reasoning_effort,
        } => {
            assert_eq!(workspace, std::path::PathBuf::from("."));
            assert_eq!(file, std::path::PathBuf::from("src/main.rs"));
            assert_eq!(offset, 12);
            assert_eq!(prompt, "sketch usage");
            assert_eq!(selection_start, Some(1));
            assert_eq!(selection_end, Some(5));
            assert_eq!(model.as_deref(), Some("gpt-5.4-mini"));
            assert_eq!(reasoning_effort.as_deref(), Some("low"));
        }
        _ => panic!("expected scratch command"),
    }
}

#[test]
fn parses_codex_sessions_flags() {
    let cli = Cli::parse_from([
        "harnessd",
        "codex-sessions",
        "--workspace",
        ".",
        "--all",
        "--limit",
        "25",
    ]);

    match cli.command {
        Commands::CodexSessions {
            workspace,
            all,
            limit,
        } => {
            assert_eq!(workspace, std::path::PathBuf::from("."));
            assert!(all);
            assert_eq!(limit, 25);
        }
        _ => panic!("expected codex-sessions command"),
    }
}

#[test]
fn parses_thread_create_flags() {
    let cli = Cli::parse_from([
        "harnessd",
        "thread",
        "create",
        "--workspace",
        ".",
        "--file",
        "src/main.rs",
        "--offset",
        "12",
        "--prompt",
        "explain this",
        "--selection-start",
        "1",
        "--selection-end",
        "5",
        "--model",
        "gpt-5.5",
        "--reasoning-effort",
        "high",
    ]);

    match cli.command {
        Commands::Thread {
            command:
                ThreadCommands::Create {
                    workspace,
                    file,
                    offset,
                    prompt,
                    selection_start,
                    selection_end,
                    model,
                    reasoning_effort,
                },
        } => {
            assert_eq!(workspace, std::path::PathBuf::from("."));
            assert_eq!(file, std::path::PathBuf::from("src/main.rs"));
            assert_eq!(offset, 12);
            assert_eq!(prompt, "explain this");
            assert_eq!(selection_start, Some(1));
            assert_eq!(selection_end, Some(5));
            assert_eq!(model.as_deref(), Some("gpt-5.5"));
            assert_eq!(reasoning_effort.as_deref(), Some("high"));
        }
        _ => panic!("expected thread create command"),
    }
}

#[test]
fn parses_thread_example_flags() {
    let cli = Cli::parse_from([
        "harnessd",
        "thread",
        "example",
        "--thread-id",
        "thread-1",
        "--workspace",
        ".",
        "--file",
        "src/main.rs",
        "--offset",
        "12",
        "--prompt",
        "show a usage example",
        "--selection-start",
        "1",
        "--selection-end",
        "5",
        "--model",
        "gpt-5.5",
        "--reasoning-effort",
        "high",
    ]);

    match cli.command {
        Commands::Thread {
            command:
                ThreadCommands::Example {
                    thread_id,
                    workspace,
                    file,
                    offset,
                    prompt,
                    selection_start,
                    selection_end,
                    model,
                    reasoning_effort,
                },
        } => {
            assert_eq!(thread_id, "thread-1");
            assert_eq!(workspace, std::path::PathBuf::from("."));
            assert_eq!(file, std::path::PathBuf::from("src/main.rs"));
            assert_eq!(offset, 12);
            assert_eq!(prompt, "show a usage example");
            assert_eq!(selection_start, Some(1));
            assert_eq!(selection_end, Some(5));
            assert_eq!(model.as_deref(), Some("gpt-5.5"));
            assert_eq!(reasoning_effort.as_deref(), Some("high"));
        }
        _ => panic!("expected thread example command"),
    }
}

#[test]
fn parses_thread_delete_flags() {
    let cli = Cli::parse_from(["harnessd", "thread", "delete", "--thread-id", "thread-1"]);

    match cli.command {
        Commands::Thread {
            command: ThreadCommands::Delete { thread_id },
        } => {
            assert_eq!(thread_id, "thread-1");
        }
        _ => panic!("expected thread delete command"),
    }
}

#[test]
fn parses_mark_create_flags() {
    let cli = Cli::parse_from([
        "harnessd",
        "mark",
        "create",
        "--workspace",
        ".",
        "--file",
        "src/main.rs",
        "--offset",
        "12",
        "--thread-id",
        "thread-1",
    ]);

    match cli.command {
        Commands::Mark {
            command:
                MarkCommands::Create {
                    workspace,
                    file,
                    offset,
                    thread_id,
                },
        } => {
            assert_eq!(workspace, std::path::PathBuf::from("."));
            assert_eq!(file, std::path::PathBuf::from("src/main.rs"));
            assert_eq!(offset, 12);
            assert_eq!(thread_id.as_deref(), Some("thread-1"));
        }
        _ => panic!("expected mark create command"),
    }
}

#[test]
fn parses_mark_delete_confirmation_flag() {
    let cli = Cli::parse_from([
        "harnessd",
        "mark",
        "delete",
        "--mark-id",
        "mark-1",
        "--delete-attached-thread",
    ]);

    match cli.command {
        Commands::Mark {
            command:
                MarkCommands::Delete {
                    mark_id,
                    delete_attached_thread,
                },
        } => {
            assert_eq!(mark_id, "mark-1");
            assert!(delete_attached_thread);
        }
        _ => panic!("expected mark delete command"),
    }
}

#[test]
fn parses_settings_update_flags() {
    let cli = Cli::parse_from([
        "harnessd",
        "settings",
        "update",
        "--scratch-storage-mode",
        "temp",
        "--read-scope",
        "current_context",
    ]);

    match cli.command {
        Commands::Settings {
            command:
                SettingsCommands::Update {
                    scratch_storage_mode,
                    read_scope,
                },
        } => {
            assert!(scratch_storage_mode.is_some());
            assert!(read_scope.is_some());
        }
        _ => panic!("expected settings update command"),
    }
}
