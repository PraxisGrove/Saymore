# Template Guide

Use this repository as a starting point for Rust projects that should be easy
for humans and AI coding agents to maintain.

## Initial Adaptation

1. Rename crates from `template-*` to the project prefix.
2. Update package authors, license, and README.
3. Keep the workspace layout unless the project has a simpler need.
4. Keep Cargo commands as the required workflow.
5. Update `AGENTS.md` when project-specific rules appear.

## What To Keep

- Workspace metadata and dependency centralization.
- Workspace lints.
- Cargo-based CI.
- Rust-only xtask size gates.
- Architecture, development, testing, and review docs.

## What To Treat As Optional

- `just`
- `prek`
- Release automation
- Dependency audit tools

Optional tools should improve workflow without becoming required for a clean
checkout to build and test.
