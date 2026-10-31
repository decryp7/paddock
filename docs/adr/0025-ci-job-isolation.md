# 25. CI job isolation: any number of jobs per host, job-scoped state in the runner's job folder, kept folders per slot

- Status: Proposed (2026-10-28; amended in draft 2026-10-31 for Pitcrew 1.1; independent review 2026-10-28: "not yet", two P1s and the P2s below; all addressed on
  `ci/job-isolation`, see "Verified" and "Still open" at the end)
- Date: 2026-10-28
- Related: rule 2 (own worktrees, build folders and ports), rule 7 (tests never touch the machine), rule 40 (recurring bugs
  mean a missing design rule), rules 42/43 (share first); ADR 0014 §1 (build folders per app), ADR 0022 (all tooling in
  Rust: `paddock-dev`, `./dev`); Pitcrew ADR 0021 (a pool of capacity-1 runners per host); the branch `ci/job-isolation`
  (`fix/ci-home-relocatable`, `ci/concurrent-jobs` and `tools/paddock-dev-r0` merged, then the review's fixes)
- Code: `conformance/ci/job-isolation.sh`, `conformance/ci/kept-folders.txt`, `conformance/ci/ports.txt`,
  `.gitea/actions/job-isolation`, `crates/paddock-slots`, `tools/paddock-dev/src/slots.rs` (`./dev ci slot`),
  `tools/host-cleanup.sh`, `tools/macos-vm-smoke.sh`; tests in `conformance/test/job-isolation.test.mjs`,
  `crates/paddock-slots/tests/it/races.rs`, `tools/paddock-dev/tests/it/slots.rs`, `tools/test/*.test.mjs`

## Context

CI is the bottleneck, and the user wants more runners on the Mac mini and the Windows VM. Pitcrew ADR 0021 chose how: a
pool of pre-registered runners per host, each with `capacity: 1` and its own runner folder, all running as the same user
and so sharing **one CI home** (`~/.paddock-ci` or `PADDOCK_CI_HOME`). Raising one runner's `capacity` is not the knob. The
kit was written for one job per host: every kept folder was named after the app and job only (`~/.paddock-ci/target-<app>-<job>`,
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
- The Windows runner is `gitea-runner` v5.2.2 with the same layout, `C:\gitea-runner\work\<16 hex>\{hostexecutor,tmp}`
  (its job log of run 9679, 2026-10-24). **Not verified:** its `runner.timeout`, and whether it ends a job's processes
  through a Windows job object (which would end the slot's detached holder too; `release` reports that case, see
  "Still open").
- Linux jobs run in a container on a Docker network created for the job and removed after it
  (`GITEA-ACTIONS-TASK-<n>-WORKFLOW-...-network`, `docker create ... network=...` in the Linux job logs, run 9679), so
  concurrent containers never share a port.
- sccache's local disk cache takes one server: "The local storage only supports a single sccache server at a time.
  Multiple concurrent servers will race and cause spurious build failures"
  ([docs/Local.md](https://github.com/mozilla/sccache/blob/main/docs/Local.md), read 2026-10-28).

## Decision

Two kinds of per-host state, one mechanism, declared lists:

1. **Job-scoped state lives in the runner's own job folder.** The `job-isolation` action (first bash step of a host job)
   sets `TMPDIR` (and `TMP`/`TEMP` on Windows) and `PADDOCK_JOB_TEMP` to `$RUNNER_TEMP`. Test temp folders, the clean-up's
   temp sweep and process patterns anchored on `{tmp}` are then the job's own, and the runner deletes them with the job.
   Nothing that need not outlive the job goes in the CI home.
2. **Kept folders are per slot.** What must keep a stable absolute path to stay warm (cargo's build folder, whose
   fingerprints hold absolute paths; the stable checkout, which sccache keys on; the Angular cache; the test browser;
   libduckdb; Debrief's Windows `node_modules`; sccache's disk cache, which takes one server; npm's cache) gets a slot suffix:
   `<layout>-s<n>`. A job holds the first free slot of `s1..s$PADDOCK_CI_SLOTS` (default 4, Pitcrew's interim pool; set it
   in every runner's `runner.envs` to the pool maximum) for its whole run and gives it back in its last step
   (`if: always()`). Ports derive from the slot too (`ports.txt`).
3. **The slot lock is an OS file lock, in Rust.** `./dev ci slot acquire` (ADR 0022's `paddock-dev`, the crate
   `paddock-slots`) starts a detached holder that takes `flock`/`LockFileEx` on `<ci-home>/slots/s<n>.lock` and keeps it
   until the job's `RUNNER_TEMP` is gone or `release` (with the holder's token) asks it to stop. The kernel releases the
   lock when the holder dies, so there is no stale lock, no owner liveness test and no reclaim: two jobs cannot hold one
   slot. One age limit remains, for a **stalled runner** that keeps a job folder after the job is gone (so the holder
   would never see it disappear): the holder lets go after `DEFAULT_MAX_HOLD_SECS` (`tools/paddock-dev/src/slots.rs`) =
   `RUNNER_JOB_TIMEOUT_SECS` (3 h, the runner's and Gitea's job timeout) + `HOLD_MARGIN_SECS` (30 min for the runner's own
   clean-up), so it can never end before a live job does. A job may not run longer than that: raising `runner.timeout`
   means raising the constant in the same change. The shell `mkdir` lock and its reclaim are deleted. The holder leads its own process group (the
   runner ends the step's group), runs from the CI home and from a binary in the shared `tools` kept folder (never inside
   a job folder, which Windows could then not delete). `release` fails the job when the holder died before the job ended.
4. **Declared lists.** `conformance/ci/kept-folders.txt` declares every kept folder (kind, layout, `slot` or `shared`;
   `shared` only for stores safe for concurrent writers: tool installs renamed into place). `conformance/ci/ports.txt` declares every port block a host job binds (sccache, WebKit), one block per slot.
   Every action, workflow and CI script asks `job-isolation.sh path <kind> [<name>]` or `port <name>`; a new cache or
   port is one line.
5. **Clean-up and prune are scoped through the same script.** `host-cleanup` patterns must name `{tmp}`, `{build}` or
   `{slot}`; `{slot}` must end the pattern right after a kept folder's name and is filled in as `s<n>([^0-9]|$)`, so `s1`
   never matches `s12`. An unscoped pattern is skipped with a warning; exact process names are gone. On Windows the
   processes are ended through PowerShell over `Win32_Process` command lines (Git bash has no `pkill`, and its `ps` sees
   only MSYS programs). Afterwards a **leak check** lists any process still matching one of the job's patterns.
   Drivers outside the job's folders (tauri-driver, safaridriver, WebKitWebDriver) are never ended by name: the suite that
   starts one stops it, on ports chosen for the run (Debrief's `wdio.conf.ts`), and a pattern can find it through an
   argument naming the job's folder. A pattern that would match the slot's own holder (`ci slot hold --owner <job
   temp> --root <ci-home>`, e.g. a bare `{tmp}`) is refused, so the clean-up can never free the slot mid-job.
   `prune-build-folder` prunes only a folder `job-isolation.sh owns` (this slot's
   `target-*`).
6. **Disk is bounded, and a slot's folders are deleted only under its lock.** `./dev ci slot idle-run --slot s<n> --
   <command>` runs the command only while `s<n>` is idle and holds its lock until the command ends, so no job can take the
   slot between the check and the deletion, and a held slot is never touched (`paddock_slots::with_idle`). A slot is
   locked only when something of it is due (checked before and again under the lock), so a sweep that would remove
   nothing never pushes a new job to a colder slot. Through it, at
   every acquire and release: the folders of idle slots above `PADDOCK_CI_KEEP_SLOTS` (default `PADDOCK_CI_SLOTS`, the pool
   maximum; jobs take the lowest free slot, so higher slots exist only after that many ran at once), and folders no job
   asked for in `PADDOCK_CI_ORPHAN_DAYS` (14; `path` renews a folder's time stamp, so folders no workflow names any more,
   and old-layout folders nobody took over, age out). At acquire, a **free-disk floor** (`PADDOCK_CI_MIN_FREE_GB`, 30;
   ER-24): below it, idle slots' folders go from the highest slot down; if that is not enough the job fails with what to
   do. sccache's per-slot cap is the host budget divided by the slots (`PADDOCK_CI_SCCACHE_GB` 40 / 4 = 10 GB); build
   folders are capped by `prune-build-folder` (20 GB). The first job in `s1` renames a kept folder of the old unslotted
   layout to its `-s1` name (warm, no second copy).
7. **Host-wide state outside the CI home is per job too.** The Tart VM smoke takes a host-wide VM slot (the same Rust lock
   under `TART_HOME`, waited for; `PADDOCK_VM_MAX` 1, at most 2, macOS's limit for macOS guests) instead of a `mkdir` lock
   whose stale-lock removal raced, and names its VMs with the job's `PADDOCK_VM_PREFIX`, so `service-smoke`'s leftover
   check sees only its own VMs.
8. **Containers.** A Linux job runs in a container with its own file system and its own Docker network (verified), so its
   ports (sccache 4227, the e2e shards) never collide. All containers of a Linux host share the runner's `/sccache` mount
   (the compile, npm and Angular caches), and sccache's local cache takes one server: **one job per Linux host** stays the
   rule until containers take slots from the same Rust lock on the mount (all containers of a host see one kernel and
   one file there). Acquire and release do nothing in a container, and `path` uses `s1`.
9. **Checked.** Conformance tests fail when a host job using a kept folder lacks the acquire step before it (after Git
   bash on Windows) or the release as its last step; when any CI file spells a CI-home path instead of asking the script;
   when a declared kind is unused or a used kind undeclared; when an app's process pattern is not anchored on its job;
   and when two port blocks, for any slot up to the default `PADDOCK_CI_SLOTS`, overlap each other or an end-to-end port.
   The lock itself is tested with real processes: eight parallel acquires get eight distinct slots, racing acquirers
   never share one, a killed holder frees its slot at once, a removed job folder frees it, a sweep under `with_idle` is
   never handed out and a held slot is never swept.

## Alternatives

- **Scope everything to the runner's job folder, no slots.** Fully isolated, but every build would start cold: cargo's
  fingerprints and sccache's keys include absolute paths. Rejected for kept folders; adopted for everything else.
- **Process-tree clean-up instead of path patterns.** The runner already ends the job's process group (`Remove`); the
  strays left are those that left the group or were reparented to launchd/init, which a tree walk no longer finds.
  Rejected; patterns anchored on the job's own folders find them.
- **Slots per name (a lock per kept folder).** Keeps the base folders warm when different jobs run together, but needs a
  lock per kind and leaves temp and ports unsolved. Rejected for one slot per job.
- **A lock file with owner liveness checks (the shell stand-in).** Needs a reclaim rule, and a reclaim races (the review
  showed two jobs holding one slot). Rejected for the OS lock.
- **One sccache disk cache shared by every slot.** sccache's docs say concurrent servers on one local cache race. Rejected
  for a cache per slot with the budget divided.
- **Raise one runner's `capacity`.** One runner process, restarted only when fully idle (Pitcrew ADR 0021, B). Rejected:
  add capacity-1 runners to the pool.

## Consequences

- Disk: at most `PADDOCK_CI_KEEP_SLOTS` sets of kept folders per host, bounded further by the orphan age and the free-disk
  floor; each slot's sccache is a share of one budget. The crate job's old `test-browser` and Debrief's old
  `~/.debrief-ci` (other names now) are removed by hand once.
- The first run in a new slot is cold (its own checkout path for sccache, its own build folder, its own sccache cache).
- Each host builds `paddock-dev` once per input hash (a release build, about a minute, into the shared `tools` folder)
  the first time a job acquires a slot; until the prebuilt download (ADR 0022, R0b) exists.
- The CI home follows `PADDOCK_CI_HOME` (ci-home.sh, the one implementation); `job-isolation.sh` calls it.

## Amendment 2026-10-31 (draft for review): slot spaces, Pitcrew's session and headless runners, and the open items

Written for Pitcrew 1.1 (ADR 0021 §1 and §1a, ADR 0023). Status of this amendment: **Proposed**; the ADR as a whole stays
Proposed until an independent review accepts both. Nothing above changes for a host with one account.

**A. A slot space is one CI home.** The slot lock lives in `<ci-home>/slots/`, so the unit of isolation is the CI home, and
a CI home is per account unless `PADDOCK_CI_HOME` says otherwise. Two jobs collide only inside one slot space. Definitions:

- *Session runners* (Pitcrew session mode) run as the logged-in user: one space, `~/.paddock-ci`.
- *Headless runners* (ADR 0023: LaunchDaemon, systemd system unit, per-runner Windows service) run as the service account or
  a per-runner virtual account. Their home differs from the user's, and on Windows from each other (`NT SERVICE\<name>` has
  its own profile), so by default each headless runner is **its own space with one slot**: no collision, no sharing, cold
  caches. To share warm caches, the owner sets one explicit `PADDOCK_CI_HOME` for those runners, a folder all their accounts may
  write (`/Users/Shared/paddock-ci`, `/var/lib/paddock-ci`, `%ProgramData%\paddock-ci`), created by Pitcrew with that
  access when the pool is configured. Runners sharing that folder are one space.
- Session and headless runners are separate spaces unless the owner points both at one folder. Pitcrew does not do it for
  them: the account that writes the folder differs, and a permission mix-up costs more than a cold cache.

**B. What Pitcrew declares and checks (ADR 0021 §1).** A pool declares `ci_isolation = "slots"` **per space**. For each space
Pitcrew writes `PADDOCK_CI_SLOTS` = `PADDOCK_CI_KEEP_SLOTS` = the number of runners of the pool in that space (default cap 4, as
`PADDOCK_CI_SLOTS` above) into every runner's `runner.envs`; a space with one runner needs no slots. Pitcrew's config check
rejects: two runners of one space with different `PADDOCK_CI_HOME`; a pool whose runners share an account but name different
homes; `PADDOCK_CI_HOME` on a session slot that differs from the user's default when other session slots keep the default.
Each check offers a Fix through ADR 0011's editor. Pitcrew cannot see the kit, so `ci_isolation` stays a declared precondition.

**C. Session loss.** A session runner stops when its session ends (Pitcrew drains it between jobs, ADR 0021 §4). A job that
dies with the session leaves the holder with it: the holder is a child of the job's step, the kernel releases the lock, and the
next acquirer sweeps by `with_idle`. No change to the lock. A *headless* runner is unaffected by logon state.

**D. Closing the "Still open" items.**
1. *Concurrency with two or three runners on one host.* Becomes an acceptance test: `tools/paddock-dev` integration tests
   already run eight parallel acquires with real processes; add one that uses three real `act`-style job folders under a fake
   runner (Pitcrew's `fake_runner` example) on the three OS, run on the CI hosts at the end. The real first runs stay a rollout
   step, watched with `job-isolation.sh status`.
2. *Windows `runner.timeout`.* The holder's age limit (3 h + 30 min) assumes the runner's job timeout is at most 3 h. Pitcrew
   reads `runner.timeout` from every runner's `config.yaml` (it does, ADR 0011) and reports a pool runner whose timeout is
   longer than `RUNNER_JOB_TIMEOUT_SECS` as an error with a Fix (set it back, or the owner raises the constant in the same
   change, as this ADR requires). This makes the unverified Windows value irrelevant: Pitcrew checks the file on every host.
3. *Linux stays one job per host until containers take slots.* **Conflict to resolve in review:** ADR 0021 records that d-gtr9 runs
   one runner with `runner.capacity: 4` (container jobs), the decided Linux maximum, while §8 above says one job per Linux host
   is the rule because every container shares the `/sccache` mount and sccache's local cache takes one server. Either (a) the
   four concurrent jobs already race on sccache and the cache needs per-container slots now, or (b) they work today because
   sccache is per-container and §8 is too strict. The measurement decides: run four concurrent Linux jobs, read the sccache
   server logs and stats of each, record the result here. The measurement is the 4-concurrent-job runs of the Linux CI
   build-share A/B on d-gtr9 (`handoff/linux-ci-share-ab.md`, throw-away `ci-share-ab.yml`), after the Spotter and Pitwall tags.
   Evidence to collect from every one of those runs, per job (J1..J4) and for the run as a whole:
   - *sccache:* the server's start line and port as each job sees it (`sccache --show-stats` at the job's start and end; the
     process list of `sccache` servers inside and outside each container); whether one server served several jobs; cache
     writes that fail or report "address in use", a corrupted-entry or lock message in any job log; hits and misses per job
     against the same job run alone (a drop that is not explained by cold start is a collision).
   - *The `/sccache` mount:* file count and size before and after; any `*.tmp`/partial file left; two jobs writing one path
     (an `lsof`/inotify-free check: compare the mount's change list per job by the job-start and job-end listing).
   - *Ports:* every port each job binds (`ss -ltnp` inside the container at the job's busiest step: the e2e shards, sccache
     4227, the test browsers) and any `EADDRINUSE`/"address already in use" in a log; the Docker network name of each job
     (expected distinct: `GITEA-ACTIONS-TASK-<n>-...`).
   - *Temp and caches:* `RUNNER_TEMP` and `TMPDIR` of each job (expected distinct), and the npm and Angular cache folders as
     seen by each (shared or private); any "file exists", "ENOENT" or "lock" error in a log that the single-job baseline
     does not have.
   - *Outcome differences:* for each job, result, duration and failed-step names against its single-job baseline; any test
     that fails only when concurrent; the final line counts of warnings. A run of the four in different orders (start offsets
     of 0 and 30 s) to cover the race windows.
   - *Verdict rule:* no collision and no outcome difference in all runs => (b), section 8 is relaxed to "one sccache server per
     container is safe, the mount is shared read-mostly" with the evidence cited; any collision => (a) and package I1b. If (a), slots on the mount are the same Rust lock with the mount's
   `slots/` folder, a per-slot `/sccache/s<n>` (cap = budget divided), and acquire/release stop being no-ops in a container;
   package I1b.
4. *Disk budgets per space.* `PADDOCK_CI_SCCACHE_GB` and `PADDOCK_CI_MIN_FREE_GB` apply per space; on a host with two spaces
   the owner sets them so the sum fits (Pitcrew's pool page shows the sum and warns above the volume's free space, ER-24).

**E. Implementation (package I1, after the review accepts this).** (1) `paddock-slots`: nothing new for A; add the explicit
`PADDOCK_CI_HOME` permission note to the crate docs. (2) `conformance/ci/job-isolation.sh`: `status` prints the space (the home)
and its holders. (3) Pitcrew (ADR 0021 WP-5): the three config checks of B and the timeout check of D2, tests first with
fake `config.yaml` text. (4) The concurrency test of D1. (5) Linux per D3 only if the measurement says (a).

## Rollout

1. Merge `ci/job-isolation` (it already carries `fix/ci-home-relocatable`, `ci/concurrent-jobs` and
   `tools/paddock-dev-r0`; those branches are then deleted).
2. Per host, after "Still open" is cleared for it: set `PADDOCK_CI_SLOTS` to the pool maximum in every runner's
   `runner.envs` (only when above 4), then add runners to the pool with Pitcrew (capacity 1 each, all as the same user,
   so one CI home). Never raise a runner's `capacity`.
3. Watch the first concurrent runs: `bash conformance/ci/job-isolation.sh status` lists the holders; a stalled one
   (its job folder kept) is cleared by deleting that job folder or ending its pid, never by deleting `slots/*.lock`.

## Verified

- On the branch: the conformance, tools and scripts tests (`node --test`); `paddock-slots` and `paddock-dev` tests,
  clippy and `cargo fmt --check` on macOS, including eight parallel acquires getting eight distinct slots and the sweep
  race test (repeated); an end-to-end run on the Mac of eight parallel `job-isolation.sh acquire` with the real binary:
  eight distinct slots, three released by token, the other five freed when their job folders were deleted, no holder
  left behind.
- Runner facts above (Mac config and job folders; Windows layout from its log; Linux per-job networks from its log;
  sccache's docs).

- CI on the real host runners (ci.yml run 6856 on b92ab37b0, 2026-10-29): all 17 macOS and Windows jobs green (crate,
  paddock-dev, and the Dyno, Pitcrew, Pitwall and Spotter app legs on both systems), each taking a slot through the Rust
  holder and releasing it with its token; "holder ended before the job did" appears in no log, so the Windows runner does
  not end the detached holder with the job's processes. Earlier runs found and fixed: bash 5.2's `${v//p/r}` and `-e`
  differences, a stderr message captured as a path (`2>&1` in stable-workspace), stale app lockfiles, an unpinned
  cargo-about. One Windows job (15657, run 6731) was dropped by its runner mid-test (the next job started 16 s later;
  Gitea failed it 10 minutes on): no step of ours ran or hung; its slot was free again within a minute.

## Still open (blocks adding runners)

- Concurrency itself: every run so far had one runner per host. The first runs with two or three runners on a host
  (Pitcrew pool) are the real test; watch `job-isolation.sh status` and the release step.
- The Windows runner's `runner.timeout` (a job longer than it is killed; the holder then ends with the job folder).
- Linux stays one job per host until containers take slots (decision 8).
