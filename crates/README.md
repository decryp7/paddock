# Shared Rust crates

A Cargo workspace for Rust code the five apps share. Members so far: `paddock-jobs` (ADR 0007). Crates are extracted from the apps one at a
time (a crate moves here when a second app needs it, rule of three for new abstractions, two for copied code).

Rules for a crate here:

- Pure logic and thin adapters, no dependency on any app crate. Edition and licence come from `[workspace.package]`.
- Library errors are typed, no `unwrap`/`expect` on user data or I/O, `cargo clippy -- -D warnings` clean,
  >= 99% line coverage.
- Apps consume a crate as a git dependency pinned to a repo tag, for example
  `paddock-foo = { git = "https://dev.decryptology.net/decryp7/paddock", tag = "v0.2.0" }`.
- One semver tag for the whole repo: a release tag versions the UI package and every crate together.

Planned crates, with their designs in [`docs/adr/`](../docs/adr/README.md) (ADRs 0001 to 0006; nothing is built yet):

| Crate(s) | Purpose | Three users today? |
|---|---|---|
| `paddock-jobs` | long operations: job/phase/progress model, throttled reporter, cancel token, hash-while-writing, parallel walk/map/pipeline, job pool (ADR 0007) | yes (all five apps); built |
| `paddock-pitwall` | Pitwall client: discovery, pairing, keychain keys, typed requests, optional-AI state (ADR 0004) | yes (Debrief, Pacenotes, Sighter; Spotter, Dyno later); built first |
| `paddock-ipc`, `-guard`, `-view`, `-plugin-api`, `-plugin-host`, `-plugin-package`, `-plugin-test` | the out-of-process plugin platform (ADRs 0001 to 0003) | yes (Debrief, Pacenotes, Dyno tier 3) |
| `paddock-testplan` | `.sighterplan` / `.sighterresult` (ADR 0005) | two (Pacenotes, Sighter); shared schema, so extracted on purpose |
| `paddock-env` | per-OS data/config dirs, portable mode | yes |
| `paddock-service` | background service install/control per OS | yes |
| `paddock-proc` | child processes with timeouts, caps, kill-tree | yes |
| `paddock-window` | native window feel, tray, single instance | yes |
| `paddock-notify` | OS notifications | borderline (two to three) |
| `chromium-driver` | CDP driver (today in Debrief) | two to three (Debrief, Spotter, Sighter) |
| `paddock-cli` | shared CLI conventions (`--json`, exit codes, timeouts) | yes |

To add a crate: create `crates/<name>`, add it to `members` in `Cargo.toml`, and add its job to
`.gitea/workflows/ci.yml` (the Rust jobs are stubbed there).
