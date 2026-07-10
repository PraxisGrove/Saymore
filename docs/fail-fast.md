# Fail Fast

Fail-fast design means invalid state is rejected as close to the source as
possible. It does not mean panicking in production code.

## Preferred Techniques

- Encode invariants in types.
- Validate input at CLI, API, config, and persistence boundaries.
- Use constructors that return `Result` for fallible values.
- Use enums or newtypes for self-documenting callsites.
- Keep error variants specific enough for callers and tests.
- Add startup checks for required configuration.

## Assertions

Do not use production assertions to compensate for weak types or missing
validation. A failed assertion usually appears too late and gives callers no
structured way to recover.

Tests are different: tests should use assertions to describe expected behavior.

## Lints

The workspace denies common late-failure patterns:

```toml
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
panic_in_result_fn = "deny"
todo = "deny"
unimplemented = "deny"
```

When a lint blocks code, improve the API shape or error path instead of adding
an allow unless there is a documented reason.
