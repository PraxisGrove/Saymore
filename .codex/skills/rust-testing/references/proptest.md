# proptest

Use property-based tests when the input space is large and examples are likely
to miss edge cases.

## Good Uses

- Parsers and formatters.
- Normalization logic.
- Ordering and idempotency rules.
- Algorithms with many combinations.

## Guidelines

- State the invariant in the test name or comments.
- Generate only valid inputs unless invalid input is the point.
- Keep case counts practical for CI.
- Convert important minimized failures into fixed regression tests.

Example invariant:

```rust
proptest! {
    #[test]
    fn normalized_values_are_idempotent(input in any_valid_input()) {
        let once = normalize(&input);
        let twice = normalize(&once);

        prop_assert_eq!(once, twice);
    }
}
```
