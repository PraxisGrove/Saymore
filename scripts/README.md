## Scripts

This template keeps required maintenance tasks in Rust under `crates/xtask`.

Use:

```bash
cargo run -p xtask -- size
cargo run -p xtask -- update-readme-version <new-version>
```

Shell scripts under this directory are legacy helpers and should not be part of
the required template workflow.
