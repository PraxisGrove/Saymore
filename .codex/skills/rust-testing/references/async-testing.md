# Async Testing

## Principles

- Keep async tests deterministic.
- Avoid sleeps as synchronization.
- Prefer explicit events, channels, or cancellation tokens.
- Bound timeouts when waiting for external behavior.

## Tokio

Use `#[tokio::test]` when the crate already depends on Tokio. Select only the
runtime features the crate needs.

Example:

```rust
#[tokio::test]
async fn completes_work() {
    let result = do_work().await;

    assert_eq!(Expected::Done, result);
}
```

## Time Control

If code depends on timers, prefer injecting clock behavior or using Tokio time
control in focused tests. Avoid tests that rely on arbitrary sleep durations.

## Cancellation

Test cancellation paths explicitly when async tasks are cancellable. Assert the
observable result, not internal scheduling order.
