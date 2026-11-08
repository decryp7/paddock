# 43. Dialogs size to their content: a declared size mode, a continuous measure loop, logical units converted once at the OS edge

- Status: Accepted (2026-11-03; user decisions 2026-11-03; independent review approved). Asked for by the user on
  2026-11-03 ("should Paddock dialogs use the same design as WPF's `Window.SizeToContent`?"). Review round 1 asked for
  changes (three P1, nine P2), answered in the revision; the delta review approved it with the D3 text, fixed here. The
  Windows VM findings (placeholder section) are still to be recorded; a cause they show that this design does not
  cover reopens the ADR.
- Date: 2026-11-03
- Related: rule 17 (every dialog is a real OS window), rule 40 / ER-21 (recurring bugs mean a missing design rule),
  rule 55 / ADR 0034 (one implementation per mechanism), rule 56 / ER-27 (logs explain every failure), rule 57 / ER-28
  (a cause is confirmed only by a run without it), rule 60 (ADR 0041: tests control time); ADR 0009 (dialog kinds and
  contracts), ADR 0016 (dialog window sizing contract; this ADR **amends its decisions 1, 5 and 6**), ADR 0026 (the
  shared dialog API; this ADR **amends §1's height column and §4.2, §4.3 and §4.5**), ADR 0040 (Windows GUI test
  session; this ADR **refines its "Dialogs at two scales" step and adds a DPI-change step**), rule 1 (one release at a
  time: shared `crates/` and `ui/` changes ship inside one app's release), rule 43 (a shared change and its adoption
  land together); Dyno 1.1.0 (where this ships).
- Evidence read at `origin/main` `a18e18801`, on `origin/fix/dialog-sizing` `08ad47b22`, and in tao 0.37.2's
  `src/platform_impl/windows/event_loop.rs` (the version in the local Cargo registry), 2026-11-03.

## Context

### The class of bug

Dialog windows that are the wrong size keep coming back. ADR 0016 listed fifteen instance fixes in Debrief before
2026-10-27 and decided "declared size range, measured size"; ADR 0026 built it as `paddock-dialogs` and the shared
`DialogFrame`. Since then:

| Date | Instance | Source |
|---|---|---|
| 2026-10-27 | ADR 0016: fifteen per-dialog height and footer fixes in Debrief (`7990fef4e` ... `1b8b4e274`) | ADR 0016 Context |
| 2026-10-28 | The 1.1-0 spike: nothing that waits for a frame runs in a hidden window, so the first fit must measure synchronously ("method d") | `docs/research/2026-10-28-dialog-hidden-windows.md`, ADR 0026 §4.4 |
| 2026-10-28 | A webview shorter than the window's inner height (32 px on a Mac): the viewport delta was added | `window.rs` `viewport_delta` |
| 2026-10-28 | A zoomed webview reports CSS px that are not logical px: `zoom_of`, `zoom_after` were added | `window.rs` `zoom_after` |
| 2026-11-03 | Dyno 1.0.0 user report 7: dialogs sized wrong on Windows, notably at 150 %; the Updates window shows a scrollbar beside one line | user feedback; `docs/lessons.md` on `fix/dialog-sizing` |
| 2026-11-03 | `f90e50d24`, `08ad47b22`: content that arrives during the ready handshake was taken as already fitted (the watch baseline was read after the round trip) | `fix/dialog-sizing` |

Each fix was right and local; the next one came from a different corner of the same mechanism. The pattern is the one
ER-21 names: **the size of a dialog window is decided by a sequence of one-off events (create, a ready handshake, a
watch started afterwards, a scale-change hook), each with its own baseline, and nothing states what the size should be
at any moment.**

### How the size is decided today (read in code)

- **Contract.** `Contract { kind, width, height: Option<f64>, .. }` (`crates/paddock-dialogs/src/contract.rs:213-226`).
  The kind table (`Kind::policy`, `contract.rs:121-158`) has `fits_content` and `grows_only`: decision and form fit and
  shrink, a viewer fits but only grows (floor 680x560), a tool is user-sized. The TypeScript table is generated
  (`ts.rs`).
- **Create.** `dialog_open` places a hidden window at the declared width and `UNMEASURED_HEIGHT` 520 (decision, form)
  or the preferred height (`window.rs` `geometry`, `contract.rs:47`).
- **Ready handshake.** `DialogShell.ready` (`ui/projects/paddock-ui/shell/dialogs/src/dialog-shell.ts:155-178`) waits
  for `document.fonts.ready`, pins the frame to the contract width, measures synchronously (`DialogFrame.measure`,
  `dialog-frame.ts:451-478`) and sends `dialog_ready { measure }`. Rust `fit` (`tauri.rs:832-899`) converts CSS px to
  logical with the stored zoom, adds a viewport delta taken once, runs the pure `plan_fit` (`window.rs`), sets the OS
  minimum and the size with `physical_ceil`, moves the window onto its monitor, then `show`s it.
- **After the handshake.** The pin is released and `DialogFrame.watch` starts (`dialog-frame.ts:504-538`): a
  ResizeObserver on `.pd-frame-content` and the footer, a MutationObserver on `<html>` theme, density and font
  attributes, `document.fonts` `loadingdone`, and `requestFit()`; at most one `dialog_fit` per frame, sent when the
  measure differs from a baseline.
- **Scale change.** `WindowEvent::ScaleFactorChanged` calls `scale_changed` (`tauri.rs:969-983`), which refits from
  the last stored measure. The page is not asked to measure again.
- **Resize.** `WindowEvent::Resized` only records bounds to remember (`tauri.rs:576-580`, `681`). Every dialog window
  is resizable (`window.rs:390`, test `no_dialog_window_has_title_bar_buttons_and_every_one_stays_resizable`).

### What that design gets wrong, as a class

1. **Two sources of truth.** The handshake measure and the watch measures are produced by different code with
   different baselines; `08ad47b22` exists because the second baseline was read after the first was sent. Any future
   step between them (a lazy part, a translation load, a late font) can reopen the gap.
2. **No declared mode, so user intent is lost.** A user who drags a form or decision taller is snapped back on the next
   refit (any wrap change fires the ResizeObserver and `plan_fit` sets the height to the content). For a viewer the
   window never shrinks, even when the user did not make it taller. Read in code; not reproduced.
3. **DPI handled as a patch, not at an edge.** The page's measure is CSS px, converted with a zoom that is frozen after
   a scale change (`zoom_after`); the frame size is read as outer minus inner of the live window (`tauri.rs:812-830`),
   which on Windows is the window rectangle including the invisible resize borders (unverified on the VM); after a
   monitor change the stored logical measure is reused without letting the content re-wrap at the new DPI.
4. **The checks ask "did a fit happen", not "is the window the right size now".** The browser e2e
   (`ui/projects/paddock-e2e/src/dialog-fits.ts`) replays the last asked height into a viewport; the real-window smoke
   checks a few dialogs at 100 %; nothing checks that a window refits on late content and stays put after a user
   resize, and nothing runs at 150 % on Windows until ADR 0040 lands.

### How Microsoft solved the same problem (official docs, read 2026-11-03)

- **WPF `Window.SizeToContent`** (<https://learn.microsoft.com/en-us/dotnet/api/system.windows.window.sizetocontent>,
  page updated 2026-05-27): values `Manual` (default), `Width`, `Height`, `WidthAndHeight`. With `Height`, setting
  `Height` does not change the window's height; with `WidthAndHeight`, `Width` and `Height` are ignored. "SizeToContent
  is automatically set to Manual if a user resizes the window by using the resize grip or dragging the border", and when
  content changes cause the window to resize itself, `SizeChanged` is raised. Sizing is driven by the layout system's
  measure pass (`MeasureOverride`), so every invalidation of the content's measure re-sizes the window; Min/Max
  Width/Height constrain it. WPF works in device-independent units (1/96 inch) and converts at the `HwndSource`.
- **WinForms `AutoSize` + `AutoSizeMode`**
  (<https://learn.microsoft.com/en-us/dotnet/api/system.windows.forms.autosizemode>, updated 2026-05-27; the Form
  property page <https://learn.microsoft.com/en-us/dotnet/api/system.windows.forms.form.autosizemode>, updated
  2026-08-13): `GrowAndShrink` "grows or shrinks to fit its contents. The control cannot be resized manually";
  `GrowOnly` grows but "does not shrink smaller than the value of its Size property" and the form can be resized. Min
  and MaximumSize are respected. **`AutoScaleMode`** (<https://learn.microsoft.com/en-us/dotnet/desktop/winforms/forms/autoscale>,
  updated 2026-07-27) is a different idea: designed-in pixel sizes are *scaled* by the ratio of run-time to design-time
  font or DPI at load, on font change and (PerMonitorV2, .NET 6+) on DPI change. Its own page warns that font scheme
  changes "can be problematic" and that mixed modes give "unexpected results". This is what our fixed heights did, and
  what ADR 0016 cause 1 showed breaking.
- **Win32 TaskDialog** (`TASKDIALOGCONFIG`,
  <https://learn.microsoft.com/en-us/windows/win32/api/commctrl/ns-commctrl-taskdialogconfig>, updated 2024-02-22):
  `cxWidth` is the client width in dialog units, "If 0, the task dialog manager will calculate the ideal width";
  `TDF_SIZE_TO_CONTENT` makes the width follow the content. Height always follows the content, and on
  `TDM_SET_ELEMENT_TEXT` "the size or layout of the task dialog may change to accommodate the new text"
  (<https://learn.microsoft.com/en-us/windows/win32/controls/tdm-set-element-text>, updated 2021-04-27). A fixed width
  with a content height is exactly WPF's `Height` mode.
- **DPI at the edge.** `GetDpiForWindow` returns "the DPI of the monitor where the window is located" for a
  per-monitor-aware window
  (<https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getdpiforwindow>, updated 2025-07-01).
  `AdjustWindowRectExForDpi` "calculates the required size of the window rectangle, based on the desired size of the
  client rectangle and the provided DPI"
  (<https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-adjustwindowrectexfordpi>, updated
  2024-02-22; Windows 10 1607+). `WM_DPICHANGED` carries "a suggested size and position of the current window scaled for
  the new DPI" that apps are expected to apply with `SetWindowPos`
  (<https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged>, updated 2025-07-16), and
  `WM_GETDPISCALEDSIZE`, sent before it to Per Monitor v2 windows, lets a window that "wants to scale non-linearly"
  supply its own size (<https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-getdpiscaledsize>, updated
  2025-07-16; Windows 10 1703+). `WM_ENTERSIZEMOVE` is sent "after it enters the moving or sizing modal loop ... when
  the user clicks the window's title bar or sizing border"
  (<https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-entersizemove>, updated 2025-07-16).
- **macOS and Linux equivalents** (read 2026-11-03). AppKit `setContentSize:` sizes the content view "in the window's
  base coordinate system" (points), and `frameRectForContentRect:styleMask:` converts content to frame
  (<https://developer.apple.com/documentation/appkit/nswindow/setcontentsize(_:)>);
  `NSWindowWillStartLiveResizeNotification` is "a notification that the user is about to resize the window", sent once
  per series (<https://developer.apple.com/documentation/appkit/nswindow/willstartliveresizenotification>);
  `NSWindowDidChangeBackingPropertiesNotification` reports a backing-scale change
  (<https://developer.apple.com/documentation/appkit/nswindow/didchangebackingpropertiesnotification>). GTK 3
  `gtk_window_resize` sizes in application pixels "as if the user had done so" and with client-side decorations "will do
  its best to adjust the given size", with no guarantee when the titlebar grows later
  (<https://docs.gtk.org/gtk3/method.Window.resize.html>); `GtkWidget:scale-factor` is the integer device scale
  (<https://docs.gtk.org/gtk3/method.Widget.get_scale_factor.html>).
- **What Tauri gives us** (Tauri 2.12.5, <https://docs.rs/tauri/latest/tauri/enum.WindowEvent.html>, read
  2026-11-03): `Resized(PhysicalSize)` with the client-area size, `ScaleFactorChanged { scale_factor, new_inner_size }`,
  and no event that tells a user resize from a programmatic one. Sizes passed to `set_size` are inner (client) sizes.
- **The DOM side.** `ResizeObserver` reports changes to an element's content or border box, delivered before paint; a
  callback that resizes what it observes loops, and MDN's advice is to record the expected size and skip when it is
  already met (<https://developer.mozilla.org/en-US/docs/Web/API/ResizeObserver>, read 2026-11-03). Because it runs
  before paint, it does not run in a hidden window (the 1.1-0 spike), so the first measure must stay synchronous.

**Conclusion.** WPF's model is the right one for us, and TaskDialog shows it is also the Windows-native behaviour for a
dialog: fixed width, content height, re-layout on content change. AutoScaleMode is the model we are leaving.

## Decision

### 1. A declared size mode per dialog kind, in the Rust contract

`paddock-dialogs` gets `SizeMode`, generated into TypeScript like the kind table:

```rust
pub enum SizeMode {
    /// The window keeps the size it was given (placed, remembered or dragged); only the OS minimum follows the content.
    Manual,
    /// Width fixed (the kind's declared width, or dragged); height follows the content.
    Height { shrink: Shrink },
}
pub enum Shrink { Always, Never }   // Never: a viewer's result does not shrink under the reader (today's grows_only)
```

`KindPolicy` loses `fits_content` and `grows_only` and gains `size_mode`. **The mode is the kind's; a contract cannot
override it** (review round 1, YAGNI). A dialog that needs another behaviour is another kind, which is a decision for an
ADR, not a field.

| Kind | Mode | Notes |
|---|---|---|
| `decision` | `Height { shrink: Always }` | width `narrow` 400 or `medium` 560; remembers only its offset (D3) |
| `form` | `Height { shrink: Always }` | no declared height (unchanged from ADR 0026); remembers only its offset (D3) |
| `viewer` | `Height { shrink: Never }` | opens at `min(content, preferred)`, grows to the cap (D2); remembers only its offset (D3). Its kind floor, `VIEWER_MIN` 680 x 560, is its OS minimum (`min = max(kind floor, measured min content)`), so it never opens below 560 px high: in practice at `max(560, min(content, cap))` (2026-11-04, from the kind table) |
| `tool` | `Manual` | default 960x680, OS minimum = measured minimum content; keeps its remembered size, clamped to the minimum content (the one exception, D3) |

WPF's `Width` and `WidthAndHeight` are not adopted now (see Alternatives); the enum is closed and a variant is added by
a later ADR if a dialog needs it.

Constraints are WPF's Min/Max: `min = max(kind floor, measured min content)`, `max = work area − visible frame` on the
window's current monitor; the 75 % cap (`CAP_FRACTION`) stays as the limit of the *preferred* (content) size and never
pushes below `min` (ADR 0016 decision 4, unchanged).

**A user resize switches the window to `Manual` for the rest of this opening** (WPF; D4, D6). Each opening starts at
its kind's natural size (D3): decisions, forms and viewers remember only their offset (position); their width is the
kind's declared width and their height follows the content (amends ADR 0026 §1 for forms and viewers). A tool
(`Manual`) has no natural size beyond its 960x680 default; it keeps its remembered size, clamped to the minimum content,
as the one exception (user confirmed 2026-11-03).

### 2. One measure loop in the page; the handshake no longer sizes

The DOM is the measurer. `DialogFrame` owns a single `SizeLoop`:

- **What is measured.** `.pd-frame-content` is a natural-height block inside the scrolling body; the measure is header
  + alert + body padding + content + footer (`DialogFrame.measure`, unchanged in substance), in CSS px, at the window's
  current width; before the window is shown, at the pinned contract width (method d).
- **No scrollbar feedback.** `.pd-frame-body` gets `scrollbar-gutter: stable`, so the content's width, and with it its
  wrapping, is the same whether or not the body scrolls. Without that, a window clamped at the cap shows a scrollbar,
  the content re-wraps narrower and taller, the next measure differs, and the loop can oscillate. Where a platform
  ignores `scrollbar-gutter` (WebKitGTK and WKWebView overlay scrollbars take no width, so there is nothing to re-wrap;
  to be confirmed per engine in WP-3), the content is measured at the body's `clientWidth` minus nothing, which is the
  same width either way.
- **When.** Every layout invalidation the frame can see: the ResizeObserver on the content root and the footer, the
  MutationObserver on `<html>` settings attributes, `document.fonts` `loadingdone`, the content's `requestFit()`, and
  a `dialog://remeasure` event from Rust (§4). Coalesced to one send per animation frame once shown. Before the window
  is shown there are no frames, so the loop also runs synchronously when the content is mounted and the fonts are
  ready, and once more after every awaited step of the start (a font that started loading, the `dialog_ready` round
  trip). These are calls of the **same** function, not a second measuring path.
- **Baseline.** The loop's baseline is the last measure *sent*, never one read later (the `08ad47b22` lesson).
- **Convergence invariant.** One content change produces **at most two sends**: the measure of the change, and at most
  one more after Rust's resize re-lays the page out (a rounding of the new size can move one line). A third send without
  a new trigger (content, settings, fonts, `requestFit`, `remeasure`, a user resize) is a loop: the frame stops sending,
  logs `dialog size loop did not settle` with the last three measures (`console.warn`, forwarded to the app log), and
  waits for the next trigger. The browser e2e and the Rust property test both check the invariant (check 2, 4).
- **Ratchets are forbidden in `Height` content.** A content root or a direct child whose height follows the window
  (`vh`, a `%` height, `flex-grow` along the block axis, `height: 100%`) grows with the window and so grows the measure;
  with `shrink: Never` that ratchets the window to the cap. Check 3c rejects them in `decision`, `form` and `viewer`
  content; a `tool` (`Manual`) may use them.
- **Identity of a page.** `dialog_take_input` returns an `epoch` (a counter per window, raised on every page load,
  including a reload by `paddock-startup`'s watchdog). Every measure carries `(epoch, seq)`; a new epoch resets the
  order, so a reloaded page's `seq = 1` is not dropped as stale.
- **Wire** (generated, §3): `dialog_measure { epoch, seq, measure }` replaces both `dialog_ready { measure }` and
  `dialog_fit { measure }`; `dialog_ready { epoch, seq }` says "measure `seq` is the one to show at" and carries no
  size. `Measure` gains `overflow: bool` (the body scrolls: `scrollHeight > clientHeight + 1`), so Rust's log and the
  Windows GUI judge see a scrollbar without reaching into the page (ER-27).

### 3. Rust converts once, at the OS edge, on the main thread, and owns the mode

All sizing of dialog windows happens in `paddock-dialogs`:

- **One pure size state machine** (`size.rs`): `SizeState { mode, epoch, last: Option<(seq, Measure)>, pending:
  VecDeque<Request>, scale, frame, show: ShowState }`, where a `Request` is the physical inner size Rust asked the OS
  for and why. Pure transitions `on_measure`, `on_ready`, `on_resized`, `on_user_resize_started`, `on_scale_changed`,
  `on_moved` each return a `Plan { inner, min, move_to, mode_change, show }`. Today's `plan_fit`, `fit_target`,
  `zoom_after` and `viewport_delta` fold into it; property tests cover it (check 2).
- **The main thread is the only place that sizes.** Tauri runs `async` commands off the main thread and queues their
  window calls; a `set_size` from a command can therefore land after a `Resized` the state machine has not seen yet.
  Commands (`dialog_measure`, `dialog_ready`) only post the transition to the main thread (`run_on_main_thread`), where
  the state machine, the OS calls and the window events are ordered. Each `set_size` pushes a `Request`; a `Resized`
  equal (within 1 physical px per edge) to a pending request pops it and every older one; one that matches none is
  **unexplained**, and is logged with the pending queue.
- **What counts as a user resize** is decided by the OS's own signal, not by the comparison:
  - **Windows:** a `WM_SIZING` (sent only "to a window that the user is resizing",
    <https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-sizing>, read 2026-11-03) or a `WM_SYSCOMMAND` with
    `SC_SIZE` (keyboard size). **Not** `WM_ENTERSIZEMOVE`: it is also sent for a move, which tao already tracks as
    `MARKER_IN_SIZE_MOVE` (tao 0.37.2 `event_loop.rs:969-983`). The hook is a `SetWindowSubclass` procedure on dialog
    windows only, chained after tao's own subclass.
  - **macOS:** `NSWindowWillStartLiveResizeNotification` ("the user is about to resize the window"); a move is not a
    live resize.
  - **Linux X11:** no such signal; an unexplained `Resized` after the configure for every pending request has arrived
    is taken as a user resize.
  - **Linux Wayland:** the compositor may configure the window at any time (tiling, maximise); an unexplained configure
    is a user resize (D6).
  - A user resize sets `Manual`, sends `dialog://size-mode { mode }` (the page sets `data-size-mode`), and logs it. A
    move never changes the mode (check 2 and 5).
- **An edge module per OS** (`edge/windows.rs`, `edge/macos.rs`, `edge/linux.rs`, behind the `tauri` feature), in
  physical px for the window's current monitor:
  - **Windows:** the scale from `GetDpiForWindow`; the frame for a client size from `AdjustWindowRectExForDpi` with the
    window's own style and extended style at that DPI (what `SetWindowPos` needs). That frame **includes the invisible
    resize borders** of Windows 10/11, so the clamp to the work area and `keep_on_monitor` use the **visible** frame:
    `DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)`, "the extended frame bounds rectangle in screen space"
    (<https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute>, updated 2026-03-09),
    minus the client rectangle. The work area comes from `GetMonitorInfoW` (not re-checked today).
  - **macOS:** `frameRectForContentRect:styleMask:` in points (scale-independent), the screen's `visibleFrame` (not
    re-checked today).
  - **Linux X11:** outer minus inner after the window is mapped (GTK client-side decorations give no API), re-read on
    each fit; the monitor's work area from GDK.
  - **Linux Wayland:** a client cannot place a top-level window, so `set_position` does nothing and `keep_on_monitor`
    has no effect; there is no work-area protocol, so the work area is the full monitor. The clamp therefore uses the
    monitor minus a margin (proposed 48 logical px top and bottom, for panels), and the dialog relies on the
    compositor to place it.
  - CSS px become logical px by the page's zoom (`devicePixelRatio / scale`), read fresh with every measure, because
    the page measures again after every scale change (§4); `zoom_after`'s "keep the old zoom" goes.
- **Rounding once.** Logical to physical with `physical_ceil` on the client size only; the frame is added in physical
  px from the edge module, never converted back and forth.
- **Show.** `dialog_ready { epoch, seq }` shows the window only once measure `(epoch, seq)` has been applied and its
  `Resized` has arrived (or matched); if the ready arrives first, the show is held. The existing show deadline (ADR
  0026 §4.3) is the watchdog: at the deadline the window is shown at whatever size it has, with a warning that names
  what was missing (no measure, measure not applied, no `Resized`).
- **Logging (ER-27).** One line per transition: dialog id, window label, `Phase` (`measure`, `ready`, `show`,
  `scale`, `moved`, `resized`, `user-resize`), `Epoch`, `Seq`, `Mode` and any `ModeChange`, content and min (CSS and
  logical), `Zoom`, `Scale`/DPI, frame (full and visible), work area, requested inner logical and physical size,
  pending queue length, read-back size, `Overflow`, and stale or unexplained events. This extends the `Phase` line of
  `08ad47b22`.

### 4. Monitor, DPI and work-area changes

- **Scale change, and why a refit inside the event is lost.** tao 0.37.2's `WM_DPICHANGED` handler
  (`src/platform_impl/windows/event_loop.rs:1837-2068`, read 2026-11-03) sends `ScaleFactorChanged` synchronously and
  then calls `SetWindowPos` itself: on Windows 11 (build ≥ 22000) with the OS's `suggested_rect`, on Windows 10 with a
  rectangle it computes. Any `set_size` made inside the event handler is overwritten by that call. Today's
  `scale_changed` (`tauri.rs:969-983`) refits inside the event, so its result can be lost (a candidate cause in the
  table below). The new design:
  - the edge module answers `WM_GETDPISCALEDSIZE` with the frame size the state machine wants at the new DPI, which
    Microsoft documents as becoming "the candidate rectangle sent to `WM_DPICHANGED`"; on Windows 11 tao then applies
    exactly that (D5);
  - the refit for the new scale is **posted** with `run_on_main_thread` from the `ScaleFactorChanged` handler, so it
    runs after tao's `SetWindowPos`, and is pushed as a `Request` like any other;
  - then Rust emits `dialog://remeasure`; the page's next measure (text may re-wrap at the new DPI) refits through the
    normal path.
  macOS and Linux have no such overwrite in their scale-change paths as far as read (to be confirmed in WP-2); they use
  the same posted refit.
- **Monitor change without a scale change** (dragged to a monitor with a smaller work area): `Moved` re-checks the
  clamp after the move ends (on Windows at `WM_EXITSIZEMOVE`, elsewhere on the next `Moved`); a window taller than the
  work area minus its visible frame is clamped and the body scrolls (the footer stays, `min` being the minimum content).
- **Work-area clamp** on every plan: inner ≤ work area − visible frame; `keep_on_monitor` keeps the window on its
  monitor (not on Wayland, above); the clamp never goes below `min` unless the work area is smaller, which is logged.
- **In `Manual` mode** only `min` and the clamp apply; the content height is not.

### 5. One wire definition

`DialogMeasureWire` (with `epoch`, `seq`, `overflow`), `DialogReadyWire { epoch, seq }`, `DialogStart.epoch` and the
event payloads `DialogSizeModeEvent { mode }` and `DialogRemeasureEvent { scale }` are Rust types exported with `ts-rs`
into `ui/projects/paddock-ui/shell/generated/` (ADR 0026 §3). `io.ts` aliases the generated types and declares none of
its own; the generator's check (`PADDOCK_WRITE_TS`) fails on drift.

### 6. What is removed or frozen

- The handshake measure as a source of truth: `dialog_ready` stops carrying a measure; `DialogShell.ready`'s own
  measure-and-compare (`08ad47b22`) is replaced by the loop.
- `fits_content`, `grows_only`, `is_fitted` and the TypeScript `fitsContent`: replaced by `SizeMode`.
- The viewport delta "taken once" (`viewport_delta_or`): the delta is part of each measure's conversion.
- `UNMEASURED_HEIGHT` as anything but the hidden window's initial size.
- **Debrief's copy is frozen now.** It is registered as copy `dialog-windows` in `docs/known-copies.toml` (line 451,
  canonical `paddock-dialogs`, rule 55 / ADR 0034): from this ADR's acceptance it receives no sizing changes except a
  fix that the rule 55 trailer covers. Moving Debrief's 50 dialogs onto the shared dialogs, and deleting the copy, is
  in Debrief 1.2's plan (ADR 0026 §7), not here.

## The check (makes the class checked, rule 40)

Each check below says which code it exercises; one that only exercises a fake is not counted as checking Rust.

1. **Contract and wire (Rust test).** Every kind has a mode; the generated TypeScript (kind table, wire types, event
   payloads) equals the Rust types (`PADDOCK_WRITE_TS` test extended).
2. **Property tests (Rust, `paddock-dialogs` `size.rs`).** For any scale 1-3 (including 1.25, 1.5, 1.75), work area
   800x600 to 7680x4320, full and visible frames 0-60 px, any measure sequence and any interleaving of `on_measure`,
   `on_resized` (matching or not), `on_user_resize_started`, `on_moved`, `on_scale_changed` and `on_ready`:
   - the planned inner size is ≥ `min`, ≤ work area − visible frame, and its physical size converted back is ≥ the
     logical request and < request + 1 physical px;
   - `Height { shrink: Always }` equals the content when it is between `min` and the cap;
   - only `on_user_resize_started` (or, on X11 and Wayland, an unexplained `Resized` with no pending request) yields
     `Manual`; **a move never does**; a `Resized` that matches a pending request never does;
   - no later `on_measure` changes a `Manual` window's size except to raise it to a new `min`;
   - a lower `seq` in the same epoch changes nothing; a new epoch is accepted;
   - the window is shown only after the ready's measure was applied, or at the deadline;
   - a scale change yields exactly one posted refit and one `remeasure`.
3. **Conformance check** (number assigned when it lands; C61 is the highest on `main`, C67-C73 are taken by ADRs
   0037-0042):
   - (a) no `set_size`/`set_min_size`/`set_position` on a dialog window outside `crates/paddock-dialogs` (scanned in
     files that use `paddock_dialogs` or dialog labels; Debrief's frozen copy waived to its rule 55 tag);
   - (b) no TypeScript calls `dialog_measure`/`dialog_ready` except `shell/dialogs/src/io.ts`, and `io.ts` declares no
     wire type of its own;
   - (c) a component registered as a `decision`, `form` or `viewer` sets no height that follows the window on its host,
     its root or their direct children: no `vh`/`dvh`/`svh` units, no `%` or `100%` `height`/`min-height`, no
     `flex-grow` or `flex: 1` along the block axis, no fixed `px` `height`/`min-height`/`max-height` (the body's minimum
     goes through `dialogMinBodyHeight`);
   - (d) every app with dialogs runs the shared e2e (check 4) over its whole contract (the visited-ids count of ADR 0016
     check 5).
4. **Page-side e2e, every dialog kind** (`@decryp7/paddock-e2e` `expectSizeToContent`; Chromium in branches, WebKit
   nightly). **This exercises the page only** (the loop, the baseline, the epoch, the wrap at a CSS zoom), never Rust's
   conversion: a browser's `deviceScaleFactor` does not reach Rust. The shell demo gets one fixture dialog per kind whose
   input can add content after the first measure (a late list, a long error, a streamed answer). For each: the last
   sent measure after the late content equals a fresh measure; the body does not scroll below the cap; the footer
   buttons are inside the viewport (`elementFromPoint`); one content change caused at most two sends; a reload sends
   with a new epoch. Run at `deviceScaleFactor` 1, 1.25 and 1.5 to catch CSS rounding and re-wrap. Waits are on state
   (`data-fit-seq` equal to the last sent seq), never on time (rule 60).
5. **Rust integration test with the mock runtime** (`paddock-dialogs` `mock-runtime` feature, already used by
   `paddock-shell`): the real `tauri.rs` glue, driven by invoking `dialog_measure`/`dialog_ready` and feeding
   `on_window_event` synthetic `Resized`, `Moved` and `ScaleFactorChanged` events, checks that a user resize stops
   refits, that a move does not, that a reload's new epoch is accepted, and that the scale-change refit is posted, not
   applied inside the event. Whether the mock runtime delivers `run_on_main_thread` closures in order is verified first
   in WP-4; if it does not, the glue takes an injected executor (rule 60 seam).
6. **Real-window checks.**
   - (a) **Tauri WebDriver** (Windows and Linux CI): a shared `tauri-driver` harness, extracted from Debrief's
     `apps/debrief/app/e2e-tauri/` (`wdio.conf.ts`, `fetch-msedgedriver.mjs`) into `@decryp7/paddock-e2e`, run by the
     shell demo: open each fixture dialog, add late content, compare the inner size with the last logged requested
     size, resize with WebDriver *Set Window Rect*, add content, and assert the size stayed and the log shows
     `ModeChange=Manual`. **Unverified:** whether msedgedriver for WebView2 and WebKitWebDriver implement *Set Window
     Rect* for a Tauri window; WP-4b verifies it first. If one does not, that OS's no-snap-back case moves to 6(b) (the
     Windows GUI session) or, on Linux, to an `xdotool` resize in the Linux CI container, and the gap is listed. macOS has
     no WebDriver for WKWebView: macOS is covered by `vm-smoke --gui` (`axdrive`) only, as a release gate, not per
     branch.
   - (b) **ADR 0040's Windows GUI step, refined and extended** (a Dyno 1.1.0 gate, below). For a `Height` dialog the
     expected logical size is the size it last asked for (the app log's last fit line for that window, ER-27), not the
     contract's: at 100 % and 150 % the client rectangle divided by `GetDpiForWindow`/96 equals it within 2 physical px
     per edge, `Overflow` is false unless the request equals the cap, the last focusable element is inside the client
     rectangle. Then: a `SendInput` drag of the bottom border by 80 px, an invoke of the fixture's "add content" button
     through UI Automation, and the client size stays where the drag left it; a `SendInput` drag of the title bar by
     200 px leaves the mode `Height` (a move is not a resize). **New DPI-change step:** with a dialog open, change the
     display scale from 100 % to 150 % (the VM's settings, no sign-out for a per-monitor-aware app), wait for the
     `Phase=scale` and the following `Phase=measure` lines (a state wait under a hang guard), and check the same size
     condition at 150 %.

## Phase log and the Windows 150 % run (placeholder, to be filled from `fix/dialog-sizing`)

> **PLACEHOLDER: findings of the Windows VM run in progress on `fix/dialog-sizing` (`08ad47b22`). The ADR was accepted
> before this section was filled; a cause it shows that the design does not cover reopens the ADR.** For Dyno's `pd.updates` (and at least one decision and one form), at 100 % and
> 150 %, record the `Phase=ready`, `Phase=fit` and `Phase=scale` lines (`Content`, `Height`, `Current`, `Decor`,
> `Delta`, `Scale`, `Zoom`, `Resized`) and the client rectangle, `GetWindowRect`, `DWMWA_EXTENDED_FRAME_BOUNDS` and
> `GetDpiForWindow` read through UI Automation and Win32.

| Candidate cause | Rules it in | Rules it out | Result |
|---|---|---|---|
| Handshake race (content during `dialog_ready`) | `Phase=fit` for the same dialog after `Phase=ready`, with a larger `Content` | one `Phase=ready` line and no later fit, scrollbar still there | _pending_ |
| DPI or frame rounding | `Current`+`Decor` ≠ outer size, or client height ≠ requested × DPI/96 by > 1 px | both match at 150 % | _pending_ |
| Viewport delta from the hidden window | `Delta` ≠ 0 at `Phase=ready` | `Delta` = 0 | _pending_ |
| Fonts not settled at the first measure | `Content` at ready < `Content` at a later fit with no content change | equal | _pending_ |
| Scale-change refit overwritten by tao's own `SetWindowPos` after `ScaleFactorChanged` (tao 0.37.2 `event_loop.rs:1837-2068`; review round 1) | a `Phase=scale` line whose `Height` differs from the client height read afterwards | the client height equals the scale line's request, or no `Phase=scale` line (the window never changed DPI) | _pending_ |
| Window created hidden at one DPI and placed on a monitor with another, then rescaled by `WM_DPICHANGED` after the first fit (unverified hypothesis) | a `Phase=scale` line after `Phase=ready` | no scale line | _pending_ |
| Frame read includes Windows 11 invisible resize borders (`GetWindowRect` vs `DWMWA_EXTENDED_FRAME_BOUNDS`; unverified hypothesis) | `Decor` width ≈ 14-16 logical px at 100 % while the visible border is 1-2 px | `Decor` equals the visible frame | _pending_ |
| Stale zoom after a scale change (`zoom_after` keeps the old zoom; unverified hypothesis) | `Zoom` ≠ `devicePixelRatio/Scale` at the last fit | equal | _pending_ |

The confirmed cause is fixed as an instance in Dyno 1.1.0 (WP-0), with its failing test first (rule 5 / ER-11) and
confirmed by a run without it (ER-28). The row then names where this design makes the cause impossible or checked (§3
edge, §4, check 2, 5 or 6b). A cause this design does not cover reopens the ADR for revision.

## Consequences

- One mechanism with one baseline, on one thread, decides a dialog's size at every moment; late content, fonts,
  density, theme and DPI are all "the measure changed", not special cases.
- A user who resizes a dialog keeps that size for the opening; moving it changes nothing; a dialog that has not been
  touched follows its content.
- Windows gets its frame and DPI from the OS APIs meant for it, with the visible frame for placement; macOS and Linux
  get the closest equivalents, and Wayland's limits are stated instead of assumed away. The edge module is the only
  platform-specific sizing code and is checked on each OS's runner.
- The wire changes (`dialog_measure`, `dialog_ready { epoch, seq }`, two events, all generated): every app with shared
  dialogs changes in the same change (rule 43; no compatibility shim before v1).
- More fit lines in the log at info level; they are short and per transition, not per frame.
- Risk: the X11 and Wayland user-resize rule depends on matching configures to requests; a false `Manual` only stops
  automatic sizing for that opening (the minimum still holds), and every unexplained `Resized` is logged with the
  pending queue so a false one can be diagnosed from a user's bundle.
- Risk: a second subclass on dialog windows beside tao's. Mitigation: `SetWindowSubclass` chains; it handles three
  messages and passes everything on; covered by check 6b.
- Risk: `scrollbar-gutter: stable` reserves a gutter in dialogs that never scroll (a few px on Windows' classic
  scrollbars). Accepted; the alternative is the oscillation above.

## Migration (every app with dialogs, read at `a18e18801`)

| App | Dialogs today | What changes |
|---|---|---|
| Dyno | shared dialogs (`paddock-dialogs`, `@decryp7/paddock-ui/shell/dialogs`): 37 registered across Dyno and shared `paddock-ui` contributions (8 decision, 13 form, 6 viewer, 10 tool by `kind:` count) | in 1.1.0: modes from kinds; Updates (`pd.updates`) and the late-status e2e move to `expectSizeToContent`; check 6b at 100 %, 150 % and a DPI change is a 1.1.0 gate |
| shell-demo, workbench-demo | shared dialogs (ADR 0016: 16 and 2 contract entries) | the per-kind fixture dialogs and the shared harnesses (checks 4 and 6a) live in the shell demo |
| Debrief | its own copy (`dialog_windows.rs`, `dialog_placement.rs`, `dialog-windows/`; 50 dialogs per ADR 0016), copy `dialog-windows` in `known-copies.toml` | frozen now (§6); adopts the shared dialogs on this model in 1.2's plan; its five fixed-height forms become `Height` forms or tools there |
| Pitwall | no dialog windows on `main` (in-page confirms; its own windows in `shell.rs`) | adopts on the shared shell in 1.1, once, on this model |
| Spotter, Pitcrew | no dialog windows on `main` (tray windows only) | adopt when they move onto the shared shell; nothing to migrate now |
| Pacenotes, Sighter | no code | start on this model |

The shared `paddock-ui` dialogs (`pd.about`, `pd.updates`, `pd.settings`, `pd.setup`, `pd.confirm`, the channel, MCP,
storage and workbench layout dialogs) take their kinds' modes.

## Work packages

Agent-days are for one agent, excluding review and CI waiting.

| WP | Scope | Owner paths | Agent-days | Depends on |
|---|---|---|---|---|
| WP-0 | Fill the placeholder from the VM run; the confirmed instance fix, with its failing test, lands in Dyno 1.1.0 | this ADR; `fix/dialog-sizing` | 0.5 | the VM run |
| WP-1 | `SizeMode` in the kind table; the pure `SizeState` (pending queue, epoch, show state, both modes); generated wire and event types; property tests (check 2); contract and wire test (check 1) | `crates/paddock-dialogs/src/{contract,window,size,ts,generated}.rs`, `ui/projects/paddock-ui/shell/generated/` | 3 | – |
| WP-2 | Main-thread glue (`run_on_main_thread` for commands and the posted scale refit), edge modules: Windows subclass (`WM_SIZING`, `SC_SIZE`, `WM_GETDPISCALEDSIZE`, `WM_EXITSIZEMOVE`), `AdjustWindowRectExForDpi`, `DWMWA_EXTENDED_FRAME_BOUNDS`, `GetMonitorInfoW`; macOS live-resize observer and `frameRectForContentRect`; Linux X11 and Wayland rules; events; fit log (§3) | `crates/paddock-dialogs/src/tauri.rs`, `src/edge/*` | 4 | WP-1 |
| WP-3 | `SizeLoop` in `DialogFrame` (one function, last-sent baseline, epoch and seq, `overflow`, the two-send invariant and its stop), `scrollbar-gutter` per engine, `DialogShell` start without its own measure, `io.ts` on generated types and events, `data-fit-seq`/`data-size-mode` | `ui/projects/paddock-ui/shell/dialogs/src/*` | 2 | WP-1 |
| WP-4 | Conformance check 3a-d; mock-runtime integration test (check 5, incl. the ordering check of `run_on_main_thread`); `expectSizeToContent` and the per-kind fixture dialogs in the shell demo (check 4) | `conformance/`, `crates/paddock-dialogs/tests/`, `ui/projects/paddock-e2e/src/`, `apps/shell-demo/`, `ui/e2e/` | 3 | WP-2, WP-3 |
| WP-4b | Shared `tauri-driver` harness extracted from Debrief's `e2e-tauri` into `paddock-e2e`, run by the shell demo on Windows and Linux CI; first verify *Set Window Rect* on msedgedriver (WebView2) and WebKitWebDriver; check 6a, or its listed fallback | `ui/projects/paddock-e2e/src/tauri/`, `apps/shell-demo/e2e-tauri/`, the shell demo's CI leg | 3 | WP-4 |
| WP-5 | ADR 0040 dialog step refinement and the DPI-change step (check 6b): judge reads the fit log; `SendInput` border and title-bar drags; UIA invoke; scale change with a dialog open | `scripts/vm-smoke/` (through ADR 0042's owner of that harness) | 2 | WP-4, ADR 0040's Windows GUI packages |
| WP-6 | Migration: Dyno and the shared dialogs (e2e switched, late-status spec kept as a case), workbench-demo; Debrief copy frozen (a `known-copies.toml` note); checklist lines in Pitwall 1.1's and Debrief 1.2's plans | `apps/dyno/app/`, `apps/workbench-demo/`, `ui/projects/paddock-ui/**/*.contributions.ts`, `docs/known-copies.toml`, `docs/plans/` | 1.5 | WP-3, WP-4 |
| WP-7 | Docs: rule 17 gains "sized by its kind's size mode (ADR 0043)"; ADR 0016 and 0026 get "amended by 0043" notes; lesson; design guide dialog section; CHANGELOG | `docs/`, `CHANGELOG.md` | 0.5 | acceptance |

**Total: about 19.5 agent-days** (19 without WP-0; the review estimated 18-21). With two agents after WP-1 (WP-2 with
WP-3; then WP-4; then WP-4b with WP-6; WP-5 when ADR 0040's packages are in) about 10-11 working days.

**Placement: inside Dyno 1.1.0 (rule 1, one release at a time).** There is no separate Dyno patch for report 7: WP-0's
instance fix goes into 1.1.0 with the rest. WP-1 to WP-6 land **early in Dyno 1.1.0's development**, as one shared
change with Dyno's adoption (rule 43), so that the shared `crates/` and `ui/` change is not in flight while another app
is in its release window. ADR 0040's 150 % dialog step and the new DPI-change step (check 6b, WP-5) become **Dyno 1.1.0
release gates**. Pitwall 1.1 and Debrief 1.2 come after and adopt the shared dialogs once, on this model.

## Decisions of the user (2026-11-03, relayed by the coordinator)

The user accepted the reviewer's answers to the open decisions. The numbers stay as first published.

- **D1. No `WidthAndHeight` mode** until a dialog needs it; a later ADR adds it then.
- **D2. Viewers never shrink** (`Height { shrink: Never }`), opening at `min(content, preferred)` and growing to the
  cap, with the viewport-content guard: check 3c rejects content whose height follows the window, so the window cannot
  ratchet itself to the cap.
- **D3. Each opening starts at its kind's natural size:** decisions, forms and viewers remember only their offset
  (position); their width is the kind's declared width and their height follows the content (**amends ADR 0026 §1**
  for forms and viewers). A tool (`Manual`) has no natural size beyond its 960x680 default; it keeps its remembered
  size, clamped to the minimum content, as the one exception (user confirmed 2026-11-03). A user resize to `Manual`
  lasts for that opening only.
- **D4. Only a real resize switches to `Manual`:** `WM_SIZING` or `SC_SIZE` on Windows, live resize on macOS, an
  unexplained configure on X11 and Wayland (§3); a move never does.
- **D5. Answer `WM_GETDPISCALEDSIZE`, and also defer the refit:** the posted refit after `ScaleFactorChanged` runs on
  every OS and corrects Windows 10, where tao computes its own rectangle.
- **D6. Windows Snap, a tiling window manager and a Wayland compositor's configure count as a user resize.**

## Alternatives considered

- **Keep the handshake and harden it further** (`08ad47b22` and successors). Each hardening closes one gap between
  two baselines; nothing stops the next asynchronous step from opening another. Rejected as the design; its instance
  fix ships in Dyno 1.1.0 (WP-0).
- **WPF's `Width` and `WidthAndHeight`** (TaskDialog's ideal width). Attractive for short confirms, but the family's
  three width steps (user, 2026-10-28) already give consistent dialogs, and measuring max-content width adds a second
  layout read and a new failure mode (long unbreakable text). Not adopted now (review round 1).
- **Per-contract mode overrides.** No dialog needs one today; an override is a second way to say what the kind says.
  Not adopted (review round 1).
- **AutoScaleMode-style scaling of declared sizes by DPI.** This is what fixed heights did, and ADR 0016 cause 1 showed
  it breaking at 125-150 % and on small screens. Rejected.
- **Let the webview report a preferred size to the OS.** Neither WebView2, WKWebView nor WebKitGTK reports one to Tauri
  (ADR 0016); the measurement has to be ours.
- **Make every dialog `Manual` with a fixed default and a scrolling body.** Simple and robust, but brings back the
  "gap below the footer" and "scrollbar beside one line" reports for decisions and forms. Rejected for those kinds; it
  is the right mode for tools.
- **Size in TypeScript (`getCurrentWindow().setSize`).** Moves the DPI and frame conversion into the page, which cannot
  see the frame or the target monitor's DPI before a move, and breaks "Rust owns the window" (ADR 0026 §3). Rejected.
- **Tell a user resize from our own by size comparison alone** (the first draft). Async commands, late Linux
  configures and moves (`WM_ENTERSIZEMOVE` fires for both) make it misfire; the OS's own resize signals are used where
  they exist and the comparison only where none does (review round 1).
