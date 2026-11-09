# ADR 0076 amendment: the orphan guardian (2026-11-09)

- Status: **proposed**. The user chose the shape (one guardian per process, in Rust, generic); this text goes to the user
  for acceptance.
- Amends: ADR 0076 (owned background work), §6 (detached kinds) and §7 (parent-death delivery).
- Implementation: `fix/chromium-orphan-guard` (`crates/paddock-proc/src/guardian.rs`, conformance check C79).

## Context

On CI slot s3 a test process (pid 11245) was killed, and a headless Chrome for Testing it had started kept running for
2.5 h (9 processes, about 1 GB) until the coordinator stopped it. On Unix a `paddock_proc::Supervised` child leads its own
process group, and everything that ends it lives in the owner: `Drop`, `paddock_proc::end_all`, the browser's
`max_lifetime` watchdog thread. A SIGKILL, a crash or a test runner's kill takes them all away. Windows is not affected:
each child runs in a kill-on-close job object, and the job handle closes when the owner dies.

A first fix (two `/bin/sh` helpers per child) was reviewed CHANGES: 2N processes, shell scripts outside Rust, a `ps`
dependency and a kill C53 could not see. The user asked for one guardian per process, written in Rust and generic.

## Decision

### 1. The mechanism (`paddock_proc::guardian`, Unix)

- **One guardian per owner process**, started on the first guarded child: the owner's own executable run again with
  `PADDOCK_PROC_GUARDIAN=1`, in a process group of its own. It reads a pipe whose write end only the owner holds
  (close-on-exec, so no other child inherits it): `add <group> <keeper pid> <keeper start> <hex length> <folder as hex>`
  per guarded child, and `del <group>` when the owner ended that child itself. End of file means the owner is gone. The
  guardian then ends every group still registered, waits (bounded, 10 s) for it to empty, removes its folder, and exits.
  A guardian that died is started again on the next child.
- **Whole lines only.** The owner builds each line in one buffer and sends it with one `write_all`; the guardian reads
  with `read_until(b'\n')` (at most 16 KiB a line) and drops a last line without its newline and an `add` whose folder hex
  is not as long as the line declares. A line the owner did not finish (it died while writing) can therefore never name a
  prefix of its folder, such as an ancestor folder.
- **One keeper per guarded child**: `/bin/cat`, started by the owner into the child's process group, reading a pipe whose
  write end only the guardian holds. While the keeper is there (alive, or a zombie not yet reaped) the group id cannot be
  handed to another group. The guardian signals a group only after checking that its keeper, by pid and start time, is
  still a member (`tree::end_pinned_group`, through `sys.rs`, so C53 and rule 53 cover the kill). A keeper ends with the
  guardian, or when its owner ends the child (the owner ends and reaps it through its handle). It stays a minimal program:
  it only blocks on a pipe, and the owner's executable run again would map tens of MB for nothing (measured: the guardian
  of a 5 MB debug test binary 6.9 MB resident, a `cat` keeper 1.3 MB).
- **Generic**: any `Supervised` child gets the group end; `Supervised::spawn_owning_folder` also names a folder to remove.
  Chromium is one user (throwaway profiles); nothing in the guardian knows about browsers.
- **Trace (ER-27)**: every action of the guardian is one line in its log file: the owner's identity, a refusal to serve
  and why, each line dropped, each group ended or left alone (its keeper gone), and each folder removed or left and why.
  The owner logs the guardian's start and, once, when the guard is off and why.
- **Cost** (docs/benchmarks.md, 2026-11-09, measured at load 10 to 12 like the first shape): N guarded children cost N + 1
  processes; a registration 0.20 to 0.26 ms p50 (0.29 to 0.35 ms with a folder, which writes its marker), an
  unregistration 0.05 to 0.08 ms, the first one (which starts the guardian) under 1 ms. The first shape (two `sh` per
  child) took 5.8 to 7.0 ms per child at the same load.

### 1a. Who the guardian obeys (the security review's P1-1)

The guardian can SIGKILL process groups and remove folders, so anyone able to start the binary with
`PADDOCK_PROC_GUARDIAN=1` and a standard input of their own (a shell, `open --env --stdin` on macOS) could make an app
with TCC grants, or a root binary, do that for them. It therefore obeys only its owner, and acts only within what the
owner marked:

- **The owner, by identity.** The owner passes its identity (`PADDOCK_PROC_GUARDIAN_OWNER`, pid and start time). Before it
  reads a line, the guardian checks that its parent (`getppid`) is that pid, that the pid still names that process (start
  time, rule 53), and that the parent's executable (`proc_pidpath` on macOS, `/proc/<pid>/exe` on Linux, canonical) is its
  own (`current_exe`, canonical). Its standard input must be a pipe; on Linux the owner must hold that pipe (its inode among
  the owner's descriptors); macOS gives no portable way to name a pipe's other end, so there the parent check stands
  alone. Otherwise it logs the refusal and exits without reading.
- **Groups**: only a group whose keeper is, by identity, still in it, and whose executable is the keeper program
  (`/bin/cat`, canonical).
- **Folders**: only an absolute, canonical path (no link on the way), a real folder (not a link), holding the owner's
  marker `.paddock-proc-guardian`: a regular file of this user, written by the owner at registration (`O_NOFOLLOW`, mode
  0600), naming the owner's identity. The marker is the capability: a forged line can only name a folder the forger could
  already write a marker into, which it could already remove. A "folder under the owner's temp root" rule was considered
  and not used: the folders guarded today are browser profiles in the app's cache folder, not in a temporary root, and the
  marker covers any location.
- **The log** is opened with `O_NOFOLLOW` and mode 0600, only in a folder this user owns that nobody else can write to
  (never `/tmp` or another shared folder). By default it is `~/Library/Logs/paddock-proc-guardian.log` on macOS and
  `$XDG_STATE_HOME` (or `~/.local/state`) `/paddock-proc-guardian.log` elsewhere; `guardian::set_log` names another.
  `PADDOCK_PROC_GUARDIAN_LOG` is ignored for root.
- **Root: off.** A process running as root does not start a guardian, and a guardian running as root exits at once. No
  root main may adopt the guardian under this amendment: Dyno's helper (`dyno-helper`, on C79's `ADOPTING`) stays as it is,
  and a later amendment must review the root case first.
- **ADR 0073 class E (guards).** `PADDOCK_PROC_GUARDIAN` (turns the binary into the guardian; obeyed only under the checks
  above), `PADDOCK_PROC_GUARDIAN_OWNER` (the owner's identity, checked against the parent) and `PADDOCK_PROC_GUARDIAN_LOG`
  (where the guardian logs; checked as above, ignored for root) are deliberate switches of every shipped binary that adopts
  the guardian. They are listed as class-E guards for ADR 0073's register, accepted together with this amendment.
- **Windows** keeps `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; no guardian is started there.

### 2. How the guardian process starts: a re-exec of the current executable (option a)

| | (a) re-exec with a reserved variable | (b) a `paddock-guard` helper binary |
| --- | --- | --- |
| Per binary | one line, `paddock_proc::guardian::run_if_requested();`, first in `main` | none in the code |
| Per app and OS | nothing to ship | a Tauri `externalBin` for each Unix target triple the app ships (Windows needs none), its build step in each app's workflow, installer and SBOM entries, notices, the macOS quarantine and ad hoc signature of a second executable, the updater carrying it |
| Tests | a test binary names an entry test (`enable_in_test_binary`) | every crate's tests need the helper built first and found (`CARGO_BIN_EXE_*` exists only inside `paddock-proc`) |
| Version skew | none: the guardian is the same build as its owner | the helper and the app can differ after a partial update; the line format becomes a versioned contract |
| Risk | a binary without the call must never re-run itself: the owner starts a guardian only after its own `main` called `run_if_requested` (or the test binary named its entry), else it logs that the guard is off | a missing or quarantined helper: the guard is off |

**Chosen: (a).** It ships nothing new, cannot skew, and costs one line per binary. The danger (re-running an app that does
not expect it) is closed by the opt-in: only a process whose `main` called `run_if_requested()` starts a guardian.

**The mains that need the call** are every `src/main.rs`, `src/bin/*.rs`, `src/bin/*/main.rs` and `[[bin]] path` of a
package whose dependency closure in its workspace's `Cargo.lock` reaches `paddock-proc`: 39 on `origin/train/2026-11-08-1`
(Debrief 22, Dyno 6, Spotter 1, the shell and workbench demos 3, shared crates and tools 7; listed in C79's `ADOPTING`).
`#![no_main]` binaries (51 fuzz targets) have no `main` and are skipped. **Check C79** reads the lockfiles and fails a main
that reaches `paddock-proc` without the call as the first statement of `fn main` (comments and strings do not count) and
is not on `ADOPTING`. `ADOPTING`
only shrinks: a listed main is a warning, a listed main that has the call is an error until it leaves the list. Each app
adopts in its own release window (rule 1); the shared fixtures and tools with the next shared batch. Test binaries
(`real_browser.rs` and the unit tests of `paddock-chromium` and `paddock-proc`) opt in through `enable_in_test_binary`
already. C79 is not yet in CI's conformance step (`.gitea/workflows/ci.yml`, owned by the integrating agent).

### 3. §6: a new detached kind

| Kind | For | Cap | Ends |
| --- | --- | --- | --- |
| `OrphanGuardian` | ending the process's supervised children (their groups, then their folders) when the process dies without ending them | 1 per process (one more after it died, started on the next child) | when its owner is gone: at once with nothing registered, else after ending what is registered (each group waited for at most 10 s); logged in its own log file |

The keepers are not detached: each is owned by its child's `Supervised` registration and ended and reaped with it.

### 4. §7: `PR_SET_PDEATHSIG`

§7 named Linux `PR_SET_PDEATHSIG` (and a macOS kqueue `NOTE_EXIT` watch) for `./dev burn`'s burners. For supervised
children it is not used, and it is not needed for the burners either:

- Setting it in the child needs a `pre_exec` hook, which the family bans (`lint/clippy.toml`: it forces fork and exec, whose
  forks carry the parent's inheritable descriptors until they exec; it refused restarts and hung spawns before).
- It fires when the *thread* that started the child ends, not the process (prctl(2): "the parent in this case is considered
  to be the thread that created this process"). A browser started from a short-lived worker thread would be killed while
  its owner runs.
- The guardian's pipe end of file works the same on macOS and Linux, needs no `pre_exec`, and covers the whole group, not
  only the direct child.

`./dev burn` therefore starts its burners as `Supervised` children of a binary that calls `run_if_requested()` (and keeps
each burner's own deadline). This replaces §7's parent-death sentence.

## Consequences

- A killed owner no longer leaves its children's trees running on macOS or Linux, once its binary calls the guardian.
  Until each main adopts, its children are as before; the owner logs that the guard is off.
- Two more kinds of process appear in process lists: one more copy of the app's executable (run with `PADDOCK_PROC_GUARDIAN=1`) per app process
  that has guarded children, and one `cat` per guarded child, in that child's group.
- Limits: a child that leaves its process group (`setsid`) is outside the guard, as it is outside the owner's tree kill. An
  owner killed between a child's start and its registration (well under a millisecond) is not covered; a caller's
  start-up sweep is the backstop for its folder (`paddock-chromium` sweeps throwaway profiles whose owner, by pid and start
  time within the clock's slack, is gone).
