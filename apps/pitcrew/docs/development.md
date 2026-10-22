# Development

## Layout

```
crates/pitcrew-core      pure logic: rules, state machines, config, protocol (no I/O)
crates/pitcrew-service   the background service: supervision, OS service integration, IPC server
app/src-tauri            the Tauri 2 shell: tray, window, commands, updater
app/src                  the Angular UI (standalone components, signals, SignalStores)
scripts/                 commit-lint, release manifest (latest.json), sidecar.mjs, check-index.mjs, macos-bootstrap.sh, macos-verify.sh
docs/adr/                architecture decisions (0001-0012)
docs/diagrams/           draw.io sources (.drawio) and exported SVGs; see design.md
.gitea/workflows/        CI
```

## Prerequisites

- Rust stable (1.88 or newer; `rustup` with `clippy` and `rustfmt`).
- Node 24 (`app/.nvmrc`) and npm.
- Tauri 2 prerequisites: on Windows the MSVC build tools and WebView2 (preinstalled on Windows 11); on macOS the Xcode
  command line tools; on Linux `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev`.
- Optional: `cargo-deny` (licences and advisories), `cargo-llvm-cov` (coverage), `lefthook` (git hooks:
  `npx lefthook install`).

Use a private build folder when several checkouts or agents build at once, e.g.
`CARGO_TARGET_DIR=~/.pitcrew-target CARGO_BUILD_JOBS=4`.

## Develop

```sh
cd app && npm ci
npm start                 # http://localhost:4200, mock backend (no Tauri, no service)
npx tauri dev             # the desktop app with hot reload
cargo run -p pitcrew-service -- --help
cargo run -p pitcrew-service -- check-config     # reads pitcrew.toml from the data folder
```

`npx tauri dev` and `tauri build` run `scripts/sidecar.mjs` first (`beforeDevCommand` / `beforeBuildCommand`): it builds
`pitcrew-service` (debug with `--dev`, the `dist` profile otherwise; honours `CARGO_TARGET_DIR`) and copies it to
`app/src-tauri/binaries/pitcrew-service-<target triple>` where Tauri's `externalBin` expects it. `tauri build` then
runs `scripts/check-index.mjs`, which fails when the built `index.html` has inline `<style>`/`<script>` (they break
component styles under Tauri's CSP; `inlineCritical` stays `false`, see [macos.md](macos.md)).

### Screenshots and browser tests

```sh
cd app
npx playwright test                                   # flows against `ng serve` with the mock backend (port 4310; PW_PORT overrides)
SCREENSHOTS=1 npx playwright test --project=screenshots   # rewrites ../docs/screenshots/<theme>/ (every screen, 1440x900 at 2x, dark and light)
```

Review the images before committing them. Nothing may be cut mid-card: the capture covers the page below the last block that
fits completely (`coverCutBlocks` in `e2e/screenshots.spec.ts`), and a shot of a long page can use a taller window or a clip.

### On a Mac

`bash scripts/macos-bootstrap.sh [--check]` checks the prerequisites and builds the `.app` and `.dmg` (nothing is
installed system-wide); `bash scripts/macos-verify.sh` checks an installed Pitcrew (read-only). See [macos.md](macos.md).

### On Linux

`bash scripts/linux-bootstrap.sh [--check]` checks the prerequisites and builds the `.deb` and the AppImage;
`bash scripts/linux-verify.sh` checks an installed Pitcrew (read-only). See [linux.md](linux.md).

The data folder is `%ProgramData%\Pitcrew` on Windows, `~/Library/Application Support/Pitcrew` on macOS and
`~/.local/share/Pitcrew` on Linux (`$XDG_DATA_HOME/Pitcrew`).

### The Windows service

From an elevated prompt (the app will do this through UAC):

```powershell
cargo build -p pitcrew-service
target\debug\pitcrew-service.exe install     # registers "Pitcrew" (automatic, delayed start, LocalSystem)
sc.exe start Pitcrew
sc.exe query Pitcrew
target\debug\pitcrew-service.exe uninstall
```

Register the binary from a stable location: the SCM keeps the path. `pitcrew-service run` supervises in the
foreground (for development); `status` asks a running service for its state.

## Quality gates

Everything in AGENTS.md "Testing and quality gates" must pass before committing:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny --workspace check
node --test scripts/commit-lint.test.mjs scripts/release/latest-json.test.mjs scripts/check-index.test.mjs
(cd app && npm audit --omit=dev --audit-level=high && npx ng lint && npx ng build && npx ng test --watch=false)
```

Platform code must type-check on the other OS too (no SDK needed for these crates):

```sh
rustup target add x86_64-apple-darwin x86_64-pc-windows-msvc
cargo clippy -p pitcrew-core -p pitcrew-service --all-targets --target x86_64-apple-darwin -- -D warnings
cargo clippy -p pitcrew-core -p pitcrew-service --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```

Coverage of the core: `cargo llvm-cov -p pitcrew-core --summary-only` (target ≥ 99% lines). UI coverage:
`npx ng test --watch=false --coverage`.

## App icon

The icon is `app/src-tauri/icons-src/pitcrew-icon.svg`; regenerate the set with
`cd app && npx tauri icon src-tauri/icons-src/pitcrew-icon.svg -o src-tauri/icons`, then delete the `android`, `ios`
and `Square*`/`StoreLogo` files (Pitcrew ships no mobile or Store packages).

## CI/CD

`.gitea/workflows/build.yml`: jobs `rust-lint` (format + clippy, Linux), `rust-test` (Linux and the Windows host runner),
`macos` (clippy + tests on the Mac host runner, kept lean), `licences` (cargo-deny, commit messages, the three script
tests), `frontend` (lint, build, unit tests with coverage); `bundle` (Windows NSIS and macOS `.dmg`) on `v*` tags, the
nightly run and manual runs; the `release` job publishes the installers, release notes (git-cliff) and `latest.json`.
See [releasing.md](releasing.md).

## Architecture documents and diagrams

- Prose: [architecture.md](architecture.md); every diagram on one page: [design.md](design.md); decisions:
  [adr/](adr/0001-architecture.md). Update them with the code: a change to a module, a protocol message, a state or a
  platform changes the matching diagram and paragraph in the same commit.
- Diagrams are draw.io files in `docs/diagrams/*.drawio` (uncompressed XML) following
  [diagrams/STYLE.md](diagrams/STYLE.md). After editing one, export all of them:

  ```sh
  bash scripts/export-diagrams.sh   # needs draw.io desktop (DRAWIO overrides the executable) and node
  ```

  It writes `docs/diagrams/svg/<name>.svg` (transparent, follows light or dark; `design.md` and the README embed these)
  and `docs/diagrams/png/<name>.png` (2x, not committed). `pitcrew-overview.svg` is a hand-written SVG with the same tokens
  and is edited directly. The export script is copied unchanged from Paddock's diagram kit (`diagrams/` in the paddock
  repo), which also has the shape library and templates. Open the exported SVGs and check them for overlaps and clipped
  text, in light and dark, before committing.
- Screenshots: `SCREENSHOTS=1 npx playwright test --project=screenshots` (see above).

## Where to add things

- **A protocol message:** `pitcrew_core::protocol` (`Request`/`Response`), the supervisor's `request` handler, the app's
  DTOs (`app/src-tauri/src/dto.rs`) and `app/src/app/core/api.types.ts` in the same change; bump `PROTOCOL_VERSION` if the
  meaning of an existing message changes. Queries must not mutate (`Request::mutates`).
- **A rule** (stall, restart, update, cleanup, status): a pure function or state machine in `pitcrew-core` with a unit
  test first; the service only feeds it inputs and carries out its actions.
- **A runner kind:** a `RunnerKind` variant with its release description in `distribution.rs` (and a discovery parser if
  needed), then the service adapter; update the diagrams that name the kinds.
- **An OS:** a `platform` adapter (`dispatch`, `install`, `uninstall`), launcher behaviour, `service_paths` and
  `service_spec`; add the target to the type-check commands above and a guide like [linux.md](linux.md).
- **A screen:** a standalone component, a SignalStore if it has state, and the backend method on both the Tauri backend
  and the mock; component styles use the tokens ([ADR 0010](adr/0010-design-system.md)).

## Tests

| Layer | Where | Runs |
| --- | --- | --- |
| Core unit tests (rules, state machines, parsing, migration) | `crates/pitcrew-core/src` | `cargo test -p pitcrew-core`; coverage target 99 % lines |
| Service integration tests against a fake runner and a controllable clock | `crates/pitcrew-service/tests`, `examples/fake_runner.rs` | `cargo test -p pitcrew-service` (Windows, Linux, macOS CI) |
| Transport tests (the real pipe or socket) | `crates/pitcrew-service/tests/transport.rs` | same |
| App commands against a fake `ServiceClient` | `app/src-tauri/src` | `cargo test --workspace` |
| UI unit and component tests | `app/src/**/*.spec.ts` | `npx ng test --watch=false` |
| Browser flows with the mock backend | `app/e2e` | `npx playwright test` |
| Things that need a real desktop | [testing-manual.md](testing-manual.md) | by hand before a release |
