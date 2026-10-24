# Spotter — engineering rules for coding agents and contributors

Spotter watches for new releases anywhere (folders and shares, HTTP listings, JSON APIs, forges, feeds, web pages
including JavaScript-rendered ones) and tells you the moment one appears: a Rust core (`crates/spotter-core`), source
adapters (`crates/spotter-sources`), the engine (`crates/spotter-engine`), the renderer (`crates/spotter-render`), a
Tauri 2 shell (`app/src-tauri`) and an Angular UI (`app/src`). It is part of the Paddock suite and is local only: no
cloud, no telemetry. See README.md, `docs/adr/` and `docs/suite-lessons.md` (follow every lesson in it).

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

## Design principles

- **Single responsibility**, **open/closed**, **Liskov**, **interface segregation**, **dependency inversion**: as in
  Pitcrew. New source kinds are new modules behind the `ReleaseSource` port and a catalog entry; new presets and
  version schemes are declarative TOML **packs**, not code. There is no plugin system (ADR 0001).
- **Pure core, thin edges.** Version parsing and ordering, filters, soak time, diffing, scheduling, digest and quiet
  hours, notification policy and config migration are pure functions with time passed in as values. Network, files,
  SQLite, processes and the OS stay thin.
- **Ports and adapters.** The core never depends on an adapter. The Tauri commands depend on engine traits; the
  renderer is used only through the small `PageRenderer` trait (its CDP driver will move to the shared paddock repo).
- **Angular never calls Tauri `invoke` directly**: it goes through the `BACKEND` token (real Tauri backend or mock),
  state lives in SignalStores. **Never await IPC before first paint.** Every Tauri command is `async`.
- **Parse, don't validate; make illegal states unrepresentable; command-query separation.**
- Also: KISS, YAGNI, DRY with the rule of three, composition over inheritance, fail fast with a message that tells the
  user what to fix, leave touched code better than you found it.

## Security and robustness (untrusted pages and feeds)

- Never trust the webview; validate every IPC argument in Rust. Open only paths the user chose; canonicalize them.
- Everything fetched is untrusted: HTTPS preferred, redirects capped, bodies capped (`MAX_BODY`), timeouts on every
  request, and the per-site minimum interval and `ETag`/`Last-Modified` always honoured. No requests to the loopback or
  link-local ranges unless the watch is explicitly local. Never render fetched text with `[innerHTML]`; fetched HTML is
  parsed, never executed, except inside the renderer.
- **Renderer rules (ADR 0002).** Pages run in a separate supervised Chromium process with a throwaway per-watch
  profile, hard caps on time and memory, no extensions, no stealth tricks. Readiness = load finished + element present
  + DOM quiet (not network idle). A challenge (login, captcha) is handed to the user as "Needs you", never solved.
  Tokens and cookies never leave the credential store/profile and are never logged.
- User regexes only through `user_regex::compile` (length and compiled-size caps). Parsers never panic and enforce
  limits. No `unwrap()`/`expect()` on user data or I/O outside tests. Library errors are typed.
- Tauri capabilities stay least-privilege; CSP free of `unsafe-eval` and remote origins. External tools and child
  processes run with timeouts and no console window on Windows. Logs contain no fetched content or secrets.
- Persistent state (config, SQLite) is versioned, written atomically and tolerates missing/corrupt files by falling
  back to defaults. Every robustness behaviour has a test that triggers the failure.

## Languages

All shipped code is Rust plus the Angular UI. `unsafe` is denied workspace-wide. No Python/C#/C++ in the product.

## Where logic lives

The Rust engine is the single source of truth (versions, filters, diffing, scheduling, notifications, previews).
Angular is presentation only; if the UI needs a computation, add an engine operation and a command. The mock backend
exists for browser development and tests only, and tests never use production names, paths or data directories.

## Working in parallel (agents)

Each task owns an explicit set of files. Shared integration points (`lib.rs` files, the workspace `Cargo.toml`,
`api.types.ts` / `dto.rs`) are changed by the integrating agent only. Do not run workspace-wide `cargo fmt`/`clippy
--fix` while others edit; format your own files. Use `CARGO_BUILD_JOBS=2` and a private `CARGO_TARGET_DIR`. Playwright
runs with `--workers=1` on a non-default port (4370). Never leave dev processes running.

## Contracts

The UI-backend contract is `app/src/app/core/api.types.ts` (camelCase JSON), mirrored by `app/src-tauri/src/dto.rs`;
change both in one commit. Config and state are versioned (`SCHEMA`), with migration and a test.

## Testing and quality gates

Every change ships with tests; every bug fix starts with a failing regression test. Targets: `spotter-core` >= 99%
lines, Angular >= 80% overall and near 100% for stores/services/utilities. Before committing:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny --workspace check
(cd app && npm audit --omit=dev --audit-level=high && npx ng lint && npx ng build && npx ng test --watch=false)
```

## Dependencies and releases

Permissive licences only (`deny.toml`). Commit `Cargo.lock` and `package-lock.json`. Conventional Commits (no AI
attribution lines); releases are `v*` tags. Significant decisions are ADRs in `docs/adr/`.

## Style

Rust: `rustfmt.toml` (max_width 140), doc comments on public items. UI: standalone components, signals, OnPush, `sp`
selector prefix, `@decryp7/paddock-ui` (`pd-` prefix) for the shared kit, accent #0E8577, CDK for behaviour, icons
registered as literals (no icon fonts, no runtime URLs). Never run `prettier --write`.
