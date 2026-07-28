# Settings Page Layout

The five settings sections use one shared page-header and content rhythm:

- General
- Shortcuts & Audio
- Delivery & Permissions
- Privacy & Data
- About & Updates

## Header

- Keep the title and supporting detail horizontally centered in the content
  column.
- Place the header 40 px below the settings content area's top edge.
- Use a 70 px header block: a 34 px title row and a 20 px detail row starting at
  50 px.
- Keep the content column at a maximum width of 880 px, with 20 px side gutters
  in compact layouts.

Centered alignment is intentional for this desktop settings shell. The title
identifies the active destination while the left-aligned section labels below
support scanning. A left-aligned page title would compete with those section
labels and weaken the distinction between page identity and page content.

## Content Rhythm

The first section label on every settings page starts 16 px below the header
block. Subsequent section labels, cards, and controls continue to use the
existing 16 px vertical spacing rhythm.

```text
content-area top
    40 px
centered title and detail (70 px)
    16 px
first left-aligned section label
```

The shared implementation lives in `apps/desktop/ui/settings-page-header.slint`;
settings pages must not introduce section-specific top offsets.

## Links And Support

The About & Updates support row has four equal-width interactive targets. Hover
feedback covers the complete target area, while the icon itself remains
unframed. Each target opens the corresponding published product resource:

- Website: the Saymore product site.
- Feedback: the Saymore GitHub Issues page.
- Terms of Service: the license linked from the product site's legal footer.
- Privacy Policy: the privacy boundary document linked from the product site.
