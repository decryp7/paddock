# Shared Rust crates

A Cargo workspace for Rust code the apps share. Members: `paddock-jobs` (ADR 0008) and the app shell crates of ADR 0012
(`paddock-commands`, `-dialogs`, `-download`, `-health`, `-menu`, `-settings`, `-startup`, `-updater`, `-window`; scaffolds
until their work packages in [docs/plans/workbench.md](../docs/plans/workbench.md) land). Crates are extracted from the apps one
at a time (a crate moves here when a second app needs it, rule of three for new abstractions, two for copied code).

Rules for a crate here:

- Pure logic and thin adapters, no dependency on any app crate. Edition and licence come from `[workspace.package]`.
- Library errors are typed, no `unwrap`/`expect` on user data or I/O, `cargo clippy -- -D warnings` clean,
  >= 99% line coverage.
- A pure core with no Tauri dependency, and a `tauri` feature for the adapter (`src/tauri.rs`, `pub mod tauri` behind
  `#[cfg(feature = "tauri")]`). Tauri comes from `[workspace.dependencies]` without default features (the app enables
  the runtime); CI builds every crate with `--all-features` on Linux, Windows and macOS.
- A crate that uses another one writes `paddock-jobs = { workspace = true }` (the workspace dependency carries path,
  version and registry, so the published crate points at the registry).
- `publish = ["decryp7"]` in every crate. A new crate starts with `[package.metadata.paddock] scaffold = true`, which keeps
  the publish job from uploading it; its work package removes the marker. A new member also needs its step in the `crate`
  job of `.gitea/workflows/ci.yml` (`scripts/publish.test.mjs` fails without it).
- Published to the Gitea cargo registry by the tag workflow only (`.gitea/workflows/publish.yml`, rule 9). One repo tag
  `vX.Y.Z` releases everything; each crate bumps its own `version` only when it changes, and a version the registry
  already has is skipped. `node scripts/publish.mjs` is the local dry run (lists every crate and npm package with what a
  tag would publish).

Apps consume the crates from the registry. In the app's `.cargo/config.toml`:

```toml
[registries.decryp7]
index = "sparse+https://dev.decryptology.net/api/packages/decryp7/cargo/"
```

and in its `Cargo.toml` (pre-1.0: `0.N` accepts patches of that minor only), with `Cargo.lock` committed:

```toml
paddock-jobs = { version = "0.1", registry = "decryp7", features = ["tauri"] }
```

The app's `deny.toml` allows the registry: `allow-registry = ["https://github.com/rust-lang/crates.io-index",
"sparse+https://dev.decryptology.net/api/packages/decryp7/cargo/"]`.

Crates, built and planned, with their designs in [`docs/adr/`](../docs/adr/README.md):

| Crate(s) | Purpose | Three users today? |
|---|---|---|
| `paddock-jobs` | long operations: job/phase/progress model, throttled reporter, cancel token, hash-while-writing, parallel walk/map/pipeline, job pool (ADR 0008) | yes (all five apps); built |
| `paddock-commands`, `-health`, `-dialogs`, `-startup`, `-download`, `-settings`, `-updater`, `-window`, `-menu`, `-shell` | the app shell (ADR 0012) | yes; scaffolds, plan S1-S17 |
| `paddock-pitwall` | Pitwall client: discovery, pairing, keychain keys, typed requests, optional-AI state (ADR 0004) | yes (Debrief, Pacenotes, Sighter; Spotter, Dyno later); built first |
| `paddock-ipc`, `-guard`, `-view`, `-plugin-api`, `-plugin-host`, `-plugin-package`, `-plugin-test` | the out-of-process plugin platform (ADRs 0001 to 0003) | yes (Debrief, Pacenotes, Dyno tier 3) |
| `paddock-mcp` | MCP protocol, tool schemas, limits, paging, audit log, local transports (ADR 0007) | three (Pacenotes, Sighter, Debrief); extracted with Debrief's server, after Debrief 1.0.2 |
| `paddock-testplan` | `.sighterplan` / `.sighterresult` (ADR 0005) | two (Pacenotes, Sighter); shared schema, so extracted on purpose |
| `paddock-env` | per-OS data/config dirs, portable mode | yes |
| `paddock-service` | background service install/control per OS | yes |
| `paddock-proc` | child processes with timeouts, caps, kill-tree | yes |
| `paddock-window` | native window feel, tray, single instance | yes |
| `paddock-notify` | OS notifications | borderline (two to three) |
| `chromium-driver` | CDP driver (today in Debrief) | two to three (Debrief, Spotter, Sighter) |
| `paddock-cli` | shared CLI conventions (`--json`, exit codes, timeouts) | yes |

To add a crate: create `crates/<name>` (as one of the scaffolds), add it to `members` and `[workspace.dependencies]` in
`Cargo.toml`, and add its step to the `crate` job in `.gitea/workflows/ci.yml`.
