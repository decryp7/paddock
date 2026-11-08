# 61. Jobs carry the instance that runs them

- Status: Proposed (2026-11-08; ER-21 redesign, user decision 2026-11-08 "do all redesigns now"; revision 2 answers the first
  independent review: P1-1 to P1-4 and P2 1 to 12)
- Date: 2026-11-08
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-17 to ER-19 (long operations: off the UI thread,
  labelled progress, no UI timeouts), ER-22 / rule 42 (share first), ER-27 (logs explain every failure), ER-28 (causes
  confirmed by measurement), rule 53 (never act on a reusable name without its identity; ADR 0033), rule 55 and ADR 0034 (one
  implementation per mechanism), ADR 0008 (long operations), ADR 0012 §3-§4 (the shell's jobs, jobs relayed from an agent),
  ADR 0031 (waits judged on state, never given up on), ADR 0049 §4 (wire versions and notes), ADR 0056 (the agent swap and
  retire), ADR 0058 (failed reads retry; the same store file), Dyno 1.1.1 plan WP-8 (`docs/dyno-1-1-1-plan` 2fc247b35,
  `apps/dyno/docs/plans/dyno-1-1-1.md`). It answers WP-8's design question: the identity lives in `paddock-jobs`, shared.

## Pattern

Three reports from the Dyno 1.1.0 VM rounds on 2026-11-08. The coordinator log cited as `log:<line>` is
`/Volumes/d7sst7/dev-targets/handoff/2026-10-29-overnight.md` (outside the repository); the VM evidence is under
`/private/tmp/claude-501/vm-round-2e66322a4/` (outside the repository, not kept).

| # | Seen | Evidence |
| --- | --- | --- |
| 1 | A job running when Dyno's agent restarts is lost, but the shared Jobs list keeps showing it "running", and its Cancel answers `AgentUnavailable`. | `log:1331` |
| 2 | A reading page (Extras > Sleep) shows "That job has no result ..." after an agent restart, with no recovery. | `log:1342`; Windows VM, rc 9d9c2c814: report `out-0ad-win/local-fa1ac1c3b4a5/2026-11-08T09-12-53-336Z-windows-ui.md:612`, app log `evidence/2026-11-08T09-12-53-336Z-win-app-log.txt:38-138` |
| 3 | A benchmark started during an agent restart stays at "Starting" in the Jobs list. | the brief of 2026-11-08. The benchmark page does not use `startJob`: it calls `startBenchmark` and polls the overview (`app/src/app/state/bench.store.ts`). Its run registers a job, "Running benchmarks" (`dyno-agent/src/bench.rs:863`), and "Starting" is the shared Jobs list's headline for a **running job that has no phase yet** (`ui/projects/paddock-ui/src/lib/job-view.ts:147`, `shell/jobs/src/jobs-status.ts:104`). So this is the instance-1 mechanism, seen before the first phase arrived. |

Read against `origin/main` 9e18230c5, they are one missing rule: **a job id (and a disk-scan id) names a thing only within
the agent run that issued it, and nothing on the way from the agent to a page says which run that was.**

- Ids restart at 1 in every agent run. Jobs: `JobRegistry` starts `next` at 1 (`crates/paddock-jobs/src/job.rs:109`); the app's
  own registry is moved out of the way by hand (`start_ids_at(APP_JOB_ID_BASE)`, `apps/dyno/app/src-tauri/src/shell.rs:188,276`).
  Disk scans the same way: `next: AtomicU64::new(1)` (`dyno-agent/src/disk/centre.rs:506`), a new scan takes the next id (`:946`),
  and a kept map restored at start gets a **new** id in every run (`:1319`, `status.scan = scan; status.seq = 0`). The disk-map
  store keys by scan id with the highest `seq` winning (`app/src/app/state/disk-map.store.ts:357-360`).
- The agent keeps its jobs in memory (`Book`, `dyno-agent/src/jobs.rs`); a new run does not know the old run's jobs.
- The shell's store merges by id under "highest `seq` wins" and never removes a job (`ui/projects/paddock-ui/shell/jobs/src/jobs.store.ts:94-99`).
  A job of the old run whose end never came stays active for good (instances 1 and 3). A new run's job with a reused id
  starts at `seq` 0 and is dropped as "older".
- `follow(id)` resolves at once when the store already holds a finished job of that id (`jobs.store.ts:230`). A page that
  starts a read on the new run and gets a reused id is handed the old run's `done` snapshot and asks the new run for the
  result of a job that is still running there: the agent's "That job has no result: it is still running, ..."
  (`dyno-agent/src/jobs.rs:167`). Measured below.
- Cancel goes to whichever registry says yes first (`apps/dyno/app/src/app/shell/engines.ts:119-136`); during the restart the
  link fails fast with `AgentUnavailable` (`app/src-tauri/src/link/mod.rs:544`), and after it the new run answers "unknown".
- The agent serves a new connection's requests before it registers that connection's push queue: `BodyPeer::start(..serve(handler)..)`
  at `dyno-agent/src/server.rs:98`, the `Sub` pushed at `:103`. A job started by a request in that gap emits its first
  snapshots to no queue for that connection, so the app never hears them.
- The page learns of an accepted job only through events: `startJob` returns the id alone (`app/src/app/core/tauri-backend.ts:778`),
  and `progressOf` is `null` until the store has heard of it (`app/src/app/core/job-starter.ts:14-18`), shown as "Starting…"
  (`features/settings/tool-card.ts:39`, `features/power/wake-devices.ts:95`).

The class is rule 53 (a reusable name acted on without its identity) applied to ids the agent issues. Fixing the three
places one by one would leave every other holder of such an id (the Dyno stores listed in J6, the disk map, the shell's
list, status item, follow helper and updates dialog, the CLI) to meet it next (ER-21).

### Causes, measured where the evidence allows (ER-28)

| Instance | Cause | Observation | Result |
| --- | --- | --- | --- |
| 2 | Id reuse: the new run's job has an id the store holds as `done` from the old run | App log: the old connection closed at 16:51:23.22 (`:38`), the agent was replaced (new `AgentRun` 3e9b8e89, pid 4468, `:125`, 16:52:03); `startJob` answered ok at 16:55:25.420 (`:137`, `r117`) and `JobResult` failed `NotFound` at 16:55:25.424 (`:138`), **4 ms later**. A Sleep read runs OS tools and cannot finish in 4 ms; the page asks for a result only after `follow` resolved with `done`, so the store already held a finished job with the new job's id. | **Confirmed** (by the log; T2 reproduces it) |
| 2 | The old run's `done` event arrived and the result was asked of the new run | The old connection was closed 4 minutes before the failure (`:38` vs `:138`); nothing of that run can arrive after its connection closed. | Ruled out |
| 2 | The agent dropped the result (`KEPT_FINISHED` = 16) | 16 jobs cannot end in the 4 ms between the start and the result request. | Ruled out |
| 1 | The old run's active jobs are never ended in the store | Code: `merge` never removes, and nothing else ends a job (`jobs.store.ts:94-99`). T1 reproduces it. | Confirmed by code and T1 |
| 1 | Cancel routed to a dead link or the wrong registry | Code: `engines.ts:119-136`; `link/mod.rs:544` fails fast after the first connection. | Confirmed by code |
| 3 | The run's job belonged to a run that ended, and nothing ends it | Same mechanism as instance 1; T1 covers it with a job that has no phase. | Confirmed by code |
| 3 | A start served in the gap before the push queue exists loses its first events | Code: `server.rs:98` serves, `:103` pushes the `Sub`. A start that reaches a new connection first (the person clicked during the restart) falls in it. | **Confirmed by code**; J4 fixes it with a test |
| 3 | The benchmark job never reports a phase | Each kind begins a phase on its first repetition (`bench.rs:601`). Rule it out by the agent log: `run <id> <kind>: starting` (`bench.rs:391`) followed by a phase in the Jobs list on a live agent. No agent log of the reported run is in the evidence. | Not measured; the VM row in J8 measures it |

## Decision

### 1. Every registry has an instance id, and every job and job reference carries it

`paddock-jobs` gains:

```rust
/// One run of one id issuer (a job registry, Dyno's disk centre): made when it is built, never reused. A random non-zero
/// number below 2^53, so the webview reads it exactly. Not a pid, not a start time: those are reusable names (rule 53).
pub struct InstanceId(u64);

/// A job as the world refers to it: the id is unique only within its instance.
pub struct JobRef { pub instance: InstanceId, pub id: JobId }
```

- `InstanceId::new()` draws from `std::hash::RandomState` (seeded from the OS once per process, varied per call by std), masks
  to 53 bits and rejects 0. This is **not** cryptographic randomness and needs not be: the instance is not a secret, it only
  has to differ from the previous run's and from another registry's in the same process. A test builds two registries in one
  process and asserts different instances; another asserts 1,000 draws are distinct and non-zero.
- `JobRegistry::new` and `with_clock` make one; `with_instance` takes a given one (tests, and Dyno's agent, which makes one
  instance at start for its registry and its disk centre). `JobRegistry::instance()` reads it.
- `JobSnapshot` and `WireSnapshot` gain `instance` and `registered_at` (wall-clock milliseconds at registration). The
  `Reporter` stamps both, so no relay can forget them. Lists order by `registered_at` (active oldest first, ended newest
  first), and `start_ids_at` and Dyno's `APP_JOB_ID_BASE` are deleted.
- `JobContext::job_ref()`; `JobRegistry::cancel` takes a `JobRef` and answers `CancelOutcome { Cancelled, NotActive, OtherInstance }`.

**One name for "another run".** `OtherInstance` everywhere: `CancelOutcome::OtherInstance`, Dyno's `ErrorCode::OtherInstance`
and the TypeScript code `'otherInstance'`.

### 2. The jobs state knows which instances are live, and ends the jobs of the others

The adapter's existing managed state, `paddock_jobs::tauri::Jobs`, gets one port for jobs that run in another process (no new
type name):

```rust
/// Jobs of another process (Dyno's agent), relayed by the app.
pub trait RemoteJobs: Send + Sync {
    /// Stable id and the words the user reads ("dy.agent", "Dyno's agent").
    fn source(&self) -> SourceInfo;
    fn cancel(&self, job: JobRef) -> Result<CancelOutcome, RemoteError>;
}

impl Jobs {
    pub fn add_source(&self, source: Arc<dyn RemoteJobs>) -> SourceHandle;
}
impl SourceHandle {
    /// A connection to the source was made, and the source said which instance answers (section 6).
    pub fn connected(&self, instance: InstanceId);
    /// The source's job list as that instance reported it (read right after `connected`).
    pub fn listed(&self, instance: InstanceId, jobs: Vec<JobSnapshot>);
    /// A job snapshot from the source: an event or a reply that carries one (a start).
    pub fn relay(&self, snapshot: JobSnapshot);
    /// The process that ran `instance` is proven gone (a held child handle reported its exit, or its `ProcessId` no
    /// longer exists, checked with pid plus start time, rule 53).
    pub fn ended(&self, instance: InstanceId);
}
```

**`live`, defined.** For each source, `live` holds **the last instance it answered from**, and keeps it while the source is
disconnected. The live set is the in-process registry's instance plus each source's last instance. A disconnect changes
nothing; `paddock://jobs-instances` (`JobInstances { live }`) fires **only when an instance in the set changes**.

**Ending.** An instance ends when (a) `connected` reports a different instance for its source, or (b) `ended` proves its
process gone. Then every job of that instance the adapter last saw active gets one terminal snapshot,
`JobState::Lost { message }`, with `seq` = the last seen `seq` + 1, emitted on `paddock://jobs` and kept in the source's recent
list (bounded, 16 per source). The message names the source and the cause: "Stopped: Dyno's agent restarted." (a) or
"Stopped: Dyno's agent stopped." (b). A source whose instance ended by (b) has no live instance until it connects again.
The same instance reconnecting (a link error, a paused agent, App Tamer) ends nothing. While disconnected with the process
not proven gone, nothing is decided: the jobs keep their last state and go "Still working" by the existing threshold. Nothing
here is a timeout (ER-19): each end is an observed fact.

**Ended instances stay ended.** The adapter keeps the ended instances per source (bounded, the last 16). A relay or list of an
ended instance is dropped and logged (a late event after B replaced A). If the link connects to A again after B (A→B→A: an
agent that survived a swap), A is live again for new jobs, but a job that was ended as `lost` is never revived: a
`JobRef` that ended stays ended, whatever `seq` arrives later.

**Order.** A `relay` or `listed` for an instance the source has not reported in `connected` is held (bounded, 64) until
`connected` names it, or dropped when another instance is named. With the instance in the connection's first message
(section 6), `connected` always comes first in practice; the hold is the defined answer for the case that it does not.

- `Lost` is a new terminal state in `JobState` (and `isFinishedJob`). It never travels on an agent wire: the agent never
  produces it, `WireState` gets no `Lost`, and the conversion `WireSnapshot::try_from(&JobSnapshot)` refuses a `Lost` snapshot
  with a typed error (the agent never builds one; a test asserts the refusal).
- `jobs_snapshot` answers `JobsList { live: Vec<InstanceId>, jobs: Vec<JobSnapshot> }`. `job_cancel` takes `{ job: JobRef }`
  and routes by instance: the own registry, a live source, or `OtherInstance`. Dyno's TypeScript composition (`dynoJobsIpc`)
  is deleted.
- `seed` (section 3) needs **no wire change**: `StartJob` already answers a whole snapshot (`Reply::Job`); only Dyno's
  `startJob` in TypeScript stops dropping all but the id.
- Logs (ER-27): `Job source instance changed` (source, old and new instance, cause `replaced` or `process gone`, how many jobs
  ended as lost, their ids), `Job source reconnected` (same instance, jobs kept), `Dropped a job snapshot of an ended instance`
  (instance, id, `seq`), and a refused cancel with its reason.

An app with only in-process jobs adds no source: its list has one live instance and only the shapes change.

### 3. The shell store keys jobs by `JobRef` and holds the live set

In `@decryp7/paddock-ui/shell/jobs`:

- `SuJobSnapshot` gains `instance`, `registeredAt` and the state `lost` (with `message`); `parseJobSnapshot` checks them at the
  edge. `SuJob` (`src/lib/job.ts`) renders `lost` as an ended job with its message and no Cancel.
- `JobsBackend.snapshot()` returns the `JobsList`; `cancel(job: JobRef)`; `listenInstances(handler)`. `MemoryJobsBackend` gains
  `restart(sourceId)` (new instance; the old one's active jobs end as `lost`, as the adapter does), `disconnect(sourceId)` and
  `reconnect(sourceId)` (same instance), so every spec can play all three.
- `JobsStore` keys by `${instance}:${id}`, keeps `live`, and re-reads the list on `paddock://jobs-instances` (through ADR
  0058's retry). `follow(ref)` never resolves with a snapshot of another instance.
- **Backstop for a missed event.** A `JobRef` whose instance is not in `live` and has no terminal snapshot resolves as a
  local `lost` with the shell's own words ("Stopped: the process that ran it ended."), `seq` = the last known `seq` + 1 for that
  `JobRef` (0 when none is known), marked local. Any snapshot from the adapter for that `JobRef` replaces a local one,
  whatever its `seq`.
- `seed(snapshot)`: the reply of a start is taken at once, so an accepted job is never unknown to the store.
- The Jobs list and status item: no Cancel on `lost`; each Cancel button is named "Cancel <job title>" (WP-8 (c), `log:1352`).

### 4. Pages hold `JobRef` (and `ScanRef`), and say what happens when their thing is lost

- Dyno's `JobStarter.run(spec, { onLost })` takes a required `onLost: 'restartOnce' | 'report'`. The outcome is a union:
  `done` (with output), `cancelled`, `lost`; `failed` rejects as today. A result is asked with the `JobRef`; the agent answers
  `OtherInstance` for another instance.
- **Reads restart once.** A read that only looks (Sleep, Boot, App usage, Software, find files, log rescan, lookups, the
  threat-list status of `intel.store.ts`) restarts once on `lost`; a second `lost` in the same request shows the stopped
  message with Retry. **Work with effects reports**: a wake, a tool install, a helper action, a list update, a benchmark show
  the message with Retry and never restart by themselves.
- **Starting while the agent is away.** A read waits for the link to be `connected` (no bound; the wait ends when the person
  leaves the page, ADR 0031) and shows the link's own stall words meanwhile. Work with effects is refused at once with the
  link's current state words (`agentUnavailable`, the text of the stall the status chip shows: "Dyno's agent is restarting",
  "Dyno's agent is stopped by another app", and so on), with Retry. A queued retry of work with effects was rejected: it would
  start it long after the person moved on.
- **Benchmarks.** `BenchProgress` gains `job: JobRef`, and the bench store reads that job from the shell store: when the job is
  `lost`, the page stops showing the run as running and says "The run stopped: Dyno's agent restarted. Nothing was saved."
  The overview poll also stops treating a `running` of another instance as current (T4).
- **Disk scans.** `DiskScanStatus` gains `instance`; the requests that name a scan take `ScanRef { instance, scan }`
  (`StartDiskScan.replaces`, `CancelDiskScan`, `DiskChildren`, `DiskPath`, `DiskRings`, `RetryDiskScan`, `DiskReveal`) and the
  agent answers `OtherInstance` for another instance. The disk-map store keys by `ScanRef`; on a new instance it re-reads
  `DiskScans` (the kept maps come back with new refs) and moves the screen to the kept map of the same target, or says the
  scan stopped. The disk centre uses the agent's one instance; scans are not jobs, so they reach the store through their own
  `Message::DiskScan`, and the holder register has an entry for them.

### 5. The check

- **Types.** Job ids cross the shell's public API only as `JobRef`; a bare `number` id in the `shell/jobs` exports fails `tsc`.
  In Rust, `JobRegistry::cancel(JobId)` is removed.
- **Every holder.** A register `apps/dyno/app/src/testing/job-ref-holders.ts` lists each store or page that holds a `JobRef` or
  `ScanRef` (today: `job-starter`, `extras`, `issues`, `lookup`, `tool`, `wol`, `helper`, `intel`, `bench`, `disk-map`), each with a
  driver. One spec runs every entry through three scenarios: (1) start, the fake agent restarts (new instance): the holder
  reaches its declared end (`restartOnce`: a fresh run on the new instance; `report`: the stopped message with Retry), never
  stays active or "Starting…", and never asks the new instance about an old id; (2) start, disconnect, the list is re-read,
  the same instance reconnects: nothing is lost; (3) start, the old process is proven gone with no new instance yet: the
  holder shows the stopped message. The shell's holders (list, status item, follow, updates dialog) have the same spec in
  `ui/`. A conformance check (the next free C number at landing; C91 is the highest seen today) fails when a TypeScript file
  under `apps/*/app/src` or `ui/projects/paddock-ui` calls `startJob(`, `JobStarter.run` or `JobsStore.follow`, or declares a
  `JobRef` or `ScanRef` field, and is not in its register.
- **The rule in Rust.** A model test of the adapter over sequences of connect (same, new, or an ended instance), list, relay
  (before and after connect, late), cancel, disconnect and `ended`: every job of an ended instance gets exactly one `Lost`;
  `seq` only rises; disconnect then a list re-read then a same-instance reconnect ends none and fires no
  `jobs-instances`; a relay of an ended instance is dropped; a `lost` `JobRef` never revives (A→B→A); a cancel for a non-live
  instance is `OtherInstance`. `paddock-jobs` tests: every snapshot carries its registry's instance; two registries in one
  process differ; `try_from` refuses `Lost`. Dyno agent tests: `CancelJob`, `JobResult` and every `ScanRef` request with another
  instance answer `OtherInstance`; a start sent as the first request on a new connection delivers its first snapshot to that
  connection (the `Sub` fix).
- **On the real OS.** A VM row on macOS and Windows (Linux if the round runs): restart the agent during a Sleep read, a
  find-files search, a disk scan and a benchmark; the Jobs list shows each as stopped with the agent's words and no Cancel,
  Sleep shows fresh results, the disk map shows the kept map, nothing stays "Starting", and the app log has the
  `Job source instance changed` line with the ids. Rule 71: before the restart the row checks it sees the running job in the
  list and the benchmark's phase (which also measures the last row of the causes table).
- **Rule.** Propose one sentence for rule 53: "An id another process issues (a job, a scan), like a pid, is acted on only
  together with the instance that issued it." (the user decides; ER-6 sync after).

### 6. How the app learns the instance

**Preferred: in the IPC handshake.** `paddock-ipc`'s `Host` sends an optional server instance in its accept reply and
`Connection::peer_instance()` reads it, so the instance is known before any frame and belongs to the authenticated
connection. J2 first checks whether the handshake can carry it without a frame-level change for every app (an optional
trailing field the old reader ignores); if it can, this is the mechanism.

**Fallback, if the handshake cannot carry it without a frame-major change:** Dyno's agent writes `Message::Hello { instance }`
as the first frame on every connection, before it serves (the same reordering as the `Sub` fix), and the app's link publishes
the connection (`UsePeer`) only after reading it. Either way `connected` precedes every `relay` from that connection, and the
link reads `Request::Jobs` (answering `AgentJobs { instance, jobs }`) right after, for `listed`.

## Wire and protocol impact

Dyno's app-agent protocol is positional postcard (`dyno-core/src/protocol.rs:1-5`): only appended enum variants are
compatible. `main` is at **`PROTOCOL_VERSION = 6`**, shipped in Dyno 1.1.0 (`222c28f9c`, the backup-health removal; the brief's
"5" predates it). A field added to `WireSnapshot` changes the encoding of `Message::Job`, `Reply::Job` and every element of
`Reply::Jobs`, so an older peer would misread the frame: **this is a bump to 7, not a minor addition.** Since the bump is
paid anyway, everything that names an agent-issued id moves in the same version.

| Surface | Change | Kind |
| --- | --- | --- |
| `paddock_jobs::wire::WireSnapshot` | `+ instance: u64`, `+ registered_at: u64` | positional change: Dyno protocol 7 |
| `Request::CancelJob`, `Request::JobResult` | `id: u64` becomes `job: JobRef` | breaking, in 7 |
| `Reply::Jobs` | `Vec<WireSnapshot>` becomes `AgentJobs { instance, jobs }` | breaking, in 7 |
| `BenchProgress` | `+ job: Option<JobRef>` | breaking, in 7 |
| `DiskScanStatus` | `+ instance: u64` | breaking, in 7 |
| `StartDiskScan.replaces`, `CancelDiskScan`, `DiskChildren`, `DiskPath`, `DiskRings`, `RetryDiskScan`, `DiskReveal` | `scan: u64` becomes `ScanRef` | breaking, in 7 |
| `Message::Hello { instance }` | only in the fallback of section 6, appended | additive |
| `ErrorCode` | `+ OtherInstance`, appended | additive |
| `paddock_jobs::JobSnapshot` (JSON to the webview) | `+ instance`, `+ registeredAt`, `+ state: "lost"` | breaking shared JSON: every app on the shell adopts in the same change (rules 39, 42, 43) |
| `jobs_snapshot`, `job_cancel` (shell commands) | `JobsList` answer; `{ job: JobRef }` argument | breaking shared commands; generated `jobs-commands.ts` regenerated |
| `paddock://jobs-instances` | new event | additive |
| `paddock-ipc` handshake | an optional server instance (section 6), only if it needs no frame-level change | additive or not done |
| `agent.json` (discovery) | unchanged | none |
| agent-helper (`HELPER_API`), host-plugin | unchanged: neither carries jobs or scans | none |

Why a bump is the right price: app, `dyno` command and agent ship in one installer, so a bump costs one "update the older
one" refusal between a new app and an old running agent per release line (ADR 0049 §4). Staying compatible would mean
parallel variants beside the old ones, whose old path keeps losing the identity. If ADR 0049's C80 has landed by then,
`./dev wire sync` derives 7 and writes the note; otherwise the bump and the "7:" line above the constant are written by hand,
one bump for the whole 1.1.1 line. Dyno's `api.types.ts`, the mock backend, `wire_roundtrip.rs` samples and `jobs_wire.rs`
follow in the same change.

## Scope

- In: `crates/paddock-jobs`, `crates/paddock-shell` (wiring), `crates/paddock-ipc` (the handshake field, if section 6 allows),
  `ui/projects/paddock-ui/shell/jobs`, the shell holders, `src/lib/job-view.ts` and `src/lib/job.ts`, Dyno's core, agent, CLI,
  app link and UI holders, the shell demos.
- Not in: Debrief's and Spotter's own job stores (`apps/debrief/app/src/app/jobs`, `apps/spotter/app/src/app/state/jobs.store.ts`):
  their jobs run in their engine process, so no run can end under a live webview. They adopt `RemoteJobs` when a job first
  runs in another process, and their copies of the store are a rule 55 question outside this ADR.
- An agent replaced while the old one still runs (ADR 0056's swap) shows the old run's jobs as lost, which is what the person
  can reach; whether the old run's job may still write (benchmark history) is ADR 0056's retire step.

## Work packages (raw agent-days)

Every package: own worktree, failing tests first (rule 5), affected tests only (rule 3), independent review, a mutation check
over its range before landing. After ADR 0058 (WP-1 of the 1.1.1 plan) lands, because J3 and J6 change the same store files.

| # | Package | Owned files | Role | Depends on | Acceptance | Raw days |
| --- | --- | --- | --- | --- | --- | --- |
| J1 | `paddock-jobs` core: `InstanceId`, `JobRef`, registry instance, stamped snapshots with `registered_at`, `JobState::Lost`, `CancelOutcome`, `wire.rs` with `try_from`; `start_ids_at` deleted | `crates/paddock-jobs/src/{model,job,reporter,wire}.rs`, its tests | implementer | ADR accepted | the `paddock-jobs` tests of section 5; coverage >= 99 % | 1-1.5 |
| J2 | Adapter: `RemoteJobs`, `SourceHandle` (`connected`, `listed`, `relay`, `ended`), `live`, ending, tombstones, the hold, `JobsList`, routed `job_cancel`, `paddock://jobs-instances`, logs; shell wiring; the handshake check of section 6 and, if it holds, the `paddock-ipc` field | `crates/paddock-jobs/src/tauri.rs` (+ a pure `remote.rs` core), `crates/paddock-shell/src/builder.rs` (one declared line), `crates/paddock-ipc/src/handshake.rs` (if section 6 allows) | designer (model) + implementer | J1 | the model test of section 5; generated `jobs-commands.ts` check green; `paddock-ipc` tests for an old reader | 2-3 |
| J3 | Shell UI: wire parse, backend API, `MemoryJobsBackend.restart/disconnect/reconnect`, store keyed by `JobRef` with `live`, `seed`, backstop, list and status (no Cancel on lost, "Cancel <title>"), `SuJob` renders `lost`, follow, updates dialog, the storage backend's parse, demos, CHANGELOG | `ui/projects/paddock-ui/shell/jobs/src/*`, `src/lib/job-view.ts`, `src/lib/job.ts` (100 % coverage), `shell/about/src/updates-dialog.ts`, `shell/storage/src/backend.ts`, the two demos' `demo-jobs.ts`, `apps/dyno/app/src/testing/job-test-support.ts`, `CHANGELOG.md` (the shell entry) | implementer | J2 | T1 and T2 red on today's code, green after; the shell holders' spec (three scenarios); axe shows distinct Cancel names; gallery and demo e2e green | 2.5-3.5 |
| J4 | Dyno core, agent, CLI: protocol 7 (table above), the agent's one instance (registry and disk centre) and its start log line, `OtherInstance`, `ScanRef`, the `Sub` registered before serving (`server.rs`) with its test, `Message::Hello` if section 6 falls back, CLI on `JobRef` | `apps/dyno/crates/dyno-core/src/{protocol,jobs,bench}.rs`, `dyno-core/src/disk/scan.rs`, `dyno-agent/src/{jobs,handlers,bench,server}.rs`, `dyno-agent/src/disk/centre.rs`, `dyno-cli/src/lib.rs`, `dyno-agent/src/cli.rs`, wire tests | implementer | J1 | agent tests of section 5; round-trip samples for every changed type; Windows type-check (xwin) | 2.5-4 |
| J5 | Dyno app link: `RemoteJobs` over the link, `connected` from the handshake (or `Hello`), `listed` from `Request::Jobs`, `relay` of `Message::Job` and every `Reply::Job`, `ended` from the held child or the verified `ProcessId`, stall words for refusals and waits, `dynoJobsIpc` deleted | `apps/dyno/app/src-tauri/src/{link/mod.rs,link/ports.rs,link/driver.rs,shell.rs,commands.rs}`, `app/src/app/shell/engines.ts` | implementer | J2, J4 | link tests with a fake agent: new instance ends the jobs, same instance keeps them, a proven-gone process ends them, a start reply is relayed; log lines present (ER-27) | 2-3 |
| J6 | Dyno UI holders: `JobStarter` (`JobRef`, seed, outcome union, required `onLost`, waits for `connected` on reads), extras, issues, lookup, tool, wol, helper, intel, bench store (T4), disk-map store (`ScanRef`); the holder register and its spec; mock `restart` | `apps/dyno/app/src/app/core/{job-starter,backend,tauri-backend,mock-backend,api.types}.ts`, `app/src/app/state/{extras,issues,lookup,tool,wol,helper,intel,bench,disk-map}.store.ts`, `features/benchmarks/`, `features/disk-map/`, `src/testing/job-ref-holders.ts` | implementer | J3, J5 | T3 and T4 red on today's code, green after; every-holder spec green (three scenarios); Dyno affected e2e (Chromium) incl. "restart during a Sleep read" on the mock | 2.5-4 |
| J7 | The conformance check over holders, the rule 53 sentence (proposed), lessons entry | `conformance/` (one check), `docs/rules.md` (one line, after the user's OK), `docs/lessons.md` | implementer | J3, J6 | the check fails on a holder removed from a register and on a new unlisted `startJob(` call; C-number registered | 0.5-1 |
| J8 | VM rows on macOS and Windows (section 5), with the rule 71 precondition | `scripts/vm-smoke/` Dyno rows | implementer + VM agent | J1 to J6 on the 1.1.1 candidate | rows PASS or recorded `not measured` with the reason | 0.5-1 |

No J0: instance 2's cause is measured from the VM app log above, and T2 reproduces it before J1.

**Total: 13.5-21 raw agent-days** (1 + 2 + 2.5 + 2.5 + 2 + 2.5 + 0.5 + 0.5 to 1.5 + 3 + 3.5 + 4 + 3 + 4 + 1 + 1). Revision 1
said 10.5-17; the added scope is the disk scans, the `Sub` fix, `ended`, tombstones and the handshake. **Critical path** J1, J2,
J3, J6, J8 (J4 and J5 run beside J2 and J3): 1 + 2 + 2.5 + 2.5 + 0.5 = **8.5** to 1.5 + 3 + 3.5 + 4 + 1 = **13 days** with two
builders. The 1.1.1 plan guessed 3-6 days for WP-8 before this design, so **WP-8 grows the plan by about 10.5-15 raw days**
(the review's 7.5-11 was measured on revision 1's 10.5-17). A Dyno-only fix (about 4-6 days) is rejected below.

**First failing tests (written against today's API, so they are red before J1):**
- T1 (store): a fake backend whose list, after a restart, lacks a job that was running with no phase: the store must not keep
  it active ("Starting"). Red today.
- T2 (store): a `done` job id 3 in the store, then a running job id 3 from the backend: `follow(3)` must not resolve with the
  first. Red today (`jobs.store.ts:230`); the measured instance 2.
- T3 (Dyno): a job accepted by instance A, A replaced by B, no event of the job ever arrives: the holder shows `lost` (the
  stopped message), not "Starting…". Red today.
- T4 (bench store): the overview says `running`, the agent restarts, the new instance's overview has no run: the page must not
  keep showing the run as running, and says it stopped. Red today if the page keeps the old `running` or shows nothing about
  the lost run.

## Consequences

- An agent-issued id is unambiguous everywhere it travels; a restart ends exactly what it ended, in words that name the cause,
  and a reconnect to the same run ends nothing.
- Pages state what they do with a lost job; the type makes leaving it out a compile error, and the register and check keep
  new holders in the spec.
- The jobs commands and snapshot shape change for every app on the shell in one change, and Dyno's protocol goes to 7 in
  1.1.1 (one "update the older one" refusal on the update from 1.1.0).
- A later app whose jobs run in another process (a Debrief worker, a service job) gets this by adding a source.

## Alternatives rejected

- **Per-page fixes** (re-read on reconnect in each page, clear the Jobs list on reconnect): the instance-by-instance work
  ER-21 forbids; clearing on reconnect would also end jobs that survived a same-instance reconnect.
- **Stamp the instance in Dyno's app link only**, no change to `paddock-jobs` or the agent wire: works for Dyno but leaves every
  other relaying app to remember it, keeps bare ids in the shell API, and lets the agent answer an old id as if it were its
  own (ER-22, rule 53).
- **Unique ids across runs** (seed the counter from the time): hides the class, says nothing about whether a job is alive, and
  runs into the 2^53 limit.
- **Pid plus start time as the instance**: the registry issues the ids, not the process; in-process registries need one too;
  and the pid has no business on the webview wire. (The `ProcessId` is still used, for `ended`.)
- **Compatible wire additions** (new variants beside the old ones): duplicates every job and scan message and keeps an old path
  that drops the identity; the bump is cheap for a wire that ships in one installer.
- **A queued retry for work with effects started while the agent is away**: it can start the work long after the person moved
  on. Reads wait instead, because a read the person is looking at is still wanted.
