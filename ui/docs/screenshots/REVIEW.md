# paddock-ui documentation screenshots: independent review (rule 22)

Reviewed 2026-10-25 on `shell/integrate` (ae91fae). The reviewer looked at the PNGs, `hashes.json`, `e2e/screenshots.spec.ts`,
the gallery source (only to name the cause of a finding), `docs/design-guide.md`, and the text that embeds the images (`ui/README.md`).
No other `docs/*.md` file embeds these screenshots.

## How the review was done

- **229 images:** 196 component sections (14 sections x 7 accents x 2 themes), 4 gallery overviews, 2 density cards and 27 shell-demo shots.
  Every hash matches `hashes.json`.
- **Accents and themes, checked by machine:** for each section and theme, every accent was diffed against Debrief. The changed pixels
  have the same count and place in every accent, and their dominant hue is the accent's own hue from the design guide (Sighter dark
  is low in saturation, so that check was done by eye). Five sections are pixel-identical across accents, as they should be, because
  they carry no accent: icons, metric, path-text, skeleton and status-pill. Every image's corner colours match the theme in its name.
  The only "mixed" corners are rail-open-light and narrow-open-light, where the scrim makes them so. No image has the wrong theme or
  the wrong accent.
- **By eye:** every section once per theme, at full size and with crops where needed. Accent montages for controls and the tray
  popover. All 4 gallery pages, both density cards and all 27 shell-demo shots. The focus and health variants were also diffed
  against their base shot, which shows that only the intended region changes.

**Result: 133 approved and 96 not approved.** There are 3 P1, 9 P2 and 12 P3 findings. The approved images are listed in
[REVIEWED.md](REVIEWED.md). An image is approved only if it has no P1 or P2 finding.

## P1

| # | Image(s) | Problem | Fix |
|---|---|---|---|
| P1-1 | shell-demo/pages-narrow-{dark,light}, pages-narrow-open-{dark,light}, pages-focus-toggle-{dark,light}, pages-focus-more-{dark,light} (8) | At 320 px the in-app menu bar does not fit. "File View" wraps onto a second line with "Help" under it. The 40 px title bar clips both lines: "File"/"View" lose their tops at the window edge, and "Help" sits on the title bar's bottom hairline. This is a broken layout in the documentation of the narrow mode. REVIEW-3 (2026-10-24) did not report it, so it is probably a regression. | The menu bar must never wrap. At narrow widths, collapse File/View/Help into one overflow menu button (or hide them behind the sidebar toggle), and keep the title bar 40 px with nothing clipped. Add a 320 px layout test that the title bar's content box is one line high. |
| P1-2 | the same 8 narrow shots | In the Runners table the Runner column is about 90 px wide and its text is hard-clipped in the middle of a glyph ("build-li", "nuc-03.", "mac-mi"), with no ellipsis. The rows can't be identified, and this breaks rule 18 (names are truncated in the middle and the full value is in a tooltip). Meanwhile the State column keeps about 300 px of empty space. | Give the identifier column priority (a min width, or let State shrink), and truncate the name with `pd-path-text`-style middle ellipsis and a tooltip. Never use `overflow: hidden` without an ellipsis. |
| P1-3 | ui/README.md line 6 (gallery hero) | The README's first image is `docs/screenshots/gallery-dark-debrief.png`, which does not exist. The file is `gallery-debrief-dark.png`, so the package README opens with a broken image. | Point it at `gallery-debrief-dark.png`. Once G-1 below is fixed, use a framing that doesn't cut a card. |

## P2

| # | Image(s) | Problem | Fix |
|---|---|---|---|
| P2-1 | {dark,light}/*/card.png (14), gallery-*.png (4), density/card-*-dark.png (2) | The Network card's caption reads "Up 0.4 MB/s · activated 0×". The "activated 0×" part is a click counter from the gallery's test harness (`gallery.ts:79`). It reads like placeholder or debug text in every card screenshot. | Keep the counter for the e2e test in a visually hidden element (or a `data-` attribute), and show a real caption, e.g. "Up 0.4 MB/s · down 4.6 MB/s". |
| P2-2 | {dark,light}/*/error-state.png (14) | The harness counter "Retried 0×" (`gallery.ts:134`) sits outside the card's padding. It touches the card's left border and runs into the rounded bottom-left corner, so it looks like overflowing content. | Remove it from the visible section, the same as P2-1. If the gallery has to show it, put it under the card with normal spacing. |
| P2-3 | {dark,light}/*/path-text.png (14) | (a) The paths render in Courier, a serif fallback mono. The capture environment does not resolve `ui-monospace` or "SF Mono", and the result looks dated and unlike the product. (b) Item 2 shows **two** ellipses ("C…Builder.S… 2026-10-24.log"), and item 4 shows a space after the ellipsis ("/Users/li… builder.service.log"), while item 1 has none. The middle truncation looks broken. (c) The Windows separators are missing ("C:…Builder"). | (a) Give the mono stack a bundled or always-present fallback before `monospace` in the screenshot run, or install the font in the capture image. Check that every app's mono text renders in a sans mono. (b, c) Fix `pd-path-text`: one ellipsis, no gap between the head and the tail, and the separator kept next to the ellipsis ("C:\…\Builder.Service.2026-10-24.log"). Add unit tests for both shapes. |
| P2-4 | {dark,light}/*/job.png (14) | (a) "Done" and "Failed" are colour and word only, with no icon. Design guide principle 6 says status is never colour alone: word + icon + colour. (b) The failed card says "Free some space and retry" but has no Retry button (design guide 5: an error shows what failed, why, and Retry). (c) "Save project" shows a **disabled** Cancel. Paddock rule 11 says Cancel works in every phase. | (a) Add the success and danger icons, as `pd-status-pill` does. (b) Add a Retry button (and Dismiss). (c) Show an enabled Cancel in every phase. If a phase really can't be interrupted, say so in words ("Finishing the write, then it stops"), not with a disabled button. |
| P2-5 | {dark,light}/{debrief,pitcrew,pitwall,spotter,pacenotes,sighter}/tray-popover.png (12) | The popover is always Dyno's ("Dyno", "Open Dyno") but takes on the accent of whichever app is selected. So Debrief blue, Pitcrew orange and the others appear on a Dyno popover, which mislabels an app and goes against "accents never meet". The README's example (dark/dyno) is correct. | Name the popover after the accent's app (the gallery knows the accent), or use a neutral sample name. |
| P2-6 | shell-demo/pages-wide-*, pages-health-*, pages-focus-nav-*, pages-focus-nav-selected-* (8) | The status bar reads "Waiting for the device 0 s elapsed", while the job card in the same frame reads "14 s elapsed". The same job shows two elapsed times. The status text also has no separator between the phase and the time. | Feed both from the same job snapshot (the status bar is probably reading a start time that was reset when the clock was frozen), and format it as "Waiting for the device · 14 s". |
| P2-7 | shell-demo/pages-rail-{dark,light}, pages-rail-open-{dark,light} (4) | At the rail width the Last job column is truncated to "5 min a…", "18 min …" and "Yesterd…", while the State column has about 200 px of free space. Short relative times shouldn't need truncating. | Size the columns to their content (State can shrink to about 110 px), or drop Last job before truncating it. Use container queries (rule 18). |
| P2-8 | shell-demo/pages-macos-{dark,light} (2) | The frame contradicts itself and the other pages shots. Because the fake IPC answers null, the scene's job and broken index did not take effect. The result is "No jobs", while build-mac-2 is "Updating", the status bar says "1 updating" and the nav badge is 1. The chip reads "Services: unknown" with a **danger-red** dot: an unknown state is shown as an error, which breaks "calm by default". | Make the macOS shot run the same scene as the others (the Tauri fake must answer the job and health calls), so it differs only in the title bar. Separately, show "unknown" health as neutral, with a neutral dot and the word, never danger. |
| P2-9 | ui/README.md, Gallery section (lines 583-585) | The text says "every component (10 sections) in both themes and each of the five accents". The folder has 14 sections and 7 accents. | Update the text to "14 sections" and "seven accents", or drop the numbers. |

## P3 (optional polish; images are approved)

| # | Image(s) | Problem | Fix |
|---|---|---|---|
| P3-1 | gallery-*.png (4; not approved because of P2-1) | The 1440 x 900 viewport cuts the page in the middle of the Empty state card, so only its folder icon shows. This is the README hero. | Capture the full page, or end the frame at a section boundary (e.g. clip to the first four sections). |
| P3-2 | */segmented-control.png | "Selected: 24h / all" is a harness readout. It also uses "24h" while the control says "24 h". The filter counts are not grouped ("1204"), but metric uses "1,234,567". | Hide the readout as in P2-1. Format the counts with the locale's grouping. |
| P3-3 | */metric.png, gallery | The metric row has no labels, so the reader can't tell what 74/100, 3.2 % or "— 0" mean. The lone large "—" (no value) looks like a stray rule. | Add a small label under each value (e.g. "Health", "Events", "No data"). |
| P3-4 | */chart-theme.png | The four event markers under the axis (▲ ■ ▲ ●) have no labels. The first sits left of the y-axis and the last touches the card edge. The two swatch rows (categorical, sequential) have no captions, and the first sequential swatch is invisible against the page. | Label the marker row and keep it inside the plot area. Caption the swatch rows and give each swatch a hairline. |
| P3-5 | */sparkline.png, gallery | The middle sparkline card is about 8 px shorter than its neighbours. | Give the demo cards a fixed height. |
| P3-6 | */tray-popover.png | The units are written inconsistently: "45 %" in the tiles but "42%" in the process list. | Use the same unit spacing everywhere (the design guide treats the unit as a sibling of the number). |
| P3-7 | */controls.png | A button labelled "Compact" (it demonstrates the compact size) sits beside a select whose value is "Compact", which reads like a duplicate. In Pitcrew the primary orange sits next to the red Delete (a known pair in the design guide; Delete keeps its icon). | Label the size demo "Small button" or similar. |
| P3-8 | */skeleton.png, metric, path-text, segmented-control, status-pill | Each image is the full 1392 px section width, but the content fills about a third of it, so these images are mostly empty strip. | Capture the content's bounding box plus the section padding. |
| P3-9 | dark/*/skeleton.png | The dark skeleton bars are very faint (decoration, so exempt from contrast rules, but they barely show in documentation). | Optional: one step lighter in dark. |
| P3-10 | shell-demo/theme-* (5) | The open health popover covers the Engine data card's error text ("Engine da…"), and the frame ends partway through that card. In the theme-light and high-contrast shots the Appearance control shows "System" selected. | Acceptable for "the whole shell in one look". If it is reshot, scroll so the card ends inside the frame, and select the theme that is shown. |
| P3-11 | shell-demo/theme-pseudo-locale-dark | Only the shell parts and the command buttons are pseudo-localised. The page body ("App context: loaded", "Greetings", "System/Light/Dark", "Engine data", "Start-up contract") and the status-bar summary "1 offline · 1 updating" are English. This matches the README's claim, which covers shell parts only, but a reader can mistake it for a gap. | Mark the demo page's strings as well, or say in the README caption that only the shell and its commands are covered. |
| P3-12 | shell-demo pages (all) | "Local AI" uses the same info (circle-i) icon as About. In the narrow shots the Cancel button is cut by the status bar (it is in the scroll area, so not a defect). | Give Local AI its own glyph (e.g. activity or a chip). |

## Approved (133)

- All 7 accents x 2 themes of: chart-theme, controls, empty-state, icons, metric, segmented-control, skeleton, sparkline,
  status-pill (126). Their P3s are listed above.
- dark/dyno/tray-popover.png and light/dyno/tray-popover.png (2). These are the README example, and the name matches the accent.
- shell-demo/theme-dark, theme-light, theme-high-contrast-dark, theme-high-contrast-light, theme-pseudo-locale-dark (5). These are the
  five shots `ui/README.md` links under "Screenshots for review", and each caption matches its image.

## Not approved (96)

card (14), error-state (14), path-text (14), job (14), tray-popover except Dyno (12), gallery-* (4), density/* (2), and all 22
shell-demo `pages-*` shots. Fix the findings above, regenerate (`SCREENSHOTS=1 npx playwright test --project=screenshots`), and
have the changed images reviewed again.

README embeds: `light/pitcrew/card.png` (README line 587) is not approved (P2-1). `dark/dyno/tray-popover.png` and
`light/pitwall/chart-theme.png` are approved, and their captions match the images (component, theme and accent).
