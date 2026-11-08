## Working in parallel (agents)

- Each task owns an explicit set of files; don't edit outside it. Shared integration points (`lib.rs` files, the
  workspace `Cargo.toml`, the wire types and DTOs) are changed by the integrating agent only, or by adding a single
  declared line.
- Don't run `cargo fmt --all` / `cargo clippy --fix` across the workspace while others are editing; format your own
  files (`rustfmt <files>`). Use a private `CARGO_TARGET_DIR`.
- Commit only complete, owned work. Never commit a workspace `Cargo.toml` listing crates whose sources aren't
  committed. After committing, verify the committed tree in isolation (`git archive HEAD`, then build and test) before
  pushing larger changes.
- Before pushing a merge, run `node scripts/merge-regression-check.mjs <merge-commit>` (or `--branch <b> --target <t>`, also with
  conflicts: those paths are skipped and named): errors for files where the result goes backwards against the first parent (a
  stale-tree branch commit that resets a file to an older blob, or a merge that changes a file the branch never touched), a
  warning when a file loses most of an earlier commit's lines.
- Windows-only and macOS-only code is checked and tested on its own CI runner; code that needs no platform SDK must at
  least type-check for the other targets (`cargo check --target x86_64-pc-windows-msvc`).
