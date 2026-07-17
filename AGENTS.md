# AGENTS.md

Saymore is a local-first Rust desktop application built with Slint. Optimize for
small, explicit changes that humans and agents can understand and verify.

## Working Rules

- Before editing, read the request or spec, inspect the working tree, identify
  the owning crate, and read the relevant project documentation.
- Preserve changes that are not yours. Do not perform unrelated cleanup or
  silently expand scope.
- Work on one coherent feature or fix at a time. Add dependencies, widen public
  APIs, or edit generated artifacts only when the requested behavior requires
  it.
- Update the owning document when a change alters architecture, behavior,
  workflow, or another documented contract.

## Architecture

- `crates/app` owns business types, invariants, use cases, orchestration, and
  port traits. It must not contain Slint or concrete filesystem, network,
  database, or process implementations.
- `crates/infra` owns concrete implementations of app ports and may depend on
  `app`.
- `apps/desktop` owns Slint UI, dependency wiring, and process lifecycle. It may
  depend on `app` and `infra`; reusable crates must not depend on Slint.
- `crates/xtask` owns required repository maintenance and packaging automation.
- Preserve `desktop -> app` and `desktop -> infra -> app`. Never introduce a
  reverse dependency to avoid defining a proper port.
- Keep public APIs intentional and modules focused. Add a crate only for a
  substantial ownership or dependency boundary.

## Rust Constraints

- Production code must not use `unwrap`, `expect`, `panic`, `todo`,
  `unimplemented`, or assertions for validation. Reject invalid state at a
  boundary and return explicit errors.
- Prefer types that make invalid or ambiguous calls difficult: avoid unclear
  boolean or `Option` positional parameters and use exhaustive matches for
  closed domains.
- Document new traits with their role and implementation expectations.
- Put shared dependency versions in the workspace manifest and use
  `workspace = true` from member crates where possible.
- Do not add behavior to files or functions that already exceed the enforced
  size limits; split them first. See `docs/development.md` for thresholds.

## Tests

- Test observable behavior and contracts, not private implementation details.
- Do not expose production APIs only for tests or add tests for static constants
  and removed behavior.
- Keep unit tests near pure logic. Put integration tests in the owning crate's
  `tests/` directory; share helpers only after duplication becomes meaningful.

## Verification

- During implementation, run the smallest checks that cover the changed crate
  and behavior. Documentation-only changes do not require Rust workspace gates.
- `.pre-commit-config.yaml` is the single source of truth for commit and push
  gates. Run `prek install --prepare-hooks` once per checkout and never bypass
  the installed hooks with `--no-verify`.
- Do not claim completion until the requested outcome is present, relevant tests
  pass, affected documentation is current, and the final diff has been reviewed
  for both repository standards and fidelity to the originating request.
- In the final report, list verification evidence and disclose checks that could
  not run, remaining risks, or follow-up work.

## Documentation

- Architecture and boundaries: `docs/architecture.md`
- Development workflow, dependencies, and size limits: `docs/development.md`
- Testing and review: `docs/testing.md`, `docs/review.md`
- Error and validation policy: `docs/error-handling.md`, `docs/fail-fast.md`
- Technology decisions: `docs/technology-stack.md`, `docs/adr/`
- Product scope and feature decisions: `docs/product/`

For work spanning sessions, leave the current objective, completed work,
verification, blockers, changed files, and next step in the originating issue,
spec, or existing progress artifact.
