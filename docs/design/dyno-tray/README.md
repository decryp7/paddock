# Dyno menu-bar and tray items: design spec

Status: proposal 2026-11-03, revised after the first independent review (2026-11-03); for the user's decision. Answers the
user's feedback of 2026-11-03: "the metric icon should be more professional like iStat Menus" and "the metric icon
flickering on macOS". iStat Menus set the quality bar only; nothing here copies its artwork or layouts.

Sheets, rendered by the code that would ship (§1):

| Sheet | What it shows |
|---|---|
| `macos-menubar@1x.png`, `macos-menubar@2x.png` | the default set (4 items), then 9 items beside the system items: light bar, dark bar (one item highlighted), transparent bar on a tinted wallpaper (one item in alert) |
| `macos-items@2x.png` | every format in 4 states on a light and a dark bar, its fixed width; close-ups of the 1x image, the 1x alert and the @2x image |
| `windows-tray.png` | the notification area at 100, 150 and 200 % on light and dark taskbars; close-ups at 16, 20, 24 and 32 px |
| `linux-panel.png` | GNOME top bar (dark) and a KDE panel (light) at 22 px |
| `today-vs-proposed.png` | the same items as `dyno_core::tray` draws them today and as proposed: macOS @2x and 1x, Windows at 100 % |
| `numerals.png` | Dyno's dash numerals and label capitals for the Windows and Linux icons |

## 1. Code and drawing route

- **Renderers, pure, in `dyno_core::tray`:** `menu_bar` (macOS templates), `pixel` (Windows and Linux icons) and `text` (the
  text port `TextRaster`, with `BoxText` for tests). Pixels come from `tiny-skia` 0.11 (BSD-3-Clause, pure Rust), now a
  dyno-core dependency, so CI, cargo-deny (`bans ok, licenses ok`) and Renovate cover it. Nothing in the app uses them yet.
- **macOS text through CoreText**, behind the port: AppKit's `systemFontOfSize:weight:` (or
  `monospacedDigitSystemFontOfSize:weight:` for tabular figures), toll-free bridged to `CTFont`, set as a `CTLine` with the kern
  attribute and drawn into an alpha-only `CGBitmapContext`. The system chooses the optical size; no font file is read and
  nothing is bundled or fetched. Crates: `objc2-app-kit`, `objc2-core-text`, `objc2-core-graphics`, `objc2-core-foundation`
  0.3.2 (already in Dyno's `Cargo.lock` through Tauri). The adapter is `crates/dyno-core/examples/tray_sheets/coretext.rs`; the
  app's edge takes the same code.
- **Windows and Linux: no font.** Dyno's dash numerals and 5- and 7-row capitals, drawn on whole pixels for each exact size.
- **Sheets:** `cd apps/dyno && cargo run -p dyno-core --release --example tray_sheets` (macOS; it also prints every number
  below). Tests: `cargo test -p dyno-core --lib tray::` (width constant over values, states and NaN; 1x and @2x share the point
  size; a little load shows at least 1 device pixel; alert is a plate; the Windows alert below 24 px; whole pixels).

## 2. macOS menu-bar items

### Image

- **Template images only** (black plus alpha), so the bar tints them for light, dark, tinted wallpapers, inactive displays and
  the highlight. Never full colour (design guide §5). Alpha: ink 100 %, tracks 28 %, area fill 38 %, graph plate 14 %,
  unavailable item 45 %.
- **18 pt high**, one fixed width per format in whole points; **two representations**, drawn separately at 1x and @2x (the
  menu bar shows on every display, so one NSImage carries both).
- Rectangles, bars and baselines snap to whole device pixels at each scale; text and curves stay anti-aliased; nothing is
  thinner than 1 device pixel (the graph line is max(1 px, 0.75 pt)).

### Type

| Role | Size | Weight (NSFontWeight) | Figures | Tracking |
|---|---|---|---|---|
| One-line value | 13 pt (the menu-bar size) | regular | tabular | 0 |
| Label before a value or mark | 9.5 pt capitals | semibold | | +0.35 pt |
| Stack: label line | 8 pt capitals | semibold | | +0.45 pt |
| Stack: value line | 11 pt | medium | tabular | 0 |
| Network pair | 9 pt | medium | tabular | 0 |

- **Baseline** of one-line text 13.5 pt below the image top (@2x 27 px, 1x 14 px): 16.5 pt below the 24 pt bar's top, the caps
  of 13 pt text centred like the clock's. Stack baselines 7 and 16.5 pt; network 7.5 and 16.5 pt. **Verified during the VM
  run** with a screenshot overlay against the real clock at 1x and @2x.
- Tabular figures: every digit is 8.112 pt at 13 pt; a proportional "1" is 5.954 pt, so "11%" against "18%" would move 2.16 pt.

### Formats and fixed widths

A number field reserves its widest value (three tabular digits and the unit) and the number sits right-aligned in it, so the
ones digit never moves and the width never changes. Measured with CoreText on this Mac (the app measures at start-up):

| Format (`MenuBarItem`) | Drawing | Width |
|---|---|---|
| Number, **stacked (the default)** | label over value, right-aligned, 2 pt inset each side | 37 pt |
| Number, one line | the label hugging the number ("CPU 23%"), the reserved space to its left | 61 to 65 pt (CPU 63) |
| HorizontalMeter | label, 4 pt gap, 24 x 6 pt capsule | 50 to 54 pt (GPU 52) |
| VerticalBars, one metric | label, 5 x 14 pt bar | 33 to 34 pt |
| VerticalBars, per core | 2 pt column per core, 1 pt gap, 14 pt high | 3 n - 1 pt (10 cores: 29) |
| LineGraph | label, 30 x 14 pt area graph on a plate, a sample per point | 56 to 60 pt (DSK 57) |
| GraphAndNumber | 24 x 14 pt graph, 4 pt gap, the stack | 65 pt |
| Ring | **drawn as the meter on macOS**: the 270-degree gauge read as a spinner at 18 pt and was the weakest format | as the meter |
| Network (NetworkUp/Down) | up over down: arrow, `000` field, unit `B/s` to `GB/s` | 50 pt |
| Temperature | thermometer (1 pt outlined stem filling 20 to 110 °C, solid bulb), `000°` | 40 pt |

- The one-line format's reserved space ("100%" needs three digits) now sits left of the label instead of between label and
  number ("CPU   23%" in the first round): the label moves only when the digit count changes, which never moves a neighbour.
- Rates: at most three figures, "9.9" below 10, the unit switches at 999.5 so "1000" never shows.

### States

| State | Drawing |
|---|---|
| Normal | as above |
| Highlighted (popover or menu open) | the system's highlight behind the template; the image is unchanged |
| Alert | the number on a solid plate, knocked out; a stack is one full-height plate (room around both lines at 1x: the "1x alert" close-ups); items without a number invert their label. A shape, so it reads in any tint and without colour |
| Unavailable (not measurable, or the agent away) | an en dash in the field, the item at 45 %; the same width |

- Alert hysteresis: enter at the threshold held for 3 s, leave 5 points below it.
- No colour mode on macOS (a non-template image stops adapting to the bar; the design guide forbids full colour there).

### How many items

- **Default: 4 items** (CPU and memory stacks, network, temperature: 37 + 37 + 50 + 40 pt plus the system's spacing, about
  200 pt). Up to 32 can be added; the sheet's nine-item bar already reaches the app menus at 1180 pt.
- **Overflow on macOS:** the menu bar does not wrap or scroll. When the active app's menus need the space, or on a notched
  MacBook where extras would sit under the notch, macOS hides the status items that do not fit, with no indicator. The settings
  page says so beside the item list and suggests the stacked format (the narrowest number) beyond 4 items.
- **Overflow on Windows 11:** icons the user has not pinned to the taskbar go to the overflow flyout (the chevron); each keeps
  its place there through its GUID (ADR 0028).

## 3. Windows notification area

- **Exact sizes:** 16 px at 100 %, 20 at 125 %, 24 at 150 %, 32 at 200 % (40 at 250 %: the 32 layout centred). Render for the
  taskbar's DPI (`GetDpiForWindow` on `Shell_TrayWnd`) and again on `WM_DPICHANGED` and `WM_SETTINGCHANGE`.
- **Colours and contrast**, measured against flat stand-ins for the taskbar behind the figures: `#202020` (dark) and
  `#F3F3F3` (light). The real Windows 11 taskbar is Mica, tinted by the wallpaper; the VM run re-measures on screenshots.

| Pair | Dark | Light |
|---|---|---|
| Numerals and labels (white / `#1B1B1F`) on the taskbar | 16.29:1 | 15.47:1 |
| Fill (accent `#F063A8` / `#B01E6E`) on the taskbar | 5.47:1 | 5.81:1 |
| Track (ink at 30 %: `#636363` / `#B2B2B3`) on the taskbar | 2.71:1 | 1.91:1 |
| Fill against its own track | 2.02:1 | 3.04:1 |
| Alert plate (`#FF6B6B` / `#C42B2B`) on the taskbar, 24 px and up | 5.87:1 | 5.08:1 |
| Digits on the plate (`#14171C` / white) | 6.47:1 | 5.63:1 |

  Tracks are below 3:1, and the fill against the track in dark is 2.02:1: a value-only item (meter, graph, per-core bars, the
  network meters at 16 and 20 px) is read by the fill against the **taskbar** (5.5 to 5.8:1) and by its length, not against
  the track. The track only marks the full scale.
- **Alert:** from 24 px, the number on a danger plate with a 1 px margin. **Below 24 px** (16, 20 and Linux's 22) the digits
  stay solid ink and the alert is a **corner notch** (a 4 or 5 px triangle at the top left) **plus the bar under the digits in
  the danger colour**: a shape and a colour. (The first round's knocked-out "93" read as "9E" at 16 px.) In dark mode the danger
  bar and Dyno's pink are near in hue; the notch carries the alert in any colour vision.
- **Layouts** (`pixel::layout`):

| Size | Label | Numerals (w x h, stroke) | Meter |
|---|---|---|---|
| 16 | none (the tooltip names the metric) | 6 x 10, 2 px; a bare 1 when three digits must fit ("100" is exactly 16 px) | 2 px, rows 13 to 14 |
| 20 | none | 6 x 11, 2 px | 3 px |
| 22 (Linux) | 5-row capitals | 6 x 11, 2 px | 2 px |
| 24 | 5-row capitals | 7 x 12, 2 px | 2 px |
| 32 | 7-row capitals | 9 x 16, 3 px | 3 px |

- Per kind: percent (number and meter), meter, per-core bars (each with a full-height track, so the icon keeps its frame when
  idle; at least 1 px of fill above 0.5 %), graph (on a faint plate with a baseline track; a sample above 0.5 % rises at
  least 1 px above the track), gauge with the metric's letter, network (at 16 and 20 two log-scale meters with arrows; from 24
  small numerals with K, M or G), temperature (at 16 and 20 two digits with a 3 px degree ring in the corner).
- Ink coverage at 16 px on a dark taskbar (alpha >= 50 %): percent 35 %, "100" 54 %, temperature 37 %, network 25 %,
  per-core 16 %, gauge 15 %, graph 11 %, alert 48 %. (The family's 18 to 30 % rule is for app glyphs; listed for comparison.)

## 4. Linux appindicator

- 22 px icons, the Windows drawing at its 22 px layout, light or dark from the portal's colour scheme (dark when unknown).
- Where the host shows an indicator label (not verified per desktop), a one-line value can go there instead: tray-icon's Linux
  `set_title` calls `app_indicator_set_label(title, "")`; a guide string ("CPU 100%") would fix its width.

## 5. The flicker: what is fixed and what is open

Read on `feat/dyno-tray-per-metric` at a71acb4f2. Already there (67dfbf4d8): one `set_icon_with_as_template` per tick
(`paint_ops`, `image_sets == 1`), the tooltip only when it changed, only changed items re-made (`plan_sync`), a picture keeps
its size across 0 to 100 %, no redraw without a change, and a debug line per picture (`tray icon <id>: picture #n set (1
call, ...)`).

Still open, most likely first:

1. **tray-icon's macOS `set_icon_for_ns_status_item_button` calls `setImage` before `setSize` and `setTemplate`.** Even with
   one `set_icon_with_as_template` per tick, the button holds for that moment an image that is not yet a template (black:
   invisible on a dark bar) and at its pixel size (36 pt for a 36 px image), before it is resized and made a template. A display
   pass between the calls shows the wrong image.
2. **The square image** (`Image::new_owned(rgba, size, size)` in `paint`): any format wider than 18 pt needs its own width.
3. **`show_disconnected` swaps in the static icon** (another size and picture) whenever the agent is briefly away; the live
   picture returns on the next update.

**Confirm before fixing (ER-28):** a cause is confirmed only by the debug "picture #n" lines (one set per tick, and when the
static icon came in) together with a screen recording of the menu bar on the 1.0.0 build and on the fix, with flickers counted
per minute: without the fix the recording flickers; with it, it stays still as often as the rate requires.

Update rules (unchanged): values follow the agent's 1 Hz sample, graphs advance one column per sample, the 2 Hz throttle stays
as a cap, and a frame is sent only when its pixels change.

## 6. What the implementer changes

On top of `feat/dyno-tray-per-metric`:

- `tray_model`: `render_glyph` returns the new pictures through its existing `Rendered { icon, width }`: `menu_bar::render` on
  macOS (two templates, 1x and @2x, at the item's width) and `pixel::render` on Windows and Linux at the taskbar's pixel size.
  Map `MenuBarItem` to formats (network and temperature get their own; `Ring` is the meter on macOS); make stacked the default
  number layout; add alert hysteresis.
- `tray.rs`:
  - `paint`: build the image at its own width (not `size, size`); on macOS hand both representations to the new tray-icon call
    below, reached through `tray.with_inner_tray_icon(|t| t.set_icon_reps(...))`, since Tauri does not expose it.
  - `show_disconnected`: wait 3 s, then paint each item's unavailable frame (same width, dimmed, en dash) instead of the static
    icon.
  - `create_item`: start each item with its unavailable frame at its final width.
  - Windows: pass the taskbar's pixel size to `TrayItems` and re-render on DPI and theme changes.
  - The CoreText adapter (`examples/tray_sheets/coretext.rs`) moves to the app's edge, behind `TextRaster`.
- **Vendored tray-icon, a second patch file** `apps/dyno/vendor/tray-icon-image-reps.patch`, with its own entry and removal
  line in `vendor/PATCHES.md`:
  - in `set_icon_for_ns_status_item_button`, build the NSImage completely (size, template flag) before `setImage`;
  - a new macOS `TrayIcon::set_icon_reps(reps: &[(rgba, width_px, height_px)], size_pt, template)`: one NSImage with one
    `NSBitmapImageRep` per scale, each made from the RGBA bytes directly (`initWithBitmapDataPlanes:...` in
    `NSDeviceRGBColorSpace`, no PNG round trip), sized in points, template set, then one `setImage`.
  - Removal: when tray-icon builds the image before setting it and accepts several representations (an upstream issue on
    tauri-apps/tray-icon, to be filed by the coordinator), delete the patch and its PATCHES.md entry and call the upstream API.

## 7. Where it still falls short

- Baseline and spacing come from font metrics and the 18 pt rule; the VM run verifies them against a live menu bar.
- The one-line number still reserves "100%": at small values the item has about 8 pt of space before its label.
- At 16 and 20 px on Windows nothing names the metric; the order (stable through ADR 0028) and the tooltip do.
- In dark mode the danger bar and Dyno's pink are close in hue; the notch, not the colour, says "alert".
- The Windows and Linux strips use stand-in system glyphs and a stand-in clock font (SF, not Segoe UI or Cantarell), and flat
  taskbar colours instead of Mica.
