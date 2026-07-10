# Refactoring

Goal: reduce technical debt while preserving behavior and keeping review
manageable.

## Policy

When the project is a template or has no production compatibility constraints,
prefer clean replacement over compatibility layers. Do not keep old modules,
aliases, or paths unless the user explicitly asks for migration compatibility.

## Process

1. Define the target boundary.
2. Identify affected crates, public APIs, and call sites.
3. Move behavior into the correct crate or module.
4. Add or update tests around observable behavior.
5. Delete the old path.
6. Run all quality gates.

## Change Size

- Keep behavior-changing refactors under roughly 500 changed lines when
  possible.
- Split larger work into coherent stages.
- Avoid mixing refactor, dependency changes, and behavior changes unless they
  are inseparable.

## Review Checklist

- Does the new boundary reduce coupling?
- Did any public API grow unnecessarily?
- Are tests still focused on behavior?
- Did the change remove old code instead of leaving a compatibility layer?
