---
name: new-crate
description: Use when adding a new shared Rust crate to Paddock's crates/ workspace (for example "add paddock-xyz", "extract this into a shared crate"). Covers Cargo.toml publish settings, the workspace hack, tests layout, coverage, generated TypeScript, README entry and the publish test. Paddock only.
---

# New shared crate (Paddock)

Rules: share first (rule 42, check `crates/README.md` and `ui/README.md` for an existing crate), shared behaviour is tested once
here (rule 43), no `unwrap`/`expect` on user data or I/O, typed library errors. Background: `crates/README.md` ("Rules for a crate here").
Work in your own worktree with its own `CARGO_TARGET_DIR`; heavy steps through the build semaphore.

## Steps
1. **Create `crates/paddock-<name>/`** with `Cargo.toml`:
   - `edition.workspace = true`, `license.workspace = true`, `repository.workspace = true` (workspace metadata, no own copies);
   - `publish = ["decryp7"]` (anything else makes `scripts/publish.mjs` and conformance C37 fail);
   - a `description`; `version = "0.1.0"`;
   - `[package.metadata.paddock] scaffold = true` while the crate is unfinished (keeps the publish job from uploading it);
     remove it in the work package that fills the crate;
   - other shared crates as `paddock-x = { workspace = true }`; Tauri only behind an optional `tauri` feature
     (`tauri = { workspace = true, optional = true }`), the core stays pure.
2. **Add the member** to `members` in `crates/Cargo.toml` (alphabetical).
3. **Workspace hack:** `cd crates && cargo hakari manage-deps --yes && cargo hakari generate`. This adds the
   `paddock-workspace-hack` dependency and refreshes it; commit the changes to the hack crate and the lockfile. CI runs
   `cargo hakari generate --diff`, `manage-deps --dry-run` and `verify`. Never let hakari unify Tauri into pure crates (see
   `crates/.config/hakari.toml`).
4. **No `ci.yml` step.** The `crates` job's workspace-wide steps (clippy, nextest, coverage, doc tests) already cover every
   member; do not add `-p`/`--exclude` steps. Only a crate whose behaviour changes with an optional feature off joins the
   "Tests with the optional features off" step. `scripts/publish.test.mjs` and conformance C37 fail when a member is left out.
5. **Tests in a single `tests/it` binary:** `tests/it/main.rs` with `mod x;` per topic (one test binary links once; many
   binaries made linking dominate). Unit tests stay beside the code. Test children, ports and temp dirs per rule 7; no real
   user data. A `harness = false` binary must answer `--list --format terse` or nextest cannot list the crate.
6. **Coverage >= 99% lines** for the package: `cargo llvm-cov nextest --profile ci -p paddock-<name> --all-features --summary-only`.
   Native glue that tests cannot reach goes in one small `native.rs` per adapter, kept free of logic, and is added to the
   `--ignore-filename-regex` of the `crates` job's coverage step (and listed in `crates/README.md`).
7. **Generated TypeScript, if the crate has wire types or command tables:** generate with ts-rs / `paddock_commands::ts` from a
   unit test (`PADDOCK_WRITE_TS=1 cargo test -p paddock-<name> --lib generated`) into
   `ui/projects/paddock-ui/.../generated/`, and guard it with `testing::sync_generated` so a stale file fails the test.
   Commit the generated file.
8. **Document it:** add a section to `crates/README.md` (what it is, public API in a few lines, features, which apps use it,
   the generated files and their regenerate command) and a row in the member list at the top.
9. **Verify:** `cargo fmt` on your files, `cargo clippy -p paddock-<name> --all-targets --all-features -- -D warnings`,
   `cargo nextest run -p paddock-<name> --all-features`, then `node scripts/publish.test.mjs` (and `node scripts/publish.mjs
   --offline` as the dry run: the crate should show as held back while it is a scaffold).
10. **Commit** with Conventional Commits (`feat(paddock-<name>): ...`), no attribution, one complete crate per commit.

## Pitfalls
- A dev-dependency on another shared crate that carries a version breaks the publish (it must exist in the registry first);
  keep dev-dependencies path-only or order the publish.
- Bumping a crate's minor version without bumping its dependents.
- Forgetting to commit the hack crate or lockfile after `hakari generate`.
- Duplicating behaviour an existing crate already tests (rule 43).
