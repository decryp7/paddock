# Conventions

- AGENTS.md is the rulebook (SOLID, pure core / thin edges, parse don't validate, typed errors, no unwrap on I/O).
- Rust: rustfmt max_width 140; `missing_docs` on; `unsafe` denied except the commented FFI modules
  (`platform/win_session.rs`, the pipe ACL in `ipc.rs`, `app/src-tauri/src/service_install.rs`).
- Child processes only via `pitcrew_service::process::quiet_command` (clippy `disallowed_methods`), always with a
  timeout or a supervisor; error messages never include arguments (tokens are arguments).
- Wire contracts change on both sides in one commit: `pitcrew_core::protocol` (PROTOCOL_VERSION) and
  `app/src/app/core/api.types.ts` ↔ core types / `app/src-tauri/src/dto.rs`. Config keys are snake_case (TOML schema).
- Tests: every robustness behaviour has a test that triggers it; the service's end-to-end tests drive the real
  supervisor with `examples/fake_runner.rs` and a controllable clock (`tests/supervision.rs`).
- UI: standalone components, signals, OnPush, `pc` prefix, CDK for behaviour, own SVG icons via MatIconRegistry,
  tokens in styles.scss, never `[innerHTML]` for log text. Don't run `npx prettier --write`.
- Conventional Commits; no AI attribution trailers.
