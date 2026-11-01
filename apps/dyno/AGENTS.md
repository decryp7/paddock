# Dyno — engineering rules for coding agents and contributors

Dyno watches your PC and changes nothing on it ([ADR 0021](docs/adr/0021-monitoring-only.md)): a pure Rust core (`crates/dyno-core`), OS adapters
(`crates/dyno-platform`), a background agent that samples, keeps history and raises alarms (`crates/dyno-agent`), one small IPC (`crates/dyno-ipc`) shared with out-of-process plugins
(`crates/dyno-plugin-api`, `crates/dyno-plugin-host`), a Tauri 2 shell (`app/src-tauri`) and an Angular UI
(`app/src`). See README.md for what it does and `docs/adr/` for why it is built this way.

## Paddock rules (binding)

The family's binding rules live once, in the monorepo's root [AGENTS.md](../../AGENTS.md) and
[docs/rules.md](../../docs/rules.md) (ADR 0014 §10, rule 8: no per-app copies). Read them before this file; where a
rule here disagrees, they win. This file holds only what is specific to Dyno.

## Design principles

Apply these deliberately. They are not decoration:

- **Single responsibility.** One reason to change per module, type and component. Split a file when it starts doing
  several jobs (e.g. sampling vs. storage vs. alarms); keep request/response types apart from the logic that produces
  them.
- **Open/closed.** Extend by adding, not editing: a new OS is a new `dyno-platform` adapter behind the
  `dyno_core::ports` traits; a new view is a port and an adapter (or a sensor plugin); new behaviour for users is
  configuration (settings), not hard-coded special cases.
- **Liskov.** Every implementation of a port (`SystemSensors`, `ProcessSource`, `ProcessControl`, `FileLocks`,
  `StartupItems`, `FactsSource`, `UserActivity`) honours the documented contract: same error semantics
  (`PlatformError`), unavailable readings with a reason instead of zeros, no surprising panics.
- **Interface segregation.** Keep traits and TypeScript interfaces small and purpose-built.
- **Dependency inversion / code against abstractions.**
  - The agent depends on the `dyno_core::ports` traits, never on an OS API.
  - The Tauri shell talks to the agent only through `dyno_core::protocol` over `dyno-ipc`.
  - Angular components never call Tauri `invoke` directly: they go through the backend injected by the `BACKEND`
    token (real Tauri backend or the mock for browser dev/tests), and state lives in SignalStores.
- **Pure core, thin edges.** Grouping, diagnosis, anomaly detection, forecasts,
  health scoring, alerts, downsampling, sampling intervals and the tray icon are pure
  functions in `dyno-core`, with time passed in as values. Processes, files, the registry and the network stay thin
  in the platform adapters and the agent.
- **Ports and adapters.** The core never depends on the agent, the platform crate, Tauri or an OS API.
- **Parse, don't validate.** Convert untrusted input (IPC JSON from the webview, frames from other processes,
  settings files, plugin readings, command output such as `lsof` and `powercfg`) into typed values once, at
  the edge (`ProcessRef`, the typed `Request`), and work with the types
  afterwards.
- **Make illegal states unrepresentable.** Prefer enums and newtypes over strings, flags and `Option` combinations
  that can disagree (e.g. `Reading<T>` instead of a value plus an "available" flag; `ProcessRef` = pid + start time).
- **Command–query separation.** Live data, history, diagnosis and health have no side effects; only settings,
  alarm acknowledgements and Dyno's own data mutate.
- Also: separation of concerns, high cohesion and low coupling, KISS, YAGNI, DRY with the rule of three before
  abstracting, composition over inheritance, Law of Demeter, least astonishment (sensible defaults, everything off
  until enabled), fail fast with a message that tells the user what to fix, and leave touched code better than you
  found it.

## Safety

Dyno 1.0.0 only watches ([ADR 0021](docs/adr/0021-monitoring-only.md)). Therefore:

- **Never change the machine.** No process action (end, suspend, cap, priority), no delete, move or restore of files, no
  setting or startup change, no tuning. Code that does any of this is removed, not hidden behind a flag. What Dyno writes is its
  own data: history, settings, alarm acknowledgements, mutes and notes, snapshots, baselines and the exports the user saves.
- **Reads are bounded.** External tools and OS calls run with timeouts; a missing tool or right is an unavailable reading with a
  reason, never an exception.
- **No privileged helper in 1.0** (ADR 0021 point 4, ADR 0022 for 1.1). The app and the agent run with the user's rights and
  never elevate themselves; there is no way to run elevated in 1.0 (elevation is planned for 1.1). A reading the OS withholds is `Reading::Unavailable` with a plain reason,
  never 0; other accounts' processes are one "Other users" row. Any helper that comes back in 1.1 is read-only, validates every
  frame again and audits every call.
- **Plugins are sensors only.** There is no cleanup or action provider.
- **Tests act only on what they create.** Dummy processes the test spawns, temp folders it creates, registry keys
  under a test key it deletes afterwards. Never touch, kill or inspect anything else in a way that could change the
  developer's or CI machine (the macOS runner is a person's real Mac).

## Security and robustness

Dyno reads untrusted data (process names, command lines, file names, plugin output) and renders it:

- Never trust the webview: every request is parsed into `dyno_core::protocol::Request` and validated in Rust (the
  agent re-validates). Never trust another process: frames are bounded (`MAX_FRAME`) and authenticated with a
  per-launch token.
- Tauri capabilities stay least-privilege: only the plugins and permissions the UI uses, no `fs`/`shell` wildcards,
  no remote content. Keep the CSP free of `unsafe-eval` and remote origins. Never bind process or file names with
  `[innerHTML]`.
- Parsers never panic on any input and enforce limits (settings size, detail list
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

- All code that ships inside Dyno is Rust (core, platform, agent, plugins, Tauri shell) plus the Angular UI.
  Don't add C#, C++, Python, PowerShell or other runtimes to the product.
- Plugins are Rust only and always run out of process: a plugin depends on `dyno-plugin-api` alone, is served by
  `dyno_plugin_api::serve`, and talks to Dyno over the one IPC (`dyno-ipc`: length-prefixed postcard frames on a
  private local stream).
- `unsafe` is denied workspace-wide. The OS integrations that need FFI get one small module each with
  `#[allow(unsafe_code)]` and a `// SAFETY:` comment on every `unsafe` block, and tests on that platform's CI runner.

## Where logic lives

- **The Rust core is the single source of truth** for behaviour: grouping, diagnosis, anomalies, forecasts,
  health, alerts, reports, history resolution. The agent carries out the core's
  decisions; the app shows them.
- **Angular is the presentation layer**: layout, interaction state, rendering and formatting of what the agent
  returns. It must not re-implement domain rules. If the UI needs a computation, add it to the core and expose it
  through the protocol. Purely visual transforms (a process tree from parent links, chart series) are fine.
- **The mock backend** exists for browser development and UI tests only. It is never the reference for behaviour,
  and features are not "done" until they work against the real agent.

## Robustness

- **Contain failures.** One bad process, file, plugin or sensor never takes down sampling or the app. Plugins run
  out of process with timeouts and memory caps and are quarantined after repeated failures. The agent is a separate
  process: if it dies the app shows it and restarts it; if the app dies the agent keeps measuring.
- **Degrade, don't fail.** Missing sensors, missing rights, locked files and unavailable tools produce a clear
  message (`Reading::Unavailable`, `PlatformError` with a next step) instead of an exception.
- **Recoverable state.** Settings and the alarm log are versioned, written atomically with a backup, and
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
- Renovate updates dependencies and pins CI actions to commit digests. Gitea 28 lists only v4 artifacts, so CI uses the Gitea fork of the v4 actions (ChristopherHX/gitea-upload-artifact, gitea-download-artifact; docs/research/2026-11-ci-artifacts-v4.md).
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
- The UI runs on the Paddock app shell (ADR 0023): pages, status items, commands, settings pages and dialogs are
  contributions in `app/src/app/shell/contributions.ts`; the frame, the palette, notifications, jobs, Settings, About and the
  tray popover are the shell's. A dialog is an OS window: its contract is Rust's (`src-tauri/src/dialogs.rs`, generated to
  `generated/dialog-contracts.ts`), its component declares `dialogActions` and draws no footer, and a confirmation is the
  shared `pd.confirm`. Never open a CDK dialog or write a toast; use `DialogService` and `Notifier`. The legacy `--surface-*`,
  `--text-*`, `--sp-*` tokens in `styles.scss` are a bridge over `--pd-*`: new code uses `--pd-*`.
- UI building blocks: use **Angular CDK** for behaviour and accessibility (menus, overlays, listbox, scrolling) instead of
  hand-rolling them, and **Angular Material first** (rule 47: a Material component wherever one fits), themed through
  the shared `--pd-*` tokens. Icons use Material's `MatIconRegistry` with Dyno's own SVGs registered as literals in the `dy`
  namespace; no icon fonts and no icon URLs fetched at run time (the CSP forbids it).
- Charts use Apache ECharts with the CVD-checked categorical palette. Status colours always come with words or
  numbers; validate colours for contrast (WCAG AA) and colour-vision deficiency.

## Working practices

- Record every benchmark you quote (rule 48): first as a results block in the app's `docs/performance.md` (shared code:
  `docs/benchmarks.md`), then link it from the ADR, plan, CHANGELOG, commit or PR that quotes the number.
- Specs never wait a fixed number of event-loop turns and never leave a patch behind (rule 49): wait on the condition
  (Debrief's `openDialog()`, once `fix/debrief-import-range-flake` lands), never let an optional chain perform a test's
  action, and patch a prototype only with `vi.spyOn` (restored after each test).
- Spend CI only on what a change needs (rule 50): one run per branch head, none while review fixes are pending or for a
  branch already merged elsewhere, none for docs-only changes; branches run the affected plan, full gates once on main.
- A change that touches an app's release path builds that app's installers before it lands on main (rule 54): the batch
  dispatches `<app>-release.yml` with gates on and the bundle built; passing tests alone is not enough.
- One implementation per mechanism (rule 55): a mechanism listed in `docs/known-copies.toml` is fixed in its canonical
  implementation and every copy in the same change, each with a failing test first, or a copy is waived with
  `Copies-waived: <id> <path>: <reason>` in a commit that touches it (`*` only for a mechanism of at most two members
  besides wiring); a freeze does not stop such a fix; a new copy needs its entry and a delete-by tag, an entry leaves
  only with its files (or `Copies-removed:`), and a date moves only with `Copies-date: ... approved: <coordinator|user>`
  (`user` for a later release line); register check `paddock-conformance.mjs copies --check-register` (ADR 0034, C58).
- Logs explain every failure (rule 56): a fix or feature that starts, connects, runs a job or can fail leaves the logs able
  to show the cause from a Report-a-problem bundle alone (phases with timestamps and durations, spawn, connect and job
  outcomes including success, correlation ids, errors with cause chain and next step); a bug the logs could not explain
  gets the missing lines in the same change, with a test where practical; no user content in logs by default. Before fixing,
  establish the real flow from the logs (reproduce, read the phases, follow the code); if the logs cannot show it, add the
  logging first and reproduce again; a fix based on a guessed flow is not done.
  The same holds for CI, release pipelines, dev tools and test harnesses: a failing step prints one `::error::` line with
  cause and next step; long steps log phases with durations; wrappers log the exact command (secrets masked), exit code (NTSTATUS on
  Windows) and stderr tail; each job summarises what ran, was skipped and why.
- The wiki is the current-state summary (rule 59, ER-30): `docs/wiki/` links its sources and never restates a rule; only the
  coordinator or merge steward writes it, from the `Wiki notes:` line that ends every agent report; new pages are
  fact-checked independently; the writer runs the wiki lint (rule 59) with the rules on main, each open rule branch and
  the ER text before every wiki commit, and CI runs it once the Rust `wiki` CLI lands.
- Slow means two questions (rule 51): can more threads or workers run it (bounded by the kind of work), and can it be made
  cheaper (less work per item, no repeats, a cheaper algorithm or format, caching, batching, skipping)? Measure both;
  thread counts are Settings with an "Auto" default; a long or bulk operation's design has a "Parallelism and cost" section.
- Never act on a reusable OS name read earlier (rule 53): hold it (handle, job, listener) or verify its identity (pid plus
  start time, `paddock_proc::ProcessId`) immediately before acting; kills go through `paddock_proc` (Clippy
  `disallowed-methods` and C53 check it, ADR 0033). A discovery file that names a pid or port (Dyno's `agent.json`) is
  only a hint: act on it after a held lock or a live connection proves the process is there, and remove it only if it is
  still the same file (same pid and start time).
- Work a person waits for never runs at an OS's starvable background level (rule 52: macOS QoS `BACKGROUND`,
  `PRIO_DARWIN_BG`, `taskpolicy -b`/`-c background`/`-c maintenance`/`-d throttle` and `IOPOL_THROTTLE`; Windows
  background modes, idle priorities and EcoQoS; Linux `SCHED_IDLE` and idle I/O; for a service, launchd `ProcessType`
  `Background` (or `Adaptive` without XPC) and `LowPriorityIO`, systemd `CPUSchedulingPolicy=idle` and
  `IOSchedulingClass=idle`): bulk work (long, with progress and Cancel: walks, scans, compaction, exports) uses
  `QOS_CLASS_UTILITY`, nice 10 or `THREAD_PRIORITY_BELOW_NORMAL`; live work (a person waits on it within a deadline or
  for a screen to fill: requests, probes, a sampler, a page refresh) may use the first two but stays at the normal level
  on Windows; a thread holding a lock that live work waits on is live, or must not hold the lock across slow work; a
  starvable level only for work nobody waits on, with a `conformance: background-qos — <why>` comment (C54).
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
