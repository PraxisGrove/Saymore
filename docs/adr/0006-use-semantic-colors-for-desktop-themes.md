# ADR 0006: Use semantic colors for desktop themes

- Status: Accepted
- Date: 2026-07-20

## Context

Saymore needs five user-selectable color themes and both light and dark
interfaces. Direct color values in individual Slint components made it unclear
which surfaces should change together, produced inconsistent dark-mode results,
and made future theme previews expensive to implement safely.

The main application window should react to theme choices. Recording controls,
permission prompts, and compact result notifications are independent overlays
whose visual identity and legibility must remain stable while the application
theme changes.

## Decision

Represent appearance as two independent persisted settings:

- `ThemeId`: Warm Clay (`warm-clay`), Lime Pulse, Berry Graphite, Iris Mist, or
  Clear Sky.
- `ColorSchemePreference`: follow the operating system, light, or dark.

`crates/app` owns these closed types and storage identifiers. SQLite stores both
values in `app_settings`; the desktop maps them to Slint enums and updates the
UI only after the settings mutation commits.

The main window consumes semantic roles from
`apps/desktop/ui/color-system.slint`. A theme changes accent roles and the
coordinated dark palette, while layout and component behavior remain unchanged.
Light themes keep the sidebar and primary cards white. Windows title-bar colors
use the resolved canvas and ink roles.

Independent overlays consume the fixed roles in
`apps/desktop/ui/overlay-color-system.slint`. They do not read the selected
theme or color scheme. The theme picker may declare five literal swatch samples;
all other Slint components must use one of the two color systems. The
`xtask ui-colors` gate enforces that boundary.

## Consequences

- A new theme requires a complete semantic palette, not scattered component
  edits.
- Dark mode is explicit and testable; following the system is a separate stored
  preference rather than a one-time color copy.
- Overlay appearance remains stable across theme switches.
- SQLite migration and translation coverage are required when theme identifiers
  or user-facing names change.
- Visual verification must cover all five light themes and representative dark
  themes at the 920×700 default window size.

## Alternatives Considered

- Keep hard-coded component colors: simplest initially, but cannot produce a
  coherent dark system or reliably apply future themes.
- Theme every window, including overlays: visually uniform, but makes transient
  system-facing controls less predictable and broadens the regression surface.
- Store a custom user color: flexible, but cannot guarantee contrast for every
  semantic role and is outside the current product scope.
