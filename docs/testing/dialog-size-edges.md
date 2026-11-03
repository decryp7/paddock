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

## Windows

- **Message-only window tests** (`edge::windows::tests`) in `paddock-win11-test` (x64 exe from `cargo xwin`, from
  snapshot `gui-session-tools`, reverted afterwards): 15 `edge::` tests pass. The first run hung because
  `DefWindowProc` enters a modal loop on `SC_SIZE` and `SC_MOVE`; the test no longer sends `WM_SYSCOMMAND`.
- **Real dialogs at 100, 125 and 150 %:** not run yet (waiting for the VM); to be added here with screenshots in
  `scratch/dialog-edge-shots/`.
