# Independent review: family icons rebuilt with the sRGB tile-shadow filter (rule 22)

Reviewed 2026-10-25 on `origin/shell/integrate` at 416822e (fix commit f61ef4f, compared against `13c6b60^1`). Read-only.

## Method

1. **Source diff.** Every macOS master SVG was normalised (filter attribute removed, generated id prefixes masked) and
   compared old against new. All ten are identical apart from `color-interpolation-filters="sRGB"` on the tile filter
   (`*ts`) and the id prefixes. Gradient stops, paths, livery and rim strokes are byte-for-byte the same, so the
   approved tile colours (docs/design-guide.md, icon-tile section) are unchanged in the source.
2. **Own renders** (resvg 2.6.2, the build settings) of the old and new SVGs at 1024 px:
   - The changed-pixel counts and maximum channel differences match the README table exactly.
   - Alpha is unchanged on every pixel of every app, so the silhouette and the shadow footprint are the same.
   - The mean colour of the opaque tile moves by at most 0.35 levels per channel (Pacenotes blue channel), so the
     tiles keep their approved colour.
3. **Seam scan of the bare tile** (same rule as tools/scan.mjs: steps above 3 levels in the tile body): 0 for all ten
   with sRGB, largest step 2 levels. With the old linearRGB filter: 3 102 to 8 509 seam steps and steps of up to 14
   levels (Pacenotes, Workbench Demo) and 13 (Spotter). This matches the README.
4. **Committed PNGs.** Each `<app>-macos-1024.png` matches its SVG render exactly (maximum difference 0).
5. **Small frames.** The hand-tuned `small/` frames, the tray and the Windows masters are not in the diff, so they are
   byte-identical. The sheets' 32/16 px rows show no difference.
6. **Visual check.** I looked at every sheet. The ×12 difference shows only horizontal ramp bands across the tile, the
   tile edge and translucent glyph parts (lenses, light bars, wave bars) where the tile shows through. Opaque glyph
   areas are black (unchanged) on every sheet.

## Per-app verdict

| App | Tile | Verdict | Notes |
|---|---|---|---|
| Debrief | blue | **Approve** | Max 18, banding gone (3 516 steps to 0); glyph and small frames unchanged. |
| Pitcrew | orange | **Approve** | Max 18, 4 355 steps to 0. |
| Spotter | amber | **Approve** | Max 20, 5 268 steps to 0 (old steps up to 13 levels); the lens interiors change only where the tile shows through. |
| Pitwall | violet | **Approve** | Max 12, 3 862 steps to 0. |
| Dyno | magenta | **Approve** | Max 13, 3 102 steps to 0. |
| Sighter | lavender #A686D6, dark glyph | **Approve** | Already seam-free; 32 485 px smoothed (max 16) in the lower tile and under the glyph's own shadow. |
| Pacenotes | bronze-olive #6D590F | **Approve** | The dark-ramp case: old steps up to 14 levels, now 0. Clear improvement. |
| Paddock (umbrella) | graphite #2B3038 | **Approve** | Max 6, 8 509 steps to 0; the roof stripe hues are unchanged. |
| Paddock Shell Demo | light grey | **Approve** | 1 479 px change, on the tile edge only. |
| Paddock Workbench Demo | near-black | **Approve** | No pixel change (it was already built with the fix); listed for completeness. |

Nothing looks worse at 16 or 32 px: those frames are hand-tuned SVGs that do not use the filter and did not change.

## Findings

No P1 or P2 findings.

- **P3: the README's "small frames are byte-identical" covers the hand-tuned `small/` set only.** The Tauri raster
  set is made from the macOS 1024 master (`tauri icon`), so `tauri/32x32.png`, `64x64`, `128x128`, `icon.png` and the
  small slots inside `icon.icns` (Finder list view, Dock at small sizes) changed too. They differ by at most 7 levels
  (Spotter 32x32) and 3 levels for Debrief and Paddock, which is not visible. Suggest rewording the README to say this.
- **P3: the carbon alternates were rebuilt too but have no sheet.** All seven `icons/alternative/*-carbon.svg` files
  gained the sRGB attribute. At 512 px they change by at most 5 levels and alpha is unchanged. The `pacenotes-brackets-road` and
  `pacenotes-top-spiral` alternates did not change. These are not shipped, but the README could mention them.
- **P3, follow-up idea, not a blocker: the glyph shadow filter (`*gs`) still interpolates in linearRGB.** It only
  draws a soft shadow under flat glyph colours, so no banding was seen (Sighter shows a small change under its glyph
  only because the tile underneath changed). If it is ever switched to sRGB for consistency, the shadow falloff will
  change slightly, and that change needs its own before/after sheet.

## Summary

Approve all ten. The change is limited to the tile ramp and the shadow edge. The banding is gone (0 seam steps in every
tile, confirmed by an independent scan). The tile colours are the approved ones: the source is unchanged and the mean
colour shift is under 0.4 levels. The 16 and 32 px frames are unchanged.
