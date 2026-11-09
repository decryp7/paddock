# 74. Declared hosts and machine leases: one inventory of every runner, a live check against it, a lease for shared test machines

- Status: Accepted 2026-11-09 by the user: D0–D7 as recommended (revision 3, `4dcb3afbd`). History: Proposed (2026-11-09; drafted at the user's request of 2026-11-09 to write the redesign ADRs 0071 to 0074 now.
  Revision 2 answers the independent review of revision 1 (`8080eb707`): P1-1 to P1-5 and its P2s. Revision 3 applies
  the re-review of revision 2 (`e954827e0`): the D2 evidence corrected to two measured pairs, and its P2s. Needs the user's
  decisions below before any build; WP-0, the interim Windows VM lock, is being done separately by the coordinator)
- Date: 2026-11-09
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-20 / rule 51 (parallelism and cost), ER-24 /
  rule 41 (scale parallel work to the machine), ER-14 / rule 7 (tests never touch the user's machine), ER-27 (logs
  explain every failure), ER-28 (a cause is confirmed only by a run without it), ER-32 (bounded waits), ER-33 / rule 71
  (a check counts only with a verified precondition and a positive control), ER-22 / rules 42 and 55 (share first, one
  implementation per mechanism), rule 48 (benchmarks recorded), rule 53 and ADR 0033 (processes by identity); ADR 0025 (CI
  job isolation: slots, kept folders, `ports.txt`; §8 one job per Linux host), ADR 0022 (`paddock-dev`), ADR 0040 and ADR
  0042 (the VM harness and its owner; ADR 0042's "lease" is the ownership of a harness's code, not of a machine); the
  redesign drafts of the same day: ADR 0071 (ready or exit, never a fixed start budget; `docs/adr-0071-ready-or-exit`, owns
  the macOS first-start check, its D5), ADR 0072 (every connection end says why, in order; `docs/adr-0072-ordered-close`,
  whose Windows VM stress loops need the lease), ADR 0073 (no test fakes, family-wide; `docs/adr-0073-no-fakes-family`,
  revision 2 at `bb9891c18`, Proposed); Pitcrew ADR 0021 (a pool of capacity-1 runners per host)
- Code it changes: `conformance/ci/build-jobs.sh`, `.gitea/actions/{build-jobs,job-isolation,setup-rust}`, every
  `.config/nextest.toml` (7), `crates/paddock-slots`, `tools/paddock-dev` (`./dev host`, `./dev machine`),
  `scripts/vm-win.sh` and `scripts/vm-smoke/*` (on `origin/tools/vm-smoke-defender`, which contains the approved V1
  `f857083c0`), `tools/macos-vm-smoke.sh`, `.gitea/workflows/service-smoke.yml`, `.claude/hooks/` (one new hook), the
  Pitcrew runner layout (`apps/pitcrew/crates/pitcrew-core/src/layout.rs`) and the add-runners runbook (on
  `docs/pitcrew-autoscale`)

## Pattern

**Shared machines have undeclared capacity and ownership.** Whenever a machine is shared (several CI runners on one
host, agents and CI on one Mac, several agents on one test VM, several containers on one Linux host), each consumer carries
its own private guess of the machine's size, limits, volumes and who else uses it. The guesses drift apart the moment the
machine changes, and the first sign is a red run or a broken VM whose cause looks like "load" or "flaky".

**Invariant.** Every CI runner and every shared test machine is declared once, in the repository: its cores, RAM, how
many jobs or users share it at once, the share kept for others (the agents included), its limits (open files), its
volumes and its owner. Every consumer reads its numbers from the declaration, a check whose probes have shown they can
see each fact proves at run time that the live machine still matches it, and a machine that only one user may drive at
a time is used only under a held lease. Tests never depend on the size or free space of the host they run on.

This ADR has three parts that can be decided and built independently:

- **Part A, the machine lease** (§1): the urgent part; it depends on nothing else here. The user may split it into its
  own ADR (decision D0).
- **Part B, the host inventory and its live check** (§2 to §4).
- **Part C, the host-independence rule and its checks** (§5).

## Context: the incidents

All on 2026-11-08/09 unless dated otherwise. "Verified" means read in the repository, a job log, the Gitea job list or
the hand-off log; "reported" means stated in a hand-off line that I could not check myself.

| # | Incident | Evidence | What was undeclared |
|---|---|---|---|
| I1 | Three macOS runners (`d7-mac-mini`, `-2`, `-3`) went live on one 14-core Mac at 01:33 +08, while `build-jobs.sh` still described macOS as "1 job per host, 10 cores kept", the build-jobs action and README said the same, and all seven `nextest.toml` CI profiles said "A CI job has its runner to itself" with `test-threads = "num-cpus"` (14 test threads per job, 42 at once). | Verified: `origin/main` (`b51c72509`) `conformance/ci/build-jobs.sh:28-31`, `class_default` (`macOS:cores` 10, share 1 through `*:share`); the seven `nextest.toml` files; runner hand-off `runners-2026-10-28.md` (2026-11-09 01:33). Job 32417 on `d7-mac-mini-2` logged `CARGO_BUILD_JOBS=4 ((14 cores - 10 kept) / 1 jobs per host ...)` while three runners ran. The Gitea job list (D2's data) shows three macOS jobs at once for 8.9 % of the Mac's busy time from 2026-11-01 to 2026-11-09, all after the runners went live. Fixed in instances on `fix/main-after-7761` `deb01e7ca` (12 files). | Jobs per host and the share kept for the agents, per host, in one place |
| I2 | The three runners of that one host disagree about the host: `-2` and `-3` got `PADDOCK_CI_JOBS_PER_HOST=3`, `PADDOCK_CI_RESERVED_CORES=2` and `NumberOfFiles=65536` in their launchd plists; the original `d7-mac-mini` (Pitcrew-managed) got none of them. (ADR 0025's rollout names only `PADDOCK_CI_SLOTS` for `runner.envs`; the README's "Adding runners" adds `PADDOCK_CI_JOBS_PER_HOST` there, not in a plist.) | Verified: hand-off lines 2026-11-09 06:51 and 06:53, and overnight log line 1471 ("Original d7-mac-mini unchanged"). | A per-host fact stored per runner, by hand, in two different mechanisms |
| I3 | New launchd runners inherited launchd's default soft limit of 256 open files; a Debrief DuckDB writer test failed with EMFILE. | Verified: job 32417 (`d7-mac-mini-2`) log lines 4608 and 5055, `IO Error: Cannot open file ".../segments/seg-...parquet": Too many open files`; `launchctl limit maxfiles` on this Mac prints `256 unlimited`, so the 256 limit of the job is inferred, not read from the job (no job logs `ulimit -n`; the live check adds it). Fixed by hand in two plists (I2). | The runner's open-files limit |
| I4 | `stable-workspace` hit its 300 s bound on `d7-mac-mini` because the runner's work folder was on the internal disk and the CI home on `/Volumes/d7sst7`, so `mv` copied the whole checkout (7 to 293 s; nine timeouts in runs 7706 to 7761). The same cause was already recorded on 2026-11-04 (run 7501) and left as "runner fix: with the user". | Verified: `c2179dba0` message (measurements), `docs/lessons.md` "The macOS runner's stable-workspace step copies the checkout across volumes (2026-11-04)"; runner moved at 03:35 +08 (hand-off). Guard added in `c2179dba0`. | Which volume each runner folder lives on, and the rule that work folder and CI home share one |
| I5 | A Dyno disk benchmark test needed 401 GB free: the default margin is 10 % of the volume, and the CI volume is 4 TB with 266 GB free. | Verified: `2c7c8891e` (on `fix/dyno-bench-disk-size`, `train/2026-11-08-1`): the test now passes an explicit `--free-margin-mib`. | A test that reads the host's volume size |
| I6 | Two agents used the Paddock Windows test VM (`paddock-win11-test`) at once: one loaded all 4 vCPUs from an SSH session and lost SSH, another powered the VM off. | Verified: overnight log line 1474 ("INCIDENT"), `9c738b1ad` lesson; the VM's `.vmx` declares `numvcpus = "4"`, `memsize = "8192"`. `scripts/vm-win.sh` (on `origin/tools/vm-smoke-defender`) has a load check but no lock. | Who holds the VM; how much of it a user may load |
| I7 | Up to four sccache servers can share one local disk cache on the Linux host: `d-gtr9` ran four container jobs at once for 67 % of its busy time, every Linux job starts its own sccache server on the local backend at `/sccache`, and sccache's `docs/Local.md` (line 7, the same at the pinned `v0.19.0` and at `main`): "The local storage only supports a single sccache server at a time. Multiple concurrent servers will race and cause spurious build failures" (https://github.com/mozilla/sccache/blob/v0.19.0/docs/Local.md). | Verified: the Gitea job list (D2); in their job logs two pairs of sccache servers ran at once, each on `sccache: local disk /sccache`: jobs 32663 and 32664 (both servers up from 23:44:47 to 23:45:32Z on 2026-11-08) and jobs 32665 and 32666 (23:47:36 to 23:48:32Z), from each server's start to the stats step that stops it. That `/sccache` is one host folder shared by the containers is ADR 0025 §8's statement and is indicated by the warm cache (job 32663, a fresh container, hit 1,312 of 1,320 compiles); it was not read on the host. No failure has been traced to it yet; one would look like a flaky Linux compile (ER-28). | The Linux host's jobs at once, against ADR 0025 §8's one job per Linux host |
| I8 | Dyno's `link::model` probe timed out at 60 s in run 7775; in the job logs it took 15 to 26 s whenever it overlapped `dyno-bench`'s all-core tests in the same job, about 7 s otherwise. Correlated; the cause is open and was not reproduced locally. | Verified: `docs/benchmarks.md` block "2026-11-09, CI share on the shared Mac" on `deb01e7ca`; overnight log line 1474. | Tests that may size their own load from the host's core count, not the job's share |
| I9 | Earlier, same class: a Spotter test expected `NotFound` under `temp_dir()` and got `Unreachable` because the CI Mac's temp folder sat under `/Volumes/` (2026-11-04); launchd runners on `/Volumes/d7sst7` failed with exit 78 (TCC, removable volumes) until their program moved to the internal disk (2026-11-04 and 2026-11-09 01:19). | Verified: `docs/lessons.md` (two 2026-11-04 entries); hand-off 2026-11-09 01:19. | The volume and its kind (internal or removable) of each runner folder |

I1, I2 and I4 were each found by a red main run; I4 was the second time; I6 broke a shared test machine. Each fix so far
corrected one consumer's copy of a fact about the machine.

**Same nights, other classes (not in scope).** `726c0f7ce` (at a job's release step the job's sccache server and temp
folder were already gone, so `./dev` could not build paddock-dev and the release blamed the slot's holder) is an ordering
bug inside `job-isolation`, not an undeclared host fact; nothing here would have prevented it. The Pitwall e2e stall in
macOS's first-start check with three runners on one Mac (overnight log line 1457) belongs to ADR 0071 (its D5 decides
the exemption); the inventory only records that the Mac runs three jobs at once.

## Instances: every consumer of a host fact today

Read on `origin/main` (`b51c72509`) and the branches named. "Broken" failed a run; "at risk" holds a private copy of a
fact that will drift; "fine" reads the machine or a declared list.

| Where | What it assumes or reads | State |
|---|---|---|
| `conformance/ci/build-jobs.sh` header and `class_default` | jobs per host, cores and RAM kept, per runner class (macOS 1/10/32 GB on main; 3/2/32 GB on `deb01e7ca`); Linux "capacity 4", Windows "two jobs at once" in comments only | broken (I1), still a private copy after `deb01e7ca` |
| runner `runner.envs` and launchd `EnvironmentVariables` (`PADDOCK_CI_JOBS_PER_HOST`, `PADDOCK_CI_RESERVED_CORES`, `PADDOCK_CI_SLOTS`, `PADDOCK_CI_KEEP_SLOTS`, `PADDOCK_CI_HOME`) | per-runner copies of per-host facts, set by hand | broken (I2) |
| launchd plists of `-2`, `-3`; the Pitcrew service that runs `d7-mac-mini` | open-files limit | broken (I3), fixed by hand on two of three |
| `.gitea/actions/build-jobs/action.yml` description, `.gitea/actions/README.md` (build-jobs row; "Adding runners") | the macOS layout in prose | broken (I1), fixed in `deb01e7ca` |
| seven `.config/nextest.toml` (`crates`, `tools/paddock-dev`, Debrief, Dyno, Pitcrew, Pitwall, Spotter) | "A CI job has its runner to itself", `num-cpus` | broken (I1); `deb01e7ca` overrides with `NEXTEST_TEST_THREADS` from build-jobs |
| rule 41 (agents on the Mac: about 5 heavy jobs, `CARGO_BUILD_JOBS=2` each) | the agents' share of the same Mac, in prose | at risk: CI's `reserved_cores = 2` and rule 41's 10 cores are two private copies |
| `.gitea/actions/stable-workspace` | work folder and CI home on one volume | broken (I4), guarded by `c2179dba0` (falls back to a cold build) |
| `conformance/ci/job-tmpdir.sh` | macOS `TMPDIR` on the internal disk when the work volume is external (reads `diskutil`) | fine (reads the machine) |
| `.gitea/actions/setup-rust` sccache size and backend | `PADDOCK_CI_SCCACHE_GB` (40) / `PADDOCK_CI_SLOTS` per runner env; in a container the local backend at `/sccache` for every job | at risk; broken in kind on Linux (I7) |
| `job-isolation.sh` free-disk floor, orphan age | `PADDOCK_CI_MIN_FREE_GB` (30), per runner env | at risk |
| ADR 0025 §8 vs `build-jobs.sh` | "one job per Linux host" vs "d-gtr9, capacity 4" | broken (I7): the job list shows four |
| `apps/pitcrew/crates/pitcrew-core/src/layout.rs` | writes a runner's `workdir_parent` under the runner's own folder | at risk (I4, I9: the folder's volume is not chosen by any rule) |
| add-runners runbook (`docs/pitcrew-autoscale`, not on main) | hand steps per host | at risk |
| `scripts/vm-win.sh` | one VM, `PADDOCK_VM_MAX_LOAD` 25 on the host; no lock | broken (I6) |
| `scripts/vm-smoke/run-win.mjs` | drives the Windows VM through `vm-win.sh`; no lock | broken (I6) |
| `scripts/vm-smoke/chain-release.sh`, `make-no-dyno-snapshot.sh` | call `vmrun` directly; `chain-release.sh` runs its steps under `plock` when plock is installed and directly without it; `make-no-dyno-snapshot.sh` asks for plock in a comment | at risk: a lock outside the repository, optional |
| `scripts/vm-smoke/run.mjs`, `tools/macos-vm-smoke.sh`, `service-smoke.yml` | Tart VMs under a host-wide slot (`./dev ci slot` under `TART_HOME`, `PADDOCK_VM_MAX` 1, at most 2) | fine: the model for WP-0 and the lease |
| `scripts/vm-smoke/run-linux.mjs`, `login-probe-linux.sh`, `login-probe-mac.sh` | `tart clone` and `tart run` without the slot | at risk |
| an agent's ad hoc `vmrun`, `ssh` into the guest or `tart` command | nothing | broken (I6 was this shape) |
| test code reading host size or free space directly | Rust `tests/` files: 4 match the probe (`available_parallelism`, `statvfs`, `free_space`, ...); all 4 go through a seam (`Downloader::with_free_space`) or are a name match, so 0 direct reads. 36 non-test Rust files read host size or free space (Dyno 14, Debrief 14, shared crates 6, Pitwall 1, Pitcrew 1; a wider probe set finds 47); the tests that reach them are not enumerated | broken once (I5), at risk |
| tests sizing their own load from the host (`dyno-bench` all-core kernels in a CI job) | every core of the host, not the job's share | at risk (I8) |

## Part A. The machine lease

### 1. A lease for every shared test machine, independent of the inventory

**WP-0 (interim, being done separately by the coordinator, not part of this ADR's build).** `vm-win.sh`'s mutating
commands, `chain-release.sh` and `make-no-dyno-snapshot.sh` take the existing `./dev ci slot` (one slot, one fixed root),
the pattern the Tart scripts already use. It is `origin/fix/vm-smoke-take-vm-lock` (`22e048c7c`: `vm-lock.sh`, a ci slot,
taken by those three scripts). It stops I6 for the scripts on that branch now; the lease below replaces it.

**Branches that still carry unlocked VM scripts** (read 2026-11-09; each has Windows VM scripts that call `vm-win.sh` or
`vmrun` without WP-0's lock, some with the old optional plock line): `origin/feat/dev-vm-broker`,
`origin/tools/win-test-vm`, `origin/tools/win-update-test`, and the Dyno test branches `origin/test/dyno-release-vm-checks-v2`,
`test/dyno-service-model` (and `-2`, `-3`), `test/dyno-tray-icons`, `test/dyno-vm-harness-1.1` (and `-linux`, `-mac`,
`-r2`, `-win`), `test/dyno-vm-reset-and-seed`, `test/dyno-widget-lists-race`, `test/dyno-win-screens`,
`test/dyno-wire-roundtrip`. Running a VM script from any of them bypasses WP-0; they take it when they merge main, and the
hook below covers the meantime.

**The machines list.** Part A has its own small declared list, `conformance/ci/machines.toml`, so it does not wait for
the host inventory (Part B later reads it rather than repeating it):

```toml
schema = 1
lease_root = "<one fixed host-local folder>"   # the root WP-0 uses; never derived from TART_HOME or TMPDIR

[machines.paddock-win11-test]       # the Windows 11 Arm test VM (VMware Fusion) on the developer Mac
host = "mac-mini"
kind = "vmware-fusion"
vcpus = 4
ram_mb = 8192
users_at_once = 1                   # exclusive
max_load = 3                        # busy processes a user may start in it: vcpus - 1 (lesson 2026-11-09)
idle_expiry = "30m"                 # no activity for this long ends the lease (D7)
clean_state = "snapshot:gui-baseline"   # what the next holder restores after a lease that did not end by release

[machines.tart]                     # Tart clones for the macOS and Linux VM checks, local and service-smoke in CI
host = "mac-mini"
kind = "tart-pool"
users_at_once = 1                   # PADDOCK_VM_MAX today; macOS allows at most 2 macOS guests
vcpus = 4
ram_mb = 8192
idle_expiry = "30m"
clean_state = "fresh-clone"         # every user clones; a leftover clone with the old lease's prefix is deleted
```

**The lease.** `./dev machine lease`, built on `paddock-slots` (ER-22: the OS file lock the CI slots and the Tart VM slot
already use; the kernel frees it when its holder dies, so nothing is reclaimed by hand):

- `acquire <machine> --purpose "<brief or branch>" [--owner-pid <pid>] [--priority release] [--wait <dur>]` starts a
  detached holder that takes `flock`/`LockFileEx` on `<lease_root>/<machine>-s<n>.lock` (`n` up to `users_at_once`) and
  prints a **fencing token**: the lease id plus a generation number that grows with every new lease of the machine.
- **Liveness is activity, not a wall clock.** The holder keeps the lease while it is active: every `lease check` (each
  driver call), every `lease renew`, and, under `lease run <machine> -- <command>`, the command's process being alive
  (checked by identity every second). After `idle_expiry` with none of these, the lease ends and the generation moves on.
  There is no maximum for an active lease, so a long run is never cut off under its holder.
- **Fencing.** Every driver asks `lease check <machine> --token` before acting and refuses a token whose generation is not
  the current one: a holder whose lease expired cannot touch the machine any more, even if it still runs. An in-flight
  `lease run` cannot expire (its child is the activity); an SSH session opened through `vm-win.sh ssh` runs under the
  driver, which renews while the session lives. A command that a script started outside `lease run` and that outlives
  the lease keeps running in the guest; the next holder's restore (below) ends it.
- **The owner (rule 53).** `--owner-pid` names a process by pid plus start time (`paddock_proc::ProcessId`); the holder
  lets the lease go when that process ends or the pid names a process with another start time, and accepts an owner only
  if it started before the holder (a parent is never younger than its child). There is no default owner until it is
  measured what the "parent" of an agent's command is in the harness (WP-A0): each Bash call of an agent is a fresh shell
  that ends with the call, and subagents may share one session process, so neither is a usable default. Until then a
  lease without `--owner-pid` lives by activity alone.
- **The next holder checks the state.** A lease that ended by `release` leaves the machine `clean`. One that ended by
  expiry, owner death or `break` leaves it `dirty` in the lease record; the next `acquire` reports it, and the driver
  accepts only `revert` to `clean_state` (or, for Tart, deleting leftover clones of the old lease's prefix) or a read-only
  `verify` (the VM's power state, its current snapshot, the guest's processes) until one of them has run.
- **Priority and waiting.** `flock` has no queue or fairness, so the lease adds one: a ticket file per waiter in
  `<lease_root>/<machine>.queue/` (`<priority>-<sequence>-<pid>-<start time>`, created atomically; a ticket whose owner is
  gone by identity is removed). A waiter tries the lock only when its ticket is first in order, release tickets before
  all others (ER-24: tests never crowd out a release). A held lease is never pre-empted; its holder sees "a release run is waiting" on its next check.
  Every wait is bounded (`--wait`, ER-32) and its message names the holder's purpose and age.
- **Status and break.** `status` lists holders, purpose, age, last activity and state. `break <machine> --reason` ends
  the holder through `paddock_proc` by identity and logs the reason and the broken lease's purpose. Any process of the
  same account can run it: "the coordinator only" is a convention, not a control.
- **Scope.** A lease is host-local: the root is a local folder of the machine that runs the VM. It is never on a VM's
  shared folder or a network file system, where `flock` does not hold across machines.
- **The owner record** beside the lock holds the owner's identity, the purpose, the start, the last activity, the
  generation and the state; never a secret or a user's data.

**Every user takes it, and the drivers refuse without it.**

- `scripts/vm-win.sh` runs a mutating subcommand (`start`, `stop`, `ssh`, `put`, `get`, `job`, `snapshot`, `revert`,
  `gui-setup`, `install`, `baseline`) only with a `PADDOCK_LEASE_TOKEN` that `lease check paddock-win11-test` accepts;
  `status` and `ip` stay free. Child scripts inherit the token.
- The Tart scripts and `service-smoke.yml` move from `./dev ci slot --root $TART_HOME/.paddock-vm` to the `tart` lease,
  so a CI job and a developer's run share one lease.
- `chain-release.sh` and `make-no-dyno-snapshot.sh` stop calling `vmrun` directly, or call `lease check` first.
- **A Claude Code `PreToolUse` hook** (`.claude/hooks/vm-lease.sh`, tested by its `.test.sh`, the shape of
  `no-pgrep-wait.sh`) refuses a Bash command that runs `vmrun`, `tart run|stop|exec|delete`, or `ssh`/`scp` as the guest accounts
  (`paddock-test@`, `paddock-admin@`) or naming the VM (`paddock-win11-test`, its `.vmx`), without `PADDOCK_LEASE_TOKEN` in its environment, and says which lease to take. It covers agents that
  bypass the scripts, which is how I6 happened.
- Scripts that load a guest on purpose read `max_load` from the machines list.

**Checked.** A conformance test enumerates every file that invokes a VM driver (`vmrun`, `tart`, `vm-win.sh`) and fails
when it neither is a driver that checks the token nor takes the lease; a fixture script without the lease must fail it.
WP-A1's tests use real processes: two acquirers, owner killed, pid reused with another start time, an owner younger than
the holder refused, idle expiry, renew and check as heartbeats, an expired token refused by a fake driver, a `run` longer
than `idle_expiry` kept alive by its child, `dirty` after expiry and `clean` after release, release priority, and a logged
break.

## Part B. The host inventory and its live check

### 2. The inventory: `conformance/ci/hosts.toml`

One TOML file beside `kept-folders.txt`, `ports.txt` and `machines.toml`, parsed once by `paddock-dev` (parse, don't
validate: typed records, unknown keys refused). It declares hosts, their volumes and the CI runners on them. A sketch
with today's facts (each fact carries the date and source it was read from):

```toml
schema = 1
pending = ["d7-mac-mini", "d7-mac-mini-2", "d7-mac-mini-3", "d-gtr9", "decryp7ology-dev"]   # may only shrink (§3)

[hosts.mac-mini]                    # the developer Mac: agents, three macOS runners, the test VMs
os = "macos"
arch = "arm64"
cores = 14                          # 10 performance + 4 efficiency (sysctl, 2026-11-09)
ram_mb = 65536
owner = "user"                      # who may change the machine; "coordinator" may change only the shares below
ci_jobs = 3                         # runners x capacity on this host
reserved_cores = 2                  # the system, besides the agents' share
reserved_ram_mb = 32768
agents = { cores = 10, heavy_jobs = 5, build_jobs_each = 2 }   # rule 41's numbers, now declared here
cpu_oversubscription = 1.75         # allowed busy threads / cores; see below and D1
test_threads = 8                    # per CI job: coordinator decision 2026-11-09 (overnight log 1474); the pushed
                                    # fix/main-after-7761 still computes 4 = (14 - 2) / 3
open_files_min = 65536              # every CI job's soft limit (I3)
verified = "2026-11-09 sysctl, launchctl print"

[hosts.mac-mini.volumes]
system = { mount = "/", kind = "internal", warn_free_gb = 100, floor_free_gb = 40 }
d7 = { mount = "/Volumes/d7sst7", kind = "external", warn_free_gb = 150, floor_free_gb = 60, holds = ["ci-home", "runner-work", "worktrees", "vms"] }
d8 = { mount = "/Volumes/d8sst7", kind = "external", warn_free_gb = 150, floor_free_gb = 60, holds = ["main-checkout"] }

[runners.d7-mac-mini]
host = "mac-mini"
capacity = 1
labels = ["macos-latest:host", "macos-arm64:host"]
program_volume = "system"           # launchd cannot start a program on a removable volume (TCC, I9)
work_volume = "d7"                  # must hold the CI home (I4)
service = "pitcrew"                 # pitcrew | launchd | systemd | windows-service
version = "v5.4.1"

[runners.d7-mac-mini-2]
host = "mac-mini"
capacity = 1
labels = ["macos-latest:host", "macos-arm64:host"]
program_volume = "system"
work_volume = "d7"
service = "launchd"
version = "v5.2.3"

# [runners.d7-mac-mini-3] as -2. [hosts.d-gtr9]: Linux, 16 cores, 128 GB, containers, ci_jobs = 4 (Gitea job list,
# 2026-11-01..09, D2). [hosts.decryp7ology-dev]: Windows, 8 cores, 16 GB, ci_jobs = 1 (the job list: two at once for
# 0.1 % of its busy time); sizes from build-jobs.sh's comments, marked verified = "2026-10-30 job log" until the live
# check has read them.
```

Rules of the file, checked by a conformance test (`hosts`, new) that fails on today's state until migrated:

- **The arithmetic is defined, not hoped for.** With `os` = `cpu_oversubscription` (1 when not declared): build share per
  job = max(1, floor((cores × os − agents.cores − reserved_cores) / ci_jobs)), and the check is that the peak,
  `ci_jobs` × build share + `agents.cores` + `reserved_cores`, is at most `cores` × `os` (the max(1, ...) may exceed it
  only with a warning). Test threads may exceed the build share for wait-heavy suites, bounded by the same `cores` × `os`
  minus the agents' and reserved cores. On the Mac today: os = 1.75 gives floor((24.5 − 10 − 2) / 3) = floor(4.17) = 4
  build jobs, a peak of 3 × 4 + 10 + 2 = 24 ≤ 24.5. The measured slowdown at 22 to 23 busy threads, and its limits, are in
  "Parallelism and cost" (decision D1).
- `ci_jobs` equals the sum of its runners' `capacity`; every volume a runner or machine names exists on its host; a
  runner's `work_volume` holds `ci-home` (I4); its `program_volume` is `internal` on macOS (I9).
- **No secrets or personal data:** no addresses, tokens, user or account names, serial numbers; mounts and labels only
  (the test greps the file for IP-address, token and `/Users/<name>` shapes and fails on any). This guards this file
  only: `main` already carries LAN addresses in CI files and a user path in ADR 0025, which is outside this ADR.
- Every fact has a date and source; facts older than 30 days are reported (ER-30 style), not failed.

`paddock-dev` also writes a flat, generated `conformance/ci/hosts.env` (one `KEY=value` line per runner and host fact),
checked current by the same test like any generated file, because `build-jobs.sh` runs before the job has built
paddock-dev and must stay bash 3.2 and dependency-free.

### 3. Every consumer reads the inventory

- **`build-jobs.sh`** finds its runner in `hosts.env` and computes `CARGO_BUILD_JOBS` and `NEXTEST_TEST_THREADS` from the
  host's record. Its runner name comes from the runner itself if the runner exposes it as an environment variable
  (`RUNNER_NAME` or similar; not seen in the job logs read so far, WP-B2 checks it first), else from one runner env,
  `PADDOCK_RUNNER`, set by the setup below. The per-class defaults (`class_default`) and the per-runner
  `PADDOCK_CI_JOBS_PER_HOST` and `PADDOCK_CI_RESERVED_*` envs go. **Migration is a ratchet, not a date:** `hosts.toml`
  lists `pending`, the runners still allowed the old class defaults (with a warning). The list may only shrink (the
  `hosts` test compares it with `origin/main`), and an unknown runner not on it is an `::error::` that names the file to
  edit. `PADDOCK_CI_CORES`, `PADDOCK_CI_RAM_MB` and `RUNNER_OS` stay as test seams only.
- **nextest:** the seven CI profiles keep `test-threads = "num-cpus"` only as the value for a host a job has to itself,
  and say that `NEXTEST_TEST_THREADS` from the inventory replaces it; a conformance test fails on the old sentence
  ("A CI job has its runner to itself") and when a workflow sets `NEXTEST_TEST_THREADS` outside build-jobs.
- **job-isolation, setup-rust, prune:** `PADDOCK_CI_SLOTS`, `PADDOCK_CI_KEEP_SLOTS`, the sccache budget and the free-disk
  floors come from the host's record, not from runner envs.
- **stable-workspace** keeps `c2179dba0`'s volume guard as a belt; the live check catches the cause first.
- **Runner setup:** `./dev host runner-config <runner>` renders what a runner needs from its record: the `runner.envs`
  keys this kit owns (`PADDOCK_RUNNER` if needed, `PADDOCK_CI_HOME`), `host.workdir_parent` on the declared volume, and
  the service limits (launchd `SoftResourceLimits`/`HardResourceLimits` `NumberOfFiles`, systemd `LimitNOFILE`; Windows has
  no per-process open-files limit to set). `--check` reads the runner's live `config.yaml` and plist and prints, per key
  this kit owns, "matches" or "differs" with the declared value; it **never prints the value of any `runner.envs` key**
  (the runner's own secrets live there, and this repository's job logs are public), only key names. CI jobs and
  `paddock-dev` never write runner configuration; the host's owner applies it by hand or through Pitcrew, whose
  `layout.rs` takes the volume from the rendering. WP-B5's tests read fixture plists and `config.yaml` files only (ER-14).

### 4. The live check: each probe shows it can see its fact, then compares

A step in `job-isolation acquire`, after its sweep and prune (the first point at which paddock-dev exists), runs
`./dev host check`. For each fact it first runs a **live positive control** on the probe itself (ER-33, rule 71), then
reads the value and compares it with the declaration:

| Fact | Probe | Live control (must pass, else `check broken` for that fact) | Mismatch |
|---|---|---|---|
| cores | `sysctl hw.ncpu` / `nproc` (cgroup-aware) / `NUMBER_OF_PROCESSORS` | a second source, Rust `available_parallelism`, gives the same count | error |
| RAM | `sysctl hw.memsize` / cgroup `memory.max` / `/proc/meminfo` / `GlobalMemoryStatusEx` | a second source (sysinfo) agrees within 1 % | error (outside 5 %) |
| open files | the job's soft `ulimit -n` | a child started with `ulimit -Sn 123` reads back 123 through the same probe | error when below `open_files_min`; Windows: `not measured`, said so |
| volumes | device id of the work folder, the CI home and `TMPDIR`; `diskutil info -plist` internal or not | a file the check creates in the work folder has the work folder's device id | error when the work folder and CI home differ or a folder is not on its declared volume |
| free space | `statvfs` on each declared volume of the runner | a second reading (`df -k`) agrees within 1 % | warning below `warn_free_gb`, error below `floor_free_gb` |
| jobs at once | busy slots from `./dev ci slot status` | the job's own slot shows as busy | error when more than `ci_jobs` are busy; in a Linux container: `not measured` (no slots, and a container cannot see its siblings; D2) |
| runner version | the version the runner prints at job start, if exposed as an env | none possible | warning only; `not measured` when not exposed |

Which controls prove visibility: the open-files child, the job's own slot and the created file show that the probe sees
the live thing. The second sources for cores, RAM and free space read the same kernel value on macOS (`hw.ncpu` and
`available_parallelism`, `hw.memsize` and sysinfo, `statvfs` and `df -k`): they catch a parsing error, not blindness, and
count as weaker controls (said so in the output). A Linux container reports cores, RAM, open files and free space for its own container, and `not measured` for jobs at
once and for the host volumes it cannot see. The raw probe values go to the log (ER-26) and a one-line summary to the
job summary. A mismatch prints one `::error::` per fact with the declared and live value and the next step ("edit
`conformance/ci/hosts.toml`" or "set NumberOfFiles in the runner's plist: `./dev host runner-config <runner>`"). Rollout:
warning mode until each host has had a clean week, then failing (D3). Cost: a few syscalls and one child process (WP-B4
records it, rule 48).

The free-space check runs after job-isolation's sweep, so a floor the sweep can restore is never a failure, and the
error level is a hard floor well under the warning level: d7 had 165 GiB free on 2026-11-09 and was at 96 % on
2026-11-08 (overnight log), so a single floor near today's free space would turn release jobs red because of agents'
worktrees.

## Part C. Tests never depend on the host

### 5. The rule and its checks

Proposed rule (number assigned on acceptance): **"A test never depends on the size, limits or layout of the machine it
runs on.** It never reads the host's cores, RAM, free space, volume size or open-files limit to decide what it expects
or how much it loads; it injects them through a seam (as `Downloader::with_free_space` does) or states them (an explicit
margin, an explicit thread count). A test that needs many open files says so in its test group (the CI guarantees
`open_files_min`, checked live). A test that must load the machine takes the job's share (`NEXTEST_TEST_THREADS`), never
every core. *Why:* on 2026-11-08 a 16 MiB disk test needed 401 GB free on a 4 TB CI drive, and a CPU-bound test took 2
to 4 times longer whenever all-core benchmark tests ran in the same job (correlated, cause open)."

Checked twice, each with a positive control:

- **Static (conformance `host-size`, new):** every direct read of host size or limits in test code (Rust `tests/` and
  `#[cfg(test)]` modules, `*.test.mjs`, `*.spec.ts`; probes: `available_parallelism`, `num_cpus::get`, sysinfo memory,
  `statvfs`/`statfs`/`available_space`/`GetDiskFreeSpace*`, `os.cpus`, `os.totalmem`, `os.freemem`,
  `availableParallelism`, `statfsSync`, `hardwareConcurrency`, `getrlimit`) is either through a seam or carries
  `host-size: <why>` on the line before. A fixture with a bare read must fail the check. `temp_dir()` is not part of the
  static check: 50 Rust test files call it, and "used to classify a path" cannot be decided by a scan; the small-host
  leg's tmpfs temp folder covers that case.
- **Dynamic (a weekly `small-host` leg, after a spike):** the Rust suites of `crates/` and the apps' Rust workspaces run
  on the Linux host in a container pinned to 2 CPUs with `--cpuset-cpus` (a `--cpus` quota leaves `nproc` at 16, so
  build-jobs would not see it), 6 GB of memory, `ulimit -n` at the declared `open_files_min` (the leg must not contradict
  the declaration it tests), and `TMPDIR` on a 2 GiB tmpfs. Its positive controls: `nproc` and `available_parallelism`
  both read 2 inside the container, and a fixture test that sizes a file from the host's free space fails. Its own nextest
  profile scales `terminate-after` by the core ratio and reports a timeout there as "slow on a small host", apart from a
  host-sizing failure. Not feasible as written until **WP-C0** proves that the Gitea runner accepts these
  `container.options` (no workflow uses them today) without the Docker socket. It does not catch a volume-ratio bug like
  I5 (that needs a large volume with little free space), which is why the static check exists.

## Parallelism and cost

- **More parallelism.** The inventory makes the CI share explicit instead of guessed. On the Mac today: three CI jobs
  with 4 rustc each, up to 10 agent threads (rule 41) and the system add up to 22 to 23 busy threads on 14 cores. The one
  measurement (`docs/benchmarks.md`, "CI share on the shared Mac", on `deb01e7ca`) is single runs: a CPU-bound probe took
  2.6 to 4.1 s alone and 4.0 s beside 23 busy threads, so the loaded value lies inside the unloaded range and "1.5 times"
  is an indication, not a result; wait-heavy test runs took 2.3 times longer at 4 test threads than at 14, while 8
  matched 14. The block has no budget, verdict or memory measure (rule 48 gaps, to be filled from WP-B4's first week of
  job durations). On the Linux host, four jobs at once for 67 % of its busy time (D2) is the parallelism the job list
  shows; keeping it needs a sccache cache per job (D2). VMs: one Windows VM and one Tart clone at a time stay the limit
  (4 vCPUs and 8 GB each; the Windows VM loses SSH at full vCPU load).
- **Cheaper.** The live check is a few syscalls and one child process per job (WP-B4 measures it). The lease adds no time
  when uncontended; when contended it turns a broken VM (I6: a lost hour and a re-setup) into a bounded, visible wait.
  The generated `hosts.env` keeps build-jobs at shell speed before paddock-dev exists.
- **The small-host leg costs real runner time:** the same CPU-seconds as the Rust suites' Linux run on 2 cores instead of
  16, so it holds one Linux job slot several times longer than the unconstrained run (not measured; WP-C0 measures one
  suite). Hence weekly and in addition, not instead of the nightly unconstrained run (ER-8, D4).

## Alternatives

- **Keep per-runner env vars, document them better (status quo after `deb01e7ca`).** Rejected: I2 shows three runners of
  one host already hold three different stories, in two mechanisms, after one night. A reminder is not a check.
- **Discover everything at run time, declare nothing.** Probing cores and RAM is easy; what cannot be probed is intent:
  how many runners should be on the host, what share the agents keep, which volume a folder belongs on, who owns the
  machine. Without a declaration there is nothing to compare against, and an extra runner silently changes everyone's
  share. Rejected; probing is the check, not the source.
- **Pitcrew as the source of truth.** Pitcrew manages runners and could hold their settings. Rejected for now: the CI
  kit and the VM scripts must read the facts without a running app, the test VMs are not runners, and a change of share
  should be reviewed in the repository. Pitcrew reads the rendering (§3) instead; it can own the inventory later.
- **plock as the VM lock** (what `chain-release.sh` uses when present). Rejected: plock is a build queue outside the
  repository, it is optional in the one script that uses it, nothing refuses a driver call without it, and its stale
  handling is not identity-based. WP-0 and the Tart slot show the in-repo mechanism.
- **A lease with a wall-clock maximum** (revision 1). Rejected after review: it ends a live holder's lease while it still
  drives the VM and lets the next holder in, which is I6 again. Activity, fencing and a state check replace it.
- **A lock file with a pid inside.** Rejected for the reason ADR 0025 gives: a reclaim rule races, and a bare pid is a
  reusable name (rule 53).

## Consequences

- One edit adds or resizes a runner; build-jobs, nextest, slots, the sccache budget, the free-disk floors, the live check
  and the runner rendering all follow. A runner that is neither declared nor pending cannot take a job.
- Changing a machine without changing the file fails the next job on it with a message that names the line to change;
  this is the point, and it costs one red job per drift (warning mode first).
- Agents must take the lease before touching a test VM, and the hook refuses an ad hoc VM command without it. A forgotten
  lease holds a VM at most `idle_expiry` after its last activity, and leaves it `dirty` for the next holder to restore.
- The inventory names hosts, mounts and runner names that are already in the repository (`build-jobs.sh`, ADR 0025,
  `docs/lessons.md`); it adds no addresses, accounts or secrets, and its test refuses them.

## Migration

The three parts migrate independently.

- **Part A:** WP-0 (separately, now). Then WP-A0 (what an owner is in the harness), WP-A1 (the lease), WP-A2 (every VM
  script, the drivers' token check, the conformance test, the hook, the brief template line); WP-0's slot root becomes the
  lease root and the plock lines go. The brief template line: "name the machine; take its lease; hold it with `lease run`
  or `lease renew` across long local work between VM steps (an installer build longer than `idle_expiry` otherwise loses
  the lease and the next holder reverts the VM)".
- **Part B:** (1) land `deb01e7ca` as it is, the interim fix of I1; (2) `hosts.toml` with today's facts and its test
  (WP-B1); (3) build-jobs reads `hosts.env` (WP-B2) with every runner on the `pending` list, which shrinks as each runner
  gets its name (set when idle, one at a time, through `./dev host runner-config`); (4) the live check in warning mode
  (WP-B4), failing per host after a clean week; (5) remove `class_default` and the `PADDOCK_CI_*` share envs from runner
  envs and plists when `pending` is empty (WP-B6). D2's sccache change (WP-B3) lands as soon as D2 is decided.
- **Part C:** the rule and the static check (WP-C1), the instances it finds fixed through seams, one commit per crate with
  its failing test first (rule 5); the small-host leg only after WP-C0.

**Rollback.** Each step is one revert: build-jobs keeps its class defaults while runners are pending; the live check has
a warning mode; the lease check in the drivers is one function, and WP-0's slot stays usable meanwhile. Nothing deletes
runner state or VM files.

## Work packages

| WP | What | Depends on | Estimate (agent-days) |
|---|---|---|---|
| WP-0 | Interim Windows VM lock: `vm-win.sh` mutating commands, `chain-release.sh`, `make-no-dyno-snapshot.sh` take `./dev ci slot` under one fixed root | – | done separately (coordinator) |
| WP-A0 | Spike: what process an agent's command, a subagent's command and a CI step can name as owner (pid, start time, lifetime), recorded; choose the default owner or none | – | 0.5 |
| WP-A1 | `machines.toml`; `paddock-slots` owner-identity `HoldProbe` with the age check; `./dev machine lease acquire/run/check/renew/status/release/break` with activity expiry, fencing generations, `dirty`/`clean` state, release priority; the real-process tests in §1 | WP-A0 | 3.5–5 |
| WP-A2 | Drivers refuse without a current token; `dirty` allows only `revert`/`verify`; every VM script and `service-smoke.yml` on the lease; conformance test with a failing fixture; `PreToolUse` hook and its test; brief template line | WP-A1, V1 landed | 2.5–3.5 |
| WP-B1 | `hosts.toml` (schema 1, facts with sources, agents' share, `pending`), `./dev host` parser, generated `hosts.env`, conformance test `hosts` (arithmetic, volumes, no-secrets grep, ratchet on `pending`, generated file current) | – | 1.5–2.5 |
| WP-B2 | Runner name from the runner if exposed; build-jobs reads `hosts.env`; slots, sccache budget and free floors from the record; nextest comments and their conformance test; tests in `ci-actions.test.mjs` | WP-B1, `deb01e7ca` | 1.5–2 |
| WP-B3 | D2: per-job sccache cache on the Linux host (slots on the `/sccache` mount, ADR 0025 §8's follow-up) or the chosen alternative | D2 | 1–1.5 |
| WP-B4 | `./dev host check` with a live control per probe on macOS, Windows, Linux host and container; `not measured` where blind; after the sweep; warning mode; job summary; its cost and a week of job durations recorded (rule 48) | WP-B1 | 3.5–5.5 |
| WP-B5 | `./dev host runner-config <runner> [--check]` (key names only, never values); Pitcrew `layout.rs` takes the volume from it; land the runbook with the inventory steps | WP-B1 | 1.5–2.5 |
| WP-B6 | Remove the old knobs from scripts, runner envs and plists (host by host, idle); README, ADR 0025 rollout note, lessons entry, AGENTS.md pointer, wiki notes | WP-B2, WP-B4, WP-B5 | 1–1.5 |
| WP-C0 | Spike: the Gitea runner with `container.options` `--cpuset-cpus`, `--memory`, `--ulimit nofile`, `--tmpfs`; one suite's duration at 2 cores | – | 0.5 |
| WP-C1 | The rule; static check `host-size` with fixtures | – | 1.5–2 |
| WP-C2 | The weekly `small-host` leg, its nextest profile and positive controls | WP-C0 | 1.5–2.5 |
| | **Total (without WP-0)** | | **20–29.5** |

Not estimated: fixing the instances WP-C1 and WP-C2 find (known only after their first runs). Paths: Part A is
WP-A0 → WP-A1 → WP-A2, 6.5 to 9 days, and depends on nothing in Part B; Part B's longest chain is WP-B1 → WP-B4 → WP-B6,
6 to 9.5 days; Part C runs beside both. With WP-0 in place nothing here blocks an urgent fix, so the user can order the
three parts (D0).

## Decisions for the user

- **D0. One ADR or two?** Part A (the lease) stands alone. Recommendation: **keep it in this ADR as Part A, but let it be
  accepted and built on its own, first;** split it into its own ADR only if you want to decide Part B or C later.
- **D1. Keep three macOS runners on the shared Mac?** Recommendation: **yes, for now, declared as `ci_jobs = 3`,
  `reserved_cores = 2`, `agents.cores = 10`, `test_threads = 8` and `cpu_oversubscription = 1.75`, and decide again after
  one week of queue plus run times per macOS job** from the live check's logs, against the week before. For: the macOS
  runner was the main CI bottleneck (overnight log 01:0x), and three at once ran for only 8.9 % of the Mac's busy time
  so far. Against: the evidence that three are cheap is weak (single local runs, see "Parallelism and cost"; the 60 s
  timeout of run 7775 happened in CI and is not explained), the Mac is also the agents' machine, and the first-start
  check queue is shared (ADR 0071's D5). Alternative: two runners with 6 build jobs each, if the week shows longer total
  times.
- **D2. The Linux host runs four jobs at once on one sccache cache.** From the Gitea job list (300 runs, 2026-11-01 to
  2026-11-09, 3,155 jobs on `d-gtr9`): one job at once for 14.6 % of its busy time, two for 10.4 %, three for 7.7 %,
  **four for 66.7 %** (five for 0.5 %, probably the time stamps of cancelled jobs; not checked). Every Linux job runs its
  own sccache server on the local backend at `/sccache`; two pairs of servers ran at once in the four job logs read (32663
  with 32664, 23:44:47 to 23:45:32Z; 32665 with 32666, 23:47:36 to 23:48:32Z; I7), against ADR 0025 §8 and sccache's `docs/Local.md` (line 7, the same at the pinned `v0.19.0` and at `main`): "The local storage only supports a single sccache server at a time. Multiple concurrent servers will race and cause spurious build failures" (https://github.com/mozilla/sccache/blob/v0.19.0/docs/Local.md). Options: (a) a cache per job on the mount, taken through the same
  slot lock on `/sccache` (ADR 0025 §8's planned follow-up; all containers see one kernel and one file there), (b) the
  Redis backend `setup-rust` already supports (`sccache-backend: auto` with an endpoint; whether a Redis server is live
  for it is not verified), (c) capacity 1, which would queue the 67 % of busy time that runs four at once. Recommendation:
  **(a) soon** (no new service, keeps the parallelism, about a day; its cost: one cache, 99 % warm today, becomes up to four
  colder per-slot caches sharing one size budget, so hit rates drop until each slot is warm; until it lands, a Linux
  compile error that does not reproduce locally is checked for an overlapping sccache server first), (b) later if a cache shared across hosts is wanted,
  not (c). The mount and capacity are read from `d-gtr9`'s runner configuration by its owner, not by the live check,
  which cannot see sibling containers.
- **D3. Mismatch: fail or warn?** Recommendation: warn until a host has had a clean week, then fail. The fix is one line
  or one plist key, and the message names it.
- **D4. The small-host leg.** Recommendation: **in addition, weekly, after the WP-C0 spike**, keeping the unconstrained
  nightly Linux run; it costs real runner time.
- **D5. Where the VMs live.** Your drive layout of 2026-11-06 puts test machines on d8; the Windows VM
  (`/Volumes/d7sst7/vms/paddock-win11-test`) and the Tart home (`/Volumes/d7sst7/dev-targets/tart-home`) are on d7 today
  (d7 165 GiB free on 2026-11-09 and 96 % full on 2026-11-08; d8 1.0 TiB free). Recommendation: move them to d8 soon, as
  one change to the machines list and the scripts' defaults, while no lease is held.
- **D6. A second Windows test VM?** Recommendation: not now; the lease logs wait times, so decide from those numbers.
- **D7. Lease expiry.** Recommendation: **activity expiry of 30 minutes, renewed by every driver call, `renew` and a live
  `lease run` child; no wall-clock maximum for an active lease;** a lease that expires leaves the machine `dirty` for the
  next holder to restore.

## Verified while drafting (2026-11-09)

- The repository facts and instance counts above, on `origin/main` `b51c72509` and the named branches (`git show`).
- Job 32417's runner, work folder, build-jobs line and EMFILE lines (a job log copy in the triage scratch folder).
- The Gitea job list of the last 300 runs (runs 7318 to 7792, jobs started 2026-11-01 to 2026-11-09): jobs at once per
  host, computed from each job's start and end. The macOS maximum of 4 comes from `-3` showing two overlapping jobs
  (32056 and 32060, from 01:18 +08) around its killed hand-run and its launchd start (01:16 to 01:19 +08); a runner of
  capacity 1 cannot run two, so one is probably the abandoned task and the real maximum is 3 (not checked further). The sccache lines of Linux jobs 32663 to
  32666 (from their logs; the endpoint and addresses in them are not copied here).
- On the Mac: 14 cores (10 + 4), 64 GB, `launchctl limit maxfiles` 256 soft; the Windows VM's `.vmx` (4 vCPUs, 8192 MB);
  the Tart VMs' `config.json` (4 CPUs, 6 or 8 GB); free space on `/`, d7 and d8.
- Not verified: `decryp7ology-dev`'s and `d-gtr9`'s size (from `build-jobs.sh` comments of 2026-10-30); that the Linux
  containers' `/sccache` is one host folder (ADR 0025 §8); whether a Redis server is live for sccache; whether the runner
  exposes its name or version as an env; whether the Gitea runner accepts the small-host `container.options`.
