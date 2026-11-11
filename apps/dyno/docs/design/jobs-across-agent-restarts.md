# Jobs across agent restarts (Dyno 1.1.1 WP-8)

- Status: Proposed, 2026-11-12. Design note for [plan](../plans/dyno-1-1-1.md) row WP-8 (line 99). Nothing here is built.
  One independent review follows; decisions D1 to D3 at the end go to the user.
- Heads read (ADR 0091): `origin/train/2026-11-11-5` = `607bb7928`, `origin/main` = `e2a318345`, the Dyno 1.1.0 tag
  `dyno-v1.1.0` (commit `8efce3fc3`). Re-read right before the commit.
- Sources outside the repo: the coordinator log `/Volumes/d7sst7/dev-targets/handoff/2026-10-29-overnight.md` (cited as
  `log:<line>`, as the plan does); the user's decisions of 2026-11-12 (memory `decisions-2026-11-12-pitwall-scope`, D-7).

## 1. The problem, as shipped

Checked in the code at the tag `dyno-v1.1.0` and unchanged on the train head:

| # | Fact | Where |
|---|---|---|
| F1 | The agent's job ids start at 1 in every agent run. The agent's registry never calls `start_ids_at`; only the app's own registry does (`APP_JOB_ID_BASE`). | `crates/paddock-jobs/src/job.rs:125` (`AtomicU64::new(1)`; `:109` at the tag); `apps/dyno/crates/dyno-agent/src/jobs.rs:66`; `apps/dyno/app/src-tauri/src/shell.rs:188,276` |
| F2 | A new agent starts jobs on its own at once: `RebuildIntel`, then the due list and location updates. So a restarted agent hands out ids 1, 2, 3 ... again within seconds. | `apps/dyno/crates/dyno-agent/src/lib.rs:326,332` |
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

1. **Ids are unique across agent runs (Dyno only).** The agent's registry starts its ids at `first_job_id(started_ms)` =
   `started_ms × 16`. `started_ms` is the start time the agent already writes into its discovery file. This fixes F1 to
   F3 with no change to the store's highest-`seq` rule. The base grows with the start time, so the list stays in start
   order. It stays below `APP_JOB_ID_BASE` (`0x4c4f_4749_4e00`) until the year 2136. It is a safe integer for the
   webview. Two runs overlap only if a run started 16 or more jobs per millisecond of its life, or if the clock went
   back to within a few milliseconds of an earlier start. The second is a known edge, with the result in section 6.
2. **The app ends the jobs of a run that is gone (Dyno only).** The shell remembers the agent jobs it relayed that have not
   ended (`AgentJobs`, a small pure type in `app/src-tauri`). On every connect it compares the run (F8: the endpoint's
   fingerprint, which the link passes up from the connect it made) with the last one:
   - **Another run:** every remembered job ends as `Interrupted` with "Dyno's background agent restarted, so this
     stopped.". The list is not needed for this: another run cannot own an old job.
   - **The same run** (the connection dropped and came back): the shell reads `Request::Jobs` and relays every entry. The
     store keeps the highest `seq`, so endings missed while away now arrive. A remembered job missing from the list
     (more than 16 ended meanwhile) ends as `Interrupted` with "Dyno lost track of this while it reconnected. It may have
     finished.".
   The `Interrupted` snapshot has the remembered `seq + 1`, so every window's store takes it. Cancel goes away because
   the job is no longer active (fixes (a)).
3. **`Interrupted` is a shared terminal state (append-only).** The page needs a typed reason to read again by itself, and
   a message string is not a type. A Dyno-only signal needs the agent status and the job event to arrive in order (option
   D1 (b)). Debrief already has this state (F9) and brings it into `paddock-jobs` at M2a. So the state is added once,
   now, with Debrief's name, and Debrief's copy folds into it at M2a (rule 55).
4. **Reading pages read again once (Dyno only).** `JobStarter.run` takes `rerunIfInterrupted`. When the job ends
   `interrupted`, the starter starts the same spec once more and follows the new id. A second interruption shows the
   error with Retry. It never asks for the result of a job that did not end `done`, so "That job has no result" cannot
   come from a restart (fixes (b)). Only pure reads opt in: see the table in section 4.
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
  equal (`apps/dyno/crates/dyno-agent/src/jobs.rs:88-94`). It is not registered as a copy of `job-idempotency-key`. The
  key's work package should add it as a copy with a delete-by tag (rule 55). Not in WP-8.

## 3. Common practice (rule 76, ER-37)

| Software | What happens to work when its process restarts | Source |
|---|---|---|
| systemd | Each run of a unit has its own random 128-bit `$INVOCATION_ID`, which "may be used to identify this specific runtime cycle"; a new id is assigned at every start. | [systemd.exec(5), `$INVOCATION_ID`](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html) |
| Docker | "By default, when the Docker daemon terminates, it shuts down running containers"; live restore is the opt-in to keep them running. | [Docker: live restore](https://docs.docker.com/engine/daemon/live-restore/) |
| VS Code | When the extension host's RPC protocol is disposed, every pending request is rejected with a canceled error ("Release all outstanding promises with a canceled error"). The user sees "Extension host terminated unexpectedly" and restarts. Nothing is re-run on its own. | [`rpcProtocol.ts`, `dispose()`](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/services/extensions/common/rpcProtocol.ts) |
| BullMQ | A job whose worker stops renewing its lock is "stalled" and moved back to waiting (run again) or to failed. Past `maxStalledCount` (default 1) it fails with "job stalled more than allowable limit". | [BullMQ: stalled jobs](https://docs.bullmq.io/guide/jobs/stalled) |
| Sidekiq | Open source: "If Sidekiq crashes while processing that job, it is lost forever." Pro's `super_fetch` re-queues orphaned jobs after the dead process's heartbeat expires. A job rescued three times is moved to Dead. | [Sidekiq wiki: Reliability](https://github.com/sidekiq/sidekiq/wiki/Reliability) |

The common pattern: give each run an identity, end the old run's work with a typed outcome the user can see, and run it
again by itself only when that is safe, with a cap. This design does the same: the endpoint fingerprint, `Interrupted`, and one
re-read for pure reads only. It goes no further than BullMQ's default of one re-run. The cheapest common practice is VS
Code's: end the work, run nothing again, and let the user press Retry. That is option D1 (c).

Cost against that baseline: the recommended design costs **4-6 agent-days** (section 8). The VS Code-style option costs
about **2.5-3.5**: no shared state, no re-read, and the page shows the error with Retry.

## 4. Wire and type changes (all append-only)

**`paddock-jobs` (shared Rust)**

- `JobState` gains a last variant, `Interrupted { message: String }`. It is terminal (`is_terminal`). `transition` allows
  queued → interrupted and running → interrupted; from a terminal state it is refused like the others. `name()` is
  `"interrupted"`. JSON: `{"state":"interrupted","message":"..."}`.
- A constructor, `JobSnapshot::interrupted(last: &JobSnapshot, message)`, makes the ending: `seq + 1`, the same id,
  kind, title, subject and elapsed time, `eta_ms: None`, `cancellable: false`.
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

- `dyno-core::jobs`: `pub fn first_job_id(started_ms: i64) -> u64`. `APP_JOB_ID_BASE` moves here from `shell.rs` so one
  test can compare the two.
- `dyno-agent`: `JobCentre::new(shared, workers, first_id)` calls `registry.start_ids_at(first_id)`. It is called with
  `first_job_id` of the run's start time (the same value as `Discovery.started`).
- `app/src-tauri` link: `Event::Connected` and the connect log carry the endpoint fingerprint (`AgentRun`). `AgentLink`
  exposes `agent_run()` for the connection in use. This is not a new wire field: it comes from the discovery file whose
  token authenticated the connection, so it names this run (rule 53).
- `app/src-tauri`: `AgentJobs`, a pure type with `seen(&WireSnapshot)`, `reconnected(run, list) -> Vec<JobSnapshot>`
  and a cap of 256 remembered jobs (oldest dropped first, logged). `Shell::job` feeds it; `Shell::connected` asks it
  and emits the result on `paddock://jobs`.
- `app/src`: `JobStarter.run(spec, started?, { rerunIfInterrupted })`. Every page that starts a job declares its choice
  in one typed table (`Record<JobSpecKey, boolean>`), so a new job kind does not compile without one:

| Job | Re-read after a restart | Why |
|---|---|---|
| Sleep blockers, boot analysis, app usage, installed software (Extras tabs) | yes | pure reads; the tab already reads on open |
| Finding files (Extras > Files) | yes | a pure read of a folder the person chose |
| Reading system logs (Issues rescan) | yes | a pure read |
| Hashing a file (Lookup) | yes | a pure read of one file |
| Setting up administrator readings (helper) | no | changes the machine (installs or removes the helper) |
| Installing the disk-health tool | no | changes the machine |
| Waking a computer | no | sends packets; the person decides whether to send them again |
| List, location and match-index updates | not started by a page | the new agent schedules the due ones itself (F2) |

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
| Any Cancel button (screen reader) | always | "Cancel Reading sleep blockers", "Cancel Hashing a file, ntdll.pdb". The visible text stays "Cancel". |
| Any Retry button (screen reader) | failed or interrupted, where the host offers Retry | "Retry Reading sleep blockers". |

While the agent is away and has not come back yet, the job still shows as running, because the agent may come back as the
same run. Cancel then gives the existing notice that the job could not be cancelled because the agent is not running.

## 6. Failure modes

- **The clock went back** to within a few milliseconds of an earlier run's start (section 2, point 1). A new job may then
  get the id of an old, interrupted one, and the store ignores its first snapshots until their `seq` passes the old one.
  The page then shows "Still working" for a moment longer; nothing is lost or mixed up, because the page never asks
  for a result unless the job ended `done` with its own id. Not handled further: it needs a clock jump into a window a
  few milliseconds wide.
- **The agent restarts in a loop.** Each run interrupts the jobs of the run before it once. A page reads again once, then
  shows Retry. Nothing piles up.
- **Several windows.** Every window gets the same `Interrupted` events from the shell. A window opened later reads the
  list, which has no old jobs. The settings and dialog windows that hold a Jobs store get the events too.
- **Memory.** `AgentJobs` holds only jobs that have not ended, capped at 256. The agent runs at most 8 at once
  (`MAX_JOB_WORKERS`) plus its short queue.

## 7. Tests (failing first, rule 5)

**(a) The old run's job ends and its Cancel goes away**

1. `dyno-core` unit tests: `first_job_id` rises with the start time; two runs 1 ms apart with 15 jobs each do not overlap;
   the base stays below `APP_JOB_ID_BASE` up to 2136-01-01 and is a safe integer. This is the plan's "Rust test for the
   snapshot identity". It fails today: the function does not exist and the agent counts from 1.
2. `dyno-agent/tests/jobs.rs`: two agents started one after the other on one data folder, with injected start times,
   hand out disjoint ids. Fails today: both start at 1.
3. `paddock-jobs/tests/it/model_and_cancel.rs` and `wire.rs`: the transitions to `Interrupted` (queued and running yes,
   done no), the JSON shape, the lossless wire round trip, `is_terminal`, and `JobSnapshot::interrupted` setting
   `seq + 1` and `cancellable: false`.
4. `app/src-tauri` unit tests of `AgentJobs`: another run interrupts every remembered job (also one that the new list
   happens to name); the same run relays the list and interrupts only the missing ones, with the "lost track" message;
   terminal events forget a job; the cap drops the oldest and says so in the log.
5. `app/src-tauri/src/link/tests.rs`, next to `it_reconnects_after_the_agent_goes_away`: the rig starts a job that waits on
   a gate, ends the agent, waits for the new run, and asserts the sink got one `Interrupted` for the old id with the
   "restarted" message. A second test drops only the connection and asserts no `Interrupted` for a job that is still
   running.
6. Shared `shell/jobs/src/jobs.store.spec.ts`, "a restarted fake agent": the store holds a running job; the fake emits its
   `interrupted` snapshot; the job is no longer active, the list renders no Cancel for it, a warning notice is posted
   once, and a follower gets the snapshot and no notice is posted. `wire.spec.ts` parses the new state; the `job-view`
   spec covers headline, tone, detail and Retry.

**(b) A reading page reads again instead of "That job has no result"**

1. First the reproduction (ER-28, causes C1 to C3 in section 1). `extras.store.spec.ts` with a fake backend that counts
   ids from 1 per run: the Sleep tab starts job 3; the fake "restarts" and its own job 3 (another kind) ends `done`. The
   test asserts the tab never shows "That job has no result" and never calls `jobResult(3)`. It fails today.
2. With the fix: the old job's `interrupted` event makes the store start `sleepBlockers` once more. The tab shows the new
   job's progress and then the data. `startJob` is called twice, and `jobResult` only for the new id.
3. A second interruption shows the error with Retry; `startJob` is not called a third time.
4. The opt-in table: a spec checks that the helper, tool and wake stores never re-run, and the type makes a new kind
   choose.

**(c) Distinct Cancel names**

1. Shared `src/lib/job.spec.ts`: the Cancel of "Reading sleep blockers" is found by role and name
   `Cancel Reading sleep blockers`; with a subject the name has ", ntdll.pdb"; the visible text is "Cancel"; Retry is the
   same.
2. `shell/jobs/src/jobs-components.spec.ts`: a list of two hash jobs with different subjects has two Cancel buttons with
   different names.
3. The gallery's Playwright axe run on the jobs page stays clean. axe has no rule for duplicate button names, so the
   distinctness is proven by spec 2, not by axe. The plan's "axe shows distinct Cancel names" is met by spec 2 plus a
   clean axe run.

Affected runs (rule 3): `paddock-jobs` and Dyno's Rust workspace (host clippy, `cargo xwin check` for Windows), `ui` unit
tests and the gallery project, Dyno's unit tests and its jobs and extras e2e on Chromium. Pitwall, Spotter and the
demos build against the new union. The plan lists their affected specs; no behaviour of theirs changes.

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
  - The macOS Accessibility Inspector, or Narrator on Windows, reads "Cancel Finding files" on the new job's button.
- **Repeat once for the same run:** on macOS, `kill -STOP` the app (not the agent), let a quick Sleep read end in the agent,
  then `kill -CONT`. Expected: the Sleep read shows its result, and no Interrupted appears for a job that ended normally.
  Windows has no plain pause of a process, so this part is macOS only.

## 9. Slices

Branch `dyno-1-1-1/wp8-*` from main after WP-1 (WP-1, `feat/shared-read-retry` `86ddf7a2a`, is on `origin/main`
already). One implementer works through the slices in this order. S1 and S2 touch the shared package and land with
their adoption (rules 39, 42, 43).

| Slice | What | Owned files | Days |
|---|---|---|---|
| S1 | `Interrupted` in `paddock-jobs` (model, `JobSnapshot::interrupted`, `WireState`) and in `paddock-ui` (union, parse, view, notice); ADR 0008 amendment line; shared CHANGELOG; tests (a)3 and (a)6 | `crates/paddock-jobs/src/{model,wire}.rs`, its tests; `ui/projects/paddock-ui/src/lib/job-view.ts`, `shell/jobs/src/{wire,jobs.store}.ts` and specs; `docs/adr/0008-long-operations.md`; `CHANGELOG.md` | 1-1.5 |
| S2 | Distinct Cancel and Retry names in `pd-job`; tests (c)1 to (c)3 | `ui/projects/paddock-ui/src/lib/job.ts`, `job.spec.ts`, `shell/jobs/src/jobs-components.spec.ts`, the gallery axe spec if needed | 0.25-0.5 |
| S3 | Unique agent ids: `first_job_id`, `APP_JOB_ID_BASE` moved, `JobCentre::new` with the base; protocol 9 with its note and goldens; tests (a)1 and (a)2 | `apps/dyno/crates/dyno-core/src/{jobs,protocol}.rs`, `tests/{protocol_golden,jobs_wire}.rs`, `apps/dyno/crates/dyno-agent/src/{jobs,agent,lib}.rs`, `tests/jobs.rs`, `apps/dyno/app/src-tauri/src/shell.rs` (constant only) | 0.5-0.75 |
| S4 | `AgentJobs`, the run fingerprint on the link, the reconnect step in `Shell::connected`, the log line; tests (a)4 and (a)5 | `apps/dyno/app/src-tauri/src/{agent_jobs.rs (new), shell.rs}`, `link/{mod,machine,driver,tests}.rs` | 1-1.5 |
| S5 | `rerunIfInterrupted` and the opt-in table; the reproduction first; tests (b)1 to (b)4 | `apps/dyno/app/src/app/core/job-starter.ts`, `state/{extras,issues,lookup,helper,tool,wol}.store.ts` and specs, `testing/job-test-support.ts` | 0.5-1 |
| S6 | VM rows on macOS and Windows (section 8) | VM row text in the harness; the report | 0.5 |
| S7 | Dyno CHANGELOG and one user-guide line ("A job that was running when the background agent restarted shows Interrupted; reads start again by themselves") | `apps/dyno/CHANGELOG.md`, `apps/dyno/docs/user-guide.md` | 0.25 |
| | **Total** | | **4-6** |

This is inside the plan's 3-6. The one independent review covers S1 to S5 together, before landing.

## 10. Decisions for the user

- **D1, how far the fix goes.**
  - **(a) Recommended:** as above. The shared `Interrupted` state, and pure reads run once more by themselves.
    4-6 days.
  - **(b)** The same in Dyno only. The ending is a `failed` with the message. The page reads again when the agent status
    shows a new run; that needs a new `run` field on Dyno's agent status and the status to reach the window before the
    job event. About 4-5.5 days. Debrief then adds `Interrupted` at M2a anyway.
  - **(c) Cheapest, common practice (VS Code):** the jobs end as `failed` with the message, and nothing runs again by
    itself. The page shows the error with Retry. No shared state; S5 shrinks to the reproduction and a clear message.
    About 2.5-3.5 days. Plan row WP-8 (b) is then met as "a clear error with Retry" instead of "starts a fresh read".
- **D2, the same-run case.** Relay the agent's list on every reconnect, as in section 2 (recommended, included above).
  Or handle only a new run: about 0.25 day less, but an ending missed during a dropped connection keeps the job
  "running" until the window closes.
- **D3, the idempotency key.** Build the D-7 key after S1, on the same files, and register Dyno's spec-equality
  dedupe (`dyno-agent/src/jobs.rs:88-94`) as its copy then (recommended). Or fold it into WP-8; that adds 1.5 days to
  Dyno's critical path for no change the person sees in 1.1.1.
