# Development

The base workflow uses Cargo plus `cargo-nextest` and `cargo-deny`. Optional
tools can improve local ergonomics, but the mature-project gate should stay
explicit and reproducible.

## Verification Cadence

During implementation, run focused tests and checks for the code being changed.

### UI color ownership

All main-window Slint components must consume semantic roles from
`apps/desktop/ui/color-system.slint`. Independent overlays consume the fixed
roles in `apps/desktop/ui/overlay-color-system.slint`, so changing the app theme
does not recolor permission prompts, recording controls, or result
notifications. The theme picker may declare its five swatch samples directly; no
other Slint component may contain a color literal.

Run the color boundary check with:

```sh
cargo run -p xtask -- ui-colors
```

Run the complete gate below immediately before `git push`:

```bash
cargo fmt --all --check
cargo check --workspace --all-targets
cargo nextest run --workspace --all-targets
cargo test --workspace --doc
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
cargo build --workspace --all-targets --release
cargo run -p xtask -- size
```

Use `cargo fmt --all` when you want to apply formatting.

After the gate passes, review the complete change being pushed for both
repository-standard compliance and fidelity to the originating request or spec.
Ordinary task completion does not require this full gate or dual-axis review.

## Desktop Toolchain

The target desktop app is compiled through Cargo and Slint. Node.js, pnpm,
React, TypeScript, Vite, and Tauri are not part of the production toolchain.

The desktop crate's Rust build script must compile its `.slint` files so the
standard `cargo check` and `cargo build` commands validate both UI declarations
and Rust code.

## Desktop Preview

There are two macOS app workflows:

- `./scripts/dev-preview.sh` builds, installs, and watches the debug
  `Saymore Preview.app`.
- `cargo run -p xtask -- bundle-macos` creates the release-profile `Saymore.app`
  bundle.

Both workflows compile the same `saymore-desktop` package and the same Rust and
Slint sources. Preview differs only in build profile, app identity, signing, and
development data paths; UI and behavior changes are therefore included the next
time the release bundle is built.

Use the persistent preview while iterating on the desktop UI:

```bash
./scripts/dev-preview.sh
```

The preview installs a debug build at `/Applications/Saymore Preview.app` with
the stable bundle identifier `com.saymore.desktop.preview`. It also creates and
reuses a local code-signing identity under
`~/Library/Application Support/Saymore Dev/preview-signing`, because macOS TCC
does not preserve Accessibility authorization across rebuilt ad-hoc binaries.
Grant the Preview app microphone and Accessibility permission once. Saving a
Rust, Slint, Cargo, font, icon, or audio change performs an incremental debug
build and restarts the preview app without changing that authorization identity.
A failed build leaves the current preview open. `target/debug/saymore-desktop`
is only an intermediate Cargo artifact; do not launch it as a separate preview
app because it does not have the Preview bundle's stable macOS permission
identity.

The signing identity is self-signed, local to the development machine, and used
only for `Saymore Preview.app`; it does not replace release signing. On its
first creation, macOS asks for user authentication once to trust that
certificate for local code signing. The first Preview run after migrating from
the old ad-hoc workflow also requires Accessibility to be enabled once again for
the new stable identity.

Do not distribute `Saymore Preview.app` to another Mac. Its certificate is
trusted only on the machine that created it. Cross-machine testing must use the
same Developer ID signed application identity as the release build. An unsigned
or newly ad-hoc-signed replacement cannot promise that macOS will carry
Microphone or Accessibility authorization forward, even when its bundle
identifier and display name are unchanged.

The Preview bundle is also the development environment. Its bundle marker forces
the app to use `~/Library/Application Support/Saymore Dev`, the development
history key, development Provider configuration, and a separate instance lock.
It can run alongside `/Applications/Saymore.app`; refreshing Preview only stops
the Preview process. This workflow never writes production local data, performs
a release build, or overwrites `/Applications/Saymore.app`.

The two app identities currently listen for the same global Right Command
shortcut. Their storage and processes can coexist, but close one app before a
dictation test so both do not react to the same shortcut.

Create the local ad-hoc signed release bundle with:

```bash
cargo run -p xtask -- bundle-macos
```

If `just` is installed, `just preview` and `just release` are optional aliases
for these two commands.

### Windows packaging preview

The formal Windows application icon uses the black-background master at
`apps/desktop/icons/icon-master.png`. The running main window uses the separate
transparent `apps/desktop/icons/taskbar-master.png`, matching the tray icon.
Regenerate and validate both multi-resolution icons without external source
paths:

```bash
cargo run -p xtask -- windows-icons
cargo run -p xtask -- windows-icons --master apps/desktop/icons/taskbar-master.png --output apps/desktop/icons/taskbar.ico
```

On Windows, a local NSIS preview uses the same commands as the release workflow:

```bash
cargo build -p saymore-desktop --release
cargo packager --release --packages saymore-desktop --formats nsis --binaries-dir target/release --out-dir dist
Move-Item dist\saymore-desktop_*_*-setup.exe dist\Saymore-Setup.exe
```

The desktop build uses `apps/desktop/icons/taskbar.ico` as the executable's
default icon so the Start menu, desktop shortcut, and taskbar grouping all use
the transparent logo. The same icon is embedded separately for the running main
window. `cargo-packager` uses the formal black-background `icon.ico` for the
installer executable and transparent-logo artwork from `taskbar-master.png` for
the NSIS header/sidebar images, while retaining the `.icns` asset for macOS.
Release packaging renames the user-facing artifact to `Saymore-Setup.exe`.

NSIS does not receive automatic translations from Windows. `cargo-packager`
defaults to English and can select the current Windows UI language only when the
corresponding NSIS language is explicitly bundled. Each added language must be
visually verified at supported DPI scales; the stock Simplified Chinese welcome
copy currently needs a shorter custom template before it can be enabled without
line overlap at 150% scaling.

The ad-hoc bundle is for local testing only. Formal signed and notarized DMGs
are built by the release workflow described in `docs/releasing.md`.

## Optional Shortcuts

If `just` is installed, the `justfile` provides shortcuts for the same commands:

```bash
just ci
just test
just test-doc
just clippy
just deny
just size
```

Do not document `just` as a required setup step. CI should use Cargo directly.

## Dependency Changes

Declare shared versions in `[workspace.dependencies]` in the root `Cargo.toml`.
Member crates should use `workspace = true` where possible.

When adding a dependency, include the reason in the change description. Prefer
small, maintained crates with a clear API and avoid broad feature sets unless
the project needs them.

## Size Gate

The size gate is intentionally approximate. Its job is to catch large files and
large functions early, especially during AI-assisted development where changes
can grow quickly.

Warnings should trigger a split plan. Errors should block the change unless
there is a documented reason and a short-term migration plan.
