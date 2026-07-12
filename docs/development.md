# Development

The base workflow uses Cargo plus `cargo-nextest` and `cargo-deny`. Optional
tools can improve local ergonomics, but the mature-project gate should stay
explicit and reproducible.

## Required Gates

Run these before handing off a change:

```bash
cargo fmt --all --check
cargo check --workspace --all-targets
cargo nextest run --workspace --all-targets
cargo test --workspace --doc
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
cargo build --workspace --all-targets --release
cargo run -p xtask -- size
```

Use `cargo fmt --all` when you want to apply formatting.

## Desktop Toolchain

The target desktop app is compiled through Cargo and Slint. Node.js, pnpm,
React, TypeScript, Vite, and Tauri are not part of the production toolchain.

The desktop crate's Rust build script must compile its `.slint` files so the
standard `cargo check` and `cargo build` commands validate both UI declarations
and Rust code.

Create the local ad-hoc signed macOS application bundle with:

```bash
cargo run -p xtask -- bundle-macos
```

## Optional Shortcuts

If `just` is installed, the `justfile` provides shortcuts for the same commands:

```bash
just ci
just test
just test-doc
just clippy
just deny
just size
```

Do not document `just` as a required setup step. CI should use Cargo directly.

## Dependency Changes

Declare shared versions in `[workspace.dependencies]` in the root `Cargo.toml`.
Member crates should use `workspace = true` where possible.

When adding a dependency, include the reason in the change description. Prefer
small, maintained crates with a clear API and avoid broad feature sets unless
the project needs them.

## Size Gate

The size gate is intentionally approximate. Its job is to catch large files and
large functions early, especially during AI-assisted development where changes
can grow quickly.

Warnings should trigger a split plan. Errors should block the change unless
there is a documented reason and a short-term migration plan.
