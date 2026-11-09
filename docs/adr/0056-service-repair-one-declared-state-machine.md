# 56. Service repair as one declared state machine

- Status: Proposed (2026-11-05; user decision to redesign before the Dyno 1.1.0 tag; revision 4 answers the second review (N1 to N3, R1 to R7) on top of revision 3 (Linux included) and revision 2 (the first review))
- Date: 2026-11-05
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-17 to ER-19 (long operations: off the UI thread,
  labelled progress, no timeouts of running jobs), ER-27 (logs explain every failure), rule 53 (reusable OS names; held
  locks), rule 55 and ADR 0034 (one implementation per mechanism), ADR 0054 (OS integration proved on the real OS), Dyno
  ADR 0010 (platforms and services), catalogue R03 and rule 24 (replace a stale registration once per version). It amends
  the service chapters of those; it does not change what a registration is.

## Pattern

Three review rounds of the move of Dyno's macOS service code into `paddock-service` found the same kind of bug, each time
in a different entry point:

| Round | Finding | Why it happened |
| --- | --- | --- |
| 1 | A registration the OS accepted but that had not answered was forgotten and replaced at the next start (a second "Background Items Added"). | The attempt was saved after the call, not before. |
| 2 | A quit during the 120 s start wait left no record; the next start removed and registered again (P1-1). | The record was written after a long wait; the thread dies with the app. |
| 2 | A job that stays refused was re-registered at every start without limit (P1-2). | The refused branch of `plan` ran before the attempt check; `Failed` was never counted. |
| 2 | A refused launch location could still lose the resume marker (P1-3). | The marker was taken first and the location was checked on one branch only. |
| 2 | A crash between unregister and register lost the service; `resume` ignored the hold; `Await` ignored `caps`; the app's own agent settled a dead service; the repair lock was a time stamp with no owner. | Each entry point (start, first run, switch, resume, repair, uninstall) read and changed the state its own way. |

The pattern: **repair is an undeclared state machine.** Its state is spread over the record (registered version), an
attempt counter, a marker file and a lock; five entry points decide for themselves; it is saved after long waits, not
before them; "answers" is one boolean made of three different signals. Patching instances does not end (ER-21).

## Decision

### 1. One declared persisted state

The record `service-<label>.json` (format 2) holds exactly:

```text
Record { registered_version: Option<String>,        // the last version that saw the service come up
         registered_program: Option<String>,        // optional: the mechanism may not report one
         resume_pending: Option<u64>,               // "Quit and stop measuring" happened at this time (replaces the marker file)
         state: State }

State = Idle
      | Attempting { version, attempts, phase }     // phase = RemovingOld | Registering | Awaiting
      | Removing                                    // an uninstall began (the user's switch went off)
```

- **Phases** say what is safe next, because the record and the OS can otherwise look alike after a crash (a registered item
  with no program name reads the same before and after the new registration). `RemovingOld` is written before the first
  removal; `Registering` before `register` is called; `Awaiting` after the OS accepted it. After a crash: `RemovingOld` +
  registered means the old registration is still there, so replace; `RemovingOld` or `Registering` + absent means finish
  (register only; **no new attempt**, so a crash at the cap cannot lose the service); `Registering` + registered means the
  call may have landed, so await and never remove; `Awaiting` + registered awaits. An old build's running job never settles
  a new version while the phase is `RemovingOld`.
- **A hold is derived, not stored:** `attempts >= MAX_ATTEMPTS` (3) for this version is `Hold::Unanswered(n)` or
  `Hold::Refused(n)` (by the job). `attempts` counts register calls begun, not replaces, so one replace of up to
  `max_cycles + 1` cycles cannot make 12 registers: each cycle is an attempt, the backoff survives across starts, and the
  executor stops at the cap. `attempts` belongs to a `version`; another version's attempts never count.
- Repair or Install pressed by the user is consent: it runs past the cap, does not re-arm `attempts` (a held state stays
  held for unattended events until the service answers or the version changes), and its registers are not counted by the
  invariant "at most 3 per version".
- `Recovered` (the service answers) writes `Idle` and `registered_version`, clearing `attempts`. `Mark` on Linux and Windows
  (a current definition, no answer needed) writes the version only. Uninstall writes `Removing` before the first OS step, and `Idle` with no registered version when the removal is done; a removal that failed or was declined returns the record to the state before it.
  `Removing` is read only when the app has no other word on the switch (below).
- Migration, once: format 1 `{registeredVersion, registeredProgram}` becomes the same with `Idle`; format 1 with
  `attemptedVersion` and `attempts` becomes `Attempting { Awaiting }` with those attempts. A Dyno 1.0.0 install has no
  record (its `service-registered-version` is deleted by 1.1.0 and not read): `Idle`, no version, so the first start
  replaces once. A newer format is left alone. The migration runs **only under the lock**; queries (`status`, `check`)
  read without writing, quarantining or restoring anything.
- A damaged or unreadable record is an input (`Observation.record_problem`): with a service that answers, the start adopts
  it (`Mark`) and removes nothing; without one it replaces once and says so in the log.
- **No OS call without a durable write.** If `record_before` cannot be written the action is not taken: the result is
  `Hold::RecordUnavailable` with the problem and the next step. (The user's own Repair or Install proceeds, loudly.)

### 2. One pure transition function and stepwise execution

```text
transition(record: &Record, event: Event, obs: &Observation, caps: Capabilities) -> Decision { action, record_before }
```

`Event`: `Start`, `Install { unattended }` (the Settings switch on; the first run is `Install { unattended: true }`, whose
action never removes anything), `Uninstall`, `Resume`, `Repair`, `StopRequested` (Quit and stop measuring), `WaitElapsed`,
and the OS results of a running replace: `Unregistered`, `GoneTimedOut`, `RegisterOk`, `RegisterErr(kind)` with `kind` =
`NeedsApproval | NeedsElevation | Declined | Unavailable | NotAsWritten | Other`, `Cancelled`. `repair.rs` is only an
executor: it performs the one OS action the last `Decision` names, feeds back the result as an event and writes
`record_before` first. Every entry point (`on_start`, `install`, `repair`, `uninstall`, `resume`, Dyno's `login.rs`,
`quit.rs`, `shell.rs`) calls it; nothing else decides. Actions: `Leave(hold)`, `Mark`, `Offer`, `Await`, `Replace(reason)`,
`Register` (finish), `Light`, `Remove`, `Refuse(kind)`.

`Observation` separates the signals:

- `service_answers`: the service's own answer. Because the link may have started an agent of its own, it is true only when
  the connected agent is the job's process (pid and start time against `Backend::job_process`, rule 53), or the job is
  running on two looks a poll apart (a KeepAlive crash loop can show `Running` once);
- `job`: `Absent | Running | NotRunning | Refused | Unknown | Failed(LastExit)`;
- `registration`, `login_item`, `program`, `program_exists`, `points_here`, `definition_current`;
- `covered`: the app's own agent holds the data folder; it only keeps a refused job from being replaced while the app
  measures and never settles or counts an attempt (`WaitElapsed` with `covered` is not an attempt);
- `wanted`: the user's switch as the app stores it (`launchAtLogin`, read as `first_run_wanted` does), `first_run_done`,
  `location_ok`, `record_problem`, `now`.

**Reconcile at start** (the settings file is saved by the agent and applied by the app, so a death between them leaves a
mismatch): `wanted = false` with a registration present is `Remove`; `wanted = true`, absent, first run done is `Register`;
the OS `SwitchedOff` always wins. `apply_launch_at_login_with` records the step between service and autostart in the same
way (the app's own login item is reconciled the same: Windows heals with `ensure_app_entry`, macOS through the same
observation). `Removing` is then needed only when the settings file is damaged.

**The link waits for a pending registration.** While the state is `Attempting` in `Registering` or `Awaiting`, the service
probe reports `pending`, and the link neither starts an agent of its own nor retires one before the wait ends (it shows
"Waiting for the background service"), because the service's agent would find the folder locked and exit with code 3. The
fallback and the instance lock are in the simulator.

**Two copies and the location.** `LaunchdBundled` reports `program`, `program_exists` and `points_here` from the `program =`
line of `launchctl print` (to be confirmed on a VM for `BundleProgram`) and from `Manager::with_program`; an `Attempting` of
a newer version is a newer owner. `location_ok = false` (a disk image, a translocated copy) leaves everything, including
`resume_pending`, in place: the marker is part of the record, read and cleared under the lock only when the resume
completes, so it cannot be lost by a crash or a refusal.

### 3. Exclusion, cancel, locks held

The OS lock is `paddock_proc::InstanceLock::claim` (the `HELD_CHECKS` pacing; `Claim::Unsupported` means "run without it,
logged", never "held"), released by the kernel on death (rule 53); the file is never deleted by path. State is read **and
inspected** again after the lock is taken. The lock is **not** held across an `Awaiting` wait: it is released for the wait
and taken again to act (so a user's event is never blocked 231 s); user events and `StopRequested` set the cancel that the
wait honours. A real Cancel source exists (Quit sets it).

### 4. Capabilities

`automatic: false` returns `Offer` for every OS-changing action (replace, register, remove), not only `Replace`; a declined
removal returns the record to the state before it. Non-pinned mechanisms never turn an attempt into an `Await` or a replace
of a current definition. Nothing unattended opens a prompt, Login Items or UAC.

### 5. Waits, progress, logs

Waits use real elapsed time as well as the sum of polls (`max` of the two). Phases are labelled for the tracker; a replace
has a busy state the UI shows (the Settings switch). Every transition logs old state, event, signals, new state and action
in one line (ER-27); a hold says why and the next step in words true for the app. The Settings switch no longer claims to
return at once.

### 6. Linux and Windows in Dyno

**Dyno on Linux moves onto `Manager` + `Systemd` in this change** (user decision, 2026-11-05). `CliService`
(`login.rs`), `dyno_agent::service` (`ensure`, `uninstall`, the direct `systemctl --user start`) and the
`dyno-agent install|uninstall|ensure` verbs (`cli.rs`) all go through the transition; what nothing needs is deleted, so there
is no copy to register. The verbs stay as thin callers of `Manager` (`install` = `Install { unattended: false }`, `ensure` =
`Start`, `uninstall` = `Uninstall`) with the same exit codes and output, because the packages' post-install scripts and the
docs call them; their tests move to the shared simulator plus one wiring test each. The invariants are claimed for macOS
**and** Linux; the simulator has the systemd scenarios (`Caps::Linux`: no pinned identity, `automatic`, a definition that is
compared, `disable`, `stop`, a crash and a restart loop, logout without lingering). Windows Dyno has no agent service (the
agent is started from the per-user `Run` entry by the app); the `WindowsRun` backend is covered by the crate's model and
Dyno changes nothing there.

### 7. The check

The simulator lives in `paddock_service::testing` (rule 43; apps run a wiring contract against it, not their own model). The
store is a port (`RecordStore`) with the file store and an in-memory store, so a run costs microseconds, not 12 ms per
fsync and rename; one test uses the real file store and one kills a real child process holding the lock.

**Exploration is to a fixed point within stated bounds.** Per configuration the explorer visits every reachable `(record, simulated OS)`
state until nothing new appears, but rare events (a fault, an interruption) are limited to a short run of steps on each path
unless `MODEL_DEEP=1` lifts the limit; the default run covers 8 of the 11 configurations (the other 3 need `MODEL_DEEP=1`; the judged-wait amendment below added three, all in the default run),
`model_linux.rs` is bounded by its own alphabet, and the deep run does not finish in an hour on one host in a single process
(split it with `MODEL_CONFIG=<Caps>/<Resp>`, one configuration per run). The integrator adds the deep run to the nightly workflow
(`.gitea/workflows/nightly.yml`: one step per configuration, `MODEL_DEEP=1 MODEL_CONFIG=... cargo test --release -p paddock-service
--test it the_crash_model`, each bounded). Until that runs, "no state in the default bounds violates an invariant" is the claim,
not "no reachable state does". It states: the set of states is finite because attempts are at most 3 and the clock is virtual,
with timings shrunk (as `link/model.rs` does with `Tuning`).

Alphabet: Start, Install (asked and unattended), Uninstall, Resume, Repair, StopRequested, update, downgrade, a second copy
(newer and older), login and reboot, Login Items toggled, damaged and newer record, record write failure, lock contention,
cancel; OS results `NeedsApproval`, `Unavailable`, `NotAsWritten`, register `Other`, `Job::Unknown`, `Failed`, an
unreadable mark, a removal timeout, a slow OS call.

Invariants: the user's switch is never contradicted (including `wanted`); no unasked prompt; `automatic: false` changes
nothing unattended; no removal of a service that answers unless the version changed or the user pressed Repair; at most
`MAX_ATTEMPTS` unattended registers per version and exactly one for a service that answers; every hold has a way out
(Repair, a newer version, or the service answering); no lost service (one more Start leaves it registered when wanted and
the OS cooperates); a held job is not supervised; `resume_pending` is never lost without a resume; the link never falls
back while a registration is pending.

**It fails on `c612dbfa6`** for the right reasons, not `max_cycles`: unattended registers of one version without bound
(measured: 20 to 40 over 8 starts), and (once the store port exists to run it fast) the crash cases P1-1, P2-2, P2-3, P2-4
and the lock that outlives its holder. Each reproduced finding is listed in the report.

### 8. Amendments of revision 4 (these override anything above that disagrees)

**N1. A failed register is its own phase.** `Phase` gains `RegisterFailed(kind)` (`Other | NotAsWritten`), written when `register`
fails. `Registering` + absent (a crash) stays a free finish with no new attempt; `RegisterFailed` + absent retries only under
the cap, and at the cap the hold is `Hold::RegisterFailed(n)`. A register that failed with anything but `NotAsWritten` (the one failure whose write may have landed) is never settled by the old
job that still answers: the cycle failed. `NeedsApproval`, `Unavailable`, `Declined` and `NeedsElevation` are
user or OS states, not attempts: the record returns to the state before the replace and the registration state says the rest.

**N2. `pending` comes from the transition.** `Manager::pending()` is true only when the plan for a start is `Await`, or a
replace is under way (phase `RemovingOld`, `Registering`, `Awaiting` or `RegisterFailed` under the cap). It is false for
`SwitchedOff`, `NeedsApproval`, a refused location, the cap, a lost lock and `wanted = Some(false)`, so the link starts an agent
of its own then. It covers `RemovingOld` (the job is absent for up to 20 s and `supervises` is false), so the link does not start
an agent on an ordinary update. The invariant is stated that way: the link never falls back while `pending()` is true, and
`pending()` is false in every state with no way forward.

**N3. The Settings switch returns at once.** `Install` asked runs on a worker; `settings_changed` returns with the busy state
(ER-17). On `Install` asked, the app's own agent is retired first (the existing `link/machine.rs` handover is kept: the link
refreshes the service before the wait, not after), then the wait runs. The wait is cancelled by Quit and by a newer event.

**A request on a healthy service.** A requested Install or Repair never replaces a registration that is current, answers and passes the
identity check (the plan's `Healthy`): it leaves it alone, logs "{app}'s background service is already working." and returns
success. Only an unhealthy, refused, missing or stale registration is replaced, so a request cannot lose a service that works
(the model's `Install+RegOther` on a healthy service).

**A version-change replace that keeps failing.** Where registering over the old registration replaces it in one step (systemd, the
Windows `Run` value: no pinned build identity), the replace registers the new service first and does not remove the old one, so a
register that fails leaves the old service working. Where the OS needs the old registration removed first (macOS, R03), a register
that keeps failing after the removal leaves nothing registered. That is an allowed outcome, not a lost service: the hold says the
background service could not be registered, with the cause, and Repair tries again; the model records it as allowed.

**Attempts start over when the user asks again.** The cap is for unattended registers. A user's switch off and on again is a request for the
service: the removal that follows the switch going off clears the attempts in the record, so the first run after it counts from
zero. The model's count follows the same rule (it resets when the switch goes from off to on, as it already did on Uninstall).

**The register time.** The record (format 3, `registeredAt`) keeps the time of the attempt's register call. At the next start the plan
settles an accepted registration on a running job only when the job started at or after it (`job_process`); a job that started
before is the old build's that outlived the removal and settles nothing. Format 3 is the first release's: a record of an earlier
format (2) is read as it is with no register time, which falls back to today's reading; a damaged or newer file falls back to
defaults as before.

**Rule A: the attempt count only rises between proofs.** Every register call adds one to the version's attempts, the user's request
included. Only a proof that the service answers, a version change, or the switch going off resets the count. The next register of
an interrupted replace is paid once (`Phase::RemovingOld { ours, paid }`: paid by the request that began it, or by the attempt counted
when `Registering` was written), so a paid finish may run past the cap by one register and is never counted twice. A removal with
nothing registered afterwards (an approval, unavailable, declined) records "removal unfinished" with the count before that register.

**Rule B: only a proof that names both the time and the copy says "this version registered it".** The job answers or runs, it
started at or after `registeredAt`, and `points_here` is not `Some(false)`. One check (`Manager::proves_ours`) guards the plan's
settle, a start's mark and a first run's remember. A job that outlived its registration names nobody.

**Registering over a running old service.** On systemd and the Windows `Run` value, after the new registration succeeds the old service is
replaced by the new one: the unit is restarted when its text changed, and the old agent is asked to stop before the new one is
spawned. A register that fails first leaves the old service working. The "register keeps failing after the old registration was
removed" outcome (a hold with the cause, and Repair) is allowed on every platform; the model records it as allowed.

**R1.** `Looked(answers | refused | settled)` is an event for a look during a wait, and a failed `unregister` or `evict` is an
event too. **R2.** Settling by `Running` needs the same pid and start time on two looks of a process that started after the
register; `Registering` is entered only on a confirmed `Unregistered` or `GoneTimedOut` (recorded which). **R3.** `wanted` is
`Option<bool>`: `None` (a damaged settings file) never registers unattended and lets `Removing` finish; it is read again under
the lock. **R4.** `StopFailed` clears `resume_pending`. **R5.** The app's own login item gets an observation; where the OS
cannot be read, a repeat of `set` is safe (VM row: it does not announce the item again). **R6.** The fixed point saturates
`attempts` past the cap and keeps `resume_pending`, `now` and the versions out of the state key; the lock is a port too
(`InstanceLock::claim` waits about 400 ms when held; one test uses it for real, one kills a real child). **R7.** The cap is for
unattended registers (Repair and Install asked run past it); the invariants have named exceptions for `Uninstall`, reconcile
`Remove` and Install asked.

**Unattended install (the first run).** `Install { unattended: true }` never removes anything: with something registered (any
version's, up or not) it changes nothing and leaves the registration to the start's plan; with nothing registered (no
registration and no job) it registers once, with no removal step first (the phase written is `Registering`), within the
attempts left (past the cap it fails with the reason), only with a record that can be written, and only where no OS prompt is
needed (`automatic: false` refuses with a message). `Install { unattended: false }` (the user pressed it) is the Repair path
and may replace. The model checks that a first run never removes before it registers.

**Linux.** `resume_pending` lives in the record on Linux too (the marker file is gone, or peek-then-remove); the switch check at
first sight goes through reconcile; `pending()` defaults to false where nothing waits; shared Dyno code builds and tests on Linux.

**Cuts that keep the guarantee.** WP1 builds no store port on the old code: the named crash traces are replayed on `c612dbfa6`
with its real file (ER-11 holds). The identity of a copy (`points_here`, `program_exists`) comes from `registered_program`
against this copy's path plus a file check, written with `RemovingOld`; `program =` from `launchctl print` is parsed for the log
only. Tests that the explorer covers are deleted, not rewritten, if the explorer counts towards the 99% gate (rule 43).

**Mutation results (cargo-mutants 2026-11-06, in place, macOS, over the diff against `c612dbfa6`).** Shards are 0-indexed and each
shard's outcomes equal its mutants (ER-33). paddock-service: targeted rerun of the touched functions (2 shards) 107 and 106 mutants,
101+87 caught, 2+3 missed, 4+15 unviable, 0+1 timeout; shard 0/8 54 mutants (39 caught, 5 missed, 10 unviable); shard 1/8 54
(47, 1, 5, 1 timeout). dyno-app shard 0/6: 18 (10 caught, 5 missed, 2 unviable, 1 timeout); targeted login run 11 of 11 caught.
Every survivor of these runs in `Manager::pending`, `await_start`, `uninstall_locked`, `WindowsRun::register` and the login
defaults got a test; a final run over those functions had 36 paddock-service mutants (31 caught, 4 unviable, 1 timeout, 0 missed).
Survivors that remain, with the reason:

- Equivalent: `record.rs` `left > 0` to `>=` in `RecordFile::store` (the `left == 0` branch comes first); `resume.rs`
  `tried += 1` to `*= 1` in `install_patient` (the count only feeds a log line); `manager.rs` deleting the
  `Hold::Unanswered | Hold::Refused` arm in `start_with` (it only chooses the warning text).
- Real OS calls that tests must not make (ER-14): the `SmService` wrappers in `login.rs` (about lines 683-751),
  `platform_service` and `service_pending`.
- Test gap, to close after the 1.1.0 tag: `quit.rs` (about lines 315 and 332) and `shell.rs` (about line 455), which need a
  live Tauri `AppHandle` or `ShellOf`; `login.rs` `run_first_run` (same reason); `first_run_done` (reads the process-wide
  `FIRST_RUN_MARKER_PATH`; its test waits for the review-fix change that passes the path in).
- Not measured on macOS: `login.rs` `LinuxService::real` is `cfg(target_os = "linux")` and `login.rs` about line 469 is
  `cfg(windows)`; the Linux run covers the first, the Windows VM round checks the second by hand.
- `repair.rs` `Stepper::next_cycle` `+=` to `*=` is a timeout (the loop never ends), which counts as caught.

### 9. Work packages (honest estimate, revision 4)

| WP | What | Days |
| --- | --- | --- |
| 0 | Freeze of the types, `Facts`/`Observation`, hold and phase names (this change, first) | 0.25 |
| 1 | Store and lock ports, simulator in `testing`, fixed-point explorer, alphabet; named traces red on `c612dbfa6` | 2 |
| 2 | `State`, format 2, migration under the lock, read-only queries, `InstanceLock` | 1.25 |
| 3 | `transition`, stepwise executor (`repair.rs`), `OsResult` events, caps, attempts per register | 2.25 |
| 4 | Manager entry points, reconcile (`wanted`), `resume_pending`, cancel, lock release during waits | 1.5 |
| 5 | Signals, `pending`, link fallback, its model, `supervises` | 1.5 |
| 6 | Copy identity from `registered_program`, `with_program` | 0.5 |
| 7 | Dyno macOS: `login.rs`, `quit.rs`, `lib.rs`, `shell.rs` reconcile, the Settings switch on a worker with a busy state | 2 |
| 8 | Delete what the explorer covers, rewrite the rest, `control.rs`, shell-demo, paddock-shell tests, README, redraw `uml-state-service` (ER-23) | 1.5 |
| 9 | Dyno on Linux: `CliService`, `dyno_agent::service`, the verbs onto `Manager` + `Systemd`, tests, docs, Linux scenarios | 1.5 |
| 10 | Rules, lessons, docs | 0.25 |

About 14.5 days of work, 8 to 9 days of calendar with the parallel pieces below, plus a VM round of 2.5 days. The Dyno 1.1.0
tag waits for WP 0 to 9.

**Parallel after the freeze (WP 0).** WP 1 (`crates/paddock-service/src/testing/`, `tests/it/model.rs`); WP 6 (`backend/launchd.rs`,
`launch.rs`); WP 9 (`backend/systemd.rs`, `apps/dyno/app/src-tauri/src/login.rs` Linux part, `apps/dyno/agent` service and
`cli.rs`), once WP 3 has landed; WP 5's link fallback (`apps/dyno/app/src-tauri/src/link/`) once N2's `pending()` is in; WP 8's
`control.rs`, shell-demo, README and the diagram. The serial core is WP 2, 3, 4, 7 and the rest of 8.

**VM rows** (left for the VM round): `SMAppService` lag and real start time; the number of Background Items Added notices per
path; EX_CONFIG after a real update; a register with the old job lingering; a kill in each phase; a disk image and a
translocated launch, then /Applications; two copies and `program =`; Login Items off and on, `RequiresApproval`; stop,
resume, login; a slow service against the link's fallback; an update from a real 1.0.0; logout and reboot in `Awaiting`
and with a hold; systemd `disable`, `stop`, a crash and a restart loop, logout with and without lingering (Linux); Windows UAC declined, a crash in the elevated step, no
prompt on an unattended start.

**Pitwall and Pitcrew.** Adoption: construct a `Manager` with the backend, a service-only answer, `with_program` and the
data folder; call the entry points; show `tracker()` and the hold messages; run the wiring contract against the simulator.
They keep nothing of their own decisions. The elevated child and per-machine records (Windows) need their own state file
under the same type. The copies are deleted by their registered tags.

**Rule 55.** The mechanisms `launchd-job-report` and `service-registration` change in their canonical files and the Dyno
wiring in one change; the commits touching the members carry `Copies-waived:` trailers (never `approved:`) for Pitwall and
Pitcrew. Linux Dyno leaves the copies: nothing is registered for it.

## Consequences

- One place decides; entry points differ only in the `Event` they pass and the words they show.
- The record format changes from 1 to 2 with a one-way migration under the lock.
- The model test is the guard: a new entry point, signal or OS result must be added to its alphabet.

## Alternatives rejected

- Patching each finding (three rounds did).
- A background supervisor that owns the service (on macOS launchd already is one).
- A pid or time stamp in the lock file: a reusable OS name (rule 53).
- A stored `Held` state: it is derived from `attempts`, so it cannot disagree with them.

## Amendment: cancel ownership (ER-21)

A process-wide cancel flag (`CANCEL`, `QUITTING`) let one operation's or one test's cancel reach another: a cancelled
Settings change left the flag set, and the next operation that read it (the start repair, the Linux wiring tests in the
same process) was stopped by a cancel nobody had asked it for.

- **Each operation owns its cancel.** The switch worker, the start repair and the first run each take an `OpCancel`
  (`app/src-tauri/src/service_ops.rs`, an `Arc<AtomicBool>`) and pass it into the service they use (`LinuxService`,
  `SmService`; `dyno_agent::service::linux::{install, install_without_waiting, ensure}` take it as `&AtomicBool`). The
  shared manager (`paddock_service`) already takes `&AtomicBool` and checks it between OS steps, so no change there.
  `paddock_jobs::CancelToken` was not used: it does not hand out the `AtomicBool` the manager takes.
- **The Jobs view and Settings cancel one operation:** `SwitchWorker::cancel` and the job's cancel stop the token of the
  running change only.
- **Quit is the one app-wide signal.** `ServiceOps` is app state: `quit()` cancels every running operation's token and
  `begin()` then refuses new operations. It also holds the end-of-start-repair wait Quit joins.
- **A check keeps it so:** a test scans `login.rs`, `quit.rs` and `dyno-agent`'s service module and fails on any `static`
  item or `thread_local!` there (a flag in a wrapper type or a `OnceLock` included; the earlier scan matched `static` and
  `AtomicBool` on one line only and missed `FIRST_RUN_ATTEMPT`). Two items are named as the process's by nature
  (`STOP_BEGUN`, Quit's once-per-process claim; `FIRST_RUN_MARKER_PATH`, the one folder of the first-run marker), and a
  positive control shows the scan sees each shape. The first-run attempt now lives in `ServiceOps`.

## Amendment: final review of Dyno 1.1.0 (revision 5)

**Lock after a wait (amends section 3 and R1).** The replace let go of the lock at the first `WaitingForStart`, but a later look
that saw `Job::Refused` started the next cycle (`Unregister`, `Evict`, `Register`) with no lock, so an uninstall that ran during
the wait could be overwritten by a second cycle. Now `replace_bounded` takes a second hook, `after_wait`, and calls it before the
`say` and the OS call of any cycle that follows a wait. The manager takes the lock again there and continues only when the record
still shows this version's `Awaiting`; otherwise the replace ends as `Cancelled` before it changes anything, and the manager
settles nothing (the record is somebody else's). A replace that ends inside the wait settles as before (lock taken, record
checked). A test uninstalls during the wait and then lets the OS refuse the job.

**One look is not R2 (amends R2).** R2 was enforced only in the repair's `Stepper`. The three other places that settle on
`Job::Running` (`plan` for `MarkCurrent`, `await_start`, `proves_ours`) accepted one look, and `remember()` cleared the attempts.
`Manager` now keeps the last `ProcessId` it saw for a running job (`running_twice`): a running job settles the service only when
two looks name the same process (pid and start time, rule 53), or when the service itself answers. A backend that cannot name the
process is left as before. The read under the lock right after a decision is the same look, not a second one, and `await_start`
starts its own count so its first look is not confirmed by the decision's. `Facts` gains `job_confirmed`. A test sees a job once
and then not at all: the attempts and the remembered version stay.

## Amendment: the wait for the service is judged, not clocked (Pitwall 1.1 B1; amends section 5 and the alphabet and counts of section 7)

**Pattern.** Pitwall's 1.1 branch (before it took the rc's crate) gave the wait for a registered service a judge: launchd needed
about a minute and on one Mac four, and a fixed 120 s ended the repair as `Registered` ("did not answer, may still start") while
the job was alive, or waited the full time for a job that had vanished or was crash-looping. ADR 0031 says a running job is judged
on what is observed (patience), not on a wall clock. The rc's `Stepper` knew only the fixed `Timings::start_wait`. Pitwall's
repair waited a fixed 120 s until this landed; porting the judge as a side hook in the executor would be a second place that
decides how a repair ends (ER-21), so it enters the declared machine.

**Decision.** The judge is an input of the state machine, not a decider beside it.

- `StartWatch` (`start_wait.rs`) is a port: `poll(&StartObservation) -> StartVerdict`, `reset()` per registration. `StartWait` is its
  one implementation (over `paddock_proc::StartJudge`: spinning, no CPU and no progress, paused by the system, not observable for
  long; the job vanished, or restarts in a loop). `Backend::observe_start() -> Option<StartFacts>` is the backend's side; launchd
  implements it (the job's process by identity, pid and start time), systemd and Windows answer `None` and keep the fixed wait.
  `Manager::with_start_watch` installs a watch.
- **New input of the transition function:** `OsResult::Looked` gains `verdict: Option<StartVerdict>`. `None` means "not judged"
  (no watch, a backend that cannot observe, or a caller that must return at once, `Timings::without_start_wait`): the wait is bounded
  by `Timings::start_wait` exactly as before. The executor (`replace_bounded`) produces the verdict (one `observe_start` and one
  `poll` per look, also at the look where the service answers: until the answering process is confirmed by a second look the wait
  must stay a judged one) and calls `reset` at every `WaitingForStart`; it decides nothing. When the watch can see the job, those
  facts are the look: `inspect` is not asked again, so the OS is read once per look.
- **What the `Stepper` does with a verdict** (in `Stage::Waiting`, after the service-answers check, which stays first and keeps
  rule 53's process identity; and a cancel is checked **before** a verdict that ends the wait for the judge, so a user who stopped
  waiting is never told "did not start"):
  - `Waiting` or `StillWorking(note)`: keep waiting. **A judged wait is not ended by `start_wait`**; its bounds are the verdicts
    below, a cancel, and the backstop.
  - `Refused(seen)`: another cycle, exactly like `Job::Refused` seen at once (`next_cycle`, bounded by `allowed`, so the version's
    attempts still cap the registers).
  - `Failed(why)`: the repair ends `Failed("The service was registered but did not start: <why>. <next steps>")`: a job that is
    stuck, vanished, crash-looping, or **never ran**. The registration stays, the record keeps `Awaiting`, so a hold is derived as before.
  - `GaveUp(why)`: the judge stops waiting for a job that **ran and is not stuck** (only slow). The repair ends `Registered`
    ("it may still start") with **no advice to repair again**: a running job is never abandoned as broken (ER-19).
  - `Up`: a watch is told `answers: false` only (whether the service answers, and by which process, is established by the look
    itself), so `Up` cannot come from `StartWait`; a watch that says it anyway is one more poll, never a settled service. This keeps
    section 1's "the service answers" meaning one signal.
- **Two ceilings, both counted in looks, never in seconds** (a paused or starved machine does not spend them): `StartWait`'s own
  (`START_CEILING_POLLS`, 200 looks at 3 s, about ten minutes): a job that ran ends `GaveUp`, one that never ran ends `Failed`; and
  the state machine's **backstop** `Timings::judged_ceiling_polls` (a few looks later), which ends any judged wait whatever the
  watch says: `Registered` if the job was seen running (apart from an old build's job that answers), else `Failed("never started
  after N checks")`. So a watch that forgets to end itself cannot hang a repair.
- **Invariants that must still hold in every state** (section 7): no new **mutating** OS call is added (a judged wait adds the
  read-only `observe_start`, and removes the `inspect` it replaces); a judged wait changes nothing and the lock is let go as for any
  wait; the unattended register cap per version is unchanged (a `Refused` verdict spends a cycle, which is an attempt); `Recovered`
  still needs the service's answer; a cancel ends a judged wait with `Registered`; every judged wait ends within
  `judged_ceiling_polls` looks of its registration; and the hold and `Idle` writes are those of the verdict's outcome (`Failed` leaves
  a derived hold, `Recovered` writes `Idle`, `Registered` leaves `Awaiting` like any wait that ran out).
- **Notes** (ER-18): a wait that is alive and silent says "still working" after 20 looks and **again every 20**, each with the number
  of checks and the seconds so far.

**The check (amends section 7).** The alphabet grows by the OS behaviours that only a judge tells apart, as three configurations of
a Mac with a watched manager, in the **default** run (the waits shrunk, `watched_timings` in `model.rs`: a fixed wait of 12 s, a
backstop of 20 looks, the watch's ceiling 5 looks before it; so no `MODEL_DEEP` run is needed for them):
`Looping` (the job exits with code 3 and the OS restarts it: the wait fails early with the observation), `WatchedLate` (the job comes
up 30 s after the register, past the fixed wait: the judge waits for it) and `WatchedNever` (the job only ever shows "not running":
the ceiling of looks ends the wait, as a `Failed` because nothing ran). With them there are **11 configurations**; the default run
explores **8** (the original five and these three), each in a test of its own with a floor, and the long run (`MODEL_DEEP=1`) adds
`Slow`, `Late` and `Linux/Fast`. The state key does not hold the number of looks (it is per wait, reset at each register call and
bounded by the backstop), so the set of states stays finite for the same reason as before: attempts are at most 3, the clock is
virtual, and a wait is at most `judged_ceiling_polls` looks. The simulator enforces that bound: it panics when a wait has looked more
than twice the backstop since the last register, so a ceiling that stops ending waits fails the run with its reason instead of
hanging it. A positive control (`a_judged_wait_ends_as_the_judge_says_in_each_watched_configuration`) shows each outcome, and a
lib test shows that looks that only ever show a pending job fail at the 201st poll.

**What was refuted.** Treating `Failed` as a register error (it is not: the OS accepted the register); making `Refused` a new
`Hold` (a refused job already is `Hold::Refused` through `Job::Refused` and the attempts, one rule for both ways of seeing it);
keeping the verdict in the executor, which would let two places decide how the wait ends; and ending a running job at the ceiling
with `Failed` and "Repair again" (it contradicted ER-19 and itself: the job was alive).
