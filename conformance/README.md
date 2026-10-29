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
| C30 | Rust tests do not assert on the wall clock: in a `#[test]` fn an assertion comparing `elapsed()`, `Instant::now()`, `SystemTime::now()` (or a variable assigned from one) with a `Duration` is a warning; a hang guard of at least 10 s carries `// conformance: hang-guard`. Also a `thread::sleep` in a test that starts threads or uses channels, atomics or locks (the comment alone waives a deliberate one), and `Date.now()`/`performance.now()` differences under `expect` in TypeScript specs (not when the spec uses fake timers or `page.clock`). Use a `FakeClock` | |
| C31 | ADR numbers are unique: no two files in `docs/adr/` share a `NNNN-` number, and numbers have four digits (parallel branches each took "the next one") | |
| C34 | every `bundle.externalBin` sidecar is listed in `sidecars` of `paddock.conformance.json` (C04 starts it from the bundle with `--self-test`) and its crate handles `--self-test` (`paddock_proc::self_test::handle`) (rule 23) | |
| C35 | every `untimed` entry of a `withCallTimeout` policy (`@decryp7/paddock-ui/ipc`) is declared in `untimedCommands` of `paddock.conformance.json` with a reason (it starts a job / waits for the user, rule 12); stale declarations warn | |
| C36 | a parser crate (public `parse*`/`decode*`/`from_str`/`from_bytes` over text, bytes or a reader; or named in `parserCrates`) has a fuzz target or a never-panics test (`paddock_testkit::never_panics`, `proptest!`); `parserSkip` lists non-parsers. Warning | |
| C37 | one integration-test binary per crate: a `tests/<name>.rs` beside `tests/it/` is an error unless `ownTestBinaries` of `paddock.conformance.json` names it with a reason (a feature it needs, a process-wide log or hook, no harness); every Cargo workspace member is tested by a workflow step (`-p <name>`, or a `--workspace` step unless the workspace is in `perCrateSteps`) and, with cargo-hakari, depends on the workspace-hack crate (rule 40) | |
| C39 | tests never free a port they probe (ADR 0023): a port read off a listener that is then dropped (`bind(..).unwrap().local_addr()`, a block or fn that returns `l.local_addr()..` of its own listener, `drop(l)` before the port is used) is an error, in any Rust file; hold it with `paddock_testkit::net::HeldPort` or inject a refusing connector. A justified case carries `// conformance: freed-port — <reason>` | |
| C45 | `__command(` and `__prefix(`, the hidden constructors behind the `dialog_callable` markers (ADR 0026 §3), are called nowhere outside `crates/paddock-commands`, except in `#[cfg(test)]` code, `tests/` and doctests: a contract names a command by its marker, never by a string | |
| C48 | an app on the shared shell (a `src-tauri/Cargo.toml` with a `paddock-shell` dependency) grants its dialog windows the shell's dialog set: `src-tauri/capabilities/dialog.json` names `d-*` and grants `paddock-shell:dialog` (and `paddock-workbench:dialog` when it depends on `paddock-workbench`); without it Tauri refuses every dialog command (ADR 0026 §3) | |
| C50 | every app on the shared shell lets the shell open its start and tests it (ADR 0029, rule 43): no `paddock_window::tauri::start` in the app's own sources, and a Rust test that runs the composed start through `paddock_shell::testing::start` |
| C43 | a sticky table header stays above its rows: a header or footer row (`position: sticky` with `top`, `bottom`, `inset-block-*` or an `inset` shorthand) sets a z-index of at least 3 (`--pd-z-table-header`), a sticky column's header cell at least 4 (`--pd-z-table-corner`), and a sticky column in the same app sits below the lowest header (`--pd-z-table-sticky-column` 2); a sticky element that is not a table row says `/* conformance: not-a-header */` (ADR 0027) | |
| C32 | the translation catalogs are current: `paddock-i18n build --check --no-extract` over the app's `paddock-i18n.json` (every catalog equals its sources, every runtime JSON equals its catalog); a missing config warns | `npm ci` (the app's installed `@decryp7/paddock-ui`) |
| C33 | every window is checked with axe: an e2e spec runs axe, and every dialog of `dialog-contracts.json` is covered (`axeEveryDialog`, or the id in a spec that runs axe; `a11y.skip` names the exceptions with a reason). Warns when no spec walks a keyboard-only flow or tests the app's accent with `accentProblems` | |

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
production closure is listed. Without an `about.toml` of its own an app uses `templates/repo/about.toml`. CI (`app-gates`) runs
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

A waiver past its date, or without a reason or date, is an error itself (W001, W002). No silent skips.

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
(`eval "$(node release/strip-keys.mjs)"`), `macos-verify-bundle.sh` (captures command output before matching).

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

Reusable workflows in `.gitea/workflows/`: `app-fuzz.yml` (nightly cargo-fuzz in shards, `shards` is a JSON list), `app-mutants.yml`
(cargo-mutants weekly and on the changed lines of a pull request) and `app-never-panics.yml` (the proptest / `never_panics` tests with
many cases, release mode). C36 finds parser crates without a fuzz target or a never-panics test; `paddock_testkit::{corpus,
never_panics}` is the dependency-free test for a parser. Debrief's `corpus.yml` stays in Debrief (its data and pins are its own); the
`plan` and `nightly-mark` actions it hand-copied are the shared ones.
