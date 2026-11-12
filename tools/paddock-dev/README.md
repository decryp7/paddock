# paddock-dev (`./dev`)

The developer and CI tool of ADR 0022. Run it with `./dev <command>` (`dev.cmd` on Windows) from anywhere in the repository.

- **Binary cache.** `./dev` names the binary by an input hash (`inputs.txt` plus the compiler cargo uses) and keeps it in
  `$PADDOCK_DEV_HOME/bin/<hash>/` (default `~/.paddock-dev`). **Agents set `PADDOCK_DEV_HOME=/Volumes/d7sst7/dev-targets/paddock-dev-home`
  (never a folder in `~`); CI sets it under `PADDOCK_CI_HOME`. Tests always use a temporary `PADDOCK_DEV_HOME`.**
- **Build folder.** A miss, or uncommitted changes under an input path, builds into `$PADDOCK_DEV_TARGET_DIR`
  (default `tools/paddock-dev/target/`, ignored by git). Agents point it outside `~`, with `CARGO_BUILD_JOBS=2`, through `heavy.sh`.
- **Release jobs** set `PADDOCK_DEV_FRESH=1`: build from source in a fresh folder, no compiler wrapper.
- Commands today: `self hash`, `check run [--only C39,C53] [--files a,b] [--app x] [--out file]` (RT-5: the host of the repository checks in Rust, C39 and C53 first; findings, `paddock.waivers.toml` waivers, a run record per check so `not measured` is not `clean`; `--json` prints the report of `paddock-conformance` plus `runs`; exit 5 on an error; the JS host still runs both checks until the shadow release ends), `ci slot acquire|release|status`, `wait`, `graph affected`, `merge check <merge-commit> | --branch <b> --target <t>` (did a merge silently drop one side's hunks or tests? Exit 5 on findings; reasons go in the merge message as `Hunks-dropped: <path>: <reason>` (a folder ends in `/`), `Reverted: <path>: <reason>` and `Tests-removed: <name>: <reason>` (or `<file>::<name>`); generated files are skipped; a shallow clone is not measured; every judgement is three-way (base, mine, other, result) and git's own merge is the arbiter for whole-file takes; run it per commit as well as per merge (commit-level drops are invisible to a merge check); a check that could not look is an error, never a pass: ADR 0089), `locks check|sync` (every `Cargo.lock`, the npm lockfile and the cargo-hakari workspace hack are current: `cargo metadata --locked` per workspace, `npm ci --dry-run`, `hakari generate --diff`, `manage-deps --dry-run`, `verify`; one `::error::` per stale file with the fix, exit 5; `sync` regenerates them with cargo, cargo-hakari and npm only, never by hand: ER-40), `gates check|list|inputs|ratchet|model` (C100, ADR 0060: every check runs somewhere; the register is `docs/gates.toml`, the model is `src/gates/ci.rs`; `gates check --warn` keeps the findings and exits 0). `wait --plock <name> | --pid <pid> | --file <path> --line <regex> | --ci-run <id>` with a required `--max <duration>` is the one way for agents and scripts to wait: phase lines on standard error, an `::error::` line and the next step at the limit (never an endless wait); `--plock` reads plock's activity log (lines written after the wait began, `--include-past` for earlier ones), `--pid` checks pid and start time, `--ci-run` reads `GITEA_TOKEN` from the environment and never prints it. `ci slot acquire` prints `sN <token>` (JSON:
  `slot`, `token`); `release --slot sN --token <token>` frees only the holder that token belongs to. The holder is a detached process
  started in the CI home; it ends when the `--owner` job folder is gone, so run the tool from outside that folder. `--json` writes one object; exit codes are
  `paddock-cli`'s (2 usage, 4 refused, ...).
- When a path dependency in `crates/` changes its dependencies, regenerate this lockfile in the same change.

## `worktree` and `check primary` (ADR 0083, RT-3)

One tool makes, claims and removes agent worktrees, so every one is locked with an owner note and bound to its agent, and no
cleanup removes a locked one. Run it from any checkout of the repository (or pass `--repo <folder>`); `--json` as everywhere.

- `worktree add <name> --base <ref> [-b <branch>] [--detach] [--profile gates | --sparse a,b] --owner <role>:<task> [--root <folder>]`
  makes `<root>/<name>`: a full checkout (`--profile gates`, the default, because conformance, `affected.mjs`, `gates check` and `plan`
  read most folders); `--sparse` only with `--detach` (a review copy that reads a few folders). The branch is `<name>` unless `-b`;
  an existing local branch, or one only a remote has, is attached to (its ahead and behind counts against `--base` and its remote
  are printed; a branch already merged into the base, or behind its remote, is refused with the fix). Always locked with the note
  `owner=<role>:<task>; branch=; base=; created=; by=`. The root is `--root`, else `git config paddock.worktreeRoot`, else
  `PADDOCK_WORKTREE_ROOT`; with none set it fails and says how to set one. It refuses an existing path, a path in the owner map or
  being made by another `add`, a branch another worktree has checked out, and **any path inside the primary checkout or another worktree** (compared
  by real path). The owner map's lock is held only to reserve and to confirm the path, never while git checks out, so claims
  never wait for a slow `add`. It does not print `CARGO_TARGET_DIR` or an e2e port yet (RT-6).
- `worktree claim <path>` binds a locked worktree to the caller; refuses an unknown, unlocked or other-owned one.
  `reassign` (coordinator only) clears the entry and keeps the lock; `release` unlocks (the owner, or the coordinator);
  `drop` removes the caller's own worktree and refuses unpushed commits or uncommitted changes.
- `worktree list` (JSON: branch, lock note, claimed agent, lock age, merged, dirty, in use) and `worktree clean [--dry-run]`,
  which removes only worktrees that are unlocked, merged into `--base` (default `origin/main`), clean and with no process inside,
  with `git worktree remove` (never `rm`, never forced) and a branch delete that names the head it looked at. It does not remove
  build folders yet (RT-6). Where other processes' folders cannot be read (Windows) it keeps everything.
- **The caller's identity is self-declared** (`--agent`, or `PADDOCK_AGENT_ID` and `PADDOCK_AGENT_TYPE`; none means the
  coordinator) until the guard hook (RT-7) passes the real `agent_id`: this keeps honest agents out of each other's worktrees, it
  does not stop one that lies.
- The owner map is `<git-common-dir>/paddock-worktree-owners.json` with the sidecar lock `paddock-worktree-owners.lock` and the
  sidecar `paddock-worktree-owners.holder` (who holds the lock: a locked file cannot be read on Windows; removed when the update ends)
  (`src/worktree/owners.rs`, which RT-7's hook reuses). A file it cannot read is an error, never an empty map. A lock wait ends
  after 15 s and names the process that holds it.
- `check primary [--live <file>] [--allow-untracked <prefix>] [--baseline <file>] [--accept-untracked]` probes the primary
  checkout: on `main` at its tip, no commit `origin/main` lacks, no tracked change, sparse checkout or half-done merge, no worktree
  inside it, and the locked worktrees and the owner map match in both directions. Untracked entries outside the allow-list
  (`.claude/densepack-vault/`, `.serena/`, `node_modules`) and the baseline `paddock-primary-untracked.txt` in the git directory
  are errors; the coordinator records the current ones with `--accept-untracked`. Without `--live` it prints `liveness: not
  measured` and calls no lock stale. Exit codes: 0 ok, **5** a problem was found, **6** only checks that could not run (not
  measured; never a pass), as `paddock-cli` plus this one.
- Tests: `cargo test --test it worktree` builds throw-away repositories (Unix). `WORKTREE_SH=<engineering-rules>/scripts/worktree.sh`
  adds the oracle comparison with `worktree.sh` (otherwise those tests print `not measured`).

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
