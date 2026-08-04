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
models, callback wiring, and process lifecycle for the macOS and Windows app. It
may depend on `app` and `infra`; those reusable crates must not depend on Slint.

Desktop appearance has two application-owned persisted values: a six-option
theme identifier and a follow-system/light/dark preference. The desktop maps
those values to Slint's `AppColors` semantic roles; pages never select raw
colors themselves. Independent recording, permission, and result overlays use
`OverlayColors`. Their neutral surfaces remain fixed while their accent roles
follow the main-window theme. `xtask ui-colors` enforces this ownership
boundary.

Dictionary edits cross the application-owned `DictionaryStore` port by stable
entry identity. Storage adapters preserve an entry's language, origin, and
creation time, while rejecting empty spellings, unknown identities, and
normalized duplicates. Each dictation snapshots those canonical spellings as
vocabulary hints for recognizers that support them and as trusted candidates for
final LLM refinement. A non-empty personal dictionary also allows short
transcripts through refinement so phonetic ASR substitutions can be corrected,
while the output guard permits only listed canonical technical terms and
continues to protect existing URLs, commands, versions, and technical fragments.

Home-page usage totals cross the application-owned `UsageStore` port as
privacy-safe duration and character-count aggregates. SQLite records each saved
dictation identity once, increments a local-calendar daily bucket in the same
transaction as the first encrypted history insert, and never stores recognized
text in the usage tables. Deleting, expiring, clearing, or cryptographically
resetting transcript history does not subtract prior usage. Databases upgraded
from the history-derived implementation backfill retained history once before a
clear, delete, retention cleanup, or the first usage read.

Main-window modal dialogs share the Slint `ModalShell` component. The shell owns
the scrim, Pencil-aligned viewport positioning, standard, compact, and tall
sizes, surface border, corner radius, shadow, and close control. Standard
dialogs use the design's 36 px horizontal offset from the viewport center; tall
dialogs use the same offset, while compact confirmations remain centered. Dialog
components own only their header, scrollable body, and fixed footer actions.
Independent recording, permission, and result windows remain outside this modal
system.

Desktop startup is shared across macOS and Windows. It resolves application
paths, opens provider settings and local storage, loads local settings, and
wires the shared Slint settings, history, dictionary, statistics, ASR, and
dictation completion modules before platform-specific capabilities are attached.
LLM model discovery results live inside the owning Provider instance in that
same Provider catalog. Each cached directory is scoped by its model-list
endpoint and chat-completions profile, and stores the selected model and refresh
timestamp. The JSON adapter updates the catalog under one lock and atomically
replaces the file only after a successful refresh; failed refreshes leave both
the persisted directory and the currently displayed list intact. Provider
configuration saves and LLM enablement changes preserve this cache, while
endpoint or profile changes make an unrelated cache ineligible for restoration.
Selecting a model from a successfully loaded directory atomically updates both
the Provider model and the directory selection so reopening that Provider
restores the user's last choice. Provider configuration ordering is
application-owned. The `ProviderConfigurator` tests an ASR or LLM candidate
through `ProviderConnectionTester` and only then commits it through
`ProviderConfigurationStore`; connection failures therefore cannot overwrite the
saved candidate. The JSON adapter commits ASR changes under one settings lock
and commits an LLM candidate, selection, enablement, and data consent together.
Desktop settings callbacks collect draft fields, present remote data-consent
confirmation and errors, and provide the concrete connection-test adapter
without reproducing the test-before-save rule. When automatic update checks are
enabled, startup also queries the latest stable GitHub Release. A newer version
is presented as a dismissible in-window notice; manual checks continue to report
their result only in Settings. Concrete audio capture, permissions, global
shortcuts, text delivery, window behavior, and system settings actions remain
narrow adapters rather than one aggregate platform service. A platform that does
not yet implement one of those adapters must return an explicit unavailable
error; it must not replace the shared UI or bootstrap with a platform-specific
application flow.

Streaming ASR remains an application-owned port. The Paraformer implementation
in `infra` loads its pinned INT8 files once, then creates an isolated online
sherpa-onnx stream for each dictation session. Audio pushes emit only changed
non-empty partials; finishing drains final decoding and rejects an empty result.
The Whisper large-v3-turbo and Qwen3-ASR 1.7B implementations instead buffer
session PCM and use pinned sherpa-onnx offline recognizers at finish, with
ordered segmentation and no fabricated partial results. Qwen3-ASR uses a
one-second overlap plus conservative transcript-boundary deduplication for audio
longer than 30 seconds so hard cuts do not repeat or truncate words.
Cancellation drops any local-model session without finalizing it. The macOS
desktop Provider lifecycle selects an adapter only after its pinned model has
downloaded, passed size and SHA-256 validation, loaded successfully, and been
recorded in local storage. The manifest-driven infrastructure installer keeps
resumable partial files in a stable model-specific staging directory, supports
cooperative pause and cancel, and atomically renames that directory only after
every artifact validates. The desktop owns per-model UI progress and removes
staging files after an explicit cancel. A non-Slint desktop lifecycle module
owns installed-model metadata reconciliation, guarded Provider activation,
previous-runtime release, and active-model deletion protection through
application ports and infrastructure adapters. Slint callbacks submit lifecycle
actions and present their results; they do not reproduce the ordering or
recovery rules. Installation remains separate from Provider selection: a
completed download never loads or selects the model until the user explicitly
chooses it. The desktop also owns failure presentation and runtime memory
sampling. An installed model also carries a verification marker containing its
pinned identity and file metadata. Unchanged installations use that marker for a
fast startup check; missing or changed metadata falls back to full SHA-256
validation.

macOS text delivery is an incremental main-thread state machine. Focus settling,
accessibility verification, and clipboard restoration waits are represented as
delayed steps driven by the Slint event loop; each native step returns promptly
so processing animations continue to render. AppKit and Accessibility work must
remain on the main thread, and must not introduce sleeps or polling loops there.

System-output muting is also a narrow platform adapter. The desktop owns the
recording-scoped mute session so restoration occurs on stop, cancellation,
startup cleanup, or shutdown. Platform implementations restore only state still
owned by that session and preserve output changes made by the user while
recording.

On macOS, Winit owns the standard application menu and Command-Q termination.
The macOS application-menu adapter adds the standard Window menu so Command-W
routes through AppKit to the desktop's existing close-request handler, which
hides the main window without terminating the resident process.

The Windows dictation slice reuses the shared `CpalAudioRecorder`, recording
state machine, ASR session, `DictationCompletion`, and Slint overlays. Its
narrow infra adapters own AppCapability microphone checks, RegisterHotKey
lifecycle, UI Automation target classification, clipboard restoration, SendInput
paste, and Win32 nonactivating overlay styles. Windows adapters do not read
complete documents into application storage. For correction learning, the
delivery STA worker transiently reads an observable non-sensitive control,
derives bounded anchors around only the text Saymore just inserted, and polls
that anchored segment for at most 30 seconds. Password fields and controls
without a readable UI Automation Value or Text pattern are never observed;
unobservable paste remains an explicitly attempted outcome. UI Automation and
OLE clipboard work runs on a dedicated STA worker with explicit shutdown.
Clipboard restoration preserves the original OLE data object only while
Saymore's temporary Unicode text remains current, so a concurrent user copy is
not overwritten.

Windows local integration remains split into narrow adapters. Provider JSON uses
the shared schema and migration module, while its filesystem adapter applies a
protected owner/SYSTEM DACL and replaces complete, synced temporary files with
`MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`. History keys use Credential
Manager through keyring's `windows-native` backend; production and development
use distinct stable services. Launch at login uses an environment-specific value
under the current user's `Run` key and starts with `--autostart`, so the
existing window stays hidden while the shared tray remains available.

The desktop owns Windows window lifecycle. Closing the main window hides the
existing Slint window, tray actions reopen that same window, and explicit tray
quit ends the event loop so shortcut monitors can drop cleanly. A
per-environment named activation event lets a second process ask the existing
instance to show its window. Recording and result windows remain nonactivating
Win32 tool windows and therefore do not appear in the taskbar or Alt+Tab.

### Local diagnostics

Diagnostic logging is enabled by default for new installations and remains a
user-controlled local setting. The desktop tracing boundary accepts only events
with an explicit `event` field, then strips every other field before
persistence. Recognized text, provider credentials, device names, paths, and
error details therefore never enter diagnostic storage even when they are
present in transient runtime tracing fields.

Each safe event identifier is written to a private bounded rolling log and to a
bounded SQLite `diagnostic_events` table. The SQLite copy is the primary source
for an exported diagnostic report; the rolling files are the fallback when the
database is unavailable or contains no events. Reports expose only validated
event identifiers and can be exported whether diagnostic collection is currently
enabled or disabled.

### macOS global shortcuts

Shortcut capture and persistence do not require Accessibility permission. A
local AppKit event monitor captures keys delivered to the focused settings
window and preserves the physical side of standalone modifiers. When Saymore is
not trusted, a read-only key-state sampler remains as a capture fallback,
including when no shortcut is currently configured; it retains the first
observed modifier side when the system later reports an alias. Both paths accept
physical-key chords and standalone modifier releases, and Escape cancels
capture. The sampler also detects attempts to use an already configured shortcut
only to surface the permission prompt; it must not start dictation without
authorization.

Once Accessibility permission is available, the HID event tap owns global
shortcut activation and suppression. Preview and release bundles use the same
capture behavior; their distinct identities affect only whether macOS has
granted each bundle permission to activate shortcuts globally.

### Windows global shortcuts

The application layer stores each configured dictation shortcut as an opaque,
non-empty string, while the collection itself may be empty when the user
disables all shortcuts. Platform adapters own parsing and registration. Existing
macOS values remain unchanged, including `right-command`, `fn`, and numeric
`key-*` combinations. Standard Windows combinations use the namespaced canonical
form `windows:<modifiers>+<key>`, with modifiers ordered as `control`, `alt`,
`shift`, and `windows`. The single-modifier default has the explicit form
`windows:right-alt`. A Windows adapter rejects legacy macOS values instead of
interpreting their key codes.

A fresh Windows database is initialized with `windows:right-alt`; existing
databases are not rewritten. If Windows opens an older database whose shortcuts
contain no valid Windows value, the desktop runtime safely falls back to that
same default without changing the stored value. This preserves customized data
while keeping startup usable.

An explicitly saved empty collection is not treated as invalid legacy data and
must remain empty across restarts. Both platform monitors then register no
dictation trigger, and the home page presents the shortcut state as disabled.

Right Alt is the product default because it is a short, one-key dictation
gesture. On keyboard layouts where Right Alt acts as AltGr, using AltGr may also
trigger dictation; users on those layouts should configure a standard
combination instead.

The Windows monitor owns `RegisterHotKey` registrations and a message loop on a
dedicated thread. Standard combinations use `RegisterHotKey`; the Right Alt-only
binding uses a narrowly scoped `WH_KEYBOARD_LL` hook because `RegisterHotKey`
cannot represent a single modifier key. While that binding is active and Saymore
is enabled, the hook consumes both halves of each Right Alt press so foreground
applications do not also act on the dictation gesture. It restores pass-through
while Saymore is paused or shortcut capture is active. A settings change first
registers additions while retaining the old OS registrations. The new set
becomes active immediately, but removed bindings stay reserved until SQLite
confirms the FIFO settings mutation. Success releases the old bindings; failure
releases additions and reactivates the old set. Capture is limited to a
short-lived key-state sampler, suppresses shortcut actions while active, and
ends after completion, Escape, or runtime shutdown.

Shortcut storage values are stable platform identifiers, not display text. The
current UI uses the platform adapters' English labels consistently. A future
localized-keyboard-label change must expose structured key and modifier names
instead of special-casing individual storage values. Every added locale must
cover the complete named-key vocabulary in the translation build validation so
one shortcut cannot mix localized and fallback-English parts.

`xtask` owns repository maintenance, local preview and ad-hoc bundle workflows,
and size-gate commands. Formal distribution metadata lives with the desktop
package; GitHub Actions coordinates native runners and `cargo-packager` for
signed release artifacts.

## Dependency Direction

The intended dependency direction is:

```text
desktop -> app
desktop -> infra -> app
```

Avoid reverse dependencies. If `app` needs an external capability, define an app
port that can be implemented by `infra` instead.

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
