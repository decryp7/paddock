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

**Exploration is to a fixed point, not to a depth.** States are `(record, simulated OS)`; they are finite because attempts
are at most 3 and the clock is virtual with timings shrunk (as `link/model.rs` does with `Tuning`). The explorer takes
every event of the alphabet from every reachable state, with crashes as graph edges (before and after each OS effect, and
during recovery), and prints the state count.

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
the cap, and at the cap the hold is `Hold::RegisterFailed(n)`. `NeedsApproval`, `Unavailable`, `Declined` and `NeedsElevation` are
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
