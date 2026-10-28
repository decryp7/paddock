# Dialog hidden-window spike (ADR 0026, package 1.1-0)

Not product code. A minimal Tauri 2 app that opens probe dialog windows three ways and logs what the webview does
before and after the window is shown:

- `a`: created hidden; the page waits for its content, `document.fonts.ready` and the next animation frame, then reports
  its natural height; Rust fits the window and shows it.
- `b`: created visible far off-screen; same page logic; Rust fits, moves it into place and shows it.
- `c`: created hidden; the page waits for its content and `document.fonts.ready`, then reads its layout synchronously
  (`getBoundingClientRect`), with no frame wait.
- `d`: as c, with the frame laid out at the dialog's declared width (an unmapped WebKitGTK view has a 0 x 0 viewport).

Scenarios: `normal`, `font_slow` (the font arrives after 400 ms), `route_slow` (the body arrives after 600 ms),
`zoom125`/`zoom150` (webview zoom, a stand-in for display scale), `unowned` (no parent window), `monitor2` (on the last
monitor, when there are several).

```sh
cargo build                                    # own CARGO_TARGET_DIR, CARGO_BUILD_JOBS=2, through heavy.sh
PROBE_REPS=3 PROBE_OUT=probe.jsonl target/debug/dialog-hidden-spike   # opens and closes its own windows, then quits
node summarize.mjs probe.jsonl                 # one row per method and scenario
node summarize.mjs probe.jsonl --trials        # one row per trial
```

Linux: `dbus-run-session -- xvfb-run -a --server-args="-screen 0 1600x1000x24"` in the paddock-ci image. The results
and the recommendation are in `docs/research/2026-10-28-dialog-hidden-windows.md`.
