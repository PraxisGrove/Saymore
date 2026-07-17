# Local Settings Mutation Design

Date: 2026-07-17
Status: Approved for implementation

## Context

Saymore persists a small set of local product preferences through
`LocalSettingsStore`. Desktop callbacks currently coordinate each mutation by
sharing an `Arc<Mutex<()>>` and repeating the same load, mutate, save, thread,
and failure-dispatch protocol. The guard is constructed in `main` and leaks into
settings, local data, onboarding, language, shortcut, and status-tray modules.

SQLite already has one bounded worker that owns its connection, as required by
ADR-0004. That worker serializes individual load and save commands, but it does
not make a two-command read-modify-write sequence atomic. Removing the desktop
guard without replacing its responsibility would allow concurrent changes to
overwrite unrelated fields.

This design deepens local settings mutation into one application module and one
desktop runtime. Callers submit typed intent and apply the committed result;
they do not coordinate persistence or scheduling.

## Goals

- Make every local settings write pass through one small interface.
- Preserve unrelated settings during concurrent changes.
- Apply changes in the order desktop callbacks submit them.
- Keep storage coordination, validation, and persistence local to the app
  module.
- Keep Slint scheduling and process-thread ownership local to desktop.
- Preserve ADR-0004 connection ownership and the existing SQLite schema.
- Preserve the current user-visible success, failure, and rollback behavior.

## Non-Goals

- Changing provider configuration stored in `config.json`.
- Owning operating-system settings such as launch at login.
- Moving Dock, translation, recorder, shortcut-controller, diagnostics, or tray
  side effects into `app`.
- Replacing read-only `load_settings` calls used during startup, refresh, or
  dictation completion.
- Changing the SQLite schema, migrations, or connection worker.
- Coalescing queued changes or adding retries.

## Application Module

Add `crates/app/src/local_settings_mutation.rs`. Its public interface consists
of `LocalSettingsChange`, `LocalSettingsMutator`, and
`LocalSettingsMutationError`.

`LocalSettingsMutator` owns an `Arc<dyn LocalSettingsStore>` and a private
`Mutex<()>`. Its synchronous operation is conceptually:

```rust
fn apply(
    &self,
    change: LocalSettingsChange,
) -> Result<LocalSettings, LocalSettingsMutationError>;
```

`apply` validates the change before storage access, locks the private
coordinator, loads the latest settings, exhaustively applies one change,
validates the resulting combination, saves it, and returns the committed full
snapshot. The lock remains held across load and save. A failed save never
returns the candidate snapshot as committed.

The closed change type covers the existing write intents:

- set history enabled;
- set history policy as the enabled flag and retention value together;
- select a microphone by identifier and display name, or return to automatic
  selection;
- set UI language;
- set automatic update checks;
- set feedback sounds;
- set copy-to-clipboard behavior;
- set Dock visibility;
- set dictation pause state;
- set diagnostics logging;
- replace the complete dictation shortcut collection;
- set onboarding status and step together.

The module rejects an empty shortcut collection before loading settings.
Microphone selection uses a closed value type with `Automatic` and
`Specific { id, name }` variants, so a selection with only an identifier or only
a display name cannot be constructed. It rejects blank identifiers and names.
Other inputs are already constrained by application enums. It does not
interpret platform shortcut strings because that belongs to the platform
adapter.

`LocalSettingsMutationError` distinguishes invalid changes, storage errors, and
an unavailable mutation module. Lock poisoning is an implementation detail and
does not appear in the public error vocabulary. Desktop logs the specific error
while retaining the existing localized save-failure messages.

## Desktop Runtime

Add `apps/desktop/src/local_settings_runtime.rs`. `main` constructs one owning
runtime from the shared `LocalSettingsMutator` and stores it in `WiredCore`.
Callers receive a cloneable submission handle instead of `SqliteStorage` plus
`settings_guard` for writes.

The runtime owns one named worker thread and a bounded FIFO channel with capacity
32, enough to cover every currently exposed settings control in one pending
burst while retaining explicit backpressure. Submission uses `try_send`, so the
Slint event loop never blocks on a full or closed queue. Each work item contains
a `LocalSettingsChange` and a one-shot completion closure. The worker calls
`apply` sequentially and schedules each completion once on the Slint event loop.
If the event loop is already closed during shutdown, dispatch failure is logged
and the completion is not retried.

The handle reports queue saturation and worker shutdown distinctly. A mutation
failure completes only that item; the worker continues with the next queued
change, which reloads the last committed snapshot. The runtime does not retry or
reorder changes.

Handles do not own independent sender clones. They share a short-lived lock over
an optional sender. Submission and shutdown use that same state: submission
calls `try_send` while the sender is present; shutdown atomically takes and drops
the sender, preventing later acceptance. The disconnected worker drains every
previously accepted item before exiting. The owning runtime then joins it. This
makes "accepted" mean that the item is processed exactly once even when submit
and shutdown race.

## Caller Responsibilities

Desktop callers express intent and handle committed results:

1. A callback captures any previous UI or runtime value needed for rollback.
2. It optionally applies an optimistic platform or runtime side effect.
3. It submits one typed change.
4. On success, it applies only the committed state relevant to that change and
   any success-only side effect. A full-view refresh may use the snapshot only
   when it cannot overwrite a newer pending user intent.
5. On failure, it restores optimistic state and displays the existing localized
   error.

This preserves the current ordering choices:

- Dock visibility and shortcuts remain optimistic and roll back on failure.
- Pause state remains optimistic and rolls back in the tray and atomic state.
- UI language, feedback-sound state, diagnostics state, and microphone recorder
  selection change only after persistence succeeds.
- Enabling automatic update checks invokes an update check only after success.
- History settings refresh the existing history-facing UI from the committed
  snapshot. After a successful retention change, the caller runs history
  cleanup before refreshing history and usage. A cleanup failure does not roll
  back or report the already committed setting as failed.
- Onboarding persists status and step as one change.

Operating-system launch-at-login changes do not use this runtime because they
are not `LocalSettings` writes.

## Data Flow

```text
Slint callback
    -> LocalSettingsHandle::submit(change, completion)
    -> bounded desktop FIFO worker
    -> LocalSettingsMutator::apply(change)
       -> LocalSettingsStore::load_settings()
       -> validate and mutate latest snapshot
       -> LocalSettingsStore::save_settings(snapshot)
    -> Slint event-loop completion
    -> apply committed state or rollback caller-owned side effect
```

The app module is the mutation interface and test surface. The SQLite storage
adapter remains the persistence seam. The desktop runtime is process wiring, not
a new trait or externally replaceable seam.

## Migration Sequence

1. Add failing app contract tests and implement the mutation module.
2. Add failing desktop scheduling tests and implement the FIFO runtime.
3. Construct the runtime in `main`.
4. Migrate local data history and microphone writes.
5. Migrate general settings and shortcut writes.
6. Migrate UI language and status-tray pause writes.
7. Migrate onboarding persistence.
8. Delete duplicated write helpers, per-write settings threads, and
   `local_settings_guard`.
9. Confirm by literal search that no desktop code directly calls
   `save_settings`.

The application module is one independently compiling stage. The desktop
runtime and caller replacement form one compilation-coherent stage: splitting
that interface replacement across commits would either leave incompatible
function interfaces or require a temporary dual-write compatibility path. The
latter would weaken the single mutation interface this migration establishes,
so the desktop replacement lands atomically while retaining its focused module
and caller-file boundaries for review.

Read-only settings loads remain at their existing ownership locations unless a
small call-site adjustment is required by the migration.

## Testing

Application integration tests in
`crates/app/tests/local_settings_mutation.rs` exercise the public interface with
an in-memory fake store. They cover:

- a complete expected snapshot for each change variant;
- preservation of unrelated fields;
- atomic history policy, microphone selection, and onboarding progress changes;
- rejection of invalid shortcut and microphone combinations before save;
- save failure returning an error rather than a candidate snapshot;
- concurrent mutations completing without lost updates.

Desktop module tests exercise the private worker protocol. They cover FIFO
processing, continuation after one failed mutation, full/closed queue errors,
atomic submit-versus-shutdown behavior, draining accepted work during shutdown,
dispatcher failure, and one completion per accepted item while the event-loop
dispatcher remains available. They use deterministic gates rather than sleeps.
UI-specific callbacks retain focused tests only where observable behavior can be
tested without exposing production APIs.

During implementation, run focused tests and checks for `template-app` and
`saymore-desktop`, plus `cargo fmt --all --check`. The full workspace gate is not
required unless the branch is about to be pushed.

## Acceptance Criteria

- `settings_guard` and `local_settings_guard` no longer exist.
- Desktop production code has no direct `save_settings` call.
- Every local settings mutation uses `LocalSettingsChange`.
- App tests prove read-modify-write atomicity and validation through the public
  interface.
- Desktop tests prove FIFO completion behavior and failure isolation.
- Existing settings, onboarding, language, shortcut, microphone, Dock, pause,
  and diagnostics behavior is preserved.
- Focused formatting, tests, and checks pass.
