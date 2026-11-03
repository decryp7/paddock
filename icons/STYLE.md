# Family icon style guide (isometric motorsport objects)

Every family icon is one isometric motorsport object, rendered with real light and material on the family tile. Each app keeps its
own tile colour (`PALETTE` in `tools/design.mjs`); the object is its symbol:

| App | Object | Tile |
|---|---|---|
| Paddock (the umbrella mark) | the pit building | graphite `#2B3038` |
| Pitwall | team radio (walkie-talkie) | violet |
| Debrief | engineer's laptop with a telemetry trace | blue |
| Pitcrew | pneumatic wheel gun | orange |
| Dyno | tachometer pod | magenta |
| Spotter | binoculars | amber (light: dark ink) |
| Pacenotes | tablet with a route line | olive |
| Sighter | camera on a small tripod | lavender (light: dark ink) |

The Paddock pit building is the worked example (`pitBuilding()` in `tools/paddock-iso.mjs`, sheet `alternative/paddock-pit-sheet.png`).
Draw every object with the shared helpers in `tools/iso.mjs`, so projection, light, values, shadows, placement and the small-frame
grid are identical across the family.

## 0. Acceptance criterion

The user's bar, verbatim: **"make them really professional and really beautiful".** In practice: top-tier macOS app icon quality.

- **Strong silhouette first:** the object is recognisable as a black shape at 60 px before any shading.
- **Precise geometry:** one projection, parallel edges stay parallel, parts meet exactly, no stray pixels or hairline seams.
- **Believable materials:** gradients on every face, speculars where the light catches (a thin highlight on lit edges, one sheen
  on glass or gloss); metal, plastic, rubber and glass read as such.
- **Clean bevels:** slab edges and rounded corners drawn as their own faces (section 2), not outlines.
- **Soft occlusion:** where parts meet or overhang, a soft dark band (section 4); grounded by a contact shadow and a long shadow.
- An independent review judges every icon "professional vs amateur" at 512, 112 and 60 and names what looks amateur; an icon is
  done when that review finds no P1.

## 1. Projection and light

- **Masters (1024 down to 64):** true isometric, 30 degrees. In iso units, s runs along the object's long axis (to the lower right),
  d into depth (to the upper right), t up: `P(s, d, t) = [(s + d) cos 30, (s - d) / 2 - t]` (`P` in `tools/iso.mjs`). One
  projection per icon; no perspective, no mixed angles. Orient each object so its most telling face is the `front` (d = const)
  face, toward the lower left.
- **Below 32 pt:** no projection: the object seen from the front (Microsoft's guidance, section 6b: "drawn with a straight-on
  perspective"). From 32 pt up, the isometric simple frame (section 6a).
- **One key light from the top left** (azimuth about 315 degrees, elevation about 45). No second light, no rim light. Faces:
  **tops** lightest, **fronts** (toward the lower left) mid, **ends** (toward the lower right) darkest.

## 2. Values

OKLab lightness x 100 (L). Adjacent faces of one solid differ by at least 8 L, so edges read without outlines. `VALUES` in
`tools/iso.mjs`:

| Role | Colours (gradient start to end) | L |
|---|---|---|
| `top`: the topmost surface only | `#FFFFFF` to `#E9EDF2` | 100 to 94 |
| `lower`: lower tops (ledges, steps) | `#D5DBE2` to `#C3CAD3` | 88 to 82 |
| `litLight`: light fronts of thin parts in front | `#E4E8ED` to `#C8CFD8` | 92 to 84 |
| `lit`: fronts | `#C9D0D8` to `#A9B2BD` | 86 to 76 |
| `edge`: slab edges and bevels | `#AEB7C2` to `#959FAB` | 78 to 70 |
| `shade`: ends | `#7E8895` to `#626C78` | 62 to 53 |
| recesses, glass, openings | `#56606C`, `#2B323B`, `#1C2531`, `#10151B` | 45 to 19 |

- Every face has a gradient (6 to 10 L) along its long side, lighter toward the light.
- **Only the topmost surface is pure white**; lower tops step down, or stacked layers read as stairs.
- Darks (openings, screens, glass) sit below the tile's L, so they read as holes or glass, not as the tile.
- Interiors and screens may glow, but stay below the lit face.

## 3. Material per tile type

- **Dark and mid tiles (graphite, violet, blue, olive):** the object is white or near-white with greys (the ladder above).
- **Coloured tiles:** tint the greys toward the tile with `tintedValues(PALETTE[app].deep, 0.12)` (10 to 15 %), and use `deep`, not
  black, in openings and glass. The `top` stays white.
- **Mid-light tiles (Pitcrew orange, Dyno magenta):** the fronts must be near-white (L 90 or more, use `litLight`), because a grey
  front cannot reach 3:1 on orange. Only ends and recesses go darker.
- **Light tiles (Spotter amber, Sighter lavender): dark ink.** The object is the app's ink colour (`INK[app]`) with the same ladder
  inverted in range: tops L 44 to 39, fronts L 36 to 30, ends L 21 to 18, recesses down to L 12; highlights in a lighter ink (L 56).
  `darkInk(INK[app])` gives it: `.values` (flat faces, same keys as `VALUES`), `.body` (the ramp for curved surfaces) and `.metal`
  (satin fittings, L 40 to 72, with a white specular).
- **Glass and screens:** a dark vertical gradient (`VALUES.glass`), one diagonal sheen (white 28 % falling to 0 over the first
  half), thin mullions or bezels.
- **Metal:** a narrow bright specular band along the lit edge. **Rubber:** matte, darkest values, no specular.
- No textures, no noise, no outlines. One 2.5 px white highlight along the topmost front edge.

### Round parts and colour helpers (`tools/iso.mjs`)

- **Cylinders** (barrels, wheels, dials, knobs): `cylinder(kit, name, { axis: 'd' | 's', c, len, r, tone })` draws the side as the
  hull of both caps with a gradient sampled from the visible normals under the key light, and returns the visible end cap. `tone`
  maps light intensity to colour: `x => ramp(WHITE_BODY, x)` for white objects, `x => ramp(darkInk(ink).body, x)` on light tiles.
  `ring`, `disc` and `arcPts` give circles, caps and arcs on any axis frame (`AXES`); `hull` the convex hull.
- **Light:** `LIGHT` is the key light as a vector, `lit(n)` the intensity on a face with normal `n` (tops 0.78, fronts 0.55, ends 0),
  `VIEW` the view direction (a face is visible when `dot(n, VIEW) > 0`).
- **Specular:** `fadeStroke(kit, name, pts, w)` is a highlight that fades in and out along a rim or a lit metal edge.
- **OKLab:** `Lof(colour)` is the L of section 2; `atL(ref, L, c)` is a colour of `ref`'s hue at lightness L (chroma scaled by c);
  `oklab` and `fromOklab` convert; `ramp(stops, x)` samples a colour ramp.

## 4. Shadows and occlusion (1024 grid, `SHADOW` in `tools/iso.mjs`)

One family shadow for every icon: colour cool near-black `#0E1016` (`SHADOW.colour`), blurred in sRGB, under the object. Use the
helpers with a `defsKit(uid + letter)`, and add `shadowFilters(kit)` to the defs.

| Layer | Helper | Shape | Blur | Opacity |
|---|---|---|---|---|
| Long shadow | `longShadow(kit, foot, SHADOW.longSweep, longOpacity(PALETTE[app].base))` | the footprint swept (+110, +66) away from the light | 18 | 0.7 on dark tiles (base L below 50: Paddock, Pitwall, Pacenotes), 0.55 on mid and light tiles; `longOpacity(base)` picks it |
| Contact | `contactShadow(kit, d)` | the footprint, 3 to 4 units wider on the shade side | 2.5 | 0.8 |
| Occlusion | `occlusion(kit, clip, d)` | a band about 16 deep under every overhang or where parts meet, clipped to the faces below | 6 | 0.6 |
| Cast shadow | `occlusion(kit, clip, d, SHADOW.castOpacity)` | the shadow of a large overhang on the face below, about 30 deep | 6 | 0.35 |
| Recesses | flat faces | the head and the inner jamb of every opening in `recessHead` and `recessJamb` | none | 1 |

No icon uses another shadow colour, blur or opacity.

## 5. Size and placement

- **Matched visual weight:** every icon carries the same weight in the row, whatever its proportions. Fit with
  `fitBox(outlinePoints, { weight: 0.68, dx, dy })`: the outline (all outer corners, including ground details) gets a geometric-mean
  size sqrt(width x height) of 0.68 of the tile width (824 on the 1024 grid), the Paddock pit building's value. A wide object
  (binoculars, tablet) comes out wider and lower, a tall one (radio, camera on its tripod) narrower and taller. Caps: at most 80 % of
  the tile wide and 78 % high; an object that hits a cap comes out slightly lighter.
- **Measure it:** `visibleBox(pixels, basePixels)` (`tools/iso.mjs`) on the 1024 render against the empty tile
  (`masterSvg(app, { glyph: [], palette })`) gives width, height, weight and ink area. Target weight 0.68 to 0.78 measured (the
  measurement includes the contact and long shadows: the pit building measures 0.77), and ink area within about 0.08 of the
  pit building's 0.31, so a light open object (a tripod) is not too thin beside a solid one. Adjust `weight` until both land.
- The fixed `box: [690, 560]` mode is kept for the Paddock pit building (it lands on weight 0.68 that way); `mass` is kept for
  compatibility. Do not use either for new icons.
- **Clipping:** `masterSvg` clips the art to the tile, so long shadows never spill past the squircle or the Windows square.
- **Optical centring:** isometric masses lean to the upper right. Shift with `dx` and `dy` (Paddock: -4, +10) and check the icon
  beside the others at 128.
- `masterSvg(app, { art })` takes the art as `uid => ({ defs, body })`, with the body wrapped in `fit.transform`.

## 6. Detail budget per size

| Size | Content |
|---|---|
| 1024, 512, 256 | everything: fine repeats (slats, mullions, grille lines, scale ticks), and the accent at its master size |
| 128 down to 112 (the simple frame, an option of the art function) | no fine repeats; fewer, wider repeated parts; openings fully dark; the cue that carries the meaning stays, coarser; the accent a little thicker |
| 60 | the simple frame without any texture that turns grey (Paddock drops the chequer), fitted about 8 % larger so the mass matches the others |
| 64, 48, 32 (the 32 pt slot and up) | the simple frame on the small tile as vector at its size (`iso()` in `tools/iso-family.mjs`), clipped to the tile so its long shadow ends at the tile's edge |
| 40, 32, 24, 20, 16 (the 16 and 24 pt slots) | the flat front-view glyph, drawn for each size (`FLAT` in `tools/small-flat.mjs`, section 6a); coverage at 16 of 18 to 26 % for line glyphs, up to 30 % for filled silhouettes (section 9). Paddock: the slatted door under the band of the seven hues |
| Tray | the flat glyph in one tone with vector badges, at the one size each app loads: the 36 px template for 18 pt and the 32 px light and dark PNGs (section 6a) |

Rule: a straight edge sits on a whole pixel at every size; a part that cannot is redrawn or dropped at that size, never blurred. Slanted
edges and curves are anti-aliased.

### 6a. Small sizes and trays (approved by the user 2026-11-03)

The user found the small icons "pixelated" in every app (2026-11-03). Where that came from, in the set before this one (sheets in
`docs/design/icons-small/`):

- **macOS on a Retina screen** shows the 32 px pixel-art frame at 16 pt (Finder lists and sidebar: `.icns` `ic11`) and the same frame
  doubled at 32 pt (Finder icon view, Spotlight, a small Dock: `ic12`, 76 to 80 % of its pixels an exact 2x of the 32 frame).
- **Menu bar:** the template is the 16 grid at 2x: every pixel a 2 x 2 block, so arcs and diagonals step.
- **Windows:** `icon.ico` has 16, 24, 32, 48 and 256; at 125 % (20, 30 px) Windows resamples the 24 or 32 pixel-art frame.
- **Linux:** `32x32.png` is the pixel-art frame; 16 to 24 px launchers resample it.
- **In-app:** `favicon.svg` of Pitwall and Spotter is the crisp 32 frame; Dyno's and Spotter's tray pages show the 16 frame at 16
  CSS px; Paddock's 48 is its 32 pixel frame scaled.

**The frames, by point size (not by physical pixels):** the 16 and 24 pt slots take the **flat front-view glyph** at every scale
(`FLAT`, `smallFlat` in `tools/small-flat.mjs`): one glyph in the tile's ink with tile-coloured cut-outs and a soft glyph shadow in
the tile's deep colour; Paddock keeps its band of hues. From 32 pt up the icon is the **isometric object**: the simple frame on the
small tile as vector at its size (`iso()`), never a doubled pixel frame. A smooth isometric drawing at 16 to 32 px turned the objects
into blobs at 16 and 24 and was dropped.

**Drawing the flat glyph, per size** (16, 20, 24, 32, 40 px; each is drawn on its own pixels, not scaled):

- **Weights:** a frame weight `STROKE` (16: 1, 20: 1, 24: 2, 32: 2, 40: 3 px) for outlines, posts and rings, and a detail weight
  `DETAIL` (1, 1, 1, 2, 2 px) for what sits inside a frame (the trace, signal bars, grille slots, the route). Whole pixels only;
  nothing is thinner than one device pixel. Curved strokes (Dyno's bezel) follow the size more closely (1.5 to 3.25 px).
- **Snapping:** rect corners and polygon vertices on whole pixels; a line's straight runs centred for its width (half pixels for an
  odd width), so their edges are crisp; circles centred on a half pixel; curves and slants anti-aliased.
- **Insets:** a cut-out inside a frame is the frame shrunk by one frame weight (a chin by two), so the frame reads as one stroke.
- **Repeats:** slats and grille slots are a set count spaced evenly (`bars`), so the gaps are equal at every size.
- **Corner radii** scale with the size (the tile's from the 16, 24 or 32 grid, the glyph's in units of the 16 grid).
- **Optical centring** (`opticalShift`): the glyph's ink centre, averaged with its box centre, moved onto the tile's centre and 2 %
  above it (a centred mass looks low), in whole pixels, at most 1 px (2 px from 32 up).
- **Clarity at 16:** each app reads by silhouette on light and dark backgrounds, wallpapers and translucent bars (`clarity.png`).
  Pacenotes is the tablet with a pacenote's bend arrow on it (a drawn S, a road and a route with a flag read as the letters S, A, 5 and
  J). Pitcrew is the impact wrench's profile (a round housing, a square anvil collar, a socket wider than the anvil, a raked grip,
  from 24 px a trigger joined to the grip); Dyno's redline is the bezel's weight set off by a gap, its needle tapered; Pitwall's tall
  antenna sits at the body's left edge; Paddock's door has two slats at 16 and 20. The line glyphs (Debrief, Paddock) are one pixel
  heavier from 32 px, to match the solid ones.
- **The isometric frame at 32 px** (the 32 pt slot on a 1x screen) gets a light unsharp mask on its rendered pixels, so its edges
  and contrast do not go soft; at 48 and 64 it is drawn as is.

| Container | Entry | Slot | Frame |
|---|---|---|---|
| `.icns` | `icp4` (16 px) | 16 pt @1x | flat 16 |
| `.icns` | `ic11` (32 px) | 16 pt @2x | flat 32 |
| `.icns` | `icp5` (32 px) | 32 pt @1x | isometric at 32 |
| `.icns` | `ic12` (64 px) | 32 pt @2x | isometric at 64 |
| `.icns` | `ic07` and up | 128 pt and up | the master (unchanged) |
| `.ico` | 16, 20, 24, 32, 40 | 16 pt at 100 to 250 %, 24 at 100 to 150 % | flat at each size (20 and 40 are new) |
| `.ico` | 48, 64 | 24 pt at 200 % and Explorer medium; 32 at 200 % | isometric at each size (64 is new) |
| `.ico` | 256 | large icons | the Windows master |
| Linux | 16, 24, 32 (`32x32.png`) | menus, panels | flat |
| Linux | 48, 64 | launchers | isometric |
| Linux | 128 and up | app grids | the master |

The one conflict: the `.ico` has a single 48 frame for 24 pt at 200 % and for Explorer's 48 px view; it is isometric, so a 24 pt taskbar
icon is flat at 100 to 150 % and isometric at 200 %. `MIX` in `tools/small-flat.mjs` holds this table.

**Trays:** the flat glyph in one tone, anti-aliased, with vector badges whose knock-out is a 1 px ring at @2x (`smoothTray`). The apps
load **one** image (verified in each `tray.rs` and tray-icon 0.25): macOS the 36 px template, which tray-icon sets to 18 pt high (a 1x
screen downsamples it; 1x tray PNGs are not shipped), and Windows the 32 px PNG, from which tray-icon makes one HICON
that Windows scales to 16, 20 or 24 px. So the tray image must downsample well, which whole-pixel art does not. Using exact 1x frames
would need a change in the apps: on Windows, load an `.ico` with all sizes through tray-icon's `Icon::from_path` or `from_resource`
(LoadImageW picks the size), which Tauri's `Image` cannot express; on macOS, an NSImage with two representations, which tray-icon
does not build. Tray edges are anti-aliased, as every Retina menu-bar symbol's are (section 9).

**Contrast:** flat ink 3.3 to 13.3:1 on the tile base; isometric fronts 3.1 to 8.5:1, except Pitcrew's 2.7:1, the approved exception
in section 9.

The apps' bundles, trays and in-app copies carry this set since 2026-11-03 (`tools/build.mjs`, `tools/install.mjs`).

### 6b. Benchmarks (read 2026-11-03)

- **Apple HIG, App icons** (change log to 2026-10-28): "avoid extremely thin line weights and sharp corners, because they tend to lose
  detail and crispness in smaller icon sizes"; a minimal number of shapes; clearly defined edges. Taken: one frame weight per size,
  never under one pixel; few parts; crisp edges.
- **Apple HIG, The menu bar** (2025-06-09): menu bar extras use black and clear only, so the system tints them for light and dark bars
  and selection. Taken: templates are alpha only, badges cut out of the glyph.
- **Apple HIG, SF Symbols** (2025-07-28): consistency "in level of detail, optical weight, alignment, position"; optical, not
  geometric, alignment. Taken: the weight tables and optical centring.
- **Microsoft, Design guidelines for Windows app icons** (updated 2026-07-14): "drawn with a straight-on perspective" (perspective
  only where a metaphor needs it); detail only on the most prominent layer; "at least half of your icon passes a 3.0:1 contrast ratio
  on light and dark theme"; exterior corners 2 px and interior 1 px at 48. Taken: the front view below 32 pt; the tile (which passes
  3:1 against light and dark taskbars in most hues) carries the contrast; scaled radii.
- **Microsoft, Construct your Windows app's icon** (2026-07-21): Windows picks an exact size, else the next larger and scales down;
  16, 20, 24, 30, 32, 36, 40, 48, 64 and 256 are the target sizes (title bar and tray 16 to 32, taskbar 24 to 48 at 100 to 200 %).
  Taken: `.ico` frames at 16, 20, 24, 32, 40, 48, 64 and 256.

## 7. Accents

- **One accent per app**, small and saturated, on the object (never on the tile), from the app's own hue (`PALETTE[app].light` or
  `.top`): a status LED on the radio, the trace on the laptop screen, the redline on the tacho, the route on the tablet, a lens
  glint, and so on.
- **Paddock's seven-hue stripe is on Paddock only** (the canopy-edge pinstripe and the 16 px band). It is the umbrella mark's link to
  the family and the only multicolour element. No app uses more than its own hue.
- An accent must be visible at 112 without becoming the loudest part at 60. Thicken it in the simple frame or drop it there.

## 8. Tile and gloss

- Tile gradient top, base and bottom from `PALETTE`; one soft radial sheen from the top left (26 % white at its centre on the family's
  masters); the inner edge. The two diagonal livery bands are dropped from the family (2026-11-03: they read as glare stripes); the
  demo apps keep them.
- macOS: an 824 squircle at (100, 100) with the drop shadow (y 10, blur 12, black 28 %, sRGB filter). Windows: a 944 rounded square at
  (40, 40), radius 200, no outer shadow.
- Dark tiles get a light rim (Paddock: 20 % on the master, a full-pixel 40 % rim at 16, 45 % at 24 to 48).

## 9. Checks (all must pass)

| Check | Rule | Tool |
|---|---|---|
| Acceptance | independent review: professional at 512, 112 and 60, reads as its object, no P1 left | rule 22 |
| Tile CVD distance | every tile at least 17.7 normal, 11.6 protan, 13.6 deutan, 10.7 tritan from every other | `node tools/cvd.mjs` |
| Contrast | the object's front, and the flat glyph's ink, against the tile base at least 3:1 (Paddock `#C9D0D8` on `#2B3038`: 9.6:1). One approved exception (user, 2026-11-03): Pitcrew's isometric front, 2.7:1 on orange, since a front light enough for 3:1 would lose the 8 L step to its white top (section 2; mid-light fronts, section 3); its white top is 3.3:1 | `contrast()` in `tools/cvd.mjs`; `npm test` |
| Face steps | adjacent faces at least 8 L apart | OKLab |
| Visual weight | fitted with `weight: 0.68`; measured weight 0.68 to 0.78 and ink area within 0.08 of the pit building's | `visibleBox()` in `tools/iso.mjs` |
| Coverage at 16 | 18 to 26 % (line glyphs), up to 30 % (filled silhouettes); the class of each glyph is `GLYPH_KIND` in `tools/iso-family.mjs` | `npm test` (`tools/iso-family.test.mjs`) |
| Tray coverage | the same limits, measured on the tray glyph **without its state badge** (`smoothTrayGlyph()`, at 32 px). A badge adds ink by design, so the rule judges the symbol | `npm test` |
| Tray alpha | templates are black with alpha only; edges anti-aliased (the apps ship one image per platform, which the OS scales) | `npm test` |
| Whole pixels | the flat glyph's rects and polygons on whole pixels at 16 to 40, no part under one pixel, weights whole and never lighter at a larger size | `npm test`; `zoom.png` |
| Seams | the tile background has no step over 3 levels | `npm test` |

## 10. Worked example: Paddock

`pitBuilding()` (iso units): long side L 380, depth D 90; ground floor H1 120 with four bays (margin 16, pillar 14, doors 62 % down,
set 14 deep, lit inside); a canopy 16 thick cantilevered 46 over the pit lane, carrying the pinstripe, with its cast shadow; an upper
hospitality floor H2 64 with a glass band; a roof slab 16 thick with a 6 overhang; in front the apron (80 deep) with a lane line and
the pit wall (12 deep, 30 high, chequered top). Fit box 690 x 560, shift (-4, +10).

- The simple frame: three bays with pillars 22, dark openings, doors 40 % down, a coarser chequer on a 40-high wall, stripe 10.
- The 60 frame: the simple frame without the chequer, 8 % larger.
- 16 to 40 px: the flat door glyph under the band of hues (`FLAT.paddock`); 32 pt and up: the simple frame on the small tile.
- The tray: the door glyph in one tone, the band as its lintel.

`node tools/paddock-pit-sheet.mjs` renders the set and the sheet.
