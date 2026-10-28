# Development

## Layout

Machine setup shared by every Paddock app (tools, build cache, worktrees, test runs): [paddock/docs/dev-setup.md](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/dev-setup.md). This page adds only what is specific to this app.

```
crates/dyno-core         pure core: model, grouping, diagnosis, health, alarms, protocol, ports
crates/dyno-platform     OS adapters (windows/, macos/, linux/) and test fakes
crates/dyno-ipc          the one local IPC (frames, token handshake, host/child)
crates/dyno-agent        the agent: sampler, server, handlers, store, rollup, service, cli
crates/dyno-helper       the privileged helper: not shipped in 1.0, kept for 1.1 (ADR 0022)
crates/dyno-plugin-api   what a plugin depends on; crates/dyno-plugin-host guards plugins
plugins/                 example plugin
app/src-tauri            Tauri shell (commands, link to the agent, tray, windows, native feel, updater)
app/src                  Angular UI (core/, shared/, state/, shell/, features/)
app/e2e                  Playwright flows and the screenshot spec
diagrams/                draw.io sources and SVGs
docs/                    this documentation, ADRs, screenshots
scripts/                 CI guards, release helpers, Linux container tooling
```

## Prerequisites

Rust stable (1.88+, `rust-version` in the workspace `Cargo.toml`), Node 24 (`app/.nvmrc`), the
[Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) (WebView2 on Windows, Xcode command line tools on macOS,
`libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev patchelf` on Linux). Optional: `cargo-deny`,
`git-cliff`, `lefthook` (`npx lefthook install` for the pre-commit hooks), Docker or OrbStack for the Linux container.

## Develop

```sh
cd app && npm ci
npm start                 # the UI in a browser at http://localhost:4200 against the mock backend
npx tauri dev             # the desktop app; builds dyno-agent as the sidecar first (src-tauri/scripts/sidecar.mjs)
cargo run -p dyno-agent -- run     # an agent in the foreground (the app finds it through agent.json)
cargo run -p dyno-agent -- once    # one measurement
```

Use a private `CARGO_TARGET_DIR` for heavy builds and `DYNO_DATA_DIR=/tmp/dyno-dev` to keep a development agent out of your
real data folder. Do not run `cargo fmt --all` or `cargo clippy --fix` across the workspace while others are editing; format
your own files with `rustfmt --edition 2021 <files>`. Never run `prettier --write`: the TypeScript is not prettier-formatted.

### Browser tests and screenshots

```sh
cd app
npx playwright install chromium webkit
npx playwright test --workers=1                      # flows against `ng serve` on port 4352 with the mock backend
npx playwright test --workers=1 --project=chromium   # one browser
npm run screenshots                                  # docs/screenshots/<screen>-<dark|light>.png (SCREENSHOTS=1, project "screenshots")
```

The screenshot spec freezes the clock and fixes locale and timezone so images are deterministic.

### On a Mac

`docs/macos.md`. Real macOS platform tests spawn their own dummy processes and act only on those.

### On Linux

`scripts/linux-verify.sh` builds and tests `dyno-platform` in a native Linux container (OrbStack or Docker) once as root and
once as uid 1000, then runs the portable parsers natively. `scripts/linux-container.sh <command>` runs any cargo command there.
See [linux.md](linux.md).

### Windows

Windows-only code must type-check from other hosts: `cargo clippy -p dyno-platform -p dyno-agent --all-targets --target
x86_64-pc-windows-msvc` (and `x86_64-apple-darwin` for macOS code). Real behaviour is verified on the Windows CI runner.

## Quality gates

Before committing:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny --workspace check
node --test scripts/check-capabilities.test.mjs
node scripts/check-capabilities.mjs
(cd app && npm audit --omit=dev --audit-level=high && npx ng lint && npx ng build && node ../../../conformance/ci/check-index.mjs dist/dyno-app/browser/index.html && npx ng test --watch=false)
```

Coverage: `dyno-core` at least 99 % lines (`cargo llvm-cov -p dyno-core --summary-only`, with a private `CARGO_TARGET_DIR`);
Angular at least 80 % overall and near 100 % for stores, services and pure utilities (`npx ng test --watch=false --coverage`).
Tests never leave processes behind, never touch real user folders and are timezone-independent (CI runs the unit tests once
with another `TZ`).

## App icon

The icon set is not drawn in this repo. It comes from Paddock's `icons/dyno` (the family's isometric icon) and is copied in by
`node icons/tools/install.mjs` (run in the Paddock checkout): `app/src-tauri/icons` (the Tauri set and the tray PNGs the tray code embeds),
the in-app copies under `app/public`, and `docs/images/dyno-icon.svg` for the README header. `install.mjs --check` (and the `check-icons`
script) fail when a copy differs from the build. Never run `tauri icon` on a local source and never keep a local icon source: it would
revert the family icon. To change the icon, change it in Paddock (`icons/tools/iso-family.mjs`), rebuild there and install again. The in-app logo is `app/public/dyno-logo.svg` (the Windows master).

## CI/CD

Dyno is on Paddock's shared CI kit (WP-C4): its gates are legs of `.gitea/workflows/ci.yml` (`apps/dyno/paddock.ci.json`, run by
`.gitea/actions/app-gates`), its installers and release are `.gitea/workflows/dyno-release.yml` through `app-release.yml`.

| Leg | Runner | What it checks |
| --- | --- | --- |
| `app dyno (ubuntu-latest)` | Linux container | the capability/CSP guard and its tests; `cargo fmt --check`, clippy `-D warnings`, nextest (the Linux adapters and parsers against the container's real `/proc` included), doc tests, `cargo deny`; then `npm audit`, lint, build, the `index.html` check, unit tests with coverage and once more under another time zone |
| `app dyno (windows-latest)`, `(macos-latest)` | host runners | clippy (also on Windows: `windowsClippy`), nextest, doc tests for the OS adapters; tests act only on dummies they spawn; a kept build folder, host clean-up of this job's test programs |
| `e2e-1`, `e2e-2` | Linux container | Playwright against the mock, Chromium, two shards, one worker each |
| `e2e-webkit` | Linux container | Playwright, WebKit: main, tags, the nightly and a manual run with `webkit` |
| `bundle` (`dyno-release.yml`) | Windows, macOS, Linux | on a push to main, `dyno-v*` tags, nightly and manual runs: `tauri build` (NSIS, `.dmg`, `.deb` + AppImage), the run-path check (`scripts/check-bundle.sh`; on macOS the agent also measures once from the built app), the seal check and the start of every sidecar (`macos-verify`), artifacts |
| `release` | Linux container | on `dyno-v*` tags: release notes (git-cliff), `latest.json`, the Gitea release |

Host runners keep a persistent build folder outside the workspace (the job slot's kept folder `target-dyno-app-s<n>` in the
CI home, Paddock ADR 0025), which the shared
`prune-build-folder` action (`tools/prune-build-folders.sh`, 20 GB) caps at the end of each host job, so nothing piles up. CI could not be run when the workflow was written; see roadmap G15.

## Architecture documents and diagrams

The diagrams are generated, not hand-drawn: `docs/diagrams/src/*.mjs` build `diagrams/*.drawio` from the Paddock diagram kit
(`diagrams/tools` at the monorepo root) and `docs/diagrams/src/build.sh [name…]` regenerates everything and exports it
to `diagrams/svg/` (theme-aware SVG) with the draw.io desktop CLI; `dyno-overview.svg` is a hand-written SVG from
`overview.mjs`. Change the script, not the picture. Follow [`diagrams/STYLE.md`](../diagrams/STYLE.md), look at the exported
picture in both themes before committing and update the date in `common.mjs` when a diagram changes.

## Where to add things

| I want to … | Start in |
| --- | --- |
| Add a measurement | a `Reading` field in `dyno_core::metrics`, the adapter's probe per OS, the UI tile |
| Support a new OS or sensor | a new adapter behind `dyno_core::ports` in `dyno-platform` |
| Add a request | `dyno_core::protocol` (`Request`, `Reply`, `PROTOCOL_VERSION`), a handler in `dyno-agent`, the DTO in `api.types.ts`, the mock, the UI store |
| Add a setting | `dyno_core::settings` (optional with a default; bump `SCHEMA_VERSION` for a breaking change and add a migration with a test) |
| Add a screen | `app/src/app/features/<name>`, a route, `shell/nav.ts`, a store, mock data, a Playwright flow, screenshots |
| Write a plugin | [plugins.md](plugins.md) |
| Record a decision | a new ADR in `docs/adr/` and a line in its README |

## Tests

Rust: unit tests beside the code, adapter tests against temporary trees and scripted tools (Linux `HostPaths` and `Tools`),
real-OS tests that spawn and kill their own dummies, and end-to-end agent tests against a private data folder. UI: Vitest with
TestBed for stores, services, components and pure utilities (including colour contrast and colour-vision specs for the
palette); Playwright flows with the mock backend in Chromium and WebKit. Every bug fix starts with a failing regression test.

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
