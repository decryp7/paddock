# Development

The engineering rules are in [AGENTS.md](../AGENTS.md); lessons from the other suite apps are in
[suite-lessons.md](suite-lessons.md). This page is the practical side: layout, prerequisites, how to run things, the
quality gates and where to add code.

## Layout

```
crates/pitwall-core      pure logic: API formats, scheduler, memory policy, selection, keys, settings, packs (no I/O)
crates/pitwall-loopback  HTTP client that can only reach 127.0.0.1 (used to talk to engine processes)
crates/pitwall-server    the windowless server: engines, library, serving, auth, HTTP APIs, management API
app/src-tauri            the Tauri 2 shell: tray, window, commands, updater (being built; not a workspace member yet)
app/src                  the Angular UI (standalone components, signals, SignalStores)
scripts/                 commit-lint, release manifest (latest.json), export-diagrams.sh, ci/ helpers
docs/adr/                architecture decisions (index in docs/adr/README.md)
docs/diagrams/           draw.io sources (.drawio) and exported SVGs; see design.md
docs/screenshots/        UI screenshots (light and dark), produced by the Playwright capture
.gitea/workflows/        CI
```

## Prerequisites

- Rust stable 1.88 or newer (`rustup` with `clippy` and `rustfmt`).
- Node 24 (`app/.nvmrc`) and npm.
- Tauri 2 prerequisites once the shell lands: on Windows the MSVC build tools and WebView2; on macOS the Xcode command
  line tools; on Linux `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev`.
- Optional: `cargo-deny` (licences and advisories), `cargo-llvm-cov` (coverage), `git-cliff` (changelog), `lefthook`
  (git hooks: `npx lefthook install`), draw.io desktop (diagrams).

Use a private build folder when several checkouts or agents build at once, for example
`CARGO_TARGET_DIR=~/.pitwall-target CARGO_BUILD_JOBS=4`. Do not run `cargo fmt --all` or `cargo clippy --fix` across the
workspace while others are editing; format your own files with `rustfmt <files>`.

## Run the server

```sh
# A scratch data folder, so nothing touches your real one (PITWALL_HOME works too):
cargo run -p pitwall-server -- --data-dir /tmp/pitwall-dev --port 11436
curl http://127.0.0.1:11436/x/v1/health
```

The server binds `127.0.0.1` only. With no `--port` it uses the port in `settings.toml` (11435 by default); with no
`--data-dir` it uses the per-OS data folder. `--fake-engine` turns the binary into a tiny OpenAI-compatible test
engine; the tests and the engine pack example use it.

The management API needs the admin key in `<data folder>/admin.key`:

```sh
KEY=$(cat /tmp/pitwall-dev/admin.key)
curl -s -X POST http://127.0.0.1:11436/mgmt/v1/rpc/overview -H "Authorization: Bearer $KEY"
```

## Run the UI

```sh
cd app
npm ci
npx ng serve        # browser build with the mock backend; no server needed
npx ng test --watch=false
npm run e2e         # Playwright (Chromium and WebKit) against the mock backend; npm run e2e:install first
npm run screenshots # captures docs/screenshots/<screen>-<light|dark>.png from the mock backend
```

The mock backend exists for browser development and UI tests only. It mimics the contract with plausible data and is
never the reference for behaviour; a feature is not done until it works against the real server.

## Quality gates

Before committing, all of these must pass (also in [AGENTS.md](../AGENTS.md)):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny --workspace check
node --test scripts/commit-lint.test.mjs scripts/release/latest-json.test.mjs
(cd app && npm audit --omit=dev --audit-level=high && npx ng lint && npx ng build && npx ng test --watch=false)
```

Extra checks that CI runs:

- `scripts/ci/privacy-check.sh`: `reqwest` appears only in `crates/pitwall-server/src/egress.rs` and the server's
  manifest ([ADR 0001](adr/0001-local-only.md)). The Rust test `crates/pitwall-server/tests/privacy.rs` checks the same
  from inside the crate, plus that handlers cannot reach the network and that engines get a scrubbed environment.
- `scripts/ci/ui-coverage-guard.mjs`: Angular line coverage of at least 80 %.
- `scripts/ci/check-index.mjs`: the built `index.html` has no inline elements (the Tauri CSP nonce rule in
  [suite-lessons.md](suite-lessons.md)).
- `scripts/ci/macos-verify-bundle.sh`: the `.app` is ad-hoc sealed and resolves its bundled libraries.
- Coverage: `cargo llvm-cov -p pitwall-core --summary-only` (target 99 % lines).
- Cross-target type checks: `cargo clippy -p pitwall-core -p pitwall-server --target x86_64-pc-windows-msvc` and
  `--target x86_64-apple-darwin`.

Commit messages are Conventional Commits, checked by `scripts/commit-lint.mjs` (the `commit-msg` hook). Never add
Co-Authored-By or AI attribution lines.

## Tests

| Layer | Where | What it covers |
| --- | --- | --- |
| Core unit and property tests | `crates/pitwall-core/src/**` (`#[cfg(test)]`) | Every pure decision; API shapes pinned against the official SDKs; "never panics" properties for parsers |
| Server end-to-end | `crates/pitwall-server/tests/e2e.rs` | The real router and pipeline over a fake engine: keys, provenance, streaming in all three families, permissions and limits, Host/Origin refusal, pairing, engine failures, pause and release, offline mode, embeddings, an engine pack driving a real child process |
| Privacy | `crates/pitwall-server/tests/privacy.rs` | No outbound client outside egress; handlers cannot reach the network; scrubbed engine environment |
| UI unit and component | `app/src/**/*.spec.ts` (Vitest and TestBed) | Stores, services, pure utilities, components |
| UI end-to-end | `app/e2e` (Playwright) | Flows against the mock backend |
| Real engine | planned | A test that downloads the pinned CPU build and a tiny model (`PITWALL_LLAMA_E2E=1` in AGENTS.md); not implemented yet. The real-model smoke test is done by hand: [testing-manual.md](testing-manual.md) |

Tests never leave processes behind (children are killed on drop), never touch real user folders (temp dirs or
`PITWALL_HOME`), and are timezone independent.

## Diagrams

Diagram sources are draw.io files in `docs/diagrams/`; SVGs are generated.

```sh
scripts/export-diagrams.sh     # needs draw.io desktop (DRAWIO=/path overrides discovery) and node
```

Follow [diagrams/STYLE.md](diagrams/STYLE.md), keep the `.drawio` uncompressed, update the footer date when a diagram
changes, and commit the `.drawio` and the regenerated SVG together. A diagram must match the code; when behaviour
changes, change the diagram in the same commit.

## Where to add things

| I want to... | Do this |
| --- | --- |
| Support a new OpenAI-compatible local engine | Write an engine pack (`*.engine.toml`), see [user-guide.md](user-guide.md#packs); no code |
| Add a curated model list | A model catalog pack (`*.models.toml`) |
| Add an engine with its own protocol | Implement `Engine` in `crates/pitwall-server/src/engines.rs` (or a new file), register it in `AppState::open`, add the pure parts to `pitwall-core`, an ADR, and tests with the fake engine |
| Add or change an endpoint | Parse and format in `pitwall_core::api::<family>` (pure, with a contract test), add the route and handler in `http.rs`, keep `/x/v1` changes additive ([ADR 0003](adr/0003-api-surface-and-compatibility.md)) |
| Add a management operation | DTO in `pitwall_core::mgmt` and `app/src/app/core/api.types.ts` in the same commit, a method in `mgmt.rs`'s `call`, a method on the UI `Backend` and the mock |
| Change a memory or queue rule | `pitwall_core::memory` or `scheduler` with a failing test first; update [ADR 0006](adr/0006-memory-policy.md) or [0005](adr/0005-queue-and-scheduling.md) |
| Add a setting | `pitwall_core::config` (optional key with a default, validation), the UI settings page, tests; a breaking change bumps `SCHEMA_VERSION` with a migration |
| Update the pinned llama.cpp release | Edit `crates/pitwall-core/src/runtimes/llama-runtimes.json` (URLs, sizes, SHA-256); a reviewed change, verify with the manual install flow |
| Add a download host | `pitwall_core::privacy::DOWNLOAD_HOSTS` with a reason, an ADR update and a test; every new host widens what Pitwall can contact |

## Working in parallel

Each task owns an explicit set of files. Shared integration points (the `lib.rs` files, the workspace `Cargo.toml`,
`pitwall_core::mgmt`, `api.types.ts`, `app/src-tauri/src/commands.rs`) are changed by the integrating agent only, or by
adding a single declared line. Commit only complete, owned work, with `git add <paths>`; verify the committed tree in
isolation (`git archive HEAD`, build, test) before pushing larger changes.
