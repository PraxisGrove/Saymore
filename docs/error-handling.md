# Error Handling

Prefer explicit errors over assertions in production code. Errors should appear
at construction, parsing, startup, or boundary validation time instead of being
hidden behind panics.

## Rules

- Production code must not use `unwrap`, `expect`, `panic`, `todo`, or
  `unimplemented`.
- Production code should not use `assert!` or `debug_assert!` as a substitute
  for validation.
- Tests may use assertions to verify behavior.
- Libraries should expose structured errors with `thiserror` when callers need
  to handle variants.
- Binaries and `xtask` may use `anyhow` at the outer boundary to attach context.

## Fail Earlier

Use type and constructor boundaries to reject invalid state early:

```rust
pub struct NonEmptyName(String);

impl TryFrom<String> for NonEmptyName {
    type Error = NameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            return Err(NameError::Empty);
        }

        Ok(Self(value))
    }
}
```

Prefer:

- `TryFrom` for fallible value construction.
- Enums instead of boolean flags.
- Startup validation for configuration.
- Explicit parse functions for user input.
- `Result` at I/O, parsing, configuration, and external boundary layers.

Avoid:

- Validating deep inside business logic after invalid state has spread.
- Returning strings for errors that callers need to branch on.
- Hiding invalid state behind default values without documenting the behavior.
