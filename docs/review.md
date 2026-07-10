# Review

AI-assisted development makes it easy to produce large, plausible diffs. Review
rules keep changes small enough to understand and verify.

## Change Size

- Keep non-mechanical changes under roughly 500 changed lines when possible.
- Split changes over 800 lines into reviewable stages unless the diff is purely
  mechanical.
- Avoid adding behavior to files that already exceed the size gate.
- Prefer the smallest coherent stage that compiles and passes tests.

## Review Checklist

Check these before merging:

- Does the change belong in the crate where it was added?
- Did any public API grow unnecessarily?
- Are dependency changes justified?
- Are errors represented explicitly instead of hidden behind strings?
- Are tests focused on behavior rather than implementation details?
- Did the change avoid broad rewrites unrelated to the request?
- Did generated files or lockfiles change for a clear reason?

## Public API Changes

Public API changes should explain:

- Expected callers.
- Migration impact.
- Error behavior.
- Whether the API belongs in the current crate or a smaller boundary.

## Generated Code

Keep generated code separate from handwritten code. Do not manually edit
generated artifacts unless the generator is unavailable and the reason is
documented in the change.
