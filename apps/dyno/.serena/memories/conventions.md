# Conventions

- Rust: rustfmt max_width 140; `unsafe` denied workspace-wide except tiny OS FFI modules with `#[allow(unsafe_code)]` + SAFETY comments; no unwrap/expect outside tests; doc comments on public items (`missing_docs`).
- Child processes only via `dyno_platform::process::command` / `run_with_timeout` (clippy disallowed-methods on `Command::new`).
- Untrusted input parsed once at the edge into types (`Request`, `RuleSet::validate`, `HelperAction::validate`, cleanup `admit`).
- Unavailable sensors are `Reading::Unavailable(reason)`, never zeros.
- Time is passed into the core as `Timestamp` (ms); the core never reads a clock.
- UI: standalone components, signals, OnPush, SignalStores, CDK for behaviour, Material selectively, own SVG icons in the `dy` namespace, ECharts with a CVD-checked palette; brand hue #BE3584 (suite icons).
- Format only your own files (`rustfmt --edition 2021 <files>`); never `npx prettier --write`.
