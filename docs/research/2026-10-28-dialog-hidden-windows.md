# Dialog windows that open hidden: what the three webviews do (ADR 0026 package 1.1-0)

- Date: 2026-10-28
- Question: in a Tauri 2 app, does a dialog window created with `visible: false` run JS, resolve `document.fonts.ready`,
  lay out (`getBoundingClientRect`, `ResizeObserver`) and fire `requestAnimationFrame` (and so Angular's
  `afterNextRender`) before it is shown, on WKWebView (macOS), WebView2 (Windows) and WebKitGTK (Linux)? Which of ADR
  0026 §4.4's three show methods fits each OS, how long does it take, and is the size exact at first paint?
- Probe: `spikes/dialog-hidden/` (a minimal Tauri 2.12.5 app, tao 0.37.2, wry 0.57.1; not product code). Raw logs and
  summaries: see "Runs" below.
- Status: **partial.** Measured: macOS (displays asleep and session locked, methods a to c) and Linux (Xvfb, methods a
  to d). **Not measured: Windows** (its run was queued behind the main CI when this note was committed; see Runs), macOS
  with the displays awake, macOS method d, a second monitor, and Linux under a real window manager. The proposed ADR
  amendment is at the end and is conditional on the Windows run.

## Method

The probe opens a host window and then one probe dialog per trial, 560 logical px wide, created at today's guessed
height of 520. The page (`dist/probe.js`) reports every signal it sees, with the time since the window was created:
script start and `document.visibilityState`, `DOMContentLoaded`, `setTimeout(0)` lag, the initial
`document.fonts.ready`, the first two `requestAnimationFrame` callbacks, `ResizeObserver` callbacks on the dialog frame.
Then it reports `ready` with the frame's natural height, by the trial's method:

| Method | Window | The page reports `ready` after |
|---|---|---|
| **a** hidden, then measure | `visible(false)` at the placed position | content, `fonts.ready`, then the next animation frame (today's shell: `afterNextRender` + `fonts.ready`) |
| **b** visible off-screen | `visible(true)` at (-30000, -30000), unfocused | as a; Rust then moves it into place |
| **c** hidden, synchronous layout | `visible(false)` at the placed position | content, `fonts.ready`, then a synchronous `getBoundingClientRect` (no frame wait); if that read started a font load, `fonts.ready` once more |
| **d** as c, at the declared width | `visible(false)` at the placed position | as c, with the frame given the dialog's declared CSS width (`560 / zoom`) while hidden; the fixed width is dropped at show |

On `ready`, Rust converts the CSS height to logical px (`h * devicePixelRatio / scale`, which matters only under webview
zoom), calls `set_size`, shows and focuses the window, and tells the page, which measures again at once
(`shown_sync`), after the next frame (`after_show`) and 300 ms later (`settle`). **Exact** means: the frame's height
after show equals the reported one, the page's `innerHeight` equals it (the content fills the viewport, no scroll bar),
and the window's inner size equals the request. If `ready` has not come 3 s after create (the spike's deadline, three
times ADR 0026's), Rust shows the window anyway and waits 3 s more.

Scenarios: `normal`; `font_slow` (the face's file arrives 400 ms late through a custom protocol); `route_slow` (the
body is inserted 600 ms after start, standing in for a lazy route); `zoom125`, `zoom150` (`Webview::set_zoom`, a
stand-in for display scale where the machine has one scale only); `unowned` (no parent window; every other trial is
owned by the host, as a modal is); `monitor2` (centred on the last monitor, when there are several). Three repetitions
each unless noted.

## Results

### macOS 27 (WKWebView), Mac mini, two displays at scale 1, **displays asleep and session locked**

The user was away during the run: `CGDisplayIsAsleep` was true and `CGSSessionScreenIsLocked` was true, and
`NSScreen.screens` was empty (`available_monitors()` returned none, so `monitor2` did not run). This is the state of a
Mac whose user walked away, which a dialog can meet (a job that finishes and asks). It is not the state of a user who
just clicked a button. Waking the displays on someone else's machine was out of scope (ER-14); the awake run is a
decision for the user (below).

| method | scenario | n | JS runs hidden | rAF before show | fonts.ready before show | ResizeObserver before show | setTimeout(0) lag ms (median) | ready ms median / max | missed 3 s deadline | font loaded at ready | size exact after show | resize events after show |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| a | font_slow | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | - / - | 3/3 | 0/3 | 0/3 | 0 |
| a | normal | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | - / - | 3/3 | 0/3 | 0/3 | 0 |
| a | route_slow | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | - / - | 3/3 | 0/3 | 0/3 | 0 |
| a | unowned | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | - / - | 3/3 | 0/3 | 0/3 | 0 |
| a | zoom125 | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | - / - | 3/3 | 0/3 | 0/3 | 0 |
| a | zoom150 | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | - / - | 3/3 | 0/3 | 0/3 | 0 |
| b | font_slow | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | - / - | 3/3 | 0/3 | 0/3 | 0 |
| b | normal | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | - / - | 3/3 | 0/3 | 0/3 | 0 |
| b | route_slow | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | - / - | 3/3 | 0/3 | 0/3 | 0 |
| b | unowned | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | - / - | 3/3 | 0/3 | 0/3 | 0 |
| b | zoom125 | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | - / - | 3/3 | 0/3 | 0/3 | 0 |
| b | zoom150 | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | - / - | 3/3 | 0/3 | 0/3 | 0 |
| c | font_slow | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 1 | 479 / 480 | 0/3 | 3/3 | 0/3 | 1 |
| c | normal | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | 69 / 74 | 0/3 | 3/3 | 0/3 | 1 |
| c | route_slow | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | 662 / 669 | 0/3 | 3/3 | 0/3 | 1 |
| c | unowned | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | 64 / 64 | 0/3 | 3/3 | 0/3 | 1 |
| c | zoom125 | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | 71 / 73 | 0/3 | 3/3 | 0/3 | 1 |
| c | zoom150 | 3 | 3/3 | 0/3 | 3/3 | 3/3 | 2 | 69 / 70 | 0/3 | 3/3 | 0/3 | 0 |

"Size exact" is 0 everywhere on this Mac: methods a and b never reported, and c's fitted window was 32 px short
(next list). Ready times for c are from window creation: about 65 ms, plus the font's or the route's delay.

Per signal, in every trial and every method:

- JS runs, `DOMContentLoaded` fires and `setTimeout(0)` lags 1-2 ms while hidden.
- `document.fonts.ready` resolves while hidden and **waits for the face**: with the font 400 ms late it resolved at
  about 420 ms with the face loaded (`fonts.check` true) in every trial. Layout ran while hidden, so the font load had
  started.
- Layout runs while hidden: `getBoundingClientRect` returns the natural height (260 px for the probe text) and
  `ResizeObserver` fires before show.
- **`requestAnimationFrame` never fires**, hidden or shown, owned or not, off-screen or placed, for the whole 6 s of a
  trial. Method a and b therefore never reported `ready`; the deadline showed them at the guessed 520 px. WebKit stops
  the display link for a page that is not visible on screen, and with the displays asleep nothing is.
- **An owned window is on screen before it is shown.** With a parent, `is_visible()` was true right after `build()`
  in every owned trial; unowned windows were false. tao 0.37.2 adds the child with
  `addChildWindow:ordered:` at creation (`src/platform_impl/macos/window.rs`, line 303), and AppKit orders a child window
  in with its parent ([Apple: addChildWindow(_:ordered:)](https://developer.apple.com/documentation/appkit/nswindow/addchildwindow(_:ordered:))).
  So `visible: false` plus `parent` is not hidden on macOS: the user would see the 520 px guess and the jump.
- **The viewport is 32 px shorter than the window's inner size.** Tauri reported inner = outer = 560 x 520 (and 260 after
  the fit) while the page's `innerHeight` was 488 (228 after the fit), in every trial. A window fitted to the content's
  height by `set_size(content)` showed a vertical scroll bar (the frame's width dropped from 560 to 543) and cut 32 px
  off the footer: ADR 0016's first defect ("Close cut off"). This was measured with no screen attached to the session,
  so the 32 px may be a title bar that AppKit could not lay out against a screen; it must be rechecked awake. The fix
  does not depend on the cause (below: fit by the viewport delta).
- Webview zoom: `devicePixelRatio` follows the zoom (1.25, 1.5) and the CSS height shrinks with the wider CSS
  viewport; converting by `dpr / scale` gave the right logical height (381 and 492 px).

### Windows (WebView2), CI host runner in the user's desktop session

**Not measured.** The first dispatch (run 6611, on 88b0b8e4c) sat queued for an hour and was cancelled at 15:38, just
before the Mac and the Windows host crashed. The second (run 6637, on 3af528725, all four methods) was dispatched once
the runner was back and was still queued behind 13 Windows jobs of the main CI when this note was committed. Its job
log prints the summary tables and the raw JSONL between `=== PROBE JSONL BEGIN ===` markers; the table goes here.

What the documentation says (read, not verified here or by this probe): WebView2 keeps running script in a hidden controller but may throttle
timers and suspend rendering when `IsVisible` is false
([Microsoft: ICoreWebView2Controller.IsVisible](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller#get_isvisible)),
so rAF before show is not to be relied on there either; a synchronous layout read should work, as it did on the other two.

### Linux (WebKitGTK 2.52, GTK 3.24), paddock-ci:v2 under Xvfb, no window manager

| method | scenario | n | JS runs hidden | rAF before show | fonts.ready before show | ResizeObserver before show | setTimeout(0) lag ms (median) | ready ms median / max | missed 3 s deadline | font loaded at ready | size exact after show | resize events after show |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| a | font_slow | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 5 | 3514 / 3519 | 3/3 | 3/3 | 2/3 | 1 |
| a | normal | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 5 | 3515 / 4018 | 3/3 | 3/3 | 3/3 | 1 |
| a | route_slow | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 5 | 3514 / 3515 | 3/3 | 3/3 | 2/3 | 1 |
| a | unowned | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 5 | 3515 / 3516 | 3/3 | 3/3 | 3/3 | 1 |
| a | zoom125 | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 5 | 3514 / 3518 | 3/3 | 3/3 | 3/3 | 1 |
| a | zoom150 | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 6 | 3521 / 3523 | 3/3 | 3/3 | 1/3 | 1 |
| b | font_slow | 3 | 3/3 | 3/3 | 3/3 | 3/3 | 5 | 987 / 987 | 0/3 | 3/3 | 3/3 | 0 |
| b | normal | 3 | 3/3 | 3/3 | 3/3 | 3/3 | 4 | 579 / 587 | 0/3 | 3/3 | 2/3 | 0 |
| b | route_slow | 3 | 3/3 | 3/3 | 3/3 | 3/3 | 5 | 1163 / 1166 | 0/3 | 3/3 | 0/3 | 0 |
| b | unowned | 3 | 3/3 | 3/3 | 3/3 | 3/3 | 5 | 581 / 587 | 0/3 | 3/3 | 2/3 | 0 |
| b | zoom125 | 3 | 3/3 | 3/3 | 3/3 | 3/3 | 5 | 583 / 591 | 0/3 | 3/3 | 3/3 | 0 |
| b | zoom150 | 3 | 3/3 | 3/3 | 3/3 | 3/3 | 4 | 593 / 623 | 0/3 | 3/3 | 2/3 | 0 |
| c | font_slow | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 6 | 478 / 479 | 0/3 | 3/3 | 0/3 | 1 |
| c | normal | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 5 | 80 / 81 | 0/3 | 3/3 | 0/3 | 1 |
| c | route_slow | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 5 | 669 / 683 | 0/3 | 3/3 | 0/3 | 1 |
| c | unowned | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 5 | 80 / 87 | 0/3 | 3/3 | 0/3 | 1 |
| c | zoom125 | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 6 | 77 / 79 | 0/3 | 3/3 | 0/3 | 1 |
| c | zoom150 | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 5 | 78 / 99 | 0/3 | 3/3 | 0/3 | 1 |
| d | font_slow | 3 | 3/3 | 1/3 | 3/3 | 1/3 | 5 | 481 / 481 | 0/3 | 3/3 | 3/3 | 1 |
| d | normal | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 4 | 69 / 87 | 0/3 | 3/3 | 3/3 | 1 |
| d | route_slow | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 5 | 668 / 669 | 0/3 | 3/3 | 3/3 | 1 |
| d | unowned | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 5 | 74 / 75 | 0/3 | 3/3 | 3/3 | 1 |
| d | zoom125 | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 4 | 79 / 132 | 0/3 | 3/3 | 3/3 | 1 |
| d | zoom150 | 3 | 3/3 | 0/3 | 3/3 | 0/3 | 6 | 74 / 81 | 0/3 | 3/3 | 3/3 | 1 |

Run 6638 on 3af528725, compositing on, scale 1 (the table). Compositing off (`WEBKIT_DISABLE_COMPOSITING_MODE=1`, as CI's
e2e) and `GDK_SCALE=2` (one repetition) gave the same pattern; their tables are in the job log.

- JS, `DOMContentLoaded`, `fonts.ready` (waiting for the late face) all run hidden, as on macOS.
- **rAF and `ResizeObserver` mostly do not fire hidden** (rAF before show in 0 to 1 of 3 trials per scenario), so a
  misses the deadline in every trial and is fitted only after it is shown: the user sees the guess, then the jump.
- **An unmapped WebKitGTK view has a 0-wide viewport.** Method c's synchronous read wrapped the text at zero width and
  reported 1575 px for 257 px of content, in every trial. Method d, which gives the frame its declared width while
  hidden, read 257 px (and the zoomed heights) correctly and was **exact after show in 42 of 42 trials** across the three
  configurations, with `innerHeight` already right at the first measurement after show.
- d is ready about 75 ms after create; the slow font and slow route add their delay (about 480 and 670 ms).
- **b works but is slow and can flash.** Building a visible GTK window blocks about 520 ms before the page starts, so b
  is ready at about 580 ms. `set_size` is applied asynchronously: right after it the window still reports 520, and in a
  third of the trials the page's `innerHeight` was still 520 in the first frame after show (the user would see the
  guessed height for a frame). This was without a window manager; a real one may differ.

## What this means for the three methods

| | macOS (asleep) | Linux (Xvfb) | Windows |
|---|---|---|---|
| a hidden, wait a frame | never ready (rAF never fires); deadline, then guess | never ready before the deadline; guess, then jump | not measured |
| b visible off-screen | never ready (rAF never fires) | ready ~580 ms; exact after settle, sometimes one frame at the guess | not measured |
| c hidden, sync read | ready ~65 ms; viewport 32 px short of the window | wrong (0-wide viewport) | not measured |
| d hidden, sync read at declared width | not measured (expected as c) | ready ~75 ms; exact 42/42 | not measured |

Common to every measured case: **no frame callback can be relied on before show** (rAF, and so Angular's
`afterNextRender`), while script, `fonts.ready` and synchronous layout work everywhere. Today's shell waits for
`afterNextRender`, so it cannot report ready from a hidden window.

## Recommendation

**Method d on every OS, provisionally until the Windows run confirms it:** create the dialog hidden; in the page, wait
for the content and `document.fonts.ready`, give the frame its declared width in CSS px, read its height synchronously
(`getBoundingClientRect`), and if that read set `document.fonts.status` to `loading`, wait for `fonts.ready` and read
again; send `dialog_ready` without waiting for a frame. Release the fixed width at show; the natural-height
`ResizeObserver` refit (§4.5) covers anything that differs after show.

Two fixes are needed whatever the method:

1. **macOS: own the window at show, not at create.** tao adds a child window to its parent at creation, and AppKit then
   shows it, so `visible: false` plus a parent is not hidden. Create unowned and attach the parent at show (or keep
   macOS modals unowned, as the scrim design already allows).
2. **Fit by the viewport delta, not by the window's inner size.** Measure `window inner height - innerHeight` from the
   hidden window and add it to the content height. On the asleep Mac the delta was 32 px (the cut-off footer of ADR
   0016); elsewhere it was 0.

Fallback if Windows shows that a hidden WebView2 does not lay out: method b on Windows only (visible off-screen,
unfocused), which keeps the same handshake and costs a slower ready.

## Edge cases

- **Fonts not yet loaded:** `fonts.ready` waits for faces already requested; d's re-check of `fonts.status` after the
  read catches a face the read itself requested. The 400 ms late font was loaded at ready in every trial (macOS, Linux).
- **Slow route:** ready waits for the content; the deadline (1 s) covers routes slower than that, showing Loading and
  refitting once (ADR 0026 §4.3). A 600 ms route came in under the deadline on both OS.
- **HiDPI and zoom:** webview zoom 1.25 and 1.5 and `GDK_SCALE=2` were exact with d; converting with
  `dpr / scale` is correct. Real mixed-DPI monitors were not tested.
- **Multiple displays:** not measured (the Mac had no screens attached during its run; Xvfb has one). The placement code
  is unaffected by the method; the 1.1b tests must cover it.
- **Display asleep or session locked (macOS):** no frames at all, so only a frame-free method (c or d) can report ready.

## Runs

- macOS, local, 2026-10-28 14:30 to 14:47, 711 events (`mac.jsonl`), methods a to c, displays asleep. A method d run was
  started at 15:39 and lost in the crash (empty log).
- Linux, CI run 6614 (job 14072) on 478b0b309, methods a to c; CI run 6638 (job 14426) on 3af528725, methods a to d.
- Windows, CI run 6611 cancelled while queued; CI run 6637 (job 14423) on 3af528725 queued, result pending.
- Raw logs (synthetic page content only): `/Volumes/d7sst7/dev-targets/dialog-spike/logs/` on the Mac mini; summarise with
  `node spikes/dialog-hidden/summarize.mjs <file> [--trials]`.

## Proposed amendment to ADR 0026

Proposed, conditional on the Windows run:

- §4.2: the ready handshake must not wait for a frame (`afterNextRender`, rAF). The frame reports `dialog_ready` after
  content and `document.fonts.ready`, from a synchronous layout read with the frame at the declared width, re-reading
  once if the read started a font load.
- §4.2: fit with the viewport delta (the window's inner height minus the page's `innerHeight`, read hidden) added to the
  content height.
- §4.4: replace the spike paragraph with "Method d on all three OS (research note 2026-10-28)", naming b on Windows as
  the fallback if the Windows result requires it.
- §4.6: on macOS the parent is attached at show, not at create (tao orders a child window in at creation).
- §6: the probe stays as the `#[ignore]` real-window test, with method d and the exactness check.
