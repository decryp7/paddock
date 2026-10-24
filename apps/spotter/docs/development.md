# Development

## Layout

Machine setup shared by every Paddock app (tools, build cache, worktrees, test runs): [paddock/docs/dev-setup.md](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/dev-setup.md). This page adds only what is specific to this app.

```
crates/spotter-core      pure logic: versions, filters, soak time, diffing, scheduling, notification policy, config
crates/spotter-sources   source adapters behind the ReleaseSource port (folders, HTTP, JSON, forges, feeds, pages)
crates/spotter-engine    the engine: watches, scheduler, SQLite store, notifications
crates/spotter-render    the supervised Chromium renderer behind the PageRenderer trait (ADR 0002)
app/src-tauri            the Tauri 2 shell: commands, tray, window, updater
app/src                  the Angular UI (standalone components, signals, SignalStores, BACKEND token)
scripts/                 commit-lint.mjs, check-index.mjs, release/latest-json.mjs (each with a .test.mjs)
docs/adr/                architecture decisions
.gitea/workflows/        CI
```

## Prerequisites

- Rust stable (1.88 or newer) with `clippy` and `rustfmt`.
- Node (version in `app/.nvmrc`) and npm.
- Tauri 2 prerequisites: Windows MSVC build tools and WebView2; macOS Xcode command line tools; Linux
  `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev patchelf`.
- Optional: `cargo-deny`, `cargo-llvm-cov`, `lefthook` (`npx lefthook install`), Chromium for renderer tests.
- The `@decryp7` npm scope comes from the Gitea registry set in `app/.npmrc`.

## Develop

```sh
cd app && npm ci
npm start              # http://localhost:4200 with the mock backend (no Tauri)
npx tauri dev          # the desktop app with hot reload
```

## Load rules (shared machines)

Several agents and CI runners share the Macs. Be a good neighbour:

- `CARGO_BUILD_JOBS=2` and a private `CARGO_TARGET_DIR` (for example `~/.spotter-target`; CI uses `.spotter-ci`).
- Playwright runs with `--workers=1` on port **4370** (`PW_PORT=4370`), never the default 4200/4310.
- Run targeted tests while working; run the full suites once per batch of merges.
- Never leave dev servers, Chromium or `tauri dev` processes running; check `sysctl -n vm.loadavg` before measuring
  speed or memory and report the load next to the number.
- Tests use temp directories and derived names, never the production data folder, keychain entry or ports.

## Quality gates

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny --workspace check
node --test scripts/commit-lint.test.mjs scripts/release/latest-json.test.mjs scripts/check-index.test.mjs
(cd app && npm audit --omit=dev --audit-level=high && npm run lint && npx ng build && npx ng test --watch=false)
(cd app && PW_PORT=4370 npx playwright install --with-deps chromium webkit && PW_PORT=4370 npx playwright test --project=chromium --project=webkit --workers=1)
```

`npx playwright install --with-deps` needs root on Linux; on macOS and Windows omit `--with-deps`.

## Real-browser tests

The renderer tests (`crates/spotter-render/tests/real_browser.rs`) launch a browser, but never an installed Chrome, Edge
or Chromium: killing a launched system Chrome left a code-sign clone of the whole app behind on macOS (hundreds of GB).
They use only the pinned Chrome for Testing of the renderer's own manifest (ADR 0004), installed once with
`node scripts/install-test-browser.mjs` into `~/.cache/spotter-test-browser` (or `$SPOTTER_TEST_BROWSER_DIR`), or an
explicit executable in `SPOTTER_TEST_BROWSER`. With neither, those tests skip with a message. Every browser a test
starts is asked to quit (`Browser.close`) and given a few seconds before any kill.

## CI

`.gitea/workflows/build.yml` runs on pushes to `main` and `wip/**`, `v*` tags, pull requests and manual runs:
`rust-lint` (format and clippy), `rust-test` (Linux and Windows), `macos` (clippy and tests on the Mac host runner),
`licences` (commit messages, script tests, cargo-deny), `frontend` (audit, lint, build, unit tests with coverage) and
`e2e` (Playwright, Chromium and WebKit against the mock backend). `bundle` (NSIS, `.dmg`, `.deb`/AppImage) runs only
for tags, the nightly schedule and manual runs; `release` publishes for tags. See [releasing.md](releasing.md).

The workflow expects `app/.nvmrc`, `app/package-lock.json`, an `npm run lint` script, a Playwright config that honours
`PW_PORT` and defines `chromium` and `webkit` projects, and the three script tests above.

## CI image and compile cache

Linux CI jobs run in the [garage](https://dev.decryptology.net/decryp7/garage) image (Rust, Tauri libraries, Node,
Playwright browsers, sccache), so the jobs no longer install system packages. The Rust test and bundle jobs compile with
`sccache`; its cache directory `/sccache` is saved and restored with `actions/cache`, keyed on `Cargo.lock`. The
step `sccache statistics` in each job shows the hit rate.

Optional upgrade (faster, needs a change to each runner's `config.yaml`): keep the cache on the runner host so no
restore or save is needed, then drop the two `actions/cache` steps named after sccache:

```yaml
container:
  options: "-v /var/cache/sccache:/sccache"
  valid_volumes:
    - /var/cache/sccache
```

See the [garage README](https://dev.decryptology.net/decryp7/garage) for the measurements.
