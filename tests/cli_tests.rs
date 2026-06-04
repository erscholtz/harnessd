use clap::Parser;
use harnessd::cli::{Cli, Commands, ThreadCommands};

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
    ]);

    match cli.command {
        Commands::Bridge {
            method,
            file,
            line,
            text,
            cursor,
        } => {
            assert_eq!(method, "complete");
            assert_eq!(file.as_deref(), Some(std::path::Path::new("src/main.rs")));
            assert_eq!(line, None);
            assert_eq!(text, None);
            assert_eq!(cursor.as_deref(), Some("12"));
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
    ]);

    match cli.command {
        Commands::Inline {
            file,
            offset,
            prompt,
        } => {
            assert_eq!(file, std::path::PathBuf::from("src/main.rs"));
            assert_eq!(offset, 12);
            assert_eq!(prompt, "insert validation");
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
    ]);

    match cli.command {
        Commands::Scratch {
            workspace,
            file,
            offset,
            prompt,
            selection_start,
            selection_end,
        } => {
            assert_eq!(workspace, std::path::PathBuf::from("."));
            assert_eq!(file, std::path::PathBuf::from("src/main.rs"));
            assert_eq!(offset, 12);
            assert_eq!(prompt, "sketch usage");
            assert_eq!(selection_start, Some(1));
            assert_eq!(selection_end, Some(5));
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
                },
        } => {
            assert_eq!(workspace, std::path::PathBuf::from("."));
            assert_eq!(file, std::path::PathBuf::from("src/main.rs"));
            assert_eq!(offset, 12);
            assert_eq!(prompt, "explain this");
            assert_eq!(selection_start, Some(1));
            assert_eq!(selection_end, Some(5));
        }
        _ => panic!("expected thread create command"),
    }
}
