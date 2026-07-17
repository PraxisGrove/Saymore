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

Desktop startup is shared across macOS and Windows. It resolves application
paths, opens provider settings and local storage, loads local settings, and
wires the shared Slint settings, history, dictionary, statistics, ASR, and
dictation completion modules before platform-specific capabilities are attached.
Concrete audio capture, permissions, global shortcuts, text delivery, window
behavior, and system settings actions remain narrow adapters rather than one
aggregate platform service. A platform that does not yet implement one of those
adapters must return an explicit unavailable error; it must not replace the
shared UI or bootstrap with a platform-specific application flow.

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

### Windows global shortcuts

The application layer stores dictation shortcuts as opaque, non-empty strings.
Platform adapters own parsing and registration. Existing macOS values remain
unchanged, including `right-command`, `fn`, and numeric `key-*` combinations.
Standard Windows combinations use the namespaced canonical form
`windows:<modifiers>+<key>`, with modifiers ordered as `control`, `alt`,
`shift`, and `windows`. The single-modifier default has the explicit form
`windows:right-alt`. A Windows adapter rejects legacy macOS values instead of
interpreting their key codes.

A fresh Windows database is initialized with `windows:right-alt`; existing
databases are not rewritten. If Windows opens an older database whose shortcuts
contain no valid Windows value, the desktop runtime safely falls back to that
same default without changing the stored value. This preserves customized data
while keeping startup usable.

Right Alt is the product default because it is a short, one-key dictation
gesture. On keyboard layouts where Right Alt acts as AltGr, using AltGr may also
trigger dictation; users on those layouts should configure a standard
combination instead.

The Windows monitor owns `RegisterHotKey` registrations and a message loop on a
dedicated thread. Standard combinations use `RegisterHotKey`; the Right Alt-only
binding uses a narrowly scoped `WH_KEYBOARD_LL` hook because `RegisterHotKey`
cannot represent a single modifier key. A settings change first registers
additions while retaining the old OS registrations. The new set becomes active
immediately, but removed bindings stay reserved until SQLite confirms the FIFO
settings mutation. Success releases the old bindings; failure releases additions
and reactivates the old set. Capture is limited to a short-lived key-state
sampler, suppresses shortcut actions while active, and ends after completion,
Escape, or runtime shutdown.

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
