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
    cargo check --workspace --all-targets

test:
    cargo nextest run --workspace --all-targets

test-doc:
    cargo test --workspace --doc

deny:
    cargo deny check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

size:
    cargo run -p xtask -- size

build:
    cargo build --workspace --all-targets --release

preview:
    ./scripts/dev-preview.sh

release:
    cargo run -p xtask -- bundle-macos

ci:
    cargo fmt --all --check
    cargo check --workspace --all-targets
    cargo nextest run --workspace --all-targets
    cargo test --workspace --doc
    cargo clippy --workspace --all-targets -- -D warnings
    cargo deny check
    cargo build --workspace --all-targets --release
    cargo run -p xtask -- size
