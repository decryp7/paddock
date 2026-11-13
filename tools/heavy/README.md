# heavy

One heavy step (cargo, npm, ng, playwright, llvm-cov, xwin) at a time per machine. Replaces the shell `plock`
(Paddock agents) and engineering-rules `scripts/heavy.sh` (ER-24). Package `paddock-heavy`, binary `heavy`; its own Cargo
workspace and lockfile (`tools/heavy/`, ADR 0014's third kind).

```sh
heavy run [--high] [--no-gate] [--low] [--gate-load N] [--slots N] [--queue-max S] [--run-max S] [--verbose] [--log-file FILE] <agent> -- <command...>
heavy status
```

`run` queues (a FIFO ticket; `--high` goes first), takes the lock, waits for the load gate (1-minute load below the
threshold, default 30), runs the command with your stdio, releases, and exits with the command's code (127 not found, 126
not runnable, 128+signal for a signal, 130/143 when `heavy` itself was interrupted/terminated; the command's whole process
tree is ended then).

## Exit codes (the wrapper rule)

The command's own code is passed through. heavy's own failures use codes that do not look like a usual command failure (1, 2):
**64** usage, **75** the lock could not be taken, **78** refused (the home guard), **79** no slot within `--queue-max` (the command never ran), **80** the command was ended at `--run-max`, **126/127** the command could not be started,
**129/130/143** stopped by SIGHUP/SIGINT/SIGTERM (128 + the signal). A command can return these too, so a wrapper that must
tell them apart reads the failure line (`heavy <version> (<os>) exit 0x<hex>: <cause>. Next: <step>.`, printed once on stderr,
as a `::error::` annotation when `CI` is set) or the log (`SessionId`, `Exit`, the `heavy.run` events). On Windows the exit
status is the full 32 bits (an NTSTATUS crash is never turned into 0); elsewhere it is the low byte, and a non-zero code whose
low byte is 0 becomes 1.

## Compatible with the shell plock (rollout)

The lock folder, record formats and names are the shell plock's, byte for byte where a shell reader looks: the lock is the
directory `pitcrew-heavy-v2.lock` published atomically with its 6-line `owner` record (pid, start text, agent, epoch seconds,
cwd, masked command; old 3-line records are read), tickets `pitcrew-heavy.ticket.<016 microseconds>-<pid>` (4 lines), high
markers `pitcrew-heavy.high.<pid>-<name>` (3 lines), the old-style `pitcrew-heavy.lock` is honoured, and breaks/removals
and waits/takes go to `pitcrew-lock-breaks.log` / `pitcrew-lock-activity.log`. Shell and Rust holders and waiters
interoperate (tests/it/mixed.rs runs both on one folder). Slots above 1 use extra lock dirs
the shell never looks at: leave it at 1 until the shell is retired. The owner record has an optional 7th line
`child <pid> <os start microseconds>`, written once the command runs: the shell reads six lines and ignores it, and a Rust waiter
does not break the lock of a SIGKILLed heavy whose command still runs (heavy.sh's "a slot is taken while either lives").

### heavy.sh callers move at the same time

heavy.sh keeps its slots in `HEAVY_LOCK_DIR/<n>` (pid and cpid files); heavy does **not** honour that layout. A heavy.sh
caller and a heavy caller on the same machine do not exclude each other, so every caller of `scripts/heavy.sh` moves to heavy
in the same change (the rollout below makes `heavy.sh` exec the binary). To keep an inherited heavy.sh environment from silently
changing behaviour: `HEAVY_LOCK_DIR` is ignored with a warning, and `HEAVY_SLOTS` is ignored with a warning unless `HEAVY_DIR`
is set too (or `--slots` is given).

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
| slots | `--slots`, or `HEAVY_SLOTS` together with `HEAVY_DIR` | 1 |
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
| N slots | no | `HEAVY_SLOTS` | `HEAVY_SLOTS` / `--slots` |
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

## Rollout

1. **Install** from the registry once the tag workflow has published `paddock-heavy`:
   `cargo install paddock-heavy --registry decryp7 --locked --root /Volumes/d7sst7/dev-targets/heavy` (the binary lands in
   `.../heavy/bin/heavy`). Check with `heavy --version` and `heavy status`.
2. **Switch the agents' `plock`** (`/Volumes/d7sst7/dev-targets/scratch/pitcrew-lock/plock`) to a thin wrapper:
   `exec /Volumes/d7sst7/dev-targets/heavy/bin/heavy "$@"` with `HEAVY_DIR=/private/tmp/claude-501` exported (the same folder, so
   running shell and Rust holders interoperate). Keep the old script as `plock.sh.bak`. Slots stay at 1. Builds with a target
   folder under `~` are now refused (exit 78).
3. **engineering-rules:** make `scripts/heavy.sh` exec the binary when present
   (`exec heavy run "${HEAVY_AGENT:-$USER}" -- "$@"`), keeping the old body as the fallback, and update the ER-24 text: heavy
   steps go through `heavy`, the load gate is 30 by default, `HEAVY_SLOTS` needs `HEAVY_DIR`. Every heavy.sh caller moves in the
   same change (see above).
4. **Retire the shells** after a few days of mixed use with no entries in `heavy-logs/` or `pitcrew-lock-breaks.log` that need
   explaining: delete `plock`, `plock.test.sh`, the `heavy.sh` fallback body, and then raise `HEAVY_SLOTS` if wanted.

CI leg needed (not added here: `.gitea/` and `scripts/affected.mjs` belong to the integrating agent): a `tools/heavy` job in
`ci.yml` on Linux, macOS and Windows runners with `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo nextest run --profile ci` and `cargo deny check` in `tools/heavy`; `paddock.ci.json`/the planner entry for the
`tools/heavy` workspace (inputs: `tools/heavy`, `crates/paddock-proc`); and `tools/heavy` in the publish plan of
`scripts/publish.mjs` (registry `decryp7`) for the tag workflow.

## Tests

`cargo nextest run` (every test is bounded: the fake clock never sleeps, real processes run under a 40 s deadline and are
killed on drop). `tests/it/ported.rs` ports the shell plock.test.sh checks onto a fake clock and fake process table;
`real.rs` runs the binary with real processes; `mixed.rs` (macOS) runs the reference shell script
`tests/fixtures/plock.sh` and the binary on one folder; `props.rs` is proptest. Gates: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo deny check`, and `cargo xwin check --target x86_64-pc-windows-msvc`.

## Priority lane (ADR 0102)

`--high` is the lane for a landing push (the pre-push gate of a `train/*`, `main` or release-tag push). A high run announces
itself with a marker and every normal run yields while the marker is live: queued, and (with several slots) running too, because
the marker then stays until the high run releases. Running steps are never killed. The yielding is bounded by
`HEAVY_PRIORITY_MAX` (1800 s): a marker older than that, or a waiter that has itself waited that long, stops counting, and the
log says so (`heavy.priority`). `--queue-max S` ends a run that took no slot in S seconds (exit 79); `--run-max S` ends a command
after S seconds of running (exit 80). Every run prints `<agent> timing queued Q s, ran R s` (queue and load gate, then the run).
