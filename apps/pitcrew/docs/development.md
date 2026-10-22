# Development

## Layout

```
crates/pitcrew-core      pure logic: rules, state machines, config, protocol (no I/O)
crates/pitcrew-service   the background service: supervision, OS service integration, IPC server
app/src-tauri            the Tauri 2 shell: tray, window, commands, updater
app/src                  the Angular UI (standalone components, signals, SignalStores)
scripts/                 commit-lint, release manifest (latest.json)
docs/adr/                architecture decisions
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

The data folder is `%ProgramData%\Pitcrew` on Windows and `~/Library/Application Support/Pitcrew` on macOS.

### The Windows service

From an elevated prompt (the app will do this through UAC):

```powershell
cargo build -p pitcrew-service
target\debug\pitcrew-service.exe install     # registers "Pitcrew" (automatic, delayed start, LocalSystem)
sc.exe start Pitcrew
sc.exe query Pitcrew
target\debug\pitcrew-service.exe uninstall
```

Register the binary from a stable location: the SCM keeps the path. The service currently only answers the SCM
(start/stop); supervision is not built yet.

## Quality gates

Everything in AGENTS.md "Testing and quality gates" must pass before committing:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny --workspace check
node --test scripts/commit-lint.test.mjs scripts/release/latest-json.test.mjs
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

`.gitea/workflows/build.yml`: format + clippy (Linux, plus a macOS type-check), tests (Linux and the Windows host
runner), licences/advisories/commit messages/script tests, frontend lint/build/tests; installers on `v*` tags, the
nightly run and manual runs; the release job publishes the installers, release notes (git-cliff) and `latest.json`.
See [releasing.md](releasing.md).
