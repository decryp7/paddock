# Debrief 1.2: how to draw millions of events fast in the app's webviews

Status: research note, 2026-11-05, at `origin/main` 87c9b8449. Nothing in the product was built or changed.
Marks: **[M]** I measured it, **[S]** I read the source, **[D]** a primary doc or data file, **[U]** secondary or from
memory, unverified. Sizes are judgement unless marked P (from the plan).

## Answer

**Never send or draw events. Draw pixels.** The engine answers with at most about 2 buckets per pixel, so a screen is
about 50,000 rectangles at most however large the file is. That fits **Canvas 2D on the main thread**, which needs no GPU,
works under our CSP and behaves alike on all three webviews. Choose by view:

| View | Draw with | Store gives |
|---|---|---|
| Timeline lanes, histogram, minimap, scroll gutter | One viewport-sized Canvas 2D per visible panel; lane names remain DOM buttons | `Uint32Array` counts per lane on a quantised grid (below) |
| Charts | ECharts, as today (no new library) | At most 2 points per pixel, as typed columns |
| Events grid | DOM, the existing virtual rows. Never canvas | Pages of rows by key, JSON |
| Flame graph | Canvas 2D, as today, with three fixes | Engine-laid-out rectangles; JSON up to the 50,000 default, columns only near the 200,000 cap |
| Heatmaps | None exist and none are planned; if one comes, `putImageData` on Canvas 2D | `{cols, rows, counts: Uint32Array}` |

WebGL2 is not the default, because it is fast only where a GPU exists and fails badly where none does. WebGPU and a worker
are not needed either. The fallbacks are ranked in section 5.

## 1. What each option can draw

The numbers are the most items that held one frame (rAF p95 at most 18 ms), from a ladder of sizes, with a 1600x900
surface at 2x. [M], with the method, and limits, in the [results block](../../apps/debrief/docs/performance.md#2026-11-05-drawing-mechanisms-in-playwrights-browsers-debrief-12-research).
One run under load 14 to 20, so **this is an indication, and only the order of magnitude counts**. These are Playwright's
browsers, not WKWebView, WebKitGTK or WebView2. Software is Chromium with SwiftShader, as a stand-in for a VM.

| Mechanism | WebKit 27.2 | Chromium 156, Metal | Chromium, software | Next ladder step misses by |
|---|---|---|---|---|
| Canvas 2D `fillRect`, grouped by colour | 50k | 50k | 25k | WebKit 100k: 23 ms |
| Canvas 2D one path per colour | 25k | 200k | 10k | Software 25k: 67 ms |
| Canvas 2D `fillText` | 1.6k | at least 6.4k | 3.2k | WebKit 3.2k: 30 ms |
| OffscreenCanvas in a worker (`fillRect`) | 50k | 50k | 25k | same ceiling as main thread |
| WebGL2 instancing | 1.6M | 1.6M | **under 10k** | Software 10k: 200 ms median, 1150 ms p95 |
| DOM bars (plain divs, no Angular, no text) | 5k | 10k | 10k | WebKit 10k: 32 ms |
| SVG lane, 2000-bucket path rebuilt per frame | 10 lanes | 40 lanes | 5 lanes | WebKit 20: 27 ms |

What follows from it:

- **Demand is about 52,000 rectangles at worst.** 1300 plot pixels x 40 visible lanes of 22 px, every column of every lane
  occupied [calc]. Empty buckets draw nothing, so real screens are lower. A 3840-pixel-wide window can reach 140,000, and
  that needs the cheaper paths of section 5.
- **Today's shape runs out first on WebKit.** Lanes are SVG paths and a button per marker
  (`apps/debrief/app/src/app/timeline/timeline-lanes.ts:41`). This is the worst case, rebuilt on every frame as a pan would.
  ADR 0018's amendment already chose canvas lanes; the measurement supports it.
- **Do not batch into one path.** It looks like the obvious optimisation and is 4x faster on Chromium with a GPU, but it is
  slower on WebKit and much slower in software. Use `fillRect` grouped by colour, the only form that holds up everywhere.
- **A worker adds no throughput.** Same ceiling on all three; its only gain is isolation from a busy main thread. It also
  costs: no `invoke` in a worker, because the IPC key lives in a closure on `window` (`tauri-2.12.5/scripts/ipc-protocol.js`)
  [S]; no CSS custom properties, so tokens must be posted in; fonts must be loaded in the worker. Debrief and `paddock-ui`
  have no `new Worker` today [S].

### Mechanisms not chosen

- **WebGL2.** Perfetto [added a WebGL renderer on 2026-02-02](https://github.com/google/perfetto/blob/main/ui/src/base/gl/webgl_renderer.ts)
  and keeps a [Canvas 2D fallback "for when WebGL is unavailable"](https://github.com/google/perfetto/blob/main/ui/src/base/canvas2d_renderer.ts) [S].
  That is two renderers to test, justified by drawing raw slices. We draw far fewer items. Without a GPU it is the worst
  option here, or it is absent:
  - Chromium's [intent to remove the SwiftShader fallback](https://groups.google.com/a/chromium.org/g/blink-dev/c/yhFguWS_3pM)
    (2025-02-13, estimated desktop 137) says "WebGL context creation will fail" [D]. WARP on Windows is not covered, and
    whether a VM's WebView2 gets a context is unknown. Playwright passes `--enable-unsafe-swiftshader` itself [S], so my
    software row exists only because of that flag.
  - On WebKitGTK, [Tauri's docs, updated 2026-06-15](https://v2.tauri.app/develop/debug/linux-graphics/), say context
    creation succeeds even on a software or slow path, and the renderer string always reads `Apple GPU`, so the app cannot
    detect it; they advise a non-WebGL fallback and a user setting [D]. Automatic selection on Linux is therefore not possible.
- **WebGPU.** Safari 26 or later only ([MDN data](https://github.com/mdn/browser-compat-data/blob/main/api/GPU.json)) [D].
  WebKit's preference is on only under `ENABLE(WEBGPU_BY_DEFAULT)` ([source](https://github.com/WebKit/WebKit/blob/main/Source/WTF/Scripts/Preferences/UnifiedWebPreferences.yaml)) [D];
  whether WebKitGTK builds set it, or WKWebView exposes it on macOS 26, I did not confirm [U].
- **uPlot.** MIT, 22 KB gzipped against 369 KB for the full ECharts bundle [M], and its last npm publish was 2025-03-14 [D].
  It wins only on raw points, which we will not send. ECharts is already in the bundle, themed and tested
  (`ui/projects/paddock-ui/chart/`, about 200 kB in Debrief per `performance.md`, "after Angular phase 1"). Adding a second chart library breaks
  rule 42 for no gain. Rezooming 1M points [M], WebKit / Metal / software: uPlot 15 / 17 / 91 ms; ECharts with `lttb`
  15 / 17 / 61 ms; ECharts unsampled 80 / 100 / **33,301 ms**. So `sampling` must stay on, as it is in
  `metrics/chart-option.ts:91`, and the engine should bound the points anyway. Rule 47 is not affected: Material has no chart.
- **DOM with virtualization.** Right for the grid and for lane rows, wrong for bars. Good text, selection and accessibility,
  and an order of magnitude fewer items than canvas.

## 2. Platform checks

| Claim | Result | Evidence |
|---|---|---|
| Canvas 2D, WebGL2 and worker work under the CSP | Yes on WebKit and Chromium, with Debrief's policy made stricter (`style-src 'self'`), served as a header. Neither chart library raised a violation | [M] |
| Blocked by the CSP | `blob:` workers, `eval`, WebAssembly (no `wasm-unsafe-eval`), `setAttribute('style', ...)`. Setting `el.style` through CSSOM works, which is what Angular's style bindings do | [M] |
| Worker scripts | Allowed same-origin through `default-src`. Tested over `http`; **not tested over `tauri://`** | [M], unknown |
| OffscreenCanvas | Safari 16.4 (2D), 17 (`webgl2`). Enabled for GTK in WebKit's preferences at `main`, including in workers | [D] [BCD](https://github.com/mdn/browser-compat-data/blob/main/api/OffscreenCanvas.json) |
| WebKitGTK versions | Ubuntu 24.04: 2.52.6. 22.04: 2.50.4. Debian 12: 2.50.6 (packages.ubuntu.com, packages.debian.org, read 2026-11-05) | [D] |
| ResizeObserver | `contentBoxSize` everywhere. **`devicePixelContentBoxSize` is not in Safari** ([BCD](https://github.com/mdn/browser-compat-data/blob/main/api/ResizeObserverEntry.json); absent in Playwright WebKit, present in Chromium) | [D] [M] |
| High DPI | Use `contentBoxSize x devicePixelRatio`, listen for a DPR change with `matchMedia('(resolution: ...)')` for a move between monitors, and snap x and widths to device pixels so 125% and 150% scaling stay crisp. `flame-chart.ts:222,263` reads DPR only when it draws | [S], judgement |
| Debrief's real CSP | `tauri.conf.json:30` has `style-src 'self' 'unsafe-inline'`, while `AGENTS.md:81` says inline style attributes are forbidden. Canvas does not depend on this; flagged for whoever owns the policy | [S] |

## 3. Text, hit testing, accessibility

The pattern already exists: `profiling/flame/flame-chart.ts` has an `aria-hidden` canvas in a focusable surface with arrow
keys, a live region and a real table of the largest frames [S]. Lanes follow it:

- **Lane names and meta stay real `<button>`s in virtual DOM rows** (the plan's "under 100 lane rows"). Canvas carries only
  bars, ticks and glyphs.
- **Markers stay reachable.** The lane with keyboard focus materialises real buttons for its clusters, keeping today's one
  tab stop per lane, arrow keys and 24 px target (`timeline-lanes.ts`); others are painted. The same events stay reachable as
  text in the grid.
- **No colour alone.** Draw the existing `MarkerGlyph` shapes as `Path2D` sprites cached per DPR, keep the minimum height for
  error and warning segments (`histogram.ts:16`), and keep the worded legend. Colours come from resolved tokens
  (`readPaddockTokens`), re-read on theme change; a canvas does not follow forced-colours mode by itself, so test it.
- **Hit testing** is a binary search over sorted typed columns: 70 to 100 ns per lookup over 1M [M]. Keep it pure.
- **Text.** Crop and cache labels; skip labels under about 28 px as `flame-draw.ts:18` does (Perfetto uses 5 px, in
  [`slice_track.ts`](https://github.com/google/perfetto/blob/main/ui/src/components/tracks/slice_track.ts)) [S]. A screen shows
  hundreds of labels, well inside the 1.6k measured. `fitLabel`'s measuring in a binary search is the costly part; cache it.
- **Exports.** `share/visuals/` builds its own SVG with `timelineToSvg`, so it is independent of the screen renderer [S]. Keep it.

## 4. Getting data to the UI

**Tauri's three routes** [S, tauri 2.12.4 as locked and 2.12.5]:

- `invoke` with JSON: `fetch` to `ipc://` on all desktop OS, then `response.json()`. There is no longer a Linux special case
  (`scripts/ipc-protocol.js:20`).
- `invoke` returning `ipc::Response`: same route, `response.arrayBuffer()`. If the custom protocol ever fails, the fallback
  sends bytes as a JSON array through eval on macOS (`src/ipc/protocol.rs:390`), so log that case (ER-27).
- `Channel`: below 8 KB of JSON or 1 KB of raw it evaluates directly; above that, Rust queues the data and JS fetches it, one
  more round trip per message (`src/ipc/channel.rs:36-38,302-312`). Debrief uses it for query progress (`store_pane.rs:267`).

**Parse cost is lower than expected** [M]:

| Payload | JSON size | `JSON.parse` | Binary size | Typed view | JS heap, parsed |
|---|---|---|---|---|---|
| 40 lanes x 2000 buckets x 6 levels | 1.5 MB | 4 ms | 1.9 MB | under 0.1 ms | 4.2 MB |
| 200,000 flame rectangles | 24.6 MB | 19 to 24 ms | 8.6 MB | under 0.1 ms | 14.4 MB |
| 1M time and value points | 27.7 MB | 28 to 34 ms | 16 MB | under 0.1 ms | not measured |

So typed arrays save 2x memory and a dropped frame or two, not an order of magnitude. Small counts are even smaller in JSON
than in `u32`. This agrees with the audit's "not worth doing: IPC cost". **`apps/debrief/docs/performance.md` has no number
for the webview boundary**; its IPC numbers are `debrief-ipc` between processes. Rust serialisation and the copy into the
webview remain unmeasured.

**Recommendation.**

1. **Typed arrays are the contract inside the UI**, whatever the wire, created once at the backend edge. Draw loops and
   caches never see `number[][]`, and the mock backend returns the same types.
2. **New bulk endpoints answer raw; everything else stays JSON.** `lane_buckets` is new in 3.3, so it costs little to do it
   right: a version, little-endian, a small header with column offsets that keep alignment (`Float64Array` needs 8), one
   declared layout with a golden test across Rust and TypeScript. Do not migrate `query_page`, facets or others. M2 can
   overturn this if raw shows no gain.
3. **Batch per panel, per change.** One request names all visible lanes, range and resolution, and returns one buffer. Today
   each panel asks for all lanes with a bucket count derived from its width (`TimelineRequest.buckets`, 10 to 2000), on no
   fixed grid, so a pan cannot reuse an earlier answer. A newer request supersedes the older one (`query_cancel.rs` has a cancel mechanism to build on).
4. **`Channel` only for progressive delivery**, when a slow lane should not hold back the others: raw per-lane buffers. It is
   not the default for one-shot answers because of the extra round trip.

**Lane contract (for 3.3).** Following [Perfetto's `BufferedBounds`](https://github.com/google/perfetto/blob/main/ui/src/components/tracks/buffered_bounds.ts) [S]:

```
LaneBucketsRequest { generation, query, laneKeys[], resolutionMs /* power of 2 */, start, end /* 3x visible, quantised */ }
LaneBuckets        { generation, resolutionMs, start, bucketCount, truncated,
                     laneKeys[], counts: Uint32Array /* lanes x buckets */, markers: { lane, bucket, kind, count, id } columns }
```

The cache key is `(query, lane, resolution, start)`; panning inside the padded range issues no query; off-screen and hidden
panels issue none. Counts use one bucket per pixel; a series where peaks matter uses min and max at 2 per pixel. Cap the UI
cache by bytes (judgement: 16 MB, a Setting per rule 21; 7,800 buckets x 40 lanes x 4 B is 1.25 MB per screen [calc]).
Hidden panels also release their canvas: 1600x900 at 2x is 11.5 MB [calc], so the 12 extra panels of ADR 0018 would cost
about 140 MB if kept. Physical footprint in the webview process was not measured.

## 5. Ranking and fallbacks

1. **Canvas 2D on the main thread, redrawn only on change.** No rAF loop while idle, which also respects the idle-CPU work.
   The draw is a pure function `drawLanes(ctx, frame)` on an interface as `FlameCtx` is, so it can be tested and moved.
2. If M1 misses budget, **make it cheaper first** (rule 51): merge adjacent equal bars; during a pan, blit the canvas onto
   itself and draw only the exposed strip. Both are unmeasured here; self-copy in particular needs checking on WebKit.
3. Then **OffscreenCanvas in a worker**, if the main thread is what is busy; the pure draw function moves as it is.
4. Last, **WebGL2 as an explicit opt-in Setting** beside Canvas 2D, never auto-selected on Linux. Only if 3840-pixel windows
   prove to be a real problem.

**Risks.**

- The numbers do not hold on real webviews, or on WebKitGTK in software, which is the slowest likely case: M1 settles it.
- Canvas costs accessibility if the focused-lane pattern is skipped: make it part of 2.4's acceptance, with a screen-reader
  pass, because axe cannot see into a canvas.
- At least 12 `apps/debrief/app/e2e` files mention timelines or markers; DOM selectors on painted items must move to
  testids or to a test hook over the view model. Budgeted in 4.1.
- A binary wire is a second contract: the golden test and the version check are required.

## 6. Packages (sizes judgement; P from the plan)

| Package | Adds | Size |
|---|---|---|
| **M0, new** | M1 and M2 below, before 2.4 | 1 |
| **2.4** Lane canvas, P 3 | `drawLanes`, DPR and resize handling, pixel snapping, glyph sprites, hit test, focused-lane buttons, frame-budget test on WebKit and Chromium | 3 + 1.5 |
| **3.3** Timeline engine, P 3 | `lane_buckets`: batch, power-of-two grid, padded range, supersede, raw encoder, golden test. Includes the peer note's idea 4 (1 to 2 there) | 3 + 1.5 |
| **4.1** Timeline UI, P 3.5 | Adoption, typed cache with a byte cap, canvas release when hidden, e2e selectors | 3.5 + 1 |
| **3.7** Grid markers | No change: its gutter paints from 3.3's bounded markers query | 0 |
| **F1, new** Flame fixes | Hover on an overlay (`flame-chart.ts:258` redraws everything on hover); no `canvas.width` reset per draw (`:264`), which reallocates and clears; draw only visible rows through `byDepth`; DPR change | 1 |
| **C1, new** Chart bound | Engine caps `query_chart` at 2 points per pixel; columns to ECharts' dataset; test that `sampling` cannot be lost | 1 |

About 6.5 days on top of the plan's 9.5 for these three, partly overlapping the peer note's 1 to 2. 2.4 is shared code, so it
waits as the plan already orders it. ADR 0018's amendment already decides canvas; the binary layout needs a short ADR or an
amendment. Parallelism and cost (rule 51): no parallel drawing; the gain is in fewer items, fewer queries and no repeats.

## 7. Measure first

Record each as a rule 48 block. Machine load at or below 12.

- **M1, real webviews, go or no-go.** A dev-only page in the built app drawing 52,000 grouped `fillRect`s over 60 frames, on
  WKWebView, WebKitGTK and WebView2 with a GPU, plus WebKitGTK with `LIBGL_ALWAYS_SOFTWARE=1` (CI's container already runs that
  way, `debrief.yml:734`) and a Windows VM. Probe first (ER-33): rAF ticks at display rate and pixels changed. Pass: p95 within
  one frame with a GPU, and at most 2 frames in software. Also report whether `webgl2` returns a context in the VMs.
- **M2, the wire.** 500 KB, 2 MB, 16 MB through `invoke` JSON, `ipc::Response` and `Channel`: main-thread blocking, total latency,
  app footprint. Decides item 2 of section 4. Report whether the custom protocol stays up on all three OS.
- **M3, baseline.** Pan and zoom on today's timeline at 60 lanes, from plan package 0.2, so that the change has a before.
- **M4, only if step 3 of section 5 is reached.** Worker loading over `tauri://` and `http://tauri.localhost`.

## 8. Unknown

- **All frame numbers come from Playwright's browsers on an M4 Pro** in headless mode, one run, under load. No WKWebView,
  WebKitGTK or WebView2 number exists. Windows and Linux were not run at all. Headless rAF may not include composition.
- Whether WebView2 in a VM has a WebGL context today, and whether a shipped build still honours the SwiftShader switch.
- The cost at the Rust and webview boundary; the webview's physical footprint under canvas.
- How real screens fill: 52,000 is an upper bound I calculated, not a measured screen.
- Chart runs confirmed a canvas exists and that time was spent; pixels were checked only on the main-thread Canvas 2D runs.
- Screen-reader behaviour of the proposed focused-lane pattern, untested. Sizes above are judgement.
