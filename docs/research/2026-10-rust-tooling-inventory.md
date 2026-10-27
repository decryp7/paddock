# Rust tooling: inventory and measurements (2026-10-27)

Evidence for [ADR 0022](../adr/0022-rust-developer-and-ci-tool.md). Research only: nothing in the build changed.
**Verified** means measured or read in the tree (`origin/main` at `932b1728b`); **read** means taken from documentation.

## Method (verified)

- **Files:** `git ls-files` under `scripts/`, `conformance/`, `tools/`, `ci-image/`, `diagrams/{scripts,tools}/`,
  `icons/tools/`, `docs/images/src/`, `templates/repo/scripts/`, `.gitea/actions/*.yml`, `apps/*/scripts/`,
  `apps/*/app/scripts/`, `apps/*/docs/diagrams/src/` and two shell scripts inside Debrief crates; extensions `.mjs .js .cjs
  .ts .sh .ps1 .py .yml`; tests (`*.test.*`, `test/`, `tests/`) and fixtures counted apart. 309 files, 27,905 lines; 94
  test files, 8,406 lines.
- **fix:/ci:** `git log --since=2026-10-13 --no-merges origin/main`, subjects starting with `fix` or `ci`, de-duplicated by
  subject (the imported app histories repeat some commits: 154 raw commits on tooling paths, 110 distinct subjects; the
  research note counted 111). Per file: the number of distinct subjects that touched it.
- **OS refs:** occurrences of `win32 darwin windows macos codesign hdiutil xattr pwsh powershell .exe junction mklink
  process.platform RUNNER_OS runner.os uname notarytool spctl lipo otool install_name_tool launchctl systemctl appimage
  .dmg .msi nsis signtool` (case-insensitive). A proxy for per-OS branches, not a count of them.
- **Tests:** "yes" when a test file of the same area names the script (a heuristic; it can over-count).
- **Classes:** see ADR 0022 §1. After WP-C1–C6 the per-app copies are gone; they are listed last for completeness.

## Measurements (verified, Mac mini M4 Pro, 14 cores, load 3–4, warm file cache)

| What | Result |
|---|---|
| `node -e 0` | 31–33 ms |
| Rust binary, no work | 10–11 ms (first start after the build 0.3 s) |
| `bash -c true` | 9 ms |
| `node scripts/affected.mjs <base> HEAD`, base 1, 20 and 200 commits back | 0.355–0.374 s |
| `cargo metadata --no-deps --offline` on one workspace | 31–32 ms (15 workspaces) |
| `node conformance/bin/paddock-conformance.mjs --app paddock --root .` (all checks) | 2.95 s |
| Same, CI subset of 12 checks | 2.88–2.91 s; per check: C36 2.02 s, C30 1.11 s, the others 0.05–0.29 s (each including Node start and the walk) |
| Conformance on `apps/debrief` | 0.82 s |
| Conformance tests (`node --test test/*.test.mjs`) | 240 tests, 1.76 s |
| Read, regex and SHA-256 over the 2,691 tracked `.rs` files | Node (one thread) 0.12 s; Rust (`ignore` walk, rayon) 0.09 s |
| Probe tool (serde, serde_json, toml, regex, ignore, rayon, sha2, ureq, lexopt), `CARGO_BUILD_JOBS=2`, no sccache | cold release 33 s, cold debug 17 s, release after touching `main.rs` 1 s, up to date 0.06 s; binary 2.9 MB |
| Same with sccache, after `cargo clean` | first pass 25 s, second pass 5 s |
| `crates/`: `cargo build --release -p paddock-cli -p paddock-proc -p paddock-env` (with the workspace-hack), cold, 2 jobs | 41 s release, 24 s debug |
| `git rev-parse HEAD:<dir>` | 16 ms |

CI step timings (verified, from the research note's saved job records: 142 runs, 1,070 executed jobs, 2026-10-22 to
2026-10-27): `stable-workspace` 9.4 s average (183 runs), "Third-party notices are up to date" 113 s (cargo), "Licences,
advisories and sources" 22.5 s, "Publish dry run" 7.8 s, "Pinned test browser" 4.7 s, `install-test-browser` 2.8 s,
"Release facts" 0.2 s, "Affected projects" 0.3 s, macOS `.app`/DMG verification 0.5–1.0 s, `setup-node` 6.0 s and
"Install" 7.6 s on average across UI jobs. Jobs without `npm ci`: plan, conformance, tools (Node from the image). Saved
job logs (`job-*.log`, 29 files) show `npm ci` taking 3–15 s.

## Inventory per file (verified)

Lines are physical lines; for composite actions the shell inside `run:` blocks is given in brackets.

### Rust, phase 1

| File | Lang | Lines | OS refs | Tests | fix:/ci: |
|---|---|---|---|---|---|
| `apps/debrief/app/scripts/build-sidecars.mjs` | JS | 138 | 19 | yes | 7 |
| `apps/debrief/app/scripts/libduckdb.mjs` | JS | 86 | 15 | yes | 2 |
| `apps/debrief/crates/debrief-store-duck/scripts/fetch-libduckdb.sh` | sh | 57 | 10 | yes | 0 |
| `apps/debrief/scripts/ci/libduckdb-env.sh` | sh | 55 | 8 | yes | 1 |
| `apps/debrief/scripts/macos/build-app.sh` | sh | 17 | 4 | none | 1 |
| `apps/debrief/scripts/macos/dev-signing.sh` | sh | 58 | 12 | none | 1 |
| `apps/debrief/scripts/update-ffmpeg-runtimes.py` | py | 147 | 5 | none | 0 |
| `apps/debrief/scripts/update-llama-runtimes.py` | py | 149 | 6 | none | 0 |
| `apps/dyno/scripts/check-bundle.sh` | sh | 73 | 14 | none | 2 |
| `apps/spotter/scripts/install-test-browser.mjs` | JS | 55 | 8 | yes | 1 |
| `apps/spotter/scripts/pin-chromium.mjs` | JS | 72 | 6 | yes | 0 |
| `conformance/bin/paddock-sidecar.mjs` | JS | 4 | 0 | yes | 0 |
| `conformance/ci/build-env.mjs` | JS | 33 | 0 | yes | 2 |
| `conformance/ci/build-folder.sh` | sh | 39 | 6 | yes | 1 |
| `conformance/ci/install-test-browser.mjs` | JS | 151 | 15 | yes | 3 |
| `conformance/ci/pin-runtime.mjs` | JS | 127 | 4 | yes | 1 |
| `conformance/ci/plan.mjs` | JS | 38 | 0 | yes | 2 |
| `conformance/ci/release-notes.mjs` | JS | 44 | 0 | yes | 2 |
| `conformance/ci/runtime-catalogue.mjs` | JS | 118 | 9 | yes | 0 |
| `conformance/ci/tauri-bundle.sh` | sh | 53 | 7 | yes | 1 |
| `conformance/release/latest-json.mjs` | JS | 118 | 14 | yes | 1 |
| `conformance/release/macos-verify-bundle.sh` | sh | 116 | 22 | yes | 0 |
| `conformance/release/publish.mjs` | JS | 117 | 0 | yes | 2 |
| `conformance/release/strip-keys.mjs` | JS | 52 | 0 | yes | 1 |
| `conformance/src/sidecar/sidecar.mjs` | JS | 154 | 13 | yes | 0 |
| `scripts/affected.mjs` | JS | 561 | 0 | yes | 4 |
| `scripts/ci-timings.mjs` | JS | 355 | 0 | none | 0 |
| `scripts/coverage-gate.mjs` | JS | 49 | 0 | yes | 1 |
| `scripts/demo-release.mjs` | JS | 197 | 5 | yes | 1 |
| `scripts/demo/bundle.sh` | sh | 43 | 11 | yes | 0 |
| `scripts/demo/verify-macos.sh` | sh | 83 | 20 | yes | 0 |

### Rust, phase 2

| File | Lang | Lines | OS refs | Tests | fix:/ci: |
|---|---|---|---|---|---|
| `apps/debrief/app/scripts/window-weight.mjs` | JS | 107 | 2 | yes | 0 |
| `apps/debrief/crates/debrief-pdf/fonts/build-fonts.sh` | sh | 49 | 1 | none | 0 |
| `apps/debrief/scripts/build-plugins.sh` | sh | 43 | 3 | none | 0 |
| `apps/debrief/scripts/ci/e2e-linux-local.sh` | sh | 45 | 0 | none | 1 |
| `apps/debrief/scripts/ci/macos-smoke.sh` | sh | 102 | 19 | yes | 1 |
| `apps/debrief/scripts/ci/smoke-linux-docker.sh` | sh | 46 | 0 | none | 0 |
| `apps/debrief/scripts/ci/startup-bench.mjs` | JS | 151 | 11 | yes | 0 |
| `apps/debrief/scripts/ci/windows-node-modules.sh` | sh | 52 | 18 | none | 1 |
| `apps/debrief/scripts/clean-build-dirs.ps1` | ps1 | 56 | 0 | none | 0 |
| `apps/debrief/scripts/notices/generate.mjs` | JS | 213 | 1 | yes | 2 |
| `apps/dyno/scripts/ci-prune.sh` | sh | 20 | 0 | none | 1 |
| `apps/dyno/scripts/linux-container.sh` | sh | 51 | 0 | none | 0 |
| `apps/dyno/scripts/linux-verify.sh` | sh | 16 | 0 | none | 0 |
| `apps/pitcrew/scripts/linux-verify.sh` | sh | 62 | 7 | none | 0 |
| `apps/pitcrew/scripts/macos-verify.sh` | sh | 56 | 14 | yes | 0 |
| `apps/pitwall/scripts/linux-container-inner.sh` | sh | 68 | 8 | none | 0 |
| `apps/pitwall/scripts/linux-container-verify.sh` | sh | 28 | 1 | none | 0 |
| `apps/pitwall/scripts/linux-verify.sh` | sh | 57 | 7 | none | 0 |
| `apps/pitwall/scripts/macos-verify.sh` | sh | 106 | 22 | yes | 0 |
| `conformance/bin/paddock-notices.mjs` | JS | 4 | 0 | none | 0 |
| `conformance/src/notices/cli.mjs` | JS | 99 | 0 | yes | 0 |
| `conformance/src/notices/generate.mjs` | JS | 145 | 1 | yes | 0 |
| `conformance/src/notices/licences.mjs` | JS | 13 | 0 | yes | 0 |
| `conformance/src/window-weight/cli.mjs` | JS | 63 | 10 | yes | 0 |
| `conformance/src/window-weight/measure.mjs` | JS | 89 | 2 | yes | 0 |
| `scripts/new-app.mjs` | JS | 554 | 3 | yes | 4 |
| `scripts/publish.mjs` | JS | 436 | 0 | yes | 4 |
| `scripts/shell-demo/build-linux-arm64.sh` | sh | 48 | 1 | none | 0 |
| `scripts/shell-demo/smoke-desktop-linux.sh` | sh | 85 | 10 | none | 0 |
| `scripts/shell-demo/smoke-service-update-linux.sh` | sh | 67 | 7 | none | 0 |
| `scripts/shell-demo/smoke-service-update.sh` | sh | 84 | 7 | none | 1 |
| `tools/host-cleanup.sh` | sh | 47 | 1 | yes | 0 |
| `tools/linux-container.sh` | sh | 76 | 0 | yes | 0 |
| `tools/macos-vm-smoke.sh` | sh | 171 | 18 | yes | 0 |
| `tools/prune-build-folders.ps1` | ps1 | 57 | 0 | yes | 0 |
| `tools/prune-build-folders.sh` | sh | 38 | 1 | yes | 1 |
| `tools/verify-installed.sh` | sh | 117 | 21 | yes | 0 |

### Rust, phase 3 (rules engine)

| File | Lang | Lines | OS refs | Tests | fix:/ci: |
|---|---|---|---|---|---|
| `apps/debrief/scripts/ci/check-quarantine.mjs` | JS | 45 | 0 | yes | 1 |
| `apps/debrief/scripts/ci/check-smoke-dialogs.mjs` | JS | 31 | 0 | yes | 1 |
| `apps/debrief/scripts/ci/check-temp-leaks.mjs` | JS | 93 | 0 | yes | 2 |
| `apps/debrief/scripts/ci/check-workflows.py` | py | 35 | 0 | none | 1 |
| `apps/dyno/scripts/check-capabilities.mjs` | JS | 94 | 17 | yes | 1 |
| `apps/pitwall/scripts/ci/privacy-check.sh` | sh | 22 | 0 | none | 1 |
| `conformance/bin/paddock-conformance.mjs` | JS | 4 | 0 | yes | 0 |
| `conformance/src/checks/C03-icons.mjs` | JS | 50 | 0 | none | 1 |
| `conformance/src/checks/C04-macos-signing.mjs` | JS | 87 | 28 | none | 1 |
| `conformance/src/checks/C06-key-strip.mjs` | JS | 37 | 1 | none | 0 |
| `conformance/src/checks/C07-release-preflight.mjs` | JS | 116 | 2 | none | 0 |
| `conformance/src/checks/C08-release-artifacts.mjs` | JS | 89 | 2 | none | 0 |
| `conformance/src/checks/C13-rust-lints.mjs` | JS | 77 | 0 | yes | 0 |
| `conformance/src/checks/C14-hermetic.mjs` | JS | 21 | 0 | none | 0 |
| `conformance/src/checks/C15-installed-chrome.mjs` | JS | 32 | 1 | none | 1 |
| `conformance/src/checks/C16-hardcoded-version.mjs` | JS | 39 | 0 | none | 0 |
| `conformance/src/checks/C23-process.mjs` | JS | 55 | 0 | none | 0 |
| `conformance/src/checks/C24-reviewed-images.mjs` | JS | 73 | 0 | yes | 0 |
| `conformance/src/checks/C28-baseline.mjs` | JS | 19 | 0 | none | 0 |
| `conformance/src/checks/C29-window-weight.mjs` | JS | 44 | 6 | none | 0 |
| `conformance/src/checks/C30-wall-clock.mjs` | JS | 190 | 0 | yes | 1 |
| `conformance/src/checks/C31-adr-numbers.mjs` | JS | 28 | 0 | none | 1 |
| `conformance/src/checks/C32-i18n-catalogs.mjs` | JS | 40 | 0 | yes | 0 |
| `conformance/src/checks/C33-a11y-coverage.mjs` | JS | 58 | 1 | none | 0 |
| `conformance/src/checks/C34-sidecar-self-test.mjs` | JS | 56 | 0 | yes | 0 |
| `conformance/src/checks/C35-untimed-calls.mjs` | JS | 76 | 0 | yes | 0 |
| `conformance/src/checks/C36-parser-hardening.mjs` | JS | 52 | 0 | yes | 0 |
| `conformance/src/checks/C37-test-binaries.mjs` | JS | 111 | 0 | yes | 1 |
| `conformance/src/checks/index.mjs` | JS | 24 | 1 | yes | 1 |
| `conformance/src/cli.mjs` | JS | 97 | 0 | yes | 0 |
| `conformance/src/commit-lint.mjs` | JS | 72 | 0 | yes | 0 |
| `conformance/src/context.mjs` | JS | 66 | 0 | none | 0 |
| `conformance/src/finding.mjs` | JS | 6 | 0 | yes | 0 |
| `conformance/src/hermetic.mjs` | JS | 94 | 2 | yes | 0 |
| `conformance/src/index.mjs` | JS | 4 | 0 | yes | 0 |
| `conformance/src/report.mjs` | JS | 38 | 0 | yes | 0 |
| `conformance/src/runner.mjs` | JS | 39 | 0 | yes | 0 |
| `conformance/src/sync/baseline.mjs` | JS | 115 | 0 | yes | 0 |
| `conformance/src/sync/blocks.mjs` | JS | 113 | 0 | yes | 0 |
| `conformance/src/sync/fill.mjs` | JS | 16 | 0 | yes | 0 |
| `conformance/src/sync/main.mjs` | JS | 32 | 0 | yes | 0 |
| `conformance/src/sync/paddock-root.mjs` | JS | 18 | 0 | none | 0 |
| `conformance/src/sync/plan.mjs` | JS | 70 | 0 | yes | 0 |
| `conformance/src/util/fs.mjs` | JS | 63 | 0 | yes | 0 |
| `conformance/src/util/is-main.mjs` | JS | 17 | 0 | yes | 1 |
| `conformance/src/util/png.mjs` | JS | 97 | 0 | yes | 0 |
| `conformance/src/util/version.mjs` | JS | 46 | 0 | yes | 0 |
| `conformance/src/util/waivers.mjs` | JS | 55 | 0 | yes | 0 |
| `conformance/src/util/workflows.mjs` | JS | 98 | 0 | yes | 0 |

### thin YAML (logic to the tool)

| File | Lang | Lines | OS refs | Tests | fix:/ci: |
|---|---|---|---|---|---|
| `.gitea/actions/app-gates/action.yml` | yaml | 76 (17 shell) | 0 | none | 1 |
| `.gitea/actions/build-env/action.yml` | yaml | 7 (1 shell) | 0 | none | 1 |
| `.gitea/actions/build-folder/action.yml` | yaml | 17 (1 shell) | 0 | none | 1 |
| `.gitea/actions/cache-restore/action.yml` | yaml | 29 (0 shell) | 2 | none | 1 |
| `.gitea/actions/cache-save/action.yml` | yaml | 23 (0 shell) | 1 | none | 1 |
| `.gitea/actions/conformance/action.yml` | yaml | 21 (5 shell) | 0 | none | 1 |
| `.gitea/actions/host-cleanup/action.yml` | yaml | 25 (5 shell) | 0 | none | 1 |
| `.gitea/actions/host-tools/action.yml` | yaml | 42 (35 shell) | 13 | none | 1 |
| `.gitea/actions/install-test-browser/action.yml` | yaml | 44 (15 shell) | 0 | none | 2 |
| `.gitea/actions/licences/action.yml` | yaml | 24 (3 shell) | 0 | none | 1 |
| `.gitea/actions/macos-verify/action.yml` | yaml | 24 (8 shell) | 4 | none | 1 |
| `.gitea/actions/nightly-mark/action.yml` | yaml | 14 (2 shell) | 0 | none | 1 |
| `.gitea/actions/node-caches/action.yml` | yaml | 39 (22 shell) | 6 | none | 1 |
| `.gitea/actions/plan/action.yml` | yaml | 37 (8 shell) | 0 | none | 1 |
| `.gitea/actions/publish-release/action.yml` | yaml | 44 (16 shell) | 0 | none | 1 |
| `.gitea/actions/sccache-stats/action.yml` | yaml | 13 (6 shell) | 0 | none | 3 |
| `.gitea/actions/setup-node/action.yml` | yaml | 14 (0 shell) | 0 | none | 1 |
| `.gitea/actions/setup-rust/action.yml` | yaml | 164 (117 shell) | 19 | none | 9 |
| `.gitea/actions/sidecar-self-test/action.yml` | yaml | 44 (16 shell) | 4 | none | 0 |
| `.gitea/actions/stable-workspace/action.yml` | yaml | 59 (46 shell) | 10 | none | 5 |
| `.gitea/actions/strip-signing-keys/action.yml` | yaml | 26 (10 shell) | 0 | none | 1 |
| `.gitea/actions/tauri-bundle/action.yml` | yaml | 29 (1 shell) | 2 | none | 1 |

### keep JS/TS

| File | Lang | Lines | OS refs | Tests | fix:/ci: |
|---|---|---|---|---|---|
| `apps/debrief/app/scripts/build-info.mjs` | JS | 55 | 1 | yes | 0 |
| `apps/debrief/app/scripts/check-dialog-commands.mjs` | JS | 152 | 8 | yes | 3 |
| `apps/debrief/app/scripts/dialog-commands-analysis.mjs` | JS | 481 | 0 | yes | 1 |
| `apps/debrief/app/scripts/i18n/build-locales.mjs` | JS | 58 | 0 | none | 0 |
| `apps/debrief/app/scripts/i18n/locales.mjs` | JS | 226 | 0 | yes | 0 |
| `apps/debrief/app/scripts/lint-effects.mjs` | JS | 91 | 0 | yes | 0 |
| `apps/debrief/app/scripts/lint-styles.mjs` | JS | 152 | 0 | yes | 1 |
| `apps/debrief/app/scripts/screenshots/capture.ts` | TS | 442 | 1 | none | 4 |
| `apps/debrief/app/scripts/screenshots/playwright.config.ts` | TS | 28 | 0 | none | 2 |
| `apps/debrief/app/scripts/visual-qa/layout-check.ts` | TS | 127 | 0 | none | 1 |
| `apps/debrief/app/scripts/visual-qa/playwright.config.ts` | TS | 30 | 0 | none | 1 |
| `apps/debrief/app/scripts/visual-qa/sweep.ts` | TS | 93 | 5 | none | 1 |
| `apps/debrief/app/scripts/whats-new.mjs` | JS | 81 | 0 | yes | 0 |
| `apps/pitwall/scripts/ci/ui-coverage-guard.mjs` | JS | 47 | 0 | yes | 1 |
| `apps/pitwall/scripts/tauri-before-build.mjs` | JS | 18 | 2 | none | 0 |

### keep JS for now (author time)

| File | Lang | Lines | OS refs | Tests | fix:/ci: |
|---|---|---|---|---|---|
| `apps/debrief/docs/diagrams/src/` (12 files) | JS | 1680 | 30 | 5 of 12 | 0 |
| `apps/dyno/docs/diagrams/src/` (9 files) | JS | 767 | 30 | 1 of 9 | 0 |
| `apps/pacenotes/docs/diagrams/src/` (1 files) | JS | 181 | 1 | none | 0 |
| `apps/pitcrew/docs/diagrams/src/` (10 files) | JS | 727 | 19 | 1 of 10 | 0 |
| `apps/pitwall/docs/diagrams/src/` (10 files) | JS | 998 | 16 | 1 of 10 | 0 |
| `apps/sighter/docs/diagrams/src/` (5 files) | JS | 497 | 2 | none | 0 |
| `apps/spotter/docs/diagrams/src/` (8 files) | JS | 444 | 12 | none | 0 |
| `diagrams/tools/` (6 files) | JS | 718 | 2 | none | 2 |
| `docs/images/src/` (9 files) | JS | 1035 | 10 | none | 1 |
| `docs/images/src/apps/` (7 files) | JS | 230 | 16 | none | 0 |
| `icons/tools/` (23 files) | JS | 4098 | 125 | 10 of 23 | 18 |

### keep shell (bootstrap)

| File | Lang | Lines | OS refs | Tests | fix:/ci: |
|---|---|---|---|---|---|
| `apps/pitcrew/scripts/linux-bootstrap.sh` | sh | 63 | 13 | none | 0 |
| `apps/pitcrew/scripts/macos-bootstrap.sh` | sh | 56 | 19 | none | 0 |
| `apps/pitwall/scripts/linux-bootstrap.sh` | sh | 61 | 14 | none | 0 |
| `ci-image/fetch-verified.sh` | sh | 35 | 0 | none | 0 |
| `ci-image/verify.sh` | sh | 94 | 0 | none | 1 |
| `scripts/setup-dev.sh` | sh | 64 | 11 | none | 0 |
| `tools/bootstrap.sh` | sh | 112 | 33 | yes | 0 |

### keep shell (draw.io wrapper)

| File | Lang | Lines | OS refs | Tests | fix:/ci: |
|---|---|---|---|---|---|
| `apps/debrief/docs/diagrams/src/generate.sh` | sh | 6 | 0 | yes | 0 |
| `apps/debrief/scripts/notices/generate.sh` | sh | 30 | 0 | yes | 0 |
| `apps/dyno/docs/diagrams/src/build.sh` | sh | 7 | 0 | yes | 0 |
| `apps/dyno/docs/diagrams/src/export.sh` | sh | 34 | 2 | none | 0 |
| `diagrams/scripts/export-diagrams.sh` | sh | 41 | 2 | none | 0 |
| `templates/repo/scripts/export-diagrams.sh` | sh | 41 | 2 | none | 0 |

### delete (one-off)

| File | Lang | Lines | OS refs | Tests | fix:/ci: |
|---|---|---|---|---|---|
| `apps/debrief/scripts/rename-to-debrief.mjs` | JS | 336 | 2 | none | 1 |
| `apps/pitcrew/scripts/windows-retire-legacy-runner.ps1` | ps1 | 142 | 14 | none | 2 |
| `docs/images/src/rename-codebase-tool.mjs` | JS | 13 | 0 | none | 0 |

### deleted by WP-C1..C6

| File | Lang | Lines | OS refs | Tests | fix:/ci: |
|---|---|---|---|---|---|
| `apps/debrief/scripts/ci/check-icons.mjs` | JS | 121 | 0 | yes | 2 |
| `apps/debrief/scripts/ci/check-index.mjs` | JS | 71 | 0 | yes | 1 |
| `apps/debrief/scripts/ci/macos-verify-bundle.sh` | sh | 85 | 23 | none | 5 |
| `apps/debrief/scripts/commit-lint.mjs` | JS | 67 | 0 | yes | 3 |
| `apps/debrief/scripts/export-diagrams.sh` | sh | 41 | 2 | none | 0 |
| `apps/dyno/app/scripts/check-index.mjs` | JS | 22 | 0 | yes | 0 |
| `apps/dyno/scripts/ci/macos-verify-bundle.sh` | sh | 104 | 22 | yes | 2 |
| `apps/dyno/scripts/commit-lint.mjs` | JS | 59 | 0 | yes | 0 |
| `apps/dyno/scripts/export-diagrams.sh` | sh | 38 | 4 | none | 0 |
| `apps/dyno/scripts/release/latest-json.mjs` | JS | 118 | 14 | yes | 0 |
| `apps/dyno/scripts/release/publish.mjs` | JS | 93 | 0 | yes | 1 |
| `apps/pacenotes/scripts/export-diagrams.sh` | sh | 41 | 2 | none | 0 |
| `apps/pacenotes/scripts/release/publish.mjs` | JS | 93 | 0 | yes | 0 |
| `apps/pitcrew/scripts/check-index.mjs` | JS | 22 | 0 | yes | 1 |
| `apps/pitcrew/scripts/ci/macos-verify-bundle.sh` | sh | 104 | 22 | yes | 2 |
| `apps/pitcrew/scripts/commit-lint.mjs` | JS | 59 | 0 | yes | 0 |
| `apps/pitcrew/scripts/export-diagrams.sh` | sh | 41 | 2 | none | 0 |
| `apps/pitcrew/scripts/release/latest-json.mjs` | JS | 118 | 14 | yes | 0 |
| `apps/pitcrew/scripts/release/publish.mjs` | JS | 93 | 0 | yes | 1 |
| `apps/pitcrew/scripts/sidecar.mjs` | JS | 24 | 3 | yes | 0 |
| `apps/pitwall/scripts/ci/check-icons.mjs` | JS | 121 | 0 | yes | 1 |
| `apps/pitwall/scripts/ci/check-index.mjs` | JS | 22 | 0 | yes | 1 |
| `apps/pitwall/scripts/ci/macos-cleanup.sh` | sh | 15 | 1 | none | 2 |
| `apps/pitwall/scripts/ci/macos-verify-bundle.sh` | sh | 104 | 22 | yes | 3 |
| `apps/pitwall/scripts/ci/tauri-bundle.sh` | sh | 61 | 12 | none | 5 |
| `apps/pitwall/scripts/commit-lint.mjs` | JS | 59 | 0 | yes | 0 |
| `apps/pitwall/scripts/export-diagrams.sh` | sh | 41 | 2 | none | 0 |
| `apps/pitwall/scripts/release/latest-json.mjs` | JS | 118 | 14 | yes | 0 |
| `apps/pitwall/scripts/release/publish.mjs` | JS | 93 | 0 | yes | 1 |
| `apps/pitwall/scripts/sidecar.mjs` | JS | 25 | 3 | yes | 0 |
| `apps/sighter/scripts/export-diagrams.sh` | sh | 41 | 2 | none | 0 |
| `apps/sighter/scripts/release/publish.mjs` | JS | 93 | 0 | yes | 0 |
| `apps/spotter/scripts/check-index.mjs` | JS | 22 | 0 | yes | 1 |
| `apps/spotter/scripts/ci/check-icons.mjs` | JS | 121 | 0 | yes | 1 |
| `apps/spotter/scripts/ci/macos-verify-bundle.sh` | sh | 104 | 22 | yes | 2 |
| `apps/spotter/scripts/commit-lint.mjs` | JS | 59 | 0 | yes | 0 |
| `apps/spotter/scripts/export-diagrams.sh` | sh | 41 | 2 | none | 0 |
| `apps/spotter/scripts/release/latest-json.mjs` | JS | 118 | 14 | yes | 0 |
| `apps/spotter/scripts/release/publish.mjs` | JS | 93 | 0 | yes | 1 |
