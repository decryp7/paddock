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
scripts/         family scripts: ./dev plan (CI's plan), publish.mjs, new-app.mjs, demo-release.mjs
.gitea/          ci.yml (the plan job and the per-project jobs), nightly.yml, each app's <app>.yml, the shared actions
package.json     the one npm workspace and package-lock.json (ui, eslint, icons, the demos, apps/*/app)
```

## Working in the monorepo

- **Worktrees (rule 2).** Every agent works in its own worktree of `paddock`; make it with
  `./dev worktree add`: a full checkout (`--profile gates`, the default), because conformance, `./dev plan` and
  `./dev gates check` and `./dev plan` read most folders, and a sparse cone makes them stop with an `::error::` line naming the
  missing folders (measurements: [benchmarks](docs/benchmarks.md#2026-11-12-full-against-sparse-worktree-checkout-fixworktree-gates-profile));
  `--sparse` is only for a `--detach` review copy, and `-b <branch>` attaches to a branch that already exists.
  `add` also prints a WARNING (never a refusal) naming another locked worktree, its owner and the shared files when it changes files the new base
  carries or its branch has the same topic (`tools/paddock-dev/src/worktree/overlap.rs`).
  Never work in the main checkout; never `git stash`.
- **New rules take their number at landing (user, 2026-11-25).** A branch writes a new rule in docs/rules.md as `NEW-<slug>.` and
  cites it as `rule NEW-<slug>`; the train integrator assigns the next free number when the train lands and replaces every citation
  (a step of the land-train skill, which lives on `chore/playbook-skills` until train f lands). C31 refuses a repeated number and
  warns on a `NEW-` placeholder.
- **Build folders.** One build folder per app per worktree, kept between tasks for the same app (reusing it is what makes
  rebuilds fast). Pass it to cargo as `--config build.target-dir='<dir>'` (or `build.target-dir` in a `.cargo/config.toml`),
  **never as `CARGO_TARGET_DIR` or `CARGO_BUILD_TARGET_DIR`**: sccache 0.19 puts every `CARGO_*` variable into its cache key, so
  the variable gives 0% hits across worktrees and the configuration gives about 82% (ADR 0014 §1; docs/benchmarks.md). The
  tools do this themselves (push gates, `./dev gen check`, `./dev`'s own build). Your own cargo calls need nothing in a fresh shell: `./dev sccache env`
  (run once on a machine, and after a worktree is added) installs the pinned sccache, starts the one local server and writes
  `<worktree root>/.cargo/config.toml` with `build.rustc-wrapper`, so every cargo below the worktree root compiles through it; the
  wrapper starts the server again if it is down and runs plain rustc when it cannot, and an explicit `RUSTC_WRAPPER` wins
  ([docs/dev-setup.md](docs/dev-setup.md#local-sccache)). `eval "$(./dev sccache env)"` still sets the variables for one shell. Worktrees and build folders go in
  `<dev-targets>` (the parent of the worktree root; this machine's folders are in
  [docs/dev-setup.md](docs/dev-setup.md#where-things-live), the one place that names a disk, rule 40). The worktree root is
  machine config, not code: `git config paddock.worktreeRoot <dev-targets>/wt` (repo-local, so it covers every worktree), or
  `PADDOCK_WORKTREE_ROOT`, or `--root`; the flag wins, then git config, then the variable.
- **Cargo.** There is no root Cargo workspace: `crates/` and each app are their own workspaces, so each app builds with
  its own features and lockfile. Shared crates are path dependencies (`../../../../crates/<crate>` from
  `apps/<app>/app/src-tauri`). Run cargo inside the repo: it reads the root `.cargo/config.toml` (the `decryp7` registry,
  the Windows linker) from the current folder upwards.
- **Rust version.** The root `rust-toolchain.toml` pins the exact version of the CI image (rustup reads it in every workspace;
  `setup-rust` reads it in CI). Upgrading Rust is one change: bump that file, rebuild the CI image, re-capture the
  Rust-tool rows (ADR 0090) and reformat with the new rustfmt. The pinned toolchain starts without extra targets: for the
  Windows lint (`cargo xwin`) run `rustup target add x86_64-pc-windows-msvc --toolchain <the channel in the file>` once.
- **npm.** One workspace and one `package-lock.json` at the root: `./dev deps install` (a clone of the shared, content-keyed cache, or `npm ci` once; `./dev deps ui` does the same for `ui/dist`, [docs/dev-setup.md](docs/dev-setup.md#shared-npm-cache-dev-deps)) or `npm ci` there, never in a package folder; add a
  dependency with `npm install <pkg> -w <workspace>`. Apps use `@decryp7/paddock-ui` from `ui/dist` (built with
  `npm run build -w ui`). Name styles and assets in `angular.json` by package path, never `node_modules/...`.
- **Affected tests (rule 3).** `./dev plan origin/main --explain` lists the projects a change reaches (the
  Cargo and npm graphs, Tauri `frontendDist`, `tsconfig`/`angular.json`/ESLint config paths, declared doc inputs). Run
  those, plus the touched specs. CI runs the same plan; nightly.yml runs everything.
  Only `rust-toolchain`, the root `package.json`, `.npmrc`, `.nvmrc`, `.gitattributes` and `tsconfig/` still mark
  everything; the table above `GLOBAL` in `tools/paddock-dev/src/plan.rs` says what every other former global reaches:
  `tools/paddock-dev/src/plan*.rs` mark paddock-dev (the planner's tests are its own; the recorded answers in
  `tools/paddock-dev/tests/golden` are its reference), `.cargo/` and `lint/` the Rust builds (`lint/` also conformance), a `ci.yml` change only
  the jobs whose text changed (everything if the file cannot be split into jobs, uses YAML anchors, its header changed or
  the `plan` job changed), a shared action or a script of `conformance/ci` or `conformance/release` only the jobs and
  workflows that use it, following scripts that import or name it (imports built at run time are not seen), reusable
  workflows to their callers, and ignoring mentions in comments and README prose; an action nobody uses marks everything.
  `--files` means working tree against HEAD: a committed change cannot be diffed, so it marks everything. Users that run only
  nightly, on a tag or manually are named as such in `--explain`; the reference apps' browser jobs (`e2e`, `e2e-webkit`) and the
  apps' WebKit legs run only in the nightly, on a `v*` tag and on a manual run with `webkit` (user, 2026-11-24). `./dev plan`
  (`tools/paddock-dev`) narrows the same way and answers the same plan; its golden test compares it with the script.
- **Merge queue: parked (user, 2026-11-24).** `./dev queue` and `./dev pr` (docs/plans/merge-queue.md) stay in the tool, but
  nothing runs them and CI does not test them; trains are built by hand with the member gate (`./dev merge check --gates`).
  A member merges current main and re-runs its member gate on that tip before it joins a train (rule 100). `./dev merge sync-member <train>`, run after the merge, runs the
  fast static rows (conformance, gates-register, clippy, clippy-windows) on the sync tip first and fails on a bad row (`--no-static "<reason>"` skips them, logged);
  once they pass it dispatches the member's own CI trial (the affected jobs on the train, `ci dispatch --trial` on `sync/<member>/<short sha>`, a new ref per synced commit that sync-member pushes with a plain push: integrator role only) and prints the run id, which
  `./dev merge check --gates` reads later (red blocks, running is not a pass; `--no-trial "<reason>"` skips it, logged).
- **Train merges are made by `./dev merge member` (user decision 100, 2026-11-27).** In the train's worktree: `./dev merge member <member-branch-or-sync-ref> [--reland <revert>]...`
  does the `git merge --no-ff` itself with the fixed subject `Merge <member> into <train>` and the trailers `Paddock-Member`, `Paddock-Sync-Ref` (a
  `sync/<member>/<sha>` ref was given), `Paddock-Sync-Sha`, the `Reverted:` lines `merge check --write-reverted` would write and `Paddock-Reland`
  per `--reland`, then runs `./dev merge check` on the new commit. A conflict stops it with the merge in progress and the message prepared; it never
  resolves. Every reader of a train merge (sync-member's own-revert logic, the trial reader, stale-base, `--write-reverted`) reads the trailers
  through the one function `trailers::merge_member` (git's own trailer parser, `%(trailers:only,unfold)`); the old reading of the subject is only its
  fallback for merges made by hand (ER-22, ER-42). Do not write a train merge by hand.
- **Old-base block (user, 2026-11-25).** `./dev merge check` refuses a train member that lacks the train's base (the newest commit of
  main, or of the earlier train, the train has merged) with one `::error::`: merge origin/main into the member and rerun its gate;
  `--old-base-anyway "<reason>"` is the logged emergency override (`tools/paddock-dev/src/merge_check/stale_base.rs`).
- **Dispatching workflows.** Integrators dispatch through `./dev ci dispatch <workflow>.yml --ref <ref> --max <duration>`, never by a bare API call: it waits while another run has an unfinished job on the same single runner (the Mac runner took overlapping runs 7960/7961 and 7969-7971/7967 and timed out tests) and ends at `--max` with an `::error::` naming the busy run. A `train/*` run of a workflow whose first job runs the `runner-wait` action is sent at once and waits in that Linux job instead (`./dev ci runner-wait`, only for older runs), so scheduled runs see the train queued and yield to it. A full dispatch of `ci.yml` on a `train/*` ref (no `base`, no `since_run`) is complete without WebKit, as a push to main's scope has none since 2026-11-24 (the nightly runs it), so the landed train's push to main can reuse it (ER-49, docs/ci-schedules.md). It prints `run <id>`: the run whose title carries the `dispatch_id` token it sent (never "the newest run", ER-51). On a
  `train/*` ref it first runs the failed-first and green-main guards (docs/ci-dispatch-guards.toml: report-only until
  `./dev ci guard-record` records their first green use; overrides `--full-anyway` and `--red-main-anyway` take a logged reason).
- **Ownership.** [CODEOWNERS](CODEOWNERS) (Go regular expressions, Gitea's syntax) says who owns which paths; a brief
  names the paths its task owns. Shared integration points (`crates/*/src/lib.rs`, `crates/Cargo.toml`, `public-api.ts`,
  `_tokens.scss`, the root `package.json`, `.gitea/`, `renovate.json`, `CODEOWNERS`) are changed by the integrating agent
  only, or by adding a single declared line.
- **Shared code (rules 39, 42, 43).** A shared change and its adoption by the apps land in one change; there is no
  publish-and-consume step. A breaking change to a shared crate or package updates every app in the same change.
- **Workflows are named after their app** (ADR 0014's last amendment): an app with gates on several OS (Debrief, Dyno,
  Pitcrew, Pitwall, Spotter) has `.gitea/workflows/<app>.yml`, its whole CI and its release from an `<app>-v*` tag, whose
  plan job asks `./dev plan --ci --app <app>`; extra jobs are `<app>-<what>.yml` (Debrief's fuzz, bench,
  corpus, mutants). An app whose gates fit one Linux job (the demos) is a leg of ci.yml and releases through
  `<app>-release.yml`. Any of these files marks only its app in the plan.
- **A new conformance check** gets its id from `./dev check new-id --reserve <branch>` (the next free id across all branches and `conformance/reserved/`; it takes the id on origin at once by pushing a ref `refs/paddock/check-ids/C<n>`, which a second agent cannot create, so two agents never get one id; commit the reservation file as the record), never a hand-picked number.
- **A new app** gets `apps/<app>/paddock.ci.json` (`workflow` for its own workflow, or its leg: Cargo workspace,
  prepare, checks, UI commands), a CODEOWNERS line and its workflow. `tools/paddock-dev/tests/it/contracts.rs` checks all three.

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
  no `unwrap` on I/O or user data, bounded resources. Coverage: crates >= 90% lines, security code 99% target, report-only for now (llvm-cov and scripts/coverage-gate.mjs run in the nightly, a release's gates and a tag only, never on a train, a branch or a push to main: user 2026-11-26, ADR 0116 D4), components 100%.
- Anchor every ignore rule (`/ui/dist/`, not `dist/`); `node_modules` (no trailing slash, so it also matches a symlink) is the one unanchored rule; conformance C57 fails on any tracked symlink.

## Testing and quality gates

- **One way to run long commands (user, 2026-11-25).** Builds, tests and gates go through `./dev throttle [--stall S] -- <command>` or
  `throttle run [--stall S] <agent> -- <command>` (the old `heavy` names still work for now and print a deprecation line), never wrapped in `timeout` or piped through `| tail`: an outer timeout ends throttle and
  orphans the command it started, and a tail throws away the output you then need. throttle writes the whole output to
  `<dev-targets>/runs/<agent>/<start>-<name>.log`, prints that path first and a summary line (exit code, seconds, ran or hung) last;
  read the log instead of re-running. `--stall S` ends a command that wrote nothing and used no CPU for `S` seconds (exit 81); it is
  off by default, and a value is a hang detector set from a measured quiet gap with a wide margin, never a speed limit (ER-12;
  tools/throttle/README.md, "The run log and the hang detector").
- **E2E and screenshots run on fresh code (guidance; a binding rule is proposed).** A running dev server is reused only with `PW_REUSE_SERVER=1`; default ports derive from the worktree path (`e2ePort` in `@decryp7/paddock-e2e`); a config that consumes `ui/dist` or the css fails at load with "rebuild ui: npm run build -w ui" when they were built from older sources (`build-guard.ts`, content stamps; lessons.md). New Playwright configs use these.
- **Git fixtures (rule 102).** A test that needs a git repository builds it once per machine with `paddock_testkit::git_fixture::template` and copies it per test with `copy_tree`; a fixture builder with six or more `git` starts per test is a C174 finding (report-only).
- **A reader change adds or updates a replay shape (user decision 96, 2026-11-26).** A change to the CI verdict, reuse, green, fix-items or
  trial readers (`tools/paddock-dev/src/ci_runs/`, `run_scope`) records the real run that shows its shape with
  `./dev ci record-fixture <run>... --name <shape>` and adds or updates its row in `ci_runs/replay.rs`: [docs/testing/ci-replay.md](docs/testing/ci-replay.md).
- Every change ships with tests; every bug fix starts with a failing test. A colour change keeps `npm run test:tokens`
  green (WCAG AA/AAA pairs, CVD separation); change the tokens, not the test.
- **No VM or container on this Mac (user, 2026-11-26; docs/rules.md rule 101).** `./dev check --pre-push` and the member gate
  start no Docker, OrbStack or Fusion: there is no `clippy-linux`, `tests-linux` or `--warm-linux`. Linux Clippy (`-D warnings`),
  the Linux tests and the orphan guardian's acting tests as uid 65534 are CI's Linux jobs on the Linux runner (ci.yml: the crates job's
  "Orphan guardian as a user (Linux)" step, each app's Linux leg); the Windows Clippy runs here through xwin for `crates`, the apps that
  opt in (`windowsClippy`) and the developer tools `tools/paddock-dev` and `tools/throttle` (user, 2026-11-26: Windows-only dead code found
  only by CI four times in a day), as heavy rows in parallel with the others and through sccache; Windows tests stay CI's. The register (`NOT_RUN` in
  `pre_push/gates.rs`) names them CI-owned.
- The member gate (`./dev check --pre-push --member-gate`, run by `./dev merge check --gates`) also runs the full nextest `ci`
  profile of every touched Cargo workspace (the plan's reached packages, as CI's member run) on the host (`tests <ws>`). Inactive apps
  are skipped and named. A developer's push skips these rows.
- **The train's ER-53 split (user, 2026-11-26).** `./dev check --pre-push --member-gate --set fast` runs every row but the host tests
  (fmt, lockfiles, commit-lint, gates-register, conformance, Clippy, conformance-tests, member-audits) before CI is dispatched; then
  `./dev ci dispatch` prints `run <id>`; `--set slow --cancel-ci-run <id>` runs the host tests beside CI and, on a failure, cancels that
  run (`./dev ci cancel <id>`: the run's own cancel URL only, never the newest run). A slow run records its result for the commit it
  checked; landing (ER-49) is `./dev ci landing-held --sha <sha>`: the complete green CI run AND the green slow rows on the same commit
  (6, not held, while a slow row is unfinished or missing). A run with `--set fast` or `--set slow` leaves no whole-run pass record. The
  Linux tests are CI's Linux jobs on d-gtr9. A push or member gate prints a per-row table (queued s, run s, concurrent) and writes
  `row-timings.tsv` in its log folder.
- Before committing, the gates of the affected projects, for example:

  ```sh
  npm ci
  ./dev plan origin/main
  (cd ui && npm run test:tokens && npm run lint && npm test && npm run build && npx playwright test --project=gallery)
  (cd crates && cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features -- -D warnings)
  ```

## Before hand-back (Paddock commands)

Run these as well as the touched-file checks, on the merged tree when you hand back a merge, and list in the hand-off which you did not run:

- `(cd conformance && npm test)`: the full conformance tests.
- `./dev ci conformance-args` prints the conformance line of `.gitea/workflows/ci.yml`; run that line over the whole tree, not only the touched files.
- `./dev gates check`: the gate register (an ignored test needs its gates.toml entry).
- After a change to any `Cargo.toml`: `cargo metadata --locked --format-version 1 >/dev/null` in its workspace and in each workspace whose Cargo.lock lists the changed crate (`grep -l 'name = "<crate>"' $(git ls-files '*Cargo.lock')`, run in each lockfile's folder).
- `./dev check --pre-push` before every push.
- Last, on a branch for a train: merge `origin/main` into the branch, then run `./dev check --pre-push --member-gate --base origin/main`
  on that head (user, 2026-11-25). The train's member gate reuses its green rows for every workspace only this branch touched; a
  branch behind main has another tree there, so the train runs them again (`.claude/skills/land-train`, step 3).
- The member audits, the repository-wide audits of tools/paddock-dev that only the member gate runs (a developer's `./dev check --pre-push` skips them): `cargo nextest run --profile member-audits --locked --no-tests=fail` in `tools/paddock-dev`.
- A review reads a diff with `git show` and builds only in a scratch copy, never in the author's worktree.
- A design document (ADR, plan, research note) records the branch heads it read (ADR 0091), checks claims about shipped code with `git tag --contains <commit>`, and names every check it relies on as built on origin/main, held on a named branch, or a prerequisite (ADR 0096).
- A loop that runs a command, process, query or request once per item needs a `// per-item: <reason>` comment, and a new bulk command needs a budget test (rule 85, ADR 0108).

## Dependencies and releases

- Prefer permissive, well-maintained dependencies; commit `package-lock.json` and each workspace's `Cargo.lock`; CI uses
  `npm ci`. Renovate (the root `renovate.json`) groups Angular and Tauri across workspaces and keeps one lock PR per
  lockfile.
- Conventional Commits (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`, `chore:`). No Co-Authored-By or AI
  attribution.
- Tags: `v*` for the shared packages (publish.yml), `<app>-vX.Y.Z` per app (its `<app>-release.yml`), never from a
  workstation and never pushed by an agent: request a tag with the `tag` workflow (`.gitea/workflows/tag.yml`), whose release bot
  pushes it. It is a guard rail against mistakes, not a security boundary (any repo writer can dispatch from their own ref;
  single-user setup). ADRs: family ones in `docs/adr/`, an app's in `apps/<app>/docs/adr/` (numbered per app).

<!-- The sections below are docs/blocks (the engineering rules common to every app), copied verbatim;
     `tools/paddock-dev/tests/it/contracts.rs` fails when they drift. Change them in docs/blocks first. -->

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
  files (`rustfmt <files>`). Use a private build folder, passed as `--config build.target-dir=...` (never `CARGO_TARGET_DIR`).
- Before committing, check the subject: `node conformance/bin/paddock-conformance.mjs commit-lint --subject "<subject>"` (at
  most 100 characters; detail goes in the body). A rejected commit commits nothing and leaves your staged files as they are:
  shorten the subject and commit again, never amend an earlier commit.
- Commit only complete, owned work. Never commit a workspace `Cargo.toml` listing crates whose sources aren't
  committed. After committing, verify the committed tree in isolation (`git archive HEAD`, then build and test) before
  pushing larger changes.
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
  (Debrief's `openDialog()` in `testing/helpers.ts`), never let an optional chain perform a test's
  action, and patch a prototype only with `vi.spyOn` (restored after each test).
- Spend CI only on what a change needs (rule 50): one run per branch head, none while review fixes are pending or for a
  branch already merged elsewhere, none for docs-only changes; branches run the affected plan, full gates once on main.
- A change that touches an app's release path builds that app's installers before it lands on main (rule 54): the batch
  dispatches `<app>-release.yml` with gates on and the bundle built, once per train or batch that touches the release path;
  passing tests alone is not enough.
- One implementation per mechanism (rule 55): a mechanism listed in `docs/known-copies.toml` is fixed in its canonical
  implementation and every copy in the same change, each with a failing test first, or names the copy it skipped, and why, in the commit body; a freeze does not stop such a fix; a new copy needs its entry and a delete-by tag, an entry leaves
  with its files, and a date moves to a later release line only with the user's approval; a reached delete-by tag is a warning, not a tag block (user, 2026-11-11); register check C58 (planned, ADR 0034 WP-1; not built yet, the steward checks the register by hand).
- Slow means two questions (rule 13, merged with 14, 15 and 51): can more threads or workers run it (bounded by the kind of work), and can it be made
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
- Spawn subagents by role: implementer (Sonnet, mechanical), designer, reviewer, researcher (Opus).
