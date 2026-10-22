# 2. A pure core crate with thin adapters

- Status: accepted (2026-10-22)
- Related: ADR 0001 (local only), ADR 0004 (engines), ADR 0010 (no plugins), ADR 0011 (packs); code:
  [`crates/pitwall-core`](../../crates/pitwall-core/src/lib.rs), [`crates/pitwall-loopback`](../../crates/pitwall-loopback/src/lib.rs),
  [`crates/pitwall-server`](../../crates/pitwall-server/src/lib.rs)

## Context

Pitwall decides many things: which model an app gets, whether a model fits, what to release, which request runs next,
which URL may be fetched, how three API families map onto one request. These decisions must be testable without
processes, sockets, clocks or a GPU, and they must stay the same whether the caller is the server, the Tauri app or a
test. The suite rule (Pitcrew, Debrief) is a pure Rust core with thin edges.

## Decision

Three workspace crates, with dependencies pointing inward only:

- **`pitwall-core`**: pure logic, no I/O, no async runtime, no clock. Time (`Millis`), memory readings (`System`),
  randomness and file contents arrive as arguments. It holds the wire formats (`api::{openai, anthropic, ollama}`, `llama`,
  `sse`, the neutral `chat` request), serving decisions (`scheduler`, `memory`, `select`, `fit`, `backoff`), model and
  engine knowledge (`gguf`, `quant`, `hf`, `runtimes`, `hardware`, `packs`), security (`privacy`, `keys`, `limits`),
  state (`config`, `metrics`, `events`, `usage`, `discovery`) and the UI contract (`mgmt`). `missing_docs` is on and
  `unsafe` is denied workspace-wide. Target: 99% line coverage.
- **`pitwall-loopback`**: a minimal HTTP/1.1 client whose port type (`LoopbackPort`) can only mean `127.0.0.1`. It is
  how the server talks to engine processes, so no code path can use it to reach another host.
- **`pitwall-server`**: the adapter that carries the core's decisions out. It owns processes (`process`, `engines`,
  `pack_engine`), files (`paths`, `library`, `auth`), the one outbound HTTP client (`egress`, ADR 0001), the loaded
  models (`serving`), the shared state (`app`) and the sockets (`http`, `mgmt`). It runs as its own windowless process
  (`pitwall-server`, default port 11435) so it keeps serving when the app closes.

The Tauri shell (`app/src-tauri`) is a second adapter over the same server API; it does not link serving logic. The
Angular UI is presentation only and reaches the server through a backend service behind a DI token.

Ports in the hexagonal sense are few and small: the `Engine` trait (ADR 0004) is the only one the server defines; the
core exposes its decisions as plain functions (`memory::plan_release`, `memory::plan_load`, `scheduler::Scheduler`,
`select::choose`, `fit::assess`) rather than traits, because they take values and return values.

Persistent state (`settings.toml`, `apps.json`, metrics) is versioned, written atomically with a backup
(`paths::write_atomic`, `read_with_backup`) and tolerates damage by falling back to the backup, then defaults. Untrusted
input is parsed once at the edge into typed values (`DownloadUrl`, `ModelId`, `AppId`, `Priority::parse`,
`Requirements::parse_auto`, `Packs`).

## Consequences

- Decision logic is covered by unit and property tests that need no machine state; the server's end-to-end tests drive
  the whole pipeline over a fake engine (`fake_engine`, `--fake-engine`) and a real child process.
- A new API family or format is a new module in `pitwall-core::api` plus routes in `http.rs`; a new engine is an
  `Engine` implementation or a declarative pack (ADR 0011).
- The core cannot accidentally gain a network client or a process spawn: it has no dependency that could. In the server,
  `clippy.toml` and `tests/privacy.rs` confine `reqwest` and `Command::new` to `egress.rs` and `process.rs`.
- The price is plumbing: every reading (time, RAM, GPU) is gathered by the server and passed in.

## Not built yet

- The Tauri shell crate (`app/src-tauri`) is being built separately and is not part of the workspace members yet.
- Per-OS service registration lives in `pitwall-server/src/service/spec.rs` as pure functions of a target OS; the
  installers that call them are part of the shell work.
