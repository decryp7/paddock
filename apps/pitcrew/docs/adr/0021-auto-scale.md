# 21. Auto-scaling runners: a pool of pre-registered slots, started and drained from the queue and the host's load

- Status: Proposed (2026-10-28). The owner decided D1-D8 on 2026-10-28 (see "Decisions" at the end). An independent
  review on 2026-10-28 asked for changes (busy for container runners, the family ADR 0025 gate, the shared CI home, and
  P2 points); they are addressed below. It stays Proposed until a second review and WP-0's findings are in.
- Amends: ADR 0005 (the service holds its own read-only API token). Interim step: the runbook
  [add-runners.md](../runbooks/add-runners.md).
- Date: 2026-10-28
- Related: ADR 0005 (credential storage; this ADR amends it), 0006 (supervisor, "between jobs"), 0011 (runner config
  editing), 0013 (holds: low-disk guard and pause windows), 0014 (update deadline), 0015 (capacity advice, which this ADR
  turns into action); family rules 13, 21, 41, 42, 46; ER-24; family ADRs 0019 (limits explain themselves), 0020/0024
  (background work: no polling without a declared check), family ADR 0021 (portable tests: `Clock`/`FakeClock`), family
  ADR 0025 (CI job isolation: kept folders per slot, a gate for this ADR)
- Code today: `pitcrew_core::{controller, hold, pause, disk_guard, insights, gitea_api, runner_settings, config}`,
  `pitcrew_service::{supervisor, probe}`, `app/src-tauri/src/{gitea.rs, credentials.rs}`

## Context

**The problem.** Each machine (the Mac mini, the Windows VM, the Linux host) runs one Gitea runner with
`runner.capacity: 1`. CI queues are the bottleneck: 14 or more jobs wait while the runners of the other systems are
idle. The owner wants Pitcrew to watch the Actions queue and the host's load, RAM and free disk (ER-24, rule 41) and to
run more or fewer jobs at once between a minimum and a maximum, with quiet hours and a low-disk pause; ephemeral
per-job runners come later.

**What Pitcrew has today** (read in the code on 2026-10-28):

- One supervisor thread with a pure `Controller` per runner; stop, restart and **drain happen only between jobs**
  (`controller.rs`, `Input::Drain` → `between_jobs(Pending::Pause)`); a job runs while the runner has child processes
  and for 20 s after the last one (ADR 0006).
- `HoldTracker` (`hold.rs`) drains a runner for a `HoldReason` (`LowDisk`, `Schedule`, `Busy`) and resumes it when no
  reason is left; it never overrides a pause the user made (ADR 0013).
- Up to `MAX_RUNNERS = 16` runners per machine (`config.rs`), each with its own folder, `.runner`, logs, metrics and
  watchdog. `runner.capacity` is editable (1 to `MAX_CAPACITY = 64`, `runner_settings.rs`) and applied by a restart
  after the job (ADR 0011).
- The Gitea API is read **by the app**, with a read token kept in the logged-on user's credential store; "the service
  never sees it" (ADR 0005, ADR 0015). Insights already advise "add a runner or raise `runner.capacity`" from waits and
  busy share, but nothing acts on it.
- **"Busy" is only right for host-mode runners.** A job counts as running while the runner has child processes, plus
  20 s (`jobs.rs`, `probe.rs` `has_job_children`). A container job's processes are children of the Docker daemon, not
  of the runner, so a busy Linux container runner looks idle 20 s after its "task started" line. The runner logs
  nothing when a job ends (`jobs.rs`, checked with 5.2.0 on 2026-10-22; still true of the Mac's log on 2026-10-28,
  which has `task started` lines and no finished line). This already affects today's drain and updates on container
  runners; the scaler must not build on it (section 2).
- **Stops already cancel jobs.** The Mac runner's log (counted on 2026-10-28, contents not copied) has 9 "shutdown
  initiated" lines and 9 "cancelled in progress jobs during shutdown" lines; the line does not say whether a job was
  actually running. With `shutdown_timeout: 0s`, any running job is cancelled (section 4).
- **The CI kit is not ready for two jobs on one host.** Host jobs keep folders under one CI home per host
  (`${PADDOCK_CI_HOME:-~/.paddock-ci}`: the stable checkout, build folders, Angular, npm and tool caches), several files
  hard-code CI-home paths (for example `setup-rust/action.yml`, `ci.yml`, `debrief.yml`, `windows-node-modules.sh`, the
  demo release workflows), `host-cleanup` kills test processes by exact name or by patterns anchored on `.paddock-ci/`,
  and sccache uses one fixed port. Two runs of one job on one host break each other. Containers on one Linux host share
  the runner's `/sccache` mount (the Angular cache among them). Family ADR 0025 (CI job isolation: kept folders per
  slot behind an OS file lock in Rust, `PADDOCK_CI_KEEP_SLOTS`, declared ports, scoped clean-up; Proposed, not merged on
  2026-10-28) is the fix. **This ADR is gated on it** (section 1).
- The service does not use the shared crates yet: its HTTP is its own `ureq` (`net.rs`), its host readings its own
  `sysinfo` probe (`probe.rs`). `paddock-net` (egress policy), `paddock-secrets` (credential port, redacted `Secret`,
  owner-only file fallback), `paddock-metrics` (machine sample) and `paddock-jobs` (`Clock`) exist in `crates/` (rule 42).

**What Gitea offers** (verified 2026-10-28 against the owner's server, which reports `{"version":"28.2.0"}` at
`/api/v1/version`, and its `swagger.v1.json`; runner docs 5.x on docs.gitea.com):

| Need | Endpoint or fact | Verified how |
|---|---|---|
| Queued jobs | `GET /api/v1/{admin,user}/actions/jobs`, `/orgs/{org}/actions/jobs`, `/repos/{o}/{r}/actions/jobs` with `status` (`requested, pending, queued, in_progress, failure, success, skipped`), `page`, `limit`, `sort=id`, `order`; answer `{jobs:[…], total_count}` | swagger of the server |
| A job's labels | `ActionWorkflowJob` has `labels` (the job's `runs-on`), `status`, `created_at`, `started_at`, `head_branch`, `run_id`, `runner_name` | swagger |
| Runners | `GET …/actions/runners` → `{runners:[{id,name,status,busy,disabled,ephemeral,labels}], total_count}` | swagger |
| Server-side disable | `PATCH …/actions/runners/{id}` with `{disabled}` | swagger |
| Registration token | `POST …/actions/runners/registration-token` at admin, org, user and repo level | swagger |
| Token scopes | `/admin/*` needs `read:admin` (site admins only), `/user/*` `read:user`, `/orgs/*` `read:organization`, `/repos/*` `read:repository`; there is no Actions-specific scope | [docs: token scopes](https://docs.gitea.com/development/oauth2-provider) |
| Capacity | `runner.capacity`: "Maximum number of jobs that can run at once", default 1 | [runner configuration](https://docs.gitea.com/runner/configuration/) |
| Config reload | "The runner reads its configuration only at startup. Restart it after each change." | same page |
| Stop with a job running | `runner.shutdown_timeout`: "Time to let running jobs finish before canceling them during shutdown", **default `0s`** | same page |
| Registration | a token "can register any number of runners until it is reset"; each runner needs its own `.runner` ("a second process using the same file refuses to start"); `--token-file` or `GITEA_RUNNER_REGISTRATION_TOKEN` keep the token off the command line | [runner registration](https://docs.gitea.com/runner/registration/) |
| Ephemeral | `--ephemeral`: "accepts exactly one job, then exits", credentials revoked once the job is assigned; per-job starts listen to the `workflow_job` webhook; `daemon --once` exits after one job but keeps its credentials | same page |
| Label matching | a job goes to "a runner that has the labels specified in the job's `runs-on`"; whether *all* must match is not stated | [runner labels](https://docs.gitea.com/runner/labels/) |

Not verified (needs a token, WP-0): which repositories `/user/actions/jobs` covers; whether a job waiting on `needs` is
listed as `queued` or only `pending`/`requested`; whether `head_branch` carries the tag name for a tag-triggered run;
what `busy` means for a runner with capacity above 1; that matching is "all labels of `runs-on`" (assumed from Gitea's
source, not checked).

**Measured** (synthetic, [`2026-10-autoscale-sim.py`](../research/2026-10-autoscale-sim.py); `python3 -I` it, with
an optional contention factor, to reproduce): 8 simulated hours, a push every ~10 min fanning out to 1-8 jobs of ~6 min
(lognormal), 30 seeds, 5 s steps. Without contention (the first table) jobs on one host do not slow each other, so the
table is an **upper bound**. In the second table each extra job running at once makes every job 25% slower (factor
0.25), which is closer to parallel compiles sharing cores and disk:

| Strategy | Mean wait | p95 wait | Slot-hours on | Start/stop actions |
|---|---|---|---|---|
| Static, 1 slot (today) | 171 min | 326 min | 9.0 | 0 |
| Static, 4 slots | 9.3 min | 26 min | 36.0 | 0 |
| Auto 1-4, with hysteresis (20 s up, 10 min down, 15 s between starts) | 9.7 min | 26.5 min | 29.2 | 40 |
| Auto 1-4, no hysteresis | 9.7 min | 26.4 min | 24.5 | 92 |

| Contention 0.25 | Mean wait | p95 wait | Slot-hours on | Start/stop actions |
|---|---|---|---|---|
| Static, 1 slot | 171 min | 326 min | 9.0 | 0 |
| Static, 4 slots | 58 min | 116 min | 36.0 | 0 |
| Auto 1-4, with hysteresis | 59 min | 117 min | 34.1 | 11 |
| Auto 1-4, no hysteresis | 59 min | 117 min | 33.2 | 22 |

Two things follow. First, almost all of the gain is **more slots**, not cleverness: four slots cut the mean wait from
171 minutes to between 9 (no contention) and 58 (contention 0.25); the real figure depends on how much parallel jobs slow
each other on the host, which WP-0 measures with two concurrent jobs. Second, an idle slot is cheap: the owner's idle runner on this Mac had an RSS of 37 MB and 0.0% CPU
(`ps`, 2026-10-28; RSS, not rule 26's footprint, so indicative only). What costs the machine is **running jobs**. So the
scaler's real job is admission control: let up to *max* jobs run while the host has room, take extras away when the
host is loaded (the 2026-10-22 evening when agents plus CI drove the Mac to a load of 150-240), and keep idle extras off
when the queue is empty. Hysteresis costs ~20% slot-hours and halves the start/stop churn; worth it, because each start
or stop is a process, a log entry and, for a stop, a race with job pick-up (below).

**What scaling cannot do.** Pitcrew cannot send a Linux job to an idle Mac. A host can only take jobs whose labels it
serves; jobs for `ubuntu-latest` (49 of the monorepo's `runs-on` lines) wait for runners that carry that label. "14
queued while others idle" is partly a **label topology** question (D3: not addressed for now), not a capacity one.

## Options

**A. Pool of pre-registered slots, started and drained (recommended).** The owner registers a few runners per label set
once ("slots": `mac-1` … `mac-4`), each with its own folder, `.runner` and a static capacity (default 1). Scaling up is
starting a stopped slot; scaling down is draining an idle one between jobs (the existing path). No registration token is
needed at scale time, a busy runner is never restarted, every slot keeps Pitcrew's per-runner watchdog, logs, metrics
and updates. Cost: N registrations in Gitea (stopped ones show offline), N folders (cache shared via the Actions cache
server, ADR 0016), `MAX_RUNNERS` caps a host at 16.

**B. Scale `runner.capacity` of one runner.** One process; but the runner reads capacity only at start, and Pitcrew
restarts only when the runner is fully idle. Under load (exactly when more capacity is wanted) a runner with one job
running cannot be raised; lowering has the same problem. It also mixes several jobs in one runner's job tracking and
logs. Rejected as the control knob; capacity stays a static per-slot setting the owner may raise by hand.

**C. Ephemeral per-job runners.** A fresh runner registered per job, gone after it: clean state, natural isolation, the
base for "build on the host, test in a VM". Needs a reusable registration token inside the service (anyone holding it can
register a runner that receives jobs and their secrets), a registration per job (seconds of latency), and either inbound
`workflow_job` webhooks (a listener on each host, against ADR 0004's local-only IPC) or polling. Deferred to a later
phase with its own amendment; the pool's ports are shaped so that a slot can later be "ephemeral" (WP-E).

**D. An external autoscaler (GARM).** [GARM](https://github.com/cloudbase/garm) (Apache-2.0) manages ephemeral runners
for GitHub and Gitea through providers (LXD, Incus, Kubernetes, clouds). None of them is a macOS or Windows host
running host-mode jobs, it knows nothing of the host's load, and it is a second Go service to run. Rejected.

**E. Static capacity only.** Register 3-4 slots, run them all, and let ADR 0013's `when-idle` hold keep the base runner
out of the way. Simple and gets most of the wait reduction, but runs up to max jobs into a loaded host, which is the
failure rule 41 exists for. Chosen as the **interim step** (decision D8), within the same gate: Linux container runners
now, host-mode runners on the Mac and Windows only after family ADR 0025.

## Decision (proposed)

### 1. Pools and slots

`[[autoscale.pools]]` names a set of **slots**: runners already in `pitcrew.toml`, same server, same label names. A pool
has `min` and `max` running slots (`0 <= min <= max <= slots`). Slots are ordered; the scaler starts the lowest stopped
slot and drains the highest idle one, so `mac-1` is the stable base and logs and caches concentrate in the low slots. A
runner in no pool is never touched by the scaler. A slot is **available** to the scaler only when it is enabled, not in
a crash loop, not updating (ADR 0002/0014), not held (ADR 0013), not registration-broken, and not under a user's manual
action (as with holds: if the owner starts, stops, pauses or resumes a slot by hand, the scaler leaves it alone until
the owner hands it back with "Let auto-scale manage"). Held slots stay held: the scaler never resumes what a hold drained.

**The family ADR 0025 gate.** A pool may run more than one job at once on a host only when the CI kit isolates
concurrent jobs. A pool declares `ci_isolation = "none" | "slots"`. With `"none"` (the default) its effective `max` is 1
and the pool page says why ("two jobs on one host share the CI home; needs family ADR 0025"). The owner sets `"slots"`
only once family ADR 0025 is merged **with its Rust slot lock** (the shell lock it replaces has a reclaim race) and, for
container pools, once containers get slots on the shared mount (ADR 0025 §7). Pitcrew cannot see the CI kit, so this is a
declared precondition, not a detection. With `"slots"`:

- **One CI home per host.** Every slot of the host shares the host's CI home; ADR 0025 derives the kept folders and port
  blocks from the slot number, so separate homes per runner would only multiply disk. Pitcrew's config check rejects
  `runner.envs.PADDOCK_CI_HOME` on a pool slot (with a Fix that removes it: the migration from the interim runbook).
- **`PADDOCK_CI_KEEP_SLOTS` = the pool's `max`,** written into every slot's `runner.envs` through ADR 0011's editor, so
  the kept folders exist for at most as many slots as the pool can run.

### 2. Signals

| Signal | Source | Freshness | Unknown means |
|---|---|---|---|
| Queued jobs per pool | `GET <scope>/actions/jobs?status=queued&limit=50&sort=id&order=asc`, at most 4 pages (200 jobs); a job counts for a pool when its `labels` are all among the pool's label names | 15 s poll, backoff to 120 s | no scale-**up** |
| Idle runners elsewhere | `GET <scope>/actions/runners`: online, not busy, not disabled, serving the job's labels, not one of this host's slots | same poll | count 0 (overshoot, bounded by max) |
| Local jobs per slot | **host-mode slots:** the `JobTracker` (child processes + 20 s). **Container slots:** a "task started" line opens a job; it stays open while a job container of that task exists (the Docker query and the container naming are WP-0) **and**, when the queue is known, while the API's `runners[].busy` for that runner is true | each job start/end, at once; the API with the poll | **busy** (never drained) |
| Load | 1-minute and 5-minute load average per core (macOS, Linux); CPU share on Windows (no load average) | 5 s probe tick | no scale-up |
| Memory | available memory (`paddock-metrics`): Linux `MemAvailable`; Windows `ullAvailPhys`; macOS free + inactive + purgeable + speculative pages (`host_statistics64`), i.e. what can be had without compressing or swapping; WP-3 checks it against Activity Monitor's memory pressure | 5 s | no scale-up |
| Free disk | ADR 0013's guard (`SpaceCache`, ≥ 60 s) | as today | as today (holds) |
| Quiet hours | ADR 0013's pause windows | as today | as today |

A slot is **idle** only when every source that is available says so; a source that is unknown counts as busy.
Scale-down, shed and the manual drain all use this one definition.

**Release jobs** are queued jobs of a run started by a tag: the run's `event` is a tag push (the run endpoint, already
parsed by `gitea_api::parse_run`) **and** the job's `head_branch` matches an anchored pattern of `release_patterns`
(defaults `^[a-z][a-z0-9-]*-v\d+\.\d+\.\d+$` for app tags and `^v\d+\.\d+\.\d+$` for the shared packages; Rust's
`regex`, linear time, length-capped). A branch that happens to be named like a tag does not count. What `head_branch`
and `event` hold for a tag run is WP-0.

### 3. Control law (pure, per tick, per pool)

```
busy      = slots of the pool running a job (local truth)
demand    = queued jobs for the pool − idle runners elsewhere for them (floor 0)
want      = clamp(busy + demand, min, max)
want      = max(want, busy)                              -- never below what is running
if quiet hours:      want = max(busy, min(want, quiet_max), release_need)
ramping   = slots started within ramp_s (180 s) that are not yet at full load
host room (start):   (load1 + ramping × job_cores) / cores < start_load AND load5/core < start_load AND
                     available memory − ramping × slot_mem ≥ slot_mem + mem_reserve
host shed:           load1/core > shed_load for shed_after
release_need = busy + queued release jobs (capped by max)
```

- **Up** when `want > running` for `up_after` (20 s), at most one start per host every `start_gap` (60 s), only with
  host room. A job needs 2-3 minutes (checkout, toolchain, the first compile) before it loads the host, so the load
  average lags; the room check therefore counts slots started in the last `ramp_s` at their expected cost (`job_cores`,
  default 4; `slot_mem`), instead of trusting the load average alone, except `release_need`, which ignores the load ceiling and quiet
  hours but never the low-disk hold, the memory reserve or `max`.
- **Down** only through idle slots: a slot idle for `down_after` (10 min) above `want`, one per `down_gap` (2 min).
  Under **shed**, extras above `max(min, busy)` that are idle are drained at once (no `down_after`); a busy slot is
  never touched, its job finishes. Shed never drains a slot below `min`; below `min` the existing `when-idle` hold is
  the owner's tool.
- **Scale-down needs no API.** A slot that has been online and idle for `down_after` is itself evidence that nothing it
  could take is queued (it polls the server every few seconds). So scale-down runs on local facts only; only scale-up
  needs the queue.
- **Thrash guard.** More than 6 direction changes of a pool in an hour doubles its `down_after` for the next hour (cap
  60 min), recorded as a reason. A property test bounds direction changes for any input sequence.
- **Cooldown after start.** A started slot counts as available capacity for `up_after` + 30 s even before it picks a
  job, because the queue count lags the runner's first poll; this keeps two ticks from starting two slots for one job.

The planner is `fn plan(policy, inputs, state, now) -> Plan { actions, explanation, state }` with time passed in; actions
are `Start(slot)` and `Drain(slot)`. The service carries them out through the existing controller inputs (start; drain
between jobs) and never decides anything itself.

### 4. Never stop a runner mid-job

Pitcrew already stops only between jobs, but there is a race the scaler would make frequent: between "the slot is
idle" and the stop signal, the runner may fetch a task, and with `runner.shutdown_timeout: 0s` (the documented default)
a graceful stop **cancels** it. Decision: every pool slot's `config.yaml` gets `runner.shutdown_timeout` equal to its
`runner.timeout` (default 3h), written through ADR 0011's editor when the slot joins a pool, and checked on every tick
(a slot without it is not available and the pool page says why). The stop then waits for that job. The same race exists
for today's manual drain, stall restarts and updates; the fix applies to them too (the label check of ADR 0011 gains a
warning for `shutdown_timeout: 0s`). Both defaults are confirmed in `config.reference.yaml` beside the Mac's runner
(`#timeout: 3h`, `#shutdown_timeout: 0s`, "Any running jobs that haven't finished after this timeout
will be cancelled"). Pitcrew's own side already fits: the controller terminates a runner after its stop timeout "only
while no job runs" (`controller.rs`), so a slot that took a job during the stop is left to finish it, provided "a job
runs" is right for the runner kind (section 2: container slots). The installed binary on the Mac reports v5.2.3
(`gitea-runner --version`; Pitcrew's `version.txt` beside it still said 5.2.0).

**Cost of a 3 h `shutdown_timeout`.** A stop that catches a just-taken job now waits for that job (up to 3 h) instead
of cancelling it. Restarts, config changes and updates of that runner wait as well, as they already do for a job Pitcrew
sees (updates drain for up to `drain_secs` = 3 h, ADR 0014's deadline applies). The card shows *Stopping (waiting for the
job)*. How the graceful stop behaves on Windows (Ctrl+Break through Pitcrew's helper, ADR 0006) with a long
`shutdown_timeout` is WP-0.

Server-side `disabled` before the stop would close the race fully but needs a write token (`write:user`/`write:admin`)
in the service. Not chosen (decision D7).

### 5. Where the code lives

- **Core (`pitcrew_core::autoscale`, pure):** `QueueSnapshot::parse` and `FleetSnapshot::parse` (bounded by
  `MAX_API_BODY`, never panic, `parse don't validate`: label names become `LabelName`s once); `PoolPolicy` (validated
  settings); `SlotView` enum (`Stopped`, `Starting{since}`, `Idle{since}`, `Busy{release: bool}`, `Unavailable(reason)`)
  so "busy and stopped" cannot be expressed; `HostRoom`; `plan`; `Explanation` (reasons as an enum, words in one place);
  the thrash guard. Coverage ≥ 99%, proptests: never a `Drain` of a busy slot, never more than `max` running, never a
  start without host room unless for a release, never panics on any API body.
- **Ports (in the service):** `QueueSource` (`fetch() -> Result<(QueueSnapshot, FleetSnapshot), QueueError>`),
  `HostLoad` (`sample() -> HostRoomInputs`), and the supervisor's existing controller inputs. Fakes for tests.
- **Adapters:** the Gitea adapter on `paddock-net`'s `Egress` (allow-list = the pool's server host only, HTTPS, plain
  HTTP only to this computer, body cap, timeouts) on a worker thread that posts results to the supervisor's message loop
  (ADR 0006: no locks); host load through `paddock-metrics` (`MachineProbe`), which gains a load average and available
  memory (a shared change, rule 42: Dyno and Pitwall want the same); time through `paddock-jobs`' `Clock`.
- **App and UI:** settings form, status and history only. The UI never computes a target (AGENTS.md "Where logic lives").

### 6. The poll (ADR 0024 §2)

The queue poll is a declared ADR 0020 mode-2 check: interval 15 s, ×2 backoff up to 120 s while nothing is queued for
any pool and no slot is busy, **demand**: only while auto-scale is on and a pool exists, **wake-up**: a local job start
or end re-plans at once without an API call. Two requests per poll (jobs, runners) at 15 s are trivial for the server.
No webhooks: they would need a listener on every host.

### 7. Settings (rule 21)

New optional `[autoscale]` keys in `pitcrew.toml` (no `SCHEMA_VERSION` bump: optional with defaults), validated in the
service like the rest; an invalid section is ignored with a message and auto-scale stays off. Settings > Auto-scale:

| Key | Default | Meaning |
|---|---|---|
| `enabled` | `false` | off until the owner turns it on |
| `scope` | `user` (D2: the runners are registered at user level, `decryp7`); also `org:<name>`, `instance`, `repo:<o>/<r>` | where the queue is read |
| `poll_seconds` | 15 (10-300) | the check's interval |
| `pools[].slots`, `min`, `max`, `quiet_max` | -, 1, slots, `min` | per pool |
| `start_load_per_core` | 0.8 | ≈ 11 on 14 cores, inside rule 41's 6-16 window |
| `shed_load_per_core`, `shed_after_s` | 1.2, 120 | ≈ 17 on 14 cores |
| `windows_start_cpu_percent`, `windows_shed_cpu_percent` | 70, 90 | Windows has no load average |
| `slot_mem_mb`, `mem_reserve_mb` | 4096, 4096 | room needed to start one more job |
| `up_after_s`, `start_gap_s`, `down_after_s`, `down_gap_s` | 20, 60, 600, 120 | hysteresis and pacing |
| `ramp_s`, `job_cores` | 180, 4 | how long a new job takes to load the host, and how much it is assumed to use |
| `pools[].ci_isolation` | `"none"` | `"slots"` only after family ADR 0025 is merged with its Rust slot lock; `"none"` caps the pool at one job |
| `release_patterns` | `["^[a-z][a-z0-9-]*-v\\d+\\.\\d+\\.\\d+$", "^v\\d+\\.\\d+\\.\\d+$"]` | anchored tag patterns of jobs that are never starved (with a tag-push event) |
| `count_remote_idle` | `true` | subtract idle runners on other hosts |

Every tunable here is a Setting; there is no knowledge list, so no TOML pack. No LLM anywhere (rule 46).

### 8. Observability: why it scaled

Each change is a history event per slot (`ScaledUp`/`ScaledDown`, with the pool and the reason in words: "3 jobs queued
for macos-latest; load 0.4 per core"), a log line with counts only (no job names, repositories or URLs in the log), and
one entry in the pool's last-decisions ring (50, in memory). A pool that **wanted** more but was held back records the
reason once per change of reason, not per tick: "at max (4)", "load 1.1 per core above 0.8", "memory 3 GB free, needs
8 GB", "quiet hours until 07:00", "queue unknown: the server answered 401". The Dashboard gets an Auto-scale card per
pool (running / wanted / max, queued, last reason) and the Insights advice (ADR 0015) links to it. Notifications only for
conditions that need the owner (family ADR 0019, limits explain themselves): queue unknown for 10 minutes, at max with
jobs waiting longer than 15 minutes, the thrash guard engaged, a slot not available for a fixable reason (missing
`shutdown_timeout`, registration removed).

### 9. Failure modes

| Failure | Behaviour |
|---|---|
| API down, timeout, 401/403, 404 (old server), garbled or oversized answer | queue unknown: **hold**, no scale-up; scale-down continues on local idleness; status says why; 401/403 suggest the token's scope |
| Host metrics unknown | no scale-up (unlike ADR 0013's holds, where unknown must not *hold*; here unknown must not *add* load) |
| A slot fails to start or crash-loops | unavailable; the next slot is used; existing crash-loop notification |
| Registration removed on the server | the runner exits ("detects that Gitea has removed its registration"); slot unavailable; notification with "Register again" |
| Service restart | no scaler state persisted: running slots are adopted (ADR 0006), cooldowns start at the restart, so a restart cannot cause a burst |
| Flapping queue or load | `up_after`, `down_after`, gaps and the thrash guard; property-tested |
| Two hosts serving one label | each counts idle runners elsewhere; residual overshoot is idle slots, bounded by each host's `max` and drained after `down_after` |
| Owner acts by hand | the scaler leaves that slot alone until handed back |
| Low disk / quiet hours / when-idle | holds win; a held slot is never resumed by the scaler |

### 10. Security

- **A read token in the service (amends ADR 0005, decided D2).** The service must read the queue while the app is
  closed, so it gets its **own** token, separate from the app's. The runners are registered at user level (`decryp7`),
  so the token is a read-only `read:user` token (the queue at `/api/v1/user/actions/jobs`, the runners at
  `/api/v1/user/actions/runners`); no admin or write scope. It travels once, UI → app → `Request::SetAutoscaleToken { token: Secret }` → stored by the service through
  `paddock-secrets` in the service's own context (`Pitcrew auto-scale` / server URL): the user's Keychain for the macOS
  launch agent, the Secret Service or, opted in, the owner-only file fallback for the Linux user service, and the
  LocalSystem credential store on Windows (to verify in WP-0; fallback: the file store under the service's data folder,
  ACL SYSTEM-only). **Linux without a Secret Service** (a headless user session): auto-scale says "no credential store"
  and stays off; the owner-only file fallback is used only after the owner opts in. The UI can set, clear and ask whether
  it is set; it never gets it back. `Secret`'s `Debug` redacts; the adapter never logs URLs with query strings or
  headers; diagnostics bundles stay redacted.
- **A canary test** (WP-9): the tests set a recognisable fake token and assert that it appears in no log line, history
  event, notification, IPC reply, DTO, `state.json`, `pitcrew.toml` or diagnostics bundle.
- **Authorisation.** `SetAutoscaleToken` and `ClearAutoscaleToken` mutate (`Request::mutates`), so they pass ADR 0004's
  ACL like other settings changes; `HasAutoscaleToken` is a query. The service re-validates the token's shape and length.
- **The scope is confirmed, not assumed:** WP-0 checks that a `read:user` token reads `/user/actions/jobs` and
  `/user/actions/runners` (the scope table maps `/user/*` to `user`, but the Actions routes are not listed there).
- **No registration token at scale time.** Slots are registered once through the existing wizard, which gains "Add N
  slots": one token, passed to each `register` through `GITEA_RUNNER_REGISTRATION_TOKEN` in the child's environment
  or `--token-file` instead of the command line (fixes ADR 0005's "briefly visible on the command line"; both are in
  `gitea-runner register --help` of 5.2.3), never stored.
- **Egress** only to the configured server through `paddock-net`; response caps and timeouts; no redirects to another
  host.
- **Ephemeral runners** (later) would need a reusable registration token in the service: whoever holds it can register a
  runner that receives jobs and their secrets. That is a separate amendment with its own decision.

## Consequences

- More slots per host shorten the queue by a factor between about 3 and 18 in the simulation, depending on how much
  parallel jobs slow each other (to be measured in WP-0); the scaler keeps those jobs from overloading a host that is
  also running agents, and keeps idle extras off. None of it applies to host-mode pools before family ADR 0025.
- Cross-OS idleness is not solved by this ADR; it needs runners that serve the waiting labels (D3).
- More registrations and folders per host; Settings shows them as one pool. Disk: each slot has its own `work/`;
  housekeeping (ADR 0006) cleans stopped slots first. The CI home's kept folders grow with `PADDOCK_CI_KEEP_SLOTS` (family
  ADR 0025 bounds them per slot).
- The service gains a network client with a token. It is bounded, read-only and optional.
- `pitcrew-service` starts using `paddock-net`, `paddock-secrets`, `paddock-metrics` and `paddock-jobs::Clock`
  (rule 42), which also helps the planned move to family ADR 0021's shared test remedy.

## Risks

- **Unverified API semantics** (WP-0): if `needs`-blocked jobs show as `queued`, demand is overstated (bounded by max;
  extras go idle and are drained); if `/user/actions/jobs` does not cover org repos, the scope must be `org` or
  `instance`.
- **Label matching** assumed to be "all of `runs-on`"; if Gitea matches differently, pool demand is mis-assigned. WP-0
  checks with one job.
- **The stop race** depends on `shutdown_timeout` behaving as documented; WP-9 has a fake-runner test, WP-0 one real
  check.
- **Windows LocalSystem secret store** might not behave like a user vault; fallback named above.
- **The CI kit gate.** Until family ADR 0025 is merged with its Rust slot lock, two jobs on one host break each other
  (Context); a pool with `ci_isolation = "none"` runs one job at a time. Setting `"slots"` too early is the owner's
  risk; the pool page says what it relies on.
- **Contention**: even with isolation, parallel jobs share cores, disk I/O, sccache and the Actions cache server; the
  simulation shows the gain shrinking from 18× to 3× at a 25% slowdown per extra job. Load includes I/O wait on Linux,
  not on macOS, so the ceiling sees disk saturation late on the Mac.
- **Busy for container runners** depends on finding the task's containers; if WP-0 finds no reliable link, container
  slots are idle only when the API says not busy, and without the API they are never drained.

## Work packages (for the build, in order)

| WP | What | Owner | Size |
|---|---|---|---|
| WP-0 | Spike with a read-only token on the real server: `read:user` reads `/user/actions/{jobs,runners}`; `/user` vs `/admin` coverage; `queued` vs `needs`; `labels` = `runs-on`; `head_branch` and `event` on tag runs; `busy` for capacity > 1 and for a container runner; label matching; how a container job's containers are named or labelled per task (the busy rule); a graceful stop with `shutdown_timeout` on macOS, Linux and Windows (Ctrl+Break); two concurrent jobs on one host (the contention factor); LocalSystem credential store on the Windows VM. Results written into this ADR before Accept | researcher | 1 d |
| WP-1 | `pitcrew_core::autoscale`: types, parsers, `plan`, explanation, thrash guard; unit and property tests, ≥ 99% | implementer (spec'd here) | 2 d |
| WP-2 | `[autoscale]` config schema, validation, defaults; settings DTOs | implementer | 0.5 d |
| WP-3 | Shared: load average and available memory in `paddock-metrics`' `MachineProbe` (Windows: CPU share); tests in the crate | integrating agent (shared crate) | 1 d |
| WP-4 | Service adapters: Gitea `QueueSource` on `paddock-net`, token through `paddock-secrets`, the mode-2 check with backoff and demand, worker → supervisor messages | implementer | 1.5 d |
| WP-5 | Supervisor wiring: slot availability, the busy rule per runner kind (container slots: task line + containers + API), actions through the controller, manual-override hand-back, the `ci_isolation` gate, `shutdown_timeout`, `PADDOCK_CI_KEEP_SLOTS` and no per-runner `PADDOCK_CI_HOME` (check and Fix), history events, notifications | implementer, independent review | 2 d |
| WP-6 | Protocol: `SetAutoscaleToken`/`ClearAutoscaleToken`/`HasAutoscaleToken`, pool status in `Status`; `PROTOCOL_VERSION` bump; `dto.rs` and `api.types.ts` in the same commit | integrating agent | 1 d |
| WP-7 | Register wizard "Add N slots" (names, labels from the first slot, env-var token) | implementer | 1 d |
| WP-8 | UI: Settings > Auto-scale (Material), Dashboard pool card, reasons in Activity, mock backend | implementer | 1.5 d |
| WP-9 | Integration tests: fake runner + `FakeWire` Gitea + `FakeClock`: burst → up; API 401 → hold; load spike → no start, busy never drained; a container slot with no child processes but a running task (and the API saying busy) is never drained; release job in quiet hours, a branch named like a tag is not a release; ramping slots block a second start; `ci_isolation = "none"` caps at 1; service restart adoption; stop race with `shutdown_timeout`; the token canary | implementer | 2 d |
| WP-10 | Docs: user guide, architecture and its diagram (independent review, rule 22), features, roadmap | implementer + reviewer | 0.5 d |
| WP-E | Later: ephemeral slots (separate amendment: registration token custody, per-job registration, isolation) | researcher first | - |

About 13 days of work; WP-1, WP-3 and WP-7 can run in parallel after WP-0; WP-6 is the integration point.

## Decisions (the owner, 2026-10-28)

- **D1. The pool (Option A)** over scaling `runner.capacity`, with the proposed defaults: `max` Mac 4, Linux 4, Windows
  VM 2; start a job only below 0.8 load per core, shed idle extras above 1.2; 4 GB per job plus a 4 GB reserve (this was
  D4, decided with D1).
- **D2. User-level runners** (`decryp7`): the service gets its own read-only `read:user` token; ADR 0005 is amended
  (section 10; ADR 0005 carries a pointer here).
- **D3. No cross-OS containers for now:** the Mac and Windows do not serve the Linux labels; Linux jobs wait for Linux
  slots.
- **D5. Release jobs override** quiet hours and the load ceiling; never the low-disk hold, the memory reserve or `max`.
- **D6. Ephemeral runners later,** in their own amendment (on the host; VMs stay local-only).
- **D7. No server-side disable** before a stop; `runner.shutdown_timeout` = the job timeout closes the race (section 4).
- **D8. Interim now:** 2-3 more runners per host, registered by hand and always running (Option E), until the scaler
  lands; steps in [runbooks/add-runners.md](../runbooks/add-runners.md). When the scaler is built, these runners become
  the pools' slots, so nothing is registered twice. **Narrowed by the review (2026-10-28), for the owner to confirm:**
  the Mac and the Windows VM get no extra runners before family ADR 0025 is merged; d-gtr9's container runners may be
  added now with one known shared folder (the Angular cache on `/sccache`).

## Sources (accessed 2026-10-28)

- Owner's Gitea server: `https://dev.decryptology.net/api/v1/version` and `https://dev.decryptology.net/swagger.v1.json`
  (API 28.2.0): jobs, runners and registration-token endpoints and schemas quoted above.
- Gitea runner docs 5.x: [configuration](https://docs.gitea.com/runner/configuration/),
  [registration](https://docs.gitea.com/runner/registration/), [labels](https://docs.gitea.com/runner/labels/),
  [overview](https://docs.gitea.com/usage/actions/act-runner).
- Gitea docs: [Actions runner (tokens, scopes of runners)](https://docs.gitea.com/usage/actions/runner),
  [token scopes](https://docs.gitea.com/development/oauth2-provider).
- [GARM](https://github.com/cloudbase/garm) (README: Gitea support, providers, Apache-2.0).
- Synthetic queue simulation: [`2026-10-autoscale-sim.py`](../research/2026-10-autoscale-sim.py) (no real CI or user data).
