# Poller pool, background threads and priorities: a survey of the monorepo

Research note, 2026-10-27. Scope: every crate and frontend in the Paddock monorepo (`crates/`, `apps/*`, `ui/`) at
`origin/integrate/monorepo` (head `1f701932b`). It answers the user's request of 2026-10-27:

1. Where would a shared **poller pool** help (one scheduler per app, coalesced due-time queue, skip-don't-queue, pause
   without subscribers, snapshot + diff, FakeClock tests, same mechanism on every OS)? First user: Dyno's process list.
2. Which **dedicated background threads** should move onto a shared pool, and which must stay?
3. Where does **job priority** matter today?

The design that follows from it is the proposed [ADR 0020](../adr/0020-shared-executor.md) (one executor in
`paddock-jobs` with three modes: `submit`, `every`, `lease`, plus priorities).

**Method and evidence.** Read-only survey: no builds, no runs, no measurements. Candidate sites came from `git grep`
over non-test code (each file cut at its first `#[cfg(test)]`; `tests/`, benches, examples, fuzz, test kits excluded) for
`thread::sleep`, `recv_timeout`, `wait_timeout`, `park_timeout`, tokio `sleep`/`interval`, `clock.wait`, thread spawns,
`spawn_blocking`, `thread::scope`, and in TypeScript `setInterval`, `setTimeout` loops, rxjs `interval`/`timer`. 409 Rust
wait hits and 312 thread hits were then read in context and classified. Wake-up rates and thread counts are **counted
from the code, not measured**; treat them as estimates. OS facts are from the vendor pages cited.

## Summary

| Class | Meaning | Count | Where most of them are |
|---|---|---:|---|
| **A** | periodic check/poll that should move onto the pool | **44** | Debrief 13, Dyno + Pitcrew 13, shared crates 10, Pitwall + Spotter 8 |
| **B** | periodic, but stays separate (reason given) | **33** Rust + 1 TS | real-time trackers, other processes, condvar fallbacks, job-scoped progress mirrors |
| **C** | one-off wait, timeout, back-off, cancel poll (counted only) | **218** Rust; 155 UI-only TS timers | Debrief 103, shared 47, Dyno + Pitcrew 35, Pitwall + Spotter 33 |
| **D** | frontend polling that should be pushed from Rust via `paddock-live` | **17** | Dyno 8, Pitcrew 4, Debrief 4, Pitwall 1 |
| **E** | dedicated thread doing short or periodic work: should run on a shared pool | **~65** sites | one thread per job/action/timer (Debrief ~30 incl. the A threads, Dyno + Pitcrew ~18, shared 15, Pitwall + Spotter 2) |
| **F** | must stay a dedicated thread (reason given) | **~67** sites/groups | pipe and socket readers, lifelines, OS trace consumers, abandonable calls, DB writers |

Already pooled and left alone: 38 + 22 + ~15 + Dyno/Pitcrew command `spawn_blocking` sites (Tauri/tokio blocking pool,
idle threads expire) and 23 bounded `thread::scope` fan-outs.

**Idle threads and wake-ups (estimates, one window open, nobody on a detail page):**

| Process | Periodic threads today | Wake-ups/s today | After | Wake-ups/s after |
|---|---:|---:|---|---:|
| Debrief main (C = 3 supervised children) | ~9 | ~27 | 1 scheduler + 1–2 workers (blocked) | ~5 |
| Debrief store worker | 0 extra (2 condvar fallbacks) | 4 | same threads, pure condvar | 0 |
| dyno-agent | 16 total (8 loop threads) | ~24 + **~200 (accept busy-poll)** | ~10 total | ~0.6 |
| Dyno app | 5 | ~4.3 | 4 | ~0.3 |
| pitcrew-service | 4 + 2 per runner | ~6 | same count | ~1 |
| Pitcrew app | 1 (+1 for ~7 min after start) | low | 1 | low |
| pitwall-server | ~16 (tokio workers + cache-db) | ~0.7 + **0.4 process spawns/s** on macOS | same (pollers are tokio tasks) | ~0.1 without a client, 0 spawns |
| Spotter app | +1 watchdog per running render | 4 per render + 0.2 | 0 per render | ~per due check |
| Shared crates, once an app adopts them (1 watched folder) | ~5 | ~23 | 1 scheduler + 1–2 workers | 1–2 |

**Top 5 wins**

1. **Unix `accept` busy-poll** (`crates/paddock-ipc/src/endpoint/unix.rs:106-128`, copied in
   `apps/dyno/crates/dyno-ipc/src/endpoint/unix.rs:108`): a non-blocking `accept` retried every 5 ms, so every
   listening process (dyno-agent, dyno-helper, any paddock-ipc server) wakes ~200 times a second forever. Not a pool
   change: block in `poll(2)` with the timeout (or a wake pipe). Verified by reading the code.
2. **dyno-agent's eight loop threads** (network, hardware, issues, extras, unusual, netwatch, watched, boot settle) become
   checks on one scheduler; five copy the same 250 ms "sliced sleep" `wait` closure. ~24 wake-ups/s and ~6 threads. The
   sampler itself becomes the first diff-emitting check (process list).
3. **One shared process-table check** instead of a thread per supervised child: `debrief-ipc/src/process.rs:237`
   (200 ms, one thread per child) and `paddock-proc/src/supervised.rs:129-144` (200 ms, one per child) read the same data
   that Dyno's process list, `paddock-metrics::MachineProbe::process_memory`, `debrief-monitor` and pitcrew-service's
   `probe.rs` each read on their own. C−1 threads and 5·C wake-ups/s in Debrief alone.
4. **Folder-watch wake loops**: `paddock-watch/src/source.rs:12,106` wakes every 50 ms per watched folder (20/s) even
   with no change; Debrief's `watched.rs:212` scans every 5 s but naps in 100 ms steps (10/s per window) and keeps its own
   settle logic although `paddock_watch::Settle` was extracted from it.
5. **Pitwall's release tick** (`pitwall-server/src/serving.rs:1063`, `os_signals.rs:44`) starts `/usr/sbin/sysctl` and
   `/usr/bin/pmset -g assertions` every 5 s on macOS (~17 000 process spawns a day), and the 2 s live loop
   (`app.rs:353`) runs with no client connected. Read the values through `sysctlbyname`/IOKit; pause the loop without a
   subscriber.

Honourable mentions: Debrief's plugin watchdog (`debrief-core/src/plugin/watchdog.rs:53`, 10 wake-ups/s at idle: wait on a
condvar until the earliest budget instead); the 17 frontend polls (D); Spotter's two 50 ms cancel polls; one thread spawned
per guarded plugin call just to time it (`paddock-guard/src/guard.rs:154`).

**Prerequisite to most wins: adoption.** Only `shell-demo` and `workbench-demo` depend on the in-tree shared crates today;
Debrief uses `paddock-proc` from three crates, Pitwall and Spotter depend on no `paddock-*` crate, and `paddock-live` and
`paddock-metrics` have no consumer. Shared-crate threads therefore run in the demos today and in each app once it adopts
the shell; app-side wins need the app to take `paddock-jobs` first.

## What already exists (checked first, rule 42)

| Crate | Has | Fits the pool? |
|---|---|---|
| `paddock-jobs::time` | `Clock` port (monotonic, wall, cancellable `wait`), `SystemClock`, `FakeClock` (`advance`, `waiters`, `wait_for_pending`), `Deadline`, `Backoff`/`BackoffPolicy` | **Yes, reuse as is.** The scheduler sleeps with `clock.wait(next_due, token)`; tests move a `FakeClock`. To wake early on a new registration it needs a fresh `CancelToken` per wait (or a `Condvar`-based wake on the clock); a small addition. |
| `paddock-jobs::schedule` | user-facing schedules (every N, times of day, cron, zones, DST), jitter, back-off, catch-up (`due`), pause windows | **No, not as the poll engine:** every interval is clamped to `MIN_INTERVAL_SECS = 10` (`schedule/mod.rs:24`) and it computes one next run, not a queue. It stays the layer for user schedules (Spotter watches, Pitwall update checks) that the pool *hosts* as checks whose interval function calls `next_check`. |
| `paddock-jobs::JobPool` | bounded FIFO worker pool, `spawn_job`, `offload`, `hold` + `assert_nonblocking` | **Base for the worker side**; no priority, no OS priority, FIFO only. Instances only in shell-demo (2), workbench-demo (1) and `paddock-updater` (2). |
| `paddock-jobs` `CancelToken` | user/deadline reasons, `on_cancel` wakers | Reuse; needs `child()` tokens and more reasons (parent, condition, shutdown) for the executor's abort rules. |
| `paddock-watch` | `Debouncer` (Clock-driven), `ChangeSource` port, `NotifySource` (OS events, feature `notify`), `spawn` (one thread per folder, 50 ms wake), `Settle` + `scan_folder` | `ChangeSource` is **folder-only and payload-free** (`watch(dir, recursive, tx: Sender<()>)`): it cannot carry added/removed/changed diffs. The pool needs a generic diff type; `paddock-watch` can gain a poll-backed `ChangeSource` built on it. |
| `paddock-metrics::Sampler` | pure `sample()` over a `MachineProbe` on a `Clock` | A ready-made snapshot function with no driver: the pool becomes its driver. |
| `paddock-live` | `Hub` with `subscribers()`, bounded queues, `Source` + `runner::run` reconnect, Tauri channel registration | Provides the **demand signal** (`Hub::subscribers()`) for pausing checks and the **sink** for diffs; no consumer yet. |
| Pitwall `pitwall-core/src/scheduler.rs` | pure priority queue: Interactive > Background > Batch, aging (30 s per class), global/per-model/per-app caps, pause, depth per class | **The model for the executor's priority queue** (lift, don't rewrite). |

## Per-app findings

Paths are relative to the app folder unless they start with `crates/`. "thr" = dedicated threads alive at idle.

### Dyno (first user)

**Process list today.** It is read in **dyno-agent**, thread `dyno-sampler` (`crates/dyno-agent/src/lib.rs:64`, loop
`sampler.rs:49`). Intervals come from `dyno-core/src/sampling.rs:35` (defaults 1 s system, 2 s process table,
`settings.rs:66-67`; without a watcher ≥ 2 s / ≥ 10 s, 3 s with rules; battery doubles; own CPU over budget stretches).
The sleep is a condvar a new subscriber wakes. **Every read is the full table:**
`dyno-platform/src/processes.rs:118` refreshes every process with CPU, memory, disk, exe and user and builds a full
`ProcessSample` each; there is no cheap identity pass. The whole `processes` + `groups` list is cloned into every
`LiveUpdate` and pushed every tick while the Processes page is open (`sampler.rs:274-325`, `server.rs:97`); nothing is
diffed. `network.rs`, `unusual.rs` and the counters read the latest table.

**Fit:** (1) an identity check every 1 s (pid + start time) that diffs added/removed; (2) rates and details only for new,
shown and top-N processes; (3) deltas through `paddock-live`. `dyno_core::sampling::intervals` maps directly onto the
pool's "no subscriber → slow, no window → slower" demand.

| Class | file:line | What | Interval | thr today | After | Saving |
|---|---|---|---|---|---|---|
| A | `crates/dyno-agent/src/sampler.rs:49` | system sample + process table + rules + history + push | 1–2 s / 2–10 s adaptive | 1 | 2 checks: system (inline), processes (worker, diff) | 1 thread; diff shrinks work and IPC |
| A | `crates/dyno-agent/src/lib.rs:72-84`, `network.rs:155` | listener/connection look | 30 s in 500 ms slices | 1 | check 30 s | 1 thr, ~2/s |
| A | `crates/dyno-agent/src/hardware.rs:364` | facts, baselines 60 s, slow cycle 3 h | 500 ms loop with gates | 1 | 3 checks | 1 thr, ~2/s |
| A | `crates/dyno-agent/src/issues.rs:174` | OS log issue scan in maintenance windows | 15 min, 250 ms slices | 1 | check, gate `maintenance_open` | 1 thr, 4/s |
| A | `crates/dyno-agent/src/extras/mod.rs:439` | backup check + software diff | 6 h, 250 ms slices | 1 | check | 1 thr, 4/s |
| A | `crates/dyno-agent/src/extras/mod.rs:416,445` | boot settle | 5 s for 10 min | 1 (10 min) | temporary check | 1 thr |
| A | `crates/dyno-agent/src/unusual.rs:219` | unusual-program look | 30 s, 250 ms slices | 1 | check, or subscriber of the process diff | 1 thr, 4/s |
| A | `crates/dyno-agent/src/netwatch.rs:59` | interfaces, Wi-Fi, DNS change look | 20 s, 250 ms slices | 1 | check | 1 thr, 4/s |
| A | `crates/dyno-agent/src/handlers_watch.rs:180` | watched-folder sweeper | 30 s, 250 ms slices | 1 | check | 1 thr, 4/s |
| A | `crates/dyno-agent/src/sampler.rs:137`, `counters.rs:246` | pinned counters | 15 s, spawns a thread per read | 0 (+1 per read) | check on worker; the OS read stays abandonable (F) | 1 spawn per 15 s |
| A | `app/src-tauri/src/lib.rs:91` | heartbeat: re-emit agent status | 3 s | 1 | app check, paused without a window (or a live feed) | 1 thr |
| B | `crates/dyno-agent/src/server.rs:47` + `dyno-ipc/src/endpoint/unix.rs:108` | accept loop | 400 ms + **5 ms busy-poll** | 1 | stays; blocking accept | **~200/s** |
| B | `app/src-tauri/src/link.rs:304` | `peer.is_closed()` poll | 250 ms | 1 | wait on `Peer::wait_closed` | 4/s |
| B | `crates/dyno-cli/src/lib.rs:159` | CLI rate read | once | CLI | – | – |
| E | `crates/dyno-agent/src/network.rs:130,182`; `inventory.rs:12`; `extras/backup_check.rs:68`; `helper_centre.rs:272`; app `popover.rs:312`, `tray.rs:205,217`, `shell.rs:217` | per-request refresh, DNS workers, one-offs, fade timers | – | per use | worker pool / one-shot timers | churn |
| F | `crates/dyno-agent/src/server.rs:97`; `dyno-ipc/src/peer.rs:180-208`; `counters.rs:149,246`; `dyno-platform/src/unix_control.rs:235`; `dyno-platform/src/process.rs:35`; `dyno-ipc/src/child.rs:232`; `dyno-helper/src/server.rs:67`; bench | push drains, peer reader/writer/4 workers, abandonable counter reads, CPU-cap duty cycle (100 ms real-time), pipe drains, lifeline, helper sessions, benchmark | – | – | stay (Peer workers 4 → 2 possible) | – |

C: 26 (app 6, agent 6, platform 4, ipc 5, helper 3, cli 1, plugin host 1). D (frontend): `state/counters.store.ts:110` (2 s),
`network.store.ts:65` (2 s, full snapshot), `throttle.store.ts:38` (15 s), `baselines.store.ts:38` (60 s),
`features/watched/watched-page.ts:129` (30 s), `bench.store.ts:33` (1 s), `backup.store.ts:38` (1.5 s),
`helper.store.ts:50` (0.7 s): all of these are engine state the agent already has.

### Pitcrew

| Class | file:line | What | Interval | thr | After | Saving |
|---|---|---|---|---|---|---|
| A | `crates/pitcrew-service/src/supervisor.rs:474` | actor loop: `recv_timeout(250 ms)`, 1 s tick (probe 5 s, sample 30 s, save 15 s, exe 30 s) | 250 ms / 1 s | actor stays | scheduler posts `Msg::Tick` per due sub-step | ~3–4/s |
| A | `app/src-tauri/src/lib.rs:109` | macOS service-registration repair | 3 s then 60 s × 7 | 1 for ~7 min | temporary check | 1 thr |
| B | `crates/pitcrew-service/src/host.rs:103` | main waits for signal or `finished()` | 500 ms | main | await a oneshot | 2/s |
| B | `app/src-tauri/src/events.rs:171` | subscription stream, reconnect 1→15 s | back-off | 1 | stays (a `paddock-live` `Source` later) | – |
| E | `app/src-tauri/src/{events.rs:113,channels.rs:134,service_install.rs:366,service_client.rs:114}`; `pitcrew-service/src/supervisor.rs:1044`, `supervisor/ops.rs:24-105` | per action / per runner work | – | per use | worker pool, background class | churn |
| F | `pitcrew-service/src/host.rs:35,42`, `ipc.rs:227,258`, `process.rs:48`, `launch.rs:52` | actor, tokio IPC runtime, runner output readers | – | 3 + 2/runner | stay | – |

C: 9. D: `features/logs/logs-page.ts:64` (2 s), `features/runners/processes.store.ts:47` (5 s, same data as Dyno's
process check), `features/dashboard/dashboard-page.ts:177` (60 s), `state/service.store.ts:56,60` (3 s / 5 s; duplicate
of Pitwall's, while `paddock-health` already emits service health).

### Debrief

| Class | file:line | What | Interval | thr | After | Saving |
|---|---|---|---|---|---|---|
| A | `app/src-tauri/src/ai/pitwall.rs:129` | Pitwall health probe | 15 s | 1 | check (network → worker), paused when engine is built-in | 1 thr |
| A | `app/src-tauri/src/ai/update_commands.rs:165` | AI model/runtime update tick | 60 s | 1 | check | 1 thr |
| A | `app/src-tauri/src/health/commands.rs:236` | service health probe | 3 s | 1 | check, 5 s grid, slower without a window | 1 thr |
| A | `app/src-tauri/src/startup_watchdog.rs:247` | window start-up checkpoints | 500 ms for app life | 1 | check that pauses when no window is pending | 1 thr, 2/s; **duplicate of `paddock-startup`** |
| A | `crates/debrief-ipc/src/process.rs:237` | memory cap of each sandboxed child | 200 ms | 1 per child | one check over all children (process-table snapshot) | C−1 thr, 5·C/s |
| A | `app/src-tauri/src/watched.rs:212` | watched folders scan/settle/import | 5 s, 100 ms naps | 1 per window | check per window | 10/s per window; **duplicate of `paddock_watch::Settle`** |
| A | `app/src-tauri/src/jobs/service.rs:573` | jobs progress ticker | 400 ms while jobs run | 1 while busy | check paused without active jobs | churn; **duplicate of paddock-jobs** |
| A | `app/src-tauri/src/browser/sessions.rs:657` | AI browser liveness | 200 ms while open | 1 per session | check | 1 thr |
| A | `crates/chromium-driver/src/launch.rs:282` | browser time limit | 250 ms | 1 per browser | one-shot `after(deadline)` | 1 thr, 4/s |
| A | `crates/debrief-ai/src/llm/lazy.rs:179` | unload idle built-in model | 30 s | 1 | check | 1 thr |
| A | `crates/debrief-ai/src/engine/pitwall/mod.rs:385` | pairing status poll | 1 s while pairing | 1 | check in `paddock-pitwall` | 1 thr |
| A | `app/src-tauri/src/live.rs:444` | live-tail carry flush | carry_flush_ms | 1 per session while held | re-armed one-shot | 1 thr |
| A | `app/src-tauri/src/opener.rs:241` | reap opener helper | 100 ms | 1 per open | child-exit wait + deadline | 10/s while alive |
| B | `crates/debrief-core/src/plugin/watchdog.rs:53` | plugin call budget watchdog | 10 ms busy / 100 ms idle | 1 | **own thread kept** (must not share a pool with what it watches); wait until the earliest budget | 10/s → 0 |
| B | `crates/debrief-store-duck/src/catalogue.rs:239`, `fts.rs:730` | condvar + 500 ms fallback | 500 ms | store worker | pure condvar | 4/s |
| B | `app/src-tauri/src/drag_ghost/mod.rs:307`; `debrief-plugin-native/src/sampling/live.rs:281` | cursor follower 16 ms; stack sampler | real-time | during use | stay | – |
| B | `crates/debrief-core/src/workspace/tail/watch.rs:273` | live-tail coalescer over notify | event-driven | 1 per session | stays (latency) | – |
| B | `crates/debrief-monitor/src/lib.rs:81`, `debrief-plugin-tools/src/monitor_tool.rs:72` | process monitor, 20 ms stop naps | 2 s | other process | `Clock::wait` + cancel | 50/s → 0.5; duplicate of Dyno's sampling |
| B | `crates/debrief-plugin-native/src/tool.rs:69`, `debrief-plugin-dotnet/src/plugin/tool.rs:79` | capture running? | 250 ms | plugin process | stay; identical copies | – |
| B | `app/src-tauri/src/{ai/runtime_commands.rs:172,video/runtime_commands.rs:105,query_cancel.rs:45,ai/folder_setup.rs:30}` | progress mirrors | 250 ms | job-scoped | push progress instead (ER-18), not the pool | – |
| B | plugin-windows `eventlog/follower.rs:301,682`, `plugin-api/src/forward.rs:121`, `debrief-agent/src/agent.rs:205`, store-worker `server.rs:314`, CLI ×2 | other processes or batching | – | – | stay | – |

E (~17 sites + the 13 A threads): one thread per job (`jobs/service.rs:518`, `:859`), after-paint tasks
(`after_first_paint.rs:66`, a 10 s sleep-only thread at `:81`), **`browser/research.rs:463` sleeps the full
`DRAFT_TIMEOUT` even after the draft finished** (a leaked sleeping thread per research turn: a bug worth its own
regression test), SQL console timeout (`debrief-store-duck/src/console/mod.rs:209`), start-up prunes (`lib.rs:647,685`),
repair/settings/tool-stop/window-drop actions, `names.rs:275` (500 ms poll up to 10 min), live-tail reaper, import session
thread, plugin-host warm-up, config writer. F (~30): child stderr readers and lifelines, budgeted plugin calls
(`debrief-core/src/guard.rs:494`, must be abandonable), per-plugin event delivery, CDP socket, model owner thread,
ETW/EventPipe/event-log consumers, DuckDB index workers, exit watchdog (must work when everything else is wedged), cursor
tracker. C: 103, of which ~30 are the same "`try_wait`, sleep, until a deadline" loop (one shared
`wait_child(deadline, cancel)` in `paddock-proc` would replace them; not pool work). D: `live/tail-status-view.ts:130`
(2 s), `tools/eventlog/eventlog-view.ts:245` (1 s), `profiling/dotnet/dotnet-capture.store.ts:145` and
`profiling/native/native-capture.store.ts:195` (1 s, duplicates).

### Pitwall

Tokio-based: `pitwall-server` is `#[tokio::main]` (one worker per core), the app uses Tauri's runtime. Its pollers are
tokio tasks, so moving them saves wake-ups and work, not threads.

| Class | file:line | What | Interval | After | Saving |
|---|---|---|---|---|---|
| A | `crates/pitwall-server/src/app.rs:353-380` | live feed: memory sample, models, issues, queue, throughput; metrics every 5th tick | 2 s, always | paused when `live.receiver_count() == 0` | 0.5 → ~0.1/s without a client |
| A | `crates/pitwall-server/src/serving.rs:1063`, `os_signals.rs:44` | release tick; spawns `sysctl` and `pmset` on macOS | 5 s | 5 s grid; syscalls instead of child processes | 0.4 spawns/s → 0 |
| A | `crates/pitwall-server/src/app.rs:547-563` | cache sweeper | 60 s | check | coalesced |
| A | `crates/pitwall-server/src/updates.rs:411-421` | model update schedule | 5 min | check due at `next_due`; paused while off | ~0; `update_state.rs:177 due()` duplicates `paddock_jobs::schedule::due` |
| A | `crates/pitwall-server/src/service/watch.rs:93-110` | exe-changed watch | 60 s | check | **duplicate of `paddock_service::watch`** |
| A | `app/src-tauri/src/lib.rs:213-224` | macOS registration repair, 8 checks | 3 s then 60 s | bounded check | 1 thr for 7 min; duplicate of Pitcrew's and `paddock-service` repair |
| B | `app/src-tauri/src/live.rs:59-71` | live reconnect back-off | back-off | stays | duplicate of `paddock_live::runner` |
| B | `crates/pitwall-server/src/service/windows.rs:206` | Windows service supervisor | 500 ms | stays (separate process) | – |
| F | `crates/pitwall-server/src/cache_db.rs:130`, `service/exec.rs:25` | SQLite writer; pipe drains | – | stay | – |

C: 13. D: `features/service/service.store.ts:91,95` (3 s / 5 s, 3 IPC per tick). Separate idea (not measured): a
localhost server does not need one tokio worker per core; 2–4 would save ~10 parked threads.

### Spotter

| Class | file:line | What | Interval | After | Saving |
|---|---|---|---|---|---|
| A | `app/src-tauri/src/lifecycle.rs:138-145` | local-zone / DST refresh | 1 h | check | negligible |
| A (wake only) | `crates/spotter-engine/src/scheduler.rs:49-62` | asks `due_watches(now)` | fixed 5 s | sleep until the earliest `next_check` | 0.2/s → one per due watch |
| E | `crates/spotter-render/src/launch.rs:293-313` | `render-watchdog` per render: time limit + memory | 250 ms (memory 1 s) | check for the render's life | 1 thr per render; **duplicate of `paddock-chromium` launch watchdog** |
| F | `crates/spotter-render/src/cdp.rs:75`; `crates/spotter-sources/src/folder.rs:106` | CDP socket; folder scan (deliberately isolated: a hung share must not hold a pool thread) | – | stay | – |

C: 13 (two 50 ms `CANCEL_POLL` loops, `spotter-engine/src/engine.rs:125` and `spotter-sources/src/download.rs:114`, wake
20/s per running operation; an awaited cancel removes them). **`spotter-core/src/schedule.rs` (719 lines) and
`calendar.rs` are the original that `paddock-jobs::schedule` was copied from; the copies have drifted** (rule 42).
Spotter's check engine stays the user-facing schedule layer (class B); only its wake-up belongs to the pool model.

### Sighter, Pacenotes, demos

Sighter and Pacenotes have no Rust sources in the monorepo yet. shell-demo's agent loop (2 s heartbeat, stop file, exe
watch) is its own process (B). The demos run the shared-crate threads listed next.

### Shared crates

| Class | file:line | What | Interval | thr | After | Saving |
|---|---|---|---|---|---|---|
| A | `crates/paddock-watch/src/source.rs:12,106` | folder watch loop: `recv_timeout(50 ms)` + `debouncer.poll()` | 50 ms forever | 1 per folder | block on `recv`; debounce deadline as a one-shot | 20/s + 1 thr per folder |
| A | `crates/paddock-startup/src/tauri.rs:616-620` | start-up watchdog / liveness | 500 ms forever | 1 | 1 s check; **needs `late_by`** to keep "app was paused" detection | 1 thr, 1/s |
| A | `crates/paddock-health/src/tauri/os.rs:235-241` | quiet-hours digest ticker | `digest_every` (30 s) | 1 | check, paused without quiet hours | 1 thr |
| A | `crates/paddock-updater/src/tauri.rs:381` | update check due? | 60 s | 1 | check | 1 thr |
| A | `crates/paddock-service/src/watch.rs:96,159` | `ExeWatch` | 60 s | 1 (service process) | the agent's own scheduler | 1 thr |
| A | `crates/paddock-service/src/control.rs:345,366` | service start-up checks | first, every, times | 1 while running | bounded check | 1 thr |
| A | `crates/paddock-proc/src/supervised.rs:129-144` | memory cap per supervised tree | 200 ms | 1 per child | one process-table check for every cap | 1 thr + 5/s per child |
| A | `crates/paddock-pitwall/src/service.rs:559-564` | pairing poll | 1 s while pairing | 1 | check | 1 thr |
| A | `crates/paddock-metrics/src/sampler.rs:29` | `Sampler::sample` | no driver | 0 | the pool drives it | – |
| A | `crates/paddock-files/src/open.rs:326,337` | opener reaper | 20 ms until deadline | 1 per open | child wait + deadline | 50/s per open |
| B | `paddock-live/src/runner.rs:113`, `paddock-ipc/src/link/client.rs:308`, `paddock-pitwall/src/retry.rs:89,142` | reconnect / retry back-off | back-off | caller | stay (retry is not periodic); `retry.rs`'s `ThreadSleeper` (50 ms slices) duplicates `Clock::wait` | 20/s while waiting |
| B | `paddock-panels/src/tauri/native.rs:80`, `paddock-service/src/native.rs:438` | drag cursor tracker; SCM host loop | real-time / other process | stay | – | |
| E | `paddock-guard/src/guard.rs:154` | **timer thread per guarded call** | one-shot | 1 per plugin call | scheduler one-shot | a spawn per call |
| E | `paddock-chromium/src/launch.rs:642`, `paddock-ipc/src/handshake.rs:91`, `paddock-window/src/tauri/{lifecycle.rs:57,popover.rs:303}` | one-shot watchdogs and fades | one-shot | 1 per event | `after(deadline)` | a spawn each |
| E | `paddock-health/src/notify/writer.rs:54`, `paddock-startup/src/session.rs:286`, `paddock-live/src/tauri.rs:115` | idle-blocked writers / per-channel emitters | event | 1 each | one-shot + worker; one multiplexed emitter | memory only |
| E | `paddock-window/src/tauri/manager.rs:968`, `paddock-health/src/health.rs:130`, `paddock-service/src/control.rs:242,277`, `paddock-shell/src/setup/tauri.rs:24`, `paddock-channels/src/control.rs:378`, `paddock-assistant/src/service.rs:308` | short commands / jobs | – | per use | worker pool | churn |
| F | `paddock-jobs/src/pool.rs:70` and scoped helpers; `paddock-ipc` readers/accept/link; `paddock-log/src/child.rs:72`; `paddock-proc/src/run.rs:158,212`; `paddock-mcp`; `paddock-chromium/src/cdp.rs:91`; `paddock-download`; `paddock-updater/src/updater.rs:523`; `paddock-guard/src/guard.rs:140` | the pool itself, pipe/socket readers, abandonable blocking calls | – | – | stay | – |

C: 47 (several are cancel-by-polling, `recv_timeout(TICK)` + `cancel.check()`, that `CancelToken::on_cancel` could wake
instead). TS: `ui/projects/paddock-ui/workbench/panels/src/ui-sync.ts`'s hello repeat is a bounded handshake (1 s × ≤ 10,
C); Debrief's `popout/ui-sync.ts` is a copy of it. paddock-ui's jobs/notify clocks are the ER-19 "Still working" tick and
stay.

## All background threads

Dedicated threads fall into four shapes; only the first two belong on a shared pool.

1. **Sleep-loop threads** (the A sites): exist only to wait N and look. → `every` checks. Today ~9 in Debrief, ~8 in
   dyno-agent, ~5 in an app on the shared shell.
2. **Thread per short task or one-shot timer** (E): a new OS thread per job, action, guarded call, handshake, fade or
   deadline. → `submit` on a bounded pool, or `after(deadline)` on the scheduler. They cost spawn churn and, where they
   sleep to a deadline, idle threads (`research.rs:463` keeps one for the full timeout).
3. **Lifelong blocking waits** (F): pipe and socket readers, lifelines, accept loops, OS trace consumers, DB writer
   threads, actors. They cost memory, not wake-ups, and must stay dedicated; what can be shared is naming, caps and
   introspection (`lease`).
4. **Abandonable calls** (F): work that may hang in the OS or a plugin (guarded plugin calls, counter reads,
   `getaddrinfo`, blocking HTTP, browser launch). A pool worker stuck there is lost, so they run on throwaway threads
   today; the executor must either keep that (lease + abandon) or replace the worker it loses.

Counts per app (sites; E = should move, F = stays):

| App / area | E | F | Already pooled |
|---|---:|---:|---|
| Debrief | ~30 (17 + 13 A threads) | ~30 | 38 `spawn_blocking`, 23 `thread::scope` |
| Dyno | ~10 | ~10 groups | command `spawn_blocking` |
| Pitcrew | ~8 | ~4 groups | command `spawn_blocking` |
| Pitwall | 1 (+ repair thread) | 3 | ~13 `spawn_blocking` |
| Spotter | 1 | 2 | 3 `spawn_blocking` |
| Shared crates | 15 | ~20 groups | 22 `spawn_blocking` |

Threads alive at idle are in the summary table; the blocking threads of shape 3 (≈ 2 per supervised child, 1 per plugin
for event delivery, peer reader/writer per connection) are unchanged by the pool. Two side wins in shape 3: Dyno's peer
keeps 4 workers per connection (2 would do), and pitwall-server's tokio runtime has one worker per core.

## Priority today

Priority exists in two places: **Pitwall's pure scheduler** (`pitwall-core/src/scheduler.rs`: Interactive > Background >
Batch, aging one class per 30 s, global / per-model / per-app caps, per-app ceiling; clients send `X-Pitwall-Priority`,
Debrief chooses in `app/src-tauri/src/ai/core.rs:644-650`) and **ad-hoc "lower this thread/process" calls**. Everything
else is FIFO with per-kind limits (Debrief's jobs: one queue per `JobKind`, a new thread per job at normal priority,
`jobs/model.rs:64-71`; manual up/down only for AI jobs, `queue.rs:400`) or per-call semaphores (Spotter). No app has "run
next", and no job model has parents or children.

| file:line | What competes | Today | Class | Does a reserved Interactive worker fix it? |
|---|---|---|---|---|
| `debrief-store-worker/src/server.rs:1-5`; `debrief-ipc/src/peer.rs:393,413` | grid/timeline/facet queries vs import vs analysis vs export in one store-worker process | every call on a `spawn_blocking` thread; process at normal priority on purpose (`debrief-store-client/src/client.rs:273`) | queries Interactive; import User; FTS/catalogue Background | **No.** Calls already run in parallel; the contention is CPU inside the engines. The class must travel with the IPC call and set the OS priority of the thread that serves it. |
| `debrief-core/src/ingest/parallel.rs:164`, `pipeline.rs:216`, `discover.rs:262` | import parse/walk pools (cores−1, new pool per import) vs queries | **normal priority** (rule 13 asks for background priority) | User (Utility) | Partly: lowering the import pools lets query threads win the CPU. |
| `debrief-store-duck/src/lib.rs:67,622,841` | one DuckDB with `threads = 4` shared by every statement | DuckDB's own scheduler; no per-statement priority (unverified that none exists) | – | **No**; mitigations are cancelling superseded queries (exists) and bounded write steps. |
| `debrief-store-duck/src/fts.rs:657`, `catalogue.rs:199` | write lock: import vs compaction vs FTS vs catalogue | FTS/catalogue `try_lock` and yield a step | Background | Already cooperative: the model for "yield at a checkpoint". |
| `app/src-tauri/src/jobs/model.rs:26-28,86`; `ai/commands.rs:350` | chat turn vs background AI use cases in one AI queue (limit 1) | FIFO + manual up/down | chat Interactive | **Yes in kind**: a class-ordered queue; the model itself is the bottleneck (Pitwall already orders by header). |
| `spotter-engine/src/check.rs:141-160`, `scheduler.rs:9,34`, app `commands.rs:171-188` | scheduled checks vs "Check now" vs Chromium renders | each batch builds its own `Semaphore(max_concurrent)`: together they exceed the cap | Check now Interactive, scheduled Background | **Yes**: one shared group limit "render" with classes and a reserved slot. |
| `dyno-agent/src/lib.rs:64-131`, `network.rs:130,182`, `inventory.rs:12` | live sampler vs scans, inventory, DNS | all normal priority; no `lower_priority` in dyno-agent | sampler Interactive when shown; scans User; DNS Background; bench stays normal | Classes help once these share a pool. |

**Duplicated priority helpers that disagree** (rule 42): `debrief-core/src/threads.rs:116-136` (macOS
`QOS_CLASS_BACKGROUND`, Linux nice 10, Windows `THREAD_PRIORITY_BELOW_NORMAL`: CPU only) and
`debrief-ai/src/models/priority.rs:1-41` (macOS `QOS_CLASS_UTILITY`, Windows `THREAD_MODE_BACKGROUND_BEGIN`: CPU, I/O
and memory); process level `paddock-proc/src/sys.rs:43-50` and `debrief-ipc/src/process.rs:381-385`.

**OS facts** (vendor documentation, read 2026-10-27):

- Windows: `THREAD_PRIORITY_*` changes CPU priority only; background work should use `THREAD_MODE_BACKGROUND_BEGIN`,
  which lowers CPU, I/O and memory priority, applies to the current thread only, and can be ended with
  `THREAD_MODE_BACKGROUND_END`.
  <https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setthreadpriority>
- Linux: nice is per thread; an unprivileged thread can lower but not raise it again without `CAP_SYS_NICE` or an
  `RLIMIT_NICE` allowance; leaving `SCHED_IDLE` needs the same. I/O priority is per thread (`ioprio_set`), the idle
  class is allowed unprivileged. <https://man7.org/linux/man-pages/man2/setpriority.2.html>,
  <https://man7.org/linux/man-pages/man7/sched.7.html>, <https://man7.org/linux/man-pages/man2/ioprio_set.2.html>
- macOS: QoS classes (user-interactive, user-initiated, utility, background) drive CPU, I/O throughput and timer
  latency; set per thread with `pthread_set_qos_class_self_np`; a mutex holder is promoted to its waiter's QoS.
  <https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/PrioritizeWorkAtTheTaskLevel.html>
  Whether a thread may raise its own QoS again is not stated there (**unverified**).

**Design consequence:** a worker thread cannot portably be lowered for one task and raised for the next (Linux), so an
executor needs workers with a fixed OS priority per tier, and "bump" can reorder queued work but cannot raise a running
thread on every OS.

## Duplicated poll logic across apps (rule 42)

1. **Process tables:** Dyno (`dyno-platform/src/processes.rs`, sysinfo 0.39), pitcrew-service (`probe.rs:47`,
   sysinfo 0.37), `paddock-metrics::MachineProbe::process_memory`, `paddock-proc` memory caps, `debrief-ipc` memory caps,
   `debrief-monitor`/`plugin-tools`, Spotter's render watchdog, Pitwall's live loop memory sample. → one
   `paddock-proc` process-table snapshot behind one check.
2. **Schedules:** `spotter-core/src/schedule.rs` + `calendar.rs` (the original), `paddock-jobs::schedule` (the copy),
   Pitwall `update_state.rs:177 due()`.
3. **Exe-changed watch and service repair:** `paddock-service::watch`, Pitwall `service/watch.rs`, Pitwall and Pitcrew
   app repair loops, Dyno `lib.rs:102`.
4. **Reconnect loops:** `paddock_live::runner`, Pitwall app `live.rs`, `dyno-link`, `pitcrew-events`.
5. **Watchdogs:** `paddock-startup` vs Debrief `startup_watchdog.rs`; `paddock-chromium` launch watchdog vs Spotter
   render watchdog vs Debrief `chromium-driver`.
6. **Folder settle:** `paddock_watch::Settle` vs Debrief `watched.rs` vs Spotter folder sources vs Dyno watched sweeper.
7. **`run_with_timeout` / `try_wait` loops:** dyno-platform (10 ms), pitcrew-service (25 ms), Pitwall `exec.rs`, ~30
   in Debrief, `paddock-files` opener (20 ms). → `paddock-proc::wait_child(deadline, cancel)`.
8. **IPC:** `dyno-ipc` is a copy of `paddock-ipc` (with the same 5 ms accept loop).
9. **Frontend:** service repair/down polling in Pitcrew and Pitwall with the same constants; 8 "now" signal ticks
   (paddock-ui has `src/lib/clock.ts`); `popout/ui-sync.ts` copy; drag-button poll in Debrief TS and `paddock-panels`.
10. **Sliced sleeps:** five dyno-agent loops copy one `wait` closure; `paddock-pitwall`'s `ThreadSleeper` re-implements
    `Clock::wait`.

## Relation to the existing crates: extend, not a new crate

- **`paddock-jobs`: extend** with an `exec` module (the user's direction): it already owns `Clock`, `FakeClock`,
  `CancelToken`, `Deadline`, `Backoff`, `JobPool`, `Workers` and `schedule`, and every app that runs long work already
  needs it. A separate crate would depend on all of those and add nothing but a boundary. The pure timetable and queue
  stay free of threads so they can be unit-tested and reused by a tokio driver (feature `tokio`) for Pitwall and
  Spotter.
- **`paddock-watch`: extend** with a poll-backed `ChangeSource` (snapshot = `scan_folder`) for "same mechanism on every
  OS", and replace its 50 ms wake loop with a blocking `recv` plus a debounce one-shot. Whether folders keep the OS
  `NotifySource` or poll is a decision (see the ADR).
- **`paddock-metrics`: no change to the sampler;** an app registers `Sampler::sample` as a check.
- **`paddock-live`: small extension:** a demand source from `Hub::subscribers()` and a sink that publishes diff batches.
- **`paddock-proc`: extend** with a process-table snapshot (pid + start time cheap; details on demand) and
  `wait_child(deadline, cancel)`.

The API sketch, the priority design, the validation against every A/E site, the work packages and the risks are in
[ADR 0020](../adr/0020-shared-executor.md).
