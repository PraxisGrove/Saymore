# Error Handling

Prefer explicit errors and early validation over late panics.

Rules:

- Production code must not use `unwrap`, `expect`, `panic`, `todo`, or
  `unimplemented`.
- Production assertions are not validation.
- Tests may use assertions to verify behavior.
- Use `thiserror` for library errors that callers need to inspect.
- Use `anyhow` at binary or xtask boundaries where contextual reporting matters.

Fail early by using:

- `TryFrom` and fallible constructors.
- Newtypes for validated values.
- Enums instead of boolean flags.
- Startup validation for configuration.
- Explicit parse functions at user-input boundaries.
