use clap::Parser;
use harnessd::cli::{Cli, Commands};

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
