# Architecture

Saymore uses a small workspace to keep responsibilities visible. Crates should
be renamed or expanded only when a product boundary needs it.

## Crates

```text
crates/
  app/
  infra/
  xtask/
apps/
  desktop/
```

`app` owns business types, invariants, pure rules, use cases, orchestration, and
port traits for external capabilities. It must not know which concrete adapter
will satisfy a port or depend on UI and operating-system implementations.

`infra` owns concrete implementations for app ports, such as filesystem,
database, HTTP, environment, or process adapters. It may depend on `app`.

`apps/desktop` owns the Slint entrypoint, compiled `.slint` components, UI view
models, callback wiring, and process lifecycle for the macOS and Windows app.
It may depend on `app` and `infra`; those reusable crates must not depend on
Slint.

`xtask` owns repository maintenance, preview, packaging, and size-gate commands.

## Dependency Direction

The intended dependency direction is:

```text
desktop -> app
desktop -> infra -> app
```

Avoid reverse dependencies. If `app` needs an external capability, define an
app port that can be implemented by `infra` instead.

## Adding Crates

Add a new crate when it creates a clear ownership boundary, reduces coupling, or
prevents a central crate from becoming a catch-all. Do not add a crate only to
avoid a small module.

In particular, do not recreate a `domain` crate until pure business concepts
form a substantial reusable interface distinct from application use cases.

Good reasons to add a crate:

- A feature has independent public types and tests.
- A dependency should not leak into the rest of the workspace.
- A boundary will make future replacement or testing easier.
- A central crate is growing beyond a focused responsibility.

Prefer private modules and explicit public exports. Public APIs should describe
the intended use, not expose implementation details.
