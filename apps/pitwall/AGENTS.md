# Pitwall — engineering rules for coding agents and contributors

Pitwall is the local LLM server and manager of the suite (Debrief, Pitcrew, Spotter, Dyno and any OpenAI, Anthropic or
Ollama compatible tool): a Rust core (`crates/pitwall-core`), the server (`crates/pitwall-server`), a
Tauri 2 tray app (`app/src-tauri`) and an Angular UI (`app/src`). See README.md for what it does and `docs/adr/` for why
it is built this way.

## Paddock rules (binding)

The family's binding rules live once, in the monorepo's root [AGENTS.md](../../AGENTS.md) and
[docs/rules.md](../../docs/rules.md) (ADR 0014 §10, rule 8: no per-app copies). Read them before this file; where a
rule here disagrees, they win. This file holds only what is specific to Pitwall.

## Privacy: local only (non-negotiable)

This is the user's explicit requirement and overrides every other consideration (ADR 0001):

- **Nothing leaves the machine.** No cloud engines or backends, no telemetry, no analytics, no crash upload, no
  "phone home" update pings beyond the opt-in app updater. No prompt, response, file name, model name or usage number
  is ever sent anywhere.
- **The only outbound traffic** is downloading engines and models **the user asked for**, from the hosts in
  `pitwall_core::privacy::DOWNLOAD_HOSTS`, over HTTPS, shown in the UI with their host, plus, only once the user turned it
  on, the check-only model update queries (repository ids of models already downloaded, ADR 0017). **Offline mode** turns
  even that off.
- **The request path has no HTTP client.** Only `pitwall_server::egress` may build one (clippy's
  `disallowed-methods` enforces it; `egress.rs` is the single allowed place). Engines are reached only through
  `pitwall_server::loopback`, whose address type cannot hold anything but 127.0.0.1. The test
  `request_handlers_cannot_reach_the_network` fails if a route handler can open an outbound connection.
- Requests never cause fetching: images must be inline data, `previous_response_id` and remote URLs are refused,
  tools run locally.
- Logs never contain prompts, responses or secrets; request
  content is kept for the inspector only when the user turns it on, on this machine.
- A change that adds any outbound connection needs an ADR and the user's approval. When in doubt, don't.

## Design principles

Apply these deliberately. They are not decoration:

- **Single responsibility.** One reason to change per module, type and component. Split a file when it starts doing
  several jobs (e.g. parsing vs. scheduling vs. serving); keep request/response DTOs apart from the logic that
  produces them.
- **Open/closed.** Extend by adding, not editing: a new engine is a new `Engine` implementation; a new API family is a
  new module under `pitwall_core::api` plus routes; new behaviour for users is settings, not hard-coded
  special cases.
- **Liskov.** Every implementation of a trait (`Engine`, the UI backends) honours the documented
  contract: same error semantics, no surprising panics.
- **Interface segregation.** Keep traits and TypeScript interfaces small and purpose-built.
- **Dependency inversion / code against abstractions.**
  - The server's routes depend on the `Engine` trait and the services' small interfaces, never on llama-server.
  - The Tauri commands depend on the `ServerClient` trait, never on HTTP details.
  - Angular components never call Tauri `invoke` directly: they go through the backend injected by the `BACKEND`
    token (real Tauri backend or the mock for browser dev/tests), and state lives in SignalStores.
- **Pure core, thin edges.** API translation, scheduling, the memory policy, fit checks, selection, catalogs, keys,
  limits, settings and metrics are pure functions in `pitwall-core`, with time and readings passed in as values.
  Processes, files, sockets and the OS stay thin in the server.
- **Ports and adapters.** The core never depends on the server, Tauri, an engine or an OS API.
- **Parse, don't validate.** Convert untrusted input (HTTP bodies, engine output, Hugging Face answers, GGUF headers,
  archives, `settings.toml`, IPC frames) into typed values once, at the edge (`ModelId`, `AppId`, `DownloadUrl`,
  `RepoId`, `ChatRequest`, …), and work with the types afterwards.
- **Make illegal states unrepresentable.** Prefer enums and newtypes over strings, flags and `Option` combinations
  that can disagree (e.g. `LoadPlan`, `Availability`, `LoopbackPort`).
- **Command–query separation.** Listings, status and history have no side effects; only load/unload/pin, downloads,
  key and permission changes, settings and serving changes mutate.
- Also: separation of concerns, high cohesion and low coupling, KISS, YAGNI, DRY with the rule of three before
  abstracting, composition over inheritance, Law of Demeter, least astonishment (sensible defaults, per-model and
  per-app overrides), fail fast with a message that tells the user what to fix, and leave touched code better than
  you found it.

## Security and robustness

Pitwall runs native engines on untrusted model files and answers requests from other programs:

- Never trust a request, the webview, or an engine: validate every field in Rust at the edge. The API binds
  127.0.0.1 only unless the user turns on LAN exposure (TLS, keys required; ADR 0007). `Host` and `Origin` headers are
  checked (DNS rebinding, cross-site requests).
- Per-app API keys: stored hashed (SHA-256 of a 256-bit secret), shown once, compared in constant time; the admin key
  the app uses lives in the OS credential store. Never log a key.
- Never run a binary that is not verified: engines come only from the pinned catalog
  (`pitwall_core::runtimes::catalog`), SHA-256 checked before unpacking, unpacked defensively, self-tested before
  use, with roll back.
- Engines run as child processes with an empty environment plus an allow-list, a random API key in the environment,
  `--offline`, no window, below-normal priority, tied to the server's lifetime, with timeouts, health checks and
  restart backoff.
- Tauri capabilities stay least-privilege: only the plugins and permissions the UI uses, no `fs`/`shell` wildcards, no
  remote content. Keep the CSP free of `unsafe-eval` and remote origins. Never bind model output with `[innerHTML]`.
- Parsers never panic on any input and enforce limits (`MAX_EVENT_BYTES`, request body size, message/image/tool
  counts, GGUF header limits, archive entry and size caps, `MAX_SETTINGS_BYTES`).
- No `unwrap()`/`expect()` on user data or I/O outside tests (clippy warns on both). Library errors are typed; the
  edges convert them to the API family's error shape or a `CommandError` for the UI.
- Nothing runs elevated. Child processes always go through `pitwall_server::process` (no console window, bounded).
- Long-running work runs off the request and UI threads; listings are paged; streams are bounded.

## Languages

- All code that ships inside Pitwall is Rust (core, server, Tauri shell) plus the Angular UI. Don't add C#,
  C++, Python or other runtimes to the product. Engines are third-party binaries Pitwall downloads, not code it ships.
- There is no plugin system (ADR 0010). Engines are `Engine` trait adapters that run as separate processes managed by the
  server; no third-party code runs inside the Pitwall process.
- `unsafe` is denied workspace-wide. An OS integration that needs FFI gets one small module with
  `#![allow(unsafe_code)]` and a comment on every `unsafe` block, and tests on that platform's CI runner.

## Where logic lives

- **The Rust core is the single source of truth** for behaviour: API translation, what fits, what to release, what to
  run next, which model to choose, which URL may be fetched. The server carries the core's decisions out; the app
  shows them.
- **Angular is the presentation layer**: layout, interaction state, rendering and formatting of what the server
  returns. It must not re-implement domain rules (fit, release countdowns, selection). If the UI needs a computation,
  add it to the core and expose it through the management API and a command.
- **The mock backend** exists for browser development and UI tests only. It is never the reference for behaviour,
  and features are not "done" until they work against the real server.

## Robustness

- **Contain failures.** One engine's crash, one bad model file or one slow request never takes down the
  server or another model. The app can crash or close without stopping the server.
- **Degrade, don't fail.** No engine installed, no GPU, offline mode, a locked file, a gated model or a full disk
  produce a clear message and a next step instead of an exception. A request that does not fit in memory is queued or
  answered with the typed `insufficient_memory` error — never pushed into swap.
- **Never unload a busy model.** Memory release (idle, pressure, yield) never touches a model serving a request
  (`pitwall_core::memory`, with a test).
- **Recoverable state.** `settings.toml`, `apps.json`, the library index and metrics are versioned, written atomically
  with a backup, and loading tolerates damage (the backup, then the defaults). New persistent state must do the same.
- **Bounded resources.** Respect the existing limits and add limits for any new unbounded input.
- **Every robustness behaviour has a test that triggers the failure** (crashing or hanging engines, corrupt model or
  archive, checksum mismatch, failed self-test and roll back, damaged settings, full queue).

## Working in parallel (agents)

- Each task owns an explicit set of files; don't edit outside it. Shared integration points (`lib.rs` files, the
  workspace `Cargo.toml`, `pitwall_core::mgmt`, `api.types.ts`, `app/src-tauri/src/commands.rs`) are changed by the
  integrating agent only, or by adding a single declared line.
- Don't run `cargo fmt --all` / `cargo clippy --fix` across the workspace while others are editing; format your own
  files (`rustfmt <files>`). Use a private `CARGO_TARGET_DIR`.
- Commit only complete, owned work. Never commit a workspace `Cargo.toml` listing crates whose sources aren't
  committed.
- Platform code must at least type-check on the other hosts: `cargo clippy -p pitwall-core -p pitwall-server
  --target x86_64-apple-darwin` and `--target x86_64-pc-windows-msvc`; the macOS paths are verified on the
  `macos-latest` runner (Apple Silicon).

## Contracts

- The client contract apps code against is the standard APIs plus `/x/v1/…` (ADR 0003, ADR 0009), versioned by
  `pitwall_core::API_VERSION`. Within a version, changes are additive only.
- The UI ↔ app wire contract is `app/src/app/core/api.types.ts` (camelCase JSON), mirrored by `pitwall_core::mgmt`.
  Change both sides of a contract in the same commit.
- The settings schema is `pitwall_core::config` (with `SCHEMA_VERSION`). New keys are optional with defaults; a
  breaking change bumps the version and adds a migration with a test.

## Testing and quality gates

- Every change ships with tests. Every bug fix starts with a failing regression test.
- Coverage targets: `pitwall-core` ≥ 99% lines (`cargo llvm-cov -p pitwall-core --summary-only`), Angular ≥ 80% lines
  overall and near 100% for stores, services and pure utilities.
- API compatibility: the contract tests in `pitwall_core::api` and `crates/pitwall-server/tests` pin the request and
  response shapes of the official OpenAI, Anthropic and Ollama SDKs; the fake llama-server covers engine behaviour.
  A real end-to-end test downloads the pinned CPU build and a tiny model when `PITWALL_LLAMA_E2E=1`.
- Coverage is necessary, not sufficient: assert behaviour, include edge cases and error paths. Every parser has a
  proptest "never panics" property.
- Before committing, all of these must pass:

  ```sh
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  cargo deny --workspace check
  node --test scripts/ci/*.test.mjs
  node --test conformance/test/*.test.mjs   # (repo root) the shared scripts: commit lint, latest.json, publish, check-index
  (cd app && npm audit --omit=dev --audit-level=high && npx ng lint && npx ng build && npx ng test --watch=false)
  ```

## Dependencies and releases

- Prefer well-maintained, permissively licensed (MIT/Apache-2.0) dependencies (`deny.toml` lists the allowed
  licences). Commit `Cargo.lock` and `package-lock.json`; install with `npm ci` in CI.
- Renovate updates dependencies and pins CI actions to commit digests. Gitea Actions needs the v3 artifact actions.
- Updating the pinned llama.cpp release (`crates/pitwall-core/src/runtimes/llama-runtimes.json`) is a reviewed change
  that must pass the end-to-end test.
- Use Conventional Commits (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`, `chore:`) so changelogs can be
  generated (`cliff.toml`); the shared `conformance/src/commit-lint.mjs` checks them (`paddock-conformance commit-lint`). Releases are `pitwall-v*` tags following semver
  (`docs/releasing.md`). Never add Co-Authored-By or AI attribution lines.
- Record significant architecture decisions as ADRs in `docs/adr/` (numbered, with status, context, decision,
  consequences).

## Style

- Match the surrounding code: naming, comment density (doc comments on public items — `missing_docs` is on —
  comments only where intent isn't obvious), idioms. Rust uses `rustfmt.toml` (max_width 140).
- UI: standalone components, signals, OnPush, CSS custom properties for theme tokens (dark and light), accessible
  markup (real buttons, labels, aria-labels on icon buttons), the `pw` selector prefix.
- UI building blocks: use **Angular CDK** for behaviour and accessibility (menus, overlays, dialogs, listbox,
  scrolling) instead of hand-rolling them, and **Angular Material first** (rule 47: a Material component wherever one fits), themed through
  Pitwall's CSS tokens. Icons use Material's `MatIconRegistry` with Pitwall's own SVGs registered as literals in the
  `pw` namespace (`<mat-icon svgIcon="pw:models">`); no icon fonts and no icon URLs fetched at run time (the CSP
  forbids it). Charts use Apache ECharts with the CVD-checked palette.
- Status colours always come with words or shapes; validate colours for contrast and colour-vision deficiency.

## Working practices

- Record every benchmark you quote (rule 48): first as a results block in the app's `docs/performance.md` (shared code:
  `docs/benchmarks.md`), then link it from the ADR, plan, CHANGELOG, commit or PR that quotes the number.
- Spend CI only on what a change needs (rule 50): one run per branch head, none while review fixes are pending or for a
  branch already merged elsewhere, none for docs-only changes; branches run the affected plan, full gates once on main.
- Slow means two questions (rule 51): can more threads or workers run it (bounded by the kind of work), and can it be made
  cheaper (less work per item, no repeats, a cheaper algorithm or format, caching, batching, skipping)? Measure both;
  thread counts are Settings with an "Auto" default; a long or bulk operation's design has a "Parallelism and cost" section.
- Never act on a reusable OS name read earlier (rule 53): hold it (handle, job, listener) or verify its identity (pid plus
  start time, `paddock_proc::ProcessId`) immediately before acting; kills go through `paddock_proc` (Clippy
  `disallowed-methods` and C53 check it, ADR 0033).
- Research official sources before choosing libraries or relying on an engine's or API's behaviour (llama-server
  options, SDK request shapes), and record what you found and when in the ADR or doc comment.
- Navigate code with Serena's symbol tools (find symbol, references, overview) when they are available instead of
  reading whole files; it saves context and cost. The project config is `.serena/project.yml`; starter memories are in
  `.serena/memories/` (keep them current when the layout changes).
- Delegate mechanical or well-specified work (refactors with a precise spec, test scaffolding, docs) to cheaper
  models. Keep architecture and review on the main model.

## Lessons from the suite

Read [docs/suite-lessons.md](docs/suite-lessons.md) before changing build, bundle, window, service, test or CI code:
it lists the pitfalls Debrief and Pitcrew already hit (CSP and inline styles, macOS signing and drag regions,
launchd/systemd PATH, Remote Desktop sessions, conhost, Linux threads, test hygiene, release-build budgets).
