# Jobs across agent restarts (Dyno 1.1.1 WP-8)

- Status: Proposed, 2026-11-12, revision 2 after review round 1. Design note for [plan](../plans/dyno-1-1-1.md) row WP-8
  (line 99). Nothing here is built.
- **User decisions, 2026-11-12** (relayed by the coordinator): D1 **(a)**, the full design; D2, the job list is read and
  relayed on **every** reconnect, new run or same run; D3, the D-7 idempotency key is built **after S1**. Section 10 keeps
  the options as they were put.
- Heads read (ADR 0091): `origin/train/2026-11-11-5` = `607bb7928`, `origin/main` = `e2a318345`, the Dyno 1.1.0 tag
  `dyno-v1.1.0` (commit `8efce3fc3`). Re-read right before the commit.
- Sources outside the repo: the coordinator log `/Volumes/d7sst7/dev-targets/handoff/2026-10-29-overnight.md` (cited as
  `log:<line>`, as the plan does); the user's decisions of 2026-11-12 (memory `decisions-2026-11-12-pitwall-scope`, D-7).

## 1. The problem, as shipped

Checked in the code at the tag `dyno-v1.1.0` and unchanged on the train head:

| # | Fact | Where |
|---|---|---|
| F1 | The agent's job ids start at 1 in every agent run. The agent's registry never calls `start_ids_at`; only the app's own registry does (`APP_JOB_ID_BASE`). | `crates/paddock-jobs/src/job.rs:125` (`AtomicU64::new(1)`; `:109` at the tag); `apps/dyno/crates/dyno-agent/src/jobs.rs:66`; `apps/dyno/app/src-tauri/src/shell.rs:188,276` |
| F2 | A new agent starts jobs on its own at once: `RebuildIntel`, then the due list and location updates. So a restarted agent hands out ids 1, 2, 3 ... again within seconds. | `apps/dyno/crates/dyno-agent/src/lib.rs:326,332` (`:295,301` at the tag) |
| F3 | The shared Jobs store keeps the highest `seq` per id and never removes a job, not even after a re-read. A new job that reuses an old id is ignored while its `seq` is lower, and then replaces the old job once its `seq` is higher. | `ui/projects/paddock-ui/shell/jobs/src/jobs.store.ts:100` (`known.seq >= job.seq`; `:99` at the tag) |
| F4 | Nothing ends a job that was running when its agent went away: no event comes, and the app does not read the list again on reconnect (the store re-reads only while its own read is in error). The job stays "running" with Cancel; Cancel then returns `AgentUnavailable` or `false` (`log:1331`). | `apps/dyno/app/src-tauri/src/shell.rs:967-988` (`connected` reads power, Wake-on-LAN and settings, not jobs) |
| F5 | A reading page follows its job id and asks the agent for the result when it sees `done`. After a restart that id can name another job (F1, F2, F3), so the page asks for a result that is not its own and gets "That job has no result: ..." (`log:1342`). | `apps/dyno/app/src/app/core/job-starter.ts:48-52`; `apps/dyno/crates/dyno-agent/src/jobs.rs:164-177` |
| F6 | Every Cancel button in the Jobs list and on the pages is named only "Cancel", so a screen reader hears several equal buttons (`log:1352`). | `ui/projects/paddock-ui/src/lib/job.ts:66` (shared `pd-job`), used by `shell/jobs/src/jobs-list.ts:22` and Dyno's `app/src/app/shared/job-progress.ts:15` |
| F7 | Only Dyno runs jobs in another process than its window: `paddock_jobs::wire` is used by Dyno alone. Pitwall, Spotter and the demos run their jobs in the app process, so their jobs end with their window. | `git grep -l paddock_jobs::wire` lists only `apps/dyno` and `crates/paddock-jobs` |
| F8 | The agent run already has an identity: the IPC endpoint name is random per bind (`paddock-<random hex>`), it is in the discovery file with the per-launch token, and the link logs its fingerprint as `AgentRun` on every connect. | `crates/paddock-ipc/src/endpoint/unix.rs:58`, `windows.rs:217`; `apps/dyno/app/src-tauri/src/link/driver.rs:245-251` |
| F9 | Debrief's own jobs have a terminal state `Interrupted` ("Debrief closed or crashed while the job was queued or running"), retryable, shown as "Interrupted". Debrief's jobs move onto `paddock-jobs` in its plan step M2a. | `apps/debrief/app/src-tauri/src/jobs/model.rs:120-121`; `apps/debrief/app/src/app/jobs/jobs-text.ts:42-43`; `docs/plans/debrief-1-2.md:121` |

F5 is an inference from F1 to F3. Other causes are possible, and slice S5 starts by reproducing the message (ER-28):

- **C1, an id is reused** (F1 to F3). Ruled out if the page's message stays with ids that are unique across runs.
- **C2, a result is asked for a job whose output was never stored.** This covers the benchmark jobs (`register_external`) and jobs past the 16 kept. Ruled out if the reproduction's reused id names a job with an output.
- **C3, a window's store was rebuilt and re-read the list with the old id in its state.** Ruled out if the page's store and the Jobs store live for the whole test.

## 2. Decision

**The agent-run identity stays in Dyno. It is not a field of the shared snapshot.** The shared crate gets one new terminal
state, `Interrupted`, which Debrief needs anyway. The Cancel names are fixed once, in the shared component.

1. **Ids are unique across agent runs (Dyno only).** The agent's registry starts its ids at
   `first_job_id(started_ms)` = `clamp(started_ms, 0, APP_JOB_ID_BASE / 16 − 1) × 16`, computed with saturating
   arithmetic; a clamp is logged once at start with the raw value (a clock before 1970 or after 2136). `started_ms` is
   read **once**, at the top of `start_inner` (`dyno-agent/src/lib.rs:202`), and passed both to `JobCentre::new`
   (`agent.rs:771`) and to `server::bind` (`server.rs:39`, today a second `now()` read), so the id base and the
   discovery file's `started` are the same value. This fixes F1 to F3 with no change to the store's highest-`seq` rule.
   The base grows with the start time, so the list stays in start order. It stays below `APP_JOB_ID_BASE`
   (`0x4c4f_4749_4e00`) until the year 2136 and is a safe integer for the webview. Two runs overlap only if a run
   started 16 or more jobs per millisecond of its life, or if the clock went back to within about `n_old / 16` ms of an
   earlier start (`n_old`: the jobs the earlier run started). The second is a known edge; section 6 says what happens.
2. **On every connect the shell reads the list and ends the jobs of a run that is gone (Dyno only; user decision D2).**
   The shell keeps `AgentJobs`, a small pure type in `app/src-tauri`: per agent job id, the newest snapshot it has seen
   and the highest `seq` it has seen. It learns from three places: every pushed `Message::Job`
   (`Shell::job`), every `Reply::Jobs` and `Reply::Job` that passes through the app's `dyno` command (this is how a
   window's own `jobs_snapshot` read, merged in `app/src/app/shell/engines.ts:119-131`, reaches it), and its own read on
   connect. On **every** connect, new run or same run, `Shell::connected` does, in this order:
   1. read `Request::Jobs`, feed every entry to `AgentJobs` (seeding or refreshing it) and relay every entry on
      `paddock://jobs`; the store keeps the highest `seq`, so endings missed while away now arrive;
   2. compare the run with the last connect's. The run is the connection's **full endpoint string together with the pid
      and `started`** of the discovery file whose token authenticated the connection (F8, rule 53); the link passes it up
      with `Event::Connected`, and only its 32-bit fingerprint (`AgentRun`) is ever logged;
   3. **another run:** every job `AgentJobs` remembers as active from the earlier run ends as `Interrupted` with
      "Dyno's background agent restarted, so this stopped.", whether or not the new list happens to name its id (another
      run cannot own an old job). **Same run:** a remembered active job that the list no longer names (more than 16 ended
      meanwhile) ends as `Interrupted` with "Dyno lost track of this while it reconnected. It may have finished.".
   Each `Interrupted` snapshot has `seq` = the highest `seq` the shell has seen for that id, plus 1. Because every
   snapshot a window can hold went through the shell (an event, a `dyno` reply or the connect read), every window's store
   takes it. Cancel goes away because the job is no longer active (fixes (a)).
3. **`Interrupted` is a shared terminal state (append-only).** The page needs a typed reason to read again by itself, and
   a message string is not a type. A Dyno-only signal needs the agent status and the job event to arrive in order (option
   D1 (b)). Debrief already has this state (F9) and brings it into `paddock-jobs` at M2a. So the state is added once,
   now, with Debrief's name, and Debrief's copy folds into it at M2a (rule 55).
4. **Reading pages read again once (Dyno only).** There is no per-call flag. `JobStarter` looks the spec's key up in one
   table, `RERUN_IF_INTERRUPTED: Record<JobSpecKey, boolean>` (section 4), so a new job kind does not compile without a
   choice. When a job whose key says `true` ends `interrupted`, the starter starts the same spec once more, calls
   `started(newId)` again (so the page's stored job id, and with it the progress it shows, moves to the new job), and
   follows the new id. A second interruption shows the error with Retry. The starter never asks for the result of a job
   that did not end `done`, so "That job has no result" cannot come from a restart (fixes (b)).
5. **Every Cancel and Retry has a distinct name (shared).** `pd-job` names its buttons "Cancel <title>" and
   "Retry <title>", adding ", <subject>" when the job has one. The visible text stays "Cancel" and "Retry"; the
   accessible name starts with it (WCAG 2.5.3, label in name). Fixes (c) for every app at once.

### Why not the instance in the shared snapshot

- Only Dyno has the problem (F7). A window whose jobs die with it never sees a job of an earlier run.
- An instance field would change how the store keys its jobs: `(instance, id)` instead of `id`. That touches `follow`,
  `cancel`, the `pd-job` outputs and every app's call sites. Unique ids give the same safety with no shared key change.
- Dyno already has a per-run identity (F8). systemd does the same thing: `$INVOCATION_ID` is one random id per run of a
  unit, kept by the manager, not by each job (section 3).

### The idempotency key (D-7) and this design

D-7 (2026-11-12): keep Debrief's J1 and also build the shared `paddock-jobs` key (1.5 days), registered as mechanism
`job-idempotency-key` (`docs/known-copies.toml:633`, status `planned`, canonical `crates/paddock-jobs/src/queue.rs`). The
two are designed apart. Neither builds the other's part:

- A key lives in a registry, so it is scoped to one agent run by construction. After a restart the old run's keys are
  gone with it. A page that reads again gets a new job, or joins one the new run already started once Dyno adopts the
  key. Nothing in this note depends on the key, and the key needs nothing from this note.
- Both land in `paddock-jobs` and touch `model.rs`/`job.rs`, so they go one after the other on the same files, not in
  parallel. This note's slice S1 is small and goes first, because Dyno 1.1.1 is the first release.
- Found while reading: Dyno's agent already has its own key. `JobCentre::start` answers a running job whose `JobSpec` is
  equal (`apps/dyno/crates/dyno-agent/src/jobs.rs:88-94`). It is not registered as a copy of `job-idempotency-key`.
  Slice S3 registers it (rule 55): a `[[mechanism.copy]]` under `job-idempotency-key` in `docs/known-copies.toml` with
  `paths = ["apps/dyno/crates/dyno-agent/src/jobs.rs"]`, `symbols = ["item:start"]`, `owner = "dyno"`,
  `delete_by = "dyno-v1.2.0"`. Dyno adopts the shared key when it is built (after S1, decision D3), not in WP-8.

## 3. Common practice (rule 76, ER-37)

| Software | What happens to work when its process restarts | Source |
|---|---|---|
| systemd | Each run of a unit has its own random 128-bit `$INVOCATION_ID`, which "may be used to identify this specific runtime cycle"; a new id is assigned at every start. | [systemd.exec(5), `$INVOCATION_ID`](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html) |
| Docker | "By default, when the Docker daemon terminates, it shuts down running containers"; live restore is the opt-in to keep them running. | [Docker: live restore](https://docs.docker.com/engine/daemon/live-restore/) |
| VS Code | When the extension host's RPC protocol is disposed, every pending request is rejected with a canceled error ("Release all outstanding promises with a canceled error"). The user sees "Extension host terminated unexpectedly" and restarts. Nothing is re-run on its own. | [`rpcProtocol.ts`, `dispose()`](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/services/extensions/common/rpcProtocol.ts) |
| BullMQ | A job whose worker stops renewing its lock is "stalled" and moved back to waiting (run again) or to failed. Past `maxStalledCount` (default 1) it fails with "job stalled more than allowable limit". | [BullMQ: stalled jobs](https://docs.bullmq.io/guide/jobs/stalled) |
| Sidekiq | Open source: "If Sidekiq crashes while processing that job, it is lost forever." Recovery is a paid feature: Sidekiq Pro's `super_fetch` re-queues orphaned jobs after the dead process's heartbeat expires, and a job recovered three times within 72 hours is treated as a poison pill and moved to Dead. | [Sidekiq wiki: Reliability](https://github.com/sidekiq/sidekiq/wiki/Reliability) |
| Apple (XPC) | `NSXPCConnection.interruptionHandler`: "An interruption handler that is called if the remote process exits or crashes." "It may be possible to re-establish the connection by simply sending another message." Outstanding replies are delivered first; re-sending is the client's choice. | [Apple: `interruptionHandler`](https://developer.apple.com/documentation/foundation/nsxpcconnection/interruptionhandler) |
| Windows (COM) | An out-of-process call whose server went away fails with a typed code that says whether the work may have run: `RPC_E_SERVER_DIED` (0x80010007) "The call may have executed", `RPC_E_SERVER_DIED_DNE` (0x80010012) "The call did not execute". COM does not re-run anything; the client decides. | [Microsoft: COM error codes (STG, RPC)](https://learn.microsoft.com/en-us/windows/win32/com/com-error-codes-3) |

The common pattern: give each run an identity, end the old run's work with a typed outcome the user can see, and run it
again by itself only when that is safe, with a cap. The platform vendors (Apple XPC, Windows COM) stop at the typed
outcome and leave the re-run to the client. This design does the same: the endpoint identity, `Interrupted` (like
`RPC_E_SERVER_DIED`, "may have run"), and, as the client, one re-read for pure reads only. It goes no further than
BullMQ's default of one re-run. The cheapest common practice is VS Code's, Apple's and COM's: end the work, run nothing
again, and let the user press Retry. That is option D1 (c).

Cost against that baseline: the chosen design costs **4.25-6.25 agent-days** (section 9). The VS Code-style option costs
about **2.5-3.5**: no shared state, no re-read, and the page shows the error with Retry.

## 4. Wire and type changes (all append-only)

**`paddock-jobs` (shared Rust)**

- `JobState` gains a last variant, `Interrupted { message: String }`. It is terminal (`is_terminal`). `transition` allows
  queued → interrupted and running → interrupted; from a terminal state it is refused like the others. `name()` is
  `"interrupted"`. JSON: `{"state":"interrupted","message":"..."}`.
- A constructor, `JobSnapshot::interrupted(last: &JobSnapshot, seq: u64, message)`, makes the ending: the given `seq`
  (the caller passes the highest it has seen plus 1; the constructor refuses one not above `last.seq` in a debug
  assertion), the same id, kind, title, subject and elapsed time, `eta_ms: None`, `cancellable: false`.
- **Every exhaustive non-test `match` on `JobState` or `WireState` gets an `Interrupted` arm in S1** (from
  `git grep` of the non-test sources on the train head; arms with a `_` wildcard are listed too, because the wildcard
  would give the wrong answer):

| Where | What it matches | `Interrupted` arm |
|---|---|---|
| `crates/paddock-jobs/src/model.rs:134` `JobState::name` | exhaustive | `"interrupted"` |
| `crates/paddock-jobs/src/model.rs:145` `JobState::is_terminal` | `matches!` list | add to the terminal list |
| `crates/paddock-jobs/src/model.rs:153` `JobState::transition` | allowed pairs | add `Queued → Interrupted` (running already allows anything) |
| `crates/paddock-jobs/src/wire.rs:74` `WireSnapshot::is_terminal` | `matches!` list | add to the terminal list |
| `crates/paddock-jobs/src/wire.rs:82` `From<&JobSnapshot> for WireSnapshot` | exhaustive | `Interrupted { message } → WireState::Interrupted(message)` |
| `crates/paddock-jobs/src/wire.rs:115` `From<WireSnapshot> for JobSnapshot` | exhaustive | the reverse |
| `apps/pitwall/app/src-tauri/src/jobs.rs:194-235` (the snapshot to `JobEvent` route) | exhaustive | `JobEvent::Failed` with `ErrorCode::Internal` and the message (Pitwall's jobs run in its own process and never get one; the arm keeps it compiling and honest) |
| `apps/pitwall/app/src-tauri/src/jobs.rs:166-172` `terminal_word` | `_ => "total"` wildcard | `"interrupted"`, so the Performance section does not count it as "total" |
| `apps/dyno/crates/dyno-agent/src/cli.rs:193-199` (the command line waits for a job) | `Some(_) => {}` wildcard | return the message as an error, so the command line never waits on an ended job |

  Spotter (`apps/spotter/app/src-tauri/src/jobs.rs`) and Debrief have their own `JobState` types and are not affected
  until Debrief's M2a. On the TypeScript side the switches over `SuJobSnapshot['state']` are `jobView` (`job-view.ts`),
  `finishedNotice` (`jobs.store.ts:45`) and `parseJobSnapshot` (`wire.ts`); the type checker lists any other.
- `wire::WireState` gains a last variant, `Interrupted(String)`, so `From` both ways stays lossless. The agent never
  sends it (only the app makes it), but a postcard enum is positional, so Dyno's `PROTOCOL_VERSION` goes 8 → 9 with a
  version note in `protocol.rs`, and `tests/protocol_golden.rs` and `tests/jobs_wire.rs` pin the new bytes.
  `./dev wire sync` (ADR 0049 C80) is not built on the train head, so this is done by hand as for versions 7 and 8.
- ADR 0008 §1 (job model) gets a dated amendment line naming the new state.

**`@decryp7/paddock-ui` (shared TypeScript)**

- `SuJobState` (`src/lib/job-view.ts:26-31`) gains `{ state: 'interrupted'; message: string }`.
- `parseJobSnapshot` (`shell/jobs/src/wire.ts`) accepts it. Today an unknown state makes the snapshot `null`, so a webview
  built without it would drop the event and keep the job "running". Both sides ship in one installer.
- `jobView`: headline "Interrupted", tone `warning`, terminal, `canRetry: true`, detail = the message, announcement
  "<title>: interrupted".
- `finishedNotice`: a `warning` notice "<title> interrupted: <message>", unless someone follows the job.
- `pd-job`: `[attr.aria-label]` on Cancel and Retry, as in section 2 point 5 (two `$localize` ids:
  `pd.ui.job.cancel-named` and `pd.ui.job.retry-named`).

**Dyno**

- `dyno-core::jobs`: `pub fn first_job_id(started_ms: i64) -> (u64, Option<Clamped>)` (the clamp of section 2 point 1,
  saturating; `Clamped` carries the raw value for the start log). `APP_JOB_ID_BASE` moves here from `shell.rs` so one
  test can compare the two.
- `dyno-agent`: `start_inner` reads the start time once and passes it to `JobCentre::new(shared, workers, first_id)`
  (which calls `registry.start_ids_at(first_id)`) and to `server::bind`, which writes it as `Discovery.started` instead
  of reading the clock again.
- `app/src-tauri` link: `Event::Connected` carries an `AgentRunId { endpoint: String, pid: u32, started_ms: i64 }` taken
  from the discovery file whose token authenticated the connection, so it names this run (rule 53). `AgentLink` exposes
  `agent_run()` for the connection in use. The comparison uses the whole value; logs carry only
  `paddock_log::fingerprint(endpoint)` as today. Not a new wire field.
- `app/src-tauri`: `AgentJobs`, a pure type: `seen(&WireSnapshot)` (keeps the newest snapshot and the highest `seq` per
  id, forgets a job once it ends), `connected(run, list) -> Vec<JobSnapshot>` (section 2 point 2: the list to relay
  plus the `Interrupted` endings), a cap of 256 remembered jobs (oldest dropped first, logged). `Shell::job`, the `dyno`
  command's `Reply::Jobs`/`Reply::Job` path and `Shell::connected` feed it; `Shell::connected` emits what
  `connected` returns on `paddock://jobs`.
- `app/src`: `JobStarter.run(spec, started?)` is unchanged in shape. `RERUN_IF_INTERRUPTED: Record<JobSpecKey, boolean>`
  in `core/job-starter.ts` decides; `JobSpecKey` is the union of the `JobSpec` keys, so a new kind does not compile
  without a row:

| Job (`JobSpecKey`) | Re-read after a restart | Why |
|---|---|---|
| `sleepBlockers`, `bootAnalysis`, `appUsage`, `softwareInventory` (Extras tabs) | yes | pure reads; the tab already reads on open |
| `findFiles` (Extras > Files) | yes | a pure read of a folder the person chose |
| `rescanIssues` (Issues) | yes | a pure read |
| `fileHash` (Lookup) | yes | a pure read of one file |
| `helper` (Setting up administrator readings) | **no** | changes the machine (installs or removes the helper) |
| `installTool` (disk-health tool) | **no** | changes the machine |
| `wolWake` (Waking a computer) | **no** | sends packets; the person decides whether to send them again |
| `updateList` (Network > lists, "Update now", `state/intel.store.ts:88-98`) | **no** | a download the agent also schedules itself (F2). The page does not follow the job id: it puts the list in `updating` and polls the intel status until the list's `lastAttemptMs` changes or `UPDATE_POLL_MAX_MS` passes (`intel.store.ts:49-66`). When the update is interrupted, the agent has set the attempt time at the start of the fetch (`dyno-agent/src/intel_fetch.rs:244-245`); whether the new run's status shows it depends on whether it was saved before the agent died (not checked here). Either way the spinner ends: at once if the new status shows the new attempt, otherwise at `UPDATE_POLL_MAX_MS` (60 s). The row then shows its status as the new agent reports it, and Update now works again (the minimum interval may refuse it as too soon if the attempt was saved). No change in WP-8; the spec of (b)4 pins that the spinner ends within `UPDATE_POLL_MAX_MS` after an interruption. |
| `updateGeo`, `rebuildIntel` | **no** | started only by the agent (F2); the new run starts what is due |

**Logging (ER-27).** On each connect the shell logs one line: whether the run changed (old and new `AgentRun`
fingerprints), how many jobs were interrupted, and their ids and kind slugs. Titles and subjects are not logged: a
subject can be a path. The starter's re-read logs the old and new ids and the kind. No user content.

## 5. What the person sees (wording)

| Where | When | Text |
|---|---|---|
| Jobs list, the job | the agent restarted during the job | **Interrupted**. Dyno's background agent restarted, so this stopped. No Cancel. |
| Jobs list, the job | the connection came back, the job's end was missed and is no longer kept | **Interrupted**. Dyno lost track of this while it reconnected. It may have finished. |
| Notice | an interrupted job nobody follows | Building the match index interrupted: Dyno's background agent restarted, so this stopped. |
| Extras tab (Sleep, Boot, App usage, Software), Files, Issues, Lookup | the first interruption | Nothing new: the tab shows its normal progress for the new read, then the result. |
| The same pages | interrupted a second time | Dyno's background agent restarted again, so this read stopped. Retry to read it again. (error with Retry) |
| Helper card, tool install, Wake-on-LAN | interrupted | The card's error with the job's message and its own Retry. Nothing runs again by itself. |
| Any Cancel button (screen reader) | always | "Cancel Reading sleep blockers"; with a subject, the subject as the job carries it: "Cancel Finding files, /Users/me/Downloads" (the folder), "Cancel Hashing a file, C:\Users\me\Downloads\setup.exe" (the full path: `subject_of` passes the whole path, `dyno-agent/src/jobs.rs`). The visible text stays "Cancel". |
| Any Retry button (screen reader) | failed or interrupted, where the host offers Retry | "Retry Reading sleep blockers". |

While the agent is away and has not come back yet, the job still shows as running, because the agent may come back as the
same run. Cancel then gives the existing notice that the job could not be cancelled because the agent is not running.

## 6. Failure modes

- **The clock went back** to within about `n_old / 16` ms of an earlier run's start (section 2, point 1; `n_old` is the
  number of jobs the earlier run started, so the window is a few milliseconds for a run of a few dozen jobs). A new job
  may then get the id of an old job that the store already holds as ended. Two things follow:
  - the store ignores the new job's snapshots until their `seq` passes the old one, so the Jobs list shows the old
    ending for that id for a while;
  - `follow(id)` finds that id already ended in the store and **resolves at once** with the old snapshot. If it is
    `interrupted`, the starter re-reads once (a reading page) and the agent's spec dedupe hands back the new job's id
    again, which resolves the same way, so the page ends at "interrupted again" with Retry. If it is `done`, the starter
    asks the new agent for the result of that id, which is the page's own new job: while it runs the agent answers
    "That job has no result", and the page shows that error with Retry. If it is `failed` or `cancelled`, the page shows
    that ending.
  So the page can show a wrong ending with Retry, but never another job's data: a result is only ever asked of the
  agent that owns the id now, and that job is the page's own. Not handled further: it needs a clock jump into that
  window. The `first_job_id` clamp covers the far edges (before 1970, after 2136).
- **The agent restarts in a loop.** Each run interrupts the jobs of the run before it once. A page reads again once, then
  shows Retry. Nothing piles up.
- **Several windows.** Every window gets the same `Interrupted` events from the shell. If several windows hold the same
  reading page, each re-reads, and the agent's spec dedupe (`dyno-agent/src/jobs.rs:88-94`: a running job with an equal
  `JobSpec` answers the start) folds them into one job, which all of them follow. A window opened later reads the list,
  which has no old jobs. The settings and dialog windows that hold a Jobs store get the events too.
- **Memory.** `AgentJobs` holds only jobs that have not ended, capped at 256. The agent runs at most 8 at once
  (`MAX_JOB_WORKERS`) plus its short queue.

## 7. Tests (failing first, rule 5)

**(a) The old run's job ends and its Cancel goes away**

1. `dyno-core` unit tests of `first_job_id`: it rises with the start time; two runs 1 ms apart with 15 jobs each do not
   overlap; the base stays below `APP_JOB_ID_BASE` up to 2136-01-01 and is a safe integer; **both clamp edges**: a
   negative `started_ms` gives base 0 and reports a clamp, and `i64::MAX` gives `(APP_JOB_ID_BASE / 16 − 1) × 16` with
   no overflow and reports a clamp. This is the plan's "Rust test for the snapshot identity". It fails today: the
   function does not exist and the agent counts from 1.
2. `dyno-agent/tests/jobs.rs`: two agents started one after the other on one data folder, with injected start times,
   hand out disjoint ids, and each agent's first id matches the `started` its discovery file names. Fails today: both
   start at 1, and the discovery time is a second clock read.
3. `paddock-jobs/tests/it/model_and_cancel.rs` and `wire.rs`: the transitions to `Interrupted` (queued and running yes,
   done no), the JSON shape, the lossless wire round trip, `is_terminal` on both types, and `JobSnapshot::interrupted`
   taking the given `seq` and setting `cancellable: false`. Pitwall's route test gets one case for the new arm.
4. `app/src-tauri` unit tests of `AgentJobs`, with explicit `seq` values:
   - another run interrupts every remembered active job, also one whose id the new list happens to name;
   - the same run relays the list, refreshes the remembered snapshots, and interrupts only the missing ones, with the
     "lost track" message;
   - the `Interrupted` `seq` is one above the **highest** `seq` seen for the id, including one seen only in a `dyno`
     reply (seen `seq` 7 by event and 9 by reply gives 10);
   - **a job learned only from a list** (no event ever, as when a window's `jobs_snapshot` read through
     `engines.ts:119-131` brought it, or the connect read) ends `Interrupted` after a new run;
   - a terminal event forgets a job; the cap drops the oldest and says so in the log.
5. **ER-28 reproduction with the real agent**, in `app/src-tauri/src/link/tests.rs` next to
   `it_reconnects_after_the_agent_goes_away` (the rig runs real agents): start a job that waits on a gate, end the
   agent, let the link start the next one, and let that agent start its own first jobs. Today the test shows the new
   run reusing the old id (F1, F2) and no ending ever reaching the sink for the old job (F4); with the fix the sink gets
   exactly one `Interrupted` for the old id, with the "restarted" message, and the new run's ids are all above the old
   ones. A second test drops only the connection (same run) and asserts no `Interrupted` for a job that is still
   running, and that a job which ended while away is relayed with its real ending.
6. Shared `shell/jobs/src/jobs.store.spec.ts`, "a restarted fake agent", with explicit `seq` values in the fake: the
   store holds job 41 running at `seq` 5; the fake emits `interrupted` at `seq` 6; the job is no longer active, the
   list renders no Cancel for it, a warning notice is posted once, and with a follower the follower gets the snapshot
   and no notice is posted. An `interrupted` at `seq` 5 is ignored (the highest-`seq` rule holds). `wire.spec.ts`
   parses the new state; the `job-view` spec covers headline, tone, detail and Retry.

**(b) A reading page reads again instead of "That job has no result"**

1. First the reproduction (ER-28, causes C1 to C3 in section 1), in `extras.store.spec.ts`. The fake backend counts ids
   from 1 per run and gives every snapshot an explicit `seq`: the Sleep tab starts job 3 (`seq` 1, 2 running); the fake
   "restarts" and its own job 3 (an `updateGeo`) runs and ends `done` at `seq` 4. The test asserts the tab never shows
   "That job has no result" and never calls `jobResult(3)`. It fails today. The real-agent half is (a)5.
2. With the fix: the old job's `interrupted` event makes the starter start `sleepBlockers` once more; `started` fires
   again with the new id, `readJobIds.sleep` moves to it, the tab shows the new job's progress and then the data.
   `startJob` is called twice, and `jobResult` only for the new id.
3. A second interruption shows the error with Retry; `startJob` is not called a third time.
4. The table: a spec asserts `RERUN_IF_INTERRUPTED` is `false` for `helper`, `installTool`, `wolWake` and `updateList`,
   and that an interrupted `helper` job ends the helper store's run with its error and no second `startJob`. The
   `Record` type makes a new kind choose. An `intel.store` spec with a fake clock interrupts an `updateList` and
   asserts the list leaves `updating` within `UPDATE_POLL_MAX_MS` and no second `startJob` is sent.

**(c) Distinct Cancel names**

1. Shared `src/lib/job.spec.ts`: the Cancel of "Reading sleep blockers" is found by role and name
   `Cancel Reading sleep blockers`; with a subject the name is `Cancel Hashing a file, /tmp/a/setup.exe` (the subject as
   given, a full path); the visible text is "Cancel"; Retry is the same.
2. `shell/jobs/src/jobs-components.spec.ts`: a list of two hash jobs with different subjects has two Cancel buttons with
   different names.
3. The gallery's Playwright axe run on the jobs page stays clean. axe has no rule for duplicate button names, so the
   distinctness is proven by spec 2, not by axe. The plan's "axe shows distinct Cancel names" is met by spec 2 plus a
   clean axe run.

Affected runs (rule 3):
- `paddock-jobs` (fmt, clippy, tests).
- Dyno's Rust workspace: host clippy, `cargo xwin check` for Windows, the agent, core and app tests.
- **Pitwall's Rust workspace: `cargo check` and `cargo clippy --all-targets -- -D warnings`** (its exhaustive match), and
  its jobs tests.
- `ui` unit tests and the gallery project, Dyno's unit tests and its jobs and extras e2e on Chromium.
- Pitwall, Spotter and the demos type-check against the new union; no behaviour of theirs changes.

## 8. VM row (macOS and Windows; rule 71)

**Row "restart the agent during a job"**

- **Precondition, checked first:** Dyno is open on Extras > Files with a search of a large folder running. The Jobs list is
  open and shows "Finding files" with a growing count. Read the agent's pid and start time from `agent.json`.
- **Action:** end the agent. On macOS use `kill -9` on that pid, after checking the pid and start time again. On Windows use
  `taskkill /F /PID` on that pid, after checking `dyno-agent.exe` and its start time.
- **Probe, shown to work:** `agent.json` names a new pid and start time, and the app log has the connect line with a new
  `AgentRun` and "1 interrupted". If either is missing, the row is `not measured`, not PASS.
- **Expected:**
  - Within 5 s of the new connect, the Jobs list shows "Finding files" as **Interrupted**, with the "restarted" message
    and no Cancel.
  - The Files tab is searching again and ends with a result, with no "That job has no result".
  - The macOS Accessibility Inspector, or Narrator on Windows, reads "Cancel Finding files, <the folder's full path>"
    on the new job's button.

**Row "same run, connection back" (macOS only)**

- **Action:** `kill -STOP` the app (not the agent), start nothing, wait until a quick read started just before (Extras >
  Sleep) has ended in the agent, then `kill -CONT`.
- **Probe, shown to work:** the app log must have a disconnect line **and then** a reconnect line with the **same**
  `AgentRun` as before the pause. A pause that does not drop the connection shows no such pair; then the row is
  `not measured` (the agent kept the connection and pushed the ending normally, which tests nothing here), not PASS.
- **Expected:** the Sleep read shows its result, the connect line says "0 interrupted", and no Interrupted appears for a
  job that ended normally.
- Windows has no plain pause of a process, so this row is macOS only.

## 9. Slices

Branch `dyno-1-1-1/wp8-*` from main after WP-1 (WP-1, `feat/shared-read-retry` `86ddf7a2a`, is on `origin/main`
already). One implementer works through the slices in this order. S1 and S2 touch the shared package and land with
their adoption (rules 39, 42, 43). The D-7 key follows S1 on the same `paddock-jobs` files (decision D3); it is not part
of WP-8's days.

| Slice | What | Owned files | Days |
|---|---|---|---|
| S1 | `Interrupted` in `paddock-jobs` (model, `JobSnapshot::interrupted`, `WireState`) and in `paddock-ui` (union, parse, view, notice); every match in the section 4 table, Pitwall's two and Dyno's command line included; ADR 0008 amendment line; shared CHANGELOG; tests (a)3 and (a)6 | `crates/paddock-jobs/src/{model,wire}.rs` and tests; `apps/pitwall/app/src-tauri/src/jobs.rs`; `apps/dyno/crates/dyno-agent/src/cli.rs`; `ui/projects/paddock-ui/src/lib/job-view.ts`, `shell/jobs/src/{wire,jobs.store}.ts` and specs; `docs/adr/0008-long-operations.md`; `CHANGELOG.md` | 1-1.5 |
| S2 | Distinct Cancel and Retry names in `pd-job`; tests (c)1 to (c)3 | `ui/projects/paddock-ui/src/lib/job.ts`, `job.spec.ts`, `shell/jobs/src/jobs-components.spec.ts`, the gallery axe spec if needed | 0.25-0.5 |
| S3 | Unique agent ids: `first_job_id` with its clamp, `APP_JOB_ID_BASE` moved, one start-time read in `start_inner` passed to `JobCentre::new` and `server::bind`; protocol 9 with its note and goldens; the `job-idempotency-key` copy entry for Dyno's spec dedupe; tests (a)1 and (a)2 | `apps/dyno/crates/dyno-core/src/{jobs,protocol}.rs`, `tests/{protocol_golden,jobs_wire}.rs`, `apps/dyno/crates/dyno-agent/src/{jobs,agent,lib,server}.rs`, `tests/jobs.rs`, `apps/dyno/app/src-tauri/src/shell.rs` (constant only), `docs/known-copies.toml` (one `[[mechanism.copy]]`) | 0.5-0.75 |
| S4 | `AgentJobs` fed by events, `dyno` replies and the connect read; the run identity on the link; the every-connect step in `Shell::connected`; the log line; tests (a)4 and (a)5 | `apps/dyno/app/src-tauri/src/{agent_jobs.rs (new), shell.rs}`, the `dyno` command's reply path, `link/{mod,machine,driver,tests}.rs` | 1.25-1.75 |
| S5 | `RERUN_IF_INTERRUPTED` and the starter's re-run; the reproduction first; tests (b)1 to (b)4 | `apps/dyno/app/src/app/core/job-starter.ts`, `state/{extras,issues,lookup,helper,tool,wol}.store.ts` and specs, `testing/job-test-support.ts` | 0.5-1 |
| S6 | VM rows on macOS and Windows (section 8) | VM row text in the harness; the report | 0.5 |
| S7 | Dyno CHANGELOG and one user-guide line ("A job that was running when the background agent restarted shows Interrupted; reads start again by themselves") | `apps/dyno/CHANGELOG.md`, `apps/dyno/docs/user-guide.md` | 0.25 |
| | **Total** | | **4.25-6.25** |

The plan row says 3-6; the every-connect read (decision D2) adds about 0.25 day at the top. The one independent review
covers S1 to S5 together, before landing.

## 10. Decisions

Answered by the user on 2026-11-12 (relayed by the coordinator after review round 1). The options are kept as they were
put.

- **D1, how far the fix goes: (a) chosen.**
  - **(a) Chosen:** as above. The shared `Interrupted` state, and pure reads run once more by themselves. 4.25-6.25 days.
  - (b) The same in Dyno only. The ending is a `failed` with the message. The page reads again when the agent status
    shows a new run; that needs a new `run` field on Dyno's agent status and the status to reach the window before the
    job event. About 4-5.5 days. Debrief then adds `Interrupted` at M2a anyway.
  - (c) Cheapest, common practice (VS Code, Apple XPC, COM): the jobs end as `failed` with the message, and nothing runs
    again by itself. The page shows the error with Retry. About 2.5-3.5 days.
- **D2, reconnects: read and relay the job list on every reconnect, new run or same run (chosen).** The alternative
  (handle only a new run, about 0.25 day less) left an ending missed during a dropped connection showing "running".
- **D3, the idempotency key: build the D-7 key after S1, on the same files (chosen).** Dyno's spec dedupe is
  registered as its copy in S3 and adopts the key when it lands. Folding the key into WP-8 (+1.5 days on Dyno's
  critical path) was not chosen.
