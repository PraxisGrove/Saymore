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

## Control feedback

Sidebar items, provider cards and action buttons respond to pointer presses
inside a fixed hit area. Tab navigation gives the focused control an outline.
Delete icons in local model details, storage, history rows and dictionary draft
rows share a pale red resting color. Hover deepens the red and lifts the icon by
2 px. Pressing lowers the icon, and releasing outside cancels the click.
Disabled delete icons stay gray and still.

## Models

Speech recognition cards center the source label, provider name, and chevron
vertically within the same title row. The smaller source text sits 0.5 px lower
to align its visible strokes with the name. The logo sits below the source, and
the summary and status sit below the name. Each card is 88 px tall, with 12 px
between rows. Available downloads summarize the model's language or recording
mode in the list. Installed cards show the file size.

All four downloadable speech models use a 540 px dialog that grows with the
visible content. The header, test output, resource readings and footer fit
together at the minimum 820 × 560 window size.

Before installation, each model has a two-line positioning statement, a short
description of what the user hears or sees and how the dictionary helps, and a
decorative waveform. Feature descriptions share the headline's left edge. The
import button explains the folder and version requirements in its tooltip. The
facts row shows compatibility, the desktop's 10-minute recording limit, and
either the integrated recognition mode or Whisper's documented memory estimate.
The download action gives the package size. SeACo also offers local import.
Progress, pause, resume and cancellation share the download footer.

After installation, Whisper and Qwen offer **Non-streaming** and **Streaming**
in a segmented control. SeACo displays its fixed non-streaming mode and
Zipformer its streaming mode. Zipformer delivers text when recording ends.
Hovering over a mode option explains when text appears. File size and live
memory sit side by side directly below the mode. Qwen reports its recognition
worker's resident memory. The other models report Saymore's process memory. The
reading refreshes every two seconds while active. Inactive, switching and
unavailable readings have distinct labels. The sample test occupies the last
section above the footer. Each model keeps its own test status and transcript.
Tests can run before activation. The footer places a trash icon beside **Use
model** and disables deletion during testing, activation and use. Hovering
identifies the action in a tooltip. Users manage vocabulary in Dictionary.

The cloud dialogs use the API key and model as the main inputs. OpenAI, Groq and
Mistral show their preset service address in an expandable row. The custom
compatible API exposes the editable address directly. Volcengine uses its own
key and model selection. A changed configuration offers **Test and save**. A
successful transcription of the bundled 3.9-second sample saves the draft. A
configured service can then be set as current or tested again. Field errors stay
beside their input. The three named compatible presets share one saved
configuration with the custom endpoint. The dialog explains replacement when
switching between compatible configurations.

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
mirrors the macOS release provider catalog and uses sample provider states. Open
`apps/desktop/examples/asr-models.slint` to compare all four local installation
flows and four cloud configuration flows. The interactive Slint preview
simulates downloads, tests and settings in memory.

## Next Pass

History, Dictionary and the five Settings sections remain to be reviewed at
minimum window size, in both appearances, and with longer localized strings.
