# Family icon style guide (3D, Big Sur style)

How every Paddock family icon is drawn: one rendered object with real light and material on the family tile. The Paddock pit
building (`tools/paddock-iso.mjs`, `pitBuilding()`; sheet `alternative/paddock-pit-sheet.png`) is the worked example; the numbers
below are its numbers unless a rule says otherwise. Every app keeps its own tile colour (`PALETTE` in `tools/design.mjs`) and its
own symbol; only the rendering changes.

## 1. Projection and light

- **Masters (1024 down to 64):** true isometric, 30 degrees: a point (s along the long axis, d into depth, t up) lands at
  `x = (s + d) cos 30, y = (s - d) / 2 - t`. One projection per icon; no mixed angles, no perspective.
- **Frames at 32 and 24:** 2:1 pixel isometric (one pixel up or down per two across), `x = s + d, y = (s - d) / 2 - t`, with
  `shape-rendering="crispEdges"`, so the diagonals are clean pixel stairs.
- **One key light from the top left** (azimuth about 315 degrees, elevation about 45 degrees). There is no second light and no
  rim light on the object. The consequence for the three visible face orientations:
  - **tops** (facing up): lightest;
  - **lit faces** (facing the lower left, towards the light): mid;
  - **shade faces** (facing the lower right): darkest of the solid faces.

## 2. Value steps

Values are OKLab lightness x 100 (`L`). Adjacent faces of one solid differ by at least 8 L, so the edges read without outlines.

| Role | Paddock colours (gradient start to end) | L |
|---|---|---|
| Top (the topmost slab only) | `#FFFFFF` to `#E9EDF2` | 100 to 94 |
| Lower tops (canopy; the pit-wall top `#DCE1E7`) | `#D5DBE2` to `#C3CAD3` | 88 to 82 |
| Lit face | `#C9D0D8` to `#A9B2BD` | 86 to 76 |
| Slab edge (the thin front of a roof or canopy) | `#AEB7C2` to `#959FAB` | 78 to 70 |
| Shade face | `#7E8895` to `#626C78` | 62 to 53 |
| Recess, glass, openings | `#3A4656`, `#1C2531`, `#10151B` | 39, 26, 19 |
| Tile base (graphite) | `#2B3038` | 31 |

- Every face has a gentle gradient (6 to 10 L) along its long side, lighter towards the light.
- The darks (openings, glass) sit below the tile's L, so they read as holes in the object, not as the tile.
- Only the topmost surface is pure white. Lower tops step down a value, or stacked layers read as the treads of a grandstand.
- Interiors may glow, but stay below the lit face: `#0E1318`, `#2E3A48` at 55 %, `#8FA3B8` at the floor (one gradient per
  opening, since openings step down the screen). The glow gives the object life; it is neutral, not an accent.

## 3. Material

- **The object is white or near-white with greys**, so it stays the family's light glyph on any tile.
- **On a coloured tile, tint the greys towards the tile:** mix the lit and shade faces 10 to 15 % towards the tile's `deep` colour,
  and use `deep` (not black) for openings and glass. Paddock's graphite tile is neutral, so its greys are cool neutral.
- Glass: a dark vertical gradient with one diagonal sheen (white, 28 % falling to 0 over the first half) and thin mullions.
- No textures, no noise, no bevels, no outlines. Edges are defined by value steps alone, plus one 2.5 px white highlight along the
  topmost front edge.

## 4. Shadows and ambient occlusion (1024 grid)

All shadows are black, blurred in sRGB (`color-interpolation-filters="sRGB"`), and drawn under the object.

| Layer | Shape | Blur sigma | Opacity |
|---|---|---|---|
| Long shadow | the footprint swept away from the light by (+110, +66) | 18 | about 0.6 effective (0.8 x 0.75) |
| Contact occlusion | the footprint, 3 to 4 px wider on the shade side | 2.5 | 0.8 |
| Overhang occlusion | a band 16 units deep under every roof, canopy or overhang, clipped to the faces below it | 6 | 0.6 |
| Cast shadow of an overhang | a band about 30 units down the face below it; the ground strip under it, flat | 6; none | 0.35; 0.3 |
| Recess occlusion | the head and the inner jamb of every opening as flat darker faces (`#2B323B`, `#56606C`) | none | 1 |

The family glyph shadow (`feDropShadow` in the tile's `deep` colour) is replaced by these layers for rendered objects.

## 5. Size and placement

- **Visible mass:** 70 to 75 % of the tile width at 1024 (Paddock: about 72 %, x 215 to 808). Fit the object's outline (including
  overhangs and the ground details) into a 690 x 560 box (the `box` parameter); the shadows may extend past it.
- **Optical centring:** isometric masses are heavy at the upper right. Shift the art about 4 left and 10 down from the box centre
  (`dx`, `dy`) and check it against Pitwall and Dyno at 128.
- **Flat symbols** (16 px and the tray) keep the old keyline: glyph box 270 to 754 at 1024, x 3 to 12 at 16.

## 6. Detail budget per size

| Size | Content |
|---|---|
| 1024, 512, 256 | everything: fine repeats (door slats, mullions, lane line), the fine chequer (step 14, two rows), the accent at 8 units |
| 128 and below to 60 (`simple: true`) | no fine repeats (no slats, mullions or lane lines); fewer, wider repeated parts (three bays, pillars 22); the cue that carries the meaning stays, coarser (chequer step 34, one row); the accent 10; door rails 8. README size 112 and diagram size 60 are rendered from this frame |
| 32, 24 | hand-drawn pixel isometric (`PIT_SMALL`): one pixel per part, whole-pixel widths for repeated parts (three bays of whole pixels), flat tones (no gradients), the same parts as the master in order: ground detail, main face with openings, upper band, roof. At 24 drop any band that would make 1 px stripes (Paddock drops the glass) and keep end faces at least 3 px |
| 16 | the flat symbol (Paddock: the slatted door), one tone, whole pixels, coverage 18 to 26 % |
| Tray | the flat symbol as a template (alpha only, one tone), x 2 to 14; light and dark variants from it |

Rule: if a part cannot be drawn on whole pixels at a size, it is dropped at that size, never blurred.

## 7. Accents

- **At most one accent per icon**, small and saturated, on the object (never on the tile).
- An app's accent comes from its own hue (`PALETTE[app].light` or `.top`), for example a status light or a lit edge.
- Paddock's accent is the family pinstripe: a thin band of the seven app hues with hard stops (`STRIPE` order) along the canopy's
  front edge. It is the only multicolour element in the family and belongs to the umbrella mark alone.
- An accent must still be visible at 112 without becoming the loudest part at 60 (Paddock: 8 units on the master, 10 in the
  128 frame). If it cannot, drop it at that size.

## 8. Tile and gloss (unchanged)

- Tile gradient top, base and bottom from `PALETTE`; the radial sheen from the top left; the inner edge (light at the top, dark at
  the bottom); the livery sweep (two diagonal white bands at 8 %, 16 % on light tiles).
- macOS: an 824 squircle at (100, 100) with the drop shadow (y 10, blur 12, black 28 %, sRGB filter). Windows: a 944 rounded square
  at (40, 40), radius 200, no outer shadow.
- Dark tiles get a light rim (Paddock: 20 % on the master, a full-pixel 40 % rim at 16, 45 % at 24 to 48) so they hold on dark docks.

## 9. Checks (all must pass)

| Check | Rule | Tool |
|---|---|---|
| Tile CVD distance | every tile at least 17.7 normal, 11.6 protan, 13.6 deutan, 10.7 tritan (OKLab dE x 100) from every other tile | `node tools/cvd.mjs` |
| Ink contrast | the object's lit face against the tile base at least 3:1 (Paddock `#C9D0D8` on `#2B3038`: 9.6:1) | `contrast()` in `tools/cvd.mjs` |
| Face steps | adjacent faces at least 8 L apart (section 2) | OKLab |
| Size | visible mass 70 to 75 % of the tile width at 1024 | measure the render |
| Coverage at 16 | 18 to 26 % ink | `coverage()` in `tools/paddock-symbols.mjs` |
| Seams | the tile background has no step over 3 levels | `npm test`, `tools/scan.mjs` |
| Whole pixels | 32, 24 and 16 frames have no half-tone edges on repeated parts | magnify the frame (sheet close-ups) |
| Review | every new or changed icon gets an independent review: professional vs amateur at 512, 112 and 60, and meaning | rule 22 |

## 10. Worked example: Paddock

`pitBuilding()` parameters (iso units): long side L 380, depth D 90; ground floor H1 120 with four bays (margin 16, pillar 14,
doors 62 % down, set 14 deep, lit inside; three bays with pillars 22 in the 128 frame); a canopy 16 thick cantilevered 46 over the
pit lane, carrying the pinstripe, with its cast shadow; an upper hospitality floor H2 64 with a glass band; a roof slab 16 thick
with a 6 overhang; in front, the apron (80 deep, `#6A7380` to `#545D69`) with a lane line and the pit wall (12 deep, 30 high,
chequered top). Fit box 690 x 560, shift (-4, +10).
`node tools/paddock-pit-sheet.mjs` renders the set and the sheet.
