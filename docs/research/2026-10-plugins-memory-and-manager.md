# Plugins: the event-log crash, memory caps, memory use and a process manager

Research note, 2026-10-27. Scope: Debrief (`apps/debrief`) and the shared Paddock plugin platform (plan X51:
`crates/paddock-plugin-*`, `paddock-ipc`, `paddock-proc`, `paddock-metrics`, `paddock-ui`). It answers four questions
from the user:

1. "Plugin crash when importing event viewer": what crashes and why.
2. "Allow plugin unlimited memory?": should the cap be a Setting, with per-plugin overrides and a clear message.
3. "Research how to reduce plugin memory": where a plugin's memory goes and what would save it.
4. "Task/process manager for plugins": pid, memory, CPU and state per plugin; lazy start and idle shutdown; CPU, memory
   and storage on the status bar.

All measurements were made on the Mac mini (Apple silicon, macOS) with generated data only (no user logs). Tests and the
generator are in `apps/debrief/crates/debrief-plugins/tests/it/evtx_memory.rs` on this branch.

## Summary

- **Root cause of the crash: the memory cap, not a parser panic, the frame limit or the timeout.** The `.evtx` importer
  in the Windows plugin (`debrief-plugin-windows/src/evtx_import.rs`) reads the whole file, copies it once more, and
  builds every event as a `Record` before it sends the first one. The plugin needs about **11 MB of memory per MB of
  event log** (about 4.4 KB per event). Under the default plugin cap of 1024 MB, **an event log of about 90 MB or more
  is stopped** by the host's memory watcher. Real System, Application and Security logs (and "Save All Events As"
  exports) are often that large.
- The stop is then **reported as a crash**: the import fails with
  `plugin "net.decryptology.windows" (import) crashed: ... closed the connection` (or `... broke (the stream ended inside
  a frame)`), because the plugin host never asks the process why it ended. The memory-cap text that `debrief-ipc` already
  builds (`ChildProcess::exit_text`) is used by the store and AI clients but not by the plugin host.
- The stop also **counts as a crash towards quarantine**: three large event logs in ten minutes quarantine the whole
  Windows plugin (every Windows importer, the annotators and markers), not just the one import.
- Raising the cap does not fix it: the need grows with the file, and the cap cannot go above 4096 MB, so a log of about
  370 MB or more cannot be imported at any setting. **The fix is a streaming importer** (constant memory, about 50-70 MB
  whatever the size), plus the honest message and no quarantine for a cap stop.
- The cap is **already a Setting** (Settings ▸ Performance ▸ Plugin memory cap, default 1024 MB, 64-4096 MB) with
  per-plugin overrides (Plugins ▸ plugin ▸ Memory cap) and "Raise the cap to about N MB" advice in service health
  (ADR 0031). Recommendation: keep a cap on by default, let the ceiling follow the machine (not a fixed 4 GiB), add a
  per-plugin "No cap" choice guarded by a machine-wide low-memory stop, and fix the message and quarantine (WP-2).
- Idle cost: Debrief starts all **12 built-in plugin processes at launch** (`Builtins::warm_up`), **11.4 MB resident
  each, 137 MB in all**, whether a plugin is used or not. Lazy start plus idle shutdown would save most of it; sniffing
  must become declarative first, or opening any file starts every importer plugin anyway.
- For the process manager, most pieces exist but are spread out: `paddock-proc` (memory per process and tree, PSS on
  Linux, caps), `paddock-metrics` (machine CPU, memory, disk, own memory, tiered history), `paddock-plugin-host` (status
  per plugin, start on first use), `paddock-ui` (status bar items, jobs). Missing: per-process CPU, one process table
  across the app's children, an idle policy, a resource meter status item and a Processes panel. These belong in
  Paddock (rule 42); Debrief only wires them.

## 1. The crash when importing Event Viewer logs

### What runs

| What the user opens | Who reads it | Where |
| --- | --- | --- |
| `.evtx` (Event Viewer's own format, "Save All Events As" `.evtx`) | `EvtxImporter`, a `BinaryImporter` of the Windows plugin | plugin process `debrief-plugins --plugin net.decryptology.windows` |
| Exported `.xml` / `.csv` / `.txt` | the engine's own parsers (`debrief-core/parsers/windows-event.toml`) | the app's import pipeline (streamed) |
| Live channels (Event Log tool) | `eventlog::Follower`, rolling CLEF files, bounded queue | Windows plugin (elevated process for Security) |

Only the `.evtx` path goes through the plugin and only it holds whole files. The live follower is bounded by design and
could not be run on macOS; it is not the reported crash (see "Windows only" below).

### The data path today

```
app (engine)                                   plugin process (cap: 1024 MB by default)
-----------                                    ------------------------------------------
read_to_end(file) -> Vec<u8>          (1x)     HostFile::read_all: ReadFile 1 MiB at a time -> Vec<u8>   (1x file)
lend it to the call                            EvtxParser::from_buffer(bytes.to_vec())                    (2x file)
                                               records_json_value(): serde_json::Value per event (short-lived)
                                               to_record -> Vec<Record>, every event                     (~4.4 KB/event)
collect every item into Vec<Record>   (~4.4 KB/event)
                                               chunk_events -> items of <= 8192 events / ~4 MB, credit 4
legacy::import_binary -> batcher               ImportDone
```

Sources: `debrief-plugin-api/src/serve.rs` (`PluginRequest::Import`: `read_all(MAX_IMPORT_BYTES)` then `import`, then
`send_imported`), `debrief-plugin-windows/src/evtx_import.rs` (`from_buffer(bytes.to_vec())`, `records.push`),
`debrief-plugin-host/src/remote.rs` (`import_call` collects `records` before returning), `debrief-core/src/ingest/source.rs`
(`PlanKind::Binary`: `read_to_end`), `debrief-core/src/ingest/legacy.rs` (`import_binary`).

### Reproduction

The generator takes the MIT/Apache-2.0 fixture `security.evtx` (one 64 KiB chunk, four events) and fills each chunk the
way a real one is filled: the two events that only reference templates are copied until the chunk is full (about 130
events per chunk, about 2,080 events per MB, the density of a real Security log), with record numbers and ids running on
across chunks. The result is a valid EVTX of any size with the shape of a large Security log.

Each size was imported through `Workspace::open` with the Windows plugin in its own process over the real IPC, the
host's memory watcher reporting the peak (`MemoryListener`). Release build, macOS (resident memory):

| `.evtx` size | events | plugin peak | time | result at a 4096 MB cap | at the default 1024 MB cap |
| ---: | ---: | ---: | ---: | --- | --- |
| 1 MB | 2,080 | 37 MB | | ok | ok |
| 16 MB | 33,280 | 211 MB | 0.5 s | ok | ok |
| 32 MB | 66,560 | 388 MB | | ok | ok |
| 64 MB | 133,120 | 749-761 MB | 2.0 s | ok | ok |
| 96 MB | 199,680 | stopped at 1,039 MB | | ok | **stopped ("crashed")** |
| 128 MB | 266,240 | 1,467 MB | 4.0 s | ok | stopped |
| 256 MB | 532,480 | 2,892 MB | 7.7 s | ok | stopped (at 1,027 MB) |
| 512 MB | 1,064,960 | stopped at 4,200 MB | 8.8 s | **stopped** | stopped |

(Rows without a time were measured on a debug build; memory is the same within a few percent.)

The peak is linear: about **11.3 MB per MB of log**: 2 MB for the two copies of the file, the rest is the `Vec<Record>`
(2.3 GB for 532,480 events, about 4.4 KB per event: a `serde_json` map with insertion order of about 20 fields, keys and
string values each in their own allocation, plus the message).

What the user sees, verbatim from the test (path shortened):

```
plugin "net.decryptology.windows" (import) crashed: plugin "net.decryptology.windows" closed the connection
plugin "net.decryptology.windows" (import) crashed: the connection to plugin "net.decryptology.windows" broke (the stream ended inside a frame)
```

and the file is listed as unrecognised with `crashed: true`; the plugin's failure count goes up by one.

### Ruled out

- **Parser panic.** The `evtx` crate (0.12.3, `validate_checksums(false)`) parsed every generated size without an error
  or a skipped event; the importer already survives corrupted chunks and random bytes (`rejects_non_evtx_and_survives_corruption`),
  and the nightly corpus test imports 280 real attack-sample logs (all under 10 MB). A panic would show as
  `crashed: <panic message>` (`code::PANIC`), not as a closed connection.
- **IPC frame limit.** Items are cut at 8,192 events or about 4 MB (`chunk_events`, `ITEM_BYTES`), file reads are 1 MiB
  (`READ_CHUNK`), and frames may be 16 MiB (`DEFAULT_MAX_FRAME`). A too-large frame would be a `Protocol` error
  ("a frame of N bytes exceeds the limit"), which never appeared.
- **Timeout.** The import call's deadline is 108 s (90% of the 120 s `Budgets::import`); 256 MB took 7.7 s. A slow
  Windows disk or machine will be slower, but the cap is hit long before the deadline at every size. The fixed deadline
  for a whole import still breaks rule 12 for very large files; WP-1 makes it a liveness check instead.

### Second-order defects found on the way

1. **The cap stop is reported as a crash.** `debrief-plugin-host::PluginProcess::call_lending` returns the peer's generic
   `IpcError::Died("... closed the connection")`; it never maps it through `ChildProcess::exit_text()`, which knows the
   process was stopped over its cap. `debrief-store-client` and `debrief-ai` already do this mapping. The shared
   `paddock-plugin-host` names the cap in `ended_text` but only when it reaps an idle process, not for a call in flight.
2. **A cap stop counts towards quarantine.** `Ctx::fail` records every `IpcError::Died` as a crash
   (`QUARANTINE_AFTER = 3`); `paddock-plugin-host` records it as `FaultKind::Crash` too. A memory stop is a resource
   limit the user can act on, not plugin misbehaviour.
3. **The nightly corpus test cannot tell a cap stop from a crash.** `evtx_corpus::check` looks for "memory cap" in the
   error, which never contains it (defect 1).
4. **The host side holds everything too.** The app reads the whole file (`read_to_end`, which may double its buffer while
   growing) and `RemoteImporter::import` collects every event into a `Vec<Record>` before the batcher sees the first one:
   the app's own memory grows by the same ~4.4 KB per event. Not capped, so not the crash, but the app's import budget
   (ADR 0014: 768 MB) is broken by any large binary log.
5. **Windows only (not reproduced here):** an elevated plugin process caps itself with a job object
   (`JobLimits::from_sandbox` in `debrief-ipc/src/sandbox.rs`), which limits *committed* memory: an allocation over it
   fails and Rust aborts the process ("memory allocation of N bytes failed") with no message. Normal plugin processes
   are watched instead (no job memory limit). The elevated path should use the same watcher.
6. **Memory figures differ by platform.** `debrief-ipc`'s watcher uses `sysinfo`'s `memory()` (resident size, Linux
   included); rule 26 asks for PSS on Linux, which `paddock-proc::process_memory` provides. It also watches only the child,
   not its tree.

### The regression tests to start the fix with

In `apps/debrief/crates/debrief-plugins/tests/it/evtx_memory.rs` (marked `#[ignore]` with the reason until the fix lands,
so the branch stays green; the fix removes the `ignore`):

```rust
#[test]
#[ignore = "fails today: the root cause of 'plugin crash when importing event viewer' (see the module docs)"]
fn a_256_mb_event_log_imports_within_the_default_plugin_cap() {
    let dir = temp("256");
    let path = generate(&dir.0, 256);
    let (outcome, peak, stopped, records, took) = import(&path, DEFAULT_CAP_MB);
    assert_eq!(stopped, None, "stopped at the {DEFAULT_CAP_MB} MB cap (peak {} MB)", peak / MB);
    assert!(outcome.is_ok(), "{outcome:?} after {took:?}");
    // Streaming bounds the plugin by its chunk pipeline, not by the file: well under half the cap.
    assert!(peak < DEFAULT_CAP_MB * MB / 2, "peak {} MB for a 256 MB log ({records} events)", peak / MB);
}

#[test]
#[ignore = "fails today: a stop at the memory cap reaches the import as \"crashed: ... closed the connection\""]
fn a_plugin_stopped_at_its_cap_says_so_instead_of_crashed() { /* 16 MB log, 128 MB cap: the error names the cap */ }
```

Today they fail with `stopped at the 1024 MB cap (peak 1027 MB)` and with
`crashed: the connection to plugin "net.decryptology.windows" broke (the stream ended inside a frame)`.
`measure` (`DEBRIEF_EVTX_MEASURE_MB=16,64,256`) and `idle_baseline` (`DEBRIEF_PLUGIN_BASELINE=1`) print the numbers in
this note.

## 2. "Allow plugin unlimited memory?"

### What exists (ADR 0031)

- `PerformanceSettings::plugin_memory_mb` (default 1024 MB; the first run suggests a quarter of the memory pool,
  512-4096 MB) and `plugin_memory` (per-plugin overrides, bounded count), live: applied at each plugin's next start.
- Plugins ▸ detail ▸ Memory cap (`plugin-memory.ts`) with the last peak; Settings ▸ Performance lists every capped
  process with its cap, use and peak (`performance_memory`).
- The hub hears the watcher: a warning at 80% (with hysteresis), and a stop becomes a service-health cause with
  "Raise the cap to about N MB, or try again" and a Raise action (`performance/watch.rs`).
- A manifest may ask for more (`memory_limit`, up to `MAX_MEMORY_LIMIT` = 4 GiB); the user's override wins.

So the user's idea is largely built. What is missing is that the import itself says "crashed", the stop is counted as a
fault, the ceiling is fixed, and raising the cap cannot help an importer whose need grows with the file.

### Recommendation

1. **Keep a cap on by default.** It is what keeps one plugin from taking the machine (and, on Linux, the OOM killer
   from picking Debrief itself). With streaming importers (WP-1) the default 1024 MB is ample for every built-in.
2. **The ceiling follows the machine.** Replace the fixed 4096 MB (`MAX_PLUGIN_MB`, `MAX_MEMORY_LIMIT`) with "up to 75%
   of physical memory" computed at start (Settings already knows the pool for its suggestions).
3. **Per-plugin "No cap", with a machine guard.** Offer "No cap" for one plugin at a time (not as the default), shown with
   a warning icon in the Plugins table. Instead of a cap, the watcher then stops the plugin only when the machine itself
   runs low (available memory under 5% or under 512 MB), with its own message. This is the honest version of
   "unlimited": the user takes the risk for one plugin, and the machine stays usable.
4. **One message, with a working action (rule 19):** "Windows plugin hit its memory limit (1,024 MB) while importing
   Security.evtx. Raise the limit…" in the import's own error (unrecognised-file reason and the problems list), with the
   Raise action opening the plugin's memory cap preset to the suggested value; the service-health cause stays as it is.
5. **A cap stop is not a crash.** It ends the call, restarts the process on the next use without back-off, and does not
   count towards quarantine. A plugin that is stopped at its cap three times in a row for *small* inputs is still worth a
   note in its status ("keeps hitting its memory limit"), not a quarantine.
6. **Shared, not Debrief-only (rule 42):** the policy (cap, no-cap guard, the stop as its own failure kind, the message)
   belongs in `paddock-proc` (`MemoryCap` gains `MemoryPolicy::{Cap(bytes), Unlimited { machine_floor }}`) and
   `paddock-plugin-host` (`Failure::OverMemory` beside `Panic`/`Hang`, not counted by `paddock-guard`'s quarantine).
   Debrief gets it when it moves to `paddock-plugin-host` (X51 adoption).

## 3. Reducing plugin memory

### Measured

- Idle: **11.3-11.5 MB resident per built-in plugin process**, 12 processes, **137 MB** in all, started at launch.
- Import of a binary log (EVTX): **~11.3 MB per MB of input**, of which ~2 MB are file copies and ~9 MB the collected
  events. The same events are collected again in the app (~4.4 KB each) before they reach the batcher.
- The IPC itself is small and bounded: items of at most 8,192 events or ~4 MB, a credit of 4 items, 1 MiB file reads.
  Each item is copied three times on its way (the `Record` → `WireEvent` clone, the postcard body, the frame), a
  transient ~15-25 MB at the worst.
- The allocator is already mimalloc v3 with v2 purge timing (`debrief-alloc`, 10 ms purge delay); it is not where the
  memory goes.

Linux PSS could not be measured on the Mac; the shape of the numbers does not depend on the platform (the cost is the
program's own heap). WP-6 adds the Linux PSS run to the nightly benchmark.

### Proposals, ranked by saving

| # | Change | Where | Expected saving (256 MB `.evtx`) |
| --- | --- | --- | --- |
| A | **Stream binary importers**: a `BinaryImporter::import_stream(ctx, reader: &mut dyn ReadSeek, sink: &mut dyn EventSink)` beside today's method (default: read all and call `import`, so third-party plugins keep working). `HostFile` becomes `Read + Seek` with a 1-4 MiB read-ahead. The EVTX importer uses `EvtxParser::from_read_seek` and emits events chunk by chunk (64 KiB chunks are independent); `serve` sends an item whenever the sink holds 8,192 events or ~4 MB. | `debrief-plugin-api` (`importer.rs`, `host_file.rs`, `serve.rs`), `debrief-plugin-windows` | plugin peak 2.9 GB → **~50-70 MB**, constant in the file size (11 MB baseline + one item batch ~36 MB + the parser's chunk and string caches) |
| B | **Stream on the host side**: `RemoteImporter` hands each item to the engine's batcher as it arrives instead of collecting a `Vec<Record>`; the engine lends the file through `HostFile` reads from disk instead of `read_to_end`. | `debrief-plugin-host/src/remote.rs`, `debrief-core/src/ingest/{source,legacy}.rs` | app peak −2.6 GB (file + all events) → the batcher's own budget |
| C | Drop the `bytes.to_vec()` copy (borrowed buffer or the stream of A). | `evtx_import.rs` | −256 MB (−9%), a one-line interim fix before A |
| D | **Smaller items for wide events**: cut items by encoded size at 1 MB rather than 4 MB, and encode the postcard body straight into the frame buffer (no separate body `Vec`). | `debrief-plugin-api/src/protocol.rs`, `debrief-ipc` frame writer | ~10-20 MB transient |
| E | **Lazy start + idle shutdown** (section 4). | host | ~11 MB per unused plugin; ~125 MB at launch for a session that uses one plugin |
| F | **Leaner records in the plugin**: interned field names (`Arc<str>` keys shared per importer), small-string values; only matters if a plugin must hold many events (A makes that rare). | `debrief-plugin-api::model` | −30-40% of the per-event cost |
| G | Allocator: give memory back after a large call (`mi_collect(true)` at the end of an import in plugin processes). Purge 0 ms halved the store worker's peak but cost 20-40% speed (ADR 0014 notes): not worth it with A. | `debrief-alloc` | small; the peak is unchanged |
| H | Shared memory or mmap handoff of the file. With A the plugin never holds the file, so this saves only the 1 MiB read copies; it would need a new handle-passing path through the AppContainer and `sandbox-exec` profiles. **Not recommended now**; reconsider for random-access formats (dumps, profiles) if they become the next large consumers. | `paddock-ipc` | ~0 after A |

Back-pressure already exists (credits on items); with A and B it reaches end to end: the batcher's flush stalls the
host's item handler, which withholds credit, which blocks the plugin's sink, which stops the parser. Cancel stays the
cooperative check per chunk (`ImportCtx::with_cancel`). The whole-import deadline becomes a liveness deadline: no item
or progress for N seconds (rule 12: a long import is "Still working", not a timeout).

Other whole-file readers in plugins (same pattern, lower urgency): `ImportProfile`, `ImportDocument`, `Resymbolise`
(`read_all`, then an encoded document sent in 1 MiB pieces) and the minidump importer. They should move to
`import_stream` when touched; a conformance check in `paddock-plugin-test` ("importer memory does not grow with input")
keeps new importers honest.

## 4. A process manager for plugins

### What exists (rule 42 check)

| Piece | Has | Missing |
| --- | --- | --- |
| `paddock-proc` | `Supervised` children with `MemoryCap` (tree memory, warning, stop, listener), `process_memory`/`tree_memory` (PSS on Linux, threads skipped), `Registry`/`end_all` | CPU per process; a typed per-process sample; a no-cap policy |
| `paddock-ipc` | launch through `paddock-proc`, `stopped_over_memory()` | the stop reason on calls in flight |
| `paddock-plugin-host` | `Supervisor` per plugin (start on first use, back-off, quarantine), `Status` (`stopped`, `starting`, `running`, `restarting`, `quarantined`, `refused`, `disabled`), `PluginHost::list` | pid, memory, CPU, calls in flight, last use; idle shutdown; an over-memory failure kind |
| `paddock-metrics` | machine CPU, memory, free disk, own memory (app + children, PSS), tiered history, `Sampler` on the shared clock | per-process series |
| `paddock-ui` | status bar with contributed items (`defineStatusItem`, priorities, compact form), jobs list and status, service status | a resource meter item; a process table component |
| Debrief | `performance_memory` (caps, use, peak per capped process), the store pane (worker memory), plugin detail memory cap, `Builtins::warm_up` (all at launch) | CPU, pid, state in one place; lazy start; idle stop |

Debrief still runs its own `debrief-ipc` / `debrief-plugin-host` stack; adopting `paddock-plugin-host` (X51) is the
path by which it gets the shared pieces below.

### Design

**Shared (Paddock):**

1. `paddock-proc::ProcessProbe` (port) and `SystemProbe` (sysinfo; PSS on Linux): `sample(pids) -> Vec<ProcessSample>`
   with `{ pid, memory, cpu_permille, started_at }`, CPU from deltas of two samples on the injected `Clock`. A fake for
   tests. One refresh for all pids per tick, not one per watcher thread (today every capped child has its own 200 ms
   thread).
2. `paddock-proc::ProcessTable`: the app's children by role (`plugin:<id>`, `plugin:<id> (elevated)`, `worker:store`,
   `worker:ai`, `browser`, ...), each with its cap, last sample, peak, state; fed by the one sampler; snapshot as a
   versioned camelCase wire type with generated TypeScript (like `paddock-jobs`).
3. `paddock-plugin-host`:
   - `Lifecycle { start: OnFirstUse | AtStartup, idle_stop: Option<Duration> }` per plugin, from Settings (default:
     on first use, stop after 10 minutes idle). A plugin with a running task, an open output stream, a live service or
     a declared `keep_alive` (e.g. the live event-log follower, dump captures) is never idle.
   - Status gains `pid`, `since`, `last_used`, `calls_in_flight`, and `Idle` (process up, nothing running) vs `Busy`.
   - `Failure::OverMemory { limit, used }`: ends the call with the cap message, no quarantine (section 2).
   - **Declarative sniffing**: an importer declares `extensions` and `magic` (offset + bytes, e.g. `ElfFile\0` at 0) in its
     manifest. The host matches them without starting the process and only calls `sniff` on candidates (or on importers
     that declare `sniff = "dynamic"`). Without this, lazy start buys nothing: opening any file sniffs with every
     importer plugin.
4. `paddock-metrics`: per-role series in the `TieredStore` (memory, CPU), for sparklines and "peak today".
5. `paddock-ui`:
   - `pd-resource-meter` status item (via `defineStatusItem`): CPU % and memory of the app with its children, and free
     space of the data folder's volume, each with a threshold colour; compact form is an icon with the numbers in the
     tooltip; click opens the Processes panel.
   - `pd-process-table` component and a **Processes** panel (a modeless tool window or dock panel, rule 17): name, kind,
     pid, state, CPU, memory against the cap (bar), peak, uptime, last use; row actions on hover: Stop, Restart, Raise
     limit…, Open log. Container-query layout, middle truncation (rule 18). Data by command + event from the engine
     (rule 34); state in a `*.store.ts` with `withRequestStatus` (rule 30).

**Debrief wiring:**

- Replace `Builtins::warm_up` (all 12 at launch) with the lifecycle policy; keep warming the plugin of an importer the
  current project is known to need (from the project's sources) after first paint.
- Feed `performance_memory` and the store pane from the process table (one source), and contribute the meter and the
  panel to the shell; the Plugins table shows state, pid, memory and CPU columns from the same snapshot.
- Settings ▸ Performance gains "Start plugins: on first use / at startup" and "Stop idle plugins after N minutes"
  (rule 21: tunables are Settings).

## Work packages (ranked)

| WP | What | Owner | Size | Why first |
| --- | --- | --- | --- | --- |
| **WP-1** | Streaming binary import: `import_stream` + `HostFile: Read + Seek` in `debrief-plugin-api`; EVTX importer on `from_read_seek`, events per chunk; liveness deadline instead of a whole-import deadline. Unignore `a_256_mb_event_log_imports_within_the_default_plugin_cap`. Port to `paddock-plugin-api`'s host files at the same time. | Debrief + Paddock | M | Fixes the reported crash for every size |
| **WP-2** | Cap stop is not a crash: map `Died` through `exit_text()` in `debrief-plugin-host` (`call`, `call_lending`) and add `OverMemory` to the shared host; no quarantine; message with Raise action. Unignore `a_plugin_stopped_at_its_cap_says_so_instead_of_crashed`; fix `evtx_corpus::check`. | Debrief + Paddock | S | Honest message now, before WP-1 lands |
| WP-3 | Host-side streaming: items straight into the batcher; lend the file from disk. Test: app memory for the 256 MB log stays within the import budget. | Debrief | M | App memory (ADR 0014 budget) |
| WP-4 | Cap policy: ceiling from physical memory, per-plugin "No cap" with the machine-floor guard (`paddock-proc::MemoryPolicy`), Settings and Plugins UI. | Paddock + Debrief | S-M | User request 2 |
| WP-5 | Process table and probe (`paddock-proc`), lifecycle + idle stop + declarative sniffing (`paddock-plugin-host`), metrics series, `pd-resource-meter` and Processes panel (`paddock-ui`); Debrief wiring and removal of `warm_up`-all. | Paddock + Debrief | L | User request 4; −125 MB idle |
| WP-6 | Benchmark: "import memory per MB" and "idle plugin memory" budgets in the nightly benchmark (rule 16), Linux PSS and macOS; conformance check "importer memory does not grow with input" in `paddock-plugin-test`. | Paddock | S | Keeps the fix fixed |
| WP-7 | Elevated processes use the watcher, not a job commit limit; watcher measures the tree (from `paddock-proc`). | Debrief (→ shared host) | S | Windows abort without message |
| WP-8 | Other whole-file importers (`ImportProfile`, `ImportDocument`, `Resymbolise`, minidump) to `import_stream` when touched; leaner records (F) only if a plugin still has to hold many events. | Debrief | M | Lower urgency |

Rule 40 applies: "the plugin holds the whole input" is a class, not an instance. WP-1 plus the WP-6 conformance check
make it a checked contract for every importer rather than a fix to the EVTX one.
