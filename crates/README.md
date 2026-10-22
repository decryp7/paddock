# Shared Rust crates

A Cargo workspace for Rust code the five apps share. It has no members yet; crates are extracted from the apps one at a
time (a crate moves here when a second app needs it, rule of three for new abstractions, two for copied code).

Rules for a crate here:

- Pure logic and thin adapters, no dependency on any app crate. Edition and licence come from `[workspace.package]`.
- Library errors are typed, no `unwrap`/`expect` on user data or I/O, `cargo clippy -- -D warnings` clean,
  >= 99% line coverage.
- Apps consume a crate as a git dependency pinned to a repo tag, for example
  `paddock-foo = { git = "https://dev.decryptology.net/decryp7/paddock", tag = "v0.2.0" }`.
- One semver tag for the whole repo: a release tag versions the UI package and every crate together.

Planned crates (named when extracted): `paddock-env`, `paddock-service`, `paddock-proc`, `paddock-window`, `paddock-notify`,
`chromium-driver` and `paddock-cli`.

To add a crate: create `crates/<name>`, add it to `members` in `Cargo.toml`, and add its job to
`.gitea/workflows/ci.yml` (the Rust jobs are stubbed there).
