# 19. Limits explain themselves

- Status: Accepted (2026-10-27, by the user)
- Date: 2026-10-27
- Related: rule 19 (every "X is not running" has a working Repair), rule 21 (tunables are Settings), rule 40; ADR 0001
  (plugin platform crates), ADR 0010 (notifications with cause keys and service health), ADR 0018 (bounded loading);
  AGENTS.md "Degrade, don't fail" and "Bounded resources"
- Root cause of the reported crash: investigated by the plugin research on branch `docs/research-plugins` (not pushed
  yet). This ADR does not depend on its answer: whichever limit it is, the user must be told which.

## Context

A Debrief user reports that a plugin "crashes" when importing Windows Event Viewer logs (`.evtx`). Reading the path
shows a resource limit that surfaces as a crash. Paths below are under `apps/debrief/crates/` unless stated.

- `.evtx` is imported by `EvtxImporter` (`debrief-plugin-windows/src/evtx_import.rs`), plugin
  `net.decryptology.windows`, which runs out of process (`debrief-plugins`), started by `debrief-plugin-host` inside
  the store worker (`debrief-store-worker/src/lib.rs:29`).
- Memory on that path: the worker reads the whole file (`debrief-core/src/ingest/source.rs:323-327`); the plugin reads
  it again into one buffer (`debrief-plugin-api/src/host_file.rs:94-110`); `EvtxParser::from_buffer(bytes.to_vec())`
  copies it a third time and every record is held as JSON and as a `Record` (`evtx_import.rs:57-67`); the worker
  collects every returned record (`debrief-plugin-host/src/remote.rs:145-170`). Several files import at once into
  **one** plugin process with **one** memory cap (Setting `pluginMemoryMb`, default 1 024 MB,
  `app/src-tauri/src/performance/section.rs`).
- When the cap is exceeded the memory watcher kills the process (`debrief-ipc/src/process.rs:226-266`). The cap is
  named only in the child's own log. `ChildProcess::exit_text()` (which says "used more than its memory cap of N MB")
  is used by the store client (`debrief-store-client/src/client.rs:509`) and the AI worker
  (`debrief-ai/src/worker/frames.rs:352`), **not** by the plugin host, so the import fails as
  `plugin "net.decryptology.windows" (import) crashed: plugin "…" closed the connection` (`remote.rs:93`).
- The store worker starts its plugins with no memory listener, so the "Raise the cap…" remedy that exists for the
  store worker and the AI worker (`app/src-tauri/src/performance/watch.rs:104-121`, `health/hub.rs:205-223`) never
  fires for imports.
- Each failed call counts as a crash and the guard quarantines a plugin after 3, with no time window
  (`debrief-core/src/guard.rs:38,263-269`), so one kill during three parallel imports can disable the Windows plugin.
- The file then shows in Home ▸ Needs attention as plain text with no action (`app/src/app/home/home-view.ts:102-110`;
  the TypeScript `UnrecognizedFile` has no `crashed` field, `app/src/app/core/api.types.ts:197`).

The other limits on the path behave the same way: the 108 s import deadline is a hard-coded 90 % of a 120 s budget
(`debrief-core/src/guard.rs:150-153`, `remote.rs:34-36`) and reads "did not finish within 108.0 s"; the 2 GiB
`MAX_IMPORT_BYTES` (`debrief-plugin-api/src/serve.rs:25`) is a generic `INTERNAL` error; a frame over the IPC limit
(`debrief-ipc/src/frame.rs:16-18`) is reported as "it sent an invalid response" (blaming the wrong side), and Debrief's
`IpcError` has no `TooLarge` (`debrief-ipc/src/error.rs:35-49`); annotations beyond 10 000 are dropped silently
(`debrief-plugin-api/src/serve.rs:195`); engine errors beyond 5 are dropped (`app/src-tauri/src/engine_errors.rs:13`).

Only one limit explains itself today: the store worker's memory cap (message names the cap, Health shows Down with
Retry and **Raise the cap…**). The shared crates already have the typed shape on other paths: `paddock-ipc`
`IpcError::TooLarge { len, max }` (`crates/paddock-ipc/src/error.rs`), `paddock-archive` `ArchiveError::Limit { what,
limit }`, `paddock-net` `NetError::TooLarge`, `paddock-view` `ViewError::TooLarge`, `paddock-updater`
`UpdateError::TooLarge`, and `paddock-health` `Cause { message, hint, code, raise_cap, settings }` with
`RepairAction` (ADR 0010). But the type is lost at the next layer (`ArchiveError::Limit` becomes
`JobError::Failed(String)`, `crates/paddock-archive/src/error.rs:81-88`), and `paddock-plugin-host` has the same
in-flight memory gap as Debrief (`crates/paddock-plugin-host/src/supervisor.rs:580-584`: "the connection ended").

**The pattern:** a limit is enforced in one layer, and the layers above turn it into a string, a crash or nothing.
The user sees "crashed", "failed" or a missing result, and cannot tell that a number in Settings would fix it.

## Inventory of enforced limits (survey 2026-10-27)

A pattern search finds about 1 760 named limit constants in Rust and 320 in TypeScript across the family; most are
input-validation lengths. The ones that stop or cut user work, which this ADR covers, are **62**
(numbered by group; a cell names the values that make up its numbers):

| # | Group | Limits (value; where) | Setting today | When hit today |
|---|---|---|---|---|
| 1-4 | Plugin process (Debrief) | memory cap 1 024 MB (64-4 096; `performance/mod.rs:80`, `debrief-plugin-api/src/sandbox.rs:14-16`); 80 % warning (`debrief-ipc/src/memory_watch.rs:9-11`); start window 15 s; handshake 10 s / 1 024 B (`debrief-ipc/src/link.rs:16-18`) | memory only | "closed the connection" / "exited while starting" |
| 5-11 | Plugin calls (Debrief) | import 120 s (call 108 s); sniff 5 s; tail 10 s; transform 250 ms; hook/event 5 s; annotate/service/lifecycle 10 s (`debrief-core/src/guard.rs:150-166`); non-guard calls 10 s (`debrief-plugin-host/src/remote.rs:40`) | no | "did not finish within …" |
| 12-14 | Plugin host (Debrief) | pump 120 s, elevated 3 600 s (`debrief-plugin-host/src/process.rs:37-39`); manifests and probe 30 s / 4 MiB (`builtins.rs:19-21`) | no | Timeout / generic |
| 15-19 | IPC (Debrief) | frame 16 MiB, hard 64 MiB (`debrief-ipc/src/frame.rs:16-18`); concurrent calls 64 (`peer.rs:96`); cancel grace 2 s; bootstrap 64 KiB (`child.rs:20`); host-file read 60 s per 1 MiB chunk (`debrief-plugin-api/src/host_file.rs:10`) | no | "invalid response" / BUSY / Protocol |
| 20-22 | Quarantine (Debrief) | guard: 3 failures, no window (`guard.rs:38`); supervisor: 3 in 600 s, back-off 0.5-8 s (`debrief-ipc/src/supervisor.rs:144-147`) | no | "disabled after repeated crashes" (with "Allow again") |
| 23-27 | Plugin API (Debrief) | `MAX_IMPORT_BYTES` 2 GiB (`serve.rs:25`); annotations 10 000 / view 100 000 (`annotate.rs:15-17`); outputs 8 GiB / 8, log lines 10 000 (`protocol.rs:46-58`); tool budget 30 s, args 1 MiB (`tools.rs:22-24`) | no | generic / **silent truncation** |
| 28-33 | Parsing (Debrief) | `user_regex` 4 096 B pattern, 8 MiB compiled (`user_regex.rs:14,16`); `MAX_LINE_LEN` 4 MiB (`text.rs:5`); event 8 MiB (`ingest/mod.rs:58`); text event 1 MiB / 10 000 lines (`importers/text.rs:17,19`); evtx message 1 000 chars (`evtx_import.rs:10`) | no | anyhow text / counted problem / silent cut |
| 34-35 | Archives (Debrief) | entry 1 GiB, total 4 GiB (`debrief-core/src/archive.rs:10,12`); no entry-count, ratio or depth limit | no | "archive expands to more than N MiB" |
| 36-41 | Store (Debrief) | worker cap 2 GiB (`debrief-store-client/src/client.rs:68`); DuckDB 192 MB / 128 MB low-memory (`debrief-store-duck/src/lib.rs:60,122`); import and query budgets 768 / 300 MB (`ports/inspect.rs:12,14`); store message 64 MiB (`protocol.rs:17`); call 120 s / long 3 600 s; SQL 10 000 rows / 32 MB / 30 s, export 5 M rows / 2 GiB / 600 s | cap, budgets, spill | **cap explains itself**; others DuckDB or Timeout text |
| 42-45 | Result caps (Debrief) | `MAX_HITS` 10 000, `MAX_FILTER_HITS` 2 M (`fts.rs:82-86`); `MAX_ANALYSED` 2 M (`analysis.rs:31`); `MAX_LANES` 500 (`read.rs:30`); `MAX_PREVIEWED` 200 000 | no | silent, or a "capped" flag the UI rarely shows (ADR 0018) |
| 46-50 | App (Debrief) | AI worker cap 1 024 MB (explains itself); paste 8 MiB (typed); downloads: updater 512 MiB, browser runtime 400 MiB; browser profile 500 MiB (Setting); jobs queue 20, engine errors reported 5 (`engine_errors.rs:13`) | some | typed / generic / **silent drop** |
| 51-53 | `paddock-ipc`, `paddock-plugin-host` | frame 16/64 MiB, typed `TooLarge` (`crates/paddock-ipc/src/frame.rs:18-22`); plugin memory 1 GiB, 64 MiB-4 GiB (`crates/paddock-plugin-host/src/grants.rs:145-149`); call 30 s, hang grace 5 s, start 15 s (`config.rs:57-65`) | config | typed / "the connection ended" / `Fault{Hang}` |
| 54 | `paddock-guard` | quarantine 3 in 600 s (`crates/paddock-guard/src/quarantine.rs:30-33`) | policy | "switched off" |
| 55 | `paddock-archive` | entries 10 000, file 4 GiB, total 8 GiB, ratio 200, depth 32, path 1 024 (`options.rs:22-33`) | caller | typed `Limit`, lost in `JobError` |
| 56-58 | `paddock-net`, `paddock-download`, `paddock-updater` | redirects 10, header limits; connections 16, per host 4; download 512 MiB, manifest 60 s / 1 MiB | no | typed |
| 59-60 | `paddock-view`, `paddock-diag` | nodes 20 000, 1 MiB encoded, 2 000 inline rows, 20 000 chart points; report 64 files / 16 MiB | no | typed `TooLarge` / generic |
| 61 | `paddock-storage` | walk 500 000 entries, 10 s, depth 64 (`walk.rs:17-21`) | no | generic |
| 62 | `paddock-ui` | IPC call timeout 15 s (`ui/projects/paddock-ui/ipc/src/call-timeout.ts:13`) | no | "did not answer" |

Pitwall, Pitcrew, Spotter and Dyno have their own limit constants (about 100-230 each by the same search); they are
inventoried by the registry in step 1 of the migration when each app adopts this ADR, not listed here.

Tests that trigger a limit and assert its message exist for the IPC frame, quarantine, guard timeouts, archive
limits, regex length, paste and the store worker's `exit_text`. There is none for the plugin memory kill as the user
sees it, the import deadline text, `MAX_IMPORT_BYTES` in Debrief, the frame-overflow wording, annotation truncation,
quarantine by concurrent failures, evtx message truncation or the dropped engine errors; and
`debrief-plugins/tests/it/evtx_corpus.rs:137` looks for "memory cap" in a text the plugin host never produces.

## Decision

1. **One typed limit error.** A shared `LimitExceeded { limit: LimitId, value: Quantity, observed: Option<Quantity>,
   scope: Scope /* which plugin, file, query */, setting: Option<SettingPath>, remedy: Remedy }` in a small shared crate
   (`paddock-limits`, or inside `paddock-health` next to `Cause`), with `LimitId` a stable key
   (`plugin.memory`, `plugin.call_deadline.import`, `ipc.frame`, `import.file_size`, `archive.total`, `query.hits`,
   …) and `Quantity` a value with a unit (bytes, seconds, count). Every enforced limit returns or carries it; every
   error type on the way up (`IpcError`, `HostError`, `JobError`, `UnrecognizedFile`, the Tauri edge) has a variant
   that **keeps it**, never a `String`.
2. **A process killed for a limit reports the limit.** The plugin host asks the child for its exit cause
   (`exit_text`, `MemoryEvent`) before building the call error, in both `debrief-plugin-host` and
   `paddock-plugin-host`, and the store worker's plugins get a memory listener like the app's. A call that failed
   because its process was killed for memory is a `LimitExceeded`, not a crash: it does **not** count towards
   quarantine, and calls that failed with it at the same moment count once.
3. **The message is generated from the type**, never hand-written per site: "*{scope}* stopped: it used *1 210 MB* of
   memory; the limit is *1 024 MB*. Raise it in Settings → Performance → Plugin memory." The parts: what stopped, the
   observed value (when known), the limit and its value, and — when the limit is a Setting — the exact Settings path;
   when it is a hard limit, what to do instead ("Split the file", "Narrow the query").
4. **The UI shows it with Repair and Retry.** One shared `LimitNotice` (in `paddock-ui`, rendered by the notification
   service of ADR 0010 with the limit's cause key, and inline in panels and the Home "Needs attention" list): **Raise
   it…** opens Settings at that field (or a one-click "Raise to 2 048 MB and retry" when the next step is safe), and
   **Retry** re-runs the failed work (re-import that file, re-run the query). Per-file import failures get both buttons.
5. **Limits that are tunables are Settings** (rule 21), declared with the limit registry: the Setting's key is
   `LimitId`, its min and max are the hard bounds. Hard limits that protect the process (frame hard maximum, regex
   compiled size, archive ratio) stay constants but still produce `LimitExceeded`.
6. **No silent truncation.** A cut result carries `Truncated` (ADR 0018); a dropped item increments a counter that the
   UI shows ("12 annotations not shown: the limit is 10 000").
7. **One registry of limits** per app plus the shared ones: `LimitId`, unit, default, hard min/max, Setting path or
   `hard`, message template, remedy. It is the source for the Settings fields, the messages and the test list below,
   and the Help page "Limits" is generated from it.

## The check

1. **One test per limit** (rule 5 style): for every entry of the registry, a test drives the code to the limit
   (a fake plugin that allocates past a 64 MB cap; a fake that sleeps past a short deadline injected through a fake
   clock, no wall-clock sleeps; a frame one byte over; an archive one entry over; a regex one byte over; a query over
   its maximum) and asserts the error is `LimitExceeded` with the right `LimitId`, value and Setting path, and that
   the rendered message contains the limit name, the value and "Settings →" when it is a Setting. The test harness
   (`limit_case!` in `paddock-testkit`) makes it one line per limit plus the trigger.
2. **Registry completeness, two directions** (conformance check): every `LimitId` in the registry has a test (the
   harness records the ids it exercised; a missing id fails); and a source scan for limit-shaped constants
   (`MAX_*`, `*_LIMIT`, `*_TIMEOUT`, `*_CAP`, `*_BUDGET` used in a comparison that returns an error, truncates or
   kills) that are not referenced from the registry warns, with a waiver list for input-validation lengths.
3. **The evtx regression first:** an evtx-shaped import through the real `debrief-plugin-host` and a fake plugin
   that exceeds a small memory cap must produce `LimitExceeded { limit: plugin.memory }`, a "Raise it" action, no
   quarantine, and a Retry that succeeds after the Setting is raised. Then the real `.evtx` corpus test asserts the
   same text instead of searching for one that never appears.
4. **UI**: a Playwright spec with the mock backend raising each `LimitId` kind once and asserting the notice shows the
   message, Raise it (opening the right Settings field) and Retry.

## Consequences

- "The plugin crashed" becomes "the plugin used more memory than its 1 024 MB limit; raise it", with a button. Users
  can fix their own problem; bug reports say which limit.
- Quarantine is reserved for real crashes; a limit kill no longer disables a plugin.
- Error types on every layer grow one variant; string errors on these paths go away, which also removes wrong
  wordings such as "invalid response" for an oversized frame.
- The 62 limits get a test each; most are short because the harness does the assertion.
- The registry makes a later review of defaults (with ADR 0018's measurements) a one-file change.

## Migration plan

1. Failing evtx regression test (decision 2), then the plugin host and worker listener fix in Debrief and
   `paddock-plugin-host`, and quarantine that ignores limit kills.
2. `LimitExceeded`, `LimitId`, the registry and `limit_case!`; Debrief's plugin, IPC, import and archive limits move
   first (rows 1-35), then store and results (36-50), then the shared crates (51-62).
3. `LimitNotice` with Raise it / Retry in `paddock-ui`; Home "Needs attention" uses it.
4. Conformance check in warning mode, waivers for validation lengths; error once Debrief is clean; each other app
   adopts with its own registry.
5. Root cause of the evtx crash confirmed by `docs/research-plugins`; whichever limit it is, it already explains
   itself after step 1. Streaming the evtx import (ADR 0018) removes the cause.

## Alternatives considered

- **Better strings at each site.** That is today's state for the store worker; it did not spread to the plugin path
  because nothing checks it.
- **Raise the defaults.** Moves the failure to bigger files, and the user still cannot tell which number to change.
- **Catch limit kills in the UI by matching text.** ADR 0010 replaced exactly that (Debrief's `StoreHealth`
  string-matching); types survive rewording and translation.
