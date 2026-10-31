# heavy

One heavy step (cargo, npm, ng, playwright, llvm-cov, xwin) at a time per machine. Replaces the shell `plock`
(Paddock agents) and engineering-rules `scripts/heavy.sh` (ER-24). Package `paddock-heavy`, binary `heavy`; its own Cargo
workspace and lockfile (`tools/heavy/`, ADR 0014's third kind).

```sh
heavy run [--high] [--no-gate] [--low] [--gate-load N] [--slots N] [--verbose] [--log-file FILE] <agent> -- <command...>
heavy status
```

`run` queues (a FIFO ticket; `--high` goes first), takes the lock, waits for the load gate (1-minute load below the
threshold, default 30), runs the command with your stdio, releases, and exits with the command's code (127 not found, 126
not runnable, 128+signal for a signal, 130/143 when `heavy` itself was interrupted/terminated; the command's whole process
tree is ended then). Exit 2 is a usage error or the home guard.

## Compatible with the shell plock (rollout)

The lock folder, record formats and names are the shell plock's, byte for byte where a shell reader looks: the lock is the
directory `pitcrew-heavy-v2.lock` published atomically with its 6-line `owner` record (pid, start text, agent, epoch seconds,
cwd, masked command; old 3-line records are read), tickets `pitcrew-heavy.ticket.<016 microseconds>-<pid>` (4 lines), high
markers `pitcrew-heavy.high.<pid>-<name>` (3 lines), the old-style `pitcrew-heavy.lock` is honoured, and breaks/removals
and waits/takes go to `pitcrew-lock-breaks.log` / `pitcrew-lock-activity.log`. Shell and Rust holders and waiters
interoperate (tests/it/mixed.rs runs both on one folder). Slots above 1 (`HEAVY_SLOTS`, from heavy.sh) use extra lock dirs
the shell never looks at: leave it at 1 until the shell is retired.

## Environment

`HEAVY_*` wins over the legacy `PLOCK_*`. A value that does not parse is reported and the default used.

| Setting | Names | Default |
|---|---|---|
| lock folder | `HEAVY_DIR`, `PLOCK_DIR`, `PLOCK_ROOT`, `HEAVY_LOCK_DIR` | `/private/tmp/claude-501` if it exists, else `$TMPDIR/heavy-lock` |
| poll seconds | `HEAVY_POLL`, `PLOCK_POLL` | 5 |
| seconds between "waiting for X" lines | `HEAVY_WAIT_LOG`, `PLOCK_WAIT_LOG` | 60 |
| ownerless lock/ticket broken after | `HEAVY_UNREADABLE_LIMIT`, `PLOCK_UNREADABLE_LIMIT` | 120 s |
| load gate threshold / poll | `HEAVY_GATE_LOAD` (`--gate-load`), `HEAVY_GATE_POLL` | 30 / 20 s |
| slots | `HEAVY_SLOTS` (`--slots`) | 1 |
| log | `HEAVY_LOG_FILE` (`--log-file`), `HEAVY_VERBOSE=1` (`--verbose`) | `<folder>/heavy-logs/heavy.<day>.log` |
| stderr tail kept for failures | `HEAVY_STDERR_TAIL=0` turns it off | on |
| home guard | `HEAVY_ALLOW_HOME_TARGET=1`, `HEAVY_BUILD_ROOT_HINT` | on |

## Parity with the two shell scripts

| Feature | plock | heavy.sh | heavy |
|---|---|---|---|
| FIFO queue, `--high` first | yes | no (polling) | yes |
| load gate | 30, fixed | none | threshold and poll settable; skipped with a note on Windows (no load average there) |
| N slots | no | `HEAVY_SLOTS` | `HEAVY_SLOTS` / `--slots` |
| home guard (ER-14) | no | yes | yes (`CARGO_TARGET_DIR`, `--target-dir`, `--config build.target-dir`) |
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

## Tests

`cargo nextest run` (every test is bounded: the fake clock never sleeps, real processes run under a 40 s deadline and are
killed on drop). `tests/it/ported.rs` ports the shell plock.test.sh checks onto a fake clock and fake process table;
`real.rs` runs the binary with real processes; `mixed.rs` (macOS) runs the reference shell script
`tests/fixtures/plock.sh` and the binary on one folder; `props.rs` is proptest. Gates: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo deny check`, and `cargo xwin check --target x86_64-pc-windows-msvc`.
