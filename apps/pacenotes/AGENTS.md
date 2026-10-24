# Pacenotes — engineering rules for coding agents and contributors

Pacenotes understands code (main use case: understand any new codebase, in any language, in tiers: text, syntax, semantic): it indexes a repository, navigates and reviews it, draws and traces it, and serves all of
that to local AI over MCP. The stack follows the Paddock family: a Rust core, a Tauri 2 shell and an Angular UI.
**No product code exists yet**; the design is `docs/proposal.md` (open questions pending) and the structure is in
`docs/architecture-direction.md`. Read both and `docs/suite-lessons.md` before building.

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
  up Pitwall. Use the shared Paddock Pitwall client and batch priority for bulk AI work.
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
- Releases are `v*` tags following semver, published with `scripts/release/publish.mjs`. Never tag a `[skip ci]` commit;
  read the release lessons in `docs/suite-lessons.md`.
- Record significant architecture decisions as ADRs in `docs/adr/`.

## Style

- Match the surrounding code; Rust uses `rustfmt.toml` (max_width 140). Doc comments on public items.
- UI: standalone components, signals, OnPush, CSS custom properties for theme tokens (dark and light, plus high
  contrast), accessible markup. Use Angular CDK for behaviour and Angular Material selectively; icons through
  `MatIconRegistry` with Pacenotes's own SVGs, no icon fonts or runtime icon URLs.
- Status and review colours always come with words; validate palettes for contrast and colour-vision deficiency.

## Working practices

- Research official sources before choosing models or libraries; record what you found and when in the ADR.
- Navigate code with Serena's symbol tools when available. Delegate mechanical work to cheaper models; keep
