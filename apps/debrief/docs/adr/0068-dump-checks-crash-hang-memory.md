# 68. Dump checks: "Check crash", "Check hang", "Check memory", with .NET managed state

- Status: Proposed (2026-11-07), revision 2. Revision 2 answers the independent review of `a92474647` (four P1s: range
  reads after open, the unwind route, two-dump growth verdicts, the .NET adapter; the factual slips; the P2s). Design only:
  no product code, no builds or test runs for this ADR.
- Date: 2026-11-07
- Related: ADR 0023 (dumps and UMDH: the reader plugin, `debrief-dump`, problem-first findings), ADR 0036 (symbols by PDB
  identity), ADR 0017 (out-of-process plugins, sandbox, lent files), ADR 0019 (problem-first findings, "Explain with AI"),
  ADR 0059 (Pitwall as the AI engine), ADR 0064 (one streaming import trait; `WholeReason::DumpWithoutMemoryList`, `ByPath`;
  on `origin/docs/debrief-streaming-trait`), ADR 0065 (every bounded reply reports what it left out) and ADR 0066 (how
  Debrief reads times), both on `origin/docs/debrief-bounded-and-timezone`, ADR 0067 (folder import dialog; work package
  FM, the whole-file gate and `importers/memory.toml`; on `origin/docs/debrief-folder-import-dialog`), ADR 0069 (UMDH leak
  analysis over a series, reuses this ADR's `CheckReport`; on `origin/docs/debrief-analysis-usefulness`), family ADR 0033
  (processes by identity), family ADR 0051 (`paddock-tools`, on `origin/docs/paddock-tools-design-v2`), family ADR 0052
  (`NextStep`, `Why`, job outcomes in `paddock-jobs`; read on `origin/docs/adr-wiring-outcomes-ui`). Paddock rules 10 to 14
  (long operations), 21 (Settings and TOML packs), 46 (no LLM for large jobs), 48 (benchmarks recorded), 51 (parallelism and
  cost), 52 (thread priority), 53 (reusable OS names), 55 (one implementation per mechanism), 71 (release checks count only
  if the probe can see); ER-12, ER-14, ER-17 to ER-20, ER-27, ER-33. Memory decision of 2026-11-06 "no backward
  compatibility while unstable".
- Number: 0068. Every remote branch was listed with `git ls-tree --name-only <ref> apps/debrief/docs/adr/` on
  2026-11-07; the highest number on any branch was 0067; none held 0068 (0069 has since appeared on
  `origin/docs/debrief-analysis-usefulness`).
- Base: `origin/debrief/1.2-int` at `852b35ac0`. Every `file:line` below is at that commit unless a branch or crate
  version is named. rust-minidump sources were read in the local Cargo registry copies of `minidump-0.27.0` and
  `minidump-unwind-0.27.0`, the versions the lockfile uses.

Evidence tags: **[S]** read in the source; **[D]** read in official documentation on 2026-11-07, URL given; **[R]** read in
another ADR or a secondary source; **[U]** not verified, to be confirmed in the work package named; **[J]** judgement;
**[calc]** arithmetic from the numbers cited.

## Summary

The user finds the dump view too basic and wants buttons that answer a question: did it crash and why, is it hung and on
what, is it leaking. This ADR adds three engine operations behind three buttons, **Check crash**, **Check hang** and
**Check memory**. Each produces one **check report** computed in Rust (`debrief-dump`): a verdict, a confidence with its
reasons, evidence that links to threads, frames, modules and memory regions, the log events around the dump's time, and an
explicit list of **what it could not check** with this dump and what to capture instead. Hang and memory accept two or
more dumps of the same process (pid plus process start time plus main module identity, rule 53). Two dumps can show that
something grew; only three or more can show that it keeps growing.

Two foundations the checks need are designed here as their own pieces: **range reads after open** (section 7: the reader
fetches small pieces of dump memory on demand, over a file handle the host holds) and **Debrief's own unwinder** (section
8: framehop fed with PE unwind data from bytes, through `minidump-unwind`'s `walk_frame` seam, because the crate's own
debug-info route opens the module paths recorded in the dump).

.NET managed state comes in 1.2 through **a small, schema-versioned ClrMD helper that emits JSON**, built by us,
self-contained, pinned and installed through `paddock-tools` as an external tool and never linked into the app (section
9). Scraping `dotnet-dump`'s text output was the first revision's choice; it is a stop-gap and is rejected. The runtime's
data access component (DAC) is fetched from Microsoft's symbol server by its PE identity under the same switch as symbols;
**with that switch off and no matching runtime on the machine, .NET checks will mostly report the managed part as not
checked**.

The ADR also lists every analysis a minidump can support ([Catalogue of analyses](#catalogue-of-analyses)) with what each
needs, its value, its size and a recommendation, and what a minidump cannot give at all.

Work packages total **93 days raw** (judgement-sized, [J]); open decisions D1 to D13 are at the end.

## Context

### What Debrief does with a dump today

Read in the source at `852b35ac0` [S]:

- **Two crates and two plugins (ADR 0023).** `crates/debrief-dump` is the pure engine: model (`model.rs`), wire decode
  checked against `Limits` (`wire.rs`, `limits.rs`), per-window documents (`store.rs`, `DumpDocs`), views (`views.rs`),
  findings (`findings.rs`), exception names (`exceptions.rs`), module classes and wait functions (`modules.rs`), the AI
  summary (`explain.rs`), UMDH (`umdh.rs`). `crates/debrief-plugin-dumps` holds the readers (`dump_reader.rs`,
  `umdh_reader.rs`, `source.rs`, `pdb_symbols.rs`) and the capture tool (`capture.rs`). The app links only the engine;
  the plugin runs out of process (`net.decryptology.dumps`: no network, no writes, low integrity, a 3 GiB memory cap,
  `plugin.rs:27` `READER_MEMORY = 3 << 30`).
- **Library.** rust-minidump: `minidump = "0.27"` and `minidump-unwind = "0.27"` (`debrief-plugin-dumps/Cargo.toml:18-19`),
  `pdb = "0.8"` (`:21`) for names. **A second minidump reader** exists in the Windows plugin's log importer
  (`debrief-plugin-windows/Cargo.toml:19`, `src/minidump_import.rs`, 514 lines: its own stack scan and exception table),
  the duplicate ADR 0023 §11 left for later.
- **Streams read** (`dump_reader.rs:91-268`): system info, misc info (pid, process create time), module list (base,
  size, path, version, CodeView debug file and id), thread list (contexts, stack memory), the exception stream (code,
  flags, address, thread, up to 15 parameters), memory (`get_memory`), thread names, unloaded modules (**names only**,
  `:250-259`), handle data (**counts by type only**, `:377-392`, behind a guard against a forged descriptor size,
  `:353-371`), memory info list (**one summary**, `:394-418`). Detected: the .NET runtime by module name (`:421-435`).
- **Parsed by the library but dropped by Debrief.** rust-minidump 0.27 already reads each handle descriptor's object
  information headers into `MinidumpHandleDescriptor::object_infos` (`minidump-0.27.0/src/minidump.rs:322`, filled at
  `:1853`); only the payloads (for example a mutant's owner) are not interpreted. Debrief keeps the type name only.
- **Not read**: the thread info list (stream 17), handle object payloads, individual memory regions, unloaded module ranges
  and times, comment streams, process VM counters (22), the token stream (19), the function table stream (13), PEB or TEB
  memory, heap memory of any kind.
- **Stack walks** (`dump_reader.rs:162-244`): `minidump_unwind::walk_stack` per thread with a `MultiSymbolProvider::new()`
  that holds **no provider** (`:172`), so the walker never has call frame information: frames come from the context, frame
  pointers or stack scanning, whatever ADR 0023 §4 says about "call frame information when symbols provide it". On x64
  Windows most frames past the first are `scan` candidates. Names come afterwards from PDBs in local symbol folders
  (`pdb_symbols.rs`, ADR 0036); missing PDBs may be downloaded from the user's symbol servers when the user turned that on
  (`app/src-tauri/src/symbols.rs:1-8`; off by default). When PDBs arrive the dump is **read again** (`dumps.rs:331-356`):
  that re-names frames, but since the walker never had CFI the walk itself cannot improve. Bounds (`limits.rs:33-47`):
  4,096 threads, 256 frames, 256 KiB of stack per walk, 4,096 modules, 64 handle types, 64 notes; 60 s for all walks
  (`dump_reader.rs:21`).
- **Memory use.** A dump with a full-memory list (`Memory64ListStream`) is read as its prefix (at most 512 MiB) plus each
  thread's stack (1 MiB per thread, 256 MiB in all), with the list rewritten (`source.rs:48-52`, `:142`). **Every other
  dump is read whole into the plugin's memory, up to 1 GiB, and refused above** (`source.rs:144-156`, `limits.rs:44`).
  A "minidump with heap" has no `Memory64List` [U: dbghelp may use a 64-bit list for large private memory; WP-2], so such a
  dump over 1 GiB cannot be opened today. ADR 0064 keeps this whole read as a declared exception
  (`WholeReason::DumpWithoutMemoryList`) [R]. The decoded model lives in the app process (`app/src-tauri/src/dumps.rs:31-33`,
  `DumpsSlot`), encoded at most 512 MiB (`limits.rs:45`). On the folder-import path (the Windows plugin's log importer in
  the store worker) three 296 MiB dumps in parallel took 1,923 MB of store-worker memory, **about 2.1 times their size**
  (ADR 0067 line 118, a lower bound from a reproduction) [R].
- **The file after open.** The reader gets the file only while the open job runs: the host opens it inside the job
  (`dumps.rs:284`) and serves the reader's range reads from that handle (`FileAt`, `dumps.rs:287-293`; ADR 0023 §3). The
  handle is dropped when the job ends, so after open neither the host nor the plugin holds the file, and nothing can read
  dump memory that the model did not keep.
- **Shell and jobs.** `dumps_open` is a Jobs job with byte progress and Cancel (`dumps.rs:238-358`); 600 s per reader call
  (`dumps.rs:24`); the queries run on `spawn_blocking` (`dumps.rs:177-183`). Symbol downloads are their own job
  (`dumps.rs:386-430`). Contract: `debrief_dump::api`, generated with ts-rs into `app/src/app/core/generated/dumps.ts`.
- **Findings and thread state today** (`api.rs:391-407`, `findings.rs`): crash cause, culprit, stack overflow, heap
  corruption, bad address, lock wait, no exception, .NET runtime, missing symbols, handles, address space, and the UMDH
  kinds. `api::ThreadState` is `Faulted`, `LockWait`, `Waiting` or `Running` (`api.rs:199-208`), set by
  `views::thread_state` **by function name** in the top frames (`views.rs:58-72`, `modules.rs:149-190`), so without system
  symbols every thread is "Running".
- **UI**: tabs Findings, Threads, Modules, Details (`dump-view.ts:14`, labels at `:228-231`); a timeline marker at the
  dump's time (`dump-marker-track.ts`); "Explain with AI" sends the engine's redacted summary. The capture dialog
  (`create-dump-view.ts:144-146`) offers Minidump, Minidump with heap, Full dump; flags in `capture.rs:31-47`: every kind asks
  for handle data, unloaded modules, full memory info and thread info (`0x4 | 0x20 | 0x800 | 0x1000`), heap adds data
  segments, private read-write and indirectly referenced memory, full adds full memory.

So the parts exist, but they answer "what is in this file", not "what is wrong". The gaps: no question to start from;
thread state that collapses without symbols; no lock owners; no CFI; no thread times; no region list; one dump at a time;
nothing about what the dump could not show; no managed state; no link to the log lines around it; and no way to read dump
memory after open.

### What the minidump format offers

From `MINIDUMP_STREAM_TYPE` and `MINIDUMP_TYPE` [D: https://learn.microsoft.com/en-us/windows/win32/api/minidumpapiset/ne-minidumpapiset-minidump_stream_type,
https://learn.microsoft.com/en-us/windows/win32/api/minidumpapiset/ne-minidumpapiset-minidump_type]:

| Stream (number) | Written when | What the checks use it for |
| --- | --- | --- |
| Exception (6) | the writer passed exception parameters (WER, ProcDump `-e`, a crash handler) | crash: code, flags, address, parameters, faulting thread, its context |
| Thread list (3) | always | every thread's context and stack memory |
| Thread info list (17) | `MiniDumpWithThreadInfo` (0x1000) | per thread: create and exit time, user and kernel time (100 ns units), start address, flags such as "the thread that called MiniDumpWriteDump" [D: https://learn.microsoft.com/en-us/windows/win32/api/minidumpapiset/ns-minidumpapiset-minidump_thread_info] |
| Thread names (24) | recent writers [U] | names for the UI thread and pools |
| Handle data (12) | `MiniDumpWithHandleData` (0x4) | handles by type; per-handle object information (mutant, thread, process, event, section, semaphore), "automatically gathered", content that "can vary by operating system and processor architecture" [D: https://learn.microsoft.com/en-us/windows/win32/api/minidumpapiset/ne-minidumpapiset-minidump_handle_object_information_type] |
| Function table (13) | dynamic function tables registered with `RtlAddFunctionTable` (JIT code, .NET) | unwind data for generated code that is in no module [D: same stream page; `RtlAddFunctionTable` in https://learn.microsoft.com/en-us/cpp/build/arm64-windows-abi-conventions] |
| Memory info list (16) | `MiniDumpWithFullMemoryInfo` (0x800) | every region: base, allocation base, size, state, protection, type |
| Memory list (5) / Memory64 list (9) | stacks always; data with 0x1, 0x40, 0x200; everything with 0x2 | the bytes behind stacks, locks, PEB, images, the managed heap |
| Module list (4) | always | modules with time stamp, size, version, CodeView id |
| Unloaded modules (14) | `MiniDumpWithUnloadedModules` (0x20) | a fault in a range that a DLL used to occupy |
| System info (7) | always | OS version and build, CPU architecture and count |
| Misc info (15) | modern writers | pid, process create time (`time_t`, seconds), process user and kernel seconds [D: https://learn.microsoft.com/en-us/windows/win32/api/minidumpapiset/ns-minidumpapiset-minidump_misc_info_2]; later versions add the time zone [U: WP-2] |
| Token (19) | `MiniDumpWithTokenInformation` (0x40000) | the process token: integrity level, elevation [D: flag text "Adds security token related data"; payload layout U, WP-2] |
| Process VM counters (22) | recent dbghelp [U] | working set, private usage, page faults [R: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Diagnostics/Debug/struct.MINIDUMP_PROCESS_VM_COUNTERS_2.html] |
| Comment (10, 11) | ProcDump `-dc`, other tools [D: https://learn.microsoft.com/en-us/sysinternals/downloads/procdump] | the writer's own note, shown as-is |

rust-minidump 0.27 has typed readers for most of these, including `MinidumpThreadInfoList` and `MinidumpHandleDataStream`
with the object information headers [D: https://docs.rs/minidump/latest/minidump/; S above]; the token, function table and
VM counter streams are parsed from raw bytes with the same size guards as `read_handles`.

## Decision

### 1. Three checks, one report type

The dump view opens on a **Checks** strip with three Material buttons (verb plus object): **Check crash**, **Check
hang**, **Check memory**. The Findings tab stays; Threads, Modules and Details stay as the evidence views the reports link
into. A check is an engine operation that returns one `CheckReport` (section 10). The UI only lays it out.

The engine picks the primary button (`suggested_check` in the overview): an exception stream that is not a debugger
break-in (section 2) makes Check crash primary; otherwise Check hang. Any check can run on any dump.

**Thread classes replace thread states.** `api::ThreadState` and `views::thread_state` are removed; the Threads tab, the
findings and the checks all use one `ThreadClass` computed once per document and symbol generation (section 3). No shim
(memory decision 2026-11-06).

**Cache.** A report is cached under a key of: each document's **file identity** (volume serial and file id on Windows,
device and inode elsewhere, size, modification time) plus a hash of the dump's header and stream directory (cheap, from the
prefix already read; never a hash of the whole file), the check kind, the ordered document set, the symbol generation and,
for .NET, the helper's pin. Pressing a button twice costs nothing; a changed file never matches.

Every report has the same parts:

1. **Verdict** in one sentence, from a closed set per check.
2. **Confidence**: `High`, `Medium` or `Low`, each with the reasons that set it, computed by rules in the engine.
3. **Evidence**: typed links (thread and frame, module, memory region, handle, managed type, log event).
4. **Not checked**: what this check would look at but could not, with a reason from a closed set (`StreamMissing(kind)`,
   `MemoryNotInDump`, `NoSymbols(modules)`, `NoUnwindInfo(modules)`, `ToolUnavailable`, `DacUnavailable`, `OverBudget`,
   `DifferentProcess`, `NeedsMoreDumps { have, need }`, `FileChanged`, `Unsupported(arch)`) and a `NextStep`. **When the
   data for a verdict is missing, the verdict is `CannotTell` with this list, not a weak verdict** (ER-33's spirit).
5. **Around the dump**: the log events around the dump's time (section 6).
6. **Left out**: what the bounds cut (ADR 0065's shape).

### 2. Check crash

Needs: exception stream; faulting thread's context and stack; module list; unloaded modules; symbols if any.

- **Verdict** one of: `Exception { code, name, plain_words, faulting_address, module, offset, access }`,
  `FastFail { subcode }`, `StackOverflow { recursion }`, `HeapCorruption`, `BreakpointOrAssert`, `DebuggerBreakIn`,
  `NoExceptionRecorded`. The code is decoded by the existing table (`exceptions.rs`). The faulting address is placed in a
  module, in an unloaded module's former range (WP-2 keeps ranges), or "outside every module".
- **Debugger break-in.** A breakpoint exception (0x80000003) on a thread whose stack starts at `DbgUiRemoteBreakin`, or on
  the thread the thread-info flags mark as the writer, is a tool attaching, not a crash: verdict `DebuggerBreakIn`, and the
  report points to Check hang. Recognising `DbgUiRemoteBreakin` needs `ntdll` names or exports [U: from PDB or the export
  table; WP-4].
- **First- and second-chance.** The format records neither. The report never says "unhandled": it says "the dump does not
  record whether the program would have handled this exception. Dumps written by WER on a crash, and by ProcDump `-e`
  without `1`, are taken when the exception is unhandled; ProcDump `-e 1` also writes on first chance." [D: ProcDump page;
  https://learn.microsoft.com/en-us/windows/win32/wer/collecting-user-mode-dumps]. Which tool wrote the dump is known only
  when its comment stream or file name says so, and the report says which.
- **`NoExceptionRecorded` is honest**: the dump was probably taken on demand; run Check hang, or capture with ProcDump `-e`
  or WER local dumps.
- **Confidence**: `High` when the culprit was reached by context, CFI or frame pointer; `Medium` when it is a scanned
  candidate or unnamed; `Low` when the faulting thread's stack memory is missing.
- **Not checked** examples: no symbols or unwind data for modules on the faulting stack; no heap memory for heap checks.

### 3. Check hang

Needs: thread list; thread info list; handle data with object payloads; stack memory; for critical-section owners, the
lock's memory through range reads (section 7); symbols or exports of the system modules.

**Thread classes**: `Running`, `Waiting { on }`, `WaitingOnLock { lock, owner }`, `Sleeping`, `MessageWait`, `Exited`,
`Unknown`, each with how it was found (`ByName`, `BySyscall` from the top frame's module and export, `ByRegister` for a wait
handle read from the syscall context). The syscall-stub rule keeps classes meaningful without PDBs [U: needs exports from
the image (full dump, range reads) or the binary or PDB from the symbol folders; WP-5].

**The UI thread** is the thread with a message-loop frame (`GetMessage`, `PeekMessage`, `MsgWaitForMultipleObjects`,
`DispatchMessage`, a `win32u` user call) in its stack; when none has one, the earliest-created thread with `Low` confidence.
`MessageWait` means **the thread's top is in a message-loop wait**: the window is pumping messages at that moment. It does
not prove the window is responsive: a modal loop nested inside a long operation (a message box, `DoDragDrop`, a COM
`CoWaitForMultipleHandles` call) also pumps. When the message wait has a user frame below an outer message loop frame
further down (a nested loop), the class is `MessageWait { nested: true }` and the report says "in a nested message loop
inside `X`", not "idle".

**Lock owners and deadlocks.** A wait-for graph is built only from edges the dump proves:

| Lock kind | Owner from | Needs |
| --- | --- | --- |
| Mutex (mutant) handle | the handle's object information payload (`MiniMutantInformation2`, owner thread) [D; payload layout per OS build: U, WP-5 tests on named builds] | handle data |
| Handle a thread waits on | the handle value in the syscall's argument register (x64 `r10`/`rcx` at the `syscall` instruction; x86 on the stack) [J; U: WP-5] | thread context |
| Critical section | `RTL_CRITICAL_SECTION.OwningThread` (public layout in `winnt.h`) at the address the waiting frame names, read by range (section 7) [U: which register or stack slot per build; WP-5] | lock memory: heap dump, full dump or data segments |
| SRW lock, condition variable, `WaitOnAddress` | **no owner is recorded** | (cannot) |
| .NET `Monitor` | the sync block table from the .NET helper | full or heap dump, the helper |

A **cycle** is `Deadlock { cycle }` with `High` confidence. Without owner edges the verdict is `BlockedIn { function,
threads }` with `Low` or `Medium` confidence and a Not checked item naming the missing data. Wait Chain Traversal is live
only (catalogue).

**One dump or a series.** One dump cannot tell a hang from slow progress. With two or more dumps of one process
(section 5) the check compares per thread id: the stack from the top down to the first user frame, user plus kernel time
(thread info), and the UI thread's class.

- `Hung { threads }` only for threads that matter: the UI thread, or threads blocked in a user frame (a wait called from
  user code), with the same stack and no CPU time gained. Pool workers parked in their idle wait are never "hung".
- `Spinning { thread, cpu_share }` when a thread's CPU time grew by at least 80% of the wall-clock gap. The gap comes from
  the dump header time stamps, which have one-second resolution, so the share is reported as a range (for a 5 s gap,
  ±20%) and a gap under 3 s gives no spin verdict [calc, J]; thread times are in 100 ns units and are exact.
- `SlowProgress` when stacks change and the UI thread returns to a non-nested message wait; `CannotTell` otherwise.

The UI recommends three dumps a few seconds apart (ProcDump `-n 3 -s 5` [D]) and Debrief's capture offers series
(section 5).

### 4. Check memory

Needs: memory info list; thread list; module list; handle data; process VM counters if present; a series for growth; the
managed heap through the .NET helper.

- **One dump: composition, not a verdict.** Committed memory split into images, mapped files, private memory with thread
  stacks carved out, and for .NET the managed heap; the 20 largest regions grouped by allocation base; reserved versus
  committed; free space and the largest free block; thread count; handles by type. Private memory is labelled "private
  (heaps and other allocations)" unless the heap list was readable [J]. The report lists **suspects**, never "leak".
- **Two dumps: `Grew { what, delta }` at most, confidence at most `Medium`, and always a Not checked item
  `NeedsMoreDumps { have: 2, need: 3 }`**: one step cannot tell a leak from a cache filling or a burst of work.
- **Three or more dumps: `Growing { what, rate }`** only when the item grew in **every** step, with the rate per minute;
  `GrewOnce` when one step grew and the rest are flat; `NoGrowth`; `CannotTell`. Items: categories, region groups (matched
  by allocation base, type and protection), handle types, thread count, module set, managed types.
- **Native per-allocation stacks are not in a dump**: the report points to UMDH (ADR 0069).

### 5. Two or more dumps: picking, the same-process check, capture

- **Picking.** Hang and memory reports show **Add dumps to compare…**. The engine sorts the other open dumps into "same
  process", "same program, another run", "other program"; only the first group gives series verdicts. Up to 16 dumps per
  check (`Limits.max_check_docs`).
- **Same process (rule 53, family ADR 0033)** only when pid, process create time (seconds) and the main module's path and
  CodeView identity are equal; the earliest thread create time (100 ns) is a second witness when both dumps have it. A
  missing create time gives `SameProcessUnconfirmed` and confidence at most `Low`. Every pairing decision is logged with its
  reason code (section 14).
- **Capture a series**: `create { pid, kind, folder, count, interval_s }` (2 to 5 dumps, 1 to 600 s), holding the process
  handle across the series (rule 53), files `<exe>.<pid>.<kind>.<UTC stamp>.<n>.dmp`, opened as one group. Presets with
  purpose: **For a crash** (ProcDump `-e` or WER local dumps), **For a hang** (Minidump with heap, 3 dumps 5 s apart),
  **For memory** (Full dump, 3 or more dumps minutes apart, or UMDH).
- **Capture flags (decision D9).** Added to every kind: `MiniDumpWithTokenInformation` (0x40000: integrity level and
  elevation) and `MiniDumpWithProcessThreadData` (0x100: PEB and TEBs, so the UI thread's TEB and the command line are
  readable later). Added to Minidump and Minidump with heap: `MiniDumpWithModuleHeaders` (0x80000: module header pages)
  [U: whether this carries `.pdata`; WP-11 measures]. **Not** added by default: `MiniDumpWithCodeSegs` (0x2000), which
  writes every module's code and makes a minidump many times larger [J; size measured in WP-11 before any number is quoted];
  unwind data comes from the symbol folders instead (section 8). WP-11 records each flag's size cost on invented processes
  in `performance.md` (rule 48).
- **Which dump to take.** Every Not checked item carries its `NextStep`; the overview shows "This dump holds: …; not: …"
  from `DumpCapabilities`.

### 6. The log events around the dump's time

Each report gets `EventsAround { at, window, counts_by_level, first_events, query }`: the dump's time (UTC seconds), a
window (default 2 minutes before and 10 s after; for a series, first to last dump plus 1 minute; a Setting), counts by
level, the first 50 events by severity, and the timeline query. Sources are restricted to the same machine when a host
field matches; otherwise the report says that clocks of different machines may differ. Times follow ADR 0066. The query is
an engine operation over the store, bounded by ADR 0065's reply shape.

### 7. Range reads after open (new mechanism)

The checks need dump memory the model does not keep: critical sections, the PEB and TEBs, image pages with `.pdata` and
`.xdata` in full dumps, export tables, managed objects for the helper's summaries. The model must not grow to hold it
(bounded, sent over IPC), so the reader gets **`read_ranges`**:

- **Contract.** A new reader operation in the dumps plugin, `DumpQuery { doc, kind, ranges: Vec<VaRange> }` →
  `DumpQueryReply { pieces, missing }`, where `kind` names what to decode (`CriticalSection`, `Peb`, `Teb`,
  `UnwindTables { module }`, `Exports { module }`, `Raw`). The plugin parses the bytes (untrusted parsing stays out of the
  app, ADR 0023 §2) and returns typed values; `Raw` is capped small and used only by tests.
- **VA to file offset.** At open the reader keeps a compact, sorted memory index in the model: for each descriptor of the
  memory list and the Memory64 list, `(va_start, len, file_offset)` (Memory64 offsets are the list's base RVA plus the
  running sum of sizes). At most 4 Mi entries, the existing `MAX_RANGES` (`source.rs`), about 96 MiB at 24 bytes an entry
  in the worst case [calc]; a dump with more is noted and its query coverage is partial. A query range that falls outside
  the index is `missing` with `MemoryNotInDump`, never an error.
- **The file.** The **host holds the file handle** it opened for `dumps_open` for the document's lifetime (rule 53: hold the
  name, do not re-open by path), with share modes that allow other readers. For a query, the host lends that handle by
  ranges to the plugin exactly as for an import (`Lent::ranges` over the host's `RangeRead`), for the query's duration only.
  If the handle could not be held (a session restored from a project, a file on a share that drops handles), the host
  re-opens by path and **re-verifies the identity** (file id, size, modification time, header and directory hash) against
  the values stored at open; a mismatch is `FileChanged` with "The dump file changed since it was opened; open it again."
- **Plugin lifetime.** Queries are stateless for correctness: each carries the document's memory index slice it needs, so a
  restarted or quarantined-then-released plugin answers the same. For speed the plugin keeps the parsed prefix of the last
  two documents (LRU, bounded by the prefix limit, 512 MiB each at most) [J]. A quarantined plugin makes every pending
  query `ToolUnavailable`, and the checks degrade to their no-memory verdicts.
- **Bounds** (new `Limits` fields): per query at most 4,096 ranges and 16 MiB; per check at most 64 queries and 256 MiB;
  unwind tables at most 16 MiB per module. Each bound that cuts reports in Left out.
- **Who calls it.** The engine plans the reads (pure: which addresses a waiting frame names), the shell runs them inside
  the check's job (phase "Reading memory from the dump", count of ranges), the engine interprets the typed replies.
- **Heap dumps by ranges (decision D5).** The same path removes the whole read of dumps without a Memory64 list (section
  12), so a heap dump over 1 GiB opens.

### 8. Native stack walking: Debrief's own unwinder

**Why not the crate's route.** `minidump-unwind` 0.27's `debuginfo-unwind` feature pulls framehop, `object` and `memmap2`
[D: https://docs.rs/crate/minidump-unwind/latest/features], but its loader `EffectiveDebugFile::for_module` takes the
module's **`code_file()` path recorded in the dump** (or its debug file) and `File::open`s it on the analysing machine,
choosing between them by the analysing host's `cfg!(all(windows, target_arch = "x86_64"))`
(`minidump-unwind-0.27.0/src/symbols/debuginfo.rs:410-470`) [S]. That cannot read images inside the dump or from a symbol
store by code identity, and it opens an untrusted path from the file. Not used.

**Decision.** A Debrief `SymbolProvider` implements `walk_frame` — the seam the walker calls for CFI
(`minidump-unwind-0.27.0/src/amd64.rs:26-44`, `args.symbol_provider.walk_frame(module, &mut stack_walker)`) [S] — and feeds
**framehop** (MIT or Apache-2.0; x86_64 and aarch64; "PE unwind info in `.pdata`, `.rdata` and `.xdata` (for Windows
x86_64)"; "still a work in progress. Its API is subject to change") [D: https://github.com/mstange/framehop] with unwind
data **from bytes**:

1. **Full dumps**: the module's image pages are in the dump; `.pdata`, `.xdata` and the section table are fetched with
   `read_ranges` (`UnwindTables { module }`) and given to framehop as an in-memory module.
2. **Binaries from the user's symbol folders**, found by code identity (PE time stamp and image size, the symbol-store
   layout `<name>/<TIMESTAMP><SIZE>/<name>`), never by name alone (ADR 0036's rule applied to binaries); read inside the
   sandbox's `ReadFolders` right.
3. **Symbol servers**, only when the user turned them on (D6): the host fetches the binary like a PDB today; the reader
   never gets network.
4. **Dynamic function tables** (stream 13) for generated code.

Per architecture: x64 Windows, the above, else scan (marked). x86 Windows: frame pointers plus FPO frame data from the PDB
through the same provider [U: the provider can answer `walk_frame` for x86 with FPO data; WP-3]. arm64 Windows: the frame
pointer chain is required by the ABI ("The frame pointer (x29) is required for compatibility with fast stack walking used
by ETW and other services") [D: https://learn.microsoft.com/en-us/cpp/build/arm64-windows-abi-conventions]; arm64 PE
unwind codes are not in framehop's list and are not needed in 1.2.

**Without unwind data or symbols** every frame says how it was found; scanned frames are labelled "candidate"; confidence
drops with a Not checked item naming the modules; thread classes fall back to the syscall-stub rule.

**Re-walk is new work.** Today a symbols download re-reads the dump and only renames frames. With this provider a re-read
after new binaries or PDBs re-walks the threads whose frames can improve (scanned frames in modules that now have unwind
data), in parallel across threads, and the cached reports of that document are dropped (symbol generation in the key).

### 9. .NET managed state (research folded in)

**What any option needs** [D]: the DAC of exactly the dump's runtime version and architecture (`mscordaccore.dll` for .NET,
`mscordacwks.dll` for .NET Framework); "This is a native DLL which is tied to the architecture of the dump you are
debugging" [D: https://github.com/microsoft/clrmd/blob/main/doc/FAQ.md]. ClrMD by default looks the DAC up on Microsoft's
symbol server on Windows and verifies its signature; on Linux and macOS it does not download; **the runtime's own folder is
not searched**: "Applications that deploy a DAC beside the runtime must pass its trusted path explicitly" to
`ClrInfo.CreateRuntime(dacPath)` [D: https://github.com/microsoft/clrmd/blob/main/doc/GettingStarted.md]. Heap walking needs
heap memory and can fail for a dump taken mid-GC (`CanWalkHeap`) [D: same page].

| Option | Licence | Platforms for a Windows dump | Bundling, offline | Bounds | Effort | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| A. `dotnet-dump analyze -c …`, text output parsed | MIT [D: https://github.com/dotnet/diagnostics/blob/main/LICENSE.TXT]; 10.0.750501, 2026-10-07 [D: https://www.nuget.org/packages/dotnet-dump] | Windows; other hosts not documented [U] | framework-dependent, "require a 3.1 or newer .NET Core runtime" [D: https://github.com/dotnet/diagnostics/blob/main/documentation/single-file-tools.md]; .NET Framework dumps not documented [U] | timeout and output cap only; text is not a contract | M | **No**: a stop-gap (scraping human output), against the user's "best foundation, no stop-gaps" |
| **B. Our ClrMD helper, JSON out** | ClrMD MIT [D: https://github.com/microsoft/clrmd/blob/main/LICENSE]; 4.2.751201, 2026-10-12, netstandard2.0 and net10.0 [D: https://www.nuget.org/packages/Microsoft.Diagnostics.Runtime] | Windows (a Windows dump needs the Windows DAC) [J] | self-contained single-file per RID, no runtime prerequisite [J; size U, WP-0]; .NET Framework through ClrMD [U, WP-0] | our caps enforced inside the helper; versioned JSON schema | L | **Recommended** |
| C. Load the DAC from Rust (`ICLRDataTarget`, `ISOSDacInterface`) | ours MIT; the DAC is the runtime's | Windows | none extra | ours | XL; no Rust crate found [R: web search 2026-11-07] | **No** for 1.2 |
| D. Rust reader of the runtime's contract descriptors (cDAC) | MIT | all | no DAC | ours | XL; status in released runtimes unconfirmed [R; U] | **Later**, watch |
| E. WinDbg/CDB with SOS | Microsoft terms, not permissive | Windows | not redistributable | — | — | **No** |

**Decision: B.** A C# helper, `debrief-clr-inspect`, in the repository under `tools/` (not in the app), built by CI as a
self-contained single-file executable for win-x64, win-x86 and win-arm64, published to the Gitea package registry, pinned
by hash in `paddock-tools`' catalogue (ADR 0051: pins of the build only), and installed on demand; it is **never linked**
into Debrief. This needs D3: an amendment to Debrief's "Languages" rule allowing a C# external tool outside the app.

- **Contract.** `debrief-clr-inspect --dump <path> --dac <path> --request <json>` writes one JSON document to stdout with
  `schema: 1` and sections `threads`, `exceptions`, `stacks`, `heapStats`, `syncBlocks`, `threadPool`, `finalizer`, each
  with `status: ok | failed | cut` and a reason. The schema is defined once as Rust types in `debrief-dump::managed`, and a
  JSON Schema is generated from them; the helper's tests validate its output against that schema, so the two sides cannot
  drift (one declared definition).
- **Bounds inside the helper**: managed types listed at most 10,000, frames 256 per thread, objects walked for heap
  statistics at most a Setting (default 200 million [J]), text clipped to `max_text`, a wall-clock budget passed in; on any
  bound the section says `cut`. Outside: a Windows job object memory limit (a Setting, default 4 GiB, Auto = half of
  physical memory up to 8 GiB [J]), an output cap of 64 MiB, Cancel kills the process through `paddock_proc` by held handle.
- **DAC acquisition (decision D4).** The engine reads the runtime module's identity (name, PE time stamp, image size) from
  the dump's module list. The host looks for the DAC: (1) a folder the user added to the .NET DAC folders setting, searched
  by that identity in symbol-store layout; (2) a matching locally installed runtime (`shared/Microsoft.NETCore.App/<version>/`
  or the .NET Framework folder), accepted only if its PE identity matches; (3) **Microsoft's public symbol server by that PE
  identity, only when the symbol-server switch is on (the same switch as D6)**, into Debrief's symbol cache. The found path
  is passed explicitly (`--dac`, ClrMD's `CreateRuntime(dacPath)`); the helper is started with ClrMD's own symbol lookups
  disabled, so it never reaches the network itself. **Offline, with the switch off and no matching runtime installed, the
  managed part of every check reports `DacUnavailable` with the next step "Turn on symbol servers, or add a folder with
  mscordaccore.dll for .NET <version>".** That will be the common case for dumps from other machines, and the report says
  so plainly.
- **The plugin and the file after import.** A new plugin `net.decryptology.dumps.dotnet` holds `RunPrograms` (the helper)
  and `ReadFolders` (the DAC folders, the symbol cache), no network. ADR 0064's `ByPath` grants a path only for an import;
  here the host grants the dump's path **per managed query**: it checks the document's held handle and file identity
  (section 7), then passes the canonical path to the plugin for that one call, and the plugin passes it to the helper. On
  Windows the helper opens it with share-read while the host's handle stays open. The grant ends with the call.
- **WP-0 exit criteria** (fixed): on invented dumps of .NET 8 and .NET 10 (x64, x86) and .NET Framework 4.8 (x64), mini,
  heap and full, the helper (1) returns schema-valid JSON for every section or a typed failure; (2) names the managed
  exception and its stack on the crash fixture; (3) finds the `Monitor` owner cycle on the deadlock fixture; (4) gives heap
  statistics on the leak fixture whose top type is the invented one; (5) stays within the job memory limit and records peak
  memory and time on a 1 GB heap in `performance.md`; (6) works with a DAC passed by path and no network. If (1) to (4) fail
  for .NET Framework, Framework support is dropped from 1.2 and the report says "managed state not supported for .NET
  Framework"; if they fail for .NET, the user decides again (no silent fallback to A).

What the managed state adds: crash, the managed exception (type, message clipped, stack, inner chain); hang, managed stacks,
`Monitor` owners into the wait-for graph, thread-pool starvation; memory, managed heap size, top types by bytes and count and
their growth across a series (the same two-versus-three rule as section 4); the finalizer queue length.

### 10. Contract between engine and UI

New types in `debrief_dump::api`, generated by ts-rs into `dumps.ts`. The wire version is bumped and old documents are
re-read from their files, with no migration, on the user's decision of 2026-11-06 ("no backward compatibility while
unstable").

```rust
pub enum CheckKind { Crash, Hang, Memory }      // ADR 0069 adds HeapGrowth (UMDH series), not "Leak"
pub struct CheckRequest { pub kind: CheckKind, pub docs: Vec<u32> /* 1..=max_check_docs; the engine orders them */ }
pub struct CheckReport {
    pub kind: CheckKind,
    pub docs: Vec<CheckedDoc>,            // id, dump time, process identity, capabilities, file identity
    pub pairing: Option<Pairing>,         // SameProcess | SameProcessUnconfirmed | DifferentProcess, each with a reason code
    pub verdict: Verdict,                 // CrashVerdict | HangVerdict | MemoryVerdict, closed enums
    pub confidence: Confidence,           // { level: High | Medium | Low, because: Vec<ConfidenceReason> }
    pub evidence: Vec<Evidence>,          // Thread{doc,id,frame}, Module, Region, Handle, ManagedType, LogEvent
    pub threads: Option<Vec<ThreadClassRow>>, // every thread, its class, how found, its owner edge
    pub memory: Option<MemoryComposition>,    // composition per doc and growth rows
    pub not_checked: Vec<NotChecked>,     // { item: CheckItem, why: MissingReason, next: Option<NextStep> }
    pub around: Option<EventsAround>,
    pub left_out: Vec<Omission>,          // ADR 0065
}
pub enum ThreadClass { Running, Waiting { on: WaitTarget }, WaitingOnLock { lock: LockRef, owner: Option<u32> },
    Sleeping, MessageWait { nested: bool }, Exited, Unknown }   // replaces api::ThreadState
pub struct DumpCapabilities { pub exception: bool, pub thread_info: bool, pub handle_objects: bool,
    pub memory_regions: bool, pub stack_memory: bool, pub heap_memory: bool, pub full_memory: bool,
    pub unloaded_ranges: bool, pub process_times: bool, pub token: bool, pub function_tables: bool,
    pub managed: ManagedAvailability }
```

**Name for ADR 0069.** ADR 0069 proposes `CheckKind::Leak` for its UMDH series check. This ADR asks for
**`CheckKind::HeapGrowth`**: a check is named after what it looks at, and "leak" is a verdict that a check may or may not
reach (section 4's rule: `Growing` only with three or more steps). ADR 0069's verdict names are its own.

**`NextStep` variants.** `NextStep` and `Why` are family ADR 0052's shared enums in `paddock-jobs` (read on
`origin/docs/adr-wiring-outcomes-ui`; not found in `crates/paddock-jobs` on `origin/main` or `origin/debrief/1.2-int` on
2026-11-07). The checks need new variants (`TakeDump { kind, count, interval }`, `AddSymbolFolder`, `TurnOnSymbolServers`,
`AddDacFolder`, `OpenAgain`, `RecordUmdh`). Adding them is a shared change (rules 39, 42, 43) in its own package, WP-18,
which depends on ADR 0052's crate change landing.

Commands (one line each in the command table): `dumps_check`, `dumps_check_result`, `dumps_pair_candidates`,
`dumps_events_around`. Every error is a `CmdError`.

### 11. Jobs: off the UI thread, phases, Cancel (ER-17 to ER-19)

A check is a Jobs job (`JobKind::Analysis`) linked to its dumps, even when it finishes in milliseconds. Phases, each with
its own scale: "Reading thread details" (threads), "Reading memory from the dump" (ranges), "Walking stacks" (threads, only
for a re-walk), "Finding lock owners" (waits examined), "Reading .NET state" (helper sections, a growing count), "Comparing
dumps" (pairs), "Finding log events" (indeterminate, then the count). Cancel stops range reads between queries, the walk
between threads and kills the helper by held handle (rule 53); a cancelled check caches nothing. The UI never times out a
check (rule 12); engine-side budgets end in `OverBudget` Not checked items.

### 12. Bounded resources and memory caps

- **Reader**: ADR 0023's bounds, plus memory regions at most 65,536 (summarised beyond), handle object records at most
  65,536, the memory index (section 7), the range-read caps (section 7), `max_check_docs` 16.
- **Heap dumps by ranges (D5).** `source.rs`'s prefix-plus-ranges path is extended to dumps whose memory is in the 32-bit
  `MemoryListStream`: keep the directory and metadata and the thread stacks, index the rest, read on demand. The 1 GiB
  refusal goes and ADR 0064's `WholeReason::DumpWithoutMemoryList` is dropped (an amendment to ADR 0064, WP-8).
- **The FM gate (ADR 0067).** The dumps reader's row in `importers/memory.toml` is measured on the shipping build after D5
  (rule 48); until then ADR 0067's rules apply (an `at_least` factor of 2.1 for the store worker side, costed at the worst
  factor until measured). The check runs in the app process over the bounded model.
- **The .NET helper**: section 9's limits; one helper process at a time by default (section 13).

### 13. Parallelism and cost (ER-20, rule 51)

| Step | Kind of work | Parallel? | Cheaper? |
| --- | --- | --- | --- |
| New streams (thread info, regions, handle objects, token, function tables) | small, in the prefix already read | no need | read once at open with the prefix |
| Memory index | linear over the memory lists | no | built once at open, kept in the model |
| Thread classes, wait-for graph, cycle search | CPU, at most 4,096 × 256 frames | no: about a million frames, one pass [calc] | cached per document and symbol generation |
| Range reads | small random reads | sequential per query | only addresses a waiting frame or module names; parsed prefix cached in the plugin |
| Stack walks with unwind data | CPU per thread | yes, across threads, cores − 1 and a Setting with "Auto"; deterministic result order | re-walk only improvable threads; unwind tables parsed once per module |
| PDB and binary loading | I/O plus parse | a few at once (SSD) | cached per identity (256 PDBs today) |
| .NET helper | one process per dump, memory-heavy | **one at a time by default**, a Setting (bounded by RAM) | one run per dump with all sections; results cached by the cache key of section 1 |
| Comparing dumps | CPU, small | no | hash maps keyed by thread id, allocation base, type name |
| Events around | store query | the store's own | one bounded query |

Priority (rule 52): live work, normal level on Windows, at most `QOS_CLASS_UTILITY` elsewhere; the helper starts at normal
priority. No numbers are quoted until WP-14 records them in `docs/performance.md` (rule 48).

### 14. Logs (ER-27)

Each check logs under the job id: phases with start, end and duration; per dump the kind, capabilities (stream numbers
present), thread and module counts; **the cache key's document set and whether the report came from the cache**; **each
pairing decision with its reason code** (`PidDiffers`, `CreateTimeDiffers`, `CreateTimeMissing`, `MainModuleDiffers`,
`Same`); range reads as queries, ranges, bytes and missing counts, and `FileChanged` with which identity field differed;
walk results as counts per trust kind and the budget if hit; symbols and binaries found, missing, downloaded as counts; DAC
lookup outcome per step (folder, local runtime, server, off); the helper's command with the dump path replaced by a hash of
its file name, its pin, exit code, duration, peak job memory, the JSON sections' statuses, and the last 20 lines of stderr;
the verdict kind, confidence level and every Not checked reason code. No user content by default: file names only, no
function names, no exception messages, no command line, no environment, no log text.

### 15. Explain with AI

"Explain with AI" on a report sends the report's facts only (verdict, confidence reasons, evidence labels, Not checked
items, event counts), redacted as `dumps_explain` is today, to Pitwall (ADR 0059). One small request per report (rule 46).
The AI never decides a verdict.

### 16. One minidump reader (rule 55)

The Windows plugin's log importer (`debrief-plugin-windows/src/minidump_import.rs`) is a second implementation of reading
a minidump (its own stack scan and exception table). Decision D10: in 1.2 it becomes a log importer over `debrief-dump`'s
model and `debrief-plugin-dumps`' reader (one reader, one exception table), and its own parsing code is deleted. Until that
lands it is registered in `docs/known-copies.toml` with a delete-by tag of `debrief-v1.2.0`, so fixes to dump reading in
WP-2 apply to both (rule 55).

## Catalogue of analyses

Everything a minidump (or a series) can support. Size: **S** up to 2 days, **M** 3 to 5, **L** over 5, raw [J]. "Today"
means already in Debrief at the base commit [S].

### Native, one dump

| Analysis | Needs (streams, dump type) | Value | Size | Rec. |
| --- | --- | --- | --- | --- |
| Crash triage: exception in plain words, faulting address and module, culprit frame | exception, thread list, modules; any type | the first question for any crash | S on top of today | **1.2** |
| Debugger break-in told apart from a crash | exception, `DbgUiRemoteBreakin` on the stack, thread info writer flag | avoids "crashed with a breakpoint" | S | **1.2** |
| Fault in an unloaded module's former range | unloaded modules with ranges | explains "crash in nowhere" | S | **1.2** |
| Fast-fail subcodes, stack buffer overrun | exception parameters | names a whole class (today partly) | S | **1.2** |
| Stack overflow and recursion | 0xC00000FD, faulting stack | today; better with CFI | S | **1.2** |
| Heap corruption signs | 0xC0000374; heap memory for more | code today; block inspection needs full memory and page heap | M | code **1.2**; inspection **later** |
| Nested exception record | the record's pointer plus memory (range reads) | rare chained faults | S | **later** |
| First- versus second-chance | not in the format | — | — | **cannot**; the report says what the writer implies |
| Thread classes without symbols (syscall stubs) | thread list; exports from image, binary or PDB | hang triage on unsymbolised dumps | M | **1.2** |
| UI thread, nested message loops | thread list; `user32`/`win32u` names or exports | "is the window hung and why" | M | **1.2** |
| Lock owners: mutex | handle object payloads | mutex deadlocks | M | **1.2** |
| Lock owners: critical section | lock memory by range reads | the most common native deadlock | M | **1.2** (heap and full dumps) |
| Lock owners: SRW lock, condition variable | not recorded | — | — | **cannot** |
| Deadlock cycle | owner edges | the hang verdict | S after owners | **1.2** |
| Wait chains as Wait Chain Traversal shows them | live API only | — | — | **cannot** |
| Busy thread in one dump | thread info times | a hint (absolute times) | S | **1.2** |
| Memory composition, largest regions, stacks, free space | memory info list, thread list | "where the memory is" | M | **1.2** |
| 32-bit address-space exhaustion and fragmentation | memory info list, 32-bit process | today; add the free-block histogram | S | **1.2** |
| Native heap by heap and size class | heap memory, NT heap metadata (undocumented) | native leaks | L | **later** (UMDH covers stacks) |
| Per-allocation stacks | not in a dump (UMDH, page heap `+ust`) | — | — | **cannot**; UMDH (ADR 0069) |
| Handles by type, high counts | handle data | handle leaks (today a count) | S | **1.2** |
| GDI and USER object counts | not in a dump (`GetGuiResources` is live) | — | — | **cannot** |
| Module inventory: version, time stamp, path, CodeView id | module list | "which build crashed" | S | **1.2** |
| Unexpected modules: injected hooks, antivirus, user-writable paths, duplicate versions | module list; a TOML pack of known injectors (rule 21) | third-party DLLs behind odd crashes | M | **1.2** (WP-16) |
| Unsigned modules | signatures live in the files | — | — | **cannot** from the dump; with binaries **later** |
| Process info: pid, start time, uptime | misc info | context | S | **1.2** |
| Integrity level, elevation | token stream (19), `MiniDumpWithTokenInformation` | "ran elevated or not" explains access errors | S | **1.2** (with D9) |
| Command line | PEB by range reads (heap, full, or `WithProcessThreadData`) | which instance and options | M | **later** (D11); user content, on demand |
| Environment variables | PEB | often secrets | M | **no** for 1.2 |
| System info: OS build, CPU, processors | system info | context; today | S | **1.2** |
| System uptime, boot time | not in a minidump | — | — | **cannot** |
| 32-bit process dumped by a 64-bit tool (WOW64) | `wow64*.dll`, x64 context | stacks show only the WOW64 layer | S detect, L walk | detect **1.2**; walk **later** |
| Generated code unwinding (JIT) | function table stream (13) | stacks through JIT frames | M | **1.2** with WP-3 |
| The writer's comment stream | comment streams | the tool's note | S | **1.2** (shown, not logged) |
| Crashpad and Breakpad minidumps from other platforms | rust-minidump reads them [D] | Electron and Chromium-based apps | M | **later** |
| Kernel dumps, Linux core files | another format | — | — | **no** |

### Native, two or more dumps

| Analysis | Needs | Value | Size | Rec. |
| --- | --- | --- | --- | --- |
| Same-process check | pid, create time, main module identity | precondition of every series verdict (rule 53) | S | **1.2** |
| Hang versus slow progress | thread list in each | the hang verdict | M | **1.2** |
| CPU spin | thread info in each, gap ≥ 3 s | names the busy thread | S | **1.2** |
| Memory growth (2 dumps: grew; 3+: growing) | memory info list in each | the leak verdict | M | **1.2** |
| Handle and thread count growth | handle data, thread info | handle and thread leaks | S | **1.2** |
| Modules loaded or unloaded between dumps | module lists | plugin churn | S | **1.2** |
| Comparing against a known-good dump | modules, system info | "what differs on the broken machine" | M | **later** |
| Crash grouping by signature across many dumps | exception and stacks of each | "how many distinct crashes are these 40 dumps" | M | **later** (D12) |

### .NET (through the helper, section 9)

| Analysis | Needs | Value | Size | Rec. |
| --- | --- | --- | --- | --- |
| Managed exception: type, message, stack, inner chain | the exception object's memory; DAC | the .NET crash answer | M | **1.2** |
| Managed stacks per thread | stacks; DAC | names "code without names" | M | **1.2** |
| Interleaving managed and native frames | both stacks | one readable stack | M | **later** |
| Heap by type | heap or full dump | composition, suspects | S | **1.2** |
| Heap growth by type (2: grew; 3+: growing) | heap or full dumps of one process | the managed leak verdict | S | **1.2** |
| `Monitor` lock owners | heap or full dump | managed deadlocks | S | **1.2** |
| Thread-pool starvation | heap or full dump | a common .NET "hang" | S | **1.2** |
| Finalizer queue and finalizer thread | heap or full dump | a classic hidden leak | S | **1.2** |
| GC roots for a type | full dump, expensive per object | why a type is kept | M | **later** |
| Duplicate strings | full heap walk | memory savings | M | **later** |
| Async state machines | full dump | stuck awaits | M | **later** |
| Out-of-memory details, heap verification | full dump | OOM and managed corruption | S each | **later** |

### Across all checks

| Analysis | Needs | Value | Size | Rec. |
| --- | --- | --- | --- | --- |
| Events around the dump | header time; the store | ties the dump to the logs | M | **1.2** |
| Symbols and binaries: offline folders, PDBs found beside the dump | identity (ADR 0036) | names and unwind data without network | S | **1.2** (offer the folder a dump came from) |
| Unwind data for x64 | images by range reads, binaries by code id | trustworthy stacks | L | **1.2** |
| Export a report as report blocks with citations | report evidence ids | sharing | M | **later** (1.3) |
| AI explanation over computed facts | the report | readable summary | S | **1.2** |

**What a minidump cannot give**: first- or second-chance; owners of SRW locks and condition variables; live wait chains;
GDI and USER object counts; per-allocation native heap stacks; module signatures; system uptime; anything before the dump
(except an Intel PT trace, not supported); other processes; kernel stacks (ProcDump `-mk` writes a separate kernel dump
[D]); which window was hung.

## Consequences

- The user starts from a question and gets a verdict with its confidence and limits; a weak dump gives `CannotTell` and
  the dump to take next.
- Two new foundations (range reads, Debrief's own unwinder) also improve today's findings and are what later analyses
  (command line, heap inspection) build on.
- .NET support adds a C# helper outside the app (D3), a CI job that builds and publishes it, and a DAC that is usually
  unavailable offline; macOS and Linux users get native checks and a Not checked item for managed state.
- Risks: (1) lock-owner reads rely on undocumented details (syscall registers, mutant payload layout per build, the
  critical-section address on the waiting stack), mitigated by confidence levels and tests on named Windows builds; (2)
  framehop is a work in progress [D], pinned, behind the provider; (3) the helper's schema and ClrMD versions move together
  through one pin; (4) holding file handles for open dumps keeps files from being deleted on Windows unless share-delete is
  allowed, so the host opens with share-delete and re-verifies identity before each lend; (5) time alignment across
  machines can mislead, and the report says when sources differ.

## Test plan (invented dumps only, ER-14)

No real user dump is ever used in tests, fixtures, docs or commits.

- **Synthetic minidumps in Rust.** Extend `debrief_plugin_dumps::synth` (`synth.rs:1-2`, follows `minidumpapiset.h`) with
  the thread info list, handle descriptors with mutant payloads, many memory regions, misc info with process times, token,
  function table and comment streams, memory ranges holding a critical section, a PEB and image pages with `.pdata`.
  rust-minidump's `minidump-synth` (MIT) [D: https://github.com/rust-minidump/rust-minidump] is not added; `synth.rs` is
  kept [J]. Series come from one builder with the same pid and create time; a "pid reused" pair differs in create time.
- **Engine tests** per check: each verdict, confidence rule and Not checked reason. **Properties**: (a) removing any stream
  never raises confidence and always adds its Not checked item (the ER-33 probe: a known deadlock dump becomes `CannotTell`
  without its handle stream); (b) **no series of two dumps ever yields `Growing`, and a series of three or more yields
  `Growing` only when every step grew** (proptest over random growth sequences); (c) a range read outside the memory index
  is `missing`, never an error; (d) a changed file identity always gives `FileChanged`. Proptest "never panics" over the
  new parsers and fuzz seeds in `fuzz_dump_minidump`; a fuzz target for the helper's JSON decoder.
- **Windows-only integration tests** (Windows runner and local VMs): invented Rust test programs started as children, each
  with one fault (access violation, stack overflow, two-mutex deadlock, two-critical-section deadlock, SRW wait, spinning
  thread, growing allocations and handles). **Fixtures advance on the test's command, never on time** (ER-12): each child
  reads a command pipe ("allocate the next step", "enter the deadlock", "start spinning") and acknowledges, and the test
  captures a dump after each acknowledgement, so series are deterministic. The spin test reads thread times from the dumps
  and asserts on the share range, not on wall-clock sleeps. Mutant payload layout tests run on named builds: the Windows
  CI runner's stable build, plus local VMs with Windows 10 22H2 and Windows 11 24H2 [J: the builds the VMs hold are listed
  in WP-14 when it runs]. Children end through `paddock_proc` by held handle.
- **.NET** (Windows runner, D8): invented console programs in `tools/debrief-clr-inspect/fixtures/` (managed exception,
  `Monitor` deadlock, thread-pool starvation, growing `List<T>`), command-driven like the native ones, built with the pinned
  .NET SDK, dumped with the capture tool; the helper's JSON is validated against the generated schema; the Rust decoder is
  tested on every OS from committed JSON outputs of those invented dumps (no dumps committed).
- **UI**: component tests from the mock backend (the mock never computes a verdict); one Playwright spec per check; the
  real-engine path through `debrief-plugins/tests/it/dumps.rs`.

## Work packages

Judgement-sized raw days [J]; firm estimates after WP-0 and the WP-3 spike report (memory note "estimate after design").

| WP | Scope | Files | Days | Depends |
| --- | --- | --- | --- | --- |
| WP-0 | .NET helper spike against the fixed exit criteria of section 9; record peak memory and time (rule 48) | `tools/debrief-clr-inspect` (spike), notes | 3 | — |
| WP-1 | Model, wire, limits, the `CheckReport` contract with ts-rs output, `ThreadClass` replacing `ThreadState` and `views::thread_state`, confidence and Not checked framework, memory index type, cache key | `debrief-dump/src/{model,api,limits,wire,views}.rs`, new `checks/` | 4 | — |
| WP-2 | Reader: thread info, handle object payloads, memory regions, unloaded ranges and times, misc info process times and time zone, token, function tables, comment, VM counters, memory index, `DumpCapabilities` | `debrief-plugin-dumps/src/dump_reader.rs`, `synth.rs` | 4 | WP-1 |
| WP-RR | Range reads after open: `DumpQuery` operation and typed decoders, held file handle and identity re-verification in the host, lending by ranges per query, plugin prefix LRU, caps, `FileChanged` | `debrief-plugin-dumps`, `debrief-plugin-api` (one declared variant), `app/src-tauri/src/dumps.rs` | 5 | WP-1, WP-2 |
| WP-8 | Heap dumps by ranges; ADR 0064 amendment | `source.rs`, ADR 0064 | 3 | WP-RR |
| WP-3 | Debrief's unwinder: `SymbolProvider::walk_frame` over framehop from bytes; images by range reads; binaries from symbol folders by code id; host fetch when servers are on; function tables; x86 FPO; parallel re-walk of improvable threads; spike first | `dump_reader.rs`, new `unwind.rs`, `pdb_symbols.rs`, `app/src-tauri/src/symbols.rs`, `debrief-symbols` | 12 | WP-RR, WP-8 |
| WP-4 | Check crash, break-in detection | `checks/crash.rs` | 3 | WP-1, WP-2 |
| WP-5 | Check hang, one dump: thread classes incl. syscall stubs and nested loops, UI thread, owner edges (mutant payloads, register handles, critical sections by range reads), cycle search | `checks/hang.rs` | 8 | WP-3, WP-RR, WP-8 |
| WP-6 | Series: same-process check with reason codes, picker groups, hang versus progress, spin with gap ranges, growth framework with the two-versus-three rule | `checks/series.rs`, `store.rs` | 4 | WP-4, WP-5 |
| WP-7 | Check memory: composition, suspects, growth rows | `checks/memory.rs` | 4 | WP-1, WP-6 |
| WP-9 | Events around the dump | `debrief-core` query, `checks/around.rs` | 3 | WP-1 |
| WP-10 | Shell: `dumps_check` job, phases, Cancel, cache, commands, logs per section 14 | `app/src-tauri/src/dumps.rs`, command table | 3 | WP-1, WP-RR |
| WP-11 | Capture: series, presets, flags of D9 with their size cost measured and recorded | `capture.rs`, `plugin.rs`, `create-dump-view.ts`, `docs/performance.md` | 3 | — |
| WP-12 | .NET: the helper (C#, ClrMD, schema 1, bounds), its CI build and Gitea package, the `paddock-tools` pin, the `dumps.dotnet` plugin, DAC acquisition, the per-query path grant, the Rust decoder and fuzz target, feeding the three checks | `tools/debrief-clr-inspect/`, new plugin crate, `debrief-dump/src/managed.rs`, `.gitea/` (integrator) | 12 | WP-0, WP-1, WP-RR, ADR 0051's crate |
| WP-13 | UI: Checks strip, report view, pair picker, capability line, Explain with AI on a report; Material first, every state designed | `app/src/app/dumps/` | 6 | WP-10 |
| WP-14 | Tests not inside a package: command-driven Windows fixtures, named-build mutant tests, the properties of the test plan, Playwright; first measurements in `performance.md` | tests, `docs/performance.md` | 8 | all |
| WP-15 | Docs: user guide (which dump for which question), features, ADR 0023 amendment (CFI statement, §11), the Languages rule amendment of D3, independent review of the report screens (rule 22) and of this ADR's revision | `docs/`, `apps/debrief/AGENTS.md` | 2 | WP-13 |
| WP-16 | Unexpected-modules pack: TOML pack of known injectors and hook DLLs (built-in plus user `packs/`, rule 21), its parser with limits, the finding | `debrief-dump`, `packs/` | 2 | WP-2 |
| WP-17 | One minidump reader: the Windows plugin's log importer over `debrief-dump`; delete its parser; `known-copies.toml` entry until then | `debrief-plugin-windows/src/minidump_import.rs`, `docs/known-copies.toml` | 3 | WP-2 |
| WP-18 | `NextStep` variants in `paddock-jobs` (shared change, all users updated) | `crates/paddock-jobs` (integrator) | 1 | ADR 0052's crate change |
| | **Total** | | **93** | |

Order: WP-0, WP-1 and WP-11 first; then WP-2, WP-RR, WP-8; WP-3 and the checks; WP-12 after WP-0. Without .NET (WP-0,
WP-12) the native checks are 78 days [calc].

## Open decisions for the user

| # | Question | Recommendation |
| --- | --- | --- |
| D1 | .NET adapter | **B: our ClrMD helper with a versioned JSON schema**, external and pinned; A (scraping `dotnet-dump`) rejected as a stop-gap |
| D2 | .NET in its own plugin or inside the dumps reader | **Its own plugin** (`RunPrograms`); the reader keeps no programs and no network |
| D3 | Allow a C# helper outside the app (amend Debrief's "Languages" rule for external tools built in `tools/`, never linked) | **Yes, now**, so WP-0 can start |
| D4 | DAC acquisition | **User DAC folders, then a matching local runtime by PE identity, then Microsoft's symbol server by PE identity under the symbol-server switch (same as D6)**; otherwise `DacUnavailable` |
| D5 | Read heap dumps by ranges (drop ADR 0064's whole-read exception and the 1 GiB refusal) | **Yes**, in 1.2 |
| D6 | Fetch module binaries (unwind data) and DACs from the user's symbol servers when servers are on | **Yes**, same switch as PDBs; nothing by default |
| D7 | Capture series in Debrief's capture tool | **Yes** |
| D8 | Install the pinned .NET SDK on the Windows CI runner for the helper and its invented fixtures | **Yes** |
| D9 | Capture flags | **Add `WithTokenInformation` and `WithProcessThreadData` to every kind, `WithModuleHeaders` to mini and heap; not `WithCodeSegs` by default**; WP-11 measures and records the size cost |
| D10 | Fold the Windows plugin's minidump reader into `debrief-dump` (rule 55) | **Yes, in 1.2**, registered as a copy until then |
| D11 | Command line from the PEB | **Later**: user content; range reads make it a small step |
| D12 | Crash grouping across many dumps | **Later (1.3)** |
| D13 | ADR 0069's check name | **`CheckKind::HeapGrowth`**, not `Leak` |
