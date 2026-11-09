# 74. Declared hosts and machine leases: one inventory of every runner and test machine, a live check against it, a lease for shared VMs

- Status: Proposed (2026-11-09; drafted at the user's request of 2026-11-09 to write the redesign ADRs 0071 to 0074 now;
  needs an independent review and the user's decisions below before any build)
- Date: 2026-11-09
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-20 / rule 51 (parallelism and cost), ER-24 /
  rule 41 (scale parallel work to the machine), ER-14 / rule 7 (tests never touch the user's machine), ER-27 (logs
  explain every failure), ER-32 (bounded waits), ER-33 / rule 71 (a check counts only with a verified precondition and a
  positive control), ER-22 / rules 42 and 55 (share first, one implementation per mechanism), rule 53 and ADR 0033
  (processes by identity); ADR 0025 (CI job isolation: slots, kept folders, `ports.txt`), ADR 0022 (`paddock-dev`), ADR
  0040 and ADR 0042 (the VM harness and its owner; ADR 0042's "lease" is the ownership of a harness's code, not of a
  machine), Pitcrew ADR 0021 (a pool of capacity-1 runners per host)
- Code it changes: `conformance/ci/build-jobs.sh`, `.gitea/actions/{build-jobs,job-isolation,stable-workspace}`, every
  `.config/nextest.toml` (7), `crates/paddock-slots`, `tools/paddock-dev` (`./dev host`, `./dev machine`),
  `scripts/vm-win.sh` and `scripts/vm-smoke/*` (on `tools/vm-smoke`), `tools/macos-vm-smoke.sh`,
  `.gitea/workflows/service-smoke.yml`, the Pitcrew runner layout (`apps/pitcrew/crates/pitcrew-core/src/layout.rs`) and
  the add-runners runbook (on `docs/pitcrew-autoscale`)

## Pattern

**Shared machines have undeclared capacity and ownership.** Whenever a machine is shared (several CI runners on one
host, agents and CI on one Mac, several agents on one test VM), each consumer carries its own private guess of the
machine's size, limits, volumes and who else uses it. The guesses drift apart the moment the machine changes, and the
first sign is a red run or a broken VM whose cause looks like "load" or "flaky".

**Invariant.** Every CI runner and every shared test machine is declared once, in one file in the repository: its cores,
RAM, how many jobs or users share it at once, the share kept for others, its limits (open files), its volumes and its
owner. Every consumer reads its numbers from that file, a check proves at run time that the live machine still matches
it, and a machine that only one user may drive at a time is used only under a held lease. Tests never depend on the size
or free space of the host they run on.

## Context: the incidents

All on 2026-11-08/09 unless dated otherwise. "Verified" means read in the repository, a job log or the handoff log;
"reported" means stated in the brief or a handoff line that I could not check myself.

| # | Incident | Evidence | What was undeclared |
|---|---|---|---|
| I1 | Three macOS runners (`d7-mac-mini`, `-2`, `-3`) went live on one 14-core Mac at 01:33 +08, while `build-jobs.sh` still described macOS as "1 job per host, 10 cores kept", the build-jobs action and README said the same, and all seven `nextest.toml` CI profiles said "A CI job has its runner to itself" with `test-threads = "num-cpus"` (14 test threads per job, 42 at once). | Verified: `origin/main` (`b51c72509`) `conformance/ci/build-jobs.sh:28-31`, `class_default` (`macOS:cores` 10, share 1); the seven `nextest.toml` files; runner hand-off `runners-2026-10-28.md` (2026-11-09 01:33). Job 32417 on `d7-mac-mini-2` logged `CARGO_BUILD_JOBS=4 ((14 cores - 10 kept) / 1 jobs per host ...)` while three runners ran. Fixed in instances on `fix/main-after-7761` `deb01e7ca` (12 files). | Jobs per host and the share kept for the agents, per host, in one place |
| I2 | The three runners of that one host disagree about the host: `-2` and `-3` got `PADDOCK_CI_JOBS_PER_HOST=3`, `PADDOCK_CI_RESERVED_CORES=2` and `NumberOfFiles=65536` in their launchd plists; the original `d7-mac-mini` (Pitcrew-managed) got none of them. ADR 0025's rollout says to put these in `runner.envs`, not in a plist. | Verified: hand-off lines 2026-11-09 06:51 and 06:53, and overnight log line 1471 ("Original d7-mac-mini unchanged"). | A per-host fact stored per runner, by hand, in two different mechanisms |
| I3 | New launchd runners inherited launchd's default soft limit of 256 open files; a Debrief DuckDB writer test failed with EMFILE. | Verified: job 32417 (`d7-mac-mini-2`) log lines 4608 and 5055, `IO Error: Cannot open file ".../segments/seg-...parquet": Too many open files`; `launchctl limit maxfiles` on this Mac prints `256 unlimited`, so the 256 limit of the job is inferred, not read from the job (no job logs `ulimit -n`; WP-4 adds it). Fixed by hand in two plists (I2). | The runner's open-files limit |
| I4 | `stable-workspace` hit its 300 s bound on `d7-mac-mini` because the runner's work folder was on the internal disk and the CI home on `/Volumes/d7sst7`, so `mv` copied the whole checkout (7 to 293 s; nine timeouts in runs 7706 to 7761). The same cause was already recorded on 2026-11-04 (run 7501) and left as "runner fix: with the user". | Verified: `c2179dba0` message (measurements), `docs/lessons.md` "The macOS runner's stable-workspace step copies the checkout across volumes (2026-11-04)"; runner moved at 03:35 +08 (hand-off). Guard added in `c2179dba0`. | Which volume each runner folder lives on, and the rule that work folder and CI home share one |
| I5 | A Dyno disk benchmark test needed 401 GB free: the default margin is 10 % of the volume, and the CI volume is 4 TB with 266 GB free. | Verified: `2c7c8891e` (on `fix/dyno-bench-disk-size`, `train/2026-11-08-1`): the test now passes an explicit `--free-margin-mib`. | A test that reads the host's volume size |
| I6 | Two agents used the Paddock Windows test VM (`paddock-win11-test`) at once: one loaded all 4 vCPUs from an SSH session and lost SSH, another powered the VM off. | Verified: overnight log line 1474 ("INCIDENT"), `9c738b1ad` lesson; the VM's `.vmx` declares `numvcpus = "4"`, `memsize = "8192"`. `scripts/vm-win.sh` (on `tools/vm-smoke`) has a load check but no lock. | Who holds the VM; how much of it a user may load |
| I7 | At a job's release step the job's sccache server was already stopped and its temp folder removed, so `./dev` could not build paddock-dev through `RUSTC_WRAPPER=sccache`; the release then reported "the holder of sN ended before the job did" (runs 7773 to 7775), which had not happened. | Verified: `726c0f7ce` (message and diff). | The order of per-slot state (slot, sccache server, temp) at release |
| I8 | Pitwall's server e2e stalled up to 27.7 s: macOS `syspolicyd` assesses every fresh binary on first start, one queue per Mac, and three runners on one Mac fill it. | Reported: overnight log line 1457 ("CONFIRMED cause"), on `fix/pitwall-e2e-engine-health`; a user decision on exempting the runner is pending. | A host-wide service that three runners share |
| I9 | Dyno's `link::model` probe timed out at 60 s in run 7775; in the job logs it took 15 to 26 s whenever it overlapped `dyno-bench`'s all-core tests in the same job, about 7 s otherwise. | Verified: `docs/benchmarks.md` block "2026-11-09, CI share on the shared Mac" on `deb01e7ca`; not reproduced, cause open (overnight log line 1474). | Tests that size their own load from the host's core count, not the job's share |
| I10 | Earlier, same class: a Spotter test expected `NotFound` under `temp_dir()` and got `Unreachable` because the CI Mac's temp folder sat under `/Volumes/` (2026-11-04); launchd runners on `/Volumes/d7sst7` failed with exit 78 (TCC, removable volumes) until their program moved to the internal disk (2026-11-04 and 2026-11-09 01:19). | Verified: `docs/lessons.md` (two 2026-11-04 entries); hand-off 2026-11-09 01:19. | The volume and its kind (internal or removable) of each runner folder |

I1, I2 and I4 were each found by a red main run; I4 was the second time; I6 broke a shared test machine. Each fix so far
corrected one consumer's copy of a fact about the machine.

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
| `.gitea/actions/stable-workspace` | work folder and CI home on one volume | broken (I4), guarded by `c2179dba0` (falls back to a cold build) |
| `conformance/ci/job-tmpdir.sh` | macOS `TMPDIR` on the internal disk when the work volume is external (reads `diskutil`) | fine (reads the machine) |
| `.gitea/actions/setup-rust` sccache size | `PADDOCK_CI_SCCACHE_GB` (40) / `PADDOCK_CI_SLOTS` | at risk: a host budget per runner env |
| `job-isolation.sh` free-disk floor, orphan age | `PADDOCK_CI_MIN_FREE_GB` (30), per runner env | at risk |
| ADR 0025 §8 vs `build-jobs.sh` | "one job per Linux host" (shared `/sccache` mount, one sccache server) vs "d-gtr9, capacity 4" | at risk: the two disagree; which is live is not verified |
| `apps/pitcrew/crates/pitcrew-core/src/layout.rs` | writes a runner's `workdir_parent` under the runner's own folder | at risk (I4, I10: the folder's volume is not chosen by any rule) |
| add-runners runbook (`docs/pitcrew-autoscale`, not on main) | hand steps per host | at risk |
| `scripts/vm-win.sh` (`tools/vm-smoke`) | one VM, `PADDOCK_VM_MAX_LOAD` 25 on the host; no lock | broken (I6) |
| `scripts/vm-smoke/run-win.mjs`, `chain-release.sh`, `make-no-dyno-snapshot.sh` (`tools/vm-smoke`) | drive the Windows VM (the last two call `vmrun` directly); "hold the host's VM lock" only in comments (`plock run` with `PLOCK_DIR=.../vm-lock`, a convention outside the repo) | broken (I6) |
| `scripts/vm-smoke/run.mjs`, `tools/macos-vm-smoke.sh`, `service-smoke.yml` | Tart VMs under a host-wide slot (`./dev ci slot`, `PADDOCK_VM_MAX` 1, at most 2) | fine: the model for the lease below |
| `scripts/vm-smoke/run-linux.mjs`, `login-probe-linux.sh`, `login-probe-mac.sh` | `tart clone` and `tart run` without the slot | at risk |
| test code reading host size or free space directly | Rust `tests/` files: 4 match the probe (`available_parallelism`, `statvfs`, `free_space`, ...); all 4 go through a seam (`Downloader::with_free_space`) or are a name match, so 0 direct reads. 36 non-test Rust files read host size or free space (Dyno 14, Debrief 14, shared crates 6, Pitwall 1, Pitcrew 1); the tests that reach them are not enumerated | broken once (I5), at risk |
| tests sizing their own load from the host (`dyno-bench` all-core kernels in a CI job) | every core of the host, not the job's share | at risk (I9) |

## Decision

One declared inventory, read by every consumer and checked against the live machine; one lease for machines that take
one user at a time; one rule, with checks, that tests never depend on the host.

### 1. The inventory: `conformance/ci/hosts.toml`

One TOML file beside `kept-folders.txt` and `ports.txt` (the CI kit's other declared lists), parsed once by `paddock-dev`
(parse, don't validate: typed records, unknown keys refused). It declares hosts, their volumes, the CI runners on them and
the shared test machines. A sketch with today's facts (values marked `verified` carry the date and source of the fact):

```toml
schema = 1

[hosts.mac-mini]                    # the developer Mac: agents, three macOS runners, the test VMs
os = "macos"
arch = "arm64"
cores = 14                          # 10 performance + 4 efficiency (sysctl, 2026-11-09)
ram_mb = 65536
owner = "user"                      # who may change the machine; "coordinator" may change only the CI share below
shared_with = ["agents", "ci", "test-vms"]
ci_jobs = 3                         # runners x capacity on this host; the live check counts busy slots against it
reserved_cores = 2                  # kept for agents and the system (deb01e7ca); see decision D1
reserved_ram_mb = 32768
test_threads = 8                    # per CI job: coordinator decision 2026-11-09 (overnight log 1474); the pushed
                                    # fix/main-after-7761 still computes 4 = (14 - 2) / 3
open_files_min = 65536              # every CI job's soft limit (I3)
verified = "2026-11-09 sysctl, launchctl print"

[hosts.mac-mini.volumes]
system = { mount = "/", kind = "internal", min_free_gb = 60 }
d7 = { mount = "/Volumes/d7sst7", kind = "external", min_free_gb = 100, holds = ["ci-home", "runner-work", "worktrees", "vms"] }
d8 = { mount = "/Volumes/d8sst7", kind = "external", min_free_gb = 100, holds = ["main-checkout"] }

[runners.d7-mac-mini]
host = "mac-mini"
capacity = 1
labels = ["macos-latest:host", "macos-arm64:host"]
program_volume = "system"           # launchd cannot start a program on a removable volume (TCC, I10)
work_volume = "d7"                  # must equal the CI home's volume (I4)
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

# [runners.d7-mac-mini-3] as -2; [hosts.d-gtr9] (Linux, 16 cores, 128 GB, containers) and [hosts.decryp7ology-dev]
# (Windows, 8 cores, 16 GB) from build-jobs.sh's comments, marked verified = "2026-10-30 job log" until WP-4 measures them.

[machines.paddock-win11-test]       # the Windows 11 Arm test VM (VMware Fusion)
host = "mac-mini"
kind = "vmware-fusion"
vcpus = 4
ram_mb = 8192
volume = "d7"
users_at_once = 1                   # a lease is exclusive
max_load = 3                        # busy processes a user may start in it: vcpus - 1 (lesson 2026-11-09)
lease_max = "6h"                    # a lease ends by itself after this (stalled owner)

[machines.tart]                     # Tart clones for the macOS and Linux VM checks, local and service-smoke in CI
host = "mac-mini"
kind = "tart-pool"
users_at_once = 1                   # PADDOCK_VM_MAX today; macOS allows at most 2 macOS guests
vcpus = 4
ram_mb = 8192
volume = "d7"
lease_max = "4h"
```

Rules of the file, checked by a conformance test (`hosts`, new) that fails on today's state until migrated:

- the arithmetic holds: `ci_jobs` equals the sum of its runners' `capacity`; `(cores - reserved_cores) / ci_jobs >= 1`;
  `test_threads` and the derived build share fit the cores; every volume a runner or machine names exists on its host;
  a runner's `work_volume` holds `ci-home` (I4) and its `program_volume` is `internal` on macOS (I10);
- no secrets or personal data: no addresses, tokens, user or account names, serial numbers; mounts and labels only
  (the test greps the file for IP-address, token and `/Users/<name>` shapes and fails on any);
- every fact has a `verified` date and source; the wiki-style 30-day staleness flag (ER-30) is reported, not failed.

`paddock-dev` also writes a flat, generated `conformance/ci/hosts.env` (one `KEY=value` line per runner and host fact),
checked current by the same test like any generated file, because `build-jobs.sh` runs before the job has built
paddock-dev and must stay bash 3.2 and dependency-free.

### 2. Every consumer reads the inventory

- **`build-jobs.sh`** finds its runner by `PADDOCK_RUNNER` (the one runner env each runner needs; set by the setup in §4)
  in `hosts.env` and computes `CARGO_BUILD_JOBS` and `NEXTEST_TEST_THREADS` from the host's record. The per-class
  defaults (`class_default`) and the per-runner `PADDOCK_CI_JOBS_PER_HOST`, `PADDOCK_CI_RESERVED_*` envs go. An
  undeclared runner (`PADDOCK_RUNNER` unset or unknown) is an `::error::` that names the file to edit: a new runner cannot
  take jobs with a guessed share. `PADDOCK_CI_CORES`, `PADDOCK_CI_RAM_MB` and `RUNNER_OS` stay as test seams only.
- **nextest:** the seven CI profiles keep `test-threads = "num-cpus"` only as the value for a host a job has to itself,
  and say that `NEXTEST_TEST_THREADS` from the inventory replaces it; a conformance test fails when a profile claims the
  host ("to itself") or a workflow sets `NEXTEST_TEST_THREADS` outside build-jobs.
- **job-isolation, setup-rust, prune:** `PADDOCK_CI_SLOTS`, `PADDOCK_CI_KEEP_SLOTS`, the sccache budget and the free-disk
  floor come from the host's record (`ci_jobs`, the CI-home volume's `min_free_gb`), not from runner envs.
- **stable-workspace** keeps `c2179dba0`'s volume guard as a belt; the live check (§3) catches the cause first.
- **Runner setup:** `./dev host runner-config <runner>` renders what a runner needs from its record: `runner.envs`
  (`PADDOCK_RUNNER`, `PADDOCK_CI_HOME`), `host.workdir_parent` on the declared volume, and the service limits (launchd
  `SoftResourceLimits`/`HardResourceLimits` `NumberOfFiles`, systemd `LimitNOFILE`; Windows has no per-process open-files
  limit to set). `--check` compares the rendering with the runner's live `config.yaml` and plist read-only and prints the
  difference. Applying stays a hand step of the host's owner (and of Pitcrew, whose `layout.rs` takes the volume from the
  rendering); the repository never writes a runner's configuration.

### 3. The live check: every host job proves its runner matches the declaration

A step in `job-isolation acquire` (the first point at which paddock-dev exists; build-jobs has already logged the
probes) runs `./dev host check --runner "$PADDOCK_RUNNER"`. It measures and prints one table, declared against live:

| Fact | Probe | Mismatch |
|---|---|---|
| cores, RAM | `sysctl hw.ncpu`/`hw.memsize`, `nproc` and the cgroup in a container, `NUMBER_OF_PROCESSORS` | error (RAM within 5 %) |
| open files | the job's soft `ulimit -n` (Windows: skipped, said so) | error when below `open_files_min` |
| volumes | device ids and mount of the work folder, the CI home and `TMPDIR`; `diskutil info -plist` internal or not on macOS | error when the work folder and CI home differ or a folder is not on its declared volume |
| free space | `statvfs` on each declared volume of the runner | error below `min_free_gb` |
| jobs at once | busy slots from `./dev ci slot status` at acquire | error when more than `ci_jobs` are busy (an undeclared runner on the host) |
| runner version | `GITEA_RUNNER_VERSION`-style env if the runner sets one, else the version line the runner prints at job start (whether act exposes it as an env is not verified) | warning |

**Positive control (ER-33, rule 71).** Before the real comparison the same comparator runs against a copy of the record
with three seeded faults (cores + 1, `open_files_min` doubled, the CI home's volume renamed) and must report exactly those
three. If it reports fewer, the step prints `check broken` with the fact it could not see and fails; it never prints
"matches" from a probe that has not shown it can see a mismatch. The probes' raw values go to the log (ER-26: report raw
values) and a one-line summary to the job summary.

A mismatch fails the job with one `::error::` per fact, the declared and live value and the next step ("edit
`conformance/ci/hosts.toml`" or "set NumberOfFiles in the runner's plist: `./dev host runner-config <runner>`"). Rollout
starts in warning mode for one week (decision D3). The check costs well under a second (a handful of syscalls); it
replaces no other step.

### 4. A lease for every shared test machine

`./dev machine lease` takes a machine from the inventory by name, built on `paddock-slots` (ER-22: the same OS file lock
the CI slots and the Tart VM slot already use, so the kernel frees it when its holder dies and nothing is ever reclaimed
by hand):

- `acquire <machine> --purpose "<brief or branch>" [--owner-pid <pid>] [--wait <dur>]` starts a detached holder that takes
  `flock`/`LockFileEx` on `<lease root>/<machine>-s<n>.lock` (`n` up to `users_at_once`) and prints a token. The holder
  watches its **owner by identity** (rule 53): the owner's pid plus start time (`paddock_proc::ProcessId`), by default the
  calling shell's parent; it lets go when that process ends or when the pid now names a process with another start time.
  A new `HoldProbe` in `paddock-slots` beside today's owner-folder probe.
- The owner record beside the lock holds the owner's pid and start time, the purpose, the start and the token; it never
  holds a secret or a user's data. `status` lists holders and their age; `release <machine> --token` ends one.
- **Stale leases recover by themselves:** a dead holder frees the lock (kernel); a dead or replaced owner ends the
  holder (identity check every second); an owner that lives but forgot the machine (a stalled agent session) loses the
  lease at the machine's `lease_max`. `break <machine> --reason "<why>"` exists for the coordinator only, logs the reason
  and the broken owner's identity, and never deletes a lock file while a holder runs.
- `run <machine> -- <command>` holds the lease for one command (the shape of `./dev ci slot idle-run`).
- **Every VM script takes it, and the drivers refuse without it.** `scripts/vm-win.sh` runs a mutating subcommand
  (`start`, `stop`, `ssh`, `put`, `get`, `job`, `snapshot`, `revert`, `gui-setup`, `install`, `baseline`) only with a
  `PADDOCK_LEASE_TOKEN` that `./dev machine lease check paddock-win11-test` accepts; `status` and `ip` stay free. The
  Tart scripts move from `./dev ci slot --root $TART_HOME/.paddock-vm` to the `tart` machine's lease (same crate, so the
  CI job `service-smoke` and a developer's run share one lease). Child scripts inherit the token.
- A load limit per machine: `max_load` (vCPUs minus one for the Windows VM, after the 2026-11-09 lesson) is what a script
  that loads the guest on purpose may start, and those scripts read it from the inventory.

### 5. Rule: tests never depend on the host

Proposed rule (number assigned on acceptance): **"A test never depends on the size, limits or layout of the machine it
runs on.** It never reads the host's cores, RAM, free space, volume size, open-files limit or temp-folder location to
decide what it expects or how much it loads; it injects them through a seam (as `Downloader::with_free_space` does) or
states them (an explicit margin, an explicit thread count). A test that must load the machine takes the job's share
(`NEXTEST_TEST_THREADS`), never every core. *Why:* on 2026-11-08 a 16 MiB disk test needed 401 GB free on a 4 TB CI
drive, a DuckDB test hit the runner's 256 open files, and a CPU-bound test took 2 to 4 times longer whenever all-core benchmark tests
ran in the same job."

Checked twice, each with a positive control:

- **Static (conformance `host-size`, new):** every direct read of host size, limits or temp location in test code (Rust
  `tests/` and `#[cfg(test)]` modules, `*.test.mjs`, `*.spec.ts`; probes: `available_parallelism`, `num_cpus::get`,
  sysinfo memory, `statvfs`/`statfs`/`available_space`/`GetDiskFreeSpace*`, `os.cpus`, `os.totalmem`, `os.freemem`,
  `availableParallelism`, `statfsSync`, `hardwareConcurrency`, `temp_dir()` used to classify a path) is either through a
  seam or carries `host-size: <why>` on the line before. A fixture with a bare read must fail the check.
- **Dynamic (a nightly `small-host` leg):** the Rust suites of `crates/` and the apps' Rust workspaces run on the Linux
  host in a container limited to 2 CPUs and 6 GB, `ulimit -n 256` (launchd's default), and `TMPDIR` on a 2 GiB tmpfs.
  A test that sized itself from the host fails there. A fixture test that opens 300 files must fail the leg (positive
  control). It does not catch a volume-ratio bug like I5 (that needs a large volume with little free space), which is
  why the static check exists.

## Parallelism and cost

- **More parallelism.** The inventory makes the CI share explicit instead of guessed. On the Mac today: `ci_jobs` 3 ×
  4 rustc plus up to 10 agent threads (rule 41) is 22 busy threads on 14 cores; measured, that slowed a CPU-bound test
  1.5 times (2.7 to 4.0 s), and wait-heavy test runs took 2.3 times longer at 4 test threads than at 14 while 8 matched 14
  (`docs/benchmarks.md`, "CI share on the shared Mac", on `deb01e7ca`). So the share is set per host from measurement,
  and the inventory is where the measured value lives (`test_threads = 8`). VMs: one Windows VM and one Tart clone at a
  time stay the limit (4 vCPUs and 8 GB each; the Windows VM loses SSH at full vCPU load). A second Windows test VM would
  double VM throughput at about 8 GB of RAM and 60 GB of disk; not proposed (decision D6).
- **Cheaper.** The live check is a few syscalls per job (< 1 s, not measured yet; WP-4 records it per rule 48). The lease
  adds no time when uncontended; when contended it turns a broken VM (I6: a lost hour and a re-setup) into a bounded
  wait (`--wait`, ER-32). The generated `hosts.env` keeps build-jobs at shell speed before paddock-dev exists. The
  small-host leg is the one real cost: one Linux runner for one full Rust test pass per night (not measured; the full
  nightly already runs these suites, so the leg can replace their nightly Linux run rather than add one, decision D4).

## Alternatives

- **Keep per-runner env vars, document them better (status quo after `deb01e7ca`).** Rejected: I2 shows three runners of
  one host already hold three different stories, in two mechanisms, after one night. A reminder is not a check.
- **Discover everything at run time, declare nothing.** Probing cores and RAM is easy; what cannot be probed is intent:
  how many runners should be on the host, what share the agents keep, which volume a folder belongs on, who owns the
  machine. Without a declaration there is nothing to compare against, and an extra runner silently changes everyone's
  share. Rejected; probing is the check, not the source.
- **Pitcrew as the source of truth.** Pitcrew manages runners and could hold their settings. Rejected for now: the CI
  kit and the VM scripts must read the facts without a running app, the test VMs are not runners, and the repository
  must be able to review a change of share. Pitcrew reads the rendering (§2) instead; it can own the inventory later.
- **plock as the VM lock** (the convention in two script comments). Rejected: plock is a build queue outside the
  repository, the scripts do not take it, nothing refuses a driver call without it, and its stale handling is not
  identity-based. The Tart slot already shows the in-repo mechanism working.
- **A lock file with a pid inside.** Rejected for the reason ADR 0025 gives: a reclaim rule races, and a bare pid is a
  reusable name (rule 53).

## Consequences

- One edit adds or resizes a runner; build-jobs, nextest, slots, the sccache budget, the free-disk floors, the live
  check and the runner rendering all follow. A runner that is not in the file cannot take a job.
- Changing a machine without changing the file fails the next job on it with a message that names the line to change;
  this is the point, and it costs one red job per drift (warning mode first).
- Agents must take the lease before touching a test VM; briefs name the machine and the lease command. A forgotten lease
  holds a VM at most `lease_max`.
- The inventory names hosts, mounts and runner names that are already in the repository (`build-jobs.sh`, ADR 0025,
  `docs/lessons.md`); it adds no addresses, accounts or secrets, and its test refuses them.

## Migration

Order matters: the inventory first (read by nobody), then each consumer, then the old knobs go.

1. Land `hosts.toml` with today's facts and its conformance test (WP-1). Everything else unchanged.
2. Land `deb01e7ca` (on `fix/main-after-7761`) as it is; it is the interim fix of I1.
3. `build-jobs.sh` reads `hosts.env` (WP-2); runners get `PADDOCK_RUNNER` (one env each, set when idle, one runner at a
   time, through `./dev host runner-config`). Until a runner has it, build-jobs falls back to today's class defaults with
   a warning, for one week only.
4. The live check in warning mode (WP-4), then failing after a clean week.
5. The lease (WP-6), then every VM script and driver (WP-7), after `tools/vm-smoke` (V1, `f857083c0`) lands, since the
   Windows driver lives there. The `plock run ... vm-lock` comments go.
6. The rule and its static check (WP-8); the instances it finds are fixed through seams, one commit per crate, each with
   its failing test first (rule 5); the small-host leg runs nightly.
7. Remove `class_default`, `PADDOCK_CI_JOBS_PER_HOST`, `PADDOCK_CI_RESERVED_*`, `PADDOCK_CI_SLOTS`/`KEEP_SLOTS` from
   runner envs and plists (WP-9), host by host when idle; the docs (`.gitea/actions/README.md`, ADR 0025's rollout, the
   runbook) point at the inventory.

**Rollback.** Each step is one revert: build-jobs keeps its class defaults until step 7; the live check has a warning
mode; the lease check in the drivers is one function. Nothing deletes runner state or VM files.

## Work packages

| WP | What | Depends on | Estimate (agent-days) |
|---|---|---|---|
| WP-1 | `conformance/ci/hosts.toml` (schema 1, today's facts with sources), `./dev host` parser with typed records, generated `hosts.env`, conformance test `hosts` (arithmetic, volumes, no-secrets grep, generated file current) | – | 1.5–2.5 |
| WP-2 | `build-jobs.sh` reads `hosts.env` by `PADDOCK_RUNNER`; undeclared runner is an error after the fallback week; slots, sccache budget and free floors from the record; tests in `ci-actions.test.mjs` | WP-1, `deb01e7ca` | 1–1.5 |
| WP-3 | nextest profiles' comments and the conformance test (no "to itself", no `NEXTEST_TEST_THREADS` outside build-jobs) | WP-2 | 0.5 |
| WP-4 | `./dev host check` (macOS, Linux container, Windows), positive control with seeded faults, step in `job-isolation acquire`, warning mode switch, job summary line; benchmark of its cost recorded (rule 48) | WP-1 | 2.5–4 |
| WP-5 | `./dev host runner-config <runner> [--check]` (runner.envs, workdir, launchd/systemd limits); Pitcrew `layout.rs` takes the volume from it; land the runbook with the inventory steps | WP-1 | 1.5–2.5 |
| WP-6 | `paddock-slots` owner-identity `HoldProbe`; `./dev machine lease acquire/run/check/status/release/break` with `lease_max` and owner records; real-process tests (owner killed, pid reused with another start time, two acquirers, stalled owner past `lease_max`, break logged) | WP-1 | 2.5–4 |
| WP-7 | VM scripts adopt the lease: `vm-win.sh` refuses mutating commands without a token; `run-win.mjs`, `chain-release.sh`, `make-no-dyno-snapshot.sh` (no direct `vmrun` without the token), `run.mjs`, `run-linux.mjs`, `login-probe-*.sh`, `tools/macos-vm-smoke.sh`, `service-smoke.yml`; conformance check that every file invoking a VM driver takes the lease (fixture without it must fail); brief template line | WP-6, V1 landed | 2–3 |
| WP-8 | the rule; static check `host-size` with fixtures; nightly `small-host` leg with a positive-control fixture | – | 3–5 |
| WP-9 | remove the old knobs from scripts, runner envs and plists (host by host, idle); README, ADR 0025 rollout note, lessons entry, AGENTS.md pointer, wiki notes | WP-2 to WP-7 | 1–1.5 |
| | **Total** | | **15.5–24.5** |

Not estimated: fixing the instances WP-8 finds (their number is known only after the first run of the static check and
the small-host leg), and anything decision D2 adds for the Linux host. Critical path: WP-1 → WP-6 → WP-7 (6–9.5 days);
WP-2/3, WP-4/5 and WP-8 run beside it. The lease (WP-6, WP-7) is the most urgent part: I6 can recur at the next VM run.

## Decisions for the user

- **D1. Keep three macOS runners on the shared Mac?** Recommendation: **yes, keep three for now, declared as
  `ci_jobs = 3`, `reserved_cores = 2`, `test_threads = 8`,** and decide again after one week of job durations from the
  live check's logs (queue time plus run time per macOS job, three runners against the week before). Reasons: the macOS
  runner was the main CI bottleneck (overnight log 01:0x), and the measured cost of three at once is about 1.5 times per
  CPU-bound test, not the 20 times of the run-7775 timeout (`docs/benchmarks.md`). Against: the Mac is also the agents'
  machine (rule 41 lets them use 10 cores, so 22 busy threads at peak) and syspolicyd's first-start queue is shared (I8,
  its own pending decision). Alternative: two runners with 6 build jobs each, if the week shows longer total times.
- **D2. The Linux host: one job or four?** ADR 0025 §8 says one job per Linux host until containers take slots (all
  containers share one `/sccache` disk cache, which takes one sccache server); `build-jobs.sh` says `d-gtr9` runs with
  capacity 4. Recommendation: declare what WP-4 measures, and if it is 4, give containers slots on the mount (ADR 0025's
  own follow-up) as a separate change before trusting the shared cache.
- **D3. Mismatch: fail or warn?** Recommendation: warn for one week, then fail. A failing check blocks a release on a
  drifted runner; the fix is one line or one plist key, and the message names it.
- **D4. Small-host leg: nightly in addition, or instead of the nightly Linux Rust run?** Recommendation: instead (same
  tests, tighter box), so it costs no extra runner time; keep an unconstrained run only on a tag.
- **D5. Where the VMs live.** Your drive layout of 2026-11-06 puts test machines on d8; the Windows VM
  (`/Volumes/d7sst7/vms/paddock-win11-test`) and the Tart home (`/Volumes/d7sst7/dev-targets/tart-home`) are on d7 today
  (d7 had 165 GiB free when this was written, d8 1.0 TiB). Recommendation: declare them where they are now, and move
  them to d8 as one change to the inventory and the scripts' defaults when no lease is held.
- **D6. A second Windows test VM?** Recommendation: not now; the lease makes contention visible (wait times in its
  log), so decide from those numbers.
- **D7. Lease length.** Recommendation: `lease_max` 6 h for the Windows VM (a full release chain), 4 h for Tart; longer
  only through `break`-free renewal by the same owner.

## Verified while drafting (2026-11-09)

- The repository facts and instance counts above, on `origin/main` `b51c72509` and the named branches (`git show`).
- Job 32417's runner, work folder, build-jobs line and EMFILE lines (job log copy in the triage scratch folder).
- On the Mac: 14 cores (10 + 4), 64 GB, `launchctl limit maxfiles` 256 soft; the Windows VM's `.vmx` (4 vCPUs, 8192 MB);
  the Tart VMs' `config.json` (4 CPUs, 6 or 8 GB); free space on `/`, d7 and d8.
- Not verified: d-gtr9's and decryp7ology-dev's capacity and size (from `build-jobs.sh` comments of 2026-10-30), whether
  the runner exposes its name or version as an env, I8's cause (reported as confirmed in the overnight log).
