# Design Guidelines

Use these rules to keep Rust code easy to understand, review, and modify.

## Principles

- Keep it simple. Prefer clear data flow over clever abstractions.
- Avoid duplication when it hides a real concept, but do not abstract too early.
- Keep public APIs small and explicit.
- Prefer composition over global state.
- Make invalid states hard to represent when doing so stays readable.

## API Shape

- Avoid boolean or ambiguous `Option` positional parameters when callsites become
  unclear. Prefer enums, named constructors, or small value types.
- Use precise error types at boundaries that need structured handling.
- Add doc comments to new traits explaining their role and implementation
  expectations.
- Prefer exhaustive `match` statements when the domain is closed.

## Modules

- Prefer private modules with explicit public exports.
- Add a new module before growing a large file.
- Keep orchestration files focused on orchestration.
- Move invariants and tests close to the code that owns them.

## AI-Assisted Development

Coding agents tend to produce plausible large diffs. Keep changes scoped:

- Avoid broad rewrites unrelated to the request.
- Keep non-mechanical changes under roughly 500 changed lines when possible.
- Split changes over 800 lines into reviewable stages.
