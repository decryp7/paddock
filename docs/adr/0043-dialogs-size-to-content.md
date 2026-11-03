# 43. Dialogs size to their content: a declared size mode, a continuous measure loop, logical units converted once at the OS edge

- Status: Proposed (2026-11-03). Asked for by the user on 2026-11-03 ("should Paddock dialogs use the same design as
  WPF's `Window.SizeToContent`?"). Not reviewed yet; needs an independent review (rule 22 / ER-23 for its figures, rule
  40 / ER-21 for the redesign) and the user's answers to the open decisions below.
- Date: 2026-11-03
- Related: rule 17 (every dialog is a real OS window), rule 40 / ER-21 (recurring bugs mean a missing design rule),
  rule 55 / ADR 0034 (one implementation per mechanism), rule 56 / ER-27 (logs explain every failure), rule 57 / ER-28
  (a cause is confirmed only by a run without it), rule 60 (ADR 0041: tests control time); ADR 0009 (dialog kinds and
  contracts), ADR 0016 (dialog window sizing contract; this ADR **amends its decisions 1, 5 and 6**), ADR 0026 (the
  shared dialog API; this ADR **amends §1's height column and §4.2, §4.3 and §4.5**), ADR 0040 (Windows GUI test
  session; this ADR **refines its "Dialogs at two scales" step**), ADR 0022 G.7 (no larger `crates/` change inside an
  app's release window).
- Evidence read at `origin/main` `a18e18801` and on `origin/fix/dialog-sizing` `08ad47b22`, 2026-11-03.

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

### 1. A declared size mode per dialog, in the Rust contract

`paddock-dialogs` gets `SizeMode`, generated into TypeScript like the kind table:

```rust
pub enum SizeMode {
    /// The window keeps the size it was given (placed, remembered or dragged); only the OS minimum follows the content.
    Manual,
    /// Width fixed (declared, then remembered or dragged); height follows the content.
    Height { shrink: Shrink },
    /// Width follows the content's max-content width within [min_width, max_width]; height follows the content.
    WidthAndHeight { shrink: Shrink },
}
pub enum Shrink { Always, Never }   // Never: a viewer's result does not shrink under the reader (today's grows_only)
```

`KindPolicy` loses `fits_content` and `grows_only` and gains `size_mode`; a contract may override it only within what
its kind allows (validated where the table is parsed, a `ContractError` otherwise):

| Kind | Default mode | Allowed | Notes |
|---|---|---|---|
| `decision` | `Height { shrink: Always }` | `Height`, `WidthAndHeight` (D1) | width `narrow` 400 or `medium` 560 |
| `form` | `Height { shrink: Always }` | `Height` | no declared height (unchanged from ADR 0026) |
| `viewer` | `Height { shrink: Never }` | `Height`, `Manual` | starts at its preferred height when content is shorter (D2) |
| `tool` | `Manual` | `Manual` | preferred 960x680, OS minimum = measured minimum content |

Constraints are WPF's Min/Max: `min = max(kind floor, measured min content)`, `max = work area − frame` on the
window's current monitor; the 75 % cap (`CAP_FRACTION`) stays as the limit of the *preferred* (content) size and never
pushes below `min` (ADR 0016 decision 4, unchanged).

**User resize switches to `Manual` for the rest of this opening** (WPF). The remembered state stays as ADR 0026 §1
says (a decision remembers its offset, a form its width, a viewer and a tool their size); a dialog opens in its
declared mode every time (D3).

### 2. One measure loop in the page; the handshake no longer sizes

The DOM is the measurer. `DialogFrame` owns a single `SizeLoop`:

- **What is measured.** `.pd-frame-content` is a natural-height block inside the scrolling body; the measure is header
  + alert + body padding + content + footer (`DialogFrame.measure`, unchanged in substance), in CSS px. In `Height`
  mode the frame is laid out at the window's current width; before the window is shown, at the pinned contract width
  (method d). In `WidthAndHeight` mode a second synchronous read lays the content out at `width: max-content` clamped to
  `[min_width, max_width]` and then measures the height at that width.
- **When.** Every layout invalidation the frame can see: the ResizeObserver on the content root and the footer, the
  MutationObserver on `<html>` settings attributes, `document.fonts` `loadingdone`, the content's `requestFit()`, and
  a `dialog://remeasure` event from Rust (§4). Coalesced to one send per animation frame once shown. Before the window
  is shown there are no frames, so the loop also runs once synchronously when the content is mounted and the fonts are
  ready, and once more after every awaited step of the start (a font that started loading, the `dialog_ready` round
  trip). These are calls of the **same** function, not a second measuring path.
- **Baseline.** The loop's baseline is the last measure *sent*, never one read later (the `08ad47b22` lesson). Every
  send carries a sequence number.
- **Wire.** One command, `dialog_measure { seq, measure }`, replaces both `dialog_ready { measure }` and
  `dialog_fit { measure }`. `dialog_ready { seq }` stays only as "the page has sent measure `seq`; show me": it carries
  no size. Rust applies the measure with the highest `seq` it has received, whatever order the invokes arrive in, and
  logs a lower one as stale and drops it.
- `Measure` gains `overflow: bool` (the body scrolls: `scrollHeight > clientHeight + 1`), so Rust's log and the
  Windows GUI judge can see a scrollbar without reaching into the page (ER-27).

### 3. Rust converts once, at the OS edge, and owns the mode

All sizing of dialog windows happens in `paddock-dialogs`:

- **A pure size state machine** in `window.rs` (or a new `size.rs`): `SizeState { mode, last: Option<(seq, Measure)>,
  applied: Option<PhysicalSize>, scale, frame }` with pure transitions `on_measure`, `on_resized`, `on_scale_changed`,
  `on_monitor_changed`, each returning a `Plan { inner: LogicalSize, min: LogicalSize, move_to, mode_change }`. Today's
  `plan_fit`, `fit_target`, `zoom_after` and `viewport_delta` fold into it; property tests cover it as ADR 0016's check
  3 required.
- **An edge module per OS** (`edge/windows.rs`, `edge/macos.rs`, `edge/linux.rs`, behind the `tauri` feature) answers
  three questions in physical px for the window's current monitor: the frame for a given client size and DPI, the work
  area, and the scale. Windows: `GetDpiForWindow` and `AdjustWindowRectExForDpi` with the window's own style and
  extended style (computed for the target DPI, not read off a hidden window), the monitor's work area from
  `GetMonitorInfoW` (not re-checked today). macOS: `frameRectForContentRect:styleMask:` in points (scale-independent), `visibleFrame` of the
  screen (not re-checked today). Linux: outer minus inner after the window is mapped (GTK's client-side decorations give no API for it), and the
  monitor workarea from GDK; the frame is re-read on each fit. CSS px become logical px by the page's zoom
  (`devicePixelRatio / scale`), read fresh with every measure because the page now re-measures after a scale change
  (§4), so `zoom_after`'s "keep the old zoom" rule goes.
- **Rounding once.** Logical to physical with `physical_ceil` on the client size only; the frame is added in physical
  px from the edge module, never converted back and forth.
- **Telling a user resize from our own.** After every `set_size`, Rust reads the inner size back and records it as
  `applied`. A `Resized` whose size differs from `applied` by more than 1 physical px on either edge, outside a pending
  scale change, is an external resize: the mode becomes `Manual`, the page gets `dialog://size-mode { mode }` (it sets
  `data-size-mode` on `<html>` for tests), and the log says so. On Windows the edge module also subclasses the window
  for `WM_ENTERSIZEMOVE`/`WM_EXITSIZEMOVE` and on macOS observes `NSWindowWillStartLiveResizeNotification`, so a drag is
  known for certain even when it ends at the applied size; on Linux the read-back comparison is the only signal. A
  window-manager resize (Windows Snap, a tiling WM) counts as a user resize, which is what the user asked for.
- **Logging (ER-27).** One line per transition: dialog id, window label, `Phase` (`measure`, `ready`, `scale`,
  `monitor`, `resized`), `Seq`, `Mode` and any `ModeChange`, content and min (CSS and logical), `Zoom`, `Scale`/DPI,
  frame, work area, requested inner logical and physical size, read-back size, `Overflow`, and whether a stale measure
  was dropped. This extends the `Phase` line of `08ad47b22`.

### 4. Monitor, DPI and work-area changes

- **Scale change** (`ScaleFactorChanged`; on Windows `WM_DPICHANGED`): Rust first refits at once from the last logical
  measure for the new scale and frame (so the window is never a frame at the old size), then emits
  `dialog://remeasure`; the page's next measure (text may re-wrap at the new DPI) refits again through the normal path.
  On Windows the edge module answers `WM_GETDPISCALEDSIZE` with the size the state machine wants for the new DPI, so
  Windows' linear suggestion is never applied first and then corrected (D5).
- **Monitor change without a scale change** (dragged to a monitor with a smaller work area): the next `Moved` re-checks
  the work-area clamp; a window taller than the new work area minus its frame is clamped and the body scrolls (the
  footer stays, because `min` is the minimum content).
- **Work-area clamp** on every plan: inner ≤ work area − frame; `keep_on_monitor` keeps the window on its monitor
  (unchanged); the clamp never goes below `min` unless the work area itself is smaller, which is logged.
- **In `Manual` mode** only `min` and the clamp still apply; the content height is not applied.

### 5. What is removed

- The handshake measure as a source of truth: `dialog_ready` stops carrying a measure; `DialogShell.ready`'s own
  measure-and-compare (`08ad47b22`) is replaced by the loop.
- `fits_content`, `grows_only`, `is_fitted` and the TypeScript `fitsContent`: replaced by `SizeMode`.
- The viewport delta "taken once" (`viewport_delta_or`): the delta is part of each measure's conversion.
- `UNMEASURED_HEIGHT` as anything but the hidden window's initial size.
- Debrief's copy (`dialog_windows.rs`, `dialog_placement.rs`, `dialog-windows/kit/`) when Debrief 1.2 adopts the shared
  dialogs (ADR 0026 §7; registered as copy `dialog-windows` in `docs/known-copies.toml`, rule 55).

## The check (makes the class checked, rule 40)

1. **Contract validation (Rust test).** Every table entry's mode is allowed for its kind; the generated TypeScript
   equals the Rust table (the existing `PADDOCK_WRITE_TS` test extended).
2. **Property tests (Rust, `paddock-dialogs`).** For any scale 1-3 (including 1.25, 1.5, 1.75), work area 800x600 to
   7680x4320, frame 0-60 px, any measure and any mode: the planned inner size is ≥ `min`, ≤ work area − frame, its
   physical size converted back is ≥ the logical request and < request + 1 physical px; `Height { shrink: Always }`
   equals the content when it is between min and cap; a `Resized` to anything but `applied` yields `Manual`, and no
   later `on_measure` changes the size of a `Manual` window except to raise it to a new `min`; a stale `seq` never
   changes anything.
3. **Conformance check** (number assigned when it lands; C61 is the highest on `main`, C67-C73 are taken by ADRs
   0037-0042): (a) no `set_size`/`set_min_size`/`set_position` on a dialog window outside `crates/paddock-dialogs`
   (scanned in files that use `paddock_dialogs` or dialog labels; Debrief's registered copy is waived to its rule 55
   tag); (b) no TypeScript calls `dialog_measure`/`dialog_ready` except `shell/dialogs/src/io.ts`; (c) a component
   registered as a dialog sets no fixed `height`/`min-height`/`max-height` in px on its host or root element (the
   body's declared minimum goes through `dialogMinBodyHeight`); (d) every app with dialogs runs the shared e2e below
   over its whole contract (the visited-ids count of ADR 0016 check 5).
4. **Shared e2e, every dialog kind** (`@decryp7/paddock-e2e` `expectSizeToContent`, run by the shell demo and every app
   over its contract; Chromium in branches, WebKit nightly): the shell demo gets one fixture dialog per kind and mode
   whose input can add content after the first measure (a late list, a long error, a streamed answer). For each: (i)
   the last sent measure after the late content equals a fresh measure, the body does not scroll below the cap, and
   the footer buttons are inside the viewport (`elementFromPoint`); (ii) after a simulated user resize (the io fake
   reports an external `Resized`) the mode is `Manual` and further late content sends measures but the fake window
   keeps its size; (iii) at `deviceScaleFactor` 1, 1.25 and 1.5. Waits are on state (`data-fit-seq` equal to the last
   sent seq, `data-size-mode`), never on time (rule 60).
5. **Real-window checks.** (a) The Tauri e2e (WebDriver, Windows and Linux CI) opens each fixture dialog, triggers late
   content, compares the window's inner size with the last logged requested size, resizes the window with WebDriver
   `Set Window Rect` (an external resize), triggers more content, and asserts the size did not change and the log shows
   `ModeChange=Manual`. (b) **ADR 0040's "Dialogs at two scales" step is refined:** for a `Height` or `WidthAndHeight`
   dialog the expected logical size is the size the dialog last asked for (the app log's last fit line for that window,
   ER-27), not the contract's; the window's client rectangle divided by `GetDpiForWindow`/96 equals it within 2
   physical px per edge at 100 % and 150 %; `Overflow` is false unless the request equals the cap; the last focusable
   element is inside the client rectangle. A second sub-step drags the bottom border by 80 px with `SendInput`, invokes
   the fixture's "add content" button through UI Automation, and asserts the client size stayed where the drag left it.

## Phase log and the Windows 150 % run (placeholder, to be filled from `fix/dialog-sizing`)

> **PLACEHOLDER: findings of the Windows VM run in progress on `fix/dialog-sizing` (`08ad47b22`). Do not accept this
> ADR before this section is filled.** For Dyno's `pd.updates` (and at least one decision and one form), at 100 % and
> 150 %, record the `Phase=ready` and `Phase=fit` lines (`Content`, `Height`, `Current`, `Decor`, `Delta`, `Scale`,
> `Zoom`, `Resized`) and the client rectangle and `GetDpiForWindow` read through UI Automation.

| Candidate cause | Rules it in | Rules it out | Result |
|---|---|---|---|
| Handshake race (content during `dialog_ready`) | `Phase=fit` for the same dialog after `Phase=ready`, with a larger `Content` | one `Phase=ready` line and no later fit, scrollbar still there | _pending_ |
| DPI or frame rounding | `Current`+`Decor` ≠ outer size, or client height ≠ requested × DPI/96 by > 1 px | both match at 150 % | _pending_ |
| Viewport delta from the hidden window | `Delta` ≠ 0 at `Phase=ready` | `Delta` = 0 | _pending_ |
| Fonts not settled at the first measure | `Content` at ready < `Content` at a later fit with no content change | equal | _pending_ |
| Frame read includes Windows 11 invisible resize borders (unverified hypothesis, this ADR) | `Decor` width ≈ 14-16 logical px at 100 % while the visible border is 1-2 px | `Decor` matches `AdjustWindowRectExForDpi` | _pending_ |
| Window created at one DPI, placed on a monitor with another, then linearly rescaled by `WM_DPICHANGED` after the fit (unverified hypothesis, this ADR) | a `Phase=scale` line after `Phase=ready` with a changed `Height` | no scale line | _pending_ |
| Stale zoom after a scale change (`zoom_after` keeps the old zoom) (unverified hypothesis, this ADR) | `Zoom` ≠ `devicePixelRatio/Scale` at the last fit | equal | _pending_ |

Whatever the run shows, the confirmed cause is fixed as an instance on `fix/dialog-sizing` for the next Dyno patch
(with its failing test first, rule 5 / ER-11, and confirmed by a run without it, ER-28); this ADR's design then has to
make that cause impossible or checked, and the row above names where (§3 edge, §4, or check 5b). If a cause is found
that this design does not cover, the ADR is revised before acceptance.

## Consequences

- One mechanism with one baseline decides a dialog's size at every moment; late content, fonts, density, theme and
  DPI are all "the measure changed", not special cases.
- A user who resizes a dialog keeps that size for the opening; a dialog that has not been touched follows its content.
- Windows gets its frame and DPI from the OS APIs meant for it, at the window's own monitor; macOS and Linux get the
  closest equivalents. The edge module is the only platform-specific sizing code and is checked on each OS's runner.
- The wire changes (`dialog_measure`, `dialog_ready { seq }`, two events): every app with shared dialogs changes in the
  same change (rule 43; no compatibility shim before v1).
- More fit lines in the log at info level; they are short and per transition, not per frame.
- Risk: `WidthAndHeight` measuring max-content can make a dialog wide for long unbreakable text; mitigated by
  `max_width` and kept off by default (D1).
- Risk: the read-back comparison on Linux can mistake a WM adjustment of our own request for a user resize. Mitigation:
  read back after the configure is acknowledged and compare with 1 physical px tolerance; the property test covers the
  rule; a false `Manual` only stops automatic sizing for that opening (the minimum still holds).
- Risk: subclassing the Windows window procedure beside tao's. Mitigation: `SetWindowSubclass` (comctl32), which chains
  safely, only for dialog windows; covered by the Windows GUI check.

## Migration (every app with dialogs, read at `a18e18801`)

| App | Dialogs today | What changes |
|---|---|---|
| Dyno | shared dialogs (`paddock-dialogs`, `@decryp7/paddock-ui/shell/dialogs`): 37 registered across Dyno and shared `paddock-ui` contributions (8 decision, 13 form, 6 viewer, 10 tool by `kind:` count) | modes come from kinds; Updates (`pd.updates`) and the late-status e2e move to `expectSizeToContent`; Windows GUI check at 100/150 % |
| shell-demo, workbench-demo | shared dialogs (ADR 0016: 16 and 2 contract entries) | gain the per-kind fixture dialogs; run the shared e2e |
| Debrief | its own copy (`dialog_windows.rs`, `dialog_placement.rs`, `dialog-windows/`; 50 dialogs per ADR 0016) | adopts the shared dialogs in 1.2 (ADR 0026 §7) directly on this model; its five fixed-height forms become `Height` forms or tools, decided per dialog there; the copy is deleted |
| Pitwall | no dialog windows on `main` (in-page confirms; its own windows in `shell.rs`) | adopts on the shared shell in 1.1, directly on this model |
| Spotter, Pitcrew | no dialog windows on `main` (tray windows only) | adopt when they move onto the shared shell; nothing to migrate now |
| Pacenotes, Sighter | no code | start on this model |

The shared `paddock-ui` dialogs (`pd.about`, `pd.updates`, `pd.settings`, `pd.setup`, `pd.confirm`, the channel, MCP,
storage and workbench layout dialogs) take their kinds' modes; none declares an override.

## Work packages

Agent-days are for one agent, excluding review and CI waiting.

| WP | Scope | Owner paths | Agent-days | Depends on |
|---|---|---|---|---|
| WP-0 | Fill the placeholder from the VM run; land the confirmed instance fix for the Dyno patch on `fix/dialog-sizing` (not part of this redesign) | this ADR; `fix/dialog-sizing` | 0.5 | the VM run |
| WP-1 | `SizeMode` in the contract and kind table, generated TypeScript, contract validation; the pure `SizeState` machine replacing `plan_fit`/`fit_target`/`zoom_after`; property tests (check 2) | `crates/paddock-dialogs/src/{contract,window,size,ts,generated}.rs`, `ui/projects/paddock-ui/shell/generated/dialogs.ts` | 2.5 | – |
| WP-2 | Edge modules: Windows (`GetDpiForWindow`, `AdjustWindowRectExForDpi`, `GetMonitorInfoW`, subclass for `WM_ENTERSIZEMOVE`/`WM_GETDPISCALEDSIZE`), macOS (`frameRectForContentRect`, live-resize notification), Linux (read-back); read-back and `applied`; events `dialog://size-mode`, `dialog://remeasure`; `dialog_measure`/`dialog_ready { seq }`; fit log (§3) | `crates/paddock-dialogs/src/tauri.rs`, `src/edge/*` | 3 | WP-1 |
| WP-3 | `SizeLoop` in `DialogFrame` (one function, last-sent baseline, `seq`, `overflow`, `WidthAndHeight` read), `DialogShell` start without its own measure, io (`dialog_measure`, events), browser io fake reporting external resizes, `data-fit-seq`/`data-size-mode` | `ui/projects/paddock-ui/shell/dialogs/src/*` | 2 | WP-1 (wire types) |
| WP-4 | Checks: conformance check (3a-d), `expectSizeToContent` in `paddock-e2e`, per-kind fixture dialogs in the shell demo, Tauri e2e real-window case (5a) | `conformance/`, `ui/projects/paddock-e2e/src/`, `apps/shell-demo/`, `ui/e2e/` | 2.5 | WP-2, WP-3 |
| WP-5 | ADR 0040 dialog step refinement (5b): judge reads the fit log, `SendInput` border drag, UIA invoke of the fixture button, at 100 % and 150 % | `scripts/vm-smoke/` (per ADR 0042's owner of that harness) | 1.5 | WP-4, ADR 0040's Windows GUI WPs |
| WP-6 | Migration: Dyno and the shared dialogs (e2e switched, late-status spec kept as a case), workbench-demo; Pitwall 1.1 and Debrief 1.2 adopt in their own plans (no extra package here, a checklist line each) | `apps/dyno/app/`, `apps/workbench-demo/`, `ui/projects/paddock-ui/**/*.contributions.ts` | 1.5 | WP-3, WP-4 |
| WP-7 | Docs: rule 17 gains "sized by its declared size mode (ADR 0043)"; ADR 0016 and 0026 get "amended by 0043" notes; lesson; design guide dialog section; CHANGELOG | `docs/`, `CHANGELOG.md` | 0.5 | acceptance |

**Total: about 14 agent-days** (13.5 without WP-0), about 6-7 calendar days with two agents in parallel after WP-1
(WP-2 and WP-3 in parallel, then WP-4, then WP-5 and WP-6 in parallel).

**Release-window placement (ADR 0022 G.7, rule 1).** WP-1 to WP-4 and WP-6 change `crates/` and `ui/`, which every
app's release builds from, so they land **outside any app's release window**: after the Dyno patch release that ships
WP-0's instance fix, and **before Pitwall 1.1's window opens**, so that Pitwall 1.1 and Debrief 1.2 adopt the shared
dialogs once, on this model, instead of migrating twice. WP-0 is an instance fix on Dyno's release path and follows
Dyno's own release rules. WP-5 lives in `vm-smoke` (not on an app's build path) and can land whenever ADR 0040's
packages allow. If Pitwall 1.1's window opens first, WP-1 to WP-4 wait until its tag, and Pitwall adopts afterwards in
a patch.

## Open decisions (with recommendations)

- **D1. Use `WidthAndHeight` at all?** Recommendation: define it, allow it for `decision` only, and use it for none in
  this change (all decisions stay `narrow`/`medium` with `Height`). TaskDialog's ideal-width behaviour is attractive for
  short confirms, but our three width steps (user, 2026-10-28) already give consistent dialogs. Revisit if a decision
  is reported too wide or too narrow.
- **D2. Viewers: keep "never shrink"?** Recommendation: yes, `Height { shrink: Never }`, opening at
  `min(content, preferred)` so short content leaves no band (ADR 0026 §1), and growing to the cap. A streamed answer
  must not jump under the reader.
- **D3. Does a user resize persist into the next opening as `Manual`?** Recommendation: no. The opening starts in the
  declared mode (WPF does not persist it either); what is remembered stays as ADR 0026 §1 (forms: width; viewers and
  tools: size, clamped to the minimum content).
- **D4. Does a width-only drag of a `Height` dialog switch to `Manual`?** Recommendation: yes, as WPF does: any user
  resize means "I want this size". The alternative (stay in `Height` and refit the height to the new width) is cleverer
  but surprises users who also dragged a corner by a few pixels.
- **D5. Answer `WM_GETDPISCALEDSIZE` on Windows, or accept Windows' linear rescale and refit after?** Recommendation:
  answer it (one subclass that WP-2 needs anyway for `WM_ENTERSIZEMOVE`), so a dialog moving between 100 % and 150 %
  monitors resizes once to the right size. Fallback if the subclass conflicts with tao: refit after `ScaleFactorChanged`
  (two resizes, logged), which is today's behaviour.
- **D6. Is a Windows Snap or tiling-WM resize a "user resize"?** Recommendation: yes (it is the user's action through
  the shell), and it is the only rule the Linux read-back can implement reliably.

## Alternatives considered

- **Keep the handshake and harden it further** (`08ad47b22` and successors). Each hardening closes one gap between
  two baselines; nothing stops the next asynchronous step from opening another. Rejected as the design; kept as the
  instance fix for the Dyno patch.
- **AutoScaleMode-style scaling of declared sizes by DPI.** This is what fixed heights did, and ADR 0016 cause 1 showed
  it breaking at 125-150 % and on small screens. Rejected.
- **Let the webview report a preferred size to the OS.** Neither WebView2, WKWebView nor WebKitGTK reports one to Tauri
  (ADR 0016); the measurement has to be ours.
- **Make every dialog `Manual` with a fixed default and a scrolling body.** Simple and robust, but brings back the
  "gap below the footer" and "scrollbar beside one line" reports for decisions and forms. Rejected for those kinds; it
  is the right mode for tools.
- **Size in TypeScript (`getCurrentWindow().setSize`).** Moves the DPI and frame conversion into the page, which cannot
  see the frame or the target monitor's DPI before a move, and breaks "Rust owns the window" (ADR 0026 §3). Rejected.
