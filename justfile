# https://github.com/casey/just — install: `cargo install just` or package manager

default:
    @just --list

# Build debug binary
build:
    cargo build

# Build release binary
release:
    cargo build --release

# Typecheck only
check:
    cargo check --all-targets

# Run all tests
test:
    cargo test

# Format (writes files)
fmt:
    cargo fmt

# CI-style format check
fmt-check:
    cargo fmt --check

# Lints (warnings fail)
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# fmt + clippy + test
ci: fmt-check clippy test

# Run the binary; pass args after `--`: `just run -- --help`
run *args:
    cargo run -- {{ args }}

# Foreground daemon (Ctrl+C to exit)
daemon:
    cargo run -- daemon

# Ask a running daemon to exit (`harnessd stop`)
stop:
    cargo run -- stop

# Remove target/
clean:
    cargo clean

# Update Cargo.lock from Cargo.toml (no version bumps)
update-lockfile:
    cargo update
