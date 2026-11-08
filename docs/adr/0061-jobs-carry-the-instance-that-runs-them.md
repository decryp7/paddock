# 61. Jobs carry the instance that runs them

- Status: Proposed (2026-11-08; ER-21 redesign, user decision 2026-11-08 "do all redesigns now")
- Date: 2026-11-08
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-17 to ER-19 (long operations: off the UI thread,
  labelled progress, no UI timeouts), ER-22 / rule 42 (share first), ER-27 (logs explain every failure), rule 53 (never act on
  a reusable name without its identity; ADR 0033), rule 55 and ADR 0034 (one implementation per mechanism), ADR 0008 (long
  operations), ADR 0012 §3-§4 (the shell's jobs, jobs relayed from an agent), ADR 0031 (waits judged on state, never given
  up on), ADR 0049 §4 (wire versions and notes), ADR 0058
  (failed reads retry; the same store file), Dyno 1.1.1 plan WP-8 (`docs/dyno-1-1-1-plan` 2fc247b35,
  `apps/dyno/docs/plans/dyno-1-1-1.md`). It answers WP-8's design question: the identity lives in `paddock-jobs`, shared.

## Pattern

Three reports from the Dyno 1.1.0 VM rounds on 2026-11-08, each in a different place:

| # | Seen | Evidence |
| --- | --- | --- |
| 1 | A job running when Dyno's agent restarts is lost, but the shared Jobs list keeps showing it "running", and its Cancel answers `AgentUnavailable`. | coordinator log `2026-10-29-overnight.md:1331` |
| 2 | A reading page (Extras > Sleep) attaches to a job id from before the restart: "That job has no result ...", with no recovery. | same log `:1342` (Windows VM, rc 9d9c2c814) |
| 3 | A benchmark started during an agent restart stays at "Starting". | the brief of 2026-11-08 (Windows and macOS VM rounds); "Starting" is the shared job view's headline for a running job that has no phase yet (`ui/projects/paddock-ui/src/lib/job-view.ts:147`), and a benchmark is a registered job ("Running benchmarks", `dyno-agent/src/bench.rs:863`) |

Read against `origin/main` 9e18230c5, they are one missing rule: **a job id names a job only within the process run that
issued it, and nothing on the way from the agent to a page says which run that was.**

- Ids restart at 1 in every agent run: `JobRegistry` starts `next` at 1 (`crates/paddock-jobs/src/job.rs:109`). The app's own
  registry is moved out of the way by hand (`start_ids_at(APP_JOB_ID_BASE)`, `apps/dyno/app/src-tauri/src/shell.rs:188,276`),
  but two agent runs collide with each other.
- The agent keeps its jobs in memory (`Book`, `dyno-agent/src/jobs.rs`); a new run does not know the old run's jobs.
- The shell's store merges by id under "highest `seq` wins" and never removes a job (`ui/projects/paddock-ui/shell/jobs/src/jobs.store.ts:94-99`).
  A job of the old run whose end never came stays active forever (instance 1, and instance 3 when the old run never sent
  a phase). A new run's job with a reused id starts at `seq` 0 and is dropped as "older" than the old run's job of that id.
- `follow(id)` resolves at once when the store already holds a finished job of that id (`jobs.store.ts:230`). A page that
  starts a read on the new run and gets a reused id is handed the old run's `done` snapshot, then asks the new run for the
  result of a job that is still running there: exactly the agent's "That job has no result: it is still running, ..."
  (`dyno-agent/src/jobs.rs:167`). The same text comes back when the old run's `done` event arrives and the result request
  reaches the new run (instance 2; both causes are listed below).
- Cancel goes to whichever registry says yes first (`apps/dyno/app/src/app/shell/engines.ts:119-136`, "the registry says
  false for an id that is not its own: then the agent's job it is"); during the restart the link fails fast with
  `AgentUnavailable` (`app/src-tauri/src/link/mod.rs:544`), and after it the new run answers "unknown" for the old id.
- The page learns of an accepted job only through events: `startJob` returns the id alone (`app/src/app/core/tauri-backend.ts:778`)
  and `progressOf` is `null` until the store has heard of it (`app/src/app/core/job-starter.ts:14-18`), which the cards show as
  "Starting…" (`features/settings/tool-card.ts:39`, `features/power/wake-devices.ts:95`). A job the dying run accepted and never
  reported stays there.

The class is rule 53 (a reusable name acted on without its identity) applied to job ids. Fixing the three places one by
one would leave every other holder of a job id (eight Dyno stores and pages, the shell's list, status item, follow helper and
updates dialog, the CLI) to meet it next (ER-21).

### Causes considered (ER-28)

The causes below come from reading the code; each work package that fixes one starts with the failing test that shows it
(rule 5) and records which ones the test confirmed.

| Instance | Plausible cause | Observation that rules it out |
| --- | --- | --- |
| 1 | The old run's active jobs are never ended in the store | The store, after a list read from a run that lacks the job, no longer shows it active |
| 1 | Cancel routed to the wrong registry or a dead link | `job_cancel` reaches the run that owns the id, or is refused with a typed "that run ended" |
| 2 | Id reuse: the new run's job has an id the store holds as `done` from the old run | Test T2 below: `follow` of a reused id does not resolve with the old snapshot |
| 2 | Old run's `done` event, result asked of the new run | A result request carries the run; a different run answers `InstanceGone` |
| 2 | The agent drops results too early (`KEPT_FINISHED` = 16) | The failing read happened with fewer than 16 jobs since the job ended (agent log) |
| 3 | A job the dying run accepted, never reported, waits forever | Test T3: an accepted start is known to the store from its reply, and ends when its run is gone |
| 3 | The start went to the new run, but its events were lost before the subscription | Ruled out by code: the agent adds the push queue at connect, before any request (`dyno-agent/src/server.rs:103`) |
| 3 | The benchmark job never reports a phase | Ruled out by code: each kind begins a phase (`bench.rs:601`); a run on a live agent leaves "Starting" within one repetition |

## Decision

### 1. Every registry has an instance id, and every job and job reference carries it

`paddock-jobs` gains:

```rust
/// One run of one job registry: made when the registry is built, never reused. A random non-zero number below 2^53, so the
/// webview reads it as an exact number. Not a pid, not a start time: those are reusable or guessable names (rule 53).
pub struct InstanceId(u64);

/// A job as the world refers to it: the id is unique only within its instance.
pub struct JobRef { pub instance: InstanceId, pub id: JobId }
```

- `JobRegistry::new` and `with_clock` make the instance (`std::hash::RandomState` seeded per process, masked to 53 bits, never 0;
  no new dependency). `JobRegistry::instance()` reads it; tests pass a fixed one with `with_instance`.
- `JobSnapshot` and `WireSnapshot` gain `instance`. The `Reporter` stamps it, so no relay can forget it: the producer is the
  single source (one registry per process run in every app today).
- `JobContext::job_ref()`; `JobRegistry::cancel` takes a `JobRef`, and a foreign instance is a typed
  answer (`CancelOutcome::OtherInstance`), not "unknown".

Ids still start at 1 per registry; `start_ids_at` stays only for list order in Dyno (the app's jobs after the agent's).
Making ids unique across runs instead (a time base) was rejected: it hides the class rather than naming it, and it collides
with the 2^53 limit and with Dyno's app base.

### 2. The jobs adapter knows which instances are live, and ends the jobs of the others

`paddock_jobs::tauri` (the adapter the shell manages) gets one port for jobs that run in another process:

```rust
/// Jobs of another process (Dyno's agent), relayed by the app. The adapter keeps their latest snapshots (bounded), routes
/// cancel requests to the source that owns the instance, and decides when an instance has ended.
pub trait RemoteJobs: Send + Sync {
    /// Stable id and the words the user reads ("dy.agent", "Dyno's agent").
    fn source(&self) -> SourceInfo;
    fn cancel(&self, job: JobRef) -> Result<bool, RemoteError>;
}

impl JobsAdapter {
    pub fn add_source(&self, source: Arc<dyn RemoteJobs>) -> SourceHandle;
}
impl SourceHandle {
    /// The source answered from this instance (a connection was made and its job list read).
    pub fn connected(&self, instance: InstanceId, jobs: Vec<JobSnapshot>);
    /// A job snapshot from the source: an event or a reply that carries one (a start).
    pub fn relay(&self, snapshot: JobSnapshot);
}
```

- **The rule, in one place.** When `connected` reports an instance different from the source's previous one, every job of
  the previous instance that the adapter last saw active gets one terminal snapshot, `JobState::Lost { message }` with
  `seq` = last seen + 1, emitted on `paddock://jobs` and kept in the adapter's recent list (bounded like the agent's: 16 per
  source). The message names the source: "Stopped: Dyno's agent restarted. Start it again." The same instance reconnecting
  (a link error, a paused agent, App Tamer) ends nothing. While no connection exists nothing is decided: the jobs keep their
  last state and go "Still working" by the existing threshold; nothing here is a timeout (ER-19), it is an observed fact (a
  different run answered).
- `Lost` is a new terminal state in `JobState` (and `isFinishedJob`), not a `Failed` text: pages decide on it (section 4), and
  the list shows it with its own word and no Cancel.
- `jobs_snapshot` answers `JobsList { live: Vec<InstanceId>, jobs: Vec<JobSnapshot> }`: the in-process registry's instance plus
  each connected source's, and the jobs of all of them plus the recent lost ones. `job_cancel` takes `{ job: JobRef }` and
  routes by instance: the own registry, a live source, or `OtherInstance` for an instance that is not live. The per-app
  composition in TypeScript (`dynoJobsIpc`, which merges two lists and tries two cancels) is deleted.
- A new event `paddock://jobs-instances` carries `JobInstances { live }` whenever the set changes.
- Logs (ER-27): the adapter writes `Job source instance changed` (source, old and new instance, how many jobs ended as lost
  and their ids) or `Job source reconnected` (same instance, jobs kept); a refused cancel logs its reason.

Apps with only in-process jobs (Pitwall, the demos) add no source: their list has one live instance and nothing else changes
for them except the shapes.

### 3. The shell store keys jobs by `JobRef` and holds the live set

In `@decryp7/paddock-ui/shell/jobs`:

- `SuJobSnapshot` gains `instance: number` and the state `lost` (with `message`). `parseJobSnapshot` checks both at the edge.
- `JobsBackend.snapshot()` returns the `JobsList`; `cancel(job: JobRef)`; `listenInstances(handler)`. `MemoryJobsBackend` gains
  `restart(sourceId)` (a new instance; the old one's active jobs end as `lost`, as the Rust adapter does), so every spec can play
  a restart.
- `JobsStore` keys by `${instance}:${id}`, keeps `live`, and re-reads the list on `paddock://jobs-instances` (through ADR
  0058's retry). `follow(ref)` never resolves with a snapshot of another instance. As a backstop for a missed event, a
  reference whose instance is not live and of which no terminal snapshot is known resolves as `lost` with the shell's own
  words ("Stopped: the process that ran it ended."); the adapter's message is used whenever it arrived.
- `seed(snapshot)`: the reply of a start is a snapshot, and the store takes it at once, so an accepted job is never unknown
  to the store (no "Starting…" placeholder after a start was accepted).
- The Jobs list and status item: no Cancel on `lost`; each Cancel button is named "Cancel <job title>" (WP-8 (c),
  `log:1352`).

### 4. Pages hold `JobRef`, and say what happens when their job is lost

- Dyno's `JobStarter.run(spec, { onLost })` takes a required `onLost: 'restartOnce' | 'report'`, so no call site can leave it
  undecided. The outcome is a union: `done` (with output), `cancelled`, `lost`; `failed` rejects as today. A result is asked
  with the `JobRef`, and the agent answers `InstanceGone` for another instance.
- **Reads restart once.** A read that only looks (Sleep, Boot, App usage, Software, find files, log rescan, lookups) restarts
  once on `lost` and shows its normal progress; a second `lost` in the same request shows "Stopped: Dyno's agent restarted"
  with Retry. **Work with effects reports.** A wake, a tool install, a helper action, a benchmark show that message with Retry
  and never restart by themselves (the person may no longer want them).
- **A start during a restart is refused clearly.** The link already fails fast after the first connection; while it is
  reconnecting the refusal says "Dyno's agent is restarting. Try again in a moment." (`agentUnavailable`, with Retry on the
  page). A queued retry was rejected: it would start work minutes later that the person may have given up on, and it needs a
  bound the family rules forbid on waits (ADR 0031).
- The benchmark page follows its run's job: `BenchProgress` gains `job: JobRef`, and the bench store reads the job's state from
  the shell store, so a run lost to a restart says "The run stopped: Dyno's agent restarted. Nothing was saved." instead of
  vanishing from the overview.

### 5. The check

- **Types.** Job ids cross the shell's public API only as `JobRef`; a bare `number` id in `shell/jobs` exports fails `tsc`.
  In Rust, `JobRegistry::cancel(JobId)` is removed.
- **Every holder.** A register `apps/dyno/app/src/testing/job-ref-holders.ts` lists each store or page that holds a `JobRef`
  with a driver; one spec runs every entry through: start, the fake agent restarts (new instance), and then the holder must
  reach its declared end (`restartOnce`: a fresh run started on the new instance; `report`: the lost message with Retry), never
  stay active or "Starting…", and never ask the new instance for an old job's result. A second pass reconnects the same
  instance and expects nothing lost. The shell's own holders (list, status item, follow, updates dialog) have the same spec in
  `ui/`. A conformance check (the next free C number at landing; C91 is the highest seen today) fails when a TypeScript file
  under `apps/*/app/src` or `ui/projects/paddock-ui` uses `JobStarter.run`, `JobsStore.follow` or a `JobRef`-typed field and is
  not in its register.
- **The rule in Rust.** A model test of the adapter over sequences of connect (same or new instance), relay, cancel and
  disconnect: every job of an ended instance gets exactly one `Lost`, `seq` only rises, a same-instance reconnect ends none, a
  cancel or result for a non-live instance is `OtherInstance`. A `paddock-jobs` test: every snapshot a registry emits carries
  its instance. A Dyno agent test: `CancelJob` and `JobResult` with another instance answer `InstanceGone`.
- **On the real OS.** A VM row on macOS and Windows (Linux if the round runs): restart the agent during a Sleep read, a find-files
  search and a benchmark; the Jobs list shows each as stopped with the agent's words and no Cancel, Sleep shows fresh results,
  nothing stays "Starting", and the app log has the `Job source instance changed` line with the ids (rule 71: the row first
  checks it can see a running job in the list before the restart).
- **Rule.** Propose one sentence for rule 53: "A job id, like a pid, is acted on only together with the instance that issued
  it." (the user decides; ER-6 sync after).

## Wire and protocol impact

Dyno's app-agent protocol is positional postcard (`dyno-core/src/protocol.rs:1-5`): only appended enum variants are
compatible. `main` is at **`PROTOCOL_VERSION = 6`**, shipped in Dyno 1.1.0 (`222c28f9c`, the backup-health removal; the brief's
"5" predates it). A field added to `WireSnapshot` changes the encoding of `Message::Job`, `Reply::Job` and every element of
`Reply::Jobs`, so an older peer would misread the frame: **this is a bump to 7, not a minor addition.** Since the bump is
paid anyway, the requests are made right in the same version rather than working around it.

| Surface | Change | Kind |
| --- | --- | --- |
| `paddock_jobs::wire::WireSnapshot` | `+ instance: u64` | positional change: Dyno protocol 7 |
| `Request::CancelJob`, `Request::JobResult` | `id: u64` becomes `job: JobRef` | breaking, in 7 |
| `Reply::Jobs` | `Vec<WireSnapshot>` becomes `JobsList { instance, jobs }` (the link reads the instance on connect) | breaking, in 7 |
| `BenchProgress` | `+ job: Option<JobRef>` | breaking, in 7 |
| `ErrorCode` | `+ InstanceGone`, appended | additive |
| `paddock_jobs::JobSnapshot` (JSON to the webview) | `+ instance`, `+ state: "lost"` | breaking shared JSON: every app on the shell adopts in the same change (rules 39, 42, 43) |
| `jobs_snapshot`, `job_cancel` (shell commands) | `JobsList` answer; `{ job: JobRef }` argument | breaking shared commands; generated `jobs-commands.ts` regenerated |
| `paddock://jobs-instances` | new event | additive |
| `agent.json` (discovery) | unchanged (the instance comes over the authenticated connection, not from the file) | none |
| agent-helper (`HELPER_API`), host-plugin | unchanged: neither carries jobs | none |

Why a bump is the right price: app, `dyno` command and agent ship in one installer, so a bump costs one "update the older
one" refusal between a new app and an old running agent per release line (ADR 0049 §4), which the link already handles on
update. Staying compatible would mean parallel variants (`Message::Job2`, `Request::CancelJobRef` ...) beside the old ones,
whose old path keeps losing the identity. If ADR 0049's C80 has landed by then, `./dev wire sync` derives 7 and writes the
note; otherwise the bump and the "7:" line above the constant are written by hand, one bump for the whole 1.1.1 line.
Dyno's `api.types.ts`, the mock backend, `wire_roundtrip.rs` samples and `jobs_wire.rs` follow in the same change.

## Scope

- In: `crates/paddock-jobs`, `crates/paddock-shell` (the adapter wiring), `ui/projects/paddock-ui/shell/jobs` and the shell
  holders, `ui/projects/paddock-ui/src/lib/job-view.ts`, Dyno's core, agent, CLI, app link and UI holders, the shell demos.
- Not in: Debrief's and Spotter's own job stores (`apps/debrief/app/src/app/jobs`, `apps/spotter/app/src/app/state/jobs.store.ts`):
  their jobs run in their engine process and no run can end under a live webview. They adopt `RemoteJobs` when a job first
  runs in another process (a Debrief worker), and their copies of the store are a rule 55 question outside this ADR.
- An agent replaced while the old one still runs (ADR 0056's swap between the service's agent and the app's own) shows the old
  run's jobs as lost, which is what the person can reach; whether the old run's job may still write (benchmark history) is
  ADR 0056's retire step, not this one.

## Work packages (raw agent-days)

Every package: own worktree, failing tests first (rule 5), affected tests only (rule 3), independent review, a mutation check
over its range before landing. After ADR 0058 (WP-1 of the 1.1.1 plan) lands, because 3 and 5 change the same store file.

| # | Package | Owned files | Role | Depends on | Acceptance | Raw days |
| --- | --- | --- | --- | --- | --- | --- |
| J1 | `paddock-jobs` core: `InstanceId`, `JobRef`, registry instance, stamped snapshots, `JobState::Lost`, `CancelOutcome`, `wire.rs` | `crates/paddock-jobs/src/{model,job,reporter,wire}.rs`, its tests | implementer | ADR accepted | every emitted snapshot carries its instance (test); coverage >= 99 % | 1-1.5 |
| J2 | Adapter: `RemoteJobs`, `SourceHandle`, the lost rule, `JobsList`, routed `job_cancel`, `paddock://jobs-instances`, logs; shell wiring | `crates/paddock-jobs/src/tauri.rs` (+ a pure `remote.rs` core), `crates/paddock-shell/src/builder.rs` (one declared line) | designer (model) + implementer | J1 | the model test of section 5; generated `jobs-commands.ts` check green | 1.5-2.5 |
| J3 | Shell UI: wire parse, backend API, `MemoryJobsBackend.restart`, store keyed by `JobRef` with `live`, `seed`, backstop, list and status (no Cancel on lost, "Cancel <title>"), follow, updates dialog, demos | `ui/projects/paddock-ui/shell/jobs/src/*`, `src/lib/job-view.ts`, `shell/about/src/updates-dialog.ts`, the two demos' `demo-jobs.ts` | implementer | J2 | specs T1 to T3 (below) red on today's code, green after; shell holders' every-holder spec; axe shows distinct Cancel names; gallery and demo e2e green | 2-3 |
| J4 | Dyno core, agent, CLI: protocol 7 (table above), agent registry instance and its start log line, `InstanceGone`, CLI on `JobRef` | `apps/dyno/crates/dyno-core/src/{protocol,jobs,bench}.rs`, `dyno-agent/src/{jobs,handlers,bench}.rs`, `dyno-cli/src/lib.rs`, `dyno-agent/src/cli.rs`, wire tests | implementer | J1 | agent test for `InstanceGone`; round-trip samples for every changed type; Windows type-check (xwin) | 1.5-2.5 |
| J5 | Dyno app link: `RemoteJobs` over the link, relay of `Message::Job` and every `Reply::Job`, `Jobs` read on connect, the restarting refusal text, `dynoJobsIpc` deleted | `apps/dyno/app/src-tauri/src/{link/mod.rs,link/ports.rs,shell.rs,commands.rs}`, `app/src/app/shell/engines.ts` | implementer | J2, J4 | link tests with a fake agent: new instance ends the jobs, same instance keeps them, start reply relayed; log lines present (ER-27) | 1.5-2.5 |
| J6 | Dyno UI holders: `JobStarter` (`JobRef`, seed, outcome union, required `onLost`), extras, issues, lookup, tool, wol, helper, diagnostics page, widget board, bench store; the holder register and its spec; mock `restart` | `apps/dyno/app/src/app/core/{job-starter,backend,tauri-backend,mock-backend,api.types}.ts`, `app/src/app/state/*.store.ts` (the listed ones), `features/benchmarks/`, `features/diagnostics/diagnostics-page.ts`, `features/widgets/widget-board.ts`, `src/testing/job-ref-holders.ts` | implementer | J3, J5 | every-holder spec red per holder on today's code (where the API allows) and green after; Dyno affected e2e (Chromium) incl. "restart during a Sleep read" on the mock | 2-3 |
| J7 | The conformance check over holders, the rule 53 sentence (proposed), lessons entry | `conformance/` (one check), `docs/rules.md` (one line, after the user's OK), `docs/lessons.md` | implementer | J3, J6 | the check fails on a holder removed from a register and on a new unlisted holder; C-number registered | 0.5-1 |
| J8 | VM rows on macOS and Windows (section 5), with the rule 71 precondition | `scripts/vm-smoke/` Dyno rows | implementer + VM agent | J1 to J6 on the 1.1.1 candidate | rows PASS or recorded `not measured` with the reason | 0.5-1 |

**Total: 10.5-17 raw agent-days** (the plan's WP-8 guessed 3-6 before this design; the difference is the shared adapter, the
protocol bump and the holder check, which make the class impossible rather than fixing three places). Critical path J1, J2,
J3, J6 (with J4 and J5 in parallel after J1 and J2): about 7-10 days elapsed with two builders. A Dyno-only fix (stamping in the
app link, no shared change) would be about 4-6 days and is rejected below.

**First failing tests (written against today's API, so they are red before J1):**
- T1 (store): a fake backend whose list, after a restart, lacks a job that was running: the store must not keep it active. Red today.
- T2 (store): a `done` job id 3 in the store, then a running job id 3 from the backend: `follow(3)` must not resolve with the
  first. Red today (`jobs.store.ts:230`).
- T3 (Dyno): `startJob` accepted, no event follows: the page must not show "Starting…" forever. Red today.

## Consequences

- A job is unambiguous everywhere it travels; a restart ends exactly the jobs it ended, in words that name the cause, and a
  reconnect to the same run ends none.
- Pages state what they do with a lost job; the type makes leaving it out a compile error.
- The jobs commands and snapshot shape change for every app on the shell in one change, and Dyno's protocol goes to 7 in
  1.1.1 (one "update the older one" refusal on the update from 1.1.0).
- A later app whose jobs run in another process (a Debrief worker, a Pitwall service job) gets this by adding a source.

## Alternatives rejected

- **Per-page fixes** (re-read on reconnect in each page, clear the Jobs list on reconnect). That is the instance-by-instance work
  ER-21 forbids, and clearing on reconnect would also end jobs that survived a same-instance reconnect.
- **Stamp the instance in Dyno's app link only**, no change to `paddock-jobs` or the agent wire (no protocol bump). It works for
  Dyno but leaves every other relaying app to remember it, keeps bare ids in the shell API, and lets the agent answer an old id
  as if it were its own. Rejected (ER-22, rule 53).
- **Unique ids across runs** (seed the id counter from the time). Hides the class, and has no answer for "is this job still
  alive"; it also runs into the 2^53 limit beside Dyno's app id base.
- **Pid plus start time as the instance** (rule 53's process identity). The registry is the thing that issues ids, not the
  process; in-process registries need one too; and the pid has no business on the webview wire.
- **Compatible wire additions** (new variants beside the old ones, no bump). Duplicates every job message and keeps an old path
  that drops the identity; the bump is cheap for a wire that ships in one installer.
- **A queued retry for a start during a restart.** It needs a bound on a wait (ADR 0031 says waits are not given up on), and it
  can start work long after the person moved on; a clear refusal with Retry is honest and simple.
