# Pitwall — engineering rules for coding agents and contributors

Pitwall is the local LLM server and manager of the suite (Debrief, Pitcrew, Spotter, Dyno and any OpenAI, Anthropic or
Ollama compatible tool): a Rust core (`crates/pitwall-core`), the server (`crates/pitwall-server`), a
Tauri 2 tray app (`app/src-tauri`) and an Angular UI (`app/src`). See README.md for what it does and `docs/adr/` for why
it is built this way.

## Paddock rules (binding)

The family's binding rules, copied from [paddock/docs/rules.md](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/rules.md) (the source; update there first). Where an older rule in this file disagrees, these win.

### Way of working
1. **One app at a time.** Finish the current app before starting another; parallel agents only within that app.
2. **Every agent works in its own git worktree** with its own `CARGO_TARGET_DIR` and its own e2e port; the main checkout
   stays on `main` for the coordinator.
3. **Tests in a branch: only the affected ones** (touched crates, touched specs, related Playwright files), with
   Playwright in Chromium only. The full suite, Chromium and WebKit, runs once on the combined result before a local
   build or release, plus CI. Fix everything locally first; CI confirms, it is not where failures are found.
4. **When CI fails:** fix and re-run only the failed tests locally, push once; never push fix by fix to see what CI says.
   Lint and type-check the touched files before every push.
5. **Every bug fix starts with a failing regression test.** A test-only failure is fixed properly (deterministic, no
   sleeps), never silently skipped.
6. **Tests never hard-code the app version;** they read it from its single source and check its shape.
7. **Tests never touch the user's machine:** no real launchd/Login Items/services, no installed apps, never the
   installed Google Chrome (pinned Chrome for Testing or an explicit env var only), no `~/Documents`, `~/Desktop`,
   `~/Downloads`, no real user data, no windows on the desktop, no processes left behind. Docker: only containers you
   created.
8. **Every lesson goes into [lessons.md](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/lessons.md)**, every rule here; no `Co-Authored-By` or AI attribution anywhere.
9. **Releases only from a tag in CI**, never from a local machine; a local build for the user to try is fine but is never
   published. Never tag a `[skip ci]` commit.

### Long operations and performance
10. **Every operation runs on a background thread or worker pool,** never on the UI thread, the Tauri main thread or an
    IPC handler's thread; commands return at once and report through job events.
11. **Live, phase-labelled progress from the first moment,** each phase on its own scale (a growing count when the total
    is unknown, never a static 0); progress values never go backwards; Cancel works in every phase and leaves state
    consistent (build new data, then swap).
12. **The UI never times out a running job;** it shows "Still working".
13. **Parallel where it helps, bounded by the kind of work** (CPU: cores−1 or the Performance setting; SSD I/O 4–8; HDD
    1–2; network a few per host; memory-heavy by RAM), background priority, deterministic results.
14. **Multi-stage work is a pipeline:** producers → bounded queue → workers, stage-specific thread counts, back-pressure,
    cancel across stages, per-stage progress; determinism by commit-time ordering. IDs: number only new items in sorted
    canonical relative path order; existing IDs never change; moved/renamed files are matched by path, then fingerprint.
15. **Hash while writing;** never re-read a just-downloaded file. Large downloads use segmented (Range) downloads where
    the host supports them, with per-host caps and 429 back-off.
16. **Performance has budgets** (startup, import throughput, time to first event, query latency, scroll smoothness,
    memory) checked in the nightly benchmark workflow.

### User interface
17. **Every dialog is a real OS window** (owned by and centred on the main window, remembered size/position, Escape/Enter);
    modality is explicit per dialog (modal for confirmations, credentials, unsaved changes, pickers that feed the next step;
    modeless for tools kept open beside the work). Menus, dropdowns, autocomplete and tooltips stay in-app.
18. **Panes lay out by container queries;** paths and file names truncate in the middle with the full value in a tooltip;
    no horizontal scroll at any width ≥ 320 px.
19. **A control that repairs X never depends on X;** every "X is not running" message has a working Repair button; never
    a message that points at the page it is shown on; an installable thing is never listed as "planned".
20. **Search finds identifiers however they are written** (parts, joins, separators, case, typos) and the AI resolves the
    user's words to concrete values before querying.
21. **Tunables are Settings, knowledge lists are TOML packs** (built-in + user `packs/`, parsed once with limits, never
    executing code, reloaded without a restart).
22. **Every drawing, diagram and screenshot gets an independent review;** every Paddock image or status change also
    refreshes the profile repo.

### Platform and release
23. **macOS bundles are ad-hoc sealed** (`signingIdentity: "-"`) and CI verifies them with `codesign --verify --deep
    --strict` on the .app and the DMG; release verification runs it on the published DMG. While ad-hoc, the hardened
    runtime is off (ad-hoc code under it cannot load vendor-signed libraries: "different Team IDs"); every bundled
    Mach-O carries the same TeamIdentifier, and CI starts every sidecar from the built app (`--self-test`). The hardened
    runtime comes back with a real signing identity.
24. **Services survive updates:** re-register once per version change (unregister → `launchctl bootout` → wait →
    register) and repair a stale registration automatically.
25. **Before tagging:** `cargo deny check advisories`, one CI build with bundles, the icon check, strip whitespace from
    signing secrets, and the release notes' First launch section (until the apps are signed).
26. **Process memory on Linux:** skip sysinfo thread entries and measure PSS (`smaps_rollup`).

### Angular and SignalStore (approved 2026-10-23; reasons: [research](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/research/2026-10-angular-and-signalstore.md))
27. Zoneless only. No `zone.js` in any app, no `provideZoneChangeDetection`. Every component declares `changeDetection: ChangeDetectionStrategy.OnPush` explicitly (v22 defaults to it, but explicit survives a lint or a future default change); `Eager` requires a comment.
28. `effect()` is only for side effects on things outside the signal graph (DOM, IPC, storage, logging, charts). State derived from other state is `computed`, `linkedSignal` or `resource`. An effect that calls `.set()`/`patchState()` needs a one-line justification comment, and `untracked()` is allowed only around the side effect, not to hide a dependency.
29. Reads of signals in an async function or effect happen before the first `await`.
30. Request state is a status union (`idle | loading | loaded | error`) from one shared `withRequestStatus` feature, never separate `loading`/`error` flags.
31. A store is changed only through its own methods; `protectedState` stays on; no `patchState` outside the store file; store files are named `*.store.ts`.
32. Provide a store at the narrowest scope (route/component/dialog window); `providedIn: 'root'` only for app-wide singletons. A store injects at most two other stores; more means a missing facade or feature.
33. New code uses `inject()`, signal `input/model/output/viewChild`, `@if/@for/@switch` with `track`, standalone components, and `[class.x]` over `ngClass`.
34. Each OS window is its own Angular bootstrap with its own small provider set. The engine (Rust) is the single source of truth; windows share state by commands plus events and never by importing another window's store state. Dialog windows load only the providers they need.
35. Heavy, rarely used panels are lazy (`@defer` or `loadComponent`); every release records the initial chunk size and the budget is tightened to measured size plus 10%.
36. Prefer `resource`/`rxResource` for new read-only loading with cancellation; keep `withMethods` async for commands. Do not adopt `@Service`, `injectAsync` or the NgRx events plugin until documented stable.
37. New forms use Signal Forms; existing reactive forms stay until touched.
38. Tests: Vitest through the Angular unit-test builder, `await fixture.whenStable()`, store tests via `TestBed`; use CDK harnesses for Material/CDK widgets; each lifecycle effect gets a test that triggers the failure.
39. A shared store feature or component used by two apps lives in `paddock-ui`, not copied.

### Design (user, 2026-10-24)
40. **Recurring bugs mean a missing design rule.** When the same kind of bug shows up again (in one area, across dialogs,
    panes or apps), stop fixing instances: name the pattern, propose a redesign as an ADR that makes the class impossible
    or checked (a declared contract, one shared mechanism, a lint or test over every instance), and fix the instances
    through it. Until v1, check every batch of bug reports for such patterns.

### Throughput (user, 2026-10-24)
41. **Scale parallel work to the machine.** The coordinator watches load and free disk continuously and keeps the
    machine busy but not overloaded: start more agents when load stays low, hold or stop starting them when it stays
    high (Mac mini, 14 cores: aim for a 1-minute load of about 6–16, at most ~5 heavy jobs, `CARGO_BUILD_JOBS=2` each),
    use the shared build cache (`sccache`) and copy-on-write target folders, and never let tests crowd out a release.

### Sharing (user, 2026-10-24)
42. **Share first.** Before building anything in an app, check Paddock's shared crates and packages (crates/README.md,
    ui/README.md, docs/plans/) for it. If it exists, use it; if it is missing, generic, or another app has or will need the
    same thing (rule 39), build it in Paddock and consume the published version instead of writing an app copy. Every
    work brief includes this check; every app move and every few releases a survey looks for copies that drifted apart
    and turns them into shared work packages. App-specific domain logic stays in the app.

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
  node --test scripts/commit-lint.test.mjs scripts/release/latest-json.test.mjs
  (cd app && npm audit --omit=dev --audit-level=high && npx ng lint && npx ng build && npx ng test --watch=false)
  ```

## Dependencies and releases

- Prefer well-maintained, permissively licensed (MIT/Apache-2.0) dependencies (`deny.toml` lists the allowed
  licences). Commit `Cargo.lock` and `package-lock.json`; install with `npm ci` in CI.
- Renovate updates dependencies and pins CI actions to commit digests. Gitea Actions needs the v3 artifact actions.
- Updating the pinned llama.cpp release (`crates/pitwall-core/src/runtimes/llama-runtimes.json`) is a reviewed change
  that must pass the end-to-end test.
- Use Conventional Commits (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`, `chore:`) so changelogs can be
  generated (`cliff.toml`); `scripts/commit-lint.mjs` checks them. Releases are `v*` tags following semver
  (`docs/releasing.md`). Never add Co-Authored-By or AI attribution lines.
- Record significant architecture decisions as ADRs in `docs/adr/` (numbered, with status, context, decision,
  consequences).

## Style

- Match the surrounding code: naming, comment density (doc comments on public items — `missing_docs` is on —
  comments only where intent isn't obvious), idioms. Rust uses `rustfmt.toml` (max_width 140).
- UI: standalone components, signals, OnPush, CSS custom properties for theme tokens (dark and light), accessible
  markup (real buttons, labels, aria-labels on icon buttons), the `pw` selector prefix.
- UI building blocks: use **Angular CDK** for behaviour and accessibility (menus, overlays, dialogs, listbox,
  scrolling) instead of hand-rolling them, and **Angular Material** selectively for complex inputs, themed through
  Pitwall's CSS tokens. Icons use Material's `MatIconRegistry` with Pitwall's own SVGs registered as literals in the
  `pw` namespace (`<mat-icon svgIcon="pw:models">`); no icon fonts and no icon URLs fetched at run time (the CSP
  forbids it). Charts use Apache ECharts with the CVD-checked palette.
- Status colours always come with words or shapes; validate colours for contrast and colour-vision deficiency.

## Working practices

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
