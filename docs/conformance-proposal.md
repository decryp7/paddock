# Proposal: `paddock-conformance`, one shared check every app runs

Status: proposal (2026-10-24). Nothing here is implemented. Closes the "enforced by memory" gaps of the
[problem catalogue](problem-catalogue.md). Rule 40 applies: the class is made impossible or checked, not patched per app.

## Problem

Today every guard is a per-app copy (`check-index`, `check-icons`, `macos-verify-bundle`, key strip, `publish.mjs`,
`commit-lint`), each present in two to four apps and drifting. The audits show the result: Pitcrew has no icon check, no
codesign verify and no key strip; Debrief has no inline-style check and shipped a store worker that cannot start; Dyno hard-codes
the app version in a spec. Rules 27 to 39 (Angular) and 6, 7 (tests) have no check at all.

## Shape

- A Node package `@decryp7/paddock-conformance`, built and published from Paddock CI (same pipeline as `@decryp7/paddock-ui`;
  Node is build tooling only, so the "Rust and Angular only" language rule is respected). Pure ESM, no network, no
  dependencies beyond `node:` built-ins and `yaml`/`toml` parsers; each check is a small module `checks/Cnn-name.mjs`
  exporting `{ id, title, rule, run(ctx) -> findings[] }`, with its own `node --test` file that feeds it a good and a bad
  fixture (so the checks are themselves tested; they fail closed when they cannot find what they inspect).
- One entry point: `npx paddock-conformance --app <name> [--only C04,C06] [--bundle <dir>] [--json]`.
  It reads a small `paddock.conformance.json` in the app (app name, paths of `app/`, `crates/`, `src-tauri/`, the
  single version source, sidecar names, dialog registry path) so a check never guesses paths.
- Findings have a severity (`error`, `warn`), the rule number, the file and line, and **the fix in one sentence**. Output
  goes to the log and to the Gitea job summary (Markdown), plus `conformance.json` as an artifact.
- Waivers: `paddock.waivers.toml` with `check`, `path`, `reason`, `expires`. A waiver past its date fails the build;
  a missing reason fails the build. No silent skips (rule 5).
- Distribution: pinned by tag in each app's `package.json` (`"@decryp7/paddock-conformance": "0.x"`), upgraded by Renovate
  PR, so a new check lands in every app as a visible PR with its findings, not as a surprise red main.

## CI wiring (per app, one job and two legs)

1. `conformance` job (Linux, a minute): static checks over the source tree, `package.json`, workflows, configs.
2. `conformance-bundle` legs inside the existing `bundle` job on macOS, Windows and Linux: checks that need the built artifact
   (`C04`, `C05`, `C08`, `C20`, `C21`).
3. `conformance-tests` wrapper: runs the Rust/Vitest/Playwright steps with a hermetic environment (`C14`, `C15`) and
   checks the aftermath. This is a wrapper script, not a new test runner: `paddock-conformance run -- cargo test ...`.
4. A local `paddock-conformance --prepush` runs only the cheap static checks on touched files (rule 4: lint before every push).
5. Nightly: the same job plus `cargo deny check advisories`, `npm audit`, the benchmark budgets (`C26`).

The release job `needs` the conformance job and legs; `C07` runs again at the tag (see below).

## Checks

| Id | Checks | Catches (catalogue) |
|---|---|---|
| C01 | No ignore rule hides a directory that exists under `app/src` or `crates/*/src` (`git check-ignore` over every directory) | B01 |
| C02 | Built `index.html` has no inline `<style>`/`<script>`/`style=""`; has the splash element (`id="splash"` styled from the global sheet); `inlineCritical:false` is set | B02, R01, U10 |
| C03 | `src-tauri/icons` equals `paddock/icons/<app>/tauri` (hash) and the artwork fills the canvas | B08 |
| C04 | macOS bundle: `codesign --verify --deep --strict` on the .app and the app inside the DMG; every Mach-O is signed and not linker-signed-only; **one `TeamIdentifier` (or all ad-hoc) across all Mach-Os**; `hardenedRuntime:false` in `tauri.macos.conf.json` while `signingIdentity` is `-`; every sidecar listed in `paddock.conformance.json` is started from the built .app with its `--self-test`/`--version` and must exit 0 | B03, B04, B16 |
| C05 | Bundle layout: rpaths relative or system only; sidecars exist, are executable, are real binaries above a size floor (not placeholders); the launch-agent plist is in `Contents/Library/LaunchAgents`; `.deb` has the binaries | B07, B17 |
| C06 | Every workflow step that exports `TAURI_SIGNING_PRIVATE_KEY` or `TAURI_UPDATER_PUBKEY` strips whitespace first (or calls the shared bundle script); the shared script is also exposed as a function so the apps stop copying it | B09 |
| C07 | **Tag preflight** (runs at the start of the release job and locally as `paddock-conformance --preflight vX.Y.Z`): tag commit is not `[skip ci]`; the Gitea API reports a green run of all required jobs for exactly this commit; the release job `needs` every gate; no `fix/*`/`release/*` branch has commits since the previous tag that are missing from the tag (list shown, waivable only in `docs/release-waivers.md` with a reason); version agrees in `Cargo.toml`, `package.json`, `tauri.conf.json` and the `CHANGELOG.md` head; `cargo deny check advisories` re-run now | B10, B11, B12, P08 |
| C08 | Release artifacts: installer versions equal the tag; no stale installers; release notes contain the "First launch" section; `latest.json` uses the public https base URL and is signed; the shared `publish.mjs`/`latest-json.mjs` are used unmodified (hash) | B13, B14, B15 |
| C09 | Angular rules 27 to 39 as a shared ESLint preset (`@decryp7/paddock-eslint`) plus text checks: no `zone.js`, no `provideZoneChangeDetection`; every `@Component` declares `changeDetection` explicitly; no `ngClass`, `*ngIf`, `*ngFor`; effect that calls `.set`/`patchState` has a justification comment; `patchState` only inside `*.store.ts`; no `loading:`/`error:` pair in stores (use `withRequestStatus`); `providedIn: 'root'` only on an allow-list; a store injects at most two stores; no file duplicating a `paddock-ui` export | A01 to A06, U05, U07 |
| C10 | Dialogs and health (depends on ADR 0009 and 0010): every dialog id is in the registry with kind, owner, modality, size policy, provider set, command allow-list and a Close/Escape path; a test boots every registered dialog through the real dialog bootstrap; every service-health state has a Repair action | U01, U02, R04 |
| C11 | Layout (depends on ADR 0011): every pane declares min/preferred sizes; the shared overflow test visits each page at 320, 760, 1100 px and fails on horizontal scroll or a clipped control | U03, U04 |
| C12 | Tauri surface: every `#[tauri::command]` is `async` (allow-list with reason); capabilities have no wildcards or remote URLs; `core:window:allow-start-dragging` when the title bar is an overlay; CSP has no `unsafe-eval` and no remote origin; `macos-private-api` feature and `macOSPrivateApi` agree; no `[innerHTML]` bound to log text | B05, B06, L04, S01 |
| C13 | Rust lint preset: a shared `[workspace.lints]` and `clippy.toml` (`unwrap_used`, `expect_used` outside tests, `disallowed-methods` for `Command::new`, `std::env::temp_dir`, `Mutex::lock().unwrap()`); the app must extend it, not replace it | R06, S02, T01 |
| C14 | **Hermetic test run**: tests run with `HOME`, `XDG_*`, `APPDATA`, `TMPDIR` pointing into a scratch folder; afterwards the scratch temp is empty (temp leak), nothing was created in the scratch home outside an allow-list, no process started during the run is alive (census by start time), no new `com.google.Chrome.code_sign_clone`, no launchd job, Login Item or service registered | T01, T03, T09 |
| C15 | No test or script references the installed Chrome (`/Applications/Google Chrome.app`, `chrome.exe`, `channel:'chrome'`); the pinned Chrome for Testing is installed by the shared script (SHA-256 checked) and the `<APP>_REQUIRE_TEST_BROWSER=1` env is set in CI so a skip is a failure | T02, T12 |
| C16 | No semver literal equal to the current app version appears in specs/e2e; mock backends read the version from the single source (grep for any `x.y.z` assigned to `version`/`currentVersion` in `mock*`) | T04, U08 |
| C17 | E2E hygiene: Playwright port from env, `reuseExistingServer:false` in CI, retried tests listed in the job summary (count compared with the previous main run, may not grow), no `waitForTimeout`/`sleep` in specs, every `skip`/`fixme` has a waiver, `data-testid` used by page objects, axe run on every page | T05 to T07, T10, T11, U09 |
| C18 | Unit tests run once more with `TZ=Pacific/Auckland` (and `UTC`) | T08 |
| C19 | Long operations: every command that starts a job returns at once; the UI timeout table lists no job-starting command; progress snapshots carry a phase label; `paddock-jobs` conformance tests pass for the app's adapters (monotonic, cancel from the callback, growing count). `C19b`: every IPC DTO round-trips through the real codec, and mocks decode the same fixtures | L01 to L06, R10, U08 |
| C20 | Quit: an exit watchdog is armed on exit (grep for the shared helper) and the real-app smoke asserts the process exits within 10 s after Quit, with extra windows open | R09 |
| C21 | Service survival (apps with a service or agent): `service_registered_version` is stored and compared; the macOS smoke installs build N, then N+1 over it, and asserts the agent is running and not in EX_CONFIG afterwards | R03 |
| C22 | Process memory: any use of `sysinfo` memory or `Process` iteration goes through the shared helper (thread entries skipped, PSS on Linux) | R07 |
| C23 | Process: `commit-lint` shared rule set (Conventional Commits, no `Co-Authored-By`, no AI attribution text); the app's `AGENTS.md` rules block and `docs/suite-lessons.md` equal the Paddock revision pinned in `paddock.conformance.json` (hash), and say which revision | P05, P06 |
| C24 | Docs: ADR numbers unique; every image under `docs/images` has an entry with its hash in `docs/images/REVIEWED.md` (reviewer, date); changed hash fails | B18, P07 |
| C25 | Fixture scan: no hostnames, user names, IPs, e-mail addresses or license keys from a local denylist (`~/.config/paddock/confidential.txt`, never committed) in samples, fixtures, docs or commit messages | S04 |
| C26 | Nightly: `cargo deny check advisories`, `npm audit`, benchmark budgets (rule 16) against stored baselines | B12, L10, S05 |
| C27 | Coverage and format guards (`ui-coverage-guard`, units guard) as one shared script with the app's floor in `paddock.conformance.json` | T14 |
| C28 | Repo baseline (survey X27): the config files every app repo carries equal `templates/repo` (exact, or only the named TOML tables, or a Renovate `extends`); `paddock-conformance sync --write` repairs | P05, P06 |
| C29 | Window weight (survey X46, rule 35): the JS each window loads before first paint, from `ng build --stats-json`, stays within its budget in `paddock.conformance.json`; budgets are tightened to measured plus 10% | U10 |

## Where each piece lives (rule 39)

| Piece | Home |
|---|---|
| Checks `C01` to `C27`, preflight, hermetic wrapper, waiver loader | `paddock/conformance/` (new, npm package) |
| Pinned Chrome installer and REQUIRE switch (from Spotter's `pin-chromium`, `install-test-browser`) | `paddock/conformance/browser/` |
| Release scripts (`publish.mjs`, `latest-json.mjs`, key strip, `macos-verify-bundle.sh`) | `paddock/conformance/release/`, used unmodified |
| ESLint preset (rules 27 to 39), stylelint tokens | `paddock/eslint/` (`@decryp7/paddock-eslint`) |
| `withRequestStatus`, dialog registry, truncation, tray popover | `paddock-ui` |
| Phase/progress/cancel, hash-while-writing, monotonic reporter | `paddock-jobs` (exists) |
| Service (re)registration per OS, process memory, process helpers, window state | `paddock-service`, `paddock-proc`, `paddock-window` (listed in ADR 0006) |
| Worktree and Docker helpers | `paddock/scripts/` (`paddock-worktree`, `paddock-docker`) |

## Rollout order (one app at a time, rule 1)

1. Build `C04`, `C07`, `C14`, `C15`, `C16` first: they close the two classes that reached users (B04, R03/B11) and the two that
   cost disks (T01, T02). Run them against the app being worked on (Debrief first, as it is the current app).
2. `C02`, `C03`, `C06`, `C08`, `C12`, `C13` next: these are existing per-app scripts; moving them mostly deletes code.
3. `C09` and the ESLint preset when each app is next touched (existing code gets a waiver list, new code is held to it).
4. `C10`, `C11`, `C19` follow ADRs 0009 to 0011 and the `paddock-jobs` adoption.
5. Sighter and Pacenotes start with the full set from their first commit (no waivers).

## What it would have caught

| Incident | Check | Would have failed |
|---|---|---|
| Debrief 1.0.1 store worker SIGABRT on macOS | C04 (sidecar self-test from the built .app; Team-ID check), C07 (the fix branch not merged) | the 1.0.1 and 1.0.2 CI, and the tag |
| Pitcrew/Pitwall service dead after update | C21 | CI of 1.0.0 to 1.0.3 |
| Pitwall 1.0.0 blank icon | C03 | its bundle job |
| Pitwall base64 "Invalid symbol 10"; Pitcrew's same risk today | C06 | Pitcrew's release job now |
| 98k temp folders; 441 GB Chrome clones | C14, C15 | the first test run |
| Hard-coded `0.1.0` (Debrief) and Dyno's spec today | C16 | immediately |
| Spotter/Pitcrew/Dyno unverified .app seal | C04 | bundle job |
| Quit hang | C20 | smoke |
| Blank dialogs, denied commands, missing Close | C10 | every dialog at build time |
| Zone.js, effects, flags drift | C09 | lint |

## Risks and costs

- A shared check that is too strict will be waived into uselessness: waivers expire and are listed in each release's notes
  draft, so debt is visible.
- Checks that need the built bundle make CI longer on macOS (the longest leg); `C04` adds under a minute plus sidecar
  starts. Static checks run in the cheap Linux job.
- Cross-repo versioning: a new check can turn every app red; the Renovate PR carries the findings first (rule 4: fix, push once).
