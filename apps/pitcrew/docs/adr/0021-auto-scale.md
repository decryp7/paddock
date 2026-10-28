# 21. Auto-scaling runners: a pool of pre-registered slots, started and drained from the queue and the host's load

- Status: Proposed (2026-10-28). Waits for the owner's approval; the questions to settle are in "Decisions for the
  owner" at the end.
- Date: 2026-10-28
- Related: ADR 0005 (credential storage; this ADR amends it), 0006 (supervisor, "between jobs"), 0011 (runner config
  editing), 0013 (holds: low-disk guard and pause windows), 0014 (update deadline), 0015 (capacity advice, which this ADR
  turns into action); family rules 13, 21, 41, 42, 46; ER-24; family ADRs 0019 (limits explain themselves), 0020/0024
  (background work: no polling without a declared check), 0021 (portable tests: `Clock`/`FakeClock`)
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

**Measured** (synthetic, [`2026-10-autoscale-sim.py`](../research/2026-10-autoscale-sim.py), `python3 -I` it to
reproduce): 8 simulated hours, a push every ~10 min fanning out to 1-8 jobs of ~6 min (lognormal), 30 seeds,
5 s steps:

| Strategy | Mean wait | p95 wait | Slot-hours on | Start/stop actions |
|---|---|---|---|---|
| Static, 1 slot (today) | 171 min | 326 min | 9.0 | 0 |
| Static, 4 slots | 9.3 min | 26 min | 36.0 | 0 |
| Auto 1-4, with hysteresis (20 s up, 10 min down, 15 s between starts) | 9.7 min | 26.5 min | 29.2 | 40 |
| Auto 1-4, no hysteresis | 9.7 min | 26.4 min | 24.5 | 92 |

Two things follow. First, almost all of the gain is **more slots**, not cleverness: four slots cut the mean wait from
171 to 9 minutes. Second, an idle slot is cheap: the owner's idle runner on this Mac had an RSS of 37 MB and 0.0% CPU
(`ps`, 2026-10-28; RSS, not rule 26's footprint, so indicative only). What costs the machine is **running jobs**. So the
scaler's real job is admission control: let up to *max* jobs run while the host has room, take extras away when the
host is loaded (the 2026-10-22 evening when agents plus CI drove the Mac to a load of 150-240), and keep idle extras off
when the queue is empty. Hysteresis costs ~20% slot-hours and halves the start/stop churn; worth it, because each start
or stop is a process, a log entry and, for a stop, a race with job pick-up (below).

**What scaling cannot do.** Pitcrew cannot send a Linux job to an idle Mac. A host can only take jobs whose labels it
serves; jobs for `ubuntu-latest` (49 of the monorepo's `runs-on` lines) wait for runners that carry that label. "14
queued while others idle" is partly a **label topology** question (decision D3), not a capacity one.

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
out of the way. Simple and gets most of the wait reduction today, but runs up to max jobs into a loaded host, which is the
failure rule 41 exists for. Recommended as the **interim step** until this is built (decision D8), not as the end state.

## Decision (proposed)

### 1. Pools and slots

`[[autoscale.pools]]` names a set of **slots**: runners already in `pitcrew.toml`, same server, same label names. A pool
has `min` and `max` running slots (`0 <= min <= max <= slots`). Slots are ordered; the scaler starts the lowest stopped
slot and drains the highest idle one, so `mac-1` is the stable base and logs and caches concentrate in the low slots. A
runner in no pool is never touched by the scaler. A slot is **available** to the scaler only when it is enabled, not in
a crash loop, not updating (ADR 0002/0014), not held (ADR 0013), not registration-broken, and not under a user's manual
action (as with holds: if the owner starts, stops, pauses or resumes a slot by hand, the scaler leaves it alone until
the owner hands it back with "Let auto-scale manage"). Held slots stay held: the scaler never resumes what a hold drained.

### 2. Signals

| Signal | Source | Freshness | Unknown means |
|---|---|---|---|
| Queued jobs per pool | `GET <scope>/actions/jobs?status=queued&limit=50&sort=id&order=asc`, at most 4 pages (200 jobs); a job counts for a pool when its `labels` are all among the pool's label names | 15 s poll, backoff to 120 s | no scale-**up** |
| Idle runners elsewhere | `GET <scope>/actions/runners`: online, not busy, not disabled, serving the job's labels, not one of this host's slots | same poll | count 0 (overshoot, bounded by max) |
| Local jobs per slot | the `JobTracker` (already the truth for "between jobs") | each job start/end, at once | n/a |
| Load | 1-minute and 5-minute load average per core (macOS, Linux); CPU share on Windows (no load average) | 5 s probe tick | no scale-up |
| Memory | available memory (`paddock-metrics`) | 5 s | no scale-up |
| Free disk | ADR 0013's guard (`SpaceCache`, ≥ 60 s) | as today | as today (holds) |
| Quiet hours | ADR 0013's pause windows | as today | as today |

**Release jobs** are queued jobs whose `head_branch` matches `release_patterns` (default `*-v*` and `v*`, the family's
tag scheme; to verify in WP-0, with the run's `event` as fallback).

### 3. Control law (pure, per tick, per pool)

```
busy      = slots of the pool running a job (local truth)
demand    = queued jobs for the pool − idle runners elsewhere for them (floor 0)
want      = clamp(busy + demand, min, max)
want      = max(want, busy)                              -- never below what is running
if quiet hours:      want = max(busy, quiet_max, release_need)
host room (start):   load1/core < start_load AND load5/core < start_load AND
                     available memory ≥ slot_mem + mem_reserve
host shed:           load1/core > shed_load for shed_after
release_need = busy + queued release jobs (capped by max)
```

- **Up** when `want > running` for `up_after` (20 s), at most one start per host every `start_gap` (15 s, so builds
  do not all compile at once), only with host room, except `release_need`, which ignores the load ceiling and quiet
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
warning for `shutdown_timeout: 0s`).

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
| `scope` | from the first slot's registration (`user`, `org:<name>`, `instance`, `repo:<o>/<r>`) | where the queue is read |
| `poll_seconds` | 15 (10-300) | the check's interval |
| `pools[].slots`, `min`, `max`, `quiet_max` | -, 1, slots, `min` | per pool |
| `start_load_per_core` | 0.8 | ≈ 11 on 14 cores, inside rule 41's 6-16 window |
| `shed_load_per_core`, `shed_after_s` | 1.2, 120 | ≈ 17 on 14 cores |
| `windows_start_cpu_percent`, `windows_shed_cpu_percent` | 70, 90 | Windows has no load average |
| `slot_mem_mb`, `mem_reserve_mb` | 4096, 4096 | room needed to start one more job |
| `up_after_s`, `start_gap_s`, `down_after_s`, `down_gap_s` | 20, 15, 600, 120 | hysteresis and pacing |
| `release_patterns` | `["*-v*", "v*"]` | jobs that are never starved |
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

- **A read token in the service (amends ADR 0005).** The service must read the queue while the app is closed, so it
  gets its **own** token, separate from the app's: the least scope that covers the runners' registration scope
  (`read:user` for user-level runners; `read:admin` only if the runners are instance-level, which needs a site-admin
  token). It travels once, UI → app → `Request::SetAutoscaleToken { token: Secret }` → stored by the service through
  `paddock-secrets` in the service's own context (`Pitcrew auto-scale` / server URL): the user's Keychain for the macOS
  launch agent, the Secret Service or, opted in, the owner-only file fallback for the Linux user service, and the
  LocalSystem credential store on Windows (to verify in WP-0; fallback: the file store under the service's data folder,
  ACL SYSTEM-only). The UI can set, clear and ask whether it is set; it never gets it back. `Secret`'s `Debug` redacts;
  the adapter never logs URLs with query strings or headers; diagnostics bundles stay redacted.
- **No registration token at scale time.** Slots are registered once through the existing wizard, which gains "Add N
  slots": one token, passed to each `register` through `GITEA_RUNNER_REGISTRATION_TOKEN` in the child's environment
  instead of the command line (fixes ADR 0005's "briefly visible on the command line" where the installed runner
  supports it; checked from `register --help`), never stored.
- **Egress** only to the configured server through `paddock-net`; response caps and timeouts; no redirects to another
  host.
- **Ephemeral runners** (later) would need a reusable registration token in the service: whoever holds it can register a
  runner that receives jobs and their secrets. That is a separate amendment with its own decision.

## Consequences

- Most of the queue goes away with 3-4 slots per host; the scaler keeps those jobs from overloading a host that is also
  running agents, and keeps idle extras off.
- Cross-OS idleness is not solved by this ADR; it needs runners that serve the waiting labels (D3).
- More registrations and folders per host; Settings shows them as one pool. Disk: each slot has its own `work/`;
  housekeeping (ADR 0006) cleans stopped slots first.
- The service gains a network client with a token. It is bounded, read-only and optional.
- `pitcrew-service` starts using `paddock-net`, `paddock-secrets`, `paddock-metrics` and `paddock-jobs::Clock`
  (rule 42), which also helps the planned move to ADR 0021's shared test remedy.

## Risks

- **Unverified API semantics** (WP-0): if `needs`-blocked jobs show as `queued`, demand is overstated (bounded by max;
  extras go idle and are drained); if `/user/actions/jobs` does not cover org repos, the scope must be `org` or
  `instance`.
- **Label matching** assumed to be "all of `runs-on`"; if Gitea matches differently, pool demand is mis-assigned. WP-0
  checks with one job.
- **The stop race** depends on `shutdown_timeout` behaving as documented; WP-9 has a fake-runner test, WP-0 one real
  check.
- **Windows LocalSystem secret store** might not behave like a user vault; fallback named above.
- **Cache contention**: several slots on one host share the Actions cache server and sccache; heavy parallel compiles
  can still saturate disk I/O even under the load ceiling (load includes I/O wait on Linux, not on macOS).

## Work packages (for the build, in order)

| WP | What | Owner | Size |
|---|---|---|---|
| WP-0 | Spike with a read-only token on the real server: `/user` vs `/admin` jobs coverage, `queued` vs `needs`, `labels` = `runs-on`, `head_branch` on tag runs, `busy` with capacity > 1, label matching, `register` env token, a graceful stop with `shutdown_timeout`; LocalSystem credential store on the Windows VM. Results written into this ADR before Accept | researcher | 0.5 d |
| WP-1 | `pitcrew_core::autoscale`: types, parsers, `plan`, explanation, thrash guard; unit and property tests, ≥ 99% | implementer (spec'd here) | 2 d |
| WP-2 | `[autoscale]` config schema, validation, defaults; settings DTOs | implementer | 0.5 d |
| WP-3 | Shared: load average and available memory in `paddock-metrics`' `MachineProbe` (Windows: CPU share); tests in the crate | integrating agent (shared crate) | 1 d |
| WP-4 | Service adapters: Gitea `QueueSource` on `paddock-net`, token through `paddock-secrets`, the mode-2 check with backoff and demand, worker → supervisor messages | implementer | 1.5 d |
| WP-5 | Supervisor wiring: slot availability, actions through the controller, manual-override hand-back, `shutdown_timeout` check and fix, history events, notifications | implementer, independent review | 1.5 d |
| WP-6 | Protocol: `SetAutoscaleToken`/`ClearAutoscaleToken`/`HasAutoscaleToken`, pool status in `Status`; `PROTOCOL_VERSION` bump; `dto.rs` and `api.types.ts` in the same commit | integrating agent | 1 d |
| WP-7 | Register wizard "Add N slots" (names, labels from the first slot, env-var token) | implementer | 1 d |
| WP-8 | UI: Settings > Auto-scale (Material), Dashboard pool card, reasons in Activity, mock backend | implementer | 1.5 d |
| WP-9 | Integration tests: fake runner + `FakeWire` Gitea + `FakeClock`: burst → up; API 401 → hold; load spike → no start, busy never drained; release job in quiet hours; service restart adoption; stop race with `shutdown_timeout` | implementer | 1.5 d |
| WP-10 | Docs: user guide, architecture and its diagram (independent review, rule 22), features, roadmap | implementer + reviewer | 0.5 d |
| WP-E | Later: ephemeral slots (separate amendment: registration token custody, per-job registration, isolation) | researcher first | - |

About 12 days of work; WP-1, WP-3 and WP-7 can run in parallel after WP-0; WP-6 is the integration point.

## Decisions for the owner

- **D1.** Approve Option A (pool of pre-registered slots, started and drained) over B (scaling `runner.capacity`).
- **D2.** Queue scope and token: are the runners registered at user (`decryp7`), org or instance level? The service gets
  its own read token of that scope (amends ADR 0005, which kept tokens out of the service).
- **D3.** Cross-OS: should the Mac (or Windows) also serve the Linux labels through containers, so Linux jobs stop
  waiting while those hosts idle? (Separate decision; the arm64 vs amd64 image question from the local-VM research
  applies.)
- **D4.** Defaults per host: `max` (proposal: Mac 4, Linux 4, Windows VM 2), start/shed load per core 0.8/1.2,
  4 GB per job plus a 4 GB reserve.
- **D5.** Release jobs (tag pattern `*-v*`, `v*`) override quiet hours and the load ceiling, never low disk, memory
  reserve or `max`. Agree?
- **D6.** Ephemeral runners stay a later amendment. Note: "test in a VM" for CI conflicts with the 2026-10-28 decision
  that VMs are for local testing only; ephemeral runners on the host would not.
- **D7.** No server-side disable before a stop (it needs a write token in the service); the stop race is closed with
  `shutdown_timeout` instead. Agree?
- **D8.** Interim step now, before the build: register 2-3 more slots per host by hand and run them statically
  (Option E), accepting no load control until the scaler lands?

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
