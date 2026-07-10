# Research Checklist

Goal: avoid guesswork and unnecessary custom infrastructure.

## When To Research

Research before adding new capabilities involving protocols, storage,
concurrency, cryptography, security, cross-platform behavior, or new
dependencies. Research during development when implementation complexity grows
or the tradeoff is unclear.

## Minimum Research Loop

1. Define the problem and constraints.
2. List at least two candidate approaches.
3. Compare libraries using docs, maintenance activity, API shape, feature
   surface, MSRV, and license.
4. Build a small proof of concept when the risk is not obvious.
5. Record the decision and rollback path.

## ADR Template

```md
# ADR: <topic>

## Context

What problem are we solving? What constraints matter?

## Decision

What did we choose?

## Alternatives

What did we reject and why?

## Consequences

Benefits, risks, and rollback plan.

## References

Links to docs, issues, benchmarks, or examples.
```

## Rust Sources

- Cargo Book for workspace and dependency behavior.
- docs.rs and crates.io for library APIs.
- Project issue trackers for maintenance and edge cases.
- Rust Performance Book for performance work.
