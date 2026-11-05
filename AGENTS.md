# Paddock: rules for coding agents and contributors

The Paddock app family in one repository (ADR [0014](docs/adr/0014-monorepo-under-paddock.md)): the shared Rust crates
(`crates/`), the shared UI packages `@decryp7/paddock-ui` and `@decryp7/paddock-e2e` (`ui/`), the conformance, ESLint and
TypeScript presets, the family docs (`docs/`) and the apps under `apps/<app>/`. See README.md.

Read this file and the nearest `apps/<app>/AGENTS.md` (an app's own part: its architecture, where its logic lives, its
own checks). Where they disagree, the rules win, then this file, then the app's.

## Paddock rules (binding)

The family's binding rules are [docs/rules.md](docs/rules.md), the one source (ADR 0014 §9: included here by link, not
copied). Read them before any change; their reasons are in [docs/lessons.md](docs/lessons.md).

## Layout

```
crates/          shared Rust crates, one Cargo workspace (crates/Cargo.toml); paddock-slots, paddock-repo-files and
                 paddock-repo-graph are the libraries under tools/paddock-dev
ui/              @decryp7/paddock-ui, @decryp7/paddock-e2e, the gallery and the demo UIs (one Angular workspace)
apps/<app>/      one app each (debrief, dyno, pacenotes, pitcrew, pitwall, sighter, spotter and the two demos), its tree
                 unchanged below its folder, its own Cargo workspace and Cargo.lock, and paddock.ci.json (its CI contract)
conformance/ eslint/ tsconfig/ lint/ icons/ templates/    shared checks, presets, the icon pipeline, new-app templates
docs/            family docs: rules, lessons, ADRs, plans, research; an app's docs stay in apps/<app>/docs
tools/paddock-dev  the Rust developer and CI tool (ADR 0022: its own Cargo workspace and lockfile; run it with `./dev`, which
                 builds or fetches the binary its inputs name)
scripts/         family scripts: affected.mjs (CI's plan), publish.mjs, new-app.mjs, demo-release.mjs
.gitea/          ci.yml (plan and affected jobs), nightly.yml, each app's <app>.yml, the shared actions
package.json     the one npm workspace and package-lock.json (ui, eslint, icons, the demos, apps/*/app)
```

## Working in the monorepo

- **Worktrees (rule 2).** Every agent works in its own worktree of `paddock`; for app work a sparse one:
  `git sparse-checkout set --cone crates ui scripts docs tools apps/<app>`. Never work in the main checkout; never `git stash`.
- **Build folders.** One `CARGO_TARGET_DIR` per app per worktree, kept between tasks for the same app (reusing it is
  what makes rebuilds fast; sccache gives almost nothing across worktrees, ADR 0014 §1).
- **Cargo.** There is no root Cargo workspace: `crates/` and each app are their own workspaces, so each app builds with
  its own features and lockfile. Shared crates are path dependencies (`../../../../crates/<crate>` from
  `apps/<app>/app/src-tauri`). Run cargo inside the repo: it reads the root `.cargo/config.toml` (the `decryp7` registry,
  the Windows linker) from the current folder upwards.
- **npm.** One workspace and one `package-lock.json` at the root: `npm ci` there, never in a package folder; add a
  dependency with `npm install <pkg> -w <workspace>`. Apps use `@decryp7/paddock-ui` from `ui/dist` (built with
  `npm run build -w ui`). Name styles and assets in `angular.json` by package path, never `node_modules/...`.
- **Affected tests (rule 3).** `node scripts/affected.mjs origin/main --explain` lists the projects a change reaches (the
  Cargo and npm graphs, Tauri `frontendDist`, `tsconfig`/`angular.json`/ESLint config paths, declared doc inputs). Run
  those, plus the touched specs. CI runs the same plan; nightly.yml runs everything.
  Only `rust-toolchain`, the root `package.json`, `.npmrc`, `.nvmrc`, `.gitattributes` and `tsconfig/` still mark
  everything; the table above `GLOBAL` in `scripts/affected.mjs` says what every other former global reaches:
  `scripts/affected*.mjs` mark tools, ui and paddock-dev (the `plan` and `tools` jobs run their tests; the golden test of
  `dev` uses the script as oracle), `.cargo/` and `lint/` the Rust builds (`lint/` also conformance), a `ci.yml` change only
  the jobs whose text changed (everything if the file cannot be split into jobs, uses YAML anchors, its header changed or
  the `plan` job changed), a shared action or a script of `conformance/ci` or `conformance/release` only the jobs and
  workflows that use it, following scripts that import or name it (imports built at run time are not seen), reusable
  workflows to their callers, and ignoring mentions in comments and README prose; an action nobody uses marks everything.
  `--files` means working tree against HEAD: a committed change cannot be diffed, so it marks everything. Users that run only
  nightly, on a tag or manually are named as such in `--explain`; `e2e-webkit` never runs on a pull request. The Rust port
  in `tools/paddock-dev` keeps `.gitea/` and `conformance/(ci|release|src/util)/` global (a superset of the script).
- **Ownership.** [CODEOWNERS](CODEOWNERS) (Go regular expressions, Gitea's syntax) says who owns which paths; a brief
  names the paths its task owns. Shared integration points (`crates/*/src/lib.rs`, `crates/Cargo.toml`, `public-api.ts`,
  `_tokens.scss`, the root `package.json`, `.gitea/`, `renovate.json`, `CODEOWNERS`) are changed by the integrating agent
  only, or by adding a single declared line.
- **Shared code (rules 39, 42, 43).** A shared change and its adoption by the apps land in one change; there is no
  publish-and-consume step. A breaking change to a shared crate or package updates every app in the same change.
- **Workflows are named after their app** (ADR 0014's last amendment): an app with gates on several OS (Debrief, Dyno,
  Pitcrew, Pitwall, Spotter) has `.gitea/workflows/<app>.yml`, its whole CI and its release from an `<app>-v*` tag, whose
  plan job asks `node scripts/affected.mjs --ci --app <app>`; extra jobs are `<app>-<what>.yml` (Debrief's fuzz, bench,
  corpus, mutants). An app whose gates fit one Linux job (the demos) is a leg of ci.yml and releases through
  `<app>-release.yml`. Any of these files marks only its app in the plan.
- **A new app** gets `apps/<app>/paddock.ci.json` (`workflow` for its own workflow, or its leg: Cargo workspace,
  prepare, checks, UI commands), a CODEOWNERS line and its workflow. `scripts/affected.test.mjs` checks all three.

## Shared code: `crates/` and `ui/`

- Tokens in `ui/projects/paddock-ui/scss/_tokens.scss` are the single source of truth; components use `var(--pd-...)`
  only. The design guide documents them and the token test measures them; read `docs/design-guide.md` before touching
  either.
- Statuses, accents and densities are string-literal unions; status is never colour alone. Components never throw on odd
  input (empty series, NaN, null): they render a defined state. Every state is designed (skeleton, empty, error with
  Retry). Reduced motion is respected.
- Nothing remote: no web fonts, no CDN, no icon URLs. The CSP forbids `unsafe-eval` and inline style attributes: bind
  styles with Angular style bindings only. Never bind untrusted text with `[innerHTML]`.
- The public API is `ui/projects/paddock-ui/src/public-api.ts` and the CSS custom properties `--pd-*`, `data-theme`,
  `data-accent`, `data-density`. Any change is a CHANGELOG entry; no backward-compatibility shims before Paddock's v1.
- Shared Rust crates have a pure core and a `tauri` feature for the adapter; typed errors, doc comments on public items,
  no `unwrap` on I/O or user data, bounded resources. Coverage: crates >= 99% lines (CI's llvm-cov gate), components 100%.
- Anchor every ignore rule (`/ui/dist/`, not `dist/`); `node_modules` (no trailing slash, so it also matches a symlink) is the one unanchored rule; conformance C57 fails on any tracked symlink.

## Testing and quality gates

- **E2E and screenshots run on fresh code (guidance; a binding rule is proposed).** A running dev server is reused only with `PW_REUSE_SERVER=1`; default ports derive from the worktree path (`e2ePort` in `@decryp7/paddock-e2e`); a config that consumes `ui/dist` or the css fails at load with "rebuild ui: npm run build -w ui" when they were built from older sources (`build-guard.ts`, content stamps; lessons.md). New Playwright configs use these.
- Every change ships with tests; every bug fix starts with a failing test. A colour change keeps `npm run test:tokens`
  green (WCAG AA/AAA pairs, CVD separation); change the tokens, not the test.
- Before committing, the gates of the affected projects, for example:

  ```sh
  npm ci
  node scripts/affected.mjs origin/main
  (cd ui && npm run test:tokens && npm run lint && npm test && npm run build && npx playwright test --project=gallery)
  (cd crates && cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features -- -D warnings)
  ```

## Dependencies and releases

- Prefer permissive, well-maintained dependencies; commit `package-lock.json` and each workspace's `Cargo.lock`; CI uses
  `npm ci`. Renovate (the root `renovate.json`) groups Angular and Tauri across workspaces and keeps one lock PR per
  lockfile.
- Conventional Commits (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`, `chore:`). No Co-Authored-By or AI
  attribution.
- Tags: `v*` for the shared packages (publish.yml), `<app>-vX.Y.Z` per app (its `<app>-release.yml`), never from a
  workstation. ADRs: family ones in `docs/adr/`, an app's in `apps/<app>/docs/adr/` (numbered per app).

<!-- The sections below are docs/blocks (the engineering rules common to every app), copied verbatim;
     scripts/affected.test.mjs fails when they drift. Change them in docs/blocks first. -->

## Design principles

Apply these deliberately. They are not decoration:

- **Single responsibility.** One reason to change per module, type and component. Split a file when it starts doing
  several jobs; keep request/response DTOs apart from the logic that produces them.
- **Open/closed.** Extend by adding, not editing: a new kind of thing is a new implementation of an existing trait or a
  new configuration entry, not a special case in shared code.
- **Liskov.** Every implementation of a trait or interface honours its documented contract: same error semantics, no
  surprising panics.
- **Interface segregation.** Keep traits and TypeScript interfaces small and purpose-built.
- **Dependency inversion / code against abstractions.** Tauri commands depend on traits (ports), never on a concrete
  store, pipe or OS API. Angular components never call Tauri `invoke` directly: they go through the backend injected by
  DI token (the real Tauri backend, or the mock for browser development and tests), and state lives in SignalStores.
- **Pure core, thin edges.** Parsing, rules, formatting, geometry and state machines are pure functions with time and
  I/O passed in; processes, files, network and the OS stay thin.
- **Ports and adapters.** The core never depends on an adapter (Tauri, a database, an OS API).
- **Parse, don't validate.** Convert untrusted input (file bytes, IPC JSON, config files, network replies) into typed
  values once, at the edge, and work with the types afterwards.
- **Make illegal states unrepresentable.** Prefer enums and newtypes over strings, flags and `Option` combinations that
  can disagree.
- **Command-query separation.** Queries have no side effects; only explicit commands mutate.
- Also: separation of concerns, high cohesion and low coupling, KISS, YAGNI, DRY with the rule of three before
  abstracting, composition over inheritance, Law of Demeter, least astonishment (sensible defaults, per-item overrides),
  fail fast with a message that tells the user what to fix, and leave touched code better than you found it.

## Where logic lives

- **The Rust core is the single source of truth** for data and behaviour: parsing, rules, state machines, configuration,
  analysis and exports. Results are computed in Rust and returned through ports and Tauri commands.
- **Angular is the presentation layer**: layout, interaction state (selection, zoom, open panels, unsaved form edits),
  rendering and formatting of what the engine returns. It must not re-implement domain rules. If the UI needs a
  computation (a preview, a validation, a count), add an engine operation and a command for it instead of writing it in
  TypeScript. Small purely visual transforms are fine.
- **The mock backend** exists for browser development and UI tests only. It mimics the contract with plausible data; it
  is never the reference for behaviour, and a feature is not "done" until it works against the real engine.
- Validation that matters is done in Rust; the UI may validate early for convenience, but the engine re-validates.

## Working in parallel (agents)

- Each task owns an explicit set of files; don't edit outside it. Shared integration points (`lib.rs` files, the
  workspace `Cargo.toml`, the wire types and DTOs) are changed by the integrating agent only, or by adding a single
  declared line.
- Don't run `cargo fmt --all` / `cargo clippy --fix` across the workspace while others are editing; format your own
  files (`rustfmt <files>`). Use a private `CARGO_TARGET_DIR`.
- Commit only complete, owned work. Never commit a workspace `Cargo.toml` listing crates whose sources aren't
  committed. After committing, verify the committed tree in isolation (`git archive HEAD`, then build and test) before
  pushing larger changes.
- Before pushing a merge, run `node scripts/merge-regression-check.mjs <merge-commit>` (or `--branch <b> --target <t>`): it flags files
  where the result goes backwards against the first parent, such as a stale-tree branch commit reverting fixes without a conflict.
- Windows-only and macOS-only code is checked and tested on its own CI runner; code that needs no platform SDK must at
  least type-check for the other targets (`cargo check --target x86_64-pc-windows-msvc`).

## Style

- Match the surrounding code: naming, comment density (doc comments on public items, comments only where intent isn't
  obvious), idioms. Rust uses `rustfmt.toml` (max_width 140). Never run `prettier --write` on TypeScript.
- UI: standalone components, signals, OnPush, CSS custom properties for theme tokens (dark and light), accessible markup
  (real buttons, labels, aria-labels on icon buttons), sentence-case copy, verb plus object on buttons.
- UI building blocks: use **Angular CDK** for behaviour and accessibility (menus, overlays, dialogs, listbox, scrolling)
  instead of hand-rolling them, and **Angular Material first** (rule 47: use a Material component wherever one fits), themed through the
  `--pd-*` tokens. Icons are inline SVG registered with `MatIconRegistry`; no icon fonts and no icon URLs fetched at run time (the
  CSP forbids it).
- Status colours always come with words; validate colours for contrast and colour-vision deficiency.

## Working practices

- Research official sources before choosing libraries or relying on another program's behaviour, and record what you
  found and when in the ADR or doc comment. Prefer well-maintained, permissively licensed dependencies.
- Navigate code with Serena's symbol tools (find symbol, references, overview) when they are available instead of
  reading whole files; it saves context and cost.
- Delegate mechanical or well-specified work (refactors with a precise spec, test scaffolding, docs) to cheaper models.
  Keep architecture and review on the main model.
- Share first (rule 42): check Paddock's crates and packages before writing anything an app copy would duplicate.
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
- Wait with `./dev wait`; never `pgrep -f` on a string your own command contains. `./dev wait --plock <name> | --pid <pid> | --file <path> --line <regex> | --ci-run <id> --max <duration>` always ends at `--max` with an `::error::` and the next step (a `pgrep -f` loop matches its own command line and never ends).
- Spawn subagents by role: implementer (Sonnet, mechanical), designer, reviewer, researcher (Opus); see `.claude/agents/`.
