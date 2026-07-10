# Architecture

This template uses a small workspace to keep responsibilities visible from the
start. The default crates are intentionally simple and should be renamed or
expanded to match the project domain.

## Crates

```text
crates/
  domain/
  app/
  infra/
  cli/
```

`domain` owns business types, value objects, invariants, and pure rules. It
should not depend on infrastructure, user interfaces, process environment,
filesystems, network clients, or concrete persistence.

`app` owns use cases and orchestration. It may define port traits for external
capabilities that a use case needs. It should depend on `domain`, but should not
know which concrete adapter will satisfy a port.

`infra` owns concrete implementations for app ports, such as filesystem,
database, HTTP, environment, or process adapters. It may depend on `app` and
`domain`.

`cli` owns command-line entrypoints, argument parsing, dependency wiring, and
process-level behavior. Keep business decisions out of the binary crate.

## Dependency Direction

The intended dependency direction is:

```text
cli -> app -> domain
cli -> infra -> app
```

Avoid reverse dependencies. If `domain` needs something from `infra`, define a
domain or app concept that can be implemented by infra instead.

## Adding Crates

Add a new crate when it creates a clear ownership boundary, reduces coupling, or
prevents a central crate from becoming a catch-all. Do not add a crate only to
avoid a small module.

Good reasons to add a crate:

- A feature has independent public types and tests.
- A dependency should not leak into the rest of the workspace.
- A boundary will make future replacement or testing easier.
- A central crate is growing beyond a focused responsibility.

Prefer private modules and explicit public exports. Public APIs should describe
the intended use, not expose implementation details.
