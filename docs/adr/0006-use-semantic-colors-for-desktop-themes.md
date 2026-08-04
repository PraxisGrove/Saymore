# ADR 0006: Use semantic colors for desktop themes

- Status: Accepted
- Date: 2026-07-20

## Context

Saymore needs a neutral default theme, five user-selectable color themes, and
both light and dark interfaces. Direct color values in individual Slint
components made it unclear which surfaces should change together, produced
inconsistent dark-mode results, and made future theme previews expensive to
implement safely.

The main application window should react to theme choices. Recording controls,
permission prompts, and compact result notifications are independent overlays
whose neutral surfaces and legibility must remain stable while their accents
follow the selected theme.

## Decision

Represent appearance as two independent persisted settings:

- `ThemeId`: Saymore (`saymore`), Lime Pulse (`lime-pulse`), Warm Clay, Berry
  Graphite, Iris Mist, or Sunlit Gold. Saymore is the default for new
  installations and uses neutral surfaces with blue interaction accents and
  green success states.
- `ColorSchemePreference`: follow the operating system, light, or dark.

`crates/app` owns these closed types and storage identifiers. SQLite stores both
values in `app_settings`; the desktop maps them to Slint enums and updates the
UI only after the settings mutation commits.

The main window consumes semantic roles from
`apps/desktop/ui/color-system.slint`. A theme changes accent roles and the
coordinated dark palette, while layout and component behavior remain unchanged.
Sunlit Gold uses a bright sunflower-yellow accent with dark foregrounds and
neutral surfaces; its warning roles shift toward orange-red so warnings remain
distinct from theme-colored actions. Accent fills use `brand` and `brand-hover`;
foreground accents and focus outlines use `brand-strong`, including on light
accent surfaces. The Saymore theme keeps large areas neutral, uses blue for
primary actions, enabled controls, links, focus, and active data, and reserves
green for ready, healthy, and successful states, including automatically learned
dictionary entries. Its text hierarchy reserves `ink` for selected content, page
and dialog titles, key values, and other high-priority information; ordinary
content uses `text`, while subtitles, descriptions, and helper information use
`text-muted`. Windows title-bar colors use the resolved canvas and ink roles.

Independent overlays consume roles in
`apps/desktop/ui/overlay-color-system.slint`. Their surfaces, borders, text,
shadows, and recording-control neutrals remain fixed across theme and color
scheme changes. Accent, strong accent, soft accent, on-accent foreground, and
recording activity accents resolve from the selected theme. The theme picker may
declare five literal swatch samples and use the monochrome Saymore mark for the
default theme; all other Slint components must use one of the two color systems.
The `xtask ui-colors` gate enforces that boundary.

## Consequences

- A new theme requires a complete semantic palette, not scattered component
  edits.
- Dark mode is explicit and testable; following the system is a separate stored
  preference rather than a one-time color copy.
- Overlay structure and neutral surfaces remain stable across theme switches;
  emphasis follows the selected theme.
- SQLite migration and translation coverage are required when theme identifiers
  or user-facing names change.
- Visual verification must cover all six light themes and representative dark
  themes at the 920×700 default window size.

## Alternatives Considered

- Keep hard-coded component colors: simplest initially, but cannot produce a
  coherent dark system or reliably apply future themes.
- Theme every overlay role: visually uniform, but recoloring transient surfaces
  and text makes system-facing controls less predictable and broadens the
  regression surface.
- Store a custom user color: flexible, but cannot guarantee contrast for every
  semantic role and is outside the current product scope.
