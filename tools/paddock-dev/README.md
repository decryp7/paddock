# paddock-dev (`./dev`)

The developer and CI tool of ADR 0022. Run it with `./dev <command>` (`dev.cmd` on Windows) from anywhere in the repository.

- **Binary cache.** `./dev` names the binary by an input hash (`inputs.txt` plus the compiler cargo uses) and keeps it in
  `$PADDOCK_DEV_HOME/bin/<hash>/` (default `~/.paddock-dev`). **Agents set `PADDOCK_DEV_HOME=/Volumes/d7sst7/dev-targets/paddock-dev-home`
  (never a folder in `~`); CI sets it under `PADDOCK_CI_HOME`. Tests always use a temporary `PADDOCK_DEV_HOME`.**
- **Build folder.** A miss, or uncommitted changes under an input path, builds into `$PADDOCK_DEV_TARGET_DIR`
  (default `tools/paddock-dev/target/`, ignored by git). Agents point it outside `~`, with `CARGO_BUILD_JOBS=2`, through `heavy.sh`.
- **Release jobs** set `PADDOCK_DEV_FRESH=1`: build from source in a fresh folder, no compiler wrapper.
- Commands today: `self hash`, `ci slot acquire|release|status`, `graph affected`. `ci slot acquire` prints `sN <token>` (JSON:
  `slot`, `token`); `release --slot sN --token <token>` frees only the holder that token belongs to. The holder is a detached process
  started in the CI home; it ends when the `--owner` job folder is gone, so run the tool from outside that folder. `--json` writes one object; exit codes are
  `paddock-cli`'s (2 usage, 4 refused, ...).
- When a path dependency in `crates/` changes its dependencies, regenerate this lockfile in the same change.

## Not ported yet (R0b gaps, listed on purpose)

`graph affected` reproduces `scripts/affected.mjs`'s plan for a file list, a base and head revision, and `--all` (recorded cases in
`tests/golden/`, and the script itself as a live oracle while it exists). Not ported until R4: `--ci` (the base from the event, the
Gitea API for the last green run, the job outputs and summary), `--app` / `PLAN_APP`, the CI matrix legs and nextest narrowing.
The prebuilt download of the binary, `dev-tool.yml` and the `dev-build` job, and `setup-rust`'s image fast path are R0b too.

The three integration tests that need this working tree as a git checkout (`self hash`, `graph affected`, the live golden plan) print `SKIPPED: <test> needs a git checkout: <why>` (shown by nextest through `success-output` overrides) and return when there is no `.git` at or above the tree (for example a `git archive` export); a `.git` that git cannot use fails with git's message, and they also fail when `CI` is set (not empty, `0` or `false`) or `PADDOCK_REQUIRE_GIT` is non-empty.
