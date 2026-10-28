# 25. CI job isolation: any number of jobs per host, job-scoped state in the runner's job folder, kept folders per slot

- Status: Proposed (2026-10-28)
- Date: 2026-10-28
- Related: rule 2 (own worktrees, build folders and ports), rule 7 (tests never touch the machine), rule 40 (recurring bugs
  mean a missing design rule), rules 42/43 (share first); ADR 0014 §1 (build folders per app), ADR 0022 (the kit's scripts
  move to Rust later; this contract moves with them); the branch `fix/ci-home-relocatable` (`conformance/ci/ci-home.sh`)
- Code: `conformance/ci/job-isolation.sh`, `conformance/ci/kept-folders.txt`, `.gitea/actions/job-isolation`; tests in
  `conformance/test/job-isolation.test.mjs`

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
- the e2e WebKit job binds fixed ports; two concurrent downloads of the same tool write one file;
- cargo serialises on its build-folder lock (safe, but the second job waits for the first).

Every instance is one pattern: per-host state keyed by something two concurrent jobs share. Fixing them one by one would
leave the next cache or clean-up to repeat it (rule 40).

## Facts about the runner (verified 2026-10-28)

- The Mac runner is the Gitea runner `gitea-runner version v5.2.3` (`gitea.com/gitea/runner`), host mode, `capacity: 1`,
  `host.workdir_parent: /Users/decryp7/gitea-runner/work` (its `config.yaml`, read on the machine).
- Each job gets its own folder `<workdir_parent>/<16 hex>/` with `hostexecutor/` (the checkout, `GITHUB_WORKSPACE`), `tmp/`,
  `tool_cache/` and `act/`, and the folder is removed when the job ends: observed on the Mac during a running job (one such
  folder, `tmp/` holding the job's `$RUNNER_TEMP/temp-leaks.json`), and in the source,
  [act/runner/run_context.go](https://gitea.com/gitea/runner/src/branch/main/act/runner/run_context.go)
  (`startHostEnvironment`: `miscpath := filepath.Join(cacheDir, hex.EncodeToString(randBytes))` from 8 random bytes;
  `Path: miscpath/hostexecutor`, `TmpDir: miscpath/tmp`, `CleanUp: os.RemoveAll(miscpath)`). Two concurrent jobs therefore
  have distinct folders. That `RUNNER_TEMP` is `TmpDir` is read from the observation, not from the source (the runner
  context of the host environment is set in a file not read).
- The tool cache is per job unless `runner.tool_cache_mode` shares it; the runner itself warns that a shared tool cache with
  capacity above 1 corrupts concurrent installs (its binary's message).
- Not verified: the Windows runner's version and work folder (assumed the same runner; its config should be checked when
  its capacity is raised), and whether Linux job containers get a network of their own (ports in containers are not
  part of this decision).

## Decision

Two kinds of per-host state, one mechanism, one declared list:

1. **Job-scoped state lives in the runner's own job folder.** The `job-isolation` action (first bash step of a host job)
   sets `TMPDIR` (and `TMP`/`TEMP` on Windows) and `PADDOCK_JOB_TEMP` to `$RUNNER_TEMP`. Test temp folders, the clean-up's
   temp sweep and process patterns anchored on `{tmp}` are then the job's own, and the runner deletes them with the job.
   Nothing that need not outlive the job goes in the CI home.
2. **Kept folders are per slot.** What must keep a stable absolute path to stay warm (cargo's build folder, whose
   fingerprints hold absolute paths; the stable checkout, which sccache keys on; the Angular cache; the test browser;
   libduckdb; Debrief's Windows `node_modules`) gets a slot suffix: `<kind layout>-s<n>`. A slot is
   `<ci-home>/slots/s<n>`, taken by an atomic `mkdir` (the first free of `s1..s$PADDOCK_CI_SLOTS`, default 8, so capacity 3
   or 4 needs no code change), its owner file naming the job's `RUNNER_TEMP`. It is released by the job's last step
   (`if: always()`) and reclaimed only when its owner is gone: that folder no longer exists (the runner removed it), it is
   older than `PADDOCK_SLOT_MAX_AGE_MIN` (360; longer than any job is allowed to run: the runner's job timeout, 3 h by default per Gitea's documentation, not verified on these runners), or it never got an owner file
   within 5 minutes. A live job's slot is never taken (heavy.sh's lesson).
3. **One list.** `conformance/ci/kept-folders.txt` declares every kept folder: kind, layout, `slot` or `shared`. `shared`
   only for stores safe for concurrent writers (npm's content-addressed cache, sccache's disk cache, tool installs that
   land by an atomic rename). Every action, workflow and CI script asks `job-isolation.sh path <kind> [<name>]`; a new
   cache is one line.
4. **Clean-up and prune are scoped through the same script.** `host-cleanup` patterns must name `{tmp}`, `{build}` or
   `{slot}` (filled in by `job-isolation.sh pattern`; an unscoped pattern is skipped with a warning); exact process names
   are gone. `prune-build-folder` prunes only a folder `job-isolation.sh owns` (this slot's `target-*`). The sccache
   server port and the WebKit ports are `job-isolation.sh port <base> [<stride>]`.
5. **Containers collapse.** In a Linux container (a new one per job) acquire and release do nothing and `path` uses the
   single slot `s1`.
6. **Checked.** A conformance test fails when a host job using a kept folder lacks the acquire step before it (after Git
   bash on Windows) or the release as its last step, when any CI file spells a CI-home path (`.paddock-ci/`,
   `PADDOCK_CI_HOME`, `.debrief-ci`) instead of asking the script, when a declared kind is unused or a used kind
   undeclared, and when an app's process pattern is not anchored on its job.

## Alternatives

- **Scope everything to the runner's job folder, no slots.** Simplest and fully isolated, but every build would start cold:
  cargo's fingerprints and sccache's keys include absolute paths, so a per-job build folder and checkout path miss every
  workspace crate each run. Rejected for kept folders; adopted for everything else.
- **Process-tree clean-up instead of path patterns.** Stray test processes are exactly the ones whose parent died: they are
  reparented to launchd/init and are no longer in the job's tree. Rejected; patterns anchored on the job's own folders
  find them.
- **Slots per name (a lock per kept folder).** Keeps the base folders warm when different jobs run together, but needs a
  lock per kind and leaves temp and ports unsolved. Rejected for one per-job slot.
- **A slot count equal to the capacity.** A lock left by a job the runner could not clean up would then block a job.
  Rejected for an upper bound above it.

## Consequences

- Disk: up to one set of slot folders per concurrent job (two build folders of 20 GB cap each per app at capacity 2).
  `prune-build-folder` caps each; an unused slot's folders are not pruned automatically (a later clean-up task).
- The first run in a new slot is cold (its own checkout path for sccache, its own build folder).
- The CI home moves with `PADDOCK_CI_HOME` (ci-home.sh, the one implementation); `job-isolation.sh` calls it.
- Raising capacity: merge this, then raise the runner's capacity (Pitcrew's Configuration tab or `SaveRunnerConfig`, which
  backs up and restarts after the current job). Optional: `PADDOCK_CI_SLOTS` in the runner's `runner.envs`.
