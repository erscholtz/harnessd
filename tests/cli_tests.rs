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
