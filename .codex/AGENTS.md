# Agent Instructions

Purpose: provide executable engineering constraints and working agreements for
AI coding agents. Keep guidance concise, verifiable, and traceable.

## Scope And Priority

- Prefer the closest instruction file to the current working directory.
- Priority order: current user request, local `AGENTS.override.md`, local
  `AGENTS.md`, then `$CODEX_HOME/AGENTS.md` when present.

## Required Skills

- For Rust engineering work, read and follow `skills/rust-dev/SKILL.md`.
- For Rust test work, read and follow `skills/rust-testing/SKILL.md`.

## Hard Rules

- Do not skip required analysis, validation, or quality gates to move faster.
- Do not reduce quality because of time pressure.
- Do not split work into partial delivery unless the user asks for staged work.
- Prefer established libraries and documented patterns over reinventing common
  infrastructure.
- Verify assumptions with local commands or authoritative documentation.

## Standard Workflow

1. Understand the request, current code, and relevant documentation.
2. Plan the smallest coherent change that solves the request.
3. Implement serially, keeping unrelated refactors out of scope.
4. Run the project gates:

   ```bash
   cargo fmt --all --check
   cargo check --workspace --all-targets
   cargo test --workspace --all-targets
   cargo clippy --workspace --all-targets -- -D warnings
   cargo build --workspace --all-targets --release
   cargo run -p xtask -- size
   ```

5. Report what changed, how it was verified, and any remaining risk.

## Rust Development Defaults

- Keep code simple, modular, and composable.
- Prefer clear crate boundaries over large catch-all modules.
- Production code must not use `unwrap`, `expect`, `panic`, `todo`, or
  `unimplemented`; use explicit errors and early validation instead.
- Do not use production assertions as validation. Tests may use assertions to
  verify behavior.
- Do not keep compatibility layers for old template structure unless the user
  explicitly asks for migration compatibility.
- Use direct Cargo commands as the source of truth. Optional tools such as
  `just`, `prek`, and `cargo-nextest` must stay optional.

## Testing Defaults

- Test behavior and contracts, not implementation details.
- Use mocks or fakes for external I/O boundaries.
- Keep tests deterministic: avoid real time, real network, and global shared
  state where possible.
- Prefer integration tests for externally visible behavior.

## Communication

- Lead with the result.
- Keep summaries short and concrete.
- Mention commands that were run and whether they passed.
- Call out assumptions, limitations, and follow-up work only when relevant.
