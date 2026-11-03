# Dyno menu-bar and tray items: design spec

Status: proposal 2026-11-03, for independent review and then the user. Answers the user's feedback of 2026-11-03: "the
metric icon should be more professional like iStat Menus" and "the metric icon flickering on macOS". iStat Menus was the
quality bar only; nothing here copies its artwork or layouts.

Sheets (all rendered by `render/`, the drawing route the app can use):

| Sheet | What it shows |
|---|---|
| `macos-menubar@1x.png`, `macos-menubar@2x.png` | 9 Dyno items beside the system items: light bar, dark bar (one item highlighted), transparent bar on a tinted wallpaper (one item in alert) |
| `macos-items@2x.png` | every format in all 4 states on a light and a dark bar, its fixed width, close-ups of the 1x and @2x images |
| `windows-tray.png` | the notification area at 100, 150 and 200 % on light and dark taskbars; close-ups at 16, 20, 24 and 32 px |
| `linux-panel.png` | GNOME top bar (dark) and a KDE panel (light) at 22 px |
| `today-vs-proposed.png` | the same nine items as `dyno_core::tray` draws them today and as proposed: macOS @2x and 1x, Windows at 100 % |
| `numerals.png` | Dyno's dash numerals and label capitals for the Windows and Linux icons |

Rebuild: `cd docs/design/dyno-tray/render && CARGO_TARGET_DIR=<your build dir> cargo run --release` (macOS only, since it reads
the system font; it also prints the measurements below).

## 1. Drawing route (offline, buildable)

- **Raster:** `tiny-skia` 0.11 (BSD-3-Clause), pure Rust, no system libraries (a new dependency for Dyno).
- **macOS text:** the system font, read from the OS at run time, never bundled or fetched: `/System/Library/Fonts/SFNS.ttf`.
  That is the font `NSFont.menuBarFont(ofSize: 0)` resolves to (measured on this Mac: 13 pt, opsz 17, wght 400; status bar
  thickness 22 pt). In the app, find the file through CoreText (`CTFontCreateUIFontForLanguage(kCTFontUIFontMenuItem)` and
  its `kCTFontURLAttribute`) so a renamed file still resolves; if no font loads, draw the square glyphs of today.
- **Shaping:** `rustybuzz` 0.20 (MIT) with the `tnum` feature and the `wght`, `wdth` and `opsz` axes.
- **Outlines:** `skrifa` 0.44 (MIT/Apache-2.0). `ttf-parser` 0.25 returned no outline for SF's glyphs through its `gvar`
  path (measured: `outline_glyph` = `None` for every glyph), so outlines come from skrifa, unhinted, as macOS draws them.
- **Windows and Linux:** no font at all. Dyno's own dash numerals and 5- and 7-row capitals (`render/src/pix.rs`), drawn on
  whole pixels for each exact size: a system font at 9 to 12 px unhinted is soft, and Segoe UI cannot be loaded on macOS for
  tests anyway.
- Where it lives in the app: the drawing is pure (pixels from a reading) and belongs in `dyno_core::tray`, replacing today's
  renderers; the font bytes come in through a small port (`FontSource`) filled by the Tauri edge.

## 2. macOS: menu-bar items

### Image

- **Template images only** (black plus alpha; `isTemplate`), so the bar tints them for light, dark, tinted wallpapers,
  inactive displays and the highlight. Never full colour (design guide §5, "template (monochrome) tray icon, never full
  colour"). Alpha levels carry meaning: ink 100 %, tracks 28 %, area fill 38 %, graph plate 14 %, dimmed item 45 %.
- **18 pt high** (tray-icon sets every status image to 18 pt), width per format (§2.3), always a whole number of points.
- **Two representations**, drawn separately: 1x and @2x, the same point size. Today a 36 px image is shrunk on 1x screens
  (`today-vs-proposed.png`, "Today at 1x"). The menu bar shows on every display, so one NSImage carries both.
- **Pixel rules:** rectangles, bars, meters and baselines snap to whole device pixels at each scale; text and arcs keep
  their anti-aliased outlines; nothing is thinner than 1 device pixel (the graph line is max(1 px, 0.75 pt): 1 px at 1x,
  1.5 px at @2x).

### Type and alignment

| Role | Size | Weight (wght) | Figures | Tracking |
|---|---|---|---|---|
| One-line value ("23%", "54°") | 13 pt (the menu-bar size) | 400 | tabular | 0 |
| Label before a value or a mark ("CPU") | 9.5 pt, capitals | 620 | | +0.35 pt |
| Stack: label line | 8 pt, capitals | 620 | | +0.45 pt |
| Stack: value line | 11 pt | 510 | tabular | 0 |
| Network pair | 9 pt | 510 | tabular | 0 |
| Gauge letter | 7.5 pt, capital | 700 | | 0 |

- **Baseline** of one-line text: 13.5 pt below the image top (@2x: 27 px; 1x: 14 px, the nearest whole pixel). The image
  is centred in the 24 pt bar, so this is 16.5 pt below the bar's top: the caps of 13 pt text centred on the bar, like the
  clock's. Stack baselines: 7 and 16.5 pt (1x: 7 and 17). Network baselines: 7.5 and 16.5 pt. **To verify on a device**:
  overlay a screenshot of the real clock at 1x and @2x; this was derived from the font metrics, not measured on screen.
- **Tabular digits:** all ten digits measure 8.188 pt at 13 pt with `tnum`; without it the 1 is 6.030 pt, so "11%" and
  "18%" would differ by 2.16 pt and move everything left of them.
- Labels are capitals in the item; the user's own label text (`MenuBarItem::Number.label`, up to 16 characters) is shown
  as typed in the popover and tooltip; the item shows it in capitals (proposal: at most 4 characters in the item).

### Formats and fixed widths

A number field always reserves the widest value of its format (three tabular digits plus the unit); the value sits
right-aligned in it, so the ones digit never moves and the width never changes with the value. Measured on this Mac
(rounded up to whole points; the app computes them from the font at start-up, never from the value):

| Format (`MenuBarItem`) | Drawing | Width |
|---|---|---|
| Number, one line | `CPU` label, 4 pt gap, field `000` + `%` | 62 to 66 pt (CPU 64, MEM 66, BAT 62) |
| Number, stacked (new layout option) | label over value, both right-aligned | 32 pt |
| HorizontalMeter | label, 4 pt gap, a 24 x 6 pt capsule meter | 49 to 54 pt (GPU 51) |
| VerticalBars, one metric | label, a 5 x 14 pt bar | 34 pt (MEM) |
| VerticalBars, per core | one 2 pt column per core, 1 pt gaps, 14 pt high | 3 n - 1 pt (10 cores: 29) |
| LineGraph | label, a 30 x 14 pt area graph on a 14 % plate, one sample per point | 55 to 60 pt (DSK 57) |
| GraphAndNumber | a 24 x 14 pt graph, 4 pt gap, the stack | 60 pt |
| Ring | a 270-degree gauge (14 pt across, 2 pt stroke, open at the bottom) with the metric's letter | 17 pt |
| Network (any format with NetworkUp/Down) | up over down: arrow, `000` field, unit (`B/s`, `KB/s`, `MB/s`, `GB/s`) | 49 pt |
| Temperature (Number) | a thermometer (1 pt outlined stem, filling 20 to 110 °C, solid bulb), field `000` + `°` | 40 pt |

- Labels sit 1.5 pt in from the left edge, so an alert plate fits inside the image.
- Rates: at most three figures, "9.9" below 10 then whole numbers, switching unit at 999.5 so "1000" never shows
  (measured: 999.4 B/s gives "999 B/s", 999.6 gives "1.0 KB/s", 24 300 000 gives "24 MB/s").
- Spacing between items is the system's (`NSStatusItem` padding); the images carry no outer padding.

### States

| State | Drawing | Never by colour |
|---|---|---|
| Normal | as above | |
| Highlighted (popover or menu open) | the system's highlight behind the template; nothing changes in the image | |
| Alert (over the metric's threshold) | the number on a solid plate with its digits knocked out; items without a number invert their label, the gauge its letter | a shape change, visible in any tint and in greyscale |
| Unavailable (cannot be measured, or the agent away) | an en dash in the field, the whole item at 45 % alpha; the same width | |

- Alert hysteresis: enter at the threshold held for 3 s, leave 5 points below it, so the plate does not blink at the edge.
- Colour mode for alerts on macOS: not proposed. A non-template image stops adapting to the bar and the design guide forbids
  full colour there. If the user wants it, it is an opt-in setting and keeps the plate.

## 3. Windows: notification area

- **Exact sizes:** 16 px at 100 %, 20 at 125 %, 24 at 150 %, 32 at 200 % (Windows also uses 40 at 250 %: use the 32 layout
  centred, or add a 40 layout). Render for the taskbar's DPI (`GetDpiForWindow` on `Shell_TrayWnd`), and again when it
  changes (`WM_DPICHANGED`, `WM_SETTINGCHANGE`). Today one 32 px image is shrunk by Windows.
- **Colours:** numerals and labels white on a dark taskbar, `#1B1B1F` on a light one (15.5 to 16.3:1). Meters, graphs and
  gauges in Dyno's accent from the design guide: dark `#F063A8` (5.47:1 on `#202020`), light `#B01E6E` (5.81:1 on `#F3F3F3`).
  Tracks are the ink at 30 % (2.44:1 dark, 1.95:1 light): decoration only, the fill and the number carry the value.
- **Alert:** the number on a plate in the family's danger colour: dark `#FF6B6B` with `#14171C` digits (plate 5.87:1 on the
  taskbar, digits 6.47:1 on it), light `#C42B2B` with white digits (5.08:1 and 5.63:1). The plate is a shape, so it reads
  without colour too.
- **Layouts** (`pix::layout`):

| Size | Label | Numerals (w x h, stroke) | Meter |
|---|---|---|---|
| 16 | none (the tooltip names the metric) | 6 x 10, 2 px; a bare 1 when three digits must fit ("100" is 16 px) | 2 px, rows 13 to 14 |
| 20 | none | 6 x 11, 2 px | 3 px |
| 24 | 5-row capitals | 7 x 12, 2 px | 2 px |
| 32 | 7-row capitals | 9 x 16, 3 px | 3 px |

- Numerals are segments on whole pixels with the outer corners chamfered by stroke - 1 (at least 1 px). They are Dyno's
  own "dash display" figures, not a system font.
- Per kind: percent (number and meter), meter, per-core bars, graph, 270-degree gauge with a letter, network (at 16 and 20
  two log-scale meters with arrows; from 24 two lines of small numerals with K, M or G), temperature (at 16 and 20 two digits
  with a 3 px degree ring tucked in the corner; from 24 with "°" and a label).
- Ink coverage at 16 px on a dark taskbar (alpha >= 50 %): percent 35 %, "100" 54 %, temperature 37 %, network 25 %,
  per-core 16 %, gauge 15 %, graph 11 %; the alert plate 75 % by design. (The family's 18 to 30 % coverage rule is for app
  glyphs; these are data displays and are listed for comparison only.)

## 4. Linux: appindicator

- 22 px icons, the Windows drawing at its 22 px layout (5-row label, 6 x 11 numerals, 2 px meter), light or dark from the
  portal's colour scheme (dark when unknown: GNOME's top bar is always dark).
- Where the host shows an indicator label (not verified per desktop; check GNOME's AppIndicator extension and KDE), a one-line value can go there
  instead: tray-icon's Linux `set_title` calls `app_indicator_set_label(title, "")`; pass a guide string ("CPU 100%") so the
  panel reserves a fixed width. The panel draws that text in its own font.

## 5. Updates and the flicker

Causes found in the code (tray.rs on main and the vendored tray-icon 0.25.1 on `feat/dyno-tray-per-metric`):

1. `paint` calls `set_icon` and then `set_icon_as_template`. tray-icon's macOS `set_icon` installs the new image with
   `template = false` and `set_icon_as_template` flips it afterwards: for one display pass every update is a black,
   non-template image (invisible on a dark bar, then white). This is the likely main flicker; every frame does it.
2. In `set_icon_for_ns_status_item_button` the image is set on the button before its size and template flag, so the
   button can lay out once at the PNG's pixel size (36 pt) and then at 18 pt.
3. `set_tooltip` runs on every frame and calls `update_dimensions` each time.
4. A width that follows the value (any text item drawn at its natural width) moves every item to its left; today the
   number re-centres inside its square as the digit count changes.
5. `show_disconnected` swaps live items for the square static icon (a different width) whenever the agent is briefly away.

Rules:

- One image per update, set once: `set_icon_with_as_template(Some(image), true)` on macOS.
- Redraw only when the pixels change: compare the formatted text and the quantised drawing (fill lengths in device pixels),
  not the float. The `Frame` comparison in `tray_model` already does this; keep it.
- Cadence: values follow the agent's 1 Hz sample; graphs advance one column per sample; the 2 Hz throttle stays as a cap.
- Tooltip: set only when its text changed.
- Agent away: after 3 s, the unavailable state (same width, dimmed, en dash), not the static icon.

## 6. What the implementer changes

In `apps/dyno/app/src-tauri/src/tray.rs`:

- `paint`: replace `set_icon` + `set_icon_as_template` with one `set_icon_with_as_template(Some(icon), cfg!(macos))`;
  build the image with its own width and height (`Image::new_owned(rgba, w, h)`, not `size, size`); set the tooltip only
  when it changed.
- `show_disconnected`: debounce 3 s and paint each item's unavailable frame instead of the static icon.
- `create_item`: start each item with its unavailable frame at its final width, not the square static icon.
- Windows: pass the taskbar's pixel size (16, 20, 24, 32) to `TrayItems` and re-render on DPI and theme changes
  (`dark_taskbar` is read once today).

Elsewhere:

- Vendored tray-icon (`apps/dyno/vendor/tray-icon`, `vendor/PATCHES.md`): in `set_icon_for_ns_status_item_button`, set the
  size and the template flag before `setImage`; accept two images (1x and @2x) and add both as `NSBitmapImageRep`s of one
  NSImage.
- `dyno_core::tray`: the renderers of `render/src/mac.rs` and `render/src/pix.rs` (pure, fonts through a port), with
  tests that the width is constant over values and states, that 1x and @2x share the point size, and that no device-pixel
  element is under 1 px.
- `tray_model`: map `MenuBarItem` and metric to these formats (network and temperature get their own); a "stacked" layout
  option for `Number`; alert hysteresis; the format of rates above.
- Font loading on macOS through CoreText's menu font URL (§1); a fallback to today's square glyphs when it fails, logged.

## 7. Where it still falls short

- The baseline and the item spacing are derived from font metrics and the 18 pt image rule, not measured against a live
  menu bar; check on a 1x and a Retina display before switching.
- The one-line value is wide (64 pt for "CPU 23%") because it reserves "100%"; the reserved space shows as a gap after the
  label at small values.
- The gauge letter is 7.5 pt; at 1x it is legible but small. The gauge is the weakest format for telling metrics apart.
- At 16 px on Windows nothing names the metric; the user relies on the order (stable since ADR 0028) and the tooltip.
- Windows and Linux mockups use stand-in system glyphs and a stand-in clock font (SF, not Segoe UI or Cantarell).
