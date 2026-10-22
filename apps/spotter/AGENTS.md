# Spotter — engineering rules for coding agents and contributors

Spotter watches for new releases anywhere (folders and shares, HTTP listings, JSON APIs, forges, feeds, web pages
including JavaScript-rendered ones) and tells you the moment one appears: a Rust core (`crates/spotter-core`), source
adapters (`crates/spotter-sources`), the engine (`crates/spotter-engine`), the renderer (`crates/spotter-render`), a
Tauri 2 shell (`app/src-tauri`) and an Angular UI (`app/src`). It is part of the Paddock suite and is local only: no
cloud, no telemetry. See README.md, `docs/adr/` and `docs/suite-lessons.md` (follow every lesson in it).

## Design principles

- **Single responsibility**, **open/closed**, **Liskov**, **interface segregation**, **dependency inversion**: as in
  Pitcrew. New source kinds are new modules behind the `ReleaseSource` port and a catalog entry; new presets and
  version schemes are declarative TOML **packs**, not code. There is no plugin system (ADR 0001).
- **Pure core, thin edges.** Version parsing and ordering, filters, soak time, diffing, scheduling, digest and quiet
  hours, notification policy and config migration are pure functions with time passed in as values. Network, files,
  SQLite, processes and the OS stay thin.
- **Ports and adapters.** The core never depends on an adapter. The Tauri commands depend on engine traits; the
  renderer is used only through the small `PageRenderer` trait (its CDP driver will move to the shared paddock repo).
- **Angular never calls Tauri `invoke` directly**: it goes through the `BACKEND` token (real Tauri backend or mock),
  state lives in SignalStores. **Never await IPC before first paint.** Every Tauri command is `async`.
- **Parse, don't validate; make illegal states unrepresentable; command-query separation.**
- Also: KISS, YAGNI, DRY with the rule of three, composition over inheritance, fail fast with a message that tells the
  user what to fix, leave touched code better than you found it.

## Security and robustness (untrusted pages and feeds)

- Never trust the webview; validate every IPC argument in Rust. Open only paths the user chose; canonicalize them.
- Everything fetched is untrusted: HTTPS preferred, redirects capped, bodies capped (`MAX_BODY`), timeouts on every
  request, and the per-site minimum interval and `ETag`/`Last-Modified` always honoured. No requests to the loopback or
  link-local ranges unless the watch is explicitly local. Never render fetched text with `[innerHTML]`; fetched HTML is
  parsed, never executed, except inside the renderer.
- **Renderer rules (ADR 0002).** Pages run in a separate supervised Chromium process with a throwaway per-watch
  profile, hard caps on time and memory, no extensions, no stealth tricks. Readiness = load finished + element present
  + DOM quiet (not network idle). A challenge (login, captcha) is handed to the user as "Needs you", never solved.
  Tokens and cookies never leave the credential store/profile and are never logged.
- User regexes only through `user_regex::compile` (length and compiled-size caps). Parsers never panic and enforce
  limits. No `unwrap()`/`expect()` on user data or I/O outside tests. Library errors are typed.
- Tauri capabilities stay least-privilege; CSP free of `unsafe-eval` and remote origins. External tools and child
  processes run with timeouts and no console window on Windows. Logs contain no fetched content or secrets.
- Persistent state (config, SQLite) is versioned, written atomically and tolerates missing/corrupt files by falling
  back to defaults. Every robustness behaviour has a test that triggers the failure.

## Languages

All shipped code is Rust plus the Angular UI. `unsafe` is denied workspace-wide. No Python/C#/C++ in the product.

## Where logic lives

The Rust engine is the single source of truth (versions, filters, diffing, scheduling, notifications, previews).
Angular is presentation only; if the UI needs a computation, add an engine operation and a command. The mock backend
exists for browser development and tests only, and tests never use production names, paths or data directories.

## Working in parallel (agents)

Each task owns an explicit set of files. Shared integration points (`lib.rs` files, the workspace `Cargo.toml`,
`api.types.ts` / `dto.rs`) are changed by the integrating agent only. Do not run workspace-wide `cargo fmt`/`clippy
--fix` while others edit; format your own files. Use `CARGO_BUILD_JOBS=2` and a private `CARGO_TARGET_DIR`. Playwright
runs with `--workers=1` on a non-default port (4370). Never leave dev processes running.

## Contracts

The UI-backend contract is `app/src/app/core/api.types.ts` (camelCase JSON), mirrored by `app/src-tauri/src/dto.rs`;
change both in one commit. Config and state are versioned (`SCHEMA`), with migration and a test.

## Testing and quality gates

Every change ships with tests; every bug fix starts with a failing regression test. Targets: `spotter-core` >= 99%
lines, Angular >= 80% overall and near 100% for stores/services/utilities. Before committing:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny --workspace check
(cd app && npm audit --omit=dev --audit-level=high && npx ng lint && npx ng build && npx ng test --watch=false)
```

## Dependencies and releases

Permissive licences only (`deny.toml`). Commit `Cargo.lock` and `package-lock.json`. Conventional Commits (no AI
attribution lines); releases are `v*` tags. Significant decisions are ADRs in `docs/adr/`.

## Style

Rust: `rustfmt.toml` (max_width 140), doc comments on public items. UI: standalone components, signals, OnPush, `sp`
selector prefix, `@decryp7/paddock-ui` (`pd-` prefix) for the shared kit, accent #0E8577, CDK for behaviour, icons
registered as literals (no icon fonts, no runtime URLs). Never run `prettier --write`.
