# Icons

The family icon set: app icons for Debrief, Pitcrew, Spotter, Pitwall, Dyno, Sighter and Pacenotes. All seven are approved.
Open `preview.html` to review them, or see the renders in `screenshots/`.

| App | Status |
|---|---|
| Debrief, Pitcrew, Spotter, Pitwall, Dyno | Approved |
| Sighter | Approved (2026-10-23) |
| Pacenotes | Approved (2026-10-23) |
| Paddock Shell Demo (a demo app, not a family member) | New (2026-10-24), awaiting independent review |

## Demo apps

`DEMOS` in `tools/design.mjs` are built by the same pipeline (`ONLY=shell-demo node tools/build.mjs`) but stay out of
the family sheet (`preview.html`) and the Carbon concept. Paddock Shell Demo (`apps/shell-demo`) is a neutral silver
tile (`#D1D4DA` on a `#DDE1E7` to `#B5BCC7` gradient with a 1 px inner rim at 8 % black, so its outline holds on white; dark ink `#1E2530`, 10.4:1) with the shell itself as its glyph: a window with its title bar, the pages
sidebar and the page. Silver is light like Spotter's tile, so it takes the dark ink and the light-tile material. Its tray template is solid
(alpha only: half-transparent parts would read as disabled). A search
over low-chroma tiles (`tools/cvd.mjs`) kept the family's minimums unchanged (normal 17.7, protan 11.6, deutan 13.6,
tritan 10.7) only for near-black and light silver; mid greys fell to 1.6 to 5.4 against Dyno or Pitwall. Its
`tauri/` folder is copied to `apps/shell-demo/src-tauri/icons` (`node scripts/shell-demo-release.mjs icons` checks
they are equal).

## Build

```sh
cd icons
npm ci
npm run all                  # everything: SVG, PNG, tray, Tauri sets, .ico, preview, screenshots
ONLY=pacenotes node tools/build.mjs   # one app
```

Edit only `tools/design.mjs`; every SVG is generated from the geometry there. Output is deterministic except `tauri/icon.icns`,
which embeds build-specific bytes.

## How apps consume the icons

Each `<app>/tauri/` folder is a drop-in replacement for the app's `src-tauri/icons`. Tray icons are in `<app>/tray/`.
The ecosystem image uses a copy of each macOS master in `docs/images/src/icons/` (refresh it after changing an icon, then run
`node docs/images/src/ecosystem.mjs --png`).

## Chosen direction: Enamel

Each app has a tile in its own saturated hue, one bold glyph, a faint diagonal "livery" sweep and light from above. It reads as premium software first; the motorsport theme only shows in the livery and the metaphors. The alternative direction, **Carbon** (a graphite tile with the glyph lit in the app's hue), is in `alternative/` and at the bottom of the preview. It was not chosen because the five dark tiles look alike at 16 to 32 px and sink into dark docks and taskbars.

## Grid and shape language

- **macOS master (1024):** an 824 px body at (100,100) with continuous-curvature corners (r 185.4, Big Sur grid). Shadow: y 10, blur 12, black at 28%.
- **Windows master (1024):** a 944 px rounded square at (40,40) with circular corners (r 200) and no outer shadow. Windows 11 guidance favours glyphs without a backplate, but the tile is kept for three reasons: the hue is the strongest cue at 16 to 24 px, it guarantees contrast on both light and dark taskbars, and it keeps the family consistent across platforms. The margin is cut because Windows does not apply the macOS safe area, so the icon would otherwise look small in the taskbar.
- **Glyph area:** about 270 to 754 on the 1024 grid, centred optically.
- **Strokes:** about 60 to 68 on the 1024 grid, with round caps and joins.
- **Two tones per glyph:** primary ink at 100% and secondary ink at 60% (55% on Spotter).
- **Material:** a vertical gradient (top, base, bottom), a top-left radial sheen, an inner edge (light at the top, dark at the bottom), the livery sweep (two diagonal bands of white at 8%), and a glyph shadow in the app's deep tone.
- **Small frames (`<app>/small/`):** 16 is drawn on its own 16 px grid. 24, 32 and 48 come from one 32 px grid. They are snapped to the pixel grid, drop detail (no eyepieces on Spotter, no bottom line on Debrief at 16 px), and add a 0.5 px edge so the tile reads on light taskbars. These frames go into `icon.ico`.

## Palette

| App | Base | Top → Bottom | Deep | Ink | Ink contrast on base |
|---|---|---|---|---|---|
| Debrief | `#2E6BF0` | `#5590FF` → `#1D4FD3` | `#10308A` | white | 4.7:1 |
| Pitcrew | `#EF5F1C` | `#FF8A4C` → `#D4440D` | `#8A2A04` | white | 3.3:1 |
| Spotter | `#F4B315` | `#FFD257` → `#E59600` | `#7A4E00` | `#3A2600` | 7.8:1 |
| Pitwall | `#5A24B0` | `#7B46DB` → `#43198F` | `#24094F` | white | 9.1:1 |
| Dyno | `#BE3584` | `#DE559F` → `#A0226A` | `#5E0E3C` | white | 5.2:1 |
| Pacenotes | `#6D590F` | `#9A8228` → `#6A5500` | `#302501` | white | 6.8:1 |
| Sighter | `#A686D6` | `#BFA2EE` → `#8B6DB5` | `#3C2D52` | `#2D1D43` (dark) | ink 5.1:1 on base |

- **Ink rule:** the glyph is white wherever white reaches at least 3:1 on the tile. Spotter's amber and Sighter's lavender tiles are the light ones, so they take dark ink.
- **Colour-vision check:** the hues were picked by a search in OKLCH that maximises the smallest pairwise OKLab distance under simulated protanopia, deuteranopia and tritanopia (Machado 2009, full severity). The smallest ΔE (OKLab ×100) is 17.7 for normal vision, 11.6 for protanopia, 13.6 for deuteranopia and 10.7 for tritanopia. Run `node tools/cvd.mjs '{"set":{...}}'` to check a new palette.
- **Hue is never the only cue:** each glyph has its own silhouette.
- **Status colours on the Windows tray:** busy `#2F8FFF` (ring), warning `#F5A524` (triangle), error `#E5484D` (cross), new `#E5484D` (dot), serving `#2FB36B` (dot), high load `#F2711C` (dot plus a pegged needle). Each status also has its own shape.

## Per-app concept

- **Debrief:** log lines, a telemetry trace and an event marker. It evolves the IMT icon (same blue, lines and pulse). Nothing nautical, to stay clear of the unrelated maritime tool also called Debrief.
- **Pitcrew:** a wheel nut inside a cycle arrow: keep runners alive, swap them safely, clean up.
- **Spotter:** binoculars and a spark: it watches everywhere and calls the moment a release appears.
- **Pitwall:** a radio bubble with the voice kept inside it: a private, local model.
- **Sighter:** a pointer arrow inside a sighting reticle (four ring arcs with ticks through the gaps): it finds UI elements by sight. Lavender tile `#A686D6` (2026-10-24, with the plum UI accent; it replaced the first cyan tile): the closest tile to the plum accent that keeps all four CVD minimums of the family (17.7 normal, 11.6 protan, 13.6 deutan, 10.7 tritan); a plum tile `#975EA2` dropped them to 10.2, 6.7, 5.1 against Dyno. Lime was rejected earlier (3.3 to 4.3 dE from Spotter or Pitcrew under protan/deutan). Dark ink (`#2D1D43`, 5.1:1) because white reaches only 3.0:1 on the light tile.
- **Pacenotes:** a portrait spiral notebook with a second page peeking out behind it, rings through punched holes on the left edge, ruled lines, a small route curve drawn like a pacenote and a compact `</>` mark: notes about the road ahead, here the shape of a codebase. The first version (a squarish card with flat tabs) was rejected as not reading as a notebook. At 32 px the code mark drops out, at 16 px only rings, page and one curve remain. A variant with the spiral along the top is `alternative/pacenotes-top-spiral` (the left spiral was clearer at 16 px); the first-round concept `alternative/pacenotes-brackets-road` is kept too. Bronze-olive tile `#6D590F` (2026-10-24, with the olive UI accent; it replaced the first crimson tile): the closest tile to the olive accent that keeps all four CVD minimums (an olive tile `#657C1F` dropped protanopia to 1.6 against Pitcrew). White ink gives 6.8:1.
- **Dyno:** a dynamometer gauge with a load arc and the needle near the redline.

## Layout

```
<app>/<app>-macos.svg, <app>-windows.svg       1024 masters (pure vector)
<app>/<app>-macos-1024.png, -windows-1024.png  rendered masters
<app>/small/<app>-{16,24,32,48}.svg|png         hand-tuned small frames
<app>/tray/svg/                                 tray sources (template / light / dark)
<app>/tray/macos/<name>-{16,18}ptTemplate[@2x].png   menu-bar template images
<app>/tray/windows/<name>-{light,dark}-{16,20,24,32}.png and .ico
<app>/tauri/                                    drop-in replacement for src-tauri/icons
alternative/<app>-carbon.svg                    Carbon concept
preview.html, screenshots/                      review sheet and renders
tools/                                          generators (design.mjs is the source of truth)
```

- **Tray names:** `<app>-tray` when the app has no states (Debrief), otherwise `<app>-tray-<status>`. The statuses are Pitcrew ok/busy/warning/error, Spotter normal/new, Pitwall idle/serving/warning and Dyno normal/high.
- **Light and dark tray icons:** "light" means a light taskbar (dark glyph) and "dark" means a dark taskbar (white glyph).
- **The `tauri/` set:**
  - `icon.icns` and the PNGs come from the macOS master.
  - `Square*Logo.png` and `StoreLogo.png` come from the Windows master.
  - `icon.ico` is rebuilt with the hand-tuned frames: 16, 20, 24, 32, 40 and 48 hand-tuned, 64 and 256 from the Windows master.
  - The `android/` and `ios/` outputs are removed.

