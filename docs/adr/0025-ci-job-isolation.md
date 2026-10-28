# 25. CI job isolation: any number of jobs per host, job-scoped state in the runner's job folder, kept folders per slot

- Status: Proposed (2026-10-28; independent review 2026-10-28: changes asked for, addressed below, the slot lock moves to
  Rust)
- Date: 2026-10-28
- Related: rule 2 (own worktrees, build folders and ports), rule 7 (tests never touch the machine), rule 40 (recurring bugs
  mean a missing design rule), rules 42/43 (share first); ADR 0014 §1 (build folders per app), ADR 0022 (all tooling in
  Rust: `paddock-dev`, `./dev`); the branches `ci/consolidate-dyno`, `fix/ci-home-relocatable` (`conformance/ci/ci-home.sh`)
  and `tools/paddock-dev-r0` (`./dev ci slot`)
- Code: `conformance/ci/job-isolation.sh`, `conformance/ci/kept-folders.txt`, `conformance/ci/ports.txt`,
  `.gitea/actions/job-isolation`; tests in `conformance/test/job-isolation.test.mjs`

## Context

The user wants the macOS and Windows host runners raised from capacity 1 to 2, and later possibly higher. The kit was
written for one job per host: every kept folder was named after the app and job only (`~/.paddock-ci/target-<app>-<job>`,
`src/<repo>-<job>-<os>`, `angular/<job>`, `test-browser-<app>`, `libduckdb/<job>`, Debrief's
`~/.debrief-ci/node-cache`), test temp folders lived in the user's shared `$TMPDIR`, and `host-cleanup` killed processes by
exact name or by patterns anchored on those shared folders. With two jobs of the same app and job on one host:

- `stable-workspace` of the second job `rm -rf`s and replaces the first job's checkout while it builds;
- `host-cleanup` kills the other job's test processes (exact names; patterns on the shared build folder) and deletes its
  live `<app>-it-*` temp folders;
- `prune-build-folder` prunes the folder the other job is building in; Debrief's Windows `node_modules` clean-up removes
  the copy the other job is linked to;
- `setup-rust` stops the shared sccache server on port 4227 to restart it, and `sccache-stats` stops it at the end, under
  the other job;
- the WebKit job binds fixed ports; two concurrent downloads of the same tool write one file;
- cargo serialises on its build-folder lock (safe, but the second job waits for the first).

Every instance is one pattern: per-host state keyed by something two concurrent jobs share. Fixing them one by one would
leave the next cache or clean-up to repeat it (rule 40).

## Facts about the runner (verified 2026-10-28 unless marked)

- The Mac runner is `gitea-runner version v5.2.3` (`gitea.com/gitea/runner`), host mode, `capacity: 1`,
  `host.workdir_parent: /Users/decryp7/gitea-runner/work`; its `config.yaml` sets no `runner.timeout` (keys read on the
  machine). The default is 3 h, and the Gitea instance also stops a job after 3 h by default
  ([config.example.yaml](https://gitea.com/gitea/runner/src/branch/main/internal/pkg/config/config.example.yaml)).
- Each job gets its own folder `<workdir_parent>/<16 hex>/` with `hostexecutor/` (the checkout, `GITHUB_WORKSPACE`),
  `tmp/`, `tool_cache/` and `act/`, removed when the job ends:
  [act/runner/run_context.go](https://gitea.com/gitea/runner/src/branch/main/act/runner/run_context.go),
  `startHostEnvironment` (`miscpath` from 8 random bytes; `Path: miscpath/hostexecutor`, `TmpDir: miscpath/tmp`,
  `CleanUp: os.RemoveAll(miscpath)`), so two concurrent jobs have distinct folders;
  [act/container/host_environment.go](https://gitea.com/gitea/runner/src/branch/main/act/container/host_environment.go),
  `GetRunnerContext` (`"temp": e.TmpDir`: `RUNNER_TEMP` is the job's own `tmp/`) and `Remove` (ends the job's process
  group, then deletes the folder). Observed on the Mac: one such folder during a running job, its `tmp/` holding that
  job's files.
- The tool cache is per job unless `runner.tool_cache_mode` shares it; the runner warns that a shared tool cache with
  capacity above 1 corrupts concurrent installs (its binary's message).
- **Not verified:** the Windows runner's version, work folder and `runner.timeout` (assumed the same runner and defaults;
  to be checked before its capacity is raised), and whether Linux job containers get a network of their own.

## Decision

Two kinds of per-host state, one mechanism, declared lists:

1. **Job-scoped state lives in the runner's own job folder.** The `job-isolation` action (first bash step of a host job)
   sets `TMPDIR` (and `TMP`/`TEMP` on Windows) and `PADDOCK_JOB_TEMP` to `$RUNNER_TEMP`. Test temp folders, the clean-up's
   temp sweep and process patterns anchored on `{tmp}` are then the job's own, and the runner deletes them with the job.
   Nothing that need not outlive the job goes in the CI home.
2. **Kept folders are per slot.** What must keep a stable absolute path to stay warm (cargo's build folder, whose
   fingerprints hold absolute paths; the stable checkout, which sccache keys on; the Angular cache; the test browser;
   libduckdb; Debrief's Windows `node_modules`) gets a slot suffix: `<layout>-s<n>`. A job holds the first free slot of
   `s1..s$PADDOCK_CI_SLOTS` (default 8, so capacity 3 or 4 needs no code change) for its whole run and gives it back in its
   last step (`if: always()`).
3. **The slot lock is an OS file lock, in Rust.** `./dev ci slot acquire|release|status` (ADR 0022's `paddock-dev`, branch
   `tools/paddock-dev-r0`) holds an OS file lock per slot (`flock`/`LockFileEx` on `<ci-home>/slots/s<n>.lock`) in a
   process that lives as long as the job; the kernel releases it when that process dies, so there is no stale lock and no
   reclaim (no owner liveness test, no age limit). Until it exists, this branch's shell lock (`mkdir` plus a reclaim when
   the owner's `RUNNER_TEMP` is gone) stands in; the review found a race in its reclaim (two jobs can both hold a slot), so
   this branch is not merged before the switch, and the switch deletes the `mkdir` and reclaim code and its tests.
4. **Declared lists.** `conformance/ci/kept-folders.txt` declares every kept folder (kind, layout, `slot` or `shared`;
   `shared` only for stores safe for concurrent writers: npm's content-addressed cache, sccache's disk cache, tool
   installs renamed into place). `conformance/ci/ports.txt` declares every port block a host job binds (sccache, WebKit),
   one block per slot. Every action, workflow and CI script asks `job-isolation.sh path <kind> [<name>]` or
   `port <name>`; a new cache or port is one line.
5. **Clean-up and prune are scoped through the same script.** `host-cleanup` patterns must name `{tmp}`, `{build}` or
   `{slot}`; `{slot}` must end the pattern right after a kept folder's name and is filled in as `s<n>([^0-9]|$)`, so `s1`
   never matches `s12`. An unscoped pattern is skipped with a warning; exact process names are gone. `prune-build-folder`
   prunes only a folder `job-isolation.sh owns` (this slot's `target-*`).
6. **Disk is bounded.** Kept folders exist for at most `PADDOCK_CI_KEEP_SLOTS` slots (default 2, the capacity): a job in
   a higher slot removes that slot's folders at release, and every acquire sweeps the folders of higher slots no job
   holds. Build folders are capped by `prune-build-folder`. The first job in `s1` renames a kept folder of the old
   unslotted layout to its `-s1` name (warm, no second copy).
7. **Containers.** A Linux container is a new file system per job, but every container of a host shares the runner's
   `/sccache` mount (compile, npm and Angular caches). Acquire and release do nothing there yet and `path` uses `s1`; two
   runs of one job on one Linux host share its Angular cache on the mount. Keying that folder per run was rejected (cold
   on every run, unbounded growth on the mount); with the Rust lock on the mount, which all containers of a host see,
   containers get slots too.
8. **Checked.** Conformance tests fail when a host job using a kept folder lacks the acquire step before it (after Git
   bash on Windows) or the release as its last step; when any CI file spells a CI-home path (`.paddock-ci/`,
   `PADDOCK_CI_HOME`, `.debrief-ci`) instead of asking the script; when a declared kind is unused or a used kind
   undeclared; when an app's process pattern is not anchored on its job; and when two port blocks, for any slot up to
   the default `PADDOCK_CI_SLOTS`, overlap each other or an end-to-end port.

## Alternatives

- **Scope everything to the runner's job folder, no slots.** Fully isolated, but every build would start cold: cargo's
  fingerprints and sccache's keys include absolute paths. Rejected for kept folders; adopted for everything else.
- **Process-tree clean-up instead of path patterns.** The runner already ends the job's process group (`Remove`); the
  strays left are those that left the group or were reparented to launchd/init, which a tree walk no longer finds.
  Rejected; patterns anchored on the job's own folders find them.
- **Slots per name (a lock per kept folder).** Keeps the base folders warm when different jobs run together, but needs a
  lock per kind and leaves temp and ports unsolved. Rejected for one slot per job.
- **A lock file with owner liveness checks (the shell stand-in).** Needs a reclaim rule, and a reclaim races. Rejected
  for the OS lock.

## Consequences

- Disk: at most `PADDOCK_CI_KEEP_SLOTS` sets of kept folders per host (two build folders of up to 20 GB each per app at
  capacity 2). The crate job's old `test-browser` and Debrief's old `~/.debrief-ci` (other names now) are removed by hand
  once.
- The first run in a new slot is cold (its own checkout path for sccache, its own build folder).
- The CI home follows `PADDOCK_CI_HOME` (ci-home.sh, the one implementation); `job-isolation.sh` calls it.

## Rollout

1. Merge `ci/consolidate-dyno` (this branch's base), then `fix/ci-home-relocatable`.
2. When `./dev ci slot` exists (`tools/paddock-dev-r0`): switch this branch's acquire and release to it, delete the
   `mkdir` lock, the reclaim and their tests, then merge it. Its conflicts with `fix/ci-home-relocatable` (the actions'
   CI-home lines, `build-folder.sh`, `libduckdb-env.sh`, `ci.yml`, `debrief.yml`, `ci-actions.test.mjs`) are resolved in
   favour of this branch; relocatable's test-browser assertion on `ci-home.sh)` is dropped (the folder comes from
   `job-isolation.sh path browser`).
3. Raise the Mac runner's capacity (Pitcrew's Configuration tab or `SaveRunnerConfig`, which backs up the config and
   restarts after the current job); the Windows runner's after its facts above are checked.
