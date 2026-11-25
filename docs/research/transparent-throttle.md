# Transparent throttle: Pitcrew starts CI jobs by admission, and caps what it cannot queue (2026-11-25)

Short research note, no code. Question: how can Pitcrew deploy `throttle` (today `tools/heavy`, being renamed) machine-wide
so that every project on a machine, Paddock's or not, is throttled without calling anything? Part 1 (the main question)
is CI jobs: Pitcrew keeps a pool of pre-registered runners and starts one, for one job, only when throttle admits one more. Part 2 is the rest: steps started by shells,
agents and IDEs (cgroups, Job Objects, shims, a jobserver, QoS).

**Heads read** (ADR 0091), all on 2026-11-25: Paddock `origin/main` 62049e300161; `origin/ci/slot-affinity` 835c8fdde
(slot affinity, d2e3343d0); `origin/docs/pitcrew-autoscale` 9004a4dae (Pitcrew ADR 0021, proposed). gitea/runner sources
at tag `v5.4.1` (`internal/app/cmd/register.go`, `internal/app/cmd/daemon.go`, `internal/app/poll/poller.go`, `internal/app/run/runner.go`,
`internal/pkg/config/config.example.yaml`; gitea.com raw files). The Gitea server's `/api/v1/version` answered
`{"version":"28.4.0"}` and its `swagger.v1.json` was read the same evening. Runner versions in use (ADR 0112, read from job
logs on 2026-11-21): macOS v5.4.1, Windows and Linux v6.0.1.

**What is shipped.** Nothing here is in a release. heavy is CI and agent tooling (no app tag ships it). Pitcrew's
per-runner Job Object (`pitcrew-service/src/process.rs`, `job_for`, from 25b4f2f56) is on main; `git tag --contains
25b4f2f56` lists no Pitcrew tag, so it is not in a shipped Pitcrew.

## Checks and mechanisms this note relies on (ADR 0096)

| Mechanism | State |
|---|---|
| heavy's admission rule (`gate::allowed_slots`: 15-minute load, CPU idle, memory, at most 5) and the quick lane | built on origin/main (`tools/heavy/src/gate.rs`, `lock.rs`) |
| A container run is alone and is admitted on the host (`is_container_command`, drain marker) | built on origin/main |
| CI announces a host job to heavy (`./dev ci isolation acquire` runs `heavy mark ci-<slot>`, ADR 0102 §5) | built on origin/main; option 0 removes it |
| Per-slot job folders (`crates/paddock-slots`, `ci isolation`) | built on origin/main |
| Slot affinity (a job takes the slot its kind held last) | held on branch `ci/slot-affinity` (835c8fdde) |
| The rename to `throttle` and the lock as OS file locks by handle (`std::fs::File::lock`; refuse a network folder; the lock is taken on the host around containers; every caller goes through the binary; no fd inheritance) | prerequisite: approved per the coordinator's brief; no text or code on origin/main found by `git grep File::lock` |
| Pitcrew puts each runner in a kill-on-close Job Object (Windows) | built on origin/main |
| Pitcrew tracks a runner's processes and adopts them after a restart (Pitcrew ADR 0012) | built on origin/main (accepted 2026-10-22) |
| Pitcrew's pool model, queue signals and control law (Pitcrew ADR 0021) | held on branch `docs/pitcrew-autoscale` (proposed, not accepted) |
| The runner's own per-job Job Object (Windows) and per-step process groups (Unix) | external, read from gitea/runner v5.4.1 and v6.0.1 source by ADR 0112 |

## Part 1. CI jobs: Pitcrew is the controller

### What the runner and our server can do (verified unless marked)

- **One job, then exit, with a kept identity.** gitea/runner v5.4.1 `daemon.go`: `if daemArgs.Once || reg.Ephemeral { go
  poller.PollOnce(...) } else { go poller.Poll() }`; the callback removes the `.runner` file only for an ephemeral
  registration. `poller.go`: `PollOnce` fetches until it gets one task, runs it and returns; it does not use the capacity
  semaphore, so `capacity` does not matter in this mode and **no second job can be fetched**. The daemon then calls
  `poller.Shutdown` with `runner.shutdown_timeout` and exits. Gitea's docs: "`gitea-runner daemon --once` also exits after
  one job, but its credentials stay valid" ([registration](https://docs.gitea.com/runner/registration/)). The flag's name
  `--once` is from the docs and the `daemonArgs.Once` field; its cobra registration was not in the excerpt read.
- **Start cost (read, not measured).** Before its first fetch the daemon loads config and `.runner`, takes the runner-file
  lock ("a second process using the same file refuses to start"), parses labels, waits for Docker when a label needs it (up
  to `container.docker_timeout`), may rewrite `.runner` when labels changed, and calls `Declare`, retrying every 30 s
  (`declareRetryDelay`) while Gitea is unreachable. With Gitea up and no Docker wait that is a process start plus two HTTP
  calls; the first `FetchTask` then follows `fetch_interval`.
- **Capacity is read once.** `Poll()` makes `sem := make(chan struct{}, p.cfg.Runner.Capacity)` at start and never re-reads
  it; a running runner cannot be scaled, only started and stopped.
- **An admission hook exists upstream, but only when idle.** v5.4.1 has `health_check` (off by default): "low disk space or
  a failing script pauses new task fetching; existing jobs continue. No health checks run while any job is active; the
  last result is reused until idle", with `script`, `interval: 30s`, `timeout: 10s` (`config.example.yaml`;
  `Runner.CanAcceptTask`). It does not look at load. No upstream request for load-aware fetching was found (one search).
- **Ephemeral registration** (`--ephemeral`): "accepts exactly one job, then exits. Its credentials are revoked as soon as
  the job is assigned" (same page). v5.4.1 `register.go` falls back with "cannot register new runner as ephemeral upgrade
  Gitea to gain security, run-once will be used automatically". Gitea 28.4.0's swagger has `ActionRunner.ephemeral`.
- **The registration token**: "A token can register any number of runners until it is reset"; a runner takes jobs only
  within the token's scope (instance, organization, user or repository); `--token-file <path>` or
  `GITEA_RUNNER_REGISTRATION_TOKEN` keep it off the command line (same page). Gitea 28.4.0's swagger offers
  `POST .../actions/runners/registration-token` at `/admin`, `/orgs/{org}`, `/user` and `/repos/{owner}/{repo}`; no reset
  endpoint, so a reset is done in the web UI (not verified).
- **Deleting a runner.** Gitea 28.4.0's swagger: `DELETE /admin/actions/runners/{runner_id}`, `/orgs/{org}/...`,
  `/user/...` and `/repos/{owner}/{repo}/actions/runners/{runner_id}`, with `GET` lists of `{id, name, status, busy,
  disabled, ephemeral, labels}`. Token rights: `write:organization` for `/orgs/*`, `write:repository` for `/repos/*`,
  `write:admin` for `/admin/*` ("A `write:` scope covers GET plus POST, PUT, PATCH, and DELETE";
  [token scopes](https://docs.gitea.com/development/oauth2-provider)). The first Gitea version with these endpoints was not
  checked; our server has them.
- **Offline runners.** The configuration cheat sheet's Actions cron tasks (`stop_zombie_tasks`, `stop_endless_tasks`,
  `cancel_abandoned_jobs`, `start_schedule_tasks`) contain **no task that deletes offline runners**; only jobs and tasks
  expire: `ZOMBIE_TASK_TIMEOUT` 10m, `ENDLESS_TASK_TIMEOUT` 3h, `ABANDONED_JOB_TIMEOUT` 24h ("cancel the jobs which have
  waiting status, but haven't been picked by a runner for a long time")
  ([config cheat sheet](https://docs.gitea.com/administration/config-cheat-sheet)). Our server's own `app.ini` was not read.
- **Jobs whose labels match only offline runners** stay waiting; the run page shows "No matching online runner with label"
  (a Gitea change made matching count only online runners;
  [forum report](https://forum.gitea.com/t/setting-up-runner-for-gitea-actions/9036),
  [commit](https://gitea.psi.ch/gitea/gitea/commit/4ea522fecfd8f2d6b51466cb9ab6772f4594bf2e)). Read, not tried on our server
  (WP-0). Such a job is cancelled after `ABANDONED_JOB_TIMEOUT` (24 h).
- **Host-mode folders.** `runner.go` builds the job folder as `<workdir_parent>/<repository>`; the task id is added only with
  `bind_workdir`, so "concurrent host-mode jobs for the same repository share this directory" (source comment). A runner
  whose config points `workdir_parent` at slot k's folder reuses slot k's clone.

### Option 0: a pool of pre-registered runners, each started for one job on admission (recommended)

**Set-up (once, and on every settings change; see Reconciliation).** Pitcrew registers N runners, N being the pool's
maximum (throttle's most slots on that host, at most 5 by default). Registration k is named `pitcrew-<host>-<pool>-<k>`,
has the pool's labels, and owns slot k: its `.runner` file, its config (`capacity: 1`, `workdir_parent` and the cache
folder inside slot k's kept folder, a long `shutdown_timeout`) and slot k's kept checkout and build folder. The registration
token is used only here.

**Run time, a loop per pool:**

1. **Demand.** A job is waiting: Gitea's jobs API (`GET /repos|orgs/.../actions/jobs?status=queued`, labels in
   `ActionWorkflowJob.labels`; Pitcrew ADR 0021's signals) shows a queued job this pool can serve, and no started runner of
   the pool is still waiting for its first fetch. Without an API token, the pool keeps one runner started whenever admission
   allows (it fetches when a job comes).
2. **Admission.** Pitcrew calls the throttle crate in-process ("may one more step start?": load, CPU idle, memory, most
   slots). On yes it **holds slot k** (the slot's file lock, by handle, in the service), preferring the free slot whose
   folder last served the same repository and job kind (the rule of `ci/slot-affinity`, now in Pitcrew so other projects get
   it). On no, nothing starts and the job stays queued in Gitea. A container job is admitted as a container run (alone,
   host lock), as today.
3. **Start.** `gitea-runner daemon --once --config <slot k>` as the runner user, with `THROTTLE_HELD=<k>` in its
   environment so throttle calls inside a host-mode job pass through instead of queueing behind their own job (ER-43).
   Windows: in Pitcrew's kill-on-close Job Object, now with CPU rate and job memory limits (Part 2). Linux: in the throttle
   slice (Part 2).
4. **End.** The runner exits after its one job (or crashes). Pitcrew releases slot k. Gitea shows the registration offline
   until next time; nothing is re-registered and nothing is cleaned up.
5. **Not needed after all.** A started runner that has not fetched a job when demand is gone (another runner took it) is
   stopped only while the API shows it not busy; with the long `shutdown_timeout` a job fetched in that instant is finished,
   not cancelled (the default `0s` would cancel it).

This replaces the fixed `runner.capacity` (the pool's size follows throttle's admission) and CI's `heavy mark`
announcement (the job holds a slot before its first step). No workflow changes; no project knows about throttle. Slot
affinity and warm compile-cache keys follow from registration k always using slot k's folders.

**The race that remains.** Two runners of one pool started for two queued jobs may fetch in either order; both are admitted,
so either order is fine. A runner started for a job that a runner of another host takes first waits (step 5). A job fetched
by a just-started runner is always inside an admitted slot.

**Tokens and their exposure.** Two secrets: the registration token (set-up only) and an API token (the jobs list for demand,
the runner list and `DELETE` for reconciliation; `write:organization` for org runners, `write:repository` for repo runners).

- **Scope.** The narrowest that covers the pool: a repository's token for one repository, else the org's; Pitcrew refuses an
  instance (`/admin`) registration token and an API token with `write:admin`.
- **Store.** In the service's OS credential store: the macOS keychain of the service's user, Windows Credential Manager of the
  service account, the Linux secret service where a session exists, else a 0600 file owned by the service user outside every
  working tree. Never in a repository, a runner config, a log, a diagnostic bundle or an environment dump (ER-54; Pitcrew ADR
  0005 already keeps secrets in the service).
- **Who reads it, when.** Only the service, only for a registration or an API call, never kept in memory between them.
  Registration runs `gitea-runner register --no-interactive --token-file <file>` as a short child with a scrubbed environment;
  the file is in a 0700 folder of the service user and removed right after. The runners never see either token. On macOS the
  runners run as their own user (ADR 0120), so jobs cannot read the service's keychain; on Linux and Windows the same split is
  the target, and where a host runs both as one user Settings says that a job can read the tokens.
- **What a leak allows.** A registration token registers runners that receive jobs and their secrets; an API token with
  `write:organization` can also change the org. Rotation: the user resets the token in Gitea's UI and gives Pitcrew the new
  one; existing registrations keep working without it. Pitcrew reminds after a set number of days (Settings).
- **The runners' own credentials** (each `.runner`) stay valid between jobs, as today's runners' do. In host mode a job can read
  its own runner's `.runner`; that is unchanged by this design. Ephemeral registration (below) is the hardening for it.

**Per-job start cost.** Not measured (no runner binary on this account, no test registration; WP-0). From the source: a
process start, `Declare` and the first fetch, no registration. Clone and build folders are kept per slot, so a job's
checkout and incremental build cost what they cost today. Expected seconds, against jobs of minutes.

**Crash handling.**

- Runner crash: its child handle ends; Pitcrew releases slot k; Gitea stops the task after `ZOMBIE_TASK_TIMEOUT` (10 min by
  default, checked every 5 min). The registration stays and is used again.
- Pitcrew crash: the kernel frees its file locks. Windows: the kill-on-close Job Object ends the runner (the job is lost and
  Gitea stops it). macOS and Linux: the runner keeps running unlocked, so throttle may admit one step too many until Pitcrew
  restarts (seconds under launchd `KeepAlive` and systemd `Restart=always`), adopts its runners (ADR 0012 tracking) and takes
  their slots again. Handing the lock to the runner is ruled out (no fd inheritance).

**Platforms.** macOS: the runner is a LaunchDaemon under its own user (ADR 0120, accepted); Pitcrew's service starts the
one-job runners as that user. Windows: Pitcrew's Job Object holds each runner; the runner's own per-job Job Object (v6.0.1)
nests inside it ("a child process is associated with every job in the job chain", since Windows 8;
[nested jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs)). Linux (d-gtr9, container jobs,
capacity 4 today): a pool of up to the slot count; containers go under the throttle slice through dockerd's `cgroup-parent`
(Part 2).

### Reconciliation: desired pool against actual

Settings give the desired pool (host, pool name, labels, N); Pitcrew's state file lists the registrations it created
(runner id, name, slot, labels). Gitea's runner list is the actual. Pitcrew acts only on a registration that is **both** in its
own list **and** named with its prefix (`pitcrew-<host>-`); anything else is another program's or a person's and is left
alone ("state another program owns", PRINCIPLES.md).

| Change | Steps |
|---|---|
| Grow | register the missing k (registration token), make slot k's folders, add to the list |
| Shrink | mark k draining (no new start); when its runner is not running (never kill a running job), `DELETE` the registration, then delete its `.runner`, config and slot folders, then drop it from the list |
| Label or name change | drain, `DELETE`, register again with the new labels or name, keep slot k's folders (the work is the same) |
| Uninstall | drain all, `DELETE` every listed registration, remove their files and slot folders, then the list |

Each step is idempotent and ordered so a crash between steps is finished on the next pass: the list entry goes last, a `DELETE`
answering 404 counts as done, and a listed id that Gitea no longer has is a deleted registration whose local files are
removed. A registration in the list but with a different name in Gitea (renamed by hand) is reported, not deleted.

**Test plan** (fake Gitea and a stand-in runner, invented names; no real server or runner, ER-14):

- Shrink while a job runs: the draining runner's job finishes; `DELETE` is sent only after it exits; no new start on k.
- Crash mid-unregister: kill Pitcrew after `DELETE` and before the files are removed, and again after the files and before the
  list; the next pass finishes, sends no second delete that fails the pass, and leaves no folder behind.
- A hand-registered runner with Pitcrew's prefix but not in the list, and one in neither: both survive every change and
  uninstall.
- Grow after a crashed register (the `.runner` exists, the list has no entry): adopted by name and id, not registered twice.
- Label change while a job runs: the job finishes with the old labels; the new registration has the new ones.
- A positive control for each: removing the "only listed and prefixed" filter makes the hand-registered test fail (ER-33).

### Variant 0-E: ephemeral registration per job

Same controller, but each start registers a new runner with `--ephemeral` and the daemon removes its `.runner` after the job.
Gain: the runner's credentials are revoked when the job is assigned, so a job cannot reuse them. Cost: the reusable
registration token is read by the service on every job, the Gitea runner list churns, and a runner that dies before its job
leaves a row to delete (no cron deletes offline runners). The user's pool design (0) is the better first build; 0-E is a later
hardening if the kept credentials matter.

### Option 0b: one persistent high-capacity runner, Pitcrew shapes its jobs afterwards

The runner fetches up to `capacity` jobs; Pitcrew then finds each job's processes and slows the newest.

- **Finding a job's tree from outside.** Unix: each step leads its own process group, and jobs carry `RUNNER_TRACKING_ID`
  in their environment (ADR 0112), but macOS hides the environment of Apple-signed binaries (measured there), so bash
  steps are lost. Host-mode folders are per repository, not per task, so cwd does not separate two jobs of one repo.
  Windows: the runner's per-job Job Object is unnamed, so Pitcrew cannot open it to set limits.
- **CPU share.** macOS: nice 10 or `QOS_CLASS_UTILITY` for bulk work only (rule 52); a CI job is waited work, so normal
  level, and `taskpolicy -c background` is forbidden. Linux: `cpu.weight` per job cgroup (needs the tree). Windows: Job
  Object CPU rate (`JOB_OBJECT_CPU_RATE_CONTROL_*`, Windows 8+,
  [docs](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_cpu_rate_control_information)) on a
  job Pitcrew cannot open. Weights only reorder; memory pressure is unchanged.
- **Pause the newest job** (SIGSTOP/SIGCONT, the cgroup freezer, NtSuspendProcess): the runner itself keeps running, so
  Gitea's heartbeat is fine, but `timeout-minutes` keeps counting, network peers (registries, test servers, browsers)
  time out, and a paused process that holds a lock stalls everyone behind it (ER-43). Rejected.
- **A first-step gate** (today's `heavy mark`) needs every workflow to call it: not transparent.

0b costs about 5 to 8 agent-days (tree discovery, three OS adapters, tests) and controls less, because the job has already
started when Pitcrew learns of it. **Option 0 is recommended over 0b.**

**Option 0c, the cheap fallback:** N persistent `capacity: 1` runners with `health_check.enabled` and `script: throttle admit
--probe` (non-zero when throttle would not admit). The upstream hook, no controller, 1 to 2 agent-days. Advisory: the probe
holds no slot, idle runners can all probe yes at once, and the result is cached for `interval`.

### What would make option 0 fail, and the fallback

| Failure | How WP-0 sees it | Fallback |
|---|---|---|
| `--once` missing or ignored in v5.4.1 or v6.0.1 | `daemon --help`; the runner does not exit after one job | 0c (health-check script) |
| Jobs for offline-only labels are not kept waiting on 28.4.0 | a test job with a label whose runners are all stopped | keep one runner of each label set started (the standby of step 1) |
| The jobs API does not list queued jobs with labels (ADR 0021's open questions) | API answer against a queued test job | the standby runner per pool |
| Start over about 10 s to first fetch | 10 timed starts per OS | lower `fetch_interval`, or keep one standby started |
| Our server's `app.ini` adds an offline-runner cleanup | `GET` the registrations after a day offline | re-register on demand from the list (grow path) |

## Part 2. Steps nobody queues: shells, agents, IDEs

Throttle stays a black box behind its binary. These options cap or queue work that does not come through Pitcrew's runners.

### 1. Linux: cgroup v2 slices

`CPUWeight=` (1 to 10000, default 100), `CPUQuota=` (a percentage of one CPU), `MemoryHigh=` (throttles and reclaims;
"the main control") and `MemoryMax=` (OOM inside the unit)
([systemd.resource-control](https://www.freedesktop.org/software/systemd/man/latest/systemd.resource-control.html)).
How work lands there transparently: runners, by Pitcrew starting them in a `throttle.slice` (`systemd-run --user --scope
--slice=throttle.slice`, or `Slice=` on the runner unit); Docker containers, by dockerd's `cgroup-parent` (default
`system.slice` with the systemd driver; "should be a slice name";
[dockerd](https://docs.docker.com/reference/cli/dockerd/)), otherwise a container escapes the caller's slice; login shells
and agents, only by a limit on the whole `user-<uid>.slice` (a drop-in), since logind puts each session in its own scope.
Not verified: which controllers the user manager gets by default (the page does not say; `cpu` delegation is needed for a
user-level slice). Cost: 1 to 2 agent-days. Gap: it caps, it does not queue; nothing waits in line.

### 2. Windows: Job Objects

Pitcrew already holds each runner in a kill-on-close Job Object; adding CPU rate control (weight or hard cap) and
`JOB_OBJECT_LIMIT_JOB_MEMORY` is one `SetInformationJobObject` call each. Children join the job automatically; breakaway
needs the job's permission; nested jobs work since Windows 8 and a child job's limits "cannot be less restrictive" than its
parent's ([nested jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs)). A login shell or an IDE is
not in any job Pitcrew owns; putting them in one would mean launching them through a Pitcrew wrapper, which is not
transparent. Rate control does not work under Remote Desktop with DFSS on (same CPU-rate page). Cost: 0.5 to 1 agent-day
for runners. There is no load average on Windows, so throttle's admission there is memory and CPU idle only (as today).

### 3. macOS: no cgroups

- **(a) PATH shims** for `cargo`, `docker`, `npm`/`npx` test runners and `playwright`: a shim folder first on PATH; each shim
  runs `throttle run -- <real tool>`. The real tool is found by walking PATH and skipping every entry whose canonical path is
  the shim folder (compare by file identity, not by name), so a shim never finds itself. Nesting: throttle sets
  `THROTTLE_HELD` (today `HEAVY_HELD`) for the child, and a shim that sees it execs the real tool at once (cargo in a build
  script, `cargo xtask`, npm scripts calling npx), otherwise a child waits for its parent's slot (ER-43). Gaps: absolute
  paths (`~/.cargo/bin/cargo`, rust-analyzer, Makefiles naming `/usr/local/bin/...`) bypass it; GUI-launched IDEs do not read
  the shell's PATH (launchd's user PATH is set by `launchctl config user path`, applied after a reboot); a whole `cargo test`
  takes one slot however many threads it spawns. Opt-out: `THROTTLE=off` for one command, or drop the folder from PATH.
  Cost: 2 to 3 agent-days.
- **(b) A machine-wide jobserver over a named FIFO** (`MAKEFLAGS=--jobserver-auth=fifo:PATH`, GNU make 4.4's FIFO style; the
  manual page returned HTTP 429 and was not re-read). The `jobserver` crate parses `fifo:PATH` in 0.1.26 (vendored source)
  and 0.1.34 (docs.rs source); the newest is 0.1.35. Cargo passes `CARGO_MAKEFLAGS` to build scripts and sets it "only when
  Cargo detects the existence of a jobserver" ([cargo env](https://doc.rust-lang.org/cargo/reference/environment-variables.html));
  that cargo and rustc read a FIFO jobserver from `MAKEFLAGS` at top level is read, not measured. GUI apps would get the
  variable from `launchctl setenv` ("on all future processes launched by launchd in the caller's context", `launchctl(1)`),
  which a login agent must repeat after every boot. **Rejected:** a token is a byte read from the pipe; a process killed while
  holding one loses it for good, and no one can tell a held token from a lost one (a lock that cannot release on death, against
  ER-51 and ER-33); every client also gets one implicit token, so N cargos run at least N jobs; and only jobserver-aware tools
  (make, cargo, rustc) obey it, not nextest's test processes, Playwright workers or Docker.
- **(c) taskpolicy / QoS.** `taskpolicy -c utility|background|maintenance` sets a QoS clamp, `-b` sets `PRIO_DARWIN_BG`, and
  `-m` sets a memory limit in MiB (`taskpolicy(8)` on macOS 27.2). Rule 52 allows only utility QoS or nice 10, and only for
  bulk work; CI jobs and agents' builds are waited work, so they stay at the normal level. It reorders, it does not admit.

### 4. Hybrid

OS caps where Pitcrew owns the parent (Windows Job Object, Linux slice and `cgroup-parent`), plus shims and the jobserver
everywhere. The jobserver half is rejected above; the shim half is the only transparent queue for interactive work on macOS.

### Gaps common to all of Part 2

Containers on macOS run in OrbStack's VM (its own kernel): host QoS and shims see one VM process, so the cap is OrbStack's
own `cpu` (multiples of 100%) and `memory_mib` (8 GB at most by default)
([OrbStack settings](https://docs.orbstack.dev/settings)), plus throttle's "a container run is alone" rule for runs that go
through it. A run started outside throttle is seen only through the load it causes (as today).

## Recommendation for Pitcrew

1. **Option 0** for CI jobs: a pool of pre-registered runners, one per throttle slot, each started with `--once` when the
   throttle crate admits a job, with reconciliation for settings changes. 0-E later if kept credentials matter; 0c if `--once`
   fails on our versions.
2. **OS caps on what Pitcrew starts**: Windows Job Object CPU rate and memory, Linux `throttle.slice` and dockerd's
   `cgroup-parent`. No caps on the user's own sessions by default.
3. **No jobserver, no background QoS.** Shims stay a later, opt-in Pitcrew feature for interactive work, built only if a
   measured stall from unthrottled interactive builds names the need.

**Adds:** the controller loop and one-job runner lifecycle; the pool's registrations and reconciliation; two secrets in the
service (registration token, API token); slot-to-folder affinity in Pitcrew; Job Object limits and a slice. **Removes:** fixed
`runner.capacity` per runner (Settings shows the pool's maximum instead); `heavy mark` in `ci isolation acquire` and
`release` and its CI priority marker (ADR 0102 §5); the hand-set "capacity 4" of ADR 0120; ADR 0021's separate pool
start/drain law (folded into admission); slot affinity in `paddock-dev` (moved to Pitcrew, once Paddock's runners use it).
The throttle crate gains no new mechanism: Pitcrew is one more caller of the same admission.

**What stays manual:** creating and rotating the two tokens; choosing a pool's labels and maximum; agents and shells keep
calling `throttle run` explicitly (Paddock's tools do); containers started outside throttle; OrbStack's VM limits.

## Work packages (wide-range guesses; ER-37)

| WP | What | Agent-days |
|---|---|---|
| WP-0 | Spike on each OS against Gitea 28.4.0 with one hand-made test registration: `daemon --once` exits after one job (v5.4.1, v6.0.1), a job whose runners are all offline stays waiting, the jobs API lists queued jobs with labels, 10 timed starts to first fetch, a killed runner's task stops after `ZOMBIE_TASK_TIMEOUT` | 0.5 to 1 |
| WP-1 | Throttle crate: in-process `admit()` returning a held slot (the File::lock prerequisite), `THROTTLE_HELD` pass-through | 0.5 to 1 |
| WP-2 | `pitcrew-core`: pure controller (demand, admission, start, stop-when-idle, adopt) and the reconciliation planner (desired against actual, owned-only filter); tests with a fake clock and fake Gitea, including the test plan above | 2 to 2.5 |
| WP-3 | `pitcrew-service`: register and delete through the stored tokens (credential store, token file, scrubbed environment), per-slot config and folders, start, hold, end, adopt after restart, idempotent reconciliation steps | 2 to 3 |
| WP-4 | Windows Job Object CPU rate and memory; Linux slice and `cgroup-parent`; macOS start as the runner user | 1 to 1.5 |
| WP-5 | Remove `heavy mark` from `ci isolation`, move slot affinity into Pitcrew, Settings and docs, a Pitcrew ADR (amends Pitcrew 0021 and family ADR 0102) | 1 |

Total about 7 to 10 agent-days. Common practice (ARC: a listener scales ephemeral runners to job demand,
[ARC](https://docs.github.com/en/actions/concepts/runners/actions-runner-controller)) is the same controller without the
host-load step; Gitea has no just-in-time registration token, hence the pre-registered pool. The extra over common practice is
WP-1's admission call, under 1 agent-day. Cheaper options: 0c (1 to 2 days) or today's static capacity (0 days). GARM
(Apache-2.0) manages Gitea runners through cloud and container providers, not host-mode macOS or Windows (Pitcrew ADR 0021's
branch, option D).

## Decisions for the user

- **D1.** Option 0 (pre-registered pool, `--once`) as the first build, with 0-E as later hardening; or 0c only.
- **D2.** Token scope: the org's tokens (expected) or one repository's.
- **D3.** Interactive work on macOS: leave it to explicit `throttle run` (recommended) or build the shims now.

## Sources

- Gitea runner registration: <https://docs.gitea.com/runner/registration/>
- Gitea configuration cheat sheet (Actions cron tasks, `[actions]` timeouts): <https://docs.gitea.com/administration/config-cheat-sheet>
- Gitea token scopes: <https://docs.gitea.com/development/oauth2-provider>
- Offline-only labels: <https://forum.gitea.com/t/setting-up-runner-for-gitea-actions/9036>, <https://gitea.psi.ch/gitea/gitea/commit/4ea522fecfd8f2d6b51466cb9ab6772f4594bf2e>
- gitea/runner v5.4.1: `register.go`, `daemon.go`, `poller.go`, `runner.go`, `config.example.yaml` at
  <https://gitea.com/gitea/runner/src/tag/v5.4.1>
- GitHub ARC: <https://docs.github.com/en/actions/concepts/runners/actions-runner-controller>
- Windows nested jobs: <https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs>
- Windows CPU rate control: <https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_cpu_rate_control_information>
- systemd resource control: <https://www.freedesktop.org/software/systemd/man/latest/systemd.resource-control.html>
- dockerd `cgroup-parent`: <https://docs.docker.com/reference/cli/dockerd/>
- OrbStack settings: <https://docs.orbstack.dev/settings>
- Cargo environment variables: <https://doc.rust-lang.org/cargo/reference/environment-variables.html>
- jobserver crate source: <https://docs.rs/crate/jobserver/0.1.34/source/src/unix.rs>
- GNU make jobserver (not re-read, HTTP 429): <https://www.gnu.org/software/make/manual/html_node/POSIX-Jobserver.html>
- `launchctl(1)`, `taskpolicy(8)`: the man pages on macOS 27.2
