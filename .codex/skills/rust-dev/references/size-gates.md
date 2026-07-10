# Size Gates

Size gates keep files and functions small enough for humans and coding agents to
review safely.

## Defaults

- File warning: over 600 lines.
- File error: over 800 lines.
- Function warning: over 80 body lines.
- Function error: over 150 body lines.

## Command

```bash
cargo run -p xtask -- size
```

Use the defaults unless the project has a documented reason to adjust limits.

## Notes

Function detection is intentionally approximate. It is designed to find large
functions early, not to parse every Rust construct perfectly.

Warnings should trigger a split plan. Errors should block new work unless there
is a documented short-term exception.
