# Dialog size edges: real-window runs (ADR 0043 WP-2)

Records of runs of the Windows and Linux edges (`crates/paddock-dialogs/src/edge/`) against real windows. Each run says
what was run, where, and what it showed; a section is added per run, never rewritten.

## Linux, X11 (Xvfb) with and without a window manager

- **What:** `crates/paddock-dialogs/tests/show_probe.rs` opens the real shell-demo dialog frame of each kind in a real
  webview window, lets it measure and send `dialog_ready`, and checks that the window was shown by that ready, that the
  viewport equals the expected height (below), that the page does not scroll and that the footer is fully inside.
  `PADDOCK_PROBE_DIALOG` picks the dialog. Run on the Orb machine `paddock-gate-arm` (Ubuntu 24.04, arm64,
  WebKitGTK 4.1), `xvfb-run` at 1920x1080, then again with `openbox` started first (installed for the run, purged
  after). Commit: `feat/dialog-size-to-content-edges` at the commit that adds this file.
- **Expected height:** decision and form `max(measured, 200)` (`DIALOG_MIN`); viewer `max(measured, 560)`; tool at
  least the floor (it is `Manual`: the declared 620).

| Kind | Dialog | Measured content | Viewport | No WM | openbox |
|---|---|---|---|---|---|
| decision | `sd.confirm-reset` | 146 px | 200 px | pass | pass |
| form | `sd.rename` | 145 px | 200 px | pass | pass |
| viewer | `sd.about` | 192 px | 560 px | pass | pass |
| tool | `pd.settings` | 620 px | 620 px | pass | pass |

- **Found by the run:** a hidden GTK window gets no configure event for a size asked of it and its `inner_size` keeps
  the old size, so the `Resized` echo never came and the show waited for the deadline (the first run ended "no report").
  Fixed in `tauri/sizing.rs` (the glue reports the size asked for as the echo, Linux, window not visible) and in
  `SizeState::on_resized` (a `Resized` equal to the current size is no change), each with a test. The probe itself
  forwarded no window events; it does now.
- **Not covered:** Wayland (no compositor available); the frame read with a window manager is not printed by the
  probe's log, only that nothing was cut off; work-area clamp on a small monitor; scale factors above 1 (Xvfb is 1).

### Linux scenarios (X11, openbox): remembered size and user resize

`PADDOCK_PROBE_SCENARIO=remember|resize`, all four kinds, passing:

- **remember:** the dialog is asked to open at a remembered 520x420. A tool keeps it (520x420); the form, the decision
  and the viewer ignore the size and open at their natural one (400x200, 400x200, 720x560) (D3).
- **resize:** the window is resized by the window system, outside the dialogs layer (the inference of an unexplained
  `Resized` on X11, D4). The decision, form and viewer announce `Manual` (`paddock://dialog-size-mode`) and keep the new
  size; the tool, already `Manual`, announces nothing and keeps it.
- **Oracle:** the open checks now judge with `paddock_dialogs::expect` (the spec, independent of `SizeState`), including
  the work-area clamp on a small screen as pure tests (`expect::tests::on_a_small_screen_...`).
- **Finding:** on Windows the tool opened at exactly 75 % of the work area (540 of 720, 424.8 of 566.4), that is with the
  cap computed with a frame of 0: the window has no frame before it is shown. Up to one frame (31 px) over the cap; the
  probe judges only that a tool fits the screen until the cap is recomputed with the frame (SizeState, WP-1).

## Windows

- **Message-only window tests** (`edge::windows::tests`) in `paddock-win11-test` (x64 exe from `cargo xwin`, from
  snapshot `gui-session-tools`, reverted afterwards): 15 `edge::` tests pass. The first run hung because
  `DefWindowProc` enters a modal loop on `SC_SIZE` and `SC_MOVE`; the test no longer sends `WM_SYSCOMMAND`.
## Windows 11 (VM): real dialogs at 100, 125 and 150 %

- **What:** `show_probe` (x64 exe from `cargo xwin`, tag `feat/dialog-size-to-content-edges` at the commit that adds
  this section) in `paddock-win11-test` from snapshot `gui-session-tools`, in the auto-logon console session (a scheduled
  task with `/it`), once per kind, with the user's scale set through `LogPixels` 96, 120 and 144 plus a restart each.
  The shell-demo `dist` and the capability files are copied beside the exe (`PADDOCK_PROBE_CRATE_DIR`). The probe holds
  each window up (`PADDOCK_PROBE_HOLD`) while the script takes a full-screen, DPI-aware screenshot. The VM was reverted
  to `gui-session-tools` afterwards. Screen 1024x768; the work area is 720, 708 and 696 physical px high at the three scales
  (taskbar), so 563/464 logical at 125/150 %. Screenshots: `scratch/dialog-edge-shots/<scale>-<kind>.png` (for
  independent review, ER-23); the shot is taken when the page reports, so a window may still be fading in.
- **Judged:** shown by its ready; the page's viewport equals max(floor, content) (decision, form: 200; viewer: 560), or
  the work area when that cannot hold it; no scroll; the footer is fully inside the viewport. The window's logical size
  is its physical inner size over its scale.

| Kind | Scale | Window, logical (physical) | Viewport | Result |
|---|---|---|---|---|
| decision `sd.confirm-reset` | 100 / 125 / 150 % | 400x200 (400x200, 500x250, 600x300) | 200 px, content 148 | pass |
| form `sd.rename` | 100 / 125 / 150 % | 400x200 (400x200, 500x250, 600x300) | 200 px, content 145 | pass |
| viewer `sd.about` | 100 % | 720x560 (720x560) | 560 px, content 174 | pass |
| viewer `sd.about` | 125 % | 680x535 (850x669) | 536 px: clamped to the work area | pass |
| viewer `sd.about` | 150 % | 680x433 (1020x649), at 0,0 | 433 px: clamped to the work area | pass |
| tool `pd.settings` | 100 % | 768x540 | 540 px | pass |
| tool `pd.settings` | 125 % | 614x425 | 426 px | pass |
| tool `pd.settings` | 150 % | 512x348 | 349 px | pass |

- **Reading the table:** decisions and forms are the exact logical size at every scale (the rounding is one `ceil` in
  physical px) and sit inside the work area. A viewer cannot be 560 px tall where the work area is 464 to 566 logical px:
  it is clamped to the work area less its visible frame (the floor is lowered to what the screen allows, ADR 0043 §4).
  A tool is declared 880x620; on this small screen the 75 % cap (`CAP_FRACTION`) of the work area gives 768x540 at 100 %,
  which is correct, not a defect; at 150 % the work area is 683x464 logical, so 512x348.
- **Not covered:** a scale change with a dialog open (WM_DPICHANGED with the suggested rectangle; ADR 0040's DPI-change
  step), a second monitor, user resize and snap. These need the VM-smoke harness (WP-5).

## What the probes assume (ADR 0043, from the ADR's owner)

A note for the Windows and Linux edge probes (2026-11-04):

- **Every kind has a floor of 320 x 200** (`DIALOG_MIN`), and the **viewer's kind floor is 680 x 560** (`VIEWER_MIN`). Both are the OS
  minimum (`min = max(kind floor, measured min content)`), so an inner height below 200, or a viewer below 560, is a bug.
  A probe expects `max(floor.height, min(measured, cap))` for `Height` kinds.
- A viewer therefore opens at 560 px or its content, whichever is larger, and never shrinks (`Shrink::Never`); the ADR's
  "opens at `min(content, preferred)`" is read with that floor (the table in ADR 0043 §1 now says so).
- A tool (`Manual`) opens at its contract's size, 960 x 680 by default.
