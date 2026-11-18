# Desktop UI Review

## Objective

Review the existing desktop UI page by page, record reproducible problems, and
fix each coherent group before expanding to the next page. The first pass owns
home-page layout at the default 920 × 700 and minimum 820 × 560 window sizes.

## Home Page Findings And Fixes

| Problem                                             | Evidence                                                                                                                                            | Resolution                                                                          |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| Usage content becomes unreachable in a short window | The usage card starts at y=313 and requires at least 300 px, but the minimum window leaves only 540 px for the page; there was no scroll container. | The home content now scrolls vertically below a 616 px content height.              |
| Status badges look detached from their card titles  | Separate rows placed the shortcut badge below its title and the overall configuration badge below Text delivery.                                    | Each header reserves space on the right for its badge. The shortcut title can wrap. |
| Shortcut instructions collide with Edit             | The instructions had no width bound while Edit was positioned at the right edge of the same row.                                                    | Instructions wrap within their own area, leaving room for Edit.                     |
| Retry covers the average-speed caption              | Retry started at y=105 inside the metric column; the metric caption starts at y=110.                                                                | Retry now occupies the usage card's top-right action area.                          |

Both summary cards use the same header. The shortcut keycap aligns with the
three configuration icons, and the shortcut instructions align with their
captions. Each caption is centered directly beneath its circular icon. The link
arrow sits to the right of the text within the same click target. The first and
last configuration items align with the header's left and right insets. The row
distributes the remaining width equally between the three items.

The headline also has a width bound and elides instead of drawing beyond the
content column. These changes belong to `apps/desktop/ui/pages/home.slint`.

For a standalone live preview of Home, open the production window from the
repository root. Saving an imported Slint file refreshes the window.

```sh
slint-viewer --auto-reload --component AppWindow apps/desktop/ui/app-window.slint
```

## Verification

- Desktop compilation with all targets validates the Slint declarations.
- The existing `models_navigation` executable now also exercises home-page
  resize, configuration actions, retry, and scrolling in both directions at both
  supported sizes. The home cases live in `tests/support/home_layout.rs`.
- The semantic-color gate reports no direct color literals outside color owners.
- Slint renders of the default theme in light and dark appearance, in English
  and Simplified Chinese at both window sizes, show matching top and bottom
  edges for the enabled/disabled and pending/ready badges. At minimum width, the
  English shortcut title wraps onto two lines.
- Windows DPI behavior, all themes, and the complete bilingual visual matrix
  remain unverified. The GUI event tests do not replace those visual checks.

Run the focused checks with:

```sh
cargo check -p saymore-desktop --all-targets --locked
cargo test -p saymore-desktop --test models_navigation --locked
cargo run -p xtask --locked -- ui-colors
cargo fmt --all --check
```

## Models

Speech recognition cards center the source label, provider name, and chevron
vertically within the same title row. The logo sits below the source, and the
summary and status sit below the name. Each card is 88 px tall, with 12 px
between rows.

Local speech model details put the file state and download or import action at
the top. Resource requirements, recognition mode and its explanation, the test,
and dictionary guidance follow in reading order. Longer errors and test results
can still scroll. Whisper and Qwen show help for the selected mode.

Polishing has four stops: Off, Light, Standard and Thorough. Click a stop, drag
and release, or focus the slider and use the arrow keys. Dragging previews the
description. Enabling a strength uses the existing provider consent and
connection test before applying the selection. The provider directory remains
available when polishing is off.

The polishing page uses a 26 px page title and 16 px section headings. Tabs,
provider names and the selected strength use 14 px text. Descriptions and status
use 12 px text. The strength label stays next to its slider. A single divider
separates this setting from the provider directory. Each provider card centers
its chevron vertically on the row containing the provider name and status.

Open `apps/desktop/examples/model-ui.slint` in VS Code and choose **Slint: Show
Preview** to try both tabs with in-memory controls. The speech recognition tab
mirrors the macOS release provider catalog and uses sample provider states.

## Next Pass

Continue with cloud provider configuration dialogs, then History, Dictionary,
and the five Settings sections. Check minimum-size scrolling, long localized
text, empty/loading/error states, keyboard access, and light/dark appearance.
These are pending review areas, not confirmed defects. Full workspace tests and
packaged Windows visual acceptance have not been run for this home-only change.
