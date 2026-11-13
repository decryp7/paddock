# paddock-dev (`./dev`)

The developer and CI tool of ADR 0022. Run it with `./dev <command>` (`dev.cmd` on Windows) from anywhere in the repository.

- **Binary cache.** `./dev` names the binary by an input hash (`inputs.txt` plus the compiler cargo uses) and keeps it in
  `$PADDOCK_DEV_HOME/bin/<hash>/` (default `~/.paddock-dev`). **Agents set `PADDOCK_DEV_HOME=/Volumes/d7sst7/dev-targets/paddock-dev-home`
  (never a folder in `~`); CI sets it under `PADDOCK_CI_HOME`. Tests always use a temporary `PADDOCK_DEV_HOME`.**
- **Build folder.** A miss builds in a folder of its own, `<$PADDOCK_DEV_TARGET_DIR>/target-<hash>.XXXXXX`
  (default base `tools/paddock-dev/target/`, ignored by git), never shared by two builds and removed after the tool is
  published (an old leftover goes after an hour); uncommitted changes under an input path build in
  `<base>/rustc-<compiler commit>/`, one folder per compiler. A compiler that changes while the build runs (a rustup update)
  makes the wrapper start over once with the new one, then fail with an `::error::` (E0514 and a missing `rustc.exe` were that). Agents point it outside `~`, with `CARGO_BUILD_JOBS=2`, through `heavy.sh`.
- **Release jobs** set `PADDOCK_DEV_FRESH=1`: build from source in a fresh folder, no compiler wrapper.
- `gen check [--all] [--only ids] [--files a,b] [--base rev]`: the checks of the generated and pinned files a change touches, from the registry `docs/generators.toml` (lockfiles, workspace hack, catalogues, notices, baselines, bindings, icons); `merge check <merge-commit>` runs the ones the merge makes due (the checkout must be the merge; `--no-generators` skips). Stale is exit 5 with the regenerate command, a check that could not run is exit 6.
- Commands today: `load` (start, hold or stop for new agents from the 1- and 5-minute load, the free space of d7 and d8 and the CI queue of the newest 200 runs; thresholds are flags scaled to the core count; `df` and `sysctl` time out after 10 s; a measurement that is missing or an unknown run status gives hold, never start; `--json`), `brief check <file|->` (a brief to an agent has the fields of `templates/agents/brief-template.md`, a scratch folder that is the agent's own subfolder, the worktree order claim then drop, and none of the known-bad patterns; a negation exempts only what follows it in its clause; a scout or `Worktree: none` brief needs the short field list; `-` or `--stdin` reads standard input; exit 5 on findings), `self hash`, `check run [--only C39,C53] [--files a,b] [--app x] [--out file]` (RT-5: the host of the repository checks in Rust, C39 and C53 first; findings, `paddock.waivers.toml` waivers, a run record per check so `not measured` is not `clean`; `--json` prints the report of `paddock-conformance` plus `runs`; exit 5 on an error; the JS host still runs both checks until the shadow release ends), `ci slot acquire|release|status`, `wait`, `graph affected`, `merge check <merge-commit> | --branch <b> --target <t>` (did a merge silently drop one side's hunks or tests? Exit 5 on findings; reasons go in the merge message as `Hunks-dropped: <path>: <reason>` (a folder ends in `/`), `Reverted: <path>: <reason>` and `Tests-removed: <name>: <reason>` (or `<file>::<name>`); generated files are skipped; a fifth detector runs `cargo hakari generate --diff` on the merge result (a copy of `crates/` and `.cargo/` in a temp folder, cargo-hakari prints its diff on standard error) and reports a stale `paddock-workspace-hack` as `[workspace-hack]`, which no merge message excuses (the fix: `cargo hakari generate`, then the lockfiles); a hakari that cannot run is a `NOT MEASURED` finding beside the other findings; a shallow clone is not measured; every judgement is three-way (base, mine, other, result) and git's own merge is the arbiter for whole-file takes; run it per commit as well as per merge (commit-level drops are invisible to a merge check); a check that could not look is an error, never a pass: ADR 0089), `locks check|sync` (every `Cargo.lock`, the npm lockfile and the cargo-hakari workspace hack are current: `cargo metadata --locked` per workspace, `npm ci --dry-run`, `hakari generate --diff`, `manage-deps --dry-run`, `verify`; one `::error::` per stale file with the fix, exit 5; `sync` regenerates them with cargo, cargo-hakari and npm only, never by hand: ER-40), `gates check|list|inputs|ratchet|model` (C100, ADR 0060: every check runs somewhere; the register is `docs/gates.toml`, the model is `src/gates/ci.rs`; `gates check --warn` keeps the findings and exits 0). `wait --plock <name> | --pid <pid> | --file <path> --line <regex> | --ci-run <id>` with a required `--max <duration>` is the one way for agents and scripts to wait: phase lines on standard error, an `::error::` line and the next step at the limit (never an endless wait); `--plock` reads plock's activity log (lines written after the wait began, `--include-past` for earlier ones), `--pid` checks pid and start time, `--ci-run` reads `GITEA_TOKEN` from the environment and never prints it. `ci dispatch <workflow.yml> --ref <ref> [--input k=v]... --max <duration>` dispatches a workflow only when no unfinished run shares its single-machine runner class (macOS, Windows): it waits like `wait --ci-run` and at `--max` ends with an `::error::` naming the busy run; a Linux-only workflow dispatches at once. `queue hold <pr>` / `queue unhold` stop and restart it; `queue tick --scratch <dir>` takes one step (git and the API only, in its own clone: it runs no code of any pull request) of the merge queue (build the train from `state/approved` pull requests, local gates, dispatch, drop and retry, fast-forward main; state in labels and the `train/queue` branch; docs/plans/merge-queue.md). `ci dispatch <workflow.yml> --ref <ref> [--input k=v]... --max <duration>` dispatches a workflow only when no unfinished run shares its single-machine runner class (macOS, Windows; read from the workflow files' runner labels, following `uses: ./.gitea/workflows/...`): it waits like `wait --ci-run` and logs which run it waits for, and at `--max` ends with an `::error::` naming the busy run and the next step; a Linux-only workflow dispatches at once (the token needs write scope, never printed). `ci slot acquire` prints `sN <token>` (JSON:
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
- Tests: run them with `cargo nextest run` (per-test time bound, default profile: stops a test after 120 s; a plain `cargo test` can hang forever on a test that waits for something that never comes). `cargo test --test it worktree` builds throw-away repositories (Unix). `WORKTREE_SH=<engineering-rules>/scripts/worktree.sh`
  adds the oracle comparison with `worktree.sh` (otherwise those tests print `not measured`).

## CI steps in Rust (docs/plans/ci-in-rust.md)

- `ci setup-rust [--sccache-backend local|auto|redis] [--sccache-redis-db <n>] [--tauri-linux-deps true|false]` is the setup-rust
  action after its toolchain: installs the pinned sccache (checksum-verified) when the runner lacks it, configures it for the job
  (slot port and folder on a host, the `/sccache` mount in a container, or Redis from `PADDOCK_SCCACHE_REDIS_ENDPOINT`), starts a
  fresh server and writes `RUSTC_WRAPPER=sccache` and its settings to `$GITHUB_ENV`; with the flag, the Tauri Linux libraries.
  A release, rule 54 or tag run still takes the action's shell steps until the release-path lane. `tests/it/setup_rust.rs` runs
  the action's shell and this command on the same inputs and compares what reaches the job.
- Third-party programs run through `src/ci_tool.rs` (`ToolCall`): the exact argument vector, secrets set for that program only
  and masked in every printed line, a timeout, a `run:`/`ran:` line pair (exit code with the NTSTATUS on Windows), the stderr tail
  of a failed captured run, and one `::error::` line with the next step from the caller.
## `disk`: free space, usage tracking and clean-up

Trigger: on 2026-11-12 the work disk fell to 158 GiB free unnoticed, and the clean-up that followed had to guess the owner of
about 200 build folders and 60 worktrees from names and times. Common practice: GitHub's runner images check free space at job
start, and cargo tracks the last use of its caches in SQLite (`cargo clean gc`, since 1.78).

- `./dev disk check [--path <p>]...` reads the free space of each path's disk (default the current folder). Under 200 GiB it
  prints one `::warning::` and the ten largest folders the index knows; under 50 GiB one `::error::` with the next step, and
  exit 1. `--warn-gib` / `--refuse-gib` or `PADDOCK_DISK_WARN_GIB` / `PADDOCK_DISK_REFUSE_GIB` set the levels. A disk whose free
  space cannot be read is "not measured" (a warning), never fine and never a refusal. `job-isolation acquire` runs it for the CI
  home and the workspace, so every host-runner job gets it; the shell script and the Rust port both do.
- Every folder the tooling makes gets `.paddock-owner.toml` (kind, owner, branch, head, created, last used, and the repository or
  worktree it belongs to). It is the source of truth and travels with the folder. `worktree add` writes it (and `worktree claim`
  touches it), the CI kit writes it for a slot's kept folders, and `./dev disk record <dir> --kind build --link <worktree>` writes or
  touches it for a build folder. In a worktree it is ignored through `<git-common-dir>/info/exclude`.
- The usage index (default `/Volumes/d8sst7/dev-targets/.paddock-disk.sqlite`, `--index` or `PADDOCK_DISK_INDEX`; WAL,
  15 s busy timeout) is a cache of the folders and a history of their sizes (90 days). A missing or corrupt index is set aside
  (`.corrupt-<time>`) and rebuilt from the records; nothing fails because of it.
- `./dev disk scan` refreshes states and sizes (sizes younger than 6 hours are kept; `--force` measures all). Sizes are measured on
  up to 4 threads (an SSD's worth, `--jobs`), each folder limited to 60 s (a limited size is shown with `+`).
- `./dev disk report` lists per disk every tracked folder with size, state, kind, last use, owner and branch, the totals by state
  and the folders that grew most in the last 7 days. States: live (locked, claimed, a train, a process inside, or used in the last
  48 hours), merged (HEAD is in `origin/main` or a train), stale-pushed (clean, every commit on a remote), unsaved (dirty,
  unpushed, or not established), orphan (no record).
- Every deletion of a tool-made folder goes through `safe_remove` (`src/safe_remove.rs`), the one "safe to remove" check: it
  removes only on positive proof, read again right before each removal (owner record; strictly below an allowed root with no
  symlink; no process with its current folder inside; every git repository or worktree at any depth below clean, no stash, a
  remote, nothing unpushed, a linked worktree unlocked and unclaimed). A fact it cannot establish (process list unreadable,
  a git error, a walk past its limit) is a refusal. `tests/it/safe_remove_lint.rs` fails when other code under `tools/` deletes
  folders without it; the listed exceptions (the CI slot sweep, `worktree drop` and `worktree clean`) adopt it in follow-ups.
- `./dev disk clean` is a dry run; `--apply` removes only merged and stale-pushed folders that are not live, worktrees through
  `git worktree remove` (no force), re-checking each one right before removing it, and logs every path and size. Never touched: unsaved,
  orphan and live folders, CI slots (the CI kit sweeps those), and anything without a record (runners, sccache, VMs, Docker, user folders).
- `./dev disk adopt` writes records for existing worktrees (`git worktree list`) and for build folders named after one (the
  longest matching worktree name); everything else stays an orphan.
- The nightly (`nightly.yml`, job `disk-summary`) writes `./dev disk report --summary` to the job summary; it follows the night's skip.
## `pr`: the branch ledger (user decision 2026-11-12)

Every branch is a draft pull request against main; its labels (`.gitea/labels.toml`) say where it is. Design and the merge
queue built on it: `docs/plans/merge-queue.md`. The token is `GITEA_TOKEN` (it needs write access to issues and pull
requests), sent in a header through curl's standard input and printed nowhere. `--server`, `--repo` and `--root` go right
after the command (`pr label` takes every later word as a change).

- `pr sync [--dry-run]` reads the repository's "autodetect manual merge" and warns while it is off (a fast-forwarded
  pull request then stays open on the server; `plan-status` still sees it as landed, from git). It creates the labels the server lacks and updates a changed colour, description or exclusivity; it
  never deletes one and lists the server's extra labels. `[[template]]` entries expand per app folder (`app/<app>`).
- `pr open <branch> [--labels a,b] [--base main]`: the branch must be pushed (`origin/<branch>` here). Title `WIP: <branch>`
  (Gitea's draft prefix), body = its commits, label `state/built` unless a `state/` label is given. Refuses a second open
  pull request for the branch. Opening one starts no CI run: no workflow has a `pull_request` trigger (the contract
  test `no_workflow_runs_on_a_pull_request`). It fetches the base and the branch from origin first.
- `pr label <branch> +state/approved -owes/windows-tests`: adding a label of an exclusive scope (`state/`, `review-round/`)
  removes the others of that scope; unknown labels are refused.
- `pr list [--state approved] [--no-ci]`: number, branch, head, labels and the newest run per workflow of the head.
- `pr plan-status [--write] [--files a,b]`: rows of `docs/plans/*.md` and `apps/*/docs/plans/*.md` tables with a State (or
  Status) column whose references (branch or commit code spans in the State cell, else in a Branch or PR column) disagree
  with the ledger. A pull request counts as landed when it is merged, labelled `state/landed`, or its head is on
  main (a fast-forward landing). `--write` sets landed rows to ``done (`<commit>`)``, keeping the cell's other references
  (``done (`<commit>`), was `feat/x` ``); the rest are left for a person. Exit 5 on findings;
  a reference nobody knows is `not measured`, never agreeing.
- Tests: unit tests against an in-memory server (`src/pr/fake.rs`) and `tests/it/pr.rs`, the binary and curl against a
  fake Gitea on 127.0.0.1. Never the real server.

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
