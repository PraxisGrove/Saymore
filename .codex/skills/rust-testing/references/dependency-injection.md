# Dependency Injection

Dependency injection keeps tests focused and prevents production code from
depending directly on external systems.

## Trait Boundaries

Define traits at the consumer boundary when a use case needs an external
capability:

```rust
pub trait UserStore {
    fn find_user(&self, id: UserId) -> Result<Option<User>, StoreError>;
}
```

Production implementations live in infrastructure crates. Tests can use fakes
or mocks.

## Guidelines

- Inject configuration, clocks, random generators, and I/O clients.
- Avoid hidden global singletons.
- Keep traits focused on the behavior the consumer needs.
- Do not create traits for every concrete type by default.

## Fakes Versus Mocks

Use a fake when stateful behavior is easier to understand than a list of mock
expectations. Use a mock when call count or exact parameters are part of the
contract.
