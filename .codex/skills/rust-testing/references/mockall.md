# mockall

Use `mockall` when a trait boundary needs call assertions.

## Good Uses

- Repository or service traits.
- HTTP/client traits after transport has been abstracted.
- External I/O boundaries where call count or arguments matter.

## Guidelines

- Mock the boundary, not the logic under test.
- Assert only calls that matter to the behavior.
- Keep expectation setup close to the test.
- Prefer fakes when stateful behavior is easier to read.

Example:

```rust
#[automock]
trait UserStore {
    fn load(&self, id: UserId) -> Result<Option<User>, StoreError>;
}
```

Do not add a trait solely because a concrete type is inconvenient to construct.
First ask whether the design needs a boundary.
