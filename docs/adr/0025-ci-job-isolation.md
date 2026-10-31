# 25. CI job isolation: any number of jobs per host, job-scoped state in the runner's job folder, kept folders per slot

- Status: Accepted (2026-10-31), after independent review of the amendment of 2026-10-31 (draft 3, approved); the original text was Proposed 2026-10-28. Package G (below) implements it
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
   `<layout>-s<n>`. A job holds the first free slot of `s1..s$PADDOCK_CI_SLOTS` (default 4, at most `PADDOCK_CI_SLOTS_MAX` = 4; Pitcrew writes it per slot space, see the amendment, D1) for its whole run and gives it back in its last step
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

## Amendment 2026-10-31 (draft 2, after review 1): slot spaces, Pitcrew's session and headless runners, host-wide state, open items

Written for Pitcrew 1.1 (Pitcrew ADR 0021 §1 and §1a; the headless modes' ADR is planned and not written yet, so this text
names the modes, not an ADR). Status of this amendment: **Proposed**; the ADR as a whole stays Proposed until an
independent review accepts both. Nothing above changes for a host with one account and one space. Review 1 (CHANGES, 7 P1s)
is answered here; the owner's decisions of 2026-10-31 are marked.

**A. A slot space is one CI home, and a CI home belongs to one account.** The slot lock lives in `<ci-home>/slots/`, so the
unit of isolation is the CI home. Jobs collide only inside one space (their kept folders and their lock), but **ports, the
sccache server and the macOS VM limit are host-wide**, so they need a second coordinate (B and C). Spaces on a host:

- *Session runners* (Pitcrew session mode) run as the logged-in user: one space, that user's CI home.
- *Headless runners* (LaunchDaemon, systemd system unit, Windows service: the modes of Pitcrew 1.1) of one machine **run as one
  dedicated service account when their caches are shared** (owner's decision): `UserName` of the LaunchDaemon, `User=` of the
  unit, one local Windows account. That account's CI home is one more space. Only when caches are *not* shared may Pitcrew give
  a runner its own virtual account; such a runner is a space of one slot.
- The *shared-folder variant is dropped*: no `PADDOCK_CI_HOME` is ever shared between two accounts. Pitcrew rejects a
  `PADDOCK_CI_HOME` that two accounts resolve to (an error with a Fix that removes the override from one of them).
- Space identity is derived, not typed: `SlotSpace { home: CanonicalPath, accounts: Vec<Account> }`, computed by Pitcrew from
  each runner's effective environment (`environment::compose`, Pitcrew ADR 0022) and its account, with the home resolved and
  canonicalised (symlinks, case on Windows and macOS, trailing separators). Two runners are in one space exactly when their
  canonical homes are equal.

**B. A space number keeps host-wide ports apart (P1-1).** Ports are bound on the host, not in a home: two spaces that both
take `s1` would both bind sccache `4227`. So Pitcrew assigns each space on a host a number `k` (1 to `PADDOCK_CI_SPACES_MAX`,
4), stable like `env_number` (kept while the space exists, lowest free, never reused while it exists), and writes it as
`PADDOCK_CI_SPACE` into every runner of the space. `ports.txt` blocks become
`base + ((k-1) * PADDOCK_CI_SLOTS_MAX + (n-1)) * stride` for space `k`, slot `n`; `PADDOCK_CI_SLOTS_MAX` is 4 (the default of
`PADDOCK_CI_SLOTS`), and a space with no `PADDOCK_CI_SPACE` is space 1 (a host with one account is unchanged).
`job-isolation.sh port` adds the term; the conformance test checks that no two blocks overlap for any `k <= 4` and `n <= 4`
and none overlaps an end-to-end port (it fails today for two spaces: both take `s1` and the same block).
`PADDOCK_CI_SPACES_MAX` is a constant of the kit; a host with more spaces than it is refused by Pitcrew's check.

**C. Host-wide limits that are not per space (P1-2).** The Tart VM smoke takes a VM slot under `TART_HOME`, which today lies in
one account's home, but Apple's limit of 2 macOS guests is per host. Decision: **one host-wide VM lock directory**,
`/Users/Shared/paddock-vm/slots` (mode 1777; the lock files 0666, created by the first account), used by
`macos-vm-smoke.sh` for every account regardless of `TART_HOME`; `PADDOCK_VM_MAX` is read from the job's environment as today
and must be equal for every runner (Pitcrew writes it, with the others in D). VM jobs therefore work in any space; the
alternative (VM jobs only on session runners through a `needs_session` label) is kept for hosts where the owner prefers it
and costs nothing extra. Conformance: `macos-vm-smoke.sh` must not derive its lock path from the account's home.
**Crate changes this needs (`paddock-slots`, G1).** `FsLocks` in a folder shared between accounts must work across uids, which
the current code does not: its `sN.owner`/`sN.release` records are written with the default umask and cannot be overwritten by
another uid in a sticky directory, lock files are created 0644, and creating or writing follows symlinks. Required: lock files
opened with `O_NOFOLLOW` and an explicit 0666 (`fchmod` after create), anything that is not a regular file refused, the
owner/release records named per uid (`sN.<uid>.owner`, `sN.<uid>.release`) so no account overwrites another's, tested with
two uids where the OS allows it and otherwise in the VM test. (The alternative, a root-created folder with an ACL for the CI
accounts and pre-created lock files, is rejected: it needs an admin step per host and breaks when a new account is added.)
Two further requirements (review 1, package G notes). (1) The shared VM lock folder is created with an explicit mode 1777
(`set_permissions` after create, so the umask cannot narrow it); a folder with any other mode, or one that is a symlink, is
refused with a message that says how to fix it (`chmod 1777 <folder>`, or remove the link). (2) `./dev ci slot status` and
`job-isolation.sh status` list every `sN.*.owner` record (one per uid), and a new holder clears only its own uid's release
record, never another account's.

**D. What Pitcrew declares and checks.** (Pitcrew ADR 0021 §1: `ci_isolation` is a field of `[[autoscale.pools]]`, one value
per space: `ci_isolation = { <space number> = "slots" | "none" }`, never keyed by a path string, written by Pitcrew once the owner confirms; a
pool with one space may write the plain string.) Every check runs on the composed environment of each runner and compares **canonical
resolved paths**, never `runner.envs` strings.
1. *Slots.* `PADDOCK_CI_SLOTS` = `PADDOCK_CI_KEEP_SLOTS` = the **sum of `runner.capacity` over every registered runner whose
   CI home resolves to the space**, all pools and loose runners alike, capped at `PADDOCK_CI_SLOTS_MAX`; a total above the cap
   is an error ("N jobs can run at once here, the kit isolates at most 4"). Pitcrew rewrites the values whenever a runner is
   added, removed or has its capacity changed and restarts the idle runners of the space after their jobs (ADR 0011); the
   check flags any runner whose value differs. A space whose total is 1 needs neither. `job-isolation.sh acquire` **fails fast** when `PADDOCK_CI_SLOTS` > `PADDOCK_CI_SLOTS_MAX` or `PADDOCK_CI_SPACE` is outside 1..`PADDOCK_CI_SPACES_MAX`, with a message naming the value and the limit (a conformance test covers both); a runner of a second account without `PADDOCK_CI_SPACE` is an error in Pitcrew's host check.
2. *Homes.* Two runners of one space with different `PADDOCK_CI_HOME` strings that resolve to one path are fine; one path
   resolved by two accounts is the error of A; a home on a volume with less than the space's budget (D5) warns.
3. *Space numbers.* `PADDOCK_CI_SPACE` equal for the runners of a space, distinct between spaces, written by Pitcrew.
4. *Linux host mode (P1-5).* `job-isolation.sh` treats every Linux machine as a container and does nothing for acquire and
   release. A pool with a Linux **host-mode** runner therefore gets no isolation: Pitcrew rejects `ci_isolation = "slots"` for
   it and caps its effective `max` at 1, until the kit can tell a container from a host (an explicit `PADDOCK_CI_CONTAINER=1`
   in the container image, tested).
5. *Job timeout.* A runner whose `runner.timeout` is longer than `RUNNER_JOB_TIMEOUT_SECS` is an error with a Fix (the holder's
   age limit is that value plus 30 minutes, above). The constant has one source: `tools/paddock-dev/src/slots.rs`; Pitcrew
   gets it from a generated `crates/paddock-slots` constant exported to Pitcrew, and a test asserts the two are equal (no
   second copy).
6. *Disk.* `PADDOCK_CI_SCCACHE_GB` and `PADDOCK_CI_MIN_FREE_GB` apply per space; the pool page shows the sum over a host's
   spaces and warns above the volume's free space (ER-24). A space starved by another's folders reports "the free disk is
   taken by <other space's home>, <size>" naming the folder, so the owner knows where to clean.

**E. Session loss and leftovers (P2).** A session runner stops when its session ends (Pitcrew drains it between jobs, ADR
0021 §4). A job that dies with the session leaves its holder alive (it is detached from the job's process group by design): the kernel releases the lock if the
holder dies, but a holder that survives because its job folder is never deleted (the runner is gone) waits for
`DEFAULT_MAX_HOLD_SECS`. To end it earlier, Pitcrew removes the *runner's own* job folders under its `workdir_parent` (never another runner's) **while that runner is stopped**, when no process uses them (the runner's own clean-up would have); the holder is detached from the job's process group, so it does not die with the session, and it exits once its job folder is gone. The pool page names each folder removed. A headless runner is unaffected by logon state.

**F. Closing the "Still open" items.**
1. *Concurrency with two or three runners on one host.* An acceptance test using **temporary CI homes only** (ER-14; never the
   user's `~/.paddock-ci`): three job folders under a fake runner (Pitcrew's `fake_runner` example) in one space, and a second
   test with **two spaces** (two temporary homes, both taking `s1`): all jobs get distinct slots in a space, the two spaces'
   ports differ (fails today, B), kept folders never overlap. Run on the CI hosts at the end; the real first runs stay a
   rollout step watched with `job-isolation.sh status`.
2. *Windows `runner.timeout`.* Closed by D5, which reads the file on every host.
3. *Linux, one job per host.* **The code and the ADR disagree today.** ADR 0021 records that d-gtr9 runs one runner with
   `runner.capacity: 4` (container jobs), while §8 says one job per Linux host is the rule because every container shares
   one sccache disk cache and sccache's docs say concurrent servers on one local cache race. The CI kit puts every
   container's sccache in `/sccache`; if `docker inspect` of the four containers shows the same host source for `/sccache`,
   the race holds **by construction** and d-gtr9 breaks §8 today. d-gtr9 stays as it is until the measurement. The
   measurement is the 4-concurrent-job runs of the Linux CI build-share A/B on d-gtr9 (`handoff/linux-ci-share-ab.md`,
   throw-away `ci-share-ab.yml`), after the Spotter and Pitwall tags, which also confirms the mount and its sizes. It compares
   three arms, **3 cold and 3 warm rounds each**, four jobs at once:
   - *A, shared:* today's single `/sccache` for all containers.
   - *B, per-slot folders:* `/sccache/s<n>`, a quarter of the budget each, slots from the same Rust lock on the mount (acquire
     and release stop being no-ops in a container).
   - *C, Redis:* one network cache server for the host's containers (sccache's Redis backend), the budget in the server.
   Per job and per round it records: `sccache --show-stats` at the job's end (cache errors, compilation failures, hits and
   misses), wall time, the cache folder's size against its cap, the load average and free memory (sampled every 10 s), plus the
   isolation evidence: the server and port each job sees, ports bound at the busiest step (`ss -ltnp`) and any "address in use",
   the Docker network name of each job, `RUNNER_TEMP`/`TMPDIR` and cache folders as each job sees them, and any "file exists",
   "ENOENT" or lock message the single-job baseline lacks; job result, duration and failed steps against that baseline, with
   start offsets of 0 and 30 s for the race windows. Rules: **arm A is rejected** on any cache error, any failed compilation
   attributable to the cache, or a cache folder more than 10% over its cap; between B and C, choose the arm with the higher warm
   hit rate and, on a tie within 5%, the lower median wall time. Clean runs alone **do not relax §8**: it is relaxed only if the pinned sccache documentation or source says concurrent local servers on one cache are safe (cited with version and date). Arm B is run by hand in the A/B (`SCCACHE_DIR=/sccache/s<matrix index>`, since I1b is not built), and the run records the mount's filesystem type (cache locking semantics differ on NFS, overlay and bind mounts). The results go in `apps/pitcrew/docs/performance.md` and the family
   `docs/benchmarks.md` (only like-for-like numbers are compared there). Package I1b implements the chosen arm.

**G. Implementation (package I1, after the review accepts this).** (1) `paddock-slots` and `job-isolation.sh`: the space term
in `port`, `PADDOCK_CI_CONTAINER`, the VM lock path of C, `status` printing the space (home, `k`) and its holders. (2)
Conformance: block overlap for spaces, the VM lock path, the test of F1 (including two spaces). (3) Pitcrew (ADR 0021 WP-5): the
checks of A and D, the writing of `PADDOCK_CI_SPACE`, `PADDOCK_CI_SLOTS`, `PADDOCK_CI_KEEP_SLOTS` and `PADDOCK_VM_MAX`, tests
first on composed environments with fake homes. (4) Linux per F3 (I1b), only after the measurement.

## Rollout

1. Merge `ci/job-isolation` (it already carries `fix/ci-home-relocatable`, `ci/concurrent-jobs` and
   `tools/paddock-dev-r0`; those branches are then deleted).
2. Per host, after the amendment's F items are cleared for it: Pitcrew writes `PADDOCK_CI_SPACE`, `PADDOCK_CI_SLOTS` and
   `PADDOCK_CI_KEEP_SLOTS` (at most 4) into every runner of each slot space (amendment B, D1); then add runners with Pitcrew
   (capacity 1 each). Never raise a runner's `capacity`.
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

## Still open (blocks adding runners; the amendment's F is the current list)

- Concurrency itself: every run so far had one runner per host. The first runs with two or three runners on a host
  (Pitcrew pool) are the real test; watch `job-isolation.sh status` and the release step.
- The Windows runner's `runner.timeout` (a job longer than it is killed; the holder then ends with the job folder).
- Linux stays one job per host until containers take slots (decision 8).
