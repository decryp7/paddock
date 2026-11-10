# paddock-dev (`./dev`)

The developer and CI tool of ADR 0022. Run it with `./dev <command>` (`dev.cmd` on Windows) from anywhere in the repository.

- **Binary cache.** `./dev` names the binary by an input hash (`inputs.txt` plus the compiler cargo uses) and keeps it in
  `$PADDOCK_DEV_HOME/bin/<hash>/` (default `~/.paddock-dev`). **Agents set `PADDOCK_DEV_HOME=/Volumes/d7sst7/dev-targets/paddock-dev-home`
  (never a folder in `~`); CI sets it under `PADDOCK_CI_HOME`. Tests always use a temporary `PADDOCK_DEV_HOME`.**
- **Build folder.** A miss, or uncommitted changes under an input path, builds into `$PADDOCK_DEV_TARGET_DIR`
  (default `tools/paddock-dev/target/`, ignored by git). Agents point it outside `~`, with `CARGO_BUILD_JOBS=2`, through `heavy.sh`.
- **Release jobs** set `PADDOCK_DEV_FRESH=1`: build from source in a fresh folder, no compiler wrapper.
- Commands today: `self hash`, `ci slot acquire|release|status`, `wait`, `graph affected`, `gates check|list|inputs|ratchet|model` (C100, ADR 0060: every check runs somewhere; the register is `docs/gates.toml`, the model is `src/gates/ci.rs`; `gates check --warn` keeps the findings and exits 0). `wait --plock <name> | --pid <pid> | --file <path> --line <regex> | --ci-run <id>` with a required `--max <duration>` is the one way for agents and scripts to wait: phase lines on standard error, an `::error::` line and the next step at the limit (never an endless wait); `--plock` reads plock's activity log (lines written after the wait began, `--include-past` for earlier ones), `--pid` checks pid and start time, `--ci-run` reads `GITEA_TOKEN` from the environment and never prints it. `ci slot acquire` prints `sN <token>` (JSON:
  `slot`, `token`); `release --slot sN --token <token>` frees only the holder that token belongs to. The holder is a detached process
  started in the CI home; it ends when the `--owner` job folder is gone, so run the tool from outside that folder. `--json` writes one object; exit codes are
  `paddock-cli`'s (2 usage, 4 refused, ...).
- When a path dependency in `crates/` changes its dependencies, regenerate this lockfile in the same change.

## Not ported yet (R0b gaps, listed on purpose)

`dev plan` is the port of `scripts/affected.mjs` (`--ci`, `--app`, the matrix legs and the commit range; its recorded cases are in
`tests/golden/`); `graph affected` is its graph step. Still open from R0b: the `dev-build` job (one build per run for a hash nobody
published, shared as a run artifact) and `setup-rust`'s image fast path, both deferred (ADR 0022, status below). Done: `dev-tool.yml`
publishes the binary per input hash and runner kind from `main`, and the `dev` wrapper downloads it in CI (locally with
`PADDOCK_DEV_DOWNLOAD=1`), checks its checksum and embedded hash (transport checks, not a trust boundary), and builds on any miss
or doubt; a run with `PADDOCK_DEV_DOWNLOAD=0` (release and tag jobs) never uses a downloaded copy.

## The planner in CI

`ci.yml`'s plan job runs `./dev plan --ci` for every run (the app workflows already ran `./dev plan --ci --app <app>`); the golden
tests compare it with `scripts/affected.mjs` on every sample, recorded case, the repository's history and the live narrowing edits,
including C100's content rule (a test file that adds, moves or removes an ignore attribute or an environment guard marks conformance).
Fallback for one release (ADR 0022 R4): set the repository variable `PLAN_PLANNER` to `node` and the plan job plans with the script
again (it prints a `::warning::`); unset it to return. The script, its golden generator and this switch are removed together.
