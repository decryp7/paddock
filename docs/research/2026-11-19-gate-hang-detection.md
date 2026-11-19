# Telling a hung gate row from a slow one (2026-11-19)

Short research note. Question: the push gate (`./dev check --pre-push [--member-gate]`) ends a row when a fixed budget runs out
and reports it `not measured`, which also cuts off slow but healthy work (cold doctest builds). What do well-known tools do, which
signals do we have, and what should replace the budget? No code is changed here.

**Heads read** (ADR 0091): `origin/train/2026-11-19-16` c551abd45df0, `origin/main` f5f08f71e598, both on 2026-11-19 and re-read
before the commit. Code citations are at the train head unless they say main.

**Verified** = read in the code at that head, measured here, or read on the primary source named. **Read** = a secondary source
or a summary. **Unverified** = from memory or not found on a primary source.

## 0. What the gate does today (verified, code)

- One total budget: `BUDGET = 15 min` of run time (`tools/paddock-dev/src/pre_push/exec.rs:18`; ADR 0038 amendment A2.5), time
  queued behind other agents credited back (ADR 0102, `pre_push/budget.rs`). Each step also has its own limit,
  `own_run_limit` = 3000 s heavy, 900 s otherwise (`exec.rs:282`), and runs with `heavy --run-max = min(own, budget left)`
  (`exec.rs:869`).
- `heavy` ends the command at `--run-max` with `paddock_proc::kill_tree` on the child's `ProcessId` (pid plus start time,
  rule 53; `tools/heavy/src/runner.rs:376`, `crates/paddock-proc/src/tree.rs:232`) and exits 80; the gate turns that into
  `not measured` (`Done::cut_short`, `exec.rs:55`; row at `exec.rs:383`). Nothing records *why* it was slow: no stack, no
  "last progress", no CPU reading.
- The family already has the right rule and the shared mechanism: ADR 0031 ("Patience, not wall clocks", accepted
  2026-10-29) says a liveness bound "becomes a stall judgement and never fires only because time passed", and
  `paddock_proc::StartJudge` implements it (progress counter, tree CPU time, run state, spin limit in CPU-seconds, idle limit
  in ticks, a hang guard only when nothing is observable; `crates/paddock-proc/src/start.rs`, defaults at `:48`). It is on
  main since 7f8fbc3dd and shipped (`git tag --contains`: dyno-v1.0.0, dyno-v1.1.0). The gate does not use it.

### What the budget actually cut (measured, synthetic analysis of a tool log)

heavy's activity log on this Mac (`pitcrew-lock-activity.log`, 2026-11-14 13:02Z to 2026-11-19, 13,622 lines; tool log, no user
data; only aggregates are quoted). `done rc=…, ran …s` lines, by command kind:

| Kind | n | p50 s | p90 s | p99 s | max s |
|---|---:|---:|---:|---:|---:|
| cargo clippy (host) | 1521 | 5 | 113 | 340 | 491 |
| cargo xwin clippy | 445 | 13 | 219 | 355 | 410 |
| clippy in the Linux container | 269 | 12 | 87 | 521 | 899 |
| cargo test --doc | 46 | 27 | 228 | 672 | 672 |
| cargo nextest | 214 | 48 | 200 | 654 | 757 |
| npm / npx | 155 | 10 | 41 | 129 | 139 |

- **87 steps were ended at their run limit (exit 80)** in those five days (14th: 17, 15th: 10, 16th: 5, 17th: 26, 18th: 24,
  19th: 5): host clippy 57, xwin 20, doctest 6, Linux clippy 4. Their run time at the cut: **p50 33 s**, min 1 s, max 899 s.
  So most cuts were not a step running long: the push's *total* budget had been spent by earlier or sibling rows, and the next
  step got what was left. None of the 87 is shown to have hung; the log cannot tell.
- A cold app doctest build takes **1122 s** (`apps/dyno`, `cargo test --doc --workspace`, load 30+; docs/benchmarks.md
  "Doctest row of the push gate", line 2077), over both the 900 s budget and any 5-minute no-progress guard that counts
  silence. Train 15's cut-offs (cold doctests across 10 workspaces) are from the brief; I found no run or lesson recording
  them, so they are not an incident row here.
- One real hang class did occur: heavy's drain owner and the queue head waited on each other (a wait-for cycle), "seen live
  on train 16" and fixed by 7a4aafbee with a randomised queue model test. That commit is on `origin/train/2026-11-19-16` only,
  not on main and in no tag.

## 1. Common practice (sources)

| Tool | Hang vs slow | Source |
|---|---|---|
| GitHub Actions | Fixed `timeout-minutes` per job (default 360) and per step; no progress or silence detection. | <https://docs.github.com/en/actions/writing-workflows/workflow-syntax-for-github-actions> (verified) |
| GitLab CI | Fixed job `timeout`; project default 60 min; the lower of project and runner timeout wins. No silence detection. | <https://docs.gitlab.com/ci/pipelines/settings/>, <https://docs.gitlab.com/ci/yaml/#timeout> (verified) |
| Buildkite | Fixed `timeout_in_minutes` per command step, pipeline default and maximum; on timeout SIGTERM, 10 s, then kill. No silence detection found. | <https://buildkite.com/docs/pipelines/configure/build-timeouts> (verified; heartbeat/agent-lost detection unverified) |
| Jenkins Build Timeout | Four strategies: Absolute; **No Activity** (countdown restarted by every log write); **Elastic** (percentage of the mean of the last N builds that were UNSTABLE or better, with a default when there is no history); **Likely stuck** (10 x the estimated duration, 24 h with no estimate). Jenkins core's estimate is the mean of up to 3 recent builds, successful or unstable first. | plugin source `src/main/java/hudson/plugins/build_timeout/impl/` in <https://github.com/jenkinsci/build-timeout-plugin> (read via a summary); `Job.getEstimatedDuration` in <https://github.com/jenkinsci/jenkins/blob/master/core/src/main/java/hudson/model/Job.java> (verified) |
| Bazel | Per test target a size class with a fixed timeout (60/300/900/3600 s); `--test_timeout` overrides; `--test_verbose_timeout_warnings` warns when a test's timeout is far longer than its run (history used to *tighten*, not to kill). No action-level hang detection found. | <https://bazel.build/reference/test-encyclopedia>, <https://bazel.build/docs/user-manual> (verified) |
| cargo-nextest | Per test: `slow-timeout` period (default 60 s, marks SLOW) and `terminate-after` periods, SIGTERM then SIGKILL after `grace-period` (10 s); `global-timeout` for the run; per-test overrides by filter. Leak detection: a subprocess still holding the test's stdout/stderr `leak-timeout` (100 ms) after exit is LEAK, optionally a failure. libtest-json output is experimental. | <https://nexte.st/docs/features/slow-tests/>, <https://nexte.st/docs/features/leaky-tests/>, <https://nexte.st/docs/machine-readable/libtest-json/> (verified). Paddock uses it: `crates/.config/nextest.toml:18` (30 s x 6), `:33` (leak 500 ms). |
| Go test | Fixed `-timeout` per test binary (default 10 m); on expiry the alarm calls `debug.SetTraceback("all")` and panics "test timed out after …", naming the running tests, so every goroutine's stack is printed. | `src/testing/testing.go` `startAlarm` (verified), <https://pkg.go.dev/cmd/go#hdr-Testing_flags>, `GOTRACEBACK` in <https://pkg.go.dev/runtime> (verified) |
| Chromium test launcher | Fixed per-test timeout (`--test-launcher-timeout`), kills the process group, result TEST_TIMEOUT, retried; no stack dump; after a period of no output it lists the command lines still running. | `base/test/launcher/test_launcher.cc`, `base/test/test_switches.cc` (read via a summary) |
| Python faulthandler | In-process watchdog thread: `dump_traceback_later(timeout, exit=True)` dumps every thread's stack, "useful for hung tests". | <https://docs.python.org/3/library/faulthandler.html> (verified) |
| History-based limits | Only Jenkins Elastic and Likely-stuck derive the *kill* limit from past durations. Meta's and Develocity's predictive test selection use history to *choose* tests, not to time them out; nothing found for TAP, BuildXL or Buildkite Test Analytics. | as above; <https://arxiv.org/abs/1810.05286> (read) |

**What common practice is.** Almost everyone uses a **generous fixed cap per step** (not per pipeline-run total), plus a
finer per-test limit inside the test runner (nextest, Bazel, Go). Silence detection exists (Jenkins No Activity, Chromium's
output timeout) but only as an option. History-scaled limits are rare (Jenkins). Capturing stacks before the kill is done by
language runtimes (Go, Python), not by CI systems. Nobody kills a build for a gap between finished compilation units.

## 2. Signals here, and where each misleads

All measured on this Mac on 2026-11-19 with a synthetic two-crate workspace (`slow` with a build script that spins the CPU for N
seconds, standing in for a long link or LTO; `app` depending on it), in a scratch folder under `<dev-targets>/scratch`, cargo
1.100.0, `CARGO_BUILD_JOBS=2`; plus one cold build of `tools/heavy` through `./dev heavy`.

| Signal | What it shows | Reliable? Where it misleads (measured unless marked) |
|---|---|---|
| cargo JSON `compiler-artifact` (one per unit), `build-script-executed`, `build-finished` | Real progress: a unit finished. <https://doc.rust-lang.org/cargo/reference/external-tools.html> | Best progress signal for build rows. Gaps are normal while one unit is long: the spinning build script gave **one 20.3 s gap with no event**. Cold `tools/heavy` release build (139 units, 58 s of run at 15-minute load about 19): gap p50 0.2 s, p90 1.0 s, max 7.1 s; the final link 2.8 s. A big app's longest unit (LTO, a huge crate) is not measured yet. `--message-format=json-render-diagnostics` keeps events on stdout and the rendered warnings on stderr (tried: clippy's warning still printed normally), so the row's log and tail stay readable. |
| cargo `Blocking waiting for file lock on build directory` / `on package cache` | A wait on another cargo, not a hang. | Reliable as a *state*: printed once, then silence. The blocked cargo used **0.02 s CPU in 9 s** while the holder used about 1 CPU-s per s; it finished 0.02 s after the holder. The gate's own logs show it: 5 of 205 kept gate logs (all `lockfile` rows, "package cache"). Note: clippy and doctest of one workspace share the kept build folder `<kept>/<slug(ws)>` (`pre_push/gates.rs:296-298`, `:347-349`), so with more heavy slots (train 16's load-aware slots) one row can hold a slot while it waits on the other's lock: wasted capacity (not a deadlock, the holder needs no second slot; ER-43 spirit). |
| Process-tree CPU time (`paddock_proc::tree_cpu_time`, `usage.rs:480`) | Work being done. | **Blind to cached compiles**: with sccache the compiler runs as a child of the sccache *server* (a daemon, parent pid 1), not of cargo; the cargo-side sccache client used 0.00 s CPU (live process table, 2026-11-19). A cargo tree can read near zero CPU while compiling hard. Also blind to the Linux container row (work runs in the VM, the `docker` client is idle; reasoned, not measured, ER-14 keeps agents off ad-hoc containers). Misleads the other way on a livelock: CPU grows with no progress (ADR 0031's spin case). |
| Output (bytes or lines) | Something happened. | Silent stretches are normal: lock waits, long units, rustdoc compiling doctests, a test waiting on a socket; chatty loops (a retry printing every second) look alive. Jenkins No Activity and Chromium use it only as an option. |
| heavy queue state (ticket, slot, drain marker, owner record with `child <pid> <start>`) | Queued vs running, and behind whom. | Reliable and explicit (`tools/heavy/README.md`; `heavy status`, `lock.rs:677`). Queue time is already bounded apart (`--queue-max`, `QUEUE_MAX`, exec.rs:27) and named in the row ("behind <holder>"). Wait-for edges are not published, so a cycle like 7a4aafbee's is only visible as two waits that never end. |
| nextest per-test events / `slow-timeout` | Per-test progress and per-test hang limit. | Reliable inside nextest and already configured per workspace. libtest-json is experimental, so do not parse it as a contract; human `PASS [ x s]` lines are output, not a contract either. |
| History (durations) | Expected time. | Exists only coarsely: heavy's activity log has per-step run time by command line (table in section 0), the gate's `queue-times.log` has per-push totals (`pre_push/mod.rs:585`), the gate records (`.git/paddock-pre-push/records/*.json`) hold no durations. Per-unit and per-test history is not kept anywhere. Cold and warm builds differ by about 85x (1122 s against 13 s, benchmarks.md:2077-2078), so a mean of recent runs (Jenkins) would cut every cold build. |
| Stack before the kill | Where it hung (ER-27). | macOS `sample <pid> 1 -file <f>`: **1.1 s**, no root needed for our own child, every thread's call graph (tried on a stand-in `sleep`: `nanosleep`). Linux: `/proc/<pid>/stack` is the kernel stack only and needs ptrace-attach rights (<https://man7.org/linux/man-pages/man5/proc_pid_stack.5.html>); user stacks need `eu-stack` or `gdb`, not installed here (unverified in the CI image). Windows: no built-in tool (procdump is a download; unverified). Rust binaries print no stack on SIGQUIT (unverified). |

## 3. Recommended design

Replace "a row ends when the push's total budget runs out" with **a per-row stall judgement on the row's own progress, with a
generous per-row cap as the backstop**, built on `paddock_proc::StartJudge` (ADR 0031, rule 42 share first), not a new watchdog.

1. **No total budget kills a row.** The 15-minute total stays only as the point after which *new* rows do not start (they are
   `not measured`, as today), so a push still has a bound (ER-32). A row that has started runs until it finishes, is judged
   stuck, or hits its cap.
2. **Per-row cap (the common-practice backstop):** 60 min per row (GitLab's default job timeout; measured cause: the slowest
   healthy row known is the cold app doctest, 1122 s, so 60 min is over 3x it). Recorded next to the constant with this note
   (ER-12). heavy's `--run-max` carries it, as today.
3. **Progress per row kind** (the judge's `progress` counter):
   - cargo rows (clippy, xwin, doctest, nextest's build phase): `--message-format=json-render-diagnostics`, progress = count of
     `compiler-artifact` + `build-script-executed`; for doctests, also each `test … ok/FAILED` line after `Doc-tests`.
   - nextest and npm/vitest rows: output lines (the runners' own per-test limits do the per-test work: nextest
     `slow-timeout`, vitest `testTimeout`).
   - Linux container row: output lines only (CPU is not visible).
4. **Explicit wait states, never counted as stall:** heavy queued (already separate), and cargo's `Blocking waiting for file
   lock` until the next event. While waiting, the row names what it waits on. The gate stops two of its own rows from sharing a
   build folder at once (serialise them; it costs nothing, they would serialise on cargo's lock anyway and today one of them
   may hold a slot meanwhile).
5. **Stall rule.** Stuck = **no progress for the unit-gap limit while not in a wait state**. CPU is used only to *word* the
   report (spinning vs idle), not to decide, because tree CPU is blind to sccache and containers (section 2). Unit-gap limit
   without history (cold start): **15 min**, the existing per-step value re-scoped from "a whole row" to "one gap between
   finished units"; the largest gap measured so far is 7.1 s (tools/heavy) and 20 s (synthetic), so the first work package
   measures the longest unit of every app workspace cold with `cargo build --timings` under load and sets the value from
   that (ER-12), and the watchdog starts report-only (ER-41): it logs "would have stopped" for a week of pushes while only
   the cap kills.
6. **Before any kill: capture and log (ER-27).** The row's log gets a block: row id, phase, last progress event and its age,
   events so far, CPU growth over the last window, wait state, heavy slot and queue time, and a stack file: macOS `sample`
   of each live process of the tree (at most 8, 1 s each), Linux `eu-stack -p` when present, otherwise the process list
   with CPU times (Windows, container row). Then `kill_tree(ProcessId)` as today (rule 53). The row says `not measured
   (stuck: no crate finished for 15 min while not waiting; last: <unit> at <time>; stacks in <file>)`.
7. **History (optional, section 4):** keep per-unit durations from the event timestamps per workspace and warm/cold class;
   the gap limit becomes max(floor, k x p99 of that workspace's longest unit) with k from the measured loaded/unloaded ratio,
   which is how Jenkins Elastic works; Bazel's verbose-timeout warning is the cheaper use of the same data (report
   generous limits, tighten by hand).
8. **Positive control (ER-33):** the judge gets a fake reading stream (no progress, CPU flat; no progress, CPU growing;
   a lock line then silence; events every N ticks) and a test that removes the progress parser shows the healthy stream
   then reads as stuck; an end-to-end test runs testkit `StandIn` programs that hang, spin, and print the cargo lock line,
   and checks each row's verdict and that the stack block is written. A second test deletes the judge call and shows the
   hanging stand-in is then only ended by the cap.

The **interim guard from the brief** (no output and no tree CPU for 5 min) would misjudge three measured cases: a cargo lock
wait (0.02 s CPU, one line, then silence), a build whose compiles run in the sccache server (client CPU 0.00 s), and the
Linux container row. It would need exceptions for each, which is most of the design above.

## 4. Cost and choice (ER-37)

Estimates in agent-days, for implementation plus tests, from the shape of the code above (not measured).

| Option | What | Agent-days |
|---|---|---:|
| **A. Common practice** (cheapest) | Per-row cap of 60 min instead of the total budget killing started rows; total budget only stops new rows; `sample`/process list logged before the kill; tests | 1 |
| Brief's interim guard | No output and no tree CPU for 5 min; with the needed exceptions for lock waits, sccache and the container row | 1.5 |
| **B. Recommended for this setup** | A, plus the StartJudge-based progress watchdog (cargo JSON events, output lines, lock-wait state, own rows never share a build folder), report-only first, positive controls | 3 (2 beyond A) |
| C1. History | Per-unit and per-row duration store, k x p99 limits, Bazel-style "limit too generous" report | +2 |
| C2. heavy wait-for graph | heavy publishes wait-for edges; status and the gate detect a cycle at once | +1.5 |
| C3. Container CPU | Read the container's CPU (`docker stats`/cgroup) for the Linux row | +0.5 |
| WP-0 measurement | Longest unit per app workspace, cold, under load (`--timings`), recorded in benchmarks.md | 0.5 (needed by B) |

For a single-user tool: A is what GitHub, GitLab and Buildkite do and alone removes the measured problem (87 healthy-looking
cuts at p50 33 s of run, the 1122 s doctest). B adds the "why" in the log and catches a real hang in minutes instead of an hour;
it costs 2 days beyond A, so it needs the user's yes (ER-37). C1 is beyond common practice (only Jenkins does it) and the
cold/warm spread (85x) makes it fragile; C2 is covered for the known cycle by 7a4aafbee's model test; both are not worth it now.

### Work packages (if B is chosen)

- **WP-0** (0.5 d): measure the longest unit per app workspace cold under load (`cargo build --timings`), record in
  docs/benchmarks.md.
- **WP-1** (1 d, = option A): per-row cap, total budget only gates starts, the kill-time log block with stacks
  (`sample`/`eu-stack`/process list), tests with stand-ins.
- **WP-2** (1.5 d): the progress adapter per row kind (cargo JSON events, output lines), lock-wait state, a reading stream into
  `StartJudge`, report-only verdicts in the row log; positive controls.
- **WP-3** (0.5 d, after a week of report-only data): switch the watchdog to stopping rows; serialise own rows that share a
  build folder.

### Decisions for the user

1. **A (1 d) or B (3 d)?** Recommendation: B, ranked first; A is the fallback and is WP-1 of B anyway.
2. Per-row cap 60 min (over 3x the slowest measured healthy row) or another value.
3. History-based limits (C1) and a heavy cycle detector (C2): recommend not now.

### Risks

- cargo's JSON message set is stable, but switching the gate's cargo rows to JSON changes how the row's tail is taken
  (diagnostics move to stderr); the failure line tests must follow.
- A long silent unit (LTO, a big crate) under heavy load may exceed the cold-start gap limit before WP-0 has measured it;
  the report-only phase is the guard.
- `sample` suspends the process for its duration (1 s), harmless before a kill; Linux stacks depend on `eu-stack` being in the
  image (unverified).

Recurring: yes. Class "a wall-clock bound judges liveness" (ADR 0031's class): earlier instances CI run 7096 (Debrief
`ai::tests` past nextest's 90 s under load, ADR 0031 context) and the 2026-11-12 slot-acquire wait raised from 50 ms to 5 s
(ER-12); this note's 87 exit-80 cuts are the gate instance. The redesign ADR already exists (ADR 0031, accepted); the
proposal is to apply it to the gate through its shared judge, not a new ADR.
