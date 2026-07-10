# Examples

## Unit Test

```rust
#[test]
fn formats_greeting() {
    let greeting = Greeting::new("Rust");

    assert_eq!("Hello, Rust!", greeting.message());
}
```

## Integration Test For A Binary

```rust
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn cli_prints_greeting() -> Result<(), Box<dyn std::error::Error>> {
    let mut command = Command::cargo_bin("template")?;

    command
        .arg("workspace")
        .assert()
        .success()
        .stdout(predicate::str::contains("Hello, workspace!"));

    Ok(())
}
```

## Fake Boundary

```rust
struct FakeRecipientProvider;

impl RecipientProvider for FakeRecipientProvider {
    fn recipient(&self) -> &str {
        "workspace"
    }
}
```
