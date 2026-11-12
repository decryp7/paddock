## Working in parallel (agents)

- Each task owns an explicit set of files; don't edit outside it. Shared integration points (`lib.rs` files, the
  workspace `Cargo.toml`, the wire types and DTOs) are changed by the integrating agent only, or by adding a single
  declared line.
- Don't run `cargo fmt --all` / `cargo clippy --fix` across the workspace while others are editing; format your own
  files (`rustfmt <files>`). Use a private `CARGO_TARGET_DIR`.
- Before committing, check the subject: `node conformance/bin/paddock-conformance.mjs commit-lint --subject "<subject>"` (at
  most 100 characters; detail goes in the body). A rejected commit commits nothing and leaves your staged files as they are:
  shorten the subject and commit again, never amend an earlier commit.
- Commit only complete, owned work. Never commit a workspace `Cargo.toml` listing crates whose sources aren't
  committed. After committing, verify the committed tree in isolation (`git archive HEAD`, then build and test) before
  pushing larger changes.
- Windows-only and macOS-only code is checked and tested on its own CI runner; code that needs no platform SDK must at
  least type-check for the other targets (`cargo check --target x86_64-pc-windows-msvc`).
