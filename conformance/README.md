# @decryp7/paddock-conformance

One check every Paddock app runs (docs/conformance-proposal.md). Pure ESM, no dependencies, no network except the optional
Gitea API call of the tag preflight. Not published yet.

```sh
node conformance/bin/paddock-conformance.mjs --app pitcrew [--root <app checkout>] [--only C04,C06] [--json] \
    [--out conformance.json] [--summary summary.md] [--bundle <dir>] [--artifacts <dir>] [--paddock <paddock checkout>]
node conformance/bin/paddock-conformance.mjs --app pitcrew --preflight v1.2.3 --api https://host/api/v1 --repo owner/name
node conformance/bin/paddock-conformance.mjs run -- cargo test        # C14 hermetic wrapper
node conformance/bin/paddock-conformance.mjs commit-lint --range origin/main..HEAD   # C23, part 1
node conformance/bin/paddock-conformance.mjs list
```

Exit status: 0 no unwaived error, 1 findings, 2 usage. `GITHUB_STEP_SUMMARY` receives the Markdown summary when set.

## Checks

| Id | What | Needs |
|---|---|---|
| C03 | icons equal `icons/<app>/tauri` of Paddock and fill the canvas; CI runs an icon check | `--paddock` for the hash part |
| C04 | macOS signing: static (hardened runtime off while ad-hoc, workflow verifies .app and DMG) and, with `--bundle`, the real bundle | `--bundle` for the bundle part |
| C06 | every step exporting a Tauri key strips whitespace first | |
| C07 | version agreement; release job needs the gates; with `--preflight vX.Y.Z` the tag preflight | `--api --repo` for the CI-green part |
| C08 | the shared release scripts are used; with `--artifacts` the release files | `--artifacts` |
| C13 | Rust lint preset (`lint/`): `[workspace.lints]` at least as strict, every member crate `[lints] workspace = true`, `clippy.toml` keeps every `disallowed-methods` path | `--paddock` |
| C14 | tests run hermetically (`run --`); workflows that run tests without it are warned | |
| C15 | no test or script uses the installed Chrome; CI requires the test browser | |
| C16 | no spec or mock hard-codes the current app version | |
| C23 | commit messages (no Co-Authored-By, no AI attribution); `commit-lint` CLI. With `--paddock`: the shared doc blocks of `AGENTS.md` (rules and the common sections), `docs/suite-lessons.md`, `docs/releasing.md` equal `docs/blocks` | a git checkout; `--paddock` for the blocks |
| C24 | every image under `docs/screenshots` matches its `hashes.json` entry (written by `captureDocScreens`) and has a `REVIEWED.md` row (`\| image \| sha256 \| reviewer \| date \|`) with the current hash; images under `docs/images` need the row in `docs/images/REVIEWED.md`. A regenerated image that changed fails until it is reviewed again (rule 22) | |
| C28 | the repo baseline (`.editorconfig`, `rustfmt.toml`, licences, `deny.toml` tables, `cliff.toml`, `lefthook.yml`, Renovate preset, tsconfig, Cargo profiles, ...) equals `templates/repo` | `--paddock` |
| C29 | each window's initial JS (from `ng build --stats-json`) is within its `windowWeight` budget in `paddock.conformance.json`; a budget above measured + 10% warns (rule 35). `window-weight [kind...] [--max kB] [--record]` prints a window's weight and records budgets | the build stats |
| C30 | Rust tests do not assert on the wall clock: in a `#[test]` fn an assertion comparing `elapsed()`, `Instant::now()`, `SystemTime::now()` (or a variable assigned from one) with a `Duration` is a warning; a hang guard of at least 10 s carries `// conformance: hang-guard`. Also a `thread::sleep` in a test that starts threads or uses channels, atomics or locks (the comment alone waives a deliberate one), and `Date.now()`/`performance.now()` differences under `expect` in TypeScript specs (not when the spec uses fake timers or `page.clock`). An `assert!` over a browser's `.close()` / `.close_gracefully(..)` (a product grace period measured against a loaded machine) is an error, not a warning: wait with a hang guard of at least 10 s (`// conformance: hang-guard`), or `// conformance: grace-free — <why>` when it ended already. A literal hang guard at or above CI's 60 s test budget is a warning (nextest kills the test first, so the guard never reports; 40 s is the family's guard). Known limits: only `assert!(..)` directly over the call is seen, not a result bound with `let` and asserted later or an `assert_eq!` form; a guard given as a constant (`CLOSE_GUARD`) is not length-checked. Use a `FakeClock` | |
| C31 | ADR numbers are unique: no two files in `docs/adr/` share a `NNNN-` number, and numbers have four digits (parallel branches each took "the next one") | |
| C34 | every `bundle.externalBin` sidecar is listed in `sidecars` of `paddock.conformance.json` (C04 starts it from the bundle with `--self-test`) and its crate handles `--self-test` (`paddock_proc::self_test::handle`) (rule 23) | |
| C35 | every `untimed` entry of a `withCallTimeout` policy (`@decryp7/paddock-ui/ipc`) is declared in `untimedCommands` of `paddock.conformance.json` with a reason (it starts a job / waits for the user, rule 12); stale declarations warn | |
| C36 | a parser crate (public `parse*`/`decode*`/`from_str`/`from_bytes` over text, bytes or a reader; or named in `parserCrates`) has a fuzz target or a never-panics test (`paddock_testkit::never_panics`, `proptest!`); `parserSkip` lists non-parsers. Warning | |
| C37 | one integration-test binary per crate: a `tests/<name>.rs` beside `tests/it/` is an error unless `ownTestBinaries` of `paddock.conformance.json` names it with a reason (a feature it needs, a process-wide log or hook, no harness); every Cargo workspace member is tested by a workflow step (`-p <name>`, or a `--workspace` step unless the workspace is in `perCrateSteps`) and, with cargo-hakari, depends on the workspace-hack crate (rule 40) | |
| C39 | tests never free a port they probe (ADR 0023): a port read off a listener that is then dropped (`bind(..).unwrap().local_addr()`, a block or fn that returns `l.local_addr()..` of its own listener, `drop(l)` before the port is used) is an error, in any Rust file; hold it with `paddock_testkit::net::HeldPort` or inject a refusing connector. A justified case carries `// conformance: freed-port — <reason>` | |
| C45 | `__command(` and `__prefix(`, the hidden constructors behind the `dialog_callable` markers (ADR 0026 §3), are called nowhere outside `crates/paddock-commands`, except in `#[cfg(test)]` code, `tests/` and doctests: a contract names a command by its marker, never by a string | |
| C48 | an app on the shared shell (a `src-tauri/Cargo.toml` with a `paddock-shell` dependency) grants its dialog windows the shell's dialog set: `src-tauri/capabilities/dialog.json` names `d-*` and grants `paddock-shell:dialog` (and `paddock-workbench:dialog` when it depends on `paddock-workbench`); without it Tauri refuses every dialog command (ADR 0026 §3) | |
| C50 | every app on the shared shell lets the shell open its start and tests it (ADR 0029, rule 43): it neither opens its main window itself (`paddock_window::tauri::start` or `create_main`, by path or `use`) nor gives `tauri::Builder` a `setup` of its own (it replaces the shell's; also after `add_to(…)` or `app(…)`, or on a binding of either), it installs `paddock_log`, and a Rust test runs the composed start through `paddock_shell::testing::start` and checks `main_window` | |
| C51 | report-only (warnings): a start or spawn is judged on run state, not a fixed wait (ADR 0031): a `START`/`SPAWN`/`LAUNCH`/`READY` + `TIMEOUT`/`LIMIT`/`DEADLINE`/`WAIT`/`GRACE`/`BUDGET`/`WINDOW` `Duration` constant or field, or `Deadline::after`/`recv_timeout`/`wait_timeout`/`Instant::now() +` inside a `start*`/`spawn*`/`launch*`/`boot*`/`connect*` fn, in non-test Rust of `crates/`, `apps/*/crates`, `apps/*/app/src-tauri` and `tools/`. Judge it with `paddock_proc::StartJudge`, or mark a real cap `// cap:`, `// external:`, `// shutdown:`, `// ui:` or `// pace:` with a reason | |
| C53 | nothing is ended by a bare pid (ADR 0033): `libc::kill`, nix `signal::kill`, `killpg`, `TerminateProcess`, `TerminateJobObject`, `taskkill`, a `kill`/`pkill` command and sysinfo's `kill`/`kill_with` on a process looked up by pid are errors in any Rust file outside `crates/paddock-proc/src/sys.rs`; end it through `paddock_proc` with a `ProcessId` (pid plus start time, checked before the kill) or through a handle or job object this process owns. A justified case carries `// conformance: pid-kill — <reason>` | |
| C54 | work a person waits for never runs at an OS starvable background level (rule 52). Errors, outside comments and tests (`#[cfg(test)]` modules, files under `tests/`): in Rust code macOS `QOS_CLASS_BACKGROUND`, `PRIO_DARWIN_BG`, `IOPOL_THROTTLE`, Windows `THREAD_MODE_BACKGROUND_BEGIN`, `PROCESS_MODE_BACKGROUND_BEGIN`, `IDLE_PRIORITY_CLASS`, `THREAD_PRIORITY_IDLE`, EcoQoS (`ProcessPowerThrottling`, `ThreadPowerThrottling`, `*_POWER_THROTTLING_EXECUTION_SPEED`), Linux `SCHED_IDLE`, `IOPRIO_CLASS_IDLE`, the thread-priority crate's idle levels (`NormalThreadSchedulePolicy::Idle`, `WinAPIThreadPriority::Idle`/`BackgroundModeBegin`; not `ThreadPriority::Min`), and raw numbers (`creation_flags(..)`, `PROCESS_CREATION_FLAGS(..)`, `THREAD_PRIORITY(..)`, `ioprio_set(..)`, directly or through a `const`, and `3 << 13`), except in a match arm that only reads a level (`X =>`); commands `taskpolicy -b`/`-c background`/`-c maintenance`/`-d throttle`, `ionice -c3`, `chrt --idle`, by basename and with flags from their own argument list (in Rust strings, plist `<string>`s and unit `Exec*=` lines); service settings: launchd `ProcessType` `Background`, `Adaptive` in a plist without `MachServices`, `LowPriorityIO`/`LowPriorityBackgroundIO` `true` paired with its own key, systemd `IOSchedulingClass=idle`, `CPUSchedulingPolicy=idle`, `paddock_service::ProcessType::Background`. A `conformance: background-qos — <why nobody waits on it>` comment on the line, or on a comment-only line above, waives one; a waiver in a string does not count, and one that waives nothing warns. Linear in the file size (a test counts the steps). (The runner also warns, W003, about a `paddock.waivers.toml` entry that waives nothing.) | |
| C56 | every app that calls `.build_info(` passes `option_env!` of exactly the names the `build-env` action sets (read from `.gitea/actions/build-env/action.yml`: `PADDOCK_BUILD_COMMIT`, `PADDOCK_BUILD_DATE`, in that order); other names leave the About page with no commit or date (Dyno 1.0 used `DYNO_COMMIT` and `DYNO_BUILT`) | |
| C57 | no tracked file is a symlink (`git ls-files -s -- .` mode 120000; a submodule is not one), except the paths in `ALLOWED_SYMLINKS` of the check (empty): a link names one machine's path and a trailing-slash ignore (`node_modules/`) cannot hide it. Skipped outside a git checkout | |
| C61 | report-only (warnings): an app loads the family Material bridge and no Material theme of its own. `app/angular.json` lists `paddock-ui/css/material.css` among a build target's styles (without it Material draws 56px fields with large type and pill buttons: Pitcrew), and no `app/src/**/*.scss` includes `mat.theme` or another `mat.*` theme mixin (Pitwall sets density -2 on `:root`; Debrief and Pitcrew carry a theme). Material 22.2.4's `mat.theme` emits density tokens only when the scale is not 0. Reads only `build.options.styles` of each project. A family-section `--mat-*` token on an app's `:root` or `html` is dead (the `body` section wins): tune through `--pd-*` | |
| C71 | tests do not settle on real time (opt-in: `"timingLint": true` in `paddock.conformance.json`; the Paddock repo scans every app). Errors on `waitForTimeout(`, a `setTimeout`/`sleep`/`delay` wait, a wait on `requestAnimationFrame` turns and a locator `.count()` read once then used to `.nth(..)` later, in spec, test and e2e files; and on `thread::sleep` / `tokio::time::sleep` in the `#[cfg(test)]` items and test files of Rust. Exempt per call: `// conformance: hang-guard — <why>` naming a bound of at least 10 s, a TypeScript call under `useTestClock()` / `frozenClock(`, a Rust call that takes a `FakeClock` / `ManualPace` / `TestSeams`. Today's sites are in `conformance/baselines/C71.json` (file + hash of the line; no expiry; may only shrink, and the check errors on any entry the merge base's baseline lacks; `node conformance/bin/paddock-timing-baseline.mjs` lists, `--shrink` removes stale entries). ADR 0041 (first slice: see its amendment) | |
| C43 | a sticky table header stays above its rows: a header or footer row (`position: sticky` with `top`, `bottom`, `inset-block-*` or an `inset` shorthand) sets a z-index of at least 3 (`--pd-z-table-header`), a sticky column's header cell at least 4 (`--pd-z-table-corner`), and a sticky column in the same app sits below the lowest header (`--pd-z-table-sticky-column` 2); a sticky element that is not a table row says `/* conformance: not-a-header */` (ADR 0027) | |
| C32 | the translation catalogs are current: `paddock-i18n build --check --no-extract` over the app's `paddock-i18n.json` (every catalog equals its sources, every runtime JSON equals its catalog); a missing config warns | `npm ci` (the app's installed `@decryp7/paddock-ui`) |
| C33 | every window is checked with axe: an e2e spec runs axe, and every dialog of `dialog-contracts.json` is covered (`axeEveryDialog`, or the id in a spec that runs axe; `a11y.skip` names the exceptions with a reason). Warns when no spec walks a keyboard-only flow or tests the app's accent with `accentProblems` | |
| C98 | a temporary opt-out of the read retry ends on time (ADR 0058, rule 30): in `ui/projects/paddock-ui` (specs skipped), each `withRequestStatus({ retry: false, kind: 'temporary', ... })` names `until: 'YYYY-MM-DD'`; a missing date or one before today is an error. The declaration itself is checked by the unit spec `request-status-stores.spec.ts`; the calendar is here so the unit suite never turns red by date. Adopt `retry: true`, or move the date with the reason in the commit | |
| C59 | every app that wires the shared updater says how it installs (rule 40; Pitcrew 1.1 downloaded updates it could never install): in the production Rust of an app's `src-tauri/` (comments, strings, `tests/` and `#[cfg(test)]` items ignored) a **whole file** that calls `UpdatesBuilder::new(` must also call `.installer(` (the `PluginInstaller`) or `.no_install()`. Known limits: a file with two builders passes when either declares; a builder finished in another file or helper is judged only where `UpdatesBuilder::new(` is; macros and `use … as` renames are not followed. The net under them: `tauri::install` refuses a builder that says neither (`InstallError::InstallerUndeclared`), at the app's start with or without a key | |
| C99 | no shipped process uses the macOS Keychain (rule 72, ADR 0057 §3). Layers 1 and 2 scan manifests and sources; layers 3 and 4 probe the macOS dependency graph (`cargo tree` on both Apple triples, `--all-features`) of every workspace a `paddock.ci.json` names and of `crates/`, and read the dependencies' sources for the SecItem/SecKeychain symbols; layer 5 is the Clippy preset (C13). Every graph finding has its own path `<dir>/Cargo.lock#<key>` so a waiver names exactly it. "Check broken" and "not measured" are unwaivable errors; an unreviewed prebuilt binary is an error (`#bin:<name>@<version>`); a workspace with no clippy.toml (the demos) is a warning "Clippy on macOS: not measured". `--no-graph` skips layers 3 and 4 for offline local runs (a warning says so; CI never uses it). | |
| C112 | every Markdown table in a tracked `.md` file renders as a table in GFM, which Gitea uses (rule 40; `crates/README.md` had a blank line inside its table and 28 rows showed as plain text). Per table: a header and delimiter row with the same cell count, every row with exactly the header's cell count (a literal pipe is written `\|`, also inside backticks, because backticks do not protect a pipe), no blank line or plain text line between rows, no row cut off from its table. Fenced code is skipped. | |
| C79 | binaries that can start supervised children call the orphan guardian (ADR 0076 amendment 2026-11-09, rule 53): every `src/main.rs`, `src/bin/*.rs` and `src/bin/*/main.rs` of a package whose dependency closure in its workspace's `Cargo.lock` reaches `paddock-proc` calls `paddock_proc::guardian::run_if_requested()`. `ADOPTING` in the check lists the mains that do not yet (a warning each) and only shrinks: a main missing from it, or a listed one that has the call, is an error. Test binaries opt in with `enable_in_test_binary` and are not checked. Add the call first in `main` | |
| C85 | report-only (warnings; `--strict C85` makes them errors; ADR 0052 §1.4, WP-A4): an app's `features.toml` has one `[[feature]]` row per user-facing feature, and each row names its real-engine test (`tests.tauri`, `path#title`, looked up: a Rust title must be a `fn` under a `#[test]` attribute), its visible-outcome and real-layout rows, and, for a feature that depends on a platform (a privileged wire, or not all three OS), a vm-smoke step id (`os:step`, looked up in `scripts/vm-smoke/manifest.mjs` through `--paddock`) for each OS it lists, Windows included. A test that does not exist yet is said so in `owed` with `owed_why` and a bounded `owed_until` (a tag that does not exist yet and is at most the app's next release line, or a real date within 60 days of the commit that wrote it; in a release run, `--version`, the owed findings are errors); `na` with `na_why` marks a kind of test that does not apply; a row never names a test it does not have, and a skipped TypeScript test does not count. Surface and wire kinds are an allow-list. Fails when a route (`shell/nav.ts`), dialog (`generated/dialog-contracts.ts`, compared with `shell/contributions.ts`) and Settings page (`shell/contributions.ts`), job kind (`spec("..")` or `JobSpec { kind: ".." }` outside `#[cfg(test)]` items in `src-tauri/src/app_commands.rs`) or command of `generated/app-commands.ts` belongs to no row (or `[[unlisted]]`, capped at 5, or `wiring.toml`'s `[[unwired]]`), when a row names one the app no longer has, and when a source it reads is missing ("not measured", exit 6) or yields nothing ("check broken", exit 7): both are errors no waiver covers. Until ADR 0052 WP-A1 (`wire.toml`) and ADR 0037 WP-7 (the real-app kit) exist, the command table stands in for the request kinds and `tauri` rows are `owed`. Run: `--app <app> --root apps/<app> --only C85 --paddock .` | |

## Sync (X27, X43)

```sh
paddock-conformance sync --app pitcrew [--root <app>] [--paddock <paddock checkout>] [--write]
```

Shows what differs from Paddock's `templates/repo` (C28) and `docs/blocks` (C23); `--write` repairs it. A doc block sits
between `<!-- paddock:block NAME rev=XXXXXXXX -->` and `<!-- /paddock:block NAME -->`; the section an app had under the
same heading is replaced the first time, text outside the markers is never touched. The `rules` block is derived from
`docs/rules.md`. Change the text in Paddock first. Not every app section moves: put what is specific to the app in its
own sections ("Design principles in Pitcrew").

## Third-party notices (X42)

```sh
paddock-notices --app Pitcrew [--root .] [--manifest app/src-tauri/Cargo.toml ...] [--npm app] [--out app/src-tauri/resources]
                [--rust-json cargo-about.json ...] [--about about.toml] [--check]
```

Writes `THIRD_PARTY_NOTICES.md` (Rust licences from `cargo about generate` with the app's `about.toml`, npm production
dependencies from `package-lock.json` and `node_modules`), deterministic. `--check` fails when the committed file is stale;
a package none of whose licence options is permissive fails both modes. The allowed licences are one list
(`src/notices/licences.mjs`), equal to `templates/repo/deny.toml` and `about.toml` (a test keeps them equal). Install first:
`cargo install cargo-about --version 0.9.2 --locked --features cli`, `npm ci --omit=dev --ignore-scripts`.

In the monorepo an app states its facts once, in the `notices` block of `apps/<app>/paddock.ci.json`
(`{ "name", "manifests": [...], "npm": "app", "out": ["app/src-tauri/resources"] }`, paths relative to the app), and runs
`scripts/notices.sh <app> [--check]` (`paddock-notices --config ...`). `npm` may be a workspace of the root lock: only its
production closure is listed. An optional `components` list names markdown files (software that is neither a crate nor an npm package, such as a vendor installer the app carries) that are appended verbatim under "Bundled components". Without an `about.toml` of its own an app uses `templates/repo/about.toml`. CI (`app-gates`) runs
the `--check` on the Linux leg of every app with the block.

## Configuration

`paddock.conformance.json` (optional): `app`, `appDir`, `tauriDir`, `workflowsDir`, `sidecars` (`[{ "name": "x-worker", "args": ["--self-test"] }]`),
`nonGates` (job ids the release job need not wait for), `commitRange`, `appName` (display name for templates; default the id with a capital),
`docBlocks` (`{ "AGENTS.md": ["rules", ...] }` replaces the default targets), `untimedCommands` (`{ "method": "why it is never timed out" }`, C35), `parserCrates` / `parserSkip` (C36), `ownTestBinaries` (`{ "crates/x/tests/y.rs": "why it needs its own binary" }`) and `perCrateSteps` (workspace folders whose members each need their own CI step) (C37), `windowWeight` (`{ stats, windows: { main: { maxKb, entries? } } }`), `baselineSkip` (template paths this app does not take; prefer a waiver).

`paddock.waivers.toml`:

```toml
[[waiver]]
check = "C16"
path = "app/src/app/core/mock/*"   # exact path, or a prefix ending in *
reason = "mock data of the parser registry, not the app version"
expires = "2026-12-01"
```

A waiver past its date, or without a reason or date, is an error itself (W001, W002); a C99 waiver with a `*` in its path is too (W004: it names one exact path, `Cargo.lock#<key>` as the finding prints it, and never covers "check broken" or "not measured"); a live waiver for a check that ran and waived nothing is a warning (W003: the finding is gone, delete the waiver). No silent skips.

## CI scripts (`ci/`) and the shared actions

`pin-runtime.mjs` pins a `paddock-runtime` catalogue (Chromium's `chromium.toml`, FFmpeg's, llama.cpp's): it downloads each
platform's archive once, measures size and SHA-256 while reading and rewrites `version`, `url`, `size` and `sha256` in place
(`--version <v>` moves the URLs; `--chrome-for-testing [channel]` takes Chrome for Testing's version and URLs). It replaces
Spotter's `pin-chromium.mjs`. `install-test-browser.mjs --catalogue <toml>` installs the build such a catalogue pins (it replaces
Debrief's `install-test-browser.mjs`; `--pin` and `--rust-source` stay until the apps move to the catalogue).

`plan.mjs`, `build-env.mjs`, `release-notes.mjs`, `install-test-browser.mjs` and `tauri-bundle.sh` are what the composite actions
in `.gitea/actions/` run (see its README); they are plain Node and bash, tested here, and work from a Paddock checkout. A
workflow that uses the actions can drop the steps they replace, and C03, C04 and C15 accept them.

## Release scripts (`release/`)

`publish.mjs`, `latest-json.mjs` (the apps' copies, unified; output equals Pitwall's), `strip-keys.mjs`
(`eval "$(node release/strip-keys.mjs)"`), `macos-verify-bundle.sh` (captures command output before matching),
`windows-crt-imports.mjs --src-tauri <dir> --bin-dir <dir> [--config <file.json>]...` (ADR 0032: fails when a binary the
Windows installer ships, named by the Tauri configuration, imports the VC++ runtime, VCRUNTIME140.dll, MSVCP140.dll and
kin, or is missing; `<file-or-folder>...` checks given paths; pure Node, no dumpbin; `tauri-bundle.sh` runs it after
every NSIS build).

`ci/tauri-tools.mjs` pins the bundler tools `tauri build` would download from GitHub (ADR 0085 WP-9): linuxdeploy, AppRun and
linuxdeploy-plugin-appimage (the dated build `1-alpha-20250213-1`) for an AppImage on Linux, the NSIS 3.13 zip and
nsis_tauri_utils.dll for an NSIS installer on Windows (URL and SHA-256, read from tauri-bundler 2.10.3's `linuxdeploy.rs` and
`nsis/mod.rs`). `install` empties `<build folder>/.tauri` (cargo metadata's `target_directory`, the folder `useLocalToolsDir`
makes Tauri use), puts each verified file there (from the cache folder, else a download; a tag run reads no cache) and writes
the manifest `<build folder>/.tauri.sha256` (the `--tauri-pins` format of `./dev ci release-isolation verify`); `key` prints
the actions/cache key; `check` compares the folder with the manifest after the build (anything unpinned or changed warns, and
fails a tag run). A test ties the pins to the lockfile's `@tauri-apps/cli`. `tauri-bundle.sh` and Debrief's bundle step run
`install` and `check` around every NSIS and AppImage build.

The updater key (ADR 0085 D7, rule 78): `ci/updater-key.sh` is sourced by `tauri-bundle.sh` and Debrief's bundle step; it moves
the key out of the environment before `npm ci` and any prepare command, refuses a private key on a CI run that is not a tag
and a tag without one, makes a throw-away key pair on other CI runs, and hands a key to `tauri build` only, by `export` and
`exec` (`updater_key_exec`), never on a command line. `ci/before-build.mjs` runs the app's `beforeBuildCommand` before `tauri
build`, without the key, and `tauri build` gets it emptied. `release/updater-key-check.mjs --pubkey <key> --dir <folder>`
verifies every `.sig` against the one public key the build carries. `ci/tag-node-caches.sh [<angular dir>]` gives a tag run a
new npm cache and removes a kept Angular cache link, in place of the node-caches action.

## Sidecars (`paddock-sidecar`, plan X34)

One mechanism for the helper binaries an app bundles (`bundle.externalBin`), replacing the per-app `sidecar.mjs` scripts:

```sh
# beforeBuildCommand: build the sidecars and put them at src-tauri/binaries/<name>-<target triple>[.exe]
paddock-sidecar build --root src-tauri --out src-tauri/binaries --sidecar pitcrew-service
paddock-sidecar build --root . --out app/src-tauri/binaries --profile dist --sidecar dyno-agent --sidecar dyno-helper --sidecar dyno=dyno-cli
# rule 23, in CI on the built bundle: every sidecar starts with --self-test and exits 0
paddock-sidecar self-test --dir "$APP/Contents/MacOS" --sidecar pitcrew-service
```

`--sidecar name[=package[:bin]]` (repeat). Honours `TAURI_ENV_TARGET_TRIPLE`, `CARGO_TARGET_DIR` and `PADDOCK_CARGO_PROFILE`; `--dev`
builds the debug profile, `universal-apple-darwin` builds both halves and joins them with `lipo`, an empty built file is an error. An
app with extras (Debrief's libduckdb) imports `buildSidecars` from `@decryp7/paddock-conformance/sidecar` and adds them. The Rust
side: `paddock_build::Build` (build.rs: the Windows manifest in every binary, empty placeholders, rerun triggers) and
`paddock_proc::self_test::handle` (the sidecar's `--self-test`).

## Hardening workflows (plan X41)

The reusable fuzz, mutation and never-panics workflows of the separate repositories were deleted when nothing called them (ADR 0060); Debrief's own
`debrief-fuzz.yml` and `debrief-mutants.yml` run them for Debrief. C36 finds parser crates without a fuzz target or a never-panics test; `paddock_testkit::{corpus,
never_panics}` is the dependency-free test for a parser. Debrief's `corpus.yml` stays in Debrief (its data and pins are its own); the
`plan` and `nightly-mark` actions it hand-copied are the shared ones.
