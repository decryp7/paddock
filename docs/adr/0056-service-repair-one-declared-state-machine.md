# 56. Service repair as one declared state machine

- Status: Proposed (2026-11-05; user decision to redesign before the Dyno 1.1.0 tag; independent review pending)
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
Record { registered: Option<Registered { version, program }>,   // the last version that saw the service come up
         state: State }

State = Idle
      | Attempting { version, attempts, phase }   // phase = Replacing | Awaiting
      | Held       { version, attempts, why }     // why = Unanswered | Refused | Failed
      | Removing                                   // an uninstall began
```

- `Replacing`: written before the first OS step of a replace (before unregister, or before register when only a
  registration is missing). `Awaiting`: the OS accepted the registration; the service has not answered yet. A crash or
  quit at any point leaves one of these, which says what is safe next.
- `attempts` exists only inside `Attempting` and `Held`, and always belongs to a `version`: a state with attempts but no
  version, or an attempt of another version counting towards this one, cannot be written. `attempts` counts replaces
  begun and not confirmed, per version, and is bounded by `MAX_ATTEMPTS` (3).
- `Recovered` (the service answers, or the OS runs the job) writes `Idle` with `registered = this version`, which clears
  every attempt. Uninstall writes `Removing` before the first OS step, then `Idle` with `registered = None`.
- Migration, once and nothing else: format 1 `{registeredVersion, registeredProgram}` becomes `registered` with `Idle`;
  format 1 with `attemptedVersion` and `attempts` becomes `Attempting { Awaiting }` (or `Held { Unanswered }` at or over the
  cap). A Dyno 1.0.0 install has no record (it kept `service-registered-version`, which 1.1.0 deletes and does not read):
  `Idle`, `registered = None`, so the first start replaces once. A newer format is left alone (the existing
  `VersionedFile` rule). No compatibility shim beyond this.

### 2. One pure transition function

```text
transition(record: &Record, event: Event, obs: &Observation, caps: Capabilities) -> Decision { action, record_before }
```

Every entry point goes through it: `Event` is `Start`, `FirstRun { quiet }`, `Install { quiet }` (the Settings switch on),
`Uninstall` (switch off), `Resume` (after "Quit and stop measuring"), `Repair` (the user pressed it), `Settled` and
`WaitElapsed` (continuations of a running replace). Nothing else decides: `Manager::on_start`, `install`, `repair`,
`uninstall`, `resume` and Dyno's `login.rs` and `quit.rs` call it and execute the `Action` (`Leave(hold)`, `Mark`,
`Offer`, `Await`, `Replace(reason)`, `Finish(reason)` (register only, nothing to remove), `Light`, `Remove`, `Refuse`).
`record_before` is written, atomically, before the action's first OS call.

`Observation` separates the signals instead of one `answers`:

- `service_answers`: the service's own health answer (never the app's agent);
- `job`: running, not running, refused, absent or unknown, from the OS;
- `registration`, `login_item`, `program_exists`, `points_here`, `definition_current`: as `Inspection` today;
- `covered`: the app's own agent holds the data folder. It only keeps a refused job from being replaced while the app
  measures; it never settles an attempt.

Time and OS facts are inputs; the function reads no clock and does no I/O. Dyno's own decisions (the launch location, the
user's settings, the stop marker) are inputs too: a refused location is `Observation.location_ok = false`, which leaves
the resume marker where it is.

### 3. Exclusion

One OS file lock (`File::try_lock`: `flock` / `LockFileEx`, as `paddock_proc::instance` does) on
`service-<label>.lock`, released by the kernel when the holder dies (rule 53: no pid or time stamp is trusted, nothing is
taken over by age, the file is never deleted by path). The record is read again after the lock is taken and the
transition is run on that, so an `Await` never replaces on facts another copy has just changed. A lost lock is `Busy`;
the patient variants of start, install and uninstall wait for it with a bound and a log line per look.

### 4. Windows and Linux

`caps` decides what the function may return: `automatic: false` returns `Offer`, never `Replace`; `pinned_identity: false`
(Linux, Windows) never turns a recorded attempt into an `Await` or a replace of a current definition; nothing unattended
(`Start`, `FirstRun { quiet }`, `Resume`) opens a prompt, Login Items or UAC.

### 5. Waits, progress, logs

Waits are measured by an injected elapsed-time source, not by the sum of the polls (a slow OS call counted by the poll
would stretch 120 s). A wait honours cancel, states its phase to the tracker (`Removing`, `WaitingForRemoval`,
`Registering`, `WaitingForStart`, and for the link's first look `Waiting for the background service`), and a replace has a
busy state the UI shows (Dyno's Settings switch is busy while it runs: ER-18). Every transition logs old state, event,
signals, new state and action in one line (ER-27); a hold logs why and the next step in words that are true for the app
(Dyno has no Repair button for the agent service: it says to switch Launch at login off and on).

### 6. The check

A model test (`crates/paddock-service/tests/model.rs`, public API only) drives the real `Manager` over a simulated OS
(a `Backend` with a virtual clock, a registration, a job, the user's switch, a start delay, a prompt counter):

- scenarios: service comes up fast, slowly (inside the wait), after the wait, never; job refused until removed, refused
  always; `automatic: false`; no pinned identity; the user switched it off;
- entry points: Start, FirstRun (quiet), Install, Uninstall, Resume, Repair; every sequence up to depth 3;
- crash or quit at **every OS call** (an injected unwind at the n-th backend call, then a fresh `Manager` over the same
  record file, so the lock and the record survive as they would), for each position in the sequence;
- invariants after every step: the user's switch is never contradicted (no register while switched off; no register after
  an uninstall); never an unasked prompt (unattended events open nothing, `automatic: false` registers nothing); never a
  removal of a service that answers unless the version changed or the user pressed Repair or Install; at most
  `MAX_ATTEMPTS` registers per version per scenario that never answers (and exactly one for one that answers);
  never a hold with no way out (from every `Held`, Repair or a newer version registers); never a lost service (after any
  crash, one more Start leaves the service registered whenever the user wanted it and the OS cooperates); a held job is
  not "supervised";
- a second model for Dyno's resume marker (`quit.rs`): location refused or accepted x service state x crash before and
  after each step: the marker is never lost without the service having been resumed.

Fast and deterministic: the OS is a struct, the clock virtual, the record in a per-run temp folder; no randomness (the
exploration is exhaustive and ordered), the first failing sequence is printed as a replayable list. The size is measured
and printed at the end of the run (sequences, crash points, runs); the budget is 30 s in debug.

**It fails on `c612dbfa6`.** The model, run there, reproduces: P1-1 (crash during the wait registers twice), P1-2 (a
refused job registers at every start), P2-2 (crash between unregister and register loses the service), P2-3 (resume at the
cap registers a fourth time), P2-4 (`Await` with `automatic: false` and on Linux), P2-5's lock part (a dead copy's lock
file stays "held" for 10 minutes), P1-3 (the marker is lost on the disk-image path), the uninstall crash (a switched-off
service comes back), and the held-job-is-supervised case. P2-1, 6, 8 and 9 are not state-machine properties and are closed by
named tests (below).

### 7. Work packages

| WP | What | Closes | Days |
| --- | --- | --- | --- |
| 1 | The model test and the simulated OS, red on `c612dbfa6` | proof of the pattern | 1.5 |
| 2 | `State`, format 2, migration, the OS file lock | P2-2, P2-10, P2-11, record-write failure | 1 |
| 3 | `transition`, the Manager entry points through it, `caps` | P1-1, P1-2, P2-3, P2-4, uninstall crash | 1.5 |
| 4 | Signals split, `supervises` of a held job, hold messages in `status`, logs | P2-1, P2-9, P3 logs | 0.5 |
| 5 | Dyno: `login.rs`, `quit.rs` (peek, then remove), `lib.rs` (link wait without start wait and a phase), `uninstall_patient`, the Settings toggle busy state | P1-3, P2-6, P2-7, P2-8 | 1 |
| 6 | Mutation tests (`await_start`, `quiet`, a settled `Await`) | surviving mutants | 0.5 |
| 7 | Rules and lessons, the register | rule 55 | 0.25 |

About 6.25 days of work; the Dyno 1.1.0 tag waits for WP 1 to 6.

**Pitwall and Pitcrew.** Their own `service_repair.rs` and `service_install.rs` are copies of this mechanism (rule 55;
`launchd-job-report` and `service-registration`). Adoption means: construct a `Manager` with the backend, a service-only
answer function and the data folder; call `on_start` / `install` / `uninstall` / `resume`; show `tracker()` and the hold
messages; run the same model test with their spec. They keep nothing of their own decisions. The copies are deleted by
their registered tags (Pitcrew 1.1.0, Pitwall 1.2.0).

**Rule 55.** The mechanisms `launchd-job-report` and `service-registration` change in their canonical files and in the
Dyno wiring in one change. The copies of Pitwall and Pitcrew have the defects this ADR lists; they are not changed here
(they are deleted on adoption) and the commit touching the members carries `Copies-waived:` trailers naming each
(`launchd-job-report` Pitwall and Pitcrew, `service-registration` Pitwall and Pitcrew) with that reason.

## Consequences

- One place decides; entry points differ only in the `Event` they pass and the words they show.
- The record format changes from 1 to 2 with a one-way migration; a 1.1.0 build reading a format-2 file written by a
  later build leaves it alone (the existing `VersionedFile` rule).
- Hold messages and the next step become part of the contract and are tested.
- The model test is the guard: a new entry point or a new signal must be added to it.

## Alternatives rejected

- Patching each finding (three rounds did; the next review would find the next entry point).
- A background supervisor that owns the service: a larger design, and on macOS launchd already is one.
- Keeping the time-stamp lock and adding a pid: a pid is a reusable OS name (rule 53); the OS lock needs neither.
