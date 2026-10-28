# Paddock design guide (Debrief, Pitcrew, Pitwall, Dyno, Spotter)

Status: proposal 2026-10-22, written by the design lead. Tokens are starting values: validate contrast and CVD in the
app's own token tests before freezing. Research note: web fetch worked for WCAG, Microsoft materials and the Linear
redesign; Apple HIG pages are JS-rendered and returned no text, so Apple, GNOME, Tufte, Okabe-Ito, ECharts and
product-reference points come from established knowledge (marked "[k]" in Sources), not fresh fetches.


> **paddock 0.1.0 changes to the proposal values** (measured by `ui/test/tokens`, see `ui/README.md`): the neutral-text, status,
> chart-palette and Pitwall-fill values below are the validated ones; the original proposals failed WCAG or colour-vision checks.
> A `border-control` token was added because the proposed input border (border-strong) is far below 3:1 (WCAG 1.4.11).
> `paddock-ui` is the source of truth; if this table and `ui/projects/paddock-ui/scss/_tokens.scss` disagree, the SCSS wins.

## 1. Principles

1. **One hero per card.** Each card/tile has exactly one number or chart that is the point (28 px semibold tabular);
   everything else is label (12 px muted) or detail (12-13 px). If two things compete, split the card.
2. **Calm by default, loud on exception.** Normal state is neutral: grey text, hairline borders, neutral bars. Colour
   (status or accent fill) appears only when something needs attention. A screen with nothing wrong has at most one
   saturated element: the primary action.
3. **Three density levels** (user setting, orthogonal to theme): Comfortable (row 36 px), Standard (row 32 px, default),
   Compact (row 24 px, Debrief grids/Pitcrew logs). Controls never go below 24 px high; hit targets 24 px minimum
   (WCAG 2.2 SC 2.5.8), 32 px for primary buttons.
4. **Data-ink first** (Tufte): remove chart chrome that carries no information (box borders, heavy grids, 3D, legends
   that repeat a title). Direct-label rather than legend where possible.
5. **Every state is designed**: loading (skeleton), empty (what + next step button), error (what failed, why, Retry),
   partial, stale, offline.
6. **Status is never colour alone**: word + icon + colour. Markers differ by shape.
7. **Accent is for action and selection, not for meaning.** Accent never means "warning" or "error".
8. **Honest motion**: motion explains change (appear, move, resolve), never decorates; 100-250 ms; reduced-motion safe.

## 2. Layout, spacing, radii

- 4 px base grid. Spacing scale: 2, 4, 8, 12, 16, 24, 32, 48. Card padding 16 (Standard) / 12 (Compact) / 20 (Comfortable).
  Gap between cards 16 (12 compact). Page gutter 24, max content width 1440 for dashboards, 880 for forms/settings.
- Dashboard grid: 12 columns, tiles span 3/4/6/12; row heights snap to 8 px. Align baselines of hero numbers across a row.
- Sidebar 216 px (collapsed 56), toolbar 44 px (Debrief 40), status bar 24 px, dialog widths 400/560/720.
- Title bar 40 px, its own height (not a toolbar): app name and, on Windows, Linux and in the browser, the in-app menu
  bar; on macOS the menus are in the native menu bar only and the title bar keeps 78 px clear for the window buttons.
- A sidebar shown over the page (narrow windows) is modal: it and its scrim cover everything below the title bar,
  including the status bar, which is inert meanwhile; Escape, the scrim or choosing a page closes it.
- Selected navigation item: 14 % accent tint, accent icon, 600 label and a 2 px accent bar, the same in every sidebar mode.
- **Radii** (shared, matches Fluent 4/8 and macOS inset controls): control 6, input 6, chip/badge 999 (pill) or 4 for
  tags, card 10, popover/menu 10, dialog 14, window content (macOS) 0 (OS draws). Nested radius = outer minus padding.
  Windows 11 native windows round themselves; do not fake a window radius.
- Borders: 1 px hairline only. Cards get **either** a border **or** a shadow, never both, never a border inside a
  bordered card (use a 1 px divider or 8 px background step instead).

## 3. Typography

- **System fonts** (native feel, zero bytes): `-apple-system, "SF Pro Text", "Segoe UI Variable Text", "Segoe UI",
  "Cantarell", "Noto Sans", system-ui, sans-serif`; mono `ui-monospace, "SF Mono", "Cascadia Mono", Consolas,
  "JetBrains Mono", monospace`. (Debrief's bundled Inter/Geist/Plex per ADR 0018 stays as its opt-in "theme font"
  but the default should be the system font; see the Debrief review.)
- `font-variant-numeric: tabular-nums` on every number that changes or aligns (tiles, tables, axes, status bar).
- Scale (px / line-height / weight): Caption 11/16/500 (min, uppercase labels only with +0.04em tracking);
  Label 12/16/500 muted; Body 13/20/400 (dense apps) or 14/20 (Pitcrew/Pitwall/Spotter comfortable); Body strong 13/20/600;
  Subtitle 15/22/600; Title 18/26/600 (page title); Display 28/32/600 tabular (tile hero); Hero 40/44/600 (tray/health
  score only). Weights used: 400, 500, 600. No 700, no italics for emphasis.
- Max line length 80 characters in prose; truncate with ellipsis plus tooltip, never wrap table cells to 3 lines.
- Sentence case everywhere. Units are a smaller sibling of the number (`17.1` 28 px + `GB` 13 px muted).

## 4. Colour

All colour comes from CSS custom properties; components never use literals. Dark-first, light is a full peer (tested with
the same screenshot matrix). Hexes are starting values.

### Neutrals (cool grey, slight blue; 7 steps)
| Role | Dark | Light |
|---|---|---|
| bg (window) | #0E0F12 | #F4F5F7 |
| surface-1 (cards) | #15171B | #FFFFFF |
| surface-2 (inset, table header, hover) | #1B1E23 | #F0F2F5 |
| surface-3 (raised, menus, popovers) | #22262C | #FFFFFF + shadow |
| hairline | rgba(255,255,255,.08) | rgba(15,20,30,.10) |
| border-strong (dividers, secondary buttons) | rgba(255,255,255,.16) | rgba(15,20,30,.22) |
| border-control (inputs, checkable controls; new, 3:1) | #838B98 | #6B7380 |
| text | #ECEEF1 | #14171C |
| text-muted | #A0A7B2 (7.4:1 on surface-1) | #4F5764 (6.5:1 on surface-2, 7.3:1 on white) |
| text-subtle (non-essential only) | #8B93A0 (4.9:1 on surface-3) | #5F6775 (5.2:1 on bg) |

### Status (paddock-wide, same hue in every app)
| Role | Dark (text/fill) | Light | Tint bg (dark / light) |
|---|---|---|---|
| success | #3DD68C | #187A46 | rgba(61,214,140,.14) / #E3F5EB |
| warning | #F2B84B | #8F5F00 | rgba(242,184,75,.14) / #FBF0D6 |
| danger | #FF6B6B | #C42B2B | rgba(255,107,107,.14) / #FCE6E6 |
| info | #7DB2E8 | #1F5FA8 | rgba(125,178,232,.14) / #E4EEF9 |
| neutral/paused | #A0A7B2 | #4F5764 | surface-2 |

Warning hue (amber ~42 deg) must stay at least 20 deg away from any app accent: that is why Pitcrew's accent is a
red-orange (hue 18) and not amber, and why it is never used as a status.

### Accents (one per app; 500 = fill for buttons/selection, text-on-dark and text-on-light variants for links)
| App | Mood | Dark accent | Light accent | Fill (button bg, white text, >=4.5:1) |
|---|---|---|---|---|
| Debrief | focused, forensic, calm blue | #5B9BFF | #1D5FD1 | #2F6FE0 |
| Pitcrew | energetic, mechanical, pit-lane orange | #FF7A45 | #BF3A0C | #D2420F |
| Pitwall | technical, control-room violet | #A58BFA | #6D3FD6 | #7546E0 |
| Dyno | lively, vital, magenta | #F063A8 | #B01E6E | #C2287C |
| Spotter | watchful, fresh teal | #2CC4B0 | #0B7A6E | #0E8577 |
| Pacenotes | route-map olive, steady | #8DA74E | #576D07 | #657C1F |
| Sighter | optical plum, precise | #C488CF | #884F92 | #975EA2 |
| Shell Demo (`graphite`, neutral; demo and harness apps only) | neutral, never an app's colour | #A6A8AD | #4F5764 | light #4F5764 (white text); dark #A6A8AD (text #16181C) |

Pacenotes and Sighter (chosen 2026-10-24; replaces the earlier Sighter lime proposal). With seven accents and four status
colours the hue wheel is full: under colour-vision deficiency (Machado 2009, severity 1) only two colour dimensions are
left and no vivid hue stays clearly apart from everything. The two were chosen by CIEDE2000 (smaller of the dark- and
light-scheme text colours), first by distance from the status colours in normal vision (an accent must never read as a
status), then from the other accents, with the deficiency values as a tie-break:

| Pair | Normal | Protan | Deutan | Tritan |
|---|---|---|---|---|
| Pacenotes olive vs success | 16.1 | 9.4 | 10.9 | 16.7 |
| Pacenotes olive vs warning | 23.0 | 1.4 | 4.0 | 32.5 |
| Pacenotes olive vs danger | 51.7 | 11.5 | 3.6 | 39.3 |
| Pacenotes olive vs nearest accent (Spotter) | 25.1 | 21.1 | 26.9 | 17.3 |
| Sighter plum vs info | 25.5 | 4.0 | 4.7 | 46.6 |
| Sighter plum vs Pitwall | 12.3 | 7.1 | 9.3 | 24.2 |
| Sighter plum vs Dyno | 13.9 | 8.0 | 13.6 | 15.9 |

The alternatives measured worse: the earlier lime (#A8D94A / #4D7300) is 0.7 from warning amber for deuteranopes and
4.4 from a green Pacenotes; an orchid Sighter (#CC8CF2 / #A224CC) is 8.6 from Pitwall in normal vision; a brighter green
Pacenotes (#7FC85A / #3B7114) is 10.6 from success. Every yellow-green merges with warning amber under red-green
deficiency, so for Pacenotes (and every app) warning is never colour alone: always the warning icon and its word, and
accent-coloured text is never placed directly beside a warning. Existing pairs with the same limit: Debrief and info
5.4 (normal), Debrief and Pitwall 0.8 (deuteranopia), Pitcrew and danger 8.9 (normal), Spotter and success 2.6
(tritanopia). Accents never meet in one window, and status always carries its icon and word (rule 8 below). The token
tests in `paddock-ui` check both new accents: CIEDE2000 >= 15 from every status colour and >= 12 from every other
accent (normal vision), with the usual contrast targets. `graphite` (2026-10-24) is not an app's: the demo apps
(Paddock Shell Demo) use it so they never look like the app they demonstrate; the same tests hold for it. A neutral
mid-grey fill reads as disabled, so its primary is the dark graphite with white text in light (7.3:1) and inverted in
dark (#A6A8AD fill, #16181C text, 7.5:1), as neutral prominent buttons are; it is the only accent with its own text on
the fill (`fill-dark`, `on-fill-dark` in the token map). Measured by the token tests (CIEDE2000, normal vision):

| Graphite text vs | success | warning | danger | info | nearest accent |
|---|---|---|---|---|---|
| light #4F5764 | 29.4 | 32.5 | 33.5 | 15.2 | Debrief 17.7 |
| dark #A6A8AD | 28.7 | 29.7 | 28.3 | 17.3 | Sighter 21.2 |

The focus ring (2 px graphite, 2 px offset) is 6.7:1 on the light page and 7.3:1 on white surfaces, 8.1:1 on the dark
page and 7.5:1 on dark surface 1. Against the control border it is only 1.4-1.5:1 (both are neutrals), which is why
the ring keeps its 2 px offset: the page colour always sits between the ring and the border.

App icon tiles are their own palette (Spotter: amber tile, teal accent). The tile closest to each new accent that keeps
the family's smallest OKLab distance (`icons/tools/cvd.mjs`: 17.7 normal, 11.6 protan, 13.6 deutan, 10.7 tritan) was
chosen (2026-10-24): Sighter on a lavender tile #A686D6 with a dark glyph (#2D1D43, 5.1:1; white reaches only 3.0:1),
Pacenotes on a bronze-olive tile #6D590F with a white glyph (6.8:1). A plum tile (#975EA2) would have dropped the
family to 10.2 normal and 5.1 deuteranopia against Dyno, an olive tile (#657C1F) to 1.6 protanopia against Pitcrew.
The Paddock mark itself (the umbrella, not an app) uses an asphalt-graphite tile #2B3038 with white ink and carries the seven
app hues as a stripe in its roof; it keeps every family minimum and never stands in for an app (icons/README.md).

Accent rules: primary button, selected nav item (tinted bg 14% + 2 px left or full tint, accent icon), focus ring (2 px
accent, 2 px offset), links, the single highlighted chart series, progress of the app's own primary metric. Not
for badges meaning attention, not for toggles' off state, not for body text. Info (sky) and Debrief blue are near
neighbours: never place an info badge inside accent-selected context without its icon, and Debrief uses the info icon
shape (circle-i) always.

### Contrast targets
Text >= 4.5:1 (AA), large (>=18.66 px bold or 24 px) >= 3:1, aim >= 7:1 for body text in the dense data views (AAA
where it is free). UI components, focus rings, chart marks and meaningful graphics >= 3:1 against adjacent colour
(SC 1.4.11). Focus appearance: 2 px, 3:1 (SC 2.4.11/13). Non-essential decoration is exempt. Provide a High contrast
mode that maps hairline to border-strong and removes translucency (also on Windows "contrast themes" / macOS
Increase Contrast / Reduce Transparency).

### Categorical chart palette (CVD-safe; Okabe-Ito derived, tuned per scheme; 8 max, 5 recommended)
Order matters: adjacent series differ in hue AND lightness. Assign by series identity (stable), not by index.
| # | Name | Dark | Light |
|---|---|---|---|
| 1 | sky blue | #60AEEB | #0C6DBE |
| 2 | orange | #F3AA25 | #C47600 |
| 3 | bluish green | #3DD39D | #087D51 |
| 4 | reddish purple | #D68CC2 | #B04C95 |
| 5 | vermilion | #F07D3A | #BE3108 |
| 6 | periwinkle | #7A70FF | #494FDB |
| 7 | yellow | #E8DC5A | #816A1D |
| 8 | grey | #A2ACB1 | #6F737D |
Sequential ramp (one hue, 6 steps, for heatmaps): from surface-2 to accent-500 in OKLCH lightness steps (dark: light
increases; light: dark increases). Diverging: series 1 - neutral - series 2 (never red/green). Event/level colours in
Debrief (fatal/error/warn/info/debug) use the status set + shape, not this palette. Validate with the dataviz skill's
palette validator and a Machado CVD simulation before freezing; adjacent pairs delta-E >= 20 under deuteranopia/protanopia.
Scope of the rule (no 8-colour categorical palette passes all 28 pairs, so the rule says where it must hold). It supersedes the one-line
requirement above for deuteranopia/protanopia: delta-E >= 20
under protanopia, deuteranopia and tritanopia for every **order-adjacent** pair and every **ring-adjacent** pair (a donut closes on
itself: `ringOrder` picks the slot order), and slot 6 is >= 20 from every other slot under protanopia and deuteranopia (slots 1/4 and 2/5 stay weak in any 8-colour palette). A chart with **4 or
more** coloured series must also be keyed without colour (direct labels, or hover isolation with the name in the tooltip), and
series beyond 6 fold into one neutral "Other". Known weak pair: light slot 1 and slot 6 under tritanopia is 11.7 (both blues; rare, and outside the slot 6 rule, which is protan/deutan). `ui/test/tokens/tokens.spec.ts` and `ring.spec.ts` enforce it.

## 5. Elevation and materials (per OS)

- **Layers (4)**: window bg, surface-1 card (no shadow in dark; hairline), surface-3 popover/menu (shadow-2), dialog
  (shadow-3 + scrim). Shadows dark: `0 1px 2px rgba(0,0,0,.4)`, `0 8px 24px rgba(0,0,0,.45)`; light: `0 1px 2px
  rgba(15,20,30,.06), 0 1px 1px rgba(15,20,30,.04)` and `0 8px 24px rgba(15,20,30,.14)`; top inner highlight
  `inset 0 1px 0 rgba(255,255,255,.04)` on dark surface-3.
- **macOS**: vibrancy/NSVisualEffect `sidebar` material behind the sidebar and `popover`/`menu` for tray popups,
  inset traffic lights, `.hudWindow` not used. Content area is opaque. Honour Reduce Transparency. Sidebar items 28 px, selection
  = rounded 6 px tint. Menu-bar extras: a popover 360 px wide max 520 tall, with arrow anchoring; template (monochrome)
  tray icon, never full colour; no more than 5 rows visible without scroll.
- **Windows 11**: Mica for the window (Tauri `windowEffects: mica`), Acrylic only for transient surfaces (flyouts, tray
  popup, context menus); smoke scrim under dialogs; 8 px window corner and 4 px controls are drawn by the OS/Fluent
  conventions: our controls 6 px sit well beside them. Tray flyout 360 wide, anchored above the taskbar, closes on
  light-dismiss; fall back to solid surface-3 when transparency is off (Windows 10).
- **Linux (GNOME-first, also KDE)**: solid surfaces, no compositor effects assumed; follow libadwaita: header bar
  (we use our toolbar) with 12 px card radius is acceptable, but keep paddock 10. Honour dark/light preference and
  accent from `prefers-color-scheme`/portal where available. Tray via StatusNotifier; provide a window fallback because
  GNOME hides tray icons by default.
- All three: if the effect is unavailable, the solid fallback token is identical to the designed solid colour (no
  visual jump).

## 6. Borders and icons

- 1 px hairline; dividers inside cards are hairline not full-contrast; table rows separated by hairline OR zebra, not
  both. Inputs use the control border (3:1), 1 px; focus = accent border plus the family focus ring (a solid 2 px accent outline, 2 px offset, 3:1 or better on every surface: the same ring as every other control, SC 2.4.13). Hover on an input or select moves the border to muted text; hover on a secondary button mixes 7% text colour into its fill, so it changes on every surface. Icons are never smaller than 14 px (a 1.5 px stroke at 12 px merges the "!" and "i" with their frames); the lint in `ui/test/tokens/icon-size.spec.ts` enforces it. Pitwall and the other app accents in dark use a lighter violet (`--pd-accent-control`) for switches and checks beside the deeper primary fill: two tones of one hue is accepted, not a second accent.
  A translucent halo (accent at 35% alpha) measured 1.7 to 2.0:1 and does not count as the focus indicator; use it only as decoration beside the solid ring.
  A selected segmented-control segment is an accent tint with a 1 px accent ring and a 600 label, in both schemes (a raised surface inverts in dark). The fill of an on toggle, checked box or radio is `--pd-accent-control` (3:1 on every surface; in dark it is the accent colour with dark ink, in light the accent fill with white ink).
- Icons: Paddock's own SVG set, 16 px default (20 for nav, 14 inline with 12 px text), 1.5 px stroke on the screen at every size (`vector-effect: non-scaling-stroke`, the same through `<pd-glyph>` and `<mat-icon svgIcon>`), round caps and
  joins, 24 px design grid scaled, dots drawn as filled circles never as strokes, `currentColor` only, optical alignment to cap height. Same metaphor = same icon across
  apps (settings, logs, update, runner, model, process). Filled variant only for the active nav item. Every icon-only
  button has `aria-label` and tooltip (500 ms delay). App icon: rounded square 22% radius with a single-hue gradient
  (accent 400 to 600) and a white glyph; in tray, template mono.

## 7. Charts (ECharts)

- Theme object generated from tokens (one function, shared in `paddock-ui`): font = system, 11 px axis labels
  tabular, axis text text-subtle, **no axis lines** except baseline, grid lines hairline dashed 2-4, only horizontal.
- **Sparkline**: 24-32 px high, no axes, 1.5 px line, accent or series colour, end-point dot 4 px with value
  label, area optional at 10-14% alpha. Same y-scale across sparklines in one row when comparing, otherwise state
  "peak N" in the caption (Pitcrew does this well).
- **Area gradient**: top 28% alpha to 0 at baseline, linear, one series only. Stacked areas use solid 85% fills with 1 px
  surface-colour separators; no gradients there.
- Line width 1.5 (2 for the hero series), no symbols except on hover and for single points; `connectNulls: false`
  (gaps are honest); `sampling: lttb`; animation 300 ms ease-out on first draw only, none on live updates.
- **Thresholds and annotations**: dashed 1 px line with a right-aligned label ("limit 90%"); event markers on a
  dedicated 14 px lane under the plot, shape-coded (circle, triangle, diamond, square) + legend that is also a toggle;
  tooltips are crosshair-synced, show time first, then series with colour chip, tabular values, max 8 rows.
- **Bars** start at zero, 4 px top radius, gap 25%; **heatmaps** use the sequential ramp with a legend gradient.
  Donuts/gauges only for a bounded single value (health score); prefer a bar.
- Legends: top-left, 12 px, interactive (click toggles), omitted when one series. Titles state the finding or the
  metric + unit + window ("CPU, last 24 h").
- **Empty chart**: keep axes ghosted at 40% opacity, centred message "No data for this period" + next step ("Widen to
  7 days"). **Loading**: skeleton block at final height (no layout shift). **Error**: inline message + Retry.
- Pass tokens in; re-theme on scheme change without reloading data. Do not use `dataZoom` slider chrome by default:
  use brush on the plot and a "Reset zoom" chip.

## 8. Components

- **Cards/tiles**: surface-1, hairline border, radius 10, padding 16, header = 16 px icon (muted) + 13 px/600 title +
  right-aligned status/controls; hero below; footer detail row. Whole tile is a button if it navigates (chevron on
  hover, focus ring). No nested bordered cards.
- **Tables**: row 32 (Standard), header 28 sticky, 11/12 px uppercase-free muted labels sentence case, numbers
  right-aligned tabular with units in header, first column strong, hover surface-2, selected = accent 12% tint plus
  2 px left bar, truncation with tooltip (a path is cut at segment boundaries and keeps the trailing segments, `…/Debrief/plugins/viewer/plugin-1.bin`, because the shared head is the part rows have in common), row actions appear on hover/focus but remain keyboard reachable (the "..."
  button stays visible in `--pd-row-action-rest`, subtle text at 75% (65% measured 2.75:1 in light), 3:1 or better; 100% text colour on hover, focus and open). Sort affordance visible. Never clip the last column: use sticky action column.
- **Toolbars**: 44 px, ghost buttons by default (no border until hover), one primary (filled accent) at the far right,
  groups separated by a 1 px divider, segmented controls for 2-4 mutually exclusive options (time range).
- **Buttons**: primary (accent fill), secondary (surface-2 + border-strong), ghost, danger (danger fill only in
  confirmation dialogs). Height 28 (compact) / 32. Max one primary per view region.
- **Popovers/menus**: surface-3, radius 10, shadow-2, 4 px padding, items 28 px radius 6, shortcut right-aligned
  muted, CDK menu for focus management, 120 ms fade+4 px translate.
- **Tray popups**: 360 px wide; header (icon, name, one status pill), hero tiles 2x2 (value 20/600, sparkline 24 px),
  one list (max 5 rows), footer (one primary "Open <App>", one icon-button for pause). Needs-attention banner replaces
  the first row when active. 12 px minimum text.
- **Dialogs**: modal for decisions only; title 18, one sentence, primary on the right, cancel left of it; destructive
  dialog names the thing and the consequence ("Delete 3 runners and their folders"); Esc closes, focus returns to the
  trigger. Prefer inline panels or popovers over dialogs.
- **Toasts**: bottom-right, 360 max, auto-dismiss 5 s (errors persist until dismissed), at most 3 stacked, with Undo
  where the action is reversible; `role=status` (errors `role=alert`).
- **Badges**: 20 px high, 12/500, pill, tint bg + status colour text + 12 px icon + word. Counts in nav: neutral
  surface-3 pill; use status tint only if the count means a problem. Never more than 2 badges in one table cell.
- **Skeletons**: 1.6 s shimmer on surface-2 (static in reduced motion), same geometry as content.
- **Empty states**: 32 px muted icon, 15/600 title (what is missing), one muted sentence (why / what happens next), one
  button (the next step). **Error states**: icon in danger, what failed in plain words, likely cause, Retry + secondary
  ("Open logs"). Never raw exception text as the headline.
- **Forms**: label above input 12/500, helper 12 muted, error replaces helper in danger with icon, validate on blur,
  toggles are for immediate effects, checkboxes for form submit; group settings by consequence, not by data model.

### Material controls (rule 47)
- **One theme.** `@decryp7/paddock-ui/css/material.css` is the whole Material look: Material 3 with density -3, every system and
  component token resolved to `--pd-*`, so theme, accent, contrast and density follow our tokens. An app loads it and carries no
  Material theme of its own. The accent comes from `data-accent` on `<html>`; a `data-accent` on a subtree does not reach Material
  parts (their tokens resolve on `:root`).
- **Behaviour defaults.** `providePaddockMaterial()` (`@decryp7/paddock-ui/material`): every `mat-form-field` is
  `appearance="outline"` with `floatLabel="always"`, button toggles show no check mark, and ripples are off. Feedback is the
  state layer (hover, pressed) and the keyboard ring, the same calm response as the rest of the family and nothing to switch
  off for Reduce Motion.
- **Buttons are `matButton`:** `filled` = primary (accent fill), `outlined` = secondary (surface-2 + border-strong), `text` =
  ghost, and the class `pd-button-danger` on any of them = danger. `tonal` and `elevated` are bridged only so they never fall back
  to Material's blue; the family does not use them (an app review rejects them).
- **Keyboard focus** is Material's strong indicator in `--pd-focus-ring`: a 2 px solid ring 2 px outside the control, 3:1 on every
  surface (`--pd-focus-width`, 2 px). `e2e/material.spec.ts` tabs through every kind of control and asserts it. **Exception, form
  fields:** the floated label sits on the border, so a ring around the box would cross it; the field's own outline is the ring
  instead, in `--pd-focus-ring` at 3 px (the resting 1 px border is part of it, so 2 px of new pixels change, SC 2.4.13).
- **Measured:** `npm run test:tokens` checks the contrast of every bridged component in both schemes and all accents; the gallery
  page "Material controls" and its e2e check the rendered sizes (28 px rows, 4 px panel padding, 12 px floated label).

## 9. Motion

Durations: micro (hover, press) 100 ms; standard (popover, tab, expand) 160-200 ms; large (page, dialog) 240-280 ms;
exit = 70% of enter. Easing: enter `cubic-bezier(0.2, 0, 0, 1)` (decelerate), exit `cubic-bezier(0.4, 0, 1, 1)`, move
`cubic-bezier(0.3, 0, 0.2, 1)`. Animate only `transform` and `opacity`. Number changes: no count-up on live data;
cross-fade values only on user actions. Respect `prefers-reduced-motion: reduce`: remove translation, scaling and
shimmer, keep opacity fades <= 100 ms and state changes instant; never convey information by motion alone; no
flashing >3/s. Live charts do not animate between ticks.

## 10. Microcopy

Plain, specific, calm, second person only when instructing. Sentence case. Verb + object on buttons ("Register
runner", "Clean now"); never "OK/Submit/Yes". Errors: what happened, why if known, what to do ("Can't reach the Gitea
server. Check the URL in Settings, then Retry."). No exclamation marks, no jokes in errors, no "Oops". Numbers with
units and thousands separators; relative time under 24 h with absolute on hover; "3 min ago" not "3m". Empty states
start with the benefit, not the lack. Pitwall may be slightly technical, Dyno slightly warmer, Debrief terse, Pitcrew
operational, Spotter concise.

## 11. Native-feel rules (Debrief ADR 0028 for the whole Paddock family)

App shell is `user-select: none`, default cursor on non-links, no browser context menu/keys/zoom/drag, selectable
content opts in (`.selectable`: logs, values, errors); OS-native window controls and title bar behaviour; platform
shortcuts (Cmd on macOS, Ctrl elsewhere) with menu-bar accelerators; double-click titlebar zoom; system accent for focus
only if the user opts in (we keep the app accent); scrollbars overlay/thin (8 px, hover 12) matching the OS; honour OS
text scaling, Reduce Motion, Reduce Transparency, High Contrast; dragging files in opens them; drop targets are visibly
highlighted; no hover-only functionality (keyboard and touch parity).

## 12. Per-app identity

Shared: neutrals, status, radii, type, icons, components. Distinct: accent, app icon gradient, one signature
element, mood.
- **Debrief** (cobalt): forensic precision; highest density; mono for data; signature = the timeline (shape-coded markers,
  crash band). Calm: tinted surfaces, no brand colour fill except Run/Search.
- **Pitcrew** (pit orange): operational control; medium density; signature = runner cards with live job timer and
  state-bordered tiles (only the failing card gets a danger border).
- **Pitwall** (violet): control room; signature = the approval banner with pairing code and the VRAM stacked bar; chat in the
  Playground is the only comfortable-density screen.
- **Dyno** (magenta): vital signs; most visual; signature = health ring and sparkline tiles; neutral bars, accent only for the
  hero metric; threshold colours (success/warning/danger) on bars only when crossed.
- **Spotter** (teal): watchful, quiet; signature = a "pulse" row per watched item (last checked, changed dot) and a
  change-diff view; mostly lists and timelines, so lowest chart density, highest typographic quality. Direction: a
  feed-style home (Today: what changed), grouped by watcher; change items show a before/after diff chip, severity
  (breaking / notable / info) via status colours; tray popup lists 5 latest changes with "Mark all seen"; empty state
  "Nothing changed since 09:12. Next check in 14 min".

## 13. Polish checklist (apply per screen, in both schemes, at 1280x800 and 1920x1080)

Hierarchy
1. One hero per card; hero 28/600 tabular, label 12 muted. Page title 18/600 + one muted subtitle line.
2. At most one filled accent button per region; others secondary/ghost.
3. No saturated colour unless it means something; normal state is neutral.
Layout
4. 4 px grid; gaps 16; baseline-aligned numbers; no card with big dead space (> 24 px empty): stretch content or shrink card.
5. No nested borders; border XOR shadow; radius scale only (6/10/14).
6. Tables: right-aligned tabular numbers, units in headers, no clipped last column, no 3-line wraps, sticky header.
Colour
7. Tokens only; text >= 4.5:1 (check muted/subtle text and disabled), UI parts >= 3:1; light theme has its own tuned tints (not inverted dark).
8. Status = icon + word + colour; accent never used as status.
9. Charts use the Paddock palette in stable order; no red/green pairs.
Charts
10. Titles name metric, unit, window; legends interactive; direct labels for <=3 series, from 4 series a non-colour key too (hover isolation with the name in the tooltip, `pd-chart` does it); axes tabular 11 px; gridlines hairline.
11. Empty, loading, error states present for every chart.
12. Event markers shape-coded; thresholds dashed + labelled.
Components
13. Every data view: skeleton, empty (with next step), error (with Retry).
14. Icon-only buttons have aria-label + tooltip; icons from the set, 16 px, 1.5 stroke.
15. Dialogs only for decisions; destructive names consequence; Esc/focus return.
16. Badges <= 2 per cell; nav counts neutral unless problem.
Interaction
17. Focus ring visible on every control (2 px accent, 2 offset); full keyboard path; hit targets >= 24 px.
18. Hover/pressed/disabled/selected states defined; hover actions also reachable by keyboard.
19. Motion: 100/200/260 ms, transform+opacity only, reduced-motion path.
Content
20. Sentence case, verb+object buttons, errors = what/why/next; units and tabular numbers; no truncation without tooltip.
Platform
21. Native-feel (ADR 0028) behaviours hold; materials fall back solid; tray popup 360 wide with template icon.
22. Screenshots in light and dark reviewed at 100% zoom before commit; axe clean.

## Sources
- WCAG 2.2 Understanding 1.4.11 Non-text contrast (fetched): https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html
- WCAG 2.2 Target Size (Minimum) 2.5.8, Focus Appearance 2.4.13 [k]: https://www.w3.org/TR/WCAG22/
- Microsoft, Materials used in Windows apps (fetched; Mica for base, Acrylic only transient, Smoke for modals): https://learn.microsoft.com/en-us/windows/apps/design/signature-experiences/materials
- Fluent 2 Material / Geometry (radius 4/8 convention, secondary): https://fluent2.microsoft.design/material
- Linear, "How we redesigned the Linear UI" (fetched; LCH theme generation from base/accent/contrast, reduced noise, Inter): https://linear.app/now/how-we-redesigned-the-linear-ui
- Apple HIG Materials and Typography [k; page text not retrievable]: https://developer.apple.com/design/human-interface-guidelines/materials , https://developer.apple.com/design/human-interface-guidelines/typography , menu bar extras / popovers pages
- GNOME HIG [k; fetch 404]: https://developer.gnome.org/hig/
- Okabe and Ito, Color Universal Design [k]: https://jfly.uni-koeln.de/color/
- Tufte, The Visual Display of Quantitative Information (data-ink, small multiples, sparklines) [k]
- Datawrapper Academy / Observable Plot guidance on colour and annotation [k]: https://blog.datawrapper.de/ , https://observablehq.com/plot/
- Apache ECharts handbook (themes, dataset, sampling, accessibility `aria`) [k]: https://echarts.apache.org/handbook/
- Material Design motion (duration/easing ranges) [k]: https://m3.material.io/styles/motion
- Product references studied from knowledge, not fetched: Linear, Raycast, Arc, 1Password 8, Things 3, iStat Menus, Stats, CleanMyMac, Grafana, Datadog.
