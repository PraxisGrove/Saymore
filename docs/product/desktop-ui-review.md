# Desktop UI Review

## Objective

Review the existing desktop UI page by page, record reproducible problems, and
fix each coherent group before expanding to the next page. The first pass owns
home-page layout at the default 920 × 700 and minimum 820 × 560 window sizes.

## Home Page Findings And Fixes

| Problem                                                      | Evidence                                                                                                                                            | Resolution                                                                         |
| ------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| Usage content becomes unreachable in a short window          | The usage card starts at y=313 and requires at least 300 px, but the minimum window leaves only 540 px for the page; there was no scroll container. | The home content now scrolls vertically below a 616 px content height.             |
| Shortcut and configuration titles compete with status badges | Unconstrained title text and right-aligned badges occupied the same title row, particularly with English labels.                                    | Status badges have separate rows; the shortcut title has a bounded, wrapping area. |
| Shortcut instructions collide with Edit                      | The instructions had no width bound while Edit was positioned at the right edge of the same row.                                                    | Instructions wrap within their own area, leaving room for Edit.                    |
| Retry covers the average-speed caption                       | Retry started at y=105 inside the metric column; the metric caption starts at y=110.                                                                | Retry now occupies the usage card's top-right action area.                         |

The headline also has a width bound and elides instead of drawing beyond the
content column. These changes belong to `apps/desktop/ui/pages/home.slint`.

## Verification

- Desktop compilation with all targets validates the Slint declarations.
- The existing `models_navigation` executable now also exercises home-page
  resize, retry, scrolling to the end, and scrolling back at both supported
  sizes. The home cases live in `tests/support/home_layout.rs`.
- The semantic-color gate reports no direct color literals outside color owners.
- Review of a rendered macOS Chinese/light home page confirms that the title,
  status badges, instructions, and retry occupy separate areas.
- Windows DPI behavior, all themes, and the complete bilingual visual matrix
  remain unverified. The GUI event tests do not replace those visual checks.

Run the focused checks with:

```sh
cargo check -p saymore-desktop --all-targets --locked
cargo test -p saymore-desktop --test models_navigation --locked
cargo run -p xtask --locked -- ui-colors
cargo fmt --all --check
```

## Next Pass

Continue with Models and its configuration dialogs, then History, Dictionary,
and the five Settings sections. Check minimum-size scrolling, long localized
text, empty/loading/error states, keyboard access, and light/dark appearance.
These are pending review areas, not confirmed defects. Full workspace tests and
packaged Windows visual acceptance have not been run for this home-only change.
