# 68. Dump checks: "Check crash", "Check hang", "Check memory", with .NET managed state

- Status: Proposed (2026-11-07). Design only: no product code, no builds or test runs for this ADR.
- Date: 2026-11-07
- Related: ADR 0023 (dumps and UMDH: the reader plugin, `debrief-dump`, problem-first findings), ADR 0036 (symbols by PDB
  identity), ADR 0017 (out-of-process plugins, sandbox), ADR 0019 (problem-first findings, "Explain with AI"), ADR 0059
  (Pitwall as the AI engine), ADR 0064 (one streaming import trait; `WholeReason::DumpWithoutMemoryList`, on
  `origin/docs/debrief-streaming-trait`), ADR 0065 (every bounded reply reports what it left out, on
  `origin/docs/debrief-bounded-and-timezone`), ADR 0066 (how Debrief reads times, same branch), ADR 0067 (folder import
  dialog; work package FM, the whole-file gate and `importers/memory.toml`, on `origin/docs/debrief-folder-import-dialog`),
  family ADR 0033 (processes by identity), family ADR 0051 (`paddock-tools`, on `origin/docs/paddock-tools-design-v2`),
  family ADR 0052 (`NextStep`, `Why`, job outcomes). Paddock rules 10 to 14 (long operations), 21 (Settings and TOML
  packs), 46 (no LLM for large jobs), 48 (benchmarks recorded), 51 (parallelism and cost), 52 (thread priority), 53
  (reusable OS names), 71 (release checks count only if the probe can see); ER-14, ER-17 to ER-20, ER-27, ER-33.
- Number: 0068. Every remote branch was listed with `git ls-tree --name-only <ref> apps/debrief/docs/adr/` on
  2026-11-07; the highest number on any branch is 0067 (`origin/docs/debrief-folder-import-dialog`); none holds 0068.
- Base: `origin/debrief/1.2-int` at `852b35ac0`. Every `file:line` below is at that commit unless a branch is named.

Evidence tags: **[S]** read in the source at the base commit; **[D]** read in official documentation on 2026-11-07, URL
given; **[R]** read in another ADR or a secondary source; **[U]** not verified, to be confirmed in the work package named;
**[J]** judgement; **[calc]** arithmetic from the numbers cited.

## Summary

The user finds the dump view too basic and wants buttons that answer a question: did it crash and why, is it hung and on
what, is it leaking. This ADR adds three engine operations behind three buttons, **Check crash**, **Check hang** and
**Check memory**. Each produces one **check report** computed in Rust (`debrief-dump`): a verdict, a confidence with its
reasons, evidence that links to threads, frames, modules and memory regions, the log events around the dump's time, and an
explicit list of **what it could not check** with this dump and what to capture instead. Hang and memory accept two or
more dumps of the same process (checked by pid plus process start time, rule 53) and then give the verdicts one dump
cannot: a real hang versus slow progress, and what grew.

.NET managed state (stacks, exceptions, heap by type, Monitor owners, the finalizer queue, the thread pool) comes in 1.2
through a pinned external tool run by `paddock-tools` in its own sandboxed plugin, Windows only. The recommendation is
Microsoft's `dotnet-dump` (MIT) first, behind a port that a ClrMD-based helper can replace if the spike in WP-0 shows the
text output cannot be parsed reliably.

The ADR also lists every analysis a minidump can support ([Catalogue of analyses](#catalogue-of-analyses)) with what each
needs, its value, its size and a recommendation (1.2, later or no), and what a minidump cannot give at all.

Work packages total **63 days raw** (judgement-sized, [J]); open decisions D1 to D10 are at the end.

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
  `pdb = "0.8"` (`:21`) for names. The Windows plugin's older log importer also uses `minidump = "0.27"`
  (`debrief-plugin-windows/Cargo.toml:19`, `minidump_import.rs`), the duplicate ADR 0023 §11 left for later.
- **Streams read** (`dump_reader.rs:91-268`): system info, misc info (pid, process create time), module list (base,
  size, path, version, CodeView debug file and id), thread list (contexts, stack memory), the exception stream (code,
  flags, address, thread, up to 15 parameters), memory (`get_memory`), thread names, unloaded modules (**names only**,
  `:250-259`), handle data (**counts by type only**, `:377-392`, with a guard against a forged descriptor size,
  `:353-371`), memory info list (**one summary**: committed, image, mapped, private, reserved, free, largest free,
  `:394-418`). Detected: the .NET runtime by module name (`:421-435`).
- **Not read**: the thread info list (stream 17: thread create time, user and kernel time, start address, exit state),
  handle object information (mutant owners), the individual memory regions, unloaded module ranges and times, the comment
  streams, process VM counters (stream 22), the token stream, PEB or TEB memory (command line), heap memory of any kind.
- **Stack walks** (`dump_reader.rs:162-244`): `minidump_unwind::walk_stack` per thread with a `MultiSymbolProvider::new()`
  that holds **no provider** (`:172`). So no call frame information is ever available to the walker: frames come from the
  context, frame pointers or stack scanning, whatever ADR 0023 §4 says about "call frame information when symbols provide
  it". On x64 Windows, where code is built without frame pointers by default (ADR 0023, Consequences), most frames past the
  first are `scan` candidates. Names come afterwards from PDBs in local symbol folders (`pdb_symbols.rs`, ADR 0036), and
  missing PDBs may be downloaded from the user's symbol servers when the user has turned that on (`app/src-tauri/src/symbols.rs:1-8`;
  off by default). Bounds (`limits.rs:33-47`): 4,096 threads, 256 frames, 256 KiB of stack per walk, 4,096 modules, 64
  handle types, 64 notes; a 60 s budget for all walks (`dump_reader.rs:21`).
- **Memory use.** A dump with a full-memory list (`Memory64ListStream`) is read as its prefix (at most 512 MiB) plus each
  thread's stack (1 MiB per thread, 256 MiB in all), with the list rewritten (`source.rs:45-52`, `:141-157`). **Every other
  dump is read whole into the plugin's memory, up to 1 GiB, and refused above** (`source.rs:142-156`,
  `limits.rs:44` `max_read_bytes: 1 << 30`). A "minidump with heap" (`MiniDumpWithPrivateReadWriteMemory`) has no
  `Memory64List` [U: dbghelp may switch to a 64-bit list for large private memory; to confirm in WP-1], so a heap dump over
  1 GiB cannot be opened today. ADR 0064 keeps this whole read as a declared exception (`WholeReason::DumpWithoutMemoryList`,
  line 142 of that ADR) [R]. The decoded model then lives in the app process (`app/src-tauri/src/dumps.rs:30-33`,
  `DumpsSlot`), encoded at most 512 MiB (`limits.rs:45`). On the folder-import path (the Windows plugin's log importer in the
  store worker), three 296 MiB dumps in parallel took 1,923 MB of store-worker memory, **about 2.1 times their size**
  (ADR 0067 line 118, a lower bound from a reproduction, not a shipping-build measurement) [R].
- **Shell and jobs.** `dumps_open` is a Jobs job with byte progress and Cancel (`dumps.rs:238-358`); a 600 s budget per
  reader call (`dumps.rs:23`); the queries (`dumps_overview`, `dumps_threads`, `dumps_stack`, `dumps_modules`,
  `dumps_findings`, `dumps_markers`, `dumps_explain`) run on `spawn_blocking` (`dumps.rs:177-183`). Symbol downloads are
  their own job (`dumps.rs:386-430`). Contract: `debrief_dump::api`, generated with ts-rs into
  `app/src/app/core/generated/dumps.ts` (ADR 0023 §10).
- **Findings today** (`api.rs:391-407`, `findings.rs`): crash cause, culprit, stack overflow, heap corruption, bad address,
  lock wait, no exception, .NET runtime, missing symbols, handles, address space, and the UMDH kinds. Thread state is one of
  `Faulted`, `LockWait`, `Waiting`, `Running` (`api.rs:199-208`), decided **by function name** within the top frames
  (`views.rs:58-72`, `modules.rs:149-190`), so without system symbols every thread is "Running".
- **UI** (`app/src/app/dumps/dump-view.ts:14`): tabs Findings, Threads, Modules, Details; the timeline gets a marker at
  the dump's time (`dump-marker-track.ts`); "Explain with AI" sends the engine's redacted summary. The capture dialog
  (`create-dump-view.ts:144-146`) offers Minidump, Minidump with heap, Full dump, with flags in `capture.rs:31-47`: every
  kind already asks for handle data, unloaded modules, full memory info and thread info (`0x4 | 0x20 | 0x800 | 0x1000`),
  heap adds data segments, private read-write and indirectly referenced memory, full adds full memory.

So the parts exist, but they answer "what is in this file", not "what is wrong". The gaps this ADR closes: no question
to start from; thread state that collapses without symbols; no lock owners, so no deadlock beyond a hint; no CFI, so x64
stacks are guesses; no thread times, so no busy thread and no CPU spin; no region list, so no memory composition; one dump
at a time; nothing about what the dump could not show; no managed state; no link from the dump to the log lines around it.

### What the minidump format offers

From `MINIDUMP_STREAM_TYPE` and `MINIDUMP_TYPE` [D: https://learn.microsoft.com/en-us/windows/win32/api/minidumpapiset/ne-minidumpapiset-minidump_stream_type,
https://learn.microsoft.com/en-us/windows/win32/api/minidumpapiset/ne-minidumpapiset-minidump_type]:

| Stream (number) | Written when | What the checks use it for |
| --- | --- | --- |
| Exception (6) | the writer passed exception parameters (WER, ProcDump `-e`, a crash handler) | crash: code, flags, address, parameters, faulting thread, its context |
| Thread list (3) | always | every thread's context and stack memory |
| Thread info list (17) | `MiniDumpWithThreadInfo` (0x1000) | per thread: create and exit time, user and kernel time (100 ns units), start address, flags such as "the thread that wrote the dump" [D: https://learn.microsoft.com/en-us/windows/win32/api/minidumpapiset/ns-minidumpapiset-minidump_thread_info] |
| Thread names (24) | Windows 10 1607 and later writers [U] | names for the UI thread and pools |
| Handle data (12) | `MiniDumpWithHandleData` (0x4) | handles by type; per-handle object information such as mutant (mutex) and thread details, "gathered automatically", layout "can vary by operating system" [D: https://learn.microsoft.com/en-us/windows/win32/api/minidumpapiset/ne-minidumpapiset-minidump_handle_object_information_type] |
| Memory info list (16) | `MiniDumpWithFullMemoryInfo` (0x800) | every region: base, allocation base, size, state, protection, type |
| Memory list (5) / Memory64 list (9) | stacks always; data with 0x1, 0x40, 0x200; everything with 0x2 | the bytes behind stacks, locks, PEB, managed heap |
| Module list (4) | always | modules with time stamp, size, version, CodeView id |
| Unloaded modules (14) | `MiniDumpWithUnloadedModules` (0x20) | a fault in a range that a DLL used to occupy |
| System info (7) | always | OS version and build, CPU architecture and count |
| Misc info (15) | always for modern writers | pid, process create time (`time_t`, seconds), process user and kernel seconds [D: https://learn.microsoft.com/en-us/windows/win32/api/minidumpapiset/ns-minidumpapiset-minidump_misc_info_2]; later versions add the time zone [U: rust-minidump's misc info fields, WP-1] |
| Process VM counters (22) | recent dbghelp [U] | working set, private usage, page faults [R: https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/System/Diagnostics/Debug/struct.MINIDUMP_PROCESS_VM_COUNTERS_2.html] |
| Comment (10, 11) | ProcDump `-dc`, other tools [D: ProcDump page below] | the writer's own note, shown as-is |

rust-minidump 0.27 has typed readers for most of these, including `MinidumpThreadInfoList` and `MinidumpHandleDataStream`
[D: https://docs.rs/minidump/latest/minidump/]; handle object information and process VM counters have no typed reader
there [U, WP-1], so they are parsed from the raw stream with the same size guards as `read_handles`.

## Decision

### 1. Three checks, one report type

The dump view opens on a **Checks** strip with three buttons (Material buttons, verb plus object): **Check crash**, **Check
hang**, **Check memory**. The current Findings tab stays as the list of all findings; Threads, Modules and Details stay as
the evidence views the reports link into. A check is an engine operation that returns one `CheckReport` (section 9). The
UI only lays it out; it computes nothing (AGENTS.md, "Where logic lives").

Which button is primary is decided by the engine from the dump (`suggested_check` in the overview): an exception stream
makes Check crash primary; none makes Check hang primary; the user can run any of the three on any dump. A report is cached
per document, check kind and symbol generation (section 7), so pressing a button twice costs nothing.

Every report has the same parts:

1. **Verdict** in one sentence, from a closed set per check (no free-form guessing).
2. **Confidence**: `High`, `Medium` or `Low`, each with the reasons that set it (for example "stacks walked with call frame
   information", "lock owner read from the dump's handle data", "frames named from symbols for 3 of 4 modules"). The
   confidence is computed by rules in the engine, never a score that cannot be explained.
3. **Evidence**: ordered items, each a typed link (thread and frame, module, memory region, handle, managed object type,
   log event) that the UI turns into a click.
4. **Not checked**: each thing this check would normally look at but could not, with the reason from a closed set
   (`StreamMissing(kind)`, `MemoryNotInDump`, `NoSymbols(modules)`, `NoUnwindInfo(modules)`, `ToolUnavailable`,
   `OverBudget`, `DifferentProcess`, `NeedsSecondDump`, `Unsupported(arch)`) and a `NextStep` (family ADR 0052): which dump
   to take, which symbols to add. **When the data for a verdict is missing, the verdict is `CannotTell` with this list, not
   a weak verdict** (ER-33's spirit: no "confirmed" from a probe that cannot see).
5. **Around the dump**: the log events in a window around the dump's time (section 6).
6. **Left out**: what the bounds cut (ADR 0065's shape), for example "only the first 4,096 threads".

### 2. Check crash

Needs: the exception stream; the faulting thread's context and stack; module list; unloaded modules; symbols if any.

- **Verdict** one of: `Exception { code, name, plain_words, faulting_address, module, offset, access }`,
  `FastFail { subcode }`, `StackOverflow { recursion }`, `HeapCorruption`, `BreakpointOrAssert`, `NoExceptionRecorded`.
  The code is decoded by the existing table (`exceptions.rs`) into words a support engineer can repeat ("Access violation:
  the program read address 0x10, which is a null pointer plus 16"). The faulting address is placed in a module, or in an
  unloaded module's former range (WP-1 keeps the unloaded ranges), or "outside every module" (execute of freed or
  generated code).
- **Evidence**: the faulting thread's stack with the culprit frame (the first user frame, preferring walked frames:
  `views.rs:42-49`), the module of the culprit with version and time stamp, the exception parameters.
- **`NoExceptionRecorded` is honest**: "This dump has no exception record. It was probably taken on demand (Task Manager,
  ProcDump without `-e`, Debrief's capture) and does not show a crash. Run Check hang, or capture with ProcDump `-e` or WER
  local dumps." [D: https://learn.microsoft.com/en-us/sysinternals/downloads/procdump,
  https://learn.microsoft.com/en-us/windows/win32/wer/collecting-user-mode-dumps]. A dump whose exception is a
  breakpoint raised by a dump tool (0x80000003 on the writing thread, the thread-info flag "writing thread") is reported as
  a hang-type dump, not a crash.
- **Confidence**: `High` when the faulting frame is the context frame and the culprit was reached by context, CFI or
  frame pointer; `Medium` when the culprit is a scanned candidate or unnamed; `Low` when the faulting thread's stack memory
  is missing.
- **Not checked** examples: no symbols for the culprit module (named as module plus offset); no CFI for the modules on the
  faulting stack; no heap memory, so the heap-corruption check cannot inspect the heap; **first- or second-chance cannot be
  told from the dump** (the format records no such flag; ProcDump's `-e 1` writes on first chance [D: ProcDump page]): the
  report says so and never claims "unhandled".

### 3. Check hang

Needs: thread list; thread info list (times, start address); handle data with object information; stack memory; for
critical-section owners, the memory holding the lock (heap or data); symbols for the system modules.

**Every thread is classified** into `Running`, `Waiting { on }`, `WaitingOnLock { lock, owner }`, `Sleeping`,
`MessageWait` (the thread is in its message loop: idle, not hung), `Exited`, `Unknown`, each with how the class was
found (`ByName` from symbols, `BySyscall` from the top frame's module and export offset, `ByRegister` for a wait handle
read from the syscall's register context). Not having symbols must not make every thread "Running" as today: the top frame
of a waiting thread is almost always inside a system call stub of `ntdll.dll`/`win32u.dll`, which can be recognised from
the module plus an export table [U: needs the module image or its exports; full dumps hold the image, otherwise PDB or
binary from the symbol folders; WP-3].

**The UI thread** is the thread with a message-loop frame (`GetMessage`, `PeekMessage`, `MsgWaitForMultipleObjects`,
`DispatchMessage`, a `win32u` user call) deeper in its stack; when none has one, the earliest-created thread (thread info
create time) with `Low` confidence. The report states what the UI thread is doing: in its message wait (the UI is idle,
**not hung** in this dump); waiting on a lock or object (with the owner when known); or running (busy).

**Lock owners and deadlocks.** A wait-for graph is built only from edges the dump proves:

| Lock kind | Owner from | Needs |
| --- | --- | --- |
| Mutex (mutant) handle | handle object information (`MiniMutantInformation2`: owner thread) [D, layout per OS: U] | handle data |
| Handle the thread waits on | the handle value in the syscall's argument register (x64 `r10`/`rcx` at the `syscall` instruction; x86 on the stack) [J; U: confirm on invented dumps in WP-5] | thread context |
| Critical section | `RTL_CRITICAL_SECTION.OwningThread` (public layout in `winnt.h`) at the address the waiting frame passes [U: which register or stack slot per Windows build; WP-5] | the lock's memory: heap dump, full dump, or data segments |
| SRW lock, condition variable, `WaitOnAddress` | **no owner is recorded** | (cannot) |
| .NET `Monitor` | the sync block table via the .NET tool (`syncblk`) [D: dotnet-dump page below] | full or heap dump, the .NET tool |

A **cycle** in that graph is a `Deadlock { cycle }` verdict with `High` confidence. Without owner edges the verdict is
`BlockedIn { function, threads }` ("4 threads are blocked in `EnterCriticalSection` called from `Orders!Cache::Get`; the
owner is not in this dump") with `Low` or `Medium` confidence, and the Not checked list names the missing data ("take a
Minidump with heap so lock owners can be read"). Wait Chain Traversal is a live API and is not in a dump (catalogue).

**One dump or two.** One dump cannot tell a hang from slow progress. With two or more dumps of the same process (section
5), the check compares, per thread id: the stack (same frames from the top down to the first user frame), user and kernel
time (thread info), and for the UI thread its state. Verdicts: `Hung { threads }` (the same blocked stacks and no CPU time
gained between dumps), `Spinning { thread, cpu_share }` (the same or changing stacks with CPU time growing close to the
wall-clock gap: a busy loop), `SlowProgress` (stacks change, the UI thread returns to its message wait), `CannotTell`.
The UI recommends two to three dumps a few seconds apart (ProcDump `-n 3 -s 5` [D]) and Debrief's capture offers it
(section 5).

### 4. Check memory

Needs: memory info list (regions); thread list (stack regions); module list; handle data; process VM counters if present;
two or more dumps for growth; managed heap through the .NET tool.

- **One dump: composition, not a verdict.** Committed memory split into **images** (modules), **mapped** files,
  **private** memory, with **thread stacks** carved out of private (regions matching each thread's stack range) and, for
  .NET, the **managed heap** (GC segments reported by the tool); the **largest regions** grouped by allocation base (top 20)
  with protection and type; reserved versus committed; free space and the **largest free block** (address-space exhaustion
  for 32-bit processes, today's finding); thread count; handle count by type. Native heap cannot be told apart from other
  private memory without reading the heap's own metadata (the PEB's heap list), so private memory is labelled "private
  (heaps and other allocations)" unless the heap list was readable [J]. The report lists **suspects** ("private memory is
  1.8 GB, 92% of committed; 1 region of 1.2 GB allocated at …; 14,000 Event handles"), never "leak".
- **Two or more dumps: growth.** Same process (section 5), ordered by dump time. Per category and per region group (matched
  by allocation base, type and protection), per handle type, per thread count, per module set (loaded or unloaded between
  dumps), and per managed type (count and bytes, from the tool's heap statistics): the change, the rate per minute, and
  whether it grew in every step (three or more dumps, the UMDH "steady growth" rule of ADR 0023 §7). Verdicts:
  `Growing { what, rate }`, `NoGrowth`, `GrewOnce` (one step, then flat: a cache filling), `CannotTell`.
- **Native per-allocation stacks are not in a dump.** The report says "to see which code allocates, record UMDH snapshots"
  and links the existing UMDH view (ADR 0023 §6).

### 5. Two or more dumps: picking and the same-process check

- **Picking.** A check opened on one dump shows **Add dumps to compare…** for hang and memory. The picker lists the other
  open dumps, and the engine sorts them into "same process", "same program, another run", "other program". Only the first
  group can be selected for a verdict; the second can be opened for a side-by-side but the report says "different process
  (the pid was reused or the program restarted)" and gives no hang or leak verdict. Up to 16 dumps per check
  (`Limits.max_check_docs`, new).
- **Same process (rule 53, family ADR 0033).** Two dumps are of one process only when the pid and the process create time
  (misc info, seconds) are equal and the main module's path and CodeView identity are equal. A pid alone is a reusable OS
  name and is never enough. When the create time is missing from either dump, the pair is `SameProcessUnconfirmed`: the
  check runs with confidence at most `Low` and says why. The earliest thread create time (thread info, 100 ns units) is
  used as a second witness when both dumps have it [J].
- **Capture a series.** Debrief's capture tool gets a `count` and `interval_s` (`create { pid, kind, folder, count,
  interval_s }`; 2 to 5 dumps, 1 to 600 s): it holds the process handle across the series (rule 53: hold, do not re-open by
  pid), names the files `<exe>.<pid>.<kind>.<UTC stamp>.<n>.dmp`, and Debrief opens them as one group. The capture dialog
  gets three presets with their purpose: **For a crash** (points to ProcDump `-e` or WER local dumps, since a crash cannot
  be captured on demand), **For a hang** (Minidump with heap, 3 dumps 5 s apart), **For memory** (Full dump, 2 or more dumps
  minutes apart, or UMDH for native allocation stacks).
- **Telling the user which dump to take.** Every report's Not checked list carries the `NextStep` for this dump; the
  overview shows "This dump holds: stacks, handles, thread times; not: heap memory, full memory" computed from the header
  flags and the streams present (`DumpCapabilities`, section 9).

### 6. The log events around the dump's time

The engine gives each report a `EventsAround { at, window, counts_by_level, first_events, query }`: the dump's time
(header time stamp, UTC seconds), a window (default 2 minutes before and 10 s after for crash and hang, the span between
first and last dump plus 1 minute for multi-dump checks; a Setting), the counts by level in the window and the first 50
events by severity, and the timeline query that opens the window. Sources are restricted to the same machine when the
logs carry a host field that matches the dump's (no match: all sources, and the report says that the clocks of different
machines may differ). Times follow ADR 0066; the dump's time is UTC with one-second resolution, and the report says so.
The query is an engine operation over the store (no TypeScript filtering), bounded by ADR 0065's reply shape.

### 7. Native stack walking: CFI, symbols, offline

- **Today** the walker has no CFI (section Context). **Decision**: give `minidump-unwind` unwind information.
  `minidump-unwind` 0.27 has an optional `debuginfo-unwind` feature that pulls `framehop` 0.16, `object` and `memmap2`
  [D: https://docs.rs/crate/minidump-unwind/latest/features]; framehop (MIT or Apache-2.0) unwinds x86_64 and aarch64 and
  reads "PE unwind info in `.pdata`, `.rdata` and `.xdata` (for Windows x86_64)"; it is "still a work in progress. Its API
  is subject to change" [D: https://github.com/mstange/framehop]. How `minidump-unwind` finds the module files for it is
  **not verified** [U, WP-3 spike].
- **Where the unwind tables come from, offline first**: (a) a full dump holds every image's pages, so `.pdata`/`.xdata`
  are in the dump itself and are read by ranges (no extra file); (b) the module binary from the user's symbol folders,
  found by code identity (PE time stamp and size, the symbol-store layout `<name>/<TIMESTAMP><SIZE>/<name>`), never by name
  alone (ADR 0036's rule applied to binaries); (c) only when the user has turned symbol servers on, the binary is fetched
  like a PDB today (ADR 0036), by the host, never by the sandboxed reader. Nothing is downloaded by default.
- **Per architecture**: x64 Windows: PE unwind info as above, else scan (marked). x86 Windows: frame pointers plus FPO data
  from the PDB (`STACK WIN`-style frame data, which the `pdb` crate reads) [U: whether the walker accepts it through the
  symbol provider seam; WP-3]. arm64 Windows: the frame pointer chain is required by the ABI ("The frame pointer (x29) is
  required for compatibility with fast stack walking used by ETW and other services") [D:
  https://learn.microsoft.com/en-us/cpp/build/arm64-windows-abi-conventions], so frame-pointer walks are trustworthy; arm64
  PE unwind codes are not in framehop's list [D] and are not needed in 1.2.
- **Without symbols or unwind info** the stack is still shown, every frame says how it was found (`context`, `cfi`,
  `framePointer`, `scan`), scanned frames are labelled "candidate", and the report's confidence drops with a Not checked
  item naming the modules. Thread classification falls back to the syscall-stub rule (section 3).
- **Re-walk** when symbols or binaries arrive (the existing re-read after the symbols job, `dumps.rs:331-356`), and cached
  reports for that document are dropped (symbol generation in the cache key).

### 8. .NET managed state (research folded in)

The user approved managed stacks, exceptions and heap types for 1.2 with research first. ADR 0023 §8 planned loading the
runtime's data access component (DAC) into the reader ourselves.

**What any option needs** [D]: the runtime's DAC of exactly the dump's runtime version and architecture
(`mscordaccore.dll` for .NET, `mscordacwks.dll` for .NET Framework); "This is a native DLL which is tied to the
architecture of the dump you are debugging" [D: https://github.com/microsoft/clrmd/blob/main/doc/FAQ.md]. ClrMD looks it up
on Microsoft's symbol server by default on Windows, verifying its signature; on Linux and macOS it does not download it
[D: https://github.com/microsoft/clrmd/blob/main/doc/GettingStarted.md]. Heap walking needs the heap in the dump (full or
heap dump) and may fail for a dump taken mid-GC (`CanWalkHeap`) [D: same page]. Offline, the DAC comes from a matching
locally installed runtime (`shared/Microsoft.NETCore.App/<version>/`) or from a folder the user adds; otherwise the
download is the user's opt-in, like symbol servers.

| Option | Licence | Platforms for a Windows dump | Bundling, offline | Bounds | Effort | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| **A. `dotnet-dump analyze -c …` pinned through `paddock-tools`** (Microsoft, `dotnet/diagnostics`) | MIT [D: https://github.com/dotnet/diagnostics/blob/main/LICENSE.TXT]; NuGet 10.0.750501, 2026-10-07 [D: https://www.nuget.org/packages/dotnet-dump] | Windows (direct downloads for win-x86/x64/arm64 [D: https://learn.microsoft.com/en-us/dotnet/core/diagnostics/dotnet-dump]). Windows dumps on macOS or Linux: not documented, assume no [U] | Single-file downloads are "framework dependent", "require a 3.1 or newer .NET Core runtime" [D: https://github.com/dotnet/diagnostics/blob/main/documentation/single-file-tools.md]; the NuGet tool targets .NET 8 [D: NuGet page]. So the user needs a .NET runtime installed; `paddock-tools` locates it (read-only, ADR 0051 U4) and offers the runtime download otherwise. The x86 tool for x86 dumps [D]. .NET Framework dumps: not documented [U, WP-0] | Process timeout, output cap, Cancel kills it (through `paddock_proc`); its memory is outside our caps: a job object limit on Windows [J] | M | **Recommended first** |
| **B. A small ClrMD-based helper we write, emitting JSON** | ClrMD MIT [D: https://github.com/microsoft/clrmd/blob/main/LICENSE]; 4.2.751201, 2026-10-12, netstandard2.0 and net10.0 [D: https://www.nuget.org/packages/Microsoft.Diagnostics.Runtime] | Windows; ClrMD is "fully supported on Windows, Linux, and macOS" [D: FAQ], but a Windows dump needs the Windows DAC, a Windows DLL [J] | Self-contained single-file per RID (no runtime prerequisite) [J; size U]; .NET Framework supported through ClrMD [U, WP-0]; signing: unsigned like the family | Our own caps (objects walked, types listed, time), JSON schema versioned | L, and a C# project, toolchain and CI job | **Fallback**, needs D3 (C# outside the app) |
| C. Load the DAC from Rust (implement `ICLRDataTarget`, call `ISOSDacInterface`) | our code MIT; the DAC is the runtime's (MIT for .NET; .NET Framework's `mscordacwks` is part of Windows' .NET Framework, used where installed, never redistributed) [J] | Windows only | No extra runtime; a Microsoft DLL loaded into our sandboxed reader | Ours | XL: ClrMD's stack walk and heap walk logic re-done in Rust, COM by hand; no existing Rust crate found [R: web search 2026-11-07] | **No** for 1.2 |
| D. A Rust reader of the runtime's "contract descriptors" (cDAC) | MIT | All | No DAC needed | Ours | XL, and only for runtimes that carry descriptors; their status in released runtimes is not confirmed [R: dotnet/runtime PRs, web search 2026-11-07; U] | **Later**, watch |
| E. WinDbg/CDB with SOS | Microsoft licence terms, not permissive | Windows | Not redistributable by us | — | — | **No** (licence) |

**Decision**: a new plugin `net.decryptology.dumps.dotnet` behind a port `ManagedInspector` in `debrief-dump`
(`inspect(dump_path, request) -> ManagedState`), adapter A first. It holds `RunPrograms` (the tool) and `ReadFolders`
(the DAC folders), no network, and gets the dump by path (ADR 0064's `ByPath`). The tool comes from `paddock-tools` as a
pinned, hashed entry (ADR 0051: pins of the build only, no upstream check). One run per dump: `analyze <dump> -c
"clrthreads" -c "clrstack -all" -c "pe -nested"` per thread with an exception, `-c "dumpheap -stat"`, `-c "syncblk"`, `-c
"finalizequeue -stat"` [U: exact flags in this version], `-c "threadpool"`, `-c exit`; the commands are a data file (rule 21)
so a version change is a pin change plus that file. Output is parsed by a tolerant, fuzzed parser into typed values with
limits (types at most 10,000, frames 256 per thread, text clipped); a line it does not understand becomes a note, never a
wrong value. Each section reports whether it ran, failed or was cut, which feeds the Not checked list ("managed heap not
read: this is a minidump without heap"). The parser is checked against the pinned version's output on invented dumps
(WP-12); if WP-0 shows the output is not stable enough to parse, adapter B replaces A behind the same port.

What the managed state adds to each check: crash, the managed exception (type, message clipped, stack, inner chain) on the
faulting thread; hang, managed stacks per thread, `Monitor` owners from the sync block table into the wait-for graph,
thread-pool starvation (queued work items with all workers busy); memory, the managed heap size and the top types by bytes
and count, and their growth across dumps; the finalizer queue length.

### 9. Contract between engine and UI

New types in `debrief_dump::api`, generated by ts-rs into `dumps.ts` (one definition, ADR 0023 §10). The wire version is
bumped with no migration (Debrief is pre-1.0 for this data: old documents are simply re-read from their files).

```rust
pub enum CheckKind { Crash, Hang, Memory }
pub struct CheckRequest { pub kind: CheckKind, pub docs: Vec<u32> /* 1..=max_check_docs; the engine orders them */ }
pub struct CheckReport {
    pub kind: CheckKind,
    pub docs: Vec<CheckedDoc>,            // id, dump time, process identity, capabilities
    pub pairing: Option<Pairing>,         // SameProcess | SameProcessUnconfirmed | DifferentProcess, with why
    pub verdict: Verdict,                 // CrashVerdict | HangVerdict | MemoryVerdict, closed enums
    pub confidence: Confidence,           // { level: High | Medium | Low, because: Vec<ConfidenceReason> }
    pub evidence: Vec<Evidence>,          // typed links: Thread{doc,id,frame}, Module, Region, Handle, ManagedType, LogEvent
    pub threads: Option<Vec<ThreadClass>>,// hang: every thread, its class, how it was found, its owner edge
    pub memory: Option<MemoryComposition>,// memory: composition per doc and growth rows
    pub not_checked: Vec<NotChecked>,     // { item: CheckItem, why: MissingReason, next: Option<NextStep> }
    pub around: Option<EventsAround>,
    pub left_out: Vec<Omission>,          // ADR 0065
}
pub struct DumpCapabilities { pub exception: bool, pub thread_info: bool, pub handle_objects: bool,
    pub memory_regions: bool, pub stack_memory: bool, pub heap_memory: bool, pub full_memory: bool,
    pub unloaded_ranges: bool, pub process_times: bool, pub managed: ManagedAvailability }
```

Commands (one line each in the command table): `dumps_check` (starts a job, returns its id and, when cached, the report at
once), `dumps_check_result`, `dumps_pair_candidates` (the picker's three groups), `dumps_events_around`. Every error is a
`CmdError`. Text in reports is produced in the engine in English and keyed for translation like the findings today.

### 10. Jobs: off the UI thread, phases, Cancel (ER-17 to ER-19)

A check is a Jobs job (`JobKind::Analysis`) linked to its dumps, even when it finishes in milliseconds, so that every
check has the same Cancel and the same "Still working" behaviour. Phases, each with its own scale: "Reading thread
details" (count of threads), "Walking stacks" (threads walked, only when a re-walk with new unwind data is needed),
"Finding lock owners" (range reads done), "Reading .NET state" (the tool's sections, a growing count; the tool gives no
total), "Comparing dumps" (pairs), "Finding log events" (indeterminate, then the count). Cancel sets the flag the reader
and the comparison check between items, and kills the .NET tool process through `paddock_proc` by its held handle (rule
53); a cancelled check leaves no partial report in the cache. The UI never times out a check (rule 12); the .NET tool and
the reader have engine-side budgets that end in a typed `OverBudget` Not checked item, not in a dialog.

### 11. Bounded resources and memory caps

- **The reader** keeps ADR 0023's bounds. New: memory regions at most 65,536 per dump (summarised beyond), handle object
  records at most 65,536, range reads for lock owners at most 4,096 reads and 16 MiB per check, `max_check_docs` 16.
- **Heap dumps by ranges (decision D5).** The checks need small pieces of heap memory (a critical section, the PEB), never
  the whole heap. So the prefix-plus-ranges path of `source.rs` is extended to dumps whose memory list is the 32-bit
  `MemoryListStream`: keep the directory and metadata, read memory ranges on demand. Then a heap dump is no longer read
  whole, the 1 GiB refusal goes, and ADR 0064's `WholeReason::DumpWithoutMemoryList` can be dropped. That is a change to
  ADR 0064's list of exceptions and is the user's decision.
- **The FM gate (ADR 0067).** The dumps reader's row in `importers/memory.toml` must be measured on the shipping build
  (rule 48) after D5; until then the folder-open path uses ADR 0067's rules (an `at_least` factor of 2.1 for the store
  worker side from the three-dump reproduction, costed at the worst factor until measured). The check itself runs in the
  app process over the decoded, bounded model; its own allocations are bounded by the limits above [J].
- **The .NET tool** runs in its own process with a Windows job object memory limit (a Setting, default 4 GiB, Auto meaning
  half of physical memory up to 8 GiB [J]), a wall-clock budget (default 10 minutes per dump, a Setting) and an output cap
  (64 MiB, parsed as it streams). Two dumps are inspected one after the other by default (section 12).

### 12. Parallelism and cost (ER-20, rule 51)

| Step | Kind of work | Parallel? | Cheaper? |
| --- | --- | --- | --- |
| Reading the new streams (thread info, regions, handle objects) | small, from the prefix already read | no need | read once at open, with the rest of the prefix; no second pass |
| Classifying threads, the wait-for graph, cycle search | CPU, at most 4,096 threads × 256 frames | no: about a million frames at most, a single pass [calc] | from the cached model; the report is cached per doc, kind, symbol generation |
| Stack walks with unwind data | CPU per thread, independent | yes, across threads in the reader, bounded by cores − 1 and a Setting with "Auto"; deterministic order of results | walk only threads whose trust can improve (scanned frames in modules that now have unwind data); keep the 60 s budget |
| PDB and binary loading | I/O plus parse | a few at once (SSD), today's cache of 256 parsed PDBs | parse once per PDB identity; already cached |
| Lock owner range reads | random small reads | sequential is enough (few reads) | read only addresses that a waiting frame names |
| .NET tool | one process per dump, memory-heavy | **one at a time by default**, a Setting (memory-heavy work is bounded by RAM, ER-20) | one tool run per dump with all commands chained (`-c` repeated) instead of one run per command; results cached per dump file hash |
| Comparing dumps | CPU, small | no | matched by keys (thread id, allocation base, type name) in hash maps |
| Events around | store query | the store's own | one bounded query with counts and the first 50 rows |

Priority (rule 52): a check is work a person waits for, so it runs at the normal level on Windows and at most
`QOS_CLASS_UTILITY` elsewhere; the .NET tool child is started at normal priority. No numbers are quoted here; WP-13 records
the first measurements in `docs/performance.md` before any are quoted (rule 48).

### 13. Logs (ER-27)

Each check logs, under the job id as correlation id: the phases with start, end and duration; per dump the kind, the
capabilities (stream kinds present, by number), thread and module counts; walk results as counts per trust kind and the
budget if hit; symbols found, missing, downloaded as counts; range reads as count and bytes; the .NET tool's exact command
with the dump path replaced by its file name hash, its pinned version, exit code, duration, peak memory when the job object
reports it, and the last 20 lines of stderr; the parser's notes as counts by kind; the verdict kind, confidence level and
the codes of every Not checked reason. No user content by default: no module paths (file names only, as `dumps_explain`
does), no function names, no exception messages, no command line, no environment, no log text.

### 14. Explain with AI

"Explain with AI" on a report sends the report's facts only (verdict, confidence reasons, evidence labels, Not checked
items, counts of events around), redacted as `dumps_explain` is today (no memory contents, file names only, at most 6,000
characters) to Pitwall (ADR 0059). One report is one small request, not a large job (rule 46). The AI never decides a
verdict.

## Catalogue of analyses

Everything a minidump (or a series) can support, with what it needs, value, size and recommendation. Size: **S** up to 2
days, **M** 3 to 5, **L** over 5, raw [J]. "Today" means already in Debrief at the base commit [S].

### Native, one dump

| Analysis | Needs (streams, dump type) | Value | Size | Rec. |
| --- | --- | --- | --- | --- |
| Crash triage: exception in plain words, faulting address and module, culprit frame | exception, thread list, modules; any type | the first question for any crash | S on top of today | **1.2** (Check crash) |
| Fault in an unloaded module's former range | unloaded modules with ranges (today names only) | explains "crash in nowhere" after a plugin unload | S | **1.2** |
| Fast-fail subcodes, stack buffer overrun, `/GS` | exception parameters | names a whole class (today partly) | S | **1.2** |
| Stack overflow and recursion | exception 0xC00000FD, faulting stack | today; refine with CFI frames | S | **1.2** |
| Heap corruption signs | exception 0xC0000374; heap memory for more | today as a code; inspecting heap blocks needs full memory and page heap | M | code: 1.2; heap inspection: **later** |
| Nested exception record | exception record's pointer plus memory at it | rare chained faults | S | **later** |
| First- versus second-chance | (not in the format) | — | — | **cannot** (say so) |
| Thread classification without symbols (syscall stubs) | thread list; module exports from image, PDB or binary | makes hang triage work on unsymbolised dumps | M | **1.2** |
| UI thread and what it waits on | thread list; symbols or exports of `user32`/`win32u` | "is the window hung and why" | M | **1.2** |
| Lock owners: mutex | handle data with object info | deadlocks with mutexes | M | **1.2** |
| Lock owners: critical section | lock memory (heap or data segments) | the most common native deadlock | M | **1.2** (heap dumps) |
| Lock owners: SRW lock, condition variable | (not recorded) | — | — | **cannot** ("blocked in", low confidence) |
| Deadlock cycle | owner edges above | the hang verdict with high confidence | S after owners | **1.2** |
| Wait chains as Windows' Wait Chain Traversal shows them | live API only | — | — | **cannot** from a dump |
| Busy thread in one dump | thread info user and kernel time | a hint of a spin (absolute times only) | S | **1.2** |
| Memory composition, largest regions, stacks, free space | memory info list, thread list | "where the memory is" | M | **1.2** (Check memory) |
| 32-bit address-space exhaustion and fragmentation | memory info list; 32-bit process | today as a finding; add the free-block histogram | S | **1.2** |
| Native heap by heap and by size class | heap memory, NT heap metadata (undocumented, per build) | big for native leaks | L | **later** (UMDH covers allocation stacks) |
| Per-allocation stacks | (not in a dump; UMDH or page heap `+ust`) | — | — | **cannot**; UMDH view exists |
| Handles by type, high counts | handle data | handle leaks, today as a count | S | **1.2** |
| GDI and USER object counts | (not in a dump; `GetGuiResources` is live) | — | — | **cannot** |
| Module inventory: version, time stamp, path, CodeView id | module list | "which build crashed" | S | **1.2** (modules tab exists; add time stamps) |
| Unexpected modules: injected hooks, antivirus, user-writable paths, duplicate versions | module list; a TOML pack of known injectors (rule 21) | finds third-party DLLs behind odd crashes | M | **1.2** (pack seeded small) |
| Unsigned modules | (signatures are in the files, not the dump) | — | — | **cannot** from the dump; possible only with the binaries, **later** |
| Process info: pid, start time, uptime, integrity level | misc info | context | S | **1.2** (today partly) |
| Command line | PEB memory (heap, full or `MiniDumpWithProcessThreadData`) | which instance and options | M | **later**; user content, on demand only |
| Environment variables | PEB memory | rarely needed, often secrets | M | **no** for 1.2 |
| System info: OS build, CPU, processors | system info | context; today | S | **1.2** |
| System uptime, boot time | (not in a minidump) | — | — | **cannot** |
| 32-bit process dumped by a 64-bit tool (WOW64) | modules `wow64*.dll`, x64 context | stacks show only the WOW64 layer; tell the user to use the 32-bit writer | S detect, L to walk the x86 side | detect **1.2**; walk **later** |
| The writer's comment stream | comment streams | the tool's note (ProcDump `-dc`) | S | **1.2** (shown, not logged) |
| Crashpad and Breakpad minidumps from other platforms | rust-minidump reads them [D: docs.rs page] | Electron and Chromium-based apps | M | **later** |
| Kernel dumps, Linux core files | another format | — | — | **no** (not minidumps) |

### Native, two or more dumps

| Analysis | Needs | Value | Size | Rec. |
| --- | --- | --- | --- | --- |
| Same-process check | misc info pid and create time, main module identity | the precondition of every series verdict (rule 53) | S | **1.2** |
| Hang versus slow progress (same stacks) | thread list in each | the hang verdict users ask for | M | **1.2** |
| CPU spin (thread time deltas) | thread info in each | names the busy thread | S | **1.2** |
| Memory growth by category and region group | memory info list in each | the leak verdict | M | **1.2** |
| Handle growth by type | handle data in each | handle leaks | S | **1.2** |
| Thread count growth, threads created and gone | thread list, thread info | thread leaks | S | **1.2** |
| Modules loaded or unloaded between dumps | module lists | plugin churn | S | **1.2** |
| Comparing against a known-good dump (other run, same program) | modules, system info | "what is different on the broken machine": versions, extra DLLs | M | **later** |
| Crash grouping by signature across many dumps (bucket by code, module, culprit, top user frames) | exception and stacks of each; a folder of dumps | "how many distinct crashes are these 40 WER dumps" | M | **later** (1.3), built on Check crash's signature |

### .NET (through the tool, section 8)

| Analysis | Needs | Value | Size | Rec. |
| --- | --- | --- | --- | --- |
| Managed exception: type, message, stack, inner chain | any dump with the exception object's memory; DAC | the crash answer for .NET apps | M | **1.2** |
| Managed stacks per thread | stacks; DAC | names frames that are "code without names" today | M | **1.2** |
| Interleaving managed and native frames by stack pointer | both stacks | one readable stack | M | **later** |
| Heap by type (count, bytes) | heap or full dump | composition and leak suspects | S with the tool | **1.2** |
| Heap growth by type across dumps | heap or full dumps of one process | the managed leak verdict | S | **1.2** |
| `Monitor` lock owners (sync blocks) | heap or full dump | managed deadlocks | S | **1.2** |
| Thread-pool starvation | full or heap dump | a common .NET "hang" | S | **1.2** |
| Finalizer queue length and the finalizer thread's state | heap or full dump | a classic leak behind a blocked finalizer | S | **1.2** |
| GC roots for a type (why it is kept) | full dump; expensive per object | the next step after "type X grew" | M | **later** (on demand, bounded) |
| Duplicate strings | full dump; a full heap walk | memory savings | M | **later** |
| Async state machines (`dumpasync`) | full dump | stuck awaits | M | **later** |
| Out-of-memory details (`analyzeoom`) | full dump | explains an `OutOfMemoryException` | S | **later** |
| Heap verification (`verifyheap`) | full dump | managed heap corruption | S | **later** |

### Across all checks

| Analysis | Needs | Value | Size | Rec. |
| --- | --- | --- | --- | --- |
| Dump time to log alignment, events around the dump | header time; the store | ties the dump to the story in the logs | M | **1.2** (section 6) |
| Symbols: offline symbol folders, PDBs found in the opened folder or bundle | PDB identity (ADR 0036) | names without network | S | **1.2** (offer to add the folder a dump came from) |
| Unwind information for x64 (CFI) | images in full dumps, binaries in symbol folders | trustworthy stacks | L | **1.2** (section 7) |
| Export a report as report blocks with citations | report evidence ids | sharing results | M | **later** (1.3) |
| AI explanation over computed facts only | the report | a readable summary | S | **1.2** (section 14) |

**What a minidump simply cannot give**: whether an exception was first- or second-chance; owners of SRW locks and
condition variables; live wait chains; GDI and USER object counts; per-allocation stacks of the native heap; module
signatures; system uptime; anything that happened before the dump (except an Intel PT trace, not supported); other
processes; kernel stacks (ProcDump `-mk` writes a separate kernel dump [D]); which window was hung (no window handles).
Each check names the relevant ones in its Not checked list instead of guessing.

## Consequences

- The user starts from a question and gets a verdict with its confidence and limits; a weak dump yields `CannotTell` with
  the dump to take next, never a confident wrong answer.
- Hang and memory verdicts become possible at all, through series of the same process; the capture tool learns series.
- x64 stacks become trustworthy where unwind data exists, which also improves today's findings.
- .NET support adds a downloaded tool and a .NET runtime prerequisite on the analysing machine, Windows only for Windows
  dumps. macOS and Linux users get native checks and a Not checked item for managed state.
- Risks: (1) **lock-owner reads depend on undocumented details** (the syscall register convention, mutant information
  layout per OS build, the critical-section address on the waiting stack); mitigated by confidence levels and tests on
  invented dumps of every supported Windows version the CI and VMs have; (2) **`dotnet-dump`'s text output is not a
  contract**; mitigated by pinning, a data file of commands, a tolerant fuzzed parser, golden outputs per pin, and adapter
  B as fallback; (3) **framehop is a work in progress** [D]; pinned through `minidump-unwind`'s feature, behind the
  walker; (4) **heap dumps by ranges** changes the reader's I/O pattern; ADR 0064's reader contract must allow it (D5);
  (5) **time alignment across machines** can mislead; the report says when sources are from other machines.

## Test plan (invented dumps only, ER-14)

No real user dump is ever used in tests, fixtures, docs or commits; every dump is generated.

- **Synthetic minidumps in Rust.** Extend `debrief_plugin_dumps::synth` (which follows `minidumpapiset.h`, `synth.rs:1-2`)
  with builders for the thread info list, handle descriptors with mutant object information, a memory info list with many
  regions, misc info with process times, comment streams and memory ranges holding a critical section. rust-minidump's own
  `minidump-synth` (MIT, published "only so that `cargo publish` doesn't complain") [D: https://github.com/rust-minidump/rust-minidump]
  is an alternative builder; the existing `synth.rs` is kept (no new dependency) [J]. Series are built from one builder with
  the same pid and create time and changed stacks, times and regions; a "pid reused" pair has a different create time.
- **Engine unit tests** per check over those dumps: each verdict, each confidence rule, each Not checked reason. A
  **property** for every stream: removing it from a dump never raises confidence and always adds its Not checked item
  (the ER-33 probe: the check shows it can see by flipping a known deadlock dump to `CannotTell` when the handle stream is
  removed). Proptest "never panics" over random reports' inputs; fuzz seeds for the new stream parsers in
  `fuzz_dump_minidump`.
- **Windows-only integration tests** (the Windows runner and local VMs, rule 7): invented Rust test programs in the repo,
  started as children, each with one fault: an access violation, a stack overflow, a two-mutex deadlock, a
  two-critical-section deadlock, an SRW wait, a spinning thread, a growing allocation and handle leak. The capture tool
  writes mini, heap and full dumps (series for spin and leak) into a temp folder of the test, and the checks must give the
  expected verdicts. Child processes are ended through `paddock_proc` by held handle and nothing is left behind.
- **.NET** (Windows runner): an invented console program in the test fixtures (managed exception, `Monitor` deadlock,
  thread-pool starvation, a growing `List<T>`), built on the runner with the .NET SDK (decision D8), dumped with the
  capture tool; the pinned `dotnet-dump` runs through the plugin; golden parsed outputs per pin. Parser tests run on every
  OS from committed tool outputs of those invented dumps (text only, no dumps committed).
- **UI**: component tests for the report layout from the mock backend (the mock never computes a verdict); one Playwright
  spec per check; the real-engine path through the existing plugin end-to-end tests
  (`debrief-plugins/tests/it/dumps.rs`).

## Work packages

Judgement-sized raw days [J]; firm estimates follow once WP-0 and WP-3's spikes report (memory note "estimate after
design").

| WP | Scope | Files | Days | Depends |
| --- | --- | --- | --- | --- |
| WP-0 | .NET spike on a Windows VM with the invented fixtures: `dotnet-dump` commands and output stability for the pinned version, .NET Framework support, DAC lookup offline, memory and time on a 1 GB heap; the same for a 50-line ClrMD helper; record in `performance.md` (rule 48); report A or B | notes only | 3 | — |
| WP-1 | Reader: thread info, handle object info (raw, guarded), memory regions, unloaded ranges and times, misc info process times and time zone, comment streams, VM counters, `DumpCapabilities` | `debrief-plugin-dumps/src/dump_reader.rs`, `synth.rs` | 4 | — |
| WP-2 | Model, wire, limits, the `CheckReport` contract and ts-rs output, confidence and Not checked framework | `debrief-dump/src/{model,api,limits,wire}.rs`, new `checks/` module | 3 | WP-1 |
| WP-3 | Unwind data: `debuginfo-unwind` spike, images from full dumps by ranges, binaries from symbol folders by code id, host fetch when servers are on, x86 FPO, re-walk only improvable threads, parallel walks | `dump_reader.rs`, `source.rs`, `pdb_symbols.rs`, `app/src-tauri/src/symbols.rs`, `debrief-symbols` | 7 | WP-1 |
| WP-4 | Check crash | `debrief-dump/src/checks/crash.rs` | 3 | WP-2 |
| WP-5 | Check hang, one dump: thread classes incl. syscall stubs, UI thread, owner edges (mutant, register handle, critical section by range read), cycle search | `checks/hang.rs`, `source.rs` (range reads) | 6 | WP-2, WP-3 |
| WP-6 | Series: same-process check, picker groups, hang versus progress, spin, growth framework | `checks/series.rs`, `store.rs` | 4 | WP-4, WP-5 |
| WP-7 | Check memory: composition, suspects, growth rows | `checks/memory.rs` | 4 | WP-2, WP-6 |
| WP-8 | Heap dumps by ranges (D5) | `source.rs`, ADR 0064 amendment | 3 | WP-1 |
| WP-9 | Events around the dump: store query, host match, timeline link | `debrief-core` query, `checks/around.rs` | 3 | WP-2 |
| WP-10 | Shell: `dumps_check` job with phases and Cancel, cache, commands, logs per section 13 | `app/src-tauri/src/dumps.rs`, command table | 3 | WP-2 |
| WP-11 | Capture series and presets | `capture.rs`, `plugin.rs`, `create-dump-view.ts` | 2 | — |
| WP-12 | .NET plugin: `ManagedInspector` port, `paddock-tools` entry and pin, sandbox, tool run with job object, command data file, parser with limits and fuzz target, feeding the three checks | new `debrief-plugin-dumps-dotnet` (or a module of the dumps crate, D2), `paddock-tools` catalogue | 8 | WP-0, WP-2, ADR 0051's crate |
| WP-13 | UI: Checks strip, report view (verdict, confidence, evidence links, Not checked with next steps, events around), pair picker, capability line, Explain with AI on a report; Material first, every state designed | `app/src/app/dumps/` | 6 | WP-10 |
| WP-14 | Tests across the above that are not inside a package: Windows fixtures and integration tests, the stream-removal property, Playwright; first measurements in `performance.md` | tests, `docs/performance.md` | 3 | all |
| WP-15 | Docs: user guide (which dump for which question), features, ADR 0023 amendment (CFI statement corrected), independent review of the report screens (rule 22) | `docs/` | 1 | WP-13 |
| | **Total** | | **63** | |

Order for 1.2: WP-0 and WP-1 first in parallel, then WP-2, WP-3 and WP-8; then the checks; WP-12 after WP-0's report.
Without .NET (WP-0, WP-12) the native checks are 52 days [calc].

## Open decisions for the user

| # | Question | Recommendation |
| --- | --- | --- |
| D1 | .NET adapter: `dotnet-dump` (A) or our ClrMD helper (B) | **A first**, behind the port; B only if WP-0 shows A's output cannot be parsed reliably |
| D2 | .NET in its own plugin (its own rights: `RunPrograms`) or inside the dumps reader | **Its own plugin**: the reader keeps no-network, no-programs least privilege |
| D3 | If B is ever needed: allow a C# helper, built and pinned outside the app, despite "no C# in the app" | Decide only if WP-0 picks B; then **yes**, as an external tool through `paddock-tools`, never linked |
| D4 | .NET prerequisite: require the user's installed .NET runtime, or let `paddock-tools` download one | **Locate the installed runtime; offer the official runtime download as an opt-in step** (network on by default but per feature, memory note) |
| D5 | Read heap dumps by ranges (drop ADR 0064's `DumpWithoutMemoryList` whole read and the 1 GiB refusal) | **Yes**, in 1.2 |
| D6 | Fetch module binaries (for unwind data) from the user's symbol servers when servers are on | **Yes**, same switch as PDBs; nothing by default |
| D7 | Capture series in Debrief's capture tool (2 to 5 dumps at an interval) | **Yes** |
| D8 | Install the .NET SDK on the Windows CI runner for the invented .NET fixtures | **Yes**, pinned; otherwise .NET parsing is tested only from committed text outputs |
| D9 | Command line from the PEB: show on demand in 1.2, or later | **Later**; it is user content and needs the PEB layout work |
| D10 | Crash grouping across many dumps in 1.2 | **Later (1.3)**; Check crash's signature is built so that grouping is a small step |
