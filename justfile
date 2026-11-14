# Optional convenience commands.
#
# The template does not require `just`; every recipe below is a thin wrapper
# around commands documented in README.md.

set windows-shell := ["pwsh.exe", "-NoLogo", "-NoProfile", "-Command"]

default:
    @just --list

fmt:
    cargo fmt --all --check

fmt-fix:
    cargo fmt --all

check:
    cargo check --workspace --all-targets --locked

test:
    cargo nextest run --workspace --all-targets --locked

test-doc:
    cargo test --workspace --doc --locked

deny:
    cargo deny check

clippy:
    cargo clippy --workspace --all-targets --locked -- -D warnings

size:
    cargo run --locked -p xtask -- size

build:
    cargo build --workspace --all-targets --locked --release

pre-push:
    cargo fmt --all --check
    cargo check --workspace --all-targets --locked
    cargo nextest run --workspace --all-targets --locked
    cargo test --workspace --doc --locked
    cargo clippy --workspace --all-targets --locked -- -D warnings

preview:
    ./scripts/dev-preview.sh

release:
    cargo run -p xtask -- bundle-macos

ci:
    cargo fmt --all --check
    cargo check --workspace --all-targets --locked
    cargo nextest run --workspace --all-targets --locked
    cargo test --workspace --doc --locked
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo deny check
    cargo build --workspace --all-targets --locked --release
    cargo run --locked -p xtask -- size
