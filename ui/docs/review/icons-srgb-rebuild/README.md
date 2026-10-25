# Family icons rebuilt with an sRGB drop-shadow filter (before and after)

One sheet per app. Columns: old (git `shell/x50-workbench`), new, difference (per-channel absolute difference times 12, black = identical). Rows: 1024 px macOS master; 32 px and 16 px small frames (actual size, then nearest-neighbour enlargement; the small frames do not use the filter and are byte-identical old and new). Seam steps are the count of adjacent-pixel steps above 3 levels in the bare tile (tools/scan.mjs rule), with the old linearRGB filter and with the new sRGB one.

| App | Sheet | Max channel difference (0 to 255) | Pixels changed of 1048576 | Seam steps before | Seam steps after |
|---|---|---|---|---|---|
| Debrief | [debrief.png](debrief.png) | 18 | 403457 | 3516 | 0 |
| Pitcrew | [pitcrew.png](pitcrew.png) | 18 | 411655 | 4355 | 0 |
| Spotter | [spotter.png](spotter.png) | 20 | 385388 | 5268 | 0 |
| Pitwall | [pitwall.png](pitwall.png) | 12 | 352691 | 3862 | 0 |
| Dyno | [dyno.png](dyno.png) | 13 | 362306 | 3102 | 0 |
| Sighter | [sighter.png](sighter.png) | 16 | 32485 | 0 | 0 |
| Pacenotes | [pacenotes.png](pacenotes.png) | 10 | 418686 | 3560 | 0 |
| Paddock | [paddock.png](paddock.png) | 6 | 477341 | 8509 | 0 |
| Paddock Shell Demo | [shell-demo.png](shell-demo.png) | 16 | 1479 | 0 | 0 |
| Paddock Workbench Demo | [workbench-demo.png](workbench-demo.png) | 0 | 0 | 6894 | 0 |

Notes
- The old column is the committed macOS master SVG rendered at 1024 px; the new column is the rebuilt one. "Seam steps before" re-renders the new bare tile with the old linearRGB filter, so it is meaningful for every app.
- Paddock Workbench Demo's committed master already had the fix (it was built with it), so its pixel difference is 0; its seam count before is what the old filter gives. Paddock Shell Demo and Sighter had no seam steps above 3 levels but still change by smoothing (max 16 levels, 1 479 and 32 485 pixels).
- Looked at Debrief and Spotter: the difference is horizontal ramp banding and the shadow edge only; glyphs, livery bands, rims and the 16/32 px frames are unchanged (the small frames are byte-identical).
- Windows masters do not use the filter and are unchanged (seam scan clean before and after). `icon.icns` is rebuilt per app (embeds build-specific bytes); Workbench Demo's was restored to the committed bytes because its pixels did not change.
