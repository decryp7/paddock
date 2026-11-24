# heavy

One heavy step (cargo, npm, ng, playwright, llvm-cov, xwin) at a time per machine. Replaces the shell `plock`
(Paddock agents) and engineering-playbook `scripts/heavy.sh` (ER-24). Package `paddock-heavy`, binary `heavy`; its own Cargo
workspace and lockfile (`tools/heavy/`, ADR 0014's third kind).

```sh
heavy run [--high] [--no-gate] [--low] [--gate-load N] [--slots N] [--queue-max S] [--queue-stall S] [--run-max S] [--verbose] [--log-file FILE] <agent> -- <command...>
heavy status
```

`run` queues (a FIFO ticket; `--high` goes first), takes the lock, waits for the load gate (1-minute load below the
threshold, default 30), runs the command with your stdio, releases, and exits with the command's code (127 not found, 126
not runnable, 128+signal for a signal, 130/143 when `heavy` itself was interrupted/terminated; the command's whole process
tree is ended then).

## Exit codes (the wrapper rule)

The command's own code is passed through. heavy's own failures use codes that do not look like a usual command failure (1, 2):
**64** usage, **75** the lock could not be taken, **78** refused (the home guard), **79** no slot within `--queue-max`, or a queue that made no progress for `--queue-stall` (the command never ran), **80** the command was ended at `--run-max`, **126/127** the command could not be started,
**129/130/143** stopped by SIGHUP/SIGINT/SIGTERM (128 + the signal). A command can return these too, so a wrapper that must
tell them apart reads the failure line (`heavy <version> (<os>) exit 0x<hex>: <cause>. Next: <step>.`, printed once on stderr,
as a `::error::` annotation when `CI` is set) or the log (`SessionId`, `Exit`, the `heavy.run` events). On Windows the exit
status is the full 32 bits (an NTSTATUS crash is never turned into 0); elsewhere it is the low byte, and a non-zero code whose
low byte is 0 becomes 1.

## Compatible with the shell plock (rollout)

The lock folder, record formats and names are the retired shell plock's layout, kept so an old record or marker is still read: the
lock is the directory `pitcrew-heavy-v2.lock` published atomically with its 6-line `owner` record (pid, start text, agent, epoch
seconds, cwd, masked command; old 3-line records are read), tickets `pitcrew-heavy.ticket.<016 microseconds>-<pid>` (4 lines), high
markers `pitcrew-heavy.high.<pid>-<name>` (3 lines), the old-style `pitcrew-heavy.lock` is honoured, and breaks/removals and
waits/takes go to `pitcrew-lock-breaks.log` / `pitcrew-lock-activity.log`. More slots use extra lock dirs
`pitcrew-heavy-v2.lock.slotN`. The owner record has an optional 7th line `child <pid> <os start microseconds>`, written once the
command runs: a waiter does not break the lock of a SIGKILLed heavy whose command still runs.

## Slots follow the load

`HEAVY_SLOTS` (or `--slots N`) is the most steps that may run together: `auto` (the default) is about half the cores and at most
5 (ER-24: about 5 builders on a 14-core Mac), a number is that maximum, 1 is one at a time. How many run at a moment is
`gate::allowed_slots`, read by the head of the queue on every poll:

- the cores the **15-minute load** leaves free, 2 cores a step (`CARGO_BUILD_JOBS=2`): `(cores - load15) / 2`, rounded down, between 1
  and the maximum. The count is of all steps running, heavy's own included: their load is already in the average (late), so the rule
  is the stricter reading of one thing, never a sum;
- one while the **CPU idle share** is under 30 %, and one while the **1-minute load** is at or over the core count (the fast brake: a
  burst the 15-minute average has not seen). `make -l N` and `ninja -l N` are the same shape, a count of jobs and a load brake; they
  use the 1-minute load alone, which reacts at once and flaps; this opens on the 15-minute load and closes on either;
- the **memory available** (free plus reclaimable) after a reserve of 2 GiB, 3 GiB a step (`MEMORY_RESERVE_MB`, `MEMORY_PER_SLOT_MB` in `gate.rs`, the one
  place they are decided): a second step is refused when it would not fit. On 2026-11-22 free memory fell to about 1 GB while load and CPU still looked
  acceptable (ER-24). The first step always runs;
- one when the load, the idle share or the memory is unknown (Windows has no load average; the first CPU reading needs a sampling interval, it
  is taken when heavy starts and a second one 200 ms later). Unknown is never read as plenty (ER-33).

Common practice: Bazel's `--local_resources` / `--jobs` and the cargo and make jobservers admit work by a resource budget (cores, RAM), not by one
step at a time; `make -l` and `ninja -l` add a load brake. This rule is the same shape with the 15-minute load and a memory budget.

**Each step's threads (ER-48).** The command gets `CARGO_BUILD_JOBS` = `min(2, cores / most steps at once)` unless the caller already set one (the
caller's value wins, ER-47), so steps at once times jobs stay within the cores. It is logged once (`heavy.run`, `Jobs`).

## Quick rows go first (the history)

A row that takes seconds (a warm clippy: 1 s after 270 to 948 s in the queue on 2026-11-23) must not wait behind steps that take minutes. A caller names a
row with `HEAVY_ROW` (its step plus its workspace, for example `clippy apps/dyno`; the run's agent name differs every run, so it cannot be the key).
heavy keeps how long each row took when it passed (`heavy-history.tsv` in the lock folder, written only by heavy): the expected time is the slowest of
its last 3 runs. A row expected under 10 s (`CHEAP_SECS`) is **quick**: when the machine has the idle CPU (15 %), the memory for a step more (the reserve
stays) and a 1-minute load under twice the cores (`cheap_admitted`; the 15-minute load does not decide it), it takes a place in the **quick lane**
(`pitcrew-heavy-v2.lock.cheapN`, at most 2 at once) beside the full slots, without waiting in the queue. The queued long steps are not delayed by it (the
lane does not count against their slots) and a container run waits for the lane to empty and starts nothing beside it. No history, an unreadable history,
a row at or over 10 s, or unknown readings: the row queues like any other (ER-33). A quick row that cannot get a place keeps its ticket and its place in
the FIFO queue; a waiter holds a ticket, never a slot (ER-43).

The history is bounded (ER-51): at most 5 lines a row and 512 in all, oldest first; a file over 256 KiB is read as no history. A failed run is not
recorded. It is a hint: a writer that loses a race loses a line.

**Why a stall explains itself (ER-27).** Every admission writes a `heavy.admit` event with the lane (`slot` or `quick`), how many steps ran, how many the
rule allowed, the cores, the 1- and 15-minute load, the CPU idle share, the memory available and the row's expected seconds; the wait line carries the same
numbers (`1 of at most 5 at once (14 cores, 15-minute load 15.0, CPU idle 80%, memory available 2100 MiB)`). `queue-stall` and `run-max` are unchanged.

**What to expect on this Mac:** its 15-minute load has been 16 to 48 on 14 cores for days, so `auto` gives 1 (the old behaviour) at its usual
loads, and more only when the machine is quiet (docs/benchmarks.md). The count stays at 1 for up to a quarter of an hour after a spike.

A **container run** (a command line with `docker`, `podman`, `nerdctl` or `linux-container.sh`) runs alone, and nothing starts beside a live
one, because the VM behind it takes memory and cores that heavy cannot see. The limit, plainly: heavy tells only the container runs that go
through heavy; one started outside it is invisible except through the load and idle share it causes. A container run that has waited
`HEAVY_PRIORITY_MAX` / 6 (300 s) for the machine to be free of other steps writes a drain marker (`pitcrew-heavy.drain.<pid>-<name>`): every other step
waits, high-priority ones too, until it has run (the marker counts for `HEAVY_PRIORITY_MAX` at most), so a stream of steps cannot keep it out for good.

A waiter holds a ticket, never a slot (ER-43). After taking a slot a step looks at the folder again: when more steps run than the rule
allows now (the load rose, a container run arrived, a high-priority run came in beside the head) and it is one of the younger ones (by the time of
its record, then pid), it gives the slot back and waits again with its ticket, so the oldest holder always keeps its slot. Takers read the load at
slightly different moments, so they can disagree by one poll: the count can pass the rule for one poll interval and settles at the next look; a step
back costs the younger one that interval. Running steps are never stopped when the load rises. The wait line says what the rule saw: `1 running, 1 of at most 5 at
once (14 cores, 15-minute load 15.0, CPU idle 80%)`. `HEAVY_CPUS` replaces the OS's core count (tests, a machine whose cores the OS misreports).

## Wait-for cycles

The queue knows who waits for whom (`waitfor.rs`): a ticket behind the head waits for the head; the head, and a high-priority run, wait
for a live drain marker's owner; a normal waiter waits for a high-priority marker that still counts; a drain owner goes by its marker and
waits for nobody in the queue (7a4aafbee). `heavy status` prints `WAIT-FOR CYCLE ...` with the members when the folder holds a cycle, and
a waiting run that is a member says so at once in its wait line and the activity log (`heavy.cycle` in the CLEF log), naming every
member. A drain owner that is a member withdraws its own marker, which frees the others and loses nothing (a drain only asks for
fairness); any other member only reports. Under the rules as they are no cycle can form; the detector is the net for a rule that is
changed wrongly later, and its model test (random normal, high-priority and drain-owning waiters) shows the old deadlock as a cycle when
the old rule is put back.

## The shell heavy.sh

**Today** (until the wrapper below lands in engineering-playbook) `scripts/heavy.sh` there is still the shell semaphore, with its own 5 slots in its
own folder (`HEAVY_LOCK_DIR`), and does not exclude Rust `heavy` holders: both run, up to 5 of each. Only Paddock's own tools are moved to `heavy`.
The wrapper (a patch prepared with this change: same arguments, the command's exit codes, 2 for usage, a refused build folder or a lock that could
not be taken, 127 with an `::error::` when `heavy` is not installed) makes engineering-playbook's script a thin caller of this binary; **landing it is a
prerequisite** of calling the shell retired, and it lands only once the binary can be installed for every project (a release from the registry that
the user installs, or a stable path the wrapper finds). `HEAVY_LOCK_DIR` is ignored with a warning.

Old binaries: a `heavy` built before this change looks at the first slot only (`slots` was 1). The first slot is shared by mutual exclusion (the
directory), and the new ones count every slot folder including the old one's and read its owner record (a container run held by an old binary keeps the
new ones out; tested). **The gap:** an old binary does not see `.slotN` holders or drain markers, so while one runs it can start beside new holders
in the other slots, and a container run waiting for a drain is not waited for by it. Move every machine to the new binary together (the install below).

## Environment

`HEAVY_*` wins over the legacy `PLOCK_*`; `HEAVY_LOCK_DIR` (heavy.sh) is ignored. A value that does not parse is reported and the default used.

| Setting | Names | Default |
|---|---|---|
| lock folder | `HEAVY_DIR`, `PLOCK_DIR`, `PLOCK_ROOT`, `HEAVY_LOCK_DIR` | `/private/tmp/claude-501` if it exists, else `$TMPDIR/heavy-lock` |
| poll seconds | `HEAVY_POLL`, `PLOCK_POLL` | 5 |
| seconds between "waiting for X" lines | `HEAVY_WAIT_LOG`, `PLOCK_WAIT_LOG` | 60 |
| ownerless lock/ticket broken after | `HEAVY_UNREADABLE_LIMIT`, `PLOCK_UNREADABLE_LIMIT` | 120 s |
| load gate threshold / poll | `HEAVY_GATE_LOAD` (`--gate-load`), `HEAVY_GATE_POLL` | 30 / 20 s |
| priority bound (`--high` yields to nobody; others stop yielding after this) | `HEAVY_PRIORITY_MAX` | 1800 s |
| most steps at once | `--slots N`, `HEAVY_SLOTS` (`auto` or 1..64) | auto: half the cores, at most 5 |
| the row's key for the run-time history (quick rows go first) | `HEAVY_ROW` | none: no history is read or kept |
| cores counted against the load | `HEAVY_CPUS` | the OS's count |
| wait for the command's stderr pipe after it exited (ms) | `HEAVY_DRAIN_MS` | 500 |
| correlation id | `DEVKIT_RUN_ID` is reused as the run id and passed to the command | made per run |
| log | `HEAVY_LOG_FILE` (`--log-file`), `HEAVY_VERBOSE=1` (`--verbose`) | `<folder>/heavy-logs/heavy.<day>.log` |
| stderr tail kept for failures | `HEAVY_STDERR_TAIL=0` turns it off | on |
| home guard | `HEAVY_ALLOW_HOME_TARGET=1`, `HEAVY_BUILD_ROOT_HINT` | on |

## Parity with the two shell scripts

| Feature | plock | heavy.sh | heavy |
|---|---|---|---|
| FIFO queue, `--high` first | yes | no (polling) | yes |
| load gate | 30, fixed | none | threshold and poll settable; skipped with a note on Windows (no load average there) |
| N slots | no | `HEAVY_SLOTS` | `HEAVY_SLOTS` / `--slots`, following the 15-minute load and CPU idle |
| home guard (ER-14) | no | yes | yes (`CARGO_TARGET_DIR`, `CARGO_BUILD_TARGET_DIR`, `--target-dir`, `--config build.target-dir`, `build.target-dir` in `.cargo/config.toml` up the tree and in `$CARGO_HOME`) |
| slot kept while the command runs although the owner died | yes (`cpid`) | yes | yes (line 7 of the owner record) |
| ends the command's group on INT/TERM | no | yes | yes, by process identity (`paddock_proc::kill_tree`, rule 53); 130/143 |
| zombie owner counts as gone | no | yes | yes |
| lower priority | not built in (callers use `nice`) | none | `--low` (`paddock_proc::lower_priority`: nice 10, below-normal on Windows) |
| masked command in records | yes | n/a | yes, same rules |
| troubleshooting log | activity/breaks logs | none | the same two logs plus a CLEF log (below) |

Differences to know: the command's stderr is a pipe (tee'd to your stderr) so a failure can show its tail; turn it off with
`HEAVY_STDERR_TAIL=0`. Log timestamps are UTC (the shell's were local time). The home guard is new for plock users.

## Troubleshooting log (rule 56)

Every run writes CLEF JSON lines (the format of `paddock-log`, ADR 0036; the crate itself is too large for a lock CLI) to
`<folder>/heavy-logs/heavy.<day>.log`, or `--log-file`. `SessionId` is the run's correlation id; `--verbose` echoes events to
stderr. Events: `run starts` (exact masked command, cwd, settings), each wait reason with the holder's pid, record epoch and
the observed OS start time (`Evidence`), every break, put-back and stale removal with the same evidence, phases with
`DurationMs` (`queue_wait`, `take`, `gate_wait`, `run`, `release`), and `run ends`/`the command failed` with the exit code
(signal name, or the NTSTATUS on Windows) and the stderr tail. With `CI` set, a failure prints one
`::error::heavy: <cause>. Next: <step>. (run <id>, log <file>)` line.

## Design: lessons made impossible

- Liveness compares the OS start time (`paddock_proc::ProcessId`: sysctl / `/proc` / `GetProcessTimes`) with the record's
  epoch plus 2 s; the start *text* in the record is informational and never read, so TZ and locale cannot matter.
- Every read tolerates the file vanishing (no parse of a missing value); records parse totally (proptest).
- Deletions name exact paths; nothing is globbed into a delete.
- The lock appears atomically with its owner inside; a write that leaves nothing is never published.
- A non-empty unparsable owner is never broken. An empty one only after the age limit.
- A break renames the lock aside, re-checks the directory identity and the owner, and puts it back if either changed.
- A pid we may not signal (EPERM) or whose start time is unreadable is alive. Pid 0 and out-of-range pids are never probed.
- Waits are logged every 60 s with the holder; the queue is FIFO.

## Install and release

**Install (once per machine, no manual download):** in a Paddock checkout run

```
./dev heavy-install
```

It reads the version of this checkout's `tools/heavy/Cargo.toml` (or `--version X.Y.Z`), downloads the asset for this OS and CPU and the
release's `SHA256SUMS` from the repository's Gitea release `heavy-vX.Y.Z` (`PADDOCK_HEAVY_RELEASES` replaces the base URL; the token is
`PADDOCK_DEV_TOKEN`, else `GITHUB_TOKEN`, sent through a private curl config and never printed), refuses a checksum that does not match
(`::error::heavy-install: the downloaded ... SHA256SUMS says ...`, nothing installed), renames the binary atomically into
`<dev-targets>/heavy/bin/heavy` and records the version in `<dev-targets>/heavy/VERSION`. Run again, it downloads nothing while the same
version is there and unchanged. It never touches `~/.cargo` or `~/.rustup`. Then `heavy --version` and `heavy status` check it, and
engineering-playbook's `scripts/heavy.sh` wrapper finds it (`HEAVY_BIN`, then PATH, then `$PADDOCK_DEV_TARGETS/heavy/bin/heavy`; when it is missing
the wrapper prints this one command and exits 127: it never downloads by itself).

**Requirements:** `curl` and a release over `https` (a plain `http` `PADDOCK_HEAVY_RELEASES` is refused unless it is on this machine's loopback, so the
token is never sent in the clear); the Linux binaries are built in the CI image (Ubuntu 24.04) and need **glibc 2.39 or newer**; the version
(`--version`) must be a semantic version. Measured 2026-11-19: a HEAD request to an existing release asset of this Gitea
(`.../releases/download/dyno-latest/latest.json`, no token) answers 200 with no redirect, so the install follows none (curl's default) and uses
`--proto =https`. The work folder is private (0700), the curl config (the token) is created 0600 and deleted as soon as curl ends, and folders left by a
killed run are swept at the next start. On Windows a binary that runs is moved aside (`heavy.exe.old.<pid>`) and the new one renamed into place.

**Release (CI):** the tag `heavy-vX.Y.Z` (requested through the tag workflow, after this reaches main) starts `.gitea/workflows/heavy-release.yml`: it
builds macOS arm64, Linux x86_64 and arm64 and Windows x86_64, attaches `heavy-<version>-<target>[.exe]` and `SHA256SUMS` to the Gitea release
(`conformance/release/publish.mjs`) and, when `paddock-proc` and `paddock-env` are in the `decryp7` registry at the versions this crate asks for,
publishes the crate `paddock-heavy` there as well (the registry has `paddock-proc` 0.1.0 only today: a `v*` tag publishes 0.2 first; the crate step
is optional, the release is what machines install from).

CI leg needed (not added here: `.gitea/` and `./dev plan` belong to the integrating agent): a `tools/heavy` job in
`ci.yml` on Linux, macOS and Windows runners with `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo nextest run --profile ci` and `cargo deny check` in `tools/heavy`; `paddock.ci.json`/the planner entry for the
`tools/heavy` workspace (inputs: `tools/heavy`, `crates/paddock-proc`); and `tools/heavy` in the publish plan of
`scripts/publish.mjs` (registry `decryp7`) for the tag workflow.

## Tests

`cargo nextest run` (every test is bounded: the fake clock never sleeps, real processes run under a 40 s deadline and are
killed on drop). `tests/it/ported.rs` ports the shell plock.test.sh checks onto a fake clock and fake process table;
`real.rs` runs the binary with real processes; `slots.rs` is the load-aware slot rule; `props.rs` is proptest. Gates: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo deny check`, and `cargo xwin check --target x86_64-pc-windows-msvc`.

## Priority lane (ADR 0102)

`--high` is the lane for a landing push (the pre-push gate of a `train/*`, `main` or release-tag push). A high run announces
itself with a marker and every normal run yields while the marker is live: queued, and (with several slots) running too, because
the marker then stays until the high run releases. Running steps are never killed. The yielding is bounded by
`HEAVY_PRIORITY_MAX` (1800 s): a marker older than that, or a waiter that has itself waited that long, stops counting, and the
log says so (`heavy.priority`). `--queue-max S` ends a run that took no slot in S seconds (exit 79); `--queue-stall S` ends one only when the queue showed the same picture for S seconds (the same slot holders and the same place in the queue: a queue that keeps moving is waited for), and its line names the holders; the load gate is waited for before a slot is taken (the run keeps its ticket), and a holder younger than `--run-max` counts as progress; `--run-max S` ends a command
after S seconds of running (exit 80). Every run prints `<agent> timing queued Q s, ran R s` (queue and load gate, then the run).
