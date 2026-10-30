# Pacenotes — engineering rules for coding agents and contributors

Pacenotes understands code (main use case: understand any new codebase, in any language, in tiers: text, syntax, semantic): it indexes a repository, navigates and reviews it, draws and traces it, and serves all of
that to local AI over MCP. The stack follows the Paddock family: a Rust core, a Tauri 2 shell and an Angular UI.
**No product code exists yet**; the design is `docs/proposal.md` (open questions pending) and the structure is in
`docs/architecture-direction.md`. Read both and `docs/suite-lessons.md` before building.

## Paddock rules (binding)

The family's binding rules live once, in the monorepo's root [AGENTS.md](../../AGENTS.md) and
[docs/rules.md](../../docs/rules.md) (ADR 0014 §10, rule 8: no per-app copies). Read them before this file; where a
rule here disagrees, they win. This file holds only what is specific to Pacenotes.

## Design principles

Apply these deliberately. They are not decoration:

- **Single responsibility, open/closed, Liskov, interface segregation.** One reason to change per module; extend by
  adding (a new language pack, rule pack or diagram kind is a new implementation behind a small trait); every
  implementation honours the trait's documented contract, with the same error semantics and no surprising panics.
- **Dependency inversion / ports and adapters.** The core depends on no adapter. Tauri commands depend on traits, never
  on a concrete store, git library or LLM client. Angular components never call Tauri `invoke` directly: they use the
  backend injected by DI token (real or mock), and state lives in SignalStores.
- **Pure core, thin edges.** Query semantics, ranking, repo-map budgeting, rule matching, diagram layout and formatting are
  pure functions with time and randomness passed in. Parsing is pure; files, git, network and the OS stay thin.
- **Parse, don't validate; make illegal states unrepresentable.** Convert untrusted input (source files, documents, IPC JSON, API
  requests, project files) into typed values once, at the edge. Prefer enums and newtypes over flags and `Option`
  combinations that can disagree (e.g. a finding's state is an enum).
- **Command–query separation.** Search, review, trace and diagram queries have no side effects; index, link, suppress, write-diagram and delete
  mutate.
- Also: KISS, YAGNI, DRY with the rule of three, composition over inheritance, Law of Demeter, least astonishment, fail
  fast with a message that tells the user what to fix, and leave touched code better than you found it.

## Pacenotes-specific rules

- **Never execute repository code.** No build scripts, no tests, no language servers, no package managers, no
  generators against the analysed repository, unless the user explicitly opts in for that workspace. The only
  subprocesses are our own workers, git (in-process), the optional draw.io export (with a timeout), the user's editor and, only after "trust this repository", the supervised language
  server of a tier 3 language pack (proposal section 20.4).
  Decision 2026-10-23: "Trust this repository" is the only exception to this rule. It is off by default, per repository, set by an explicit user action
  (prompt, setting or CLI flag), local to this machine, revocable, and never inferred or shared. Untrusted repositories use a pre-built SCIP index or tiers 1-2.
- **Read-only towards repositories.** Pacenotes writes only inside its own data folder and to destinations the user
  picks (a diagram file, an export). Only workspaces on the user's allow-list are readable; paths are canonicalized and
  symlinks cannot escape the workspace. Tools that write are never exposed over MCP.
- **Secrets are redacted from tool output.** A secret denylist keeps files like `.env` and key material out of the
  index, and anything that looks like a credential is masked before it reaches an MCP reply, a log or the UI.
- **Local only.** No telemetry, no cloud. MCP is served over loopback (or stdio) with per-client keys, bounded request
  sizes and per-key grants and budgets. Logs contain no source text by default.
- **Vector search is for Pacenotes only, and hybrid.** Decision of 2026-10-23: Pacenotes keeps a local, per-repository, opt-in vector index for semantic code
  search, fused with text and structural search and behind a `VectorIndex` port; embeddings come from Pitwall, a small local model, or not at all. Debrief
  deliberately has no vector search. Everything stays local.
- **Plugin architecture.** The functions (Understand, Review, Diagrams, Requirements, Tests & coverage, Debrief bridge,
  language packs) are out-of-process Rust plugins that follow Debrief's ADR 0017 model: a plugin depends on the plugin
  API crate alone, talks to the core over the one versioned IPC (length-prefixed postcard frames on a private local
  stream), is guarded (timeouts, memory caps, panics become errors attributed to the plugin), is quarantined after
  repeated faults, and is signed. Plugins contribute declarative views that the core renders; **a plugin never supplies
  web code**. The shared plugin host and IPC are to be extracted into Paddock; copy Debrief's approach and do not depend
  on Debrief crates. No SDKs for other languages, no gRPC and no in-process plugins.
- **Licences of code and rules.** No GPL, AGPL, LGPL or source-available code or rule sets: no Semgrep rules, no
  SonarQube analyzers, no universal-ctags, no Doxygen, no Serena app (GPL), no Qodo Cover, no Doorstop, no PlantUML.
  drawio-desktop may only be launched as an external program, never linked or bundled. Record the licence of every
  dependency, grammar, rule pack and model, verified against the official repository, in the ADR that adopts it.
- **Diagram outputs preserve the user's manual edits.** Generated draw.io files are canonical and round-trip safe:
  regenerating merges with hand edits (positions, styles, added shapes) and never overwrites them; stale elements are
  flagged, not silently deleted.
- **Pitwall is optional.** Every function works without it, or says clearly that it needs local AI and links to setting
  up Pitwall. Use the shared Paddock Pitwall client. No LLM per item for bulk work (family rule 46): indexing is Model2Vec + BM25 on the CPU;
  Qwen only re-ranks a query's top hits and, as the opt-in per-repository "Deep index with Qwen" job, embeds a whole
  repository after a confirmation showing the time and size estimate (ADR 0001, amended 2026-10-28).
- **Boundary with Sighter.** Sighter owns UI test execution; Pacenotes owns requirements and traceability and may draft
  test plans for Sighter, never run them.

## Security and robustness

- Never trust the webview. Validate every IPC argument and every API request in Rust. The server binds loopback only,
  with bounded request sizes and file sizes. Open only paths the user chose; canonicalize them.
- Tauri capabilities stay least-privilege: only the plugins and permissions the UI uses, no `fs`/`shell` wildcards, no
  remote content. Keep the CSP free of `unsafe-eval` and remote origins. Never bind untrusted text with `[innerHTML]`.
- Local only: no telemetry and no cloud. Crash reporting, if any, is opt-in. Logs contain no source text or
  secrets by default.
- Parsers and decoders never panic on any input and enforce limits (file size, document size, index size). No
  `unwrap()`/`expect()` on user data or I/O outside tests. Library errors are typed; the app edge converts them to
  messages for the UI.
- Parsing, indexing and AI calls run out of process or off the UI thread, with timeouts; a parser or plugin fault never
  takes down the app or the service. Large results are paged.
- Contain failures, degrade with a clear next step, version and migrate persistent state, tolerate missing or corrupt
  files, and bound every unbounded input. Every robustness behaviour has a test that triggers the failure.
- Never await IPC before the first paint; put a plain splash in `index.html` (see `docs/suite-lessons.md`).

## Where logic lives

The Rust core is the single source of truth for behaviour. Angular is the presentation layer and must not re-implement
domain rules; if the UI needs a computation, add a core operation and a command. The mock backend is for browser
development and UI tests only; features are not done until they work against the real engine.

## Working in parallel (agents)

- Each task owns an explicit set of files; don't edit outside it. Shared integration points (`lib.rs` files, the
  workspace `Cargo.toml`, `api.types.ts` / DTOs) are changed by the integrating agent only.
- Don't run `cargo fmt --all` / `cargo clippy --fix` across the workspace while others are editing; use
  `rustfmt <files>` and a private `CARGO_TARGET_DIR`. Max 5 parallel agents, `CARGO_BUILD_JOBS=2`.
- Commit only complete, owned work. Windows- and macOS-only code is checked on its own CI runner.

## Testing and quality gates

- Every change ships with tests; every bug fix starts with a failing regression test. Targets: core ≥ 99% lines,
  Angular ≥ 80% overall and near 100% for stores, services and pure utilities. Coverage is necessary, not sufficient.
- Before committing, once the code exists, these must pass:

  ```sh
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  cargo deny --workspace check
  node --test scripts/release/publish.test.mjs
  (cd app && npm audit --omit=dev --audit-level=high && npx ng lint && npx ng build && npx ng test --watch=false)
  ```

## Dependencies, commits and releases

- Prefer well-maintained, permissively licensed (MIT/Apache-2.0) dependencies; `deny.toml` lists the allowed licences.
  Commit `Cargo.lock` and `package-lock.json`; CI uses `npm ci`. Renovate updates dependencies.
- Use Conventional Commits (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`, `chore:`). **Never add a
  Co-Authored-By trailer or any AI attribution** to commits or PRs.
- **Never run `npx prettier --write`**: the TypeScript is not prettier-formatted and it reformats whole files.
- Releases are `pacenotes-v*` tags following semver, published with `scripts/release/publish.mjs`. Never tag a `[skip ci]` commit;
  read the release lessons in `docs/suite-lessons.md`.
- Record significant architecture decisions as ADRs in `docs/adr/`.

## Style

- Match the surrounding code; Rust uses `rustfmt.toml` (max_width 140). Doc comments on public items.
- UI: standalone components, signals, OnPush, CSS custom properties for theme tokens (dark and light, plus high
  contrast), accessible markup. Use Angular CDK for behaviour and Angular Material first (rule 47); icons through
  `MatIconRegistry` with Pacenotes's own SVGs, no icon fonts or runtime icon URLs.
- Status and review colours always come with words; validate palettes for contrast and colour-vision deficiency.

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
- Research official sources before choosing models or libraries; record what you found and when in the ADR.
- Navigate code with Serena's symbol tools when available. Delegate mechanical work to cheaper models; keep
