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
- **Frames at 32 and 24:** 2:1 pixel isometric, `x = s + d, y = (s - d) / 2 - t` (`pixelIso`), crisp edges, so the diagonals are
  clean pixel stairs.
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
- **Glass and screens:** a dark vertical gradient (`VALUES.glass`), one diagonal sheen (white 28 % falling to 0 over the first
  half), thin mullions or bezels.
- **Metal:** a narrow bright specular band along the lit edge. **Rubber:** matte, darkest values, no specular.
- No textures, no noise, no outlines. One 2.5 px white highlight along the topmost front edge.

## 4. Shadows and occlusion (1024 grid, `SHADOW` in `tools/iso.mjs`)

All black, blurred in sRGB, under the object. Use the helpers with a `defsKit(uid + letter)`, and add `shadowFilters(kit)` to the
defs.

| Layer | Helper | Shape | Blur | Opacity |
|---|---|---|---|---|
| Long shadow | `longShadow(kit, foot)` | the footprint swept (+110, +66) away from the light | 18 | 0.75 x 0.8 |
| Contact | `contactShadow(kit, d)` | the footprint, 3 to 4 units wider on the shade side | 2.5 | 0.8 |
| Occlusion | `occlusion(kit, clip, d)` | a band about 16 deep under every overhang or where parts meet, clipped to the faces below | 6 | 0.6 |
| Cast shadow | `occlusion(kit, clip, d, SHADOW.castOpacity)` | the shadow of a large overhang on the face below, about 30 deep | 6 | 0.35 |
| Recesses | flat faces | the head and the inner jamb of every opening in `recessHead` and `recessJamb` | none | 1 |

## 5. Size and placement

- **Fit by mass:** `fitBox(outlinePoints, { mass: 0.72, dx, dy })` scales the object so its outline (all outer corners, including
  ground details) is 72 % of the tile width (824 on the 1024 grid); the visible mass rule is 70 to 75 %. `maxHeight` (default 560)
  caps tall objects, which then come out narrower; that is fine as long as the measured mass stays at 70 % or more. Measure the
  render with `visibleMass(pixels, barePixels)` (`tools/iso.mjs`) and adjust `mass` until it lands in 70 to 75 %. The fixed
  `box: [690, 560]` mode is kept for the Paddock pit building only: wide, low objects (binoculars, tablet) reach 78 to 80 % in it.
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
| 48 | the simple frame scaled onto the small tile |
| 32, 24 | 2:1 pixel isometric with `pixelIso` and `pixelLayer` (flat colours, crisp, one pixel per part, whole-pixel widths for repeats), or a simplified object if the isometric one cannot be read; keep the defining parts even at 24, and end faces at least 3 px |
| 16 | the flat symbol: one tone, whole pixels; coverage 18 to 26 % for line glyphs, up to 30 % for filled silhouettes. Paddock: the slatted door with a 1 px band of the seven hues above it |
| Tray | the flat symbol as a template, x 2 to 14, with light and dark variants made from it. Whole pixels only: no partial alpha anywhere, except a deliberate dim of the whole glyph for a state |

Rule: a part that cannot be drawn on whole pixels at a size is dropped at that size, never blurred.

## 7. Accents

- **One accent per app**, small and saturated, on the object (never on the tile), from the app's own hue (`PALETTE[app].light` or
  `.top`): a status LED on the radio, the trace on the laptop screen, the redline on the tacho, the route on the tablet, a lens
  glint, and so on.
- **Paddock's seven-hue stripe is on Paddock only** (the canopy-edge pinstripe and the 16 px band). It is the umbrella mark's link to
  the family and the only multicolour element. No app uses more than its own hue.
- An accent must be visible at 112 without becoming the loudest part at 60. Thicken it in the simple frame or drop it there.

## 8. Tile and gloss (unchanged)

- Tile gradient top, base and bottom from `PALETTE`; the radial sheen from the top left; the inner edge; the livery sweep (two
  diagonal white bands at 8 %, 16 % on light tiles).
- macOS: an 824 squircle at (100, 100) with the drop shadow (y 10, blur 12, black 28 %, sRGB filter). Windows: a 944 rounded square at
  (40, 40), radius 200, no outer shadow.
- Dark tiles get a light rim (Paddock: 20 % on the master, a full-pixel 40 % rim at 16, 45 % at 24 to 48).

## 9. Checks (all must pass)

| Check | Rule | Tool |
|---|---|---|
| Acceptance | independent review: professional at 512, 112 and 60, reads as its object, no P1 left | rule 22 |
| Tile CVD distance | every tile at least 17.7 normal, 11.6 protan, 13.6 deutan, 10.7 tritan from every other | `node tools/cvd.mjs` |
| Contrast | the object's front against the tile base at least 3:1 (Paddock `#C9D0D8` on `#2B3038`: 9.6:1) | `contrast()` in `tools/cvd.mjs` |
| Face steps | adjacent faces at least 8 L apart | OKLab |
| Size | visible mass 70 to 75 % of the tile width at 1024 | `visibleMass()` in `tools/iso.mjs` |
| Coverage at 16 | 18 to 26 % (line glyphs), up to 30 % (filled silhouettes) | `coverage()` in `tools/paddock-symbols.mjs` |
| Tray alpha | every pixel fully opaque or fully transparent (except a whole-glyph state dim) | magnify the template |
| Whole pixels | 32, 24 and 16 frames have no half-tone edges on repeated parts | sheet close-ups |
| Seams | the tile background has no step over 3 levels | `npm test` |

## 10. Worked example: Paddock

`pitBuilding()` (iso units): long side L 380, depth D 90; ground floor H1 120 with four bays (margin 16, pillar 14, doors 62 % down,
set 14 deep, lit inside); a canopy 16 thick cantilevered 46 over the pit lane, carrying the pinstripe, with its cast shadow; an upper
hospitality floor H2 64 with a glass band; a roof slab 16 thick with a 6 overhang; in front the apron (80 deep) with a lane line and
the pit wall (12 deep, 30 high, chequered top). Fit box 690 x 560, shift (-4, +10).

- The simple frame: three bays with pillars 22, dark openings, doors 40 % down, a coarser chequer on a 40-high wall, stripe 10.
- The 60 frame: the simple frame without the chequer, 8 % larger.
- The 32 and 24 frames: `PIT_SMALL`.
- 16 and the tray: the flat slatted door.

`node tools/paddock-pit-sheet.mjs` renders the set and the sheet.
