# Dyno — engineering rules for coding agents and contributors

Dyno measures, explains and tunes your PC: a pure Rust core (`crates/dyno-core`), OS adapters
(`crates/dyno-platform`), a background agent that samples, keeps history and runs rules (`crates/dyno-agent`), a
minimal privileged helper (`crates/dyno-helper`), one small IPC (`crates/dyno-ipc`) shared with out-of-process plugins
(`crates/dyno-plugin-api`, `crates/dyno-plugin-host`), a Tauri 2 shell (`app/src-tauri`) and an Angular UI
(`app/src`). See README.md for what it does and `docs/adr/` for why it is built this way.

## Paddock rules (binding)

The family's binding rules live once, in the monorepo's root [AGENTS.md](../../AGENTS.md) and
[docs/rules.md](../../docs/rules.md) (ADR 0014 §10, rule 8: no per-app copies). Read them before this file; where a
rule here disagrees, they win. This file holds only what is specific to Dyno.

## Design principles

Apply these deliberately. They are not decoration:

- **Single responsibility.** One reason to change per module, type and component. Split a file when it starts doing
  several jobs (e.g. sampling vs. storage vs. rules); keep request/response types apart from the logic that produces
  them.
- **Open/closed.** Extend by adding, not editing: a new OS is a new `dyno-platform` adapter behind the
  `dyno_core::ports` traits; a new cleanup category is a catalog entry (or a plugin); new behaviour for users is
  configuration (rules, profiles, settings), not hard-coded special cases.
- **Liskov.** Every implementation of a port (`SystemSensors`, `ProcessSource`, `ProcessControl`, `FileLocks`,
  `StartupItems`, `FactsSource`, `UserActivity`) honours the documented contract: same error semantics
  (`PlatformError`), unavailable readings with a reason instead of zeros, no surprising panics.
- **Interface segregation.** Keep traits and TypeScript interfaces small and purpose-built.
- **Dependency inversion / code against abstractions.**
  - The agent depends on the `dyno_core::ports` traits, never on an OS API.
  - The Tauri shell talks to the agent only through `dyno_core::protocol` over `dyno-ipc`.
  - Angular components never call Tauri `invoke` directly: they go through the backend injected by the `BACKEND`
    token (real Tauri backend or the mock for browser dev/tests), and state lives in SignalStores.
- **Pure core, thin edges.** Grouping, diagnosis, anomaly detection, forecasts, the rules engine, protection,
  cleanup policy, health scoring, tuning plans, alerts, downsampling, sampling intervals and the tray icon are pure
  functions in `dyno-core`, with time passed in as values. Processes, files, the registry and the network stay thin
  in the platform adapters and the agent.
- **Ports and adapters.** The core never depends on the agent, the platform crate, Tauri or an OS API.
- **Parse, don't validate.** Convert untrusted input (IPC JSON from the webview, frames from other processes,
  settings and rule files, plugin proposals, command output such as `lsof` and `powercfg`) into typed values once, at
  the edge (`ProcessRef`, `ProcessAction`, `RuleSet::validate`, `HelperAction::validate`), and work with the types
  afterwards.
- **Make illegal states unrepresentable.** Prefer enums and newtypes over strings, flags and `Option` combinations
  that can disagree (e.g. `Reading<T>` instead of a value plus an "available" flag; `ProcessRef` = pid + start time).
- **Command–query separation.** Live data, history, diagnosis, health and scans have no side effects; only actions,
  rule changes, cleanup runs, tuning, startup changes and settings mutate.
- Also: separation of concerns, high cohesion and low coupling, KISS, YAGNI, DRY with the rule of three before
  abstracting, composition over inheritance, Law of Demeter, least astonishment (sensible defaults, everything off
  until enabled), fail fast with a message that tells the user what to fix, and leave touched code better than you
  found it.

## Safety

Dyno can end, suspend, throttle and re-prioritise processes, delete files and change system settings. Therefore:

- **Never act without an enabled rule or an explicit click.** Rules ship disabled. Scheduled cleanups run only the
  categories the user enabled. Diagnosis and health only *suggest*; a suggestion runs when the user clicks it.
- **The protected list is absolute** (`dyno_core::protect`): the OS's critical processes, the antivirus, the audio
  engine, Dyno itself and the user's entries are never touched — not by rules, not by clicks. Rules never touch the
  foreground app.
- **Every action is logged and undoable** where the OS allows (`dyno_core::activity`): rule actions are released
  when the rule stops holding, when the app exits and when Dyno stops (`ProcessControl::release_all`, also after a
  crash: named CPU-cap jobs are re-opened and released at the next start).
- **Every action targets a `ProcessRef`** (pid + start time), so a reused pid is never hit.
- **Deletes are recoverable**: Recycle Bin / Trash or a 7-day quarantine; nothing outside the category's roots,
  never links, never personal folders unless the user picked a personal category's item; previews show every item
  and its size before anything is deleted.
- **System changes only through the privileged helper** (`crates/dyno-helper`, ADR 0006): a closed set of
  `HelperAction`s, each previewed, approved with the OS prompt, validated again in the helper and recorded with its
  inverse. The app and the agent never run elevated.
- **Tests act only on what they create.** Dummy processes the test spawns, temp folders it creates, registry keys
  under a test key it deletes afterwards. Never throttle, suspend, kill or re-prioritise anything else on the
  developer's or CI machine (the macOS runner is a person's real Mac).

## Security and robustness

Dyno reads untrusted data (process names, command lines, file names, plugin output) and renders it:

- Never trust the webview: every request is parsed into `dyno_core::protocol::Request` and validated in Rust (the
  agent re-validates). Never trust another process: frames are bounded (`MAX_FRAME`) and authenticated with a
  per-launch token.
- Tauri capabilities stay least-privilege: only the plugins and permissions the UI uses, no `fs`/`shell` wildcards,
  no remote content. Keep the CSP free of `unsafe-eval` and remote origins. Never bind process or file names with
  `[innerHTML]`.
- Parsers never panic on any input and enforce limits (settings size, rule and protected-list counts, detail list
  caps, scan item caps, frame size).
- No `unwrap()`/`expect()` on user data or I/O outside tests (clippy warns on both). Library errors are typed; the
  app edge converts them to messages for the UI.
- External tools (`powercfg`, `lsof`, `codesign`, `docker`, `schtasks`) always run with timeouts through
  `dyno_platform::process` and without a console window on Windows.
- Long-running work (scans, details, facts) runs off the sampling thread and off the UI thread; results are paged or
  capped.
- Logs never contain file contents, command lines or secrets by default; any crash reporting is opt-in. Secrets
  (the local web API key) live only in the OS credential store.

## Languages

- All code that ships inside Dyno is Rust (core, platform, agent, helper, plugins, Tauri shell) plus the Angular UI.
  Don't add C#, C++, Python, PowerShell or other runtimes to the product.
- Plugins are Rust only and always run out of process: a plugin depends on `dyno-plugin-api` alone, is served by
  `dyno_plugin_api::serve`, and talks to Dyno over the one IPC (`dyno-ipc`: length-prefixed postcard frames on a
  private local stream).
- `unsafe` is denied workspace-wide. The OS integrations that need FFI get one small module each with
  `#[allow(unsafe_code)]` and a `// SAFETY:` comment on every `unsafe` block, and tests on that platform's CI runner.

## Where logic lives

- **The Rust core is the single source of truth** for behaviour: grouping, diagnosis, anomalies, forecasts, rules,
  protection, cleanup safety, health, tuning, alerts, reports, history resolution. The agent carries out the core's
  decisions; the app shows them.
- **Angular is the presentation layer**: layout, interaction state, rendering and formatting of what the agent
  returns. It must not re-implement domain rules. If the UI needs a computation, add it to the core and expose it
  through the protocol. Purely visual transforms (a process tree from parent links, chart series) are fine.
- **The mock backend** exists for browser development and UI tests only. It is never the reference for behaviour,
  and features are not "done" until they work against the real agent.

## Robustness

- **Contain failures.** One bad process, file, plugin or sensor never takes down sampling or the app. Plugins run
  out of process with timeouts and memory caps and are quarantined after repeated failures. The agent is a separate
  process: if it dies the app shows it and restarts it; if the app dies the agent exits and releases what rules did.
- **Degrade, don't fail.** Missing sensors, missing rights, locked files and unavailable tools produce a clear
  message (`Reading::Unavailable`, `PlatformError` with a next step) instead of an exception.
- **Recoverable state.** Settings, rules and the activity log are versioned, written atomically with a backup, and
  loading tolerates damage (falls back to the backup, then the defaults). The history database tolerates being
  deleted.
- **Bounded resources.** Respect the existing limits (history retention and downsampling, detail caps, scan caps,
  frame size, Dyno's own CPU budget of ~1 %) and add limits for any new unbounded input.
- **Every robustness behaviour has a test that triggers the failure** (crashing or hanging plugins, corrupt
  settings, a reused pid, a protected process, locked files, a stale scan, a missing sensor).

## Working in parallel (agents)

- Each task owns an explicit set of files; don't edit outside it. Shared integration points (`lib.rs` files, the
  workspace `Cargo.toml`, `dyno_core::protocol`, `dyno_core::ports`, `api.types.ts`) are changed by the integrating
  agent only, or by adding a single declared line.
- Don't run `cargo fmt --all` / `cargo clippy --fix` across the workspace while others are editing; format your own
  files (`rustfmt --edition 2021 <files>`). Use a private `CARGO_TARGET_DIR`.
- Commit only complete, owned work. Never commit a workspace `Cargo.toml` listing crates whose sources aren't
  committed.
- Windows-only and macOS-only code is linted and tested on its own OS's CI runner (cross-checking from another host fails on
  the bundled SQLite's C build, and on macOS needs the Windows SDK). Write it carefully and read the runner's log.

## Contracts

- The UI ↔ app wire contract is `app/src/app/core/api.types.ts` (camelCase JSON), mirrored by `dyno_core`'s serde
  types (`protocol.rs` and the types it names). The app ↔ agent contract is `dyno_core::protocol` (with
  `PROTOCOL_VERSION`). The plugin contract is `dyno_plugin_api` (with its API version). Change both sides of a
  contract in the same commit.
- The settings schema is `dyno_core::settings` (with `SCHEMA_VERSION`). New keys are optional with defaults; a
  breaking change bumps the version and adds a migration with a test.

## Testing and quality gates

- Every change ships with tests. Every bug fix starts with a failing regression test.
- Coverage targets: `dyno-core` ≥ 99% lines (`cargo llvm-cov -p dyno-core --summary-only`), Angular ≥ 80% lines
  overall and near 100% for stores, services and pure utilities.
- Coverage is necessary, not sufficient: assert behaviour, include edge cases and error paths. Platform adapters are
  tested on their OS's CI runner against processes and files the tests create.
- UI tests are layered: Vitest + TestBed for stores, services and components; Playwright flows and screenshots
  against the browser build with the mock backend (Chromium and WebKit).
- Before committing, all of these must pass:

  ```sh
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  cargo deny --workspace check
  node --test scripts/check-capabilities.test.mjs
  node scripts/check-capabilities.mjs
  (cd app && npm audit --omit=dev --audit-level=high && npx ng lint && npx ng build && npx ng test --watch=false)
  ```

## Dependencies and releases

- Prefer well-maintained, permissively licensed (MIT/Apache-2.0) dependencies (`deny.toml` lists the allowed
  licences). Commit `Cargo.lock` and `package-lock.json`; install with `npm ci` in CI.
- Renovate updates dependencies and pins CI actions to commit digests. Gitea Actions needs the v3 artifact actions.
- Use Conventional Commits (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`, `chore:`) so changelogs can be
  generated (`cliff.toml`); `paddock-conformance commit-lint` checks them (CI's `commits` job on pull requests). Never add Co-Authored-By or AI attribution
  trailers. Releases are `dyno-v*` tags following semver (`docs/releasing.md`).
- Record significant architecture decisions as ADRs in `docs/adr/` (numbered, with status, context, decision,
  consequences).

## Style

- Match the surrounding code: naming, comment density (doc comments on public items — `missing_docs` is on —
  comments only where intent isn't obvious), idioms. Rust uses `rustfmt.toml` (max_width 140).
- UI: standalone components, signals, OnPush, CSS custom properties for theme tokens (dark and light), accessible
  markup (real buttons, labels, aria-labels on icon buttons), the `dy` selector prefix.
- UI building blocks: use **Angular CDK** for behaviour and accessibility (menus, overlays, dialogs, listbox,
  scrolling) instead of hand-rolling them, and **Angular Material first** (rule 47: a Material component wherever one fits), themed through
  Dyno's CSS tokens. Icons use Material's `MatIconRegistry` with Dyno's own SVGs registered as literals in the `dy`
  namespace; no icon fonts and no icon URLs fetched at run time (the CSP forbids it).
- Charts use Apache ECharts with the CVD-checked categorical palette. Status colours always come with words or
  numbers; validate colours for contrast (WCAG AA) and colour-vision deficiency.

## Working practices

- Research official sources before choosing libraries or relying on an OS behaviour (performance counters, job
  objects, Restart Manager, `taskpolicy`), and record what you found and when in the ADR or doc comment.
- Navigate code with Serena's symbol tools (find symbol, references, overview) when they are available instead of
  reading whole files; it saves context and cost. The project config is `.serena/project.yml`; starter memories are
  in `.serena/memories/`.
- Delegate mechanical or well-specified work (refactors with a precise spec, test scaffolding, docs) to cheaper
  models. Keep architecture and review on the main model.

## Lessons from the suite

Read [docs/suite-lessons.md](docs/suite-lessons.md) before changing build, bundle, window, service, test or CI code:
it lists the pitfalls Debrief and Pitcrew already hit (CSP and inline styles, macOS signing and drag regions,
launchd/systemd PATH, Remote Desktop sessions, conhost, Linux threads, test hygiene, release-build budgets).
