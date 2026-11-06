# Icons

The family icon set: app icons for Debrief, Pitcrew, Spotter, Pitwall, Dyno, Sighter and Pacenotes (all seven approved), and the
Paddock mark itself (the umbrella, see below).
Open `preview.html` to review them, or see the renders in `screenshots/`.

**Since 2026-10-27 the family is isometric:** each app and Paddock is a rendered motorsport object (Debrief an engineer's laptop,
Pitwall a team radio, Sighter a camera on a tripod, Pitcrew an impact wrench, Spotter binoculars, Pacenotes a tablet with a stylus,
Dyno a tachometer pod, Paddock the pit building) on the unchanged enamel tiles, with hand-drawn small frames (2:1 pixel isometric at
32 and 24, a flat symbol at 16 and in the tray). The rules are in [STYLE.md](STYLE.md). The flat set it replaced (and its Carbon
concept) is kept for reference in `archive/2026-10-flat/`; the sections below on the flat glyphs describe that set.

| Module | Builds |
|---|---|
| `tools/iso.mjs` | projection, value ladder, shadows, cylinders, pixel isometric (shared) |
| `tools/iso-kit.mjs` | the scene kit: normal-shaded faces, chamfered prisms, cylinders, extrusions |
| `tools/art-scene.mjs` | the masters of Debrief, Pitwall, Sighter and Pitcrew (on the scene kit) |
| `tools/art-direct.mjs` | the masters, 32/24 frames and flat symbols of Spotter, Pacenotes and Dyno (on iso.mjs) |
| `tools/paddock-iso.mjs` | Paddock's pit building and its 32/24 frames |
| `tools/iso-family.mjs` | the registry of every frame and tray state of the eight icons, used by build, preview and tests |
| `tools/small-flat.mjs`, `tools/small-sheet.mjs` | the small sizes approved 2026-11-03 (STYLE.md section 6a): the flat glyph drawn per size for the 16 and 24 pt slots, the container map (`MIX`), the trays; and the review sheets in `docs/design/icons-small/` (family, before and after, zoom, clarity, trays); in the apps since 2026-11-03 |
| `tools/installed-sheet.mjs` | review sheets of the icons as installed in the apps (.icns slots via iconutil, .ico frames, Linux PNGs, trays) in `docs/design/icons-small/installed/` |
| `tools/install.mjs` | copies the sets into the apps (`src-tauri/icons`, tray PNGs, in-app copies) and `docs/images/src/icons` |

| App | Status |
|---|---|
| Debrief, Pitcrew, Spotter, Pitwall, Dyno | Approved |
| Sighter | Approved (2026-10-23) |
| Pacenotes | Approved (2026-10-23) |
| Paddock (umbrella mark) | Reviewed (2026-10-24) |
| Recce (planned app), Scrutineer (tool, dropped 2026-11-05) | Placeholders (2026-11-04); the real icons come later; see below |
| Paddock Shell Demo (a demo app, not a family member) | New (2026-10-24), awaiting independent review |
| Paddock Workbench Demo (a demo app, not a family member) | New (2026-10-25), awaiting independent review (rule 22); a near-black tile (`#050505`) with its own silhouette (activity-bar dots, a split dock with tabs on the top pane), white ink; family CVD minimums unchanged (review 3 P2-6) |

## Placeholders: Recce and Scrutineer (2026-11-04)

The user called this brand work a **placeholder**: the real icons come later. So each has one option, the masters and the 128 and
60 frames only (no hand-tuned 16 to 64 frames, no trays, no `.ico` or `.icns`), drawn with the shared helpers in
`tools/placeholder-art.mjs` and built by `node tools/placeholder-icons.mjs` into `alternative/recce-placeholder/` and
`alternative/scrutineer-placeholder/`, plus `docs/images/src/icons/<id>-placeholder.svg` (the 60 frame the posters inline) and the
sheet `alternative/placeholder-family-sheet.png` (the family row at 128 and 60, light and dark). Neither is in `APPS` or
`iso-family.mjs`; the build and the tests are unchanged.

- **Recce** (planned app, PC-wide search): a magnifier on a folded stage map, dark ink on a light jade tile `#53C09A` (ink
  `#002D1E`, 6.7:1), the route in jade. The tile is the best of an OKLCH grid search, but deutan is 13.1 against Pitcrew (family
  minimum 13.6); no colour keeps all four.
- **Scrutineer** (tool for code-signing certificates, dropped 2026-11-05): an inspection stamp on a certificate with its tick mark, white on a
  deep berry tile `#840070` (9.5:1). With Recce added, the best tile of the same search is well under the family minimums (normal
  14.4 against Pitwall, protan 10.4 against Paddock, deutan 11.8 against Pitwall, tritan 12.0 against Pacenotes): a real icon
  needs a tile decision.

## Demo apps

`DEMOS` in `tools/design.mjs` are built by the same pipeline (`ONLY=shell-demo node tools/build.mjs`; `ONLY=workbench-demo` for the workbench demo, copied to `apps/workbench-demo/src-tauri/icons`, checked by `node scripts/demo-release.mjs workbench-demo icons`) but stay out of
the family sheet (`preview.html`) and the Carbon concept. Paddock Shell Demo (`apps/shell-demo`) is a neutral silver
tile (`#D1D4DA` on a `#DDE1E7` to `#B5BCC7` gradient with a 1 px inner rim at 8 % black, so its outline holds on white; dark ink `#1E2530`, 10.4:1) with the shell itself as its glyph: a window with its title bar, the pages
sidebar and the page. Silver is light like Spotter's tile, so it takes the dark ink and the light-tile material. Its tray template is solid
(alpha only: half-transparent parts would read as disabled). A search
over low-chroma tiles (`tools/cvd.mjs`) kept the family's minimums unchanged (normal 17.7, protan 11.6, deutan 13.6,
tritan 10.7) only for near-black and light silver; mid greys fell to 1.6 to 5.4 against Dyno or Pitwall. Its
`tauri/` folder is copied to `apps/shell-demo/src-tauri/icons` (`node scripts/demo-release.mjs shell-demo icons` checks
they are equal).

Paddock Workbench Demo (`apps/workbench-demo`) has its own silhouette, not the shell's window with a sidebar: the activity bar as a
strip of three dots, then the dock split into a tall pane and two stacked panes, with two tabs on the top one (solid shapes, no outline
frame; the tray template is the same shape in one tone). Its tile is near-black `#050505` (neutral grey ramp `#1C1C1C` to `#000000`, R=G=B so the 8-bit steps cannot tint, deep `#000000`, a faint
light rim for dark docks), white ink: 20.4:1 on the base, 17.0:1 on the top stop, 21:1 on the bottom stop. The first warm stone tile
`#DAD5CB` was only 2.4 dE from the shell demo's silver. A grid search over every colour (`tools/cvd.mjs`, all family tiles plus Paddock and
the shell demo) found only near-black below about `#050609` (the shipped `#050505` is neutral) keeps the family minimums (normal 17.7, protan 11.6, deutan 13.6, tritan 10.7,
unchanged); every light or mid neutral lowered one of them, and Paddock's graphite `#2B3038` rules out a lighter near-black (`#0E1014`
already gives 13.5 normal). Against the whole family plus Paddock and the shell demo the tile's own nearest distances are Paddock 19.3
normal, 19.7 protan, 19.2 deutan, 19.2 tritan; against the shell demo 75.4 or more under all four. `ONLY=workbench-demo node tools/build.mjs`,
copy `workbench-demo/tauri/.` to `apps/workbench-demo/src-tauri/icons`. Side-by-side with the shell demo at 16, 32 and 1024 px:
`ui/docs/review/x50-workbench/demo-icons-compare.png`.

## Paddock redesign candidates (2026-10-25, awaiting the user's pick)

The current mark's gable reads as a generic home icon, its bays as list icons, and the angled stripe turns into a blob at 16 to 32
px. Three candidates are in `alternative/` as full sets (macOS and Windows masters with 1024 PNGs, hand-tuned 16/24/32/48 frames, tray
sources), built by `node tools/paddock-variants.mjs` from `PADDOCK_VARIANTS` in `tools/design.mjs`; `icons/paddock` is unchanged
until one is chosen. Comparison sheet: `alternative/paddock-variants-sheet.png` (current, A, B, C at 1024, 128, 32 and 16 px on light
and dark, beside Debrief and Pitwall). In all three the stripe has a hard stop per app in the stripe order, on whole pixels at 16, 24 and 32 px (each has its own 24 grid).

- **A, `paddock-flat-garage`:** one continuous flat canopy in the seven hues with a slight overhang, three wide roll-up doors (slatted,
  raised a little over a dark opening) between pillars, and the pit-lane line. Two bays at 16 px; the tray doors are solid.
- **B, `paddock-pit-boxes`:** the pit lane from above: the garage wall, seven coloured pit boxes, the dashed lane line and the pit wall.
- **C, `paddock-foundation`:** one course of seven coloured blocks with a neutral slab resting on it, a little wider than the base.

## Paddock in the family grammar (2026-10-25, graphite picked; awaiting the user's sign-off on the symbol)

The redesign candidates above keep the seven-colour stripe and many small parts, which break the family grammar (a single-hue tile,
one bold glyph with at most one translucent secondary, cut-outs in the tile colour, few parts). The new mark keeps the graphite tile
`#2B3038` (white ink 13.3:1; nearest app 19.5 normal, 17.9 protan, 20.1 deutan, 12.2 tritan) with one white glyph.

- **Hue search:** no saturated green or teal with white ink keeps the family CVD minimums (best about 9.2: grass `#00A768` 9.8 deutan
  against Pitcrew and 8.5 tritan against Debrief; teal `#00A389` 7.5 deutan against Dyno). The only passing green, jade `#4AC59D` with
  dark ink, sat on the deutan floor with no margin, so graphite was picked.
- **Symbol:** four options in `PADDOCK_SYMBOLS` (`tools/design.mjs`), compared in `alternative/paddock-symbols-sheet.png`
  (`node tools/paddock-symbols.mjs`): A a slatted roll-up door in one bay, B a raised door over a car nose, C a raised door over a tyre,
  D two slatted bays. An independent review ranked A first: it is the only one crisp at 16 px and in the tray (the car and the tyre
  blur at 16, two bays read as a grid). The roof is flush with the pillars (an overhang read as a table or a Pi) and the shutter hangs
  from it with the open gap at the bottom (a gap at the top made the slats float like a list). Ink coverage 21 % at 16.
- **Rim:** raised at small sizes so the tile's base holds on a dark Dock or taskbar: a full-pixel rim at 40 % at 16, 45 % at 24 to
  48, 20 % on the master.
- **Set:** `node tools/paddock-family.mjs` writes `alternative/paddock-family-graphite-*` (macOS and Windows masters with 1024 PNGs,
  hand-tuned 16/24/32/48) and `alternative/paddock-family-tray-*` (template SVG with 16/18 pt PNGs, light and dark SVG, PNG and .ico,
  glyph within x 2 to 14), and the sheet `alternative/paddock-family-sheet.png` (the family row at 128, 32 and 16 on light and dark,
  the row under simulated protanopia, deuteranopia and tritanopia, pixel-exact close-ups and the tray). `icons/paddock` is unchanged.

## Paddock isometric study (2026-10-25, awaiting the user's look)

Where the Paddock mark is shown today: the repo README (`icons/paddock/paddock-macos.svg` at 112 px), the ecosystem diagram
(`docs/images/src/icons/paddock.svg` at about 60 px, also in the profile repo), the profile README's Paddock heading (`images/paddock.png`
at 32 px), and the Gitea repo avatar. No Dock, taskbar or tray shows it: no shipped app uses it and the demo apps have their own icons.
So a richer mark suits it.

`node tools/paddock-iso-sheet.mjs` builds `alternative/paddock-iso-*` from `tools/paddock-iso.mjs` and the sheet
`alternative/paddock-iso-sheet.png` (the isometric master at 512 and 256 beside the flat mark, Pitwall and Dyno; each version in the
family row at 128 and hand-tuned 32/16 on light and dark, pixel-exact close-ups and the tray).

- **Master:** a rendered pit building in true 30 degree isometric projection, lit from the top left: a white roof slab with an
  overhang and a light edge strip, the long wall with four bays (half-open roll-up doors set back in the openings, slatted, dark
  interiors), darker end walls, a plinth, ambient occlusion under the roof and in the bays, a soft contact shadow. White and cool greys
  on the graphite tile. `masterSvg` takes it as `art` (a rendered glyph in place of the layer glyph).
- **128 frame** (`paddock-iso-128`, also the size the README and the diagram show): no slats (they alias to grey), flat doors with a
  light rail, wider pillars.
- **Small sizes, two choices:** a hand-drawn 2:1 pixel-isometric block at 32 and 24 with the flat slatted door at 16 and in the tray
  (`paddock-iso-{16,24,32,48}`, recommended by the review), or isometric down to 16 and in the tray (`paddock-iso-16-isometric`,
  `paddock-iso-tray-isometric-*`), which blurs.

## Paddock pit building (2026-10-25, round 2 of the isometric study, awaiting the user's look)

`pitBuilding()` in `tools/paddock-iso.mjs`, built by `node tools/paddock-pit-sheet.mjs` into `alternative/paddock-pit-*` and the sheet
`alternative/paddock-pit-sheet.png`: a motorsport pit building (ground floor with four lit bays and half-raised doors, a cantilevered
canopy carrying a thin pinstripe in the seven app hues, an upper hospitality floor with dark glass, the roof slab, and in front the
pit lane and the chequered pit wall), about 72 % of the tile width. A simplified frame (`paddock-pit-128`, also used at 112 and 60:
three dark bays with doors 40 % down, no slats or mullions, a coarser chequer on a taller wall), pixel-isometric frames at 32 and 24 (`PIT_SMALL`), and the flat slatted door
at 16 and in the tray. The rendering rules are in `STYLE.md`.

## Seam scan

`node tools/build.mjs` fails when a master has a hard seam in the tile (`tools/scan.mjs`, tests in `tools/scan.test.mjs`, `npm test`).
For every icon and both masters it renders the tile background alone (`masterSvg(app, { bare: true })`: no glyph, no livery sweep) at
1024 px and walks each pixel column. A pixel is tile body when it and the pixels 8 px left, right, above and below are fully opaque (this
drops the rounded edge, its rim strokes and the drop shadow); no channel may change by more than 3 levels between adjacent body pixels.
The macOS tile's drop-shadow filter must interpolate in sRGB (`color-interpolation-filters="sRGB"`): in the default linearRGB the dark
ramps are quantised into 6 to 13 level bands. Results with that attribute: all ten icons, both masters, 0 steps.

## Build

```sh
npm ci                       # at the repo root: one npm workspace (ADR 0014 §1)
cd icons
npm run all                  # everything: SVG, PNG, tray, Tauri sets, .ico, preview, screenshots
ONLY=pacenotes node tools/build.mjs   # one app (or ONLY=paddock for the umbrella mark)
node tools/install.mjs                # copy the built sets into the apps and the docs (--check only compares)
npm test                              # seam scan, coverage at 16 and in the tray, tray alpha, docs copies
node tools/umbrella-sheet.mjs <dir>   # review renders: the umbrella beside the apps, small frames, tray
```

Edit the art modules above (the family) or `tools/design.mjs` (tiles, the demo apps' flat glyphs); every SVG is generated. Output is
deterministic except `tauri/icon.icns`, which embeds build-specific bytes; every SVG carries explicit id prefixes, so a file does not
depend on what was built before it.

## How apps consume the icons

Each `<app>/tauri/` folder is a drop-in replacement for the app's `src-tauri/icons`. Tray icons are in `<app>/tray/`; the apps
embed the 18 pt @2x template and the 32 px light and dark PNGs. `node tools/install.mjs` copies both into each app in its own layout
(Pitwall maps its tray states Idle, Busy, Paused, Unavailable to serving, busy, idle, error) and the in-app copies (favicons, brand
logos). The ecosystem image and the app posters inline the 60 frame (`<app>/<app>-60.svg`, copied to `docs/images/src/icons/`;
`npm test` fails when a copy is stale); then run `node docs/images/src/ecosystem.mjs --png` and
`PADDOCK_APPS=$PWD/../apps node docs/images/src/app-poster.mjs all --png`.

## Chosen direction: Enamel

Each app has a tile in its own saturated hue, one bold glyph, a faint diagonal "livery" sweep and light from above. It reads as premium software first; the motorsport theme only shows in the livery and the metaphors. The alternative direction, **Carbon** (a graphite tile with the glyph lit in the app's hue), is in `alternative/` and at the bottom of the preview. It was not chosen because the five dark tiles look alike at 16 to 32 px and sink into dark docks and taskbars.

## Grid and shape language

- **macOS master (1024):** an 824 px body at (100,100) with continuous-curvature corners (r 185.4, Big Sur grid). Shadow: y 10, blur 12, black at 28%.
- **Windows master (1024):** a 944 px rounded square at (40,40) with circular corners (r 200) and no outer shadow. Windows 11 guidance favours glyphs without a backplate, but the tile is kept for three reasons: the hue is the strongest cue at 16 to 24 px, it guarantees contrast on both light and dark taskbars, and it keeps the family consistent across platforms. The margin is cut because Windows does not apply the macOS safe area, so the icon would otherwise look small in the taskbar.
- **Glyph area:** about 270 to 754 on the 1024 grid, centred optically.
- **Strokes:** about 60 to 68 on the 1024 grid, with round caps and joins.
- **Two tones per glyph:** primary ink at 100% and secondary ink at 60% (55% on Spotter).
- **Material:** a vertical gradient (top, base, bottom), a top-left radial sheen, an inner edge (light at the top, dark at the bottom), the livery sweep (two diagonal bands of white at 8%; dropped from the family's isometric masters on 2026-11-03, kept on the demos), and a glyph shadow in the app's deep tone.
- **Small frames (`<app>/small/`):** 16 is drawn on its own 16 px grid. 24, 32 and 48 come from one 32 px grid. They are snapped to the pixel grid, drop detail (no eyepieces on Spotter, no bottom line on Debrief at 16 px), and add a 0.5 px edge so the tile reads on light taskbars. These frames go into `icon.ico`.

## Palette

| App | Base | Top → Bottom | Deep | Ink | Ink contrast on base |
|---|---|---|---|---|---|
| Debrief | `#2E6BF0` | `#5590FF` → `#1D4FD3` | `#10308A` | white | 4.7:1 |
| Pitcrew | `#EF5F1C` | `#FF8A4C` → `#D4440D` | `#8A2A04` | white | 3.3:1 |
| Spotter | `#F4B315` | `#FFD257` → `#E59600` | `#7A4E00` | `#3A2600` | 7.8:1 |
| Pitwall | `#5A24B0` | `#7B46DB` → `#43198F` | `#24094F` | white | 9.1:1 |
| Dyno | `#BE3584` | `#DE559F` → `#A0226A` | `#5E0E3C` | white | 5.2:1 |
| Pacenotes | `#6F5900` | `#9C8112` → `#6C5400` | `#312500` | white | 6.8:1 |
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
- **Pacenotes:** a portrait spiral notebook with a second page peeking out behind it, rings through punched holes on the left edge, ruled lines, a small route curve drawn like a pacenote and a compact `</>` mark: notes about the road ahead, here the shape of a codebase. The first version (a squarish card with flat tabs) was rejected as not reading as a notebook. At 32 px the code mark drops out, at 16 px only rings, page and one curve remain. A variant with the spiral along the top is `alternative/pacenotes-top-spiral` (the left spiral was clearer at 16 px); the first-round concept `alternative/pacenotes-brackets-road` is kept too. Bronze-olive tile `#6D590F` (2026-10-24, with the olive UI accent; it replaced the first crimson tile): the closest tile to the olive accent that keeps all four CVD minimums (an olive tile `#657C1F` dropped protanopia to 1.6 against Pitcrew). White ink gives 6.8:1. On 2026-11-03 its chroma went up 10 % at the same lightness (`#6F5900`, less khaki); a lighter tile drops protanopia against Pitcrew below the family minimum.
- **Paddock (umbrella mark, not an app):** pit garages under one gable roof, on a shared foundation slab. The roof is the family
  stripe: the seven app hues side by side, in hue order (blue Debrief, violet Pitwall, lavender Sighter, magenta Dyno, orange
  Pitcrew, amber Spotter, olive Pacenotes), so the family literally covers the teams' bays. Asphalt-graphite tile `#2B3038` with
  white ink (13.3:1): it has no hue of its own, so it reads as the place rather than one more app, and it is clearly apart from the
  light silver Shell Demo tile (ΔE 57). A near-black tile keeps every family CVD minimum unchanged (17.7, 11.6, 13.6, 10.7); its own
  nearest neighbours are Pacenotes (19.5 normal, 20.1 deutan), Dyno (17.9 protan) and Pitwall (12.2 tritan). A lighter slate
  (`#333A44` and up) dropped tritanopia against Pitwall to 8.8. Pitwall's and Pacenotes' roof segments are lifted (`#8A5AE6`,
  `#B49A3C`) so they do not sink into the tile; the tile has a faint light rim for dark docks and taskbars. At 16 px and in the tray
  the row drops to two bays on whole pixels; monochrome renders draw the roof in plain ink. Extra web set in `paddock/web/`:
  `favicon.svg`, `favicon.ico` (16/32/48), `apple-touch-icon.png`, `paddock-512.png` and `paddock-avatar-512.png` (the Windows
  master, which fills a square avatar crop).
- **Dyno:** a dynamometer gauge with a load arc and the needle near the redline.

## Layout

```
<app>/<app>-macos.svg, <app>-windows.svg       1024 masters (pure vector)
<app>/<app>-macos-1024.png, -windows-1024.png  rendered masters
<app>/<app>-128.svg|png, <app>-60.svg|png       the simple frame (128 and 112) and the 60 frame (diagrams)
<app>/small/<app>-{16,20,24,32,40}.svg|png      flat frames (the 16 and 24 pt slots)
<app>/small/<app>-{32-iso,48,64}.svg|png       isometric frames (the 32 pt slot and up)
<app>/tray/svg/                                 tray sources (template, template-18pt, light, dark)
<app>/tray/macos/<name>-18ptTemplate@2x.png     the menu-bar template the app loads (36 px; the family ships no 1x tray PNGs)
<app>/tray/windows/<name>-{light,dark}-32.png   the notification-area images the app loads
<app>/tauri/                                    drop-in replacement for src-tauri/icons
paddock/web/                                    favicon (svg, ico), apple-touch-icon, 512 and avatar PNGs (umbrella only)
archive/2026-10-flat/                           the flat set and its Carbon concept, for reference
alternative/                                    design studies (the Paddock candidates)
preview.html, screenshots/                      review sheet and renders
tools/                                          generators (design.mjs is the source of truth)
```

- **Tray names:** `<app>-tray` when the app has no states (Debrief), otherwise `<app>-tray-<status>`. The statuses are Pitcrew ok/busy/warning/error, Spotter normal/new, Pitwall idle/serving/busy/warning/error and Dyno normal/high.
- **Light and dark tray icons:** "light" means a light taskbar (dark glyph) and "dark" means a dark taskbar (white glyph).
- **The `tauri/` set:**
  - `icon.icns` and the PNGs come from the macOS master.
  - `Square*Logo.png` and `StoreLogo.png` come from the Windows master.
  - `icon.ico` is rebuilt with the hand-tuned frames: 16, 20, 24, 32, 40 and 48 hand-tuned, 64 and 256 from the Windows master.
  - The `android/` and `ios/` outputs are removed.

