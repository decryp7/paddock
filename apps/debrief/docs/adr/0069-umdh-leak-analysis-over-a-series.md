# 69. UMDH leak analysis over a series: trends per call site, symbols by identity, findings that say what they could not check

- Status: Proposed. Revision 2 (2026-11-07) answers the independent review of `cee49f7a1` (P1-4 to P1-6 and the P2s).
- Date: 2026-11-07
- Related: ADR 0023 (dumps and UMDH: reader plugin, engine crate, problem-first views), ADR 0036 (symbols by PDB identity;
  its section 2 "UMDH is not symbolised from folders" is amended by D4 here), ADR 0059 (Pitwall as the AI engine),
  ADR 0064 (one streaming import trait; its SI-11 moves the UMDH reader to a line loop), ADR 0065 (bounded replies),
  ADR 0066 (how Debrief reads times: provenance), ADR 0068 (dump checks, Proposed, on `docs/debrief-dump-analysis-adr`:
  D7 here returns its `CheckReport` shape; where they differ, ADR 0068 wins); family
  rules 42 (share first), 46 (no LLM for large jobs), 48 (benchmarks recorded), 51 (threads or cheaper), 55 (one
  implementation per mechanism); ER-14, ER-17 to ER-20, ER-27
- Audit: [2026-11-07 analysis usefulness](../research/2026-11-07-analysis-usefulness.md), Part 1
- Evidence marks: **[S]** read in source at `origin/debrief/1.2-int` `852b35ac0`, paths under `apps/debrief/crates/`;
  **[D]** Microsoft documentation, read 2026-11-07 (URLs below); **[J]** judgement; **[U]** not verified. Nothing was built
  or run for this ADR. Sizes are judgement, raw agent-days, ±50 %.

## Context

The user: "UMDH diff analysis, right now it is not very useful". What exists today [S]:

- **Reader** (`debrief-plugin-dumps/src/umdh_reader.rs`): reads the whole file (`read_exact(0, len)`, at most
  `max_read_bytes` 1 GiB), decodes UTF-8 or UTF-16 with a BOM, and recognises two shapes: a snapshot
  (`<size> bytes + <overhead> at <address> by BackTrace<id>` per allocation, summed per back trace into `bytes`,
  `count`, `largest`) and a UMDH diff (`± delta (new - old) count allocs BackTrace<id>`, its count line, the stack,
  `Total increase == …`, hex or `-d` decimal). Frames are kept only when the line is indented, at most
  `max_umdh_frames` 64 per stack, at most `max_stacks` 200,000 stacks. The capture time is the **file's modification
  time** (`file_time_ms`); the process is never filled in (`process: None`, line 330).
- **Engine** (`debrief-dump/src/umdh.rs`): `growth(base, target)` matches stacks of **two** logs by their full frame
  list (`module!function` per frame joined into one `String` key), sorts by byte growth, and pages it grouped by stack,
  by module or by function of the *culprit* (first frame that is neither an allocator nor in a system or runtime
  module, `modules.rs:106,140`). `tree` gives the top-down tree. `findings` over the open snapshots: the heap total
  between first and last, the top 5 growing stacks if ≥ 1 MiB or ≥ 10 % of the growth (critical at ≥ 50 %), "steady
  growth" for a stack whose bytes rise **strictly in every step** of three or more snapshots, and single allocations
  ≥ 16 MiB. Constants `LEAK_BYTES`, `LEAK_SHARE`, `LARGE_ALLOCATION`, `TOP` (`umdh.rs:283-288`) are not Settings. The
  store caches one `(base, target)` pair (`store.rs:205`) and orders a series by `time_ms`, falling back to the order
  given (`store.rs:190`).
- **Symbols**: none. `import_umdh` keeps "the names UMDH wrote" (`debrief-plugin-dumps/src/import.rs:39`) and ADR 0036
  §2 rules out naming by module name alone. Raw addresses become `module+offset` when the snapshot lists modules.
- **UI** (`app/src/app/dumps/umdh-view.ts`): pick a log and an optional base; tabs Findings, Growth (table: Growth,
  Allocations, Alive, Stacks; a text filter), Tree; a stack panel marks the culprit frame ("allocated here"). The
  explanation prompt (`debrief-dump/src/explain.rs:71`) passes the findings and counts.

Why that is weak for finding a leak [J, with the cited sources]:

1. **Two points cannot tell a leak from a cache.** Every tool that investigators use for native leaks looks at
   allocations over time: WPA classifies allocations as outstanding at the end of a time selection (AIFO) against
   freed inside it (AIFI) ([WPA heap glossary](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/xperf/wpa-heap-glossary) [D]);
   DebugDiag's leak report ranks the components that allocated the most and did not release it, then the allocation
   sizes behind them ("689 MB was allocated in 422 byte chunks") and samples with allocation times
   ([Tess Ferrandez, Debug Diag 1.1](https://learn.microsoft.com/en-us/archive/blogs/tess/debugging-native-memory-leaks-with-debug-diag-1-1) [D]);
   UMDH's own guide says one log "is not sufficient" and to compare logs "recorded at different times"
   ([Using UMDH](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/using-umdh-to-find-a-user-mode-memory-leak) [D]).
   Debrief's Growth and Tree tabs only ever compare two logs; the series exists only as the strict "every step" test.
2. **The series test is strict and the size test cannot tell shapes apart.** One flat or slightly lower step (a burst
   freed, a quiet interval) removes a leaking stack from `SteadyGrowth` (`umdh.rs:383`, strict `>`); `LeakGrowth` (by size
   between first and last) may still fire, but it fires the same way for a one-off load step, so the two cases look alike.
3. **Identity is too fine.** The key is the whole stack. A growing stack is reported only when it grew ≥ 1 MiB or ≥ 10 %
   of the growth (`umdh.rs:340`, skipped when `d < LEAK_BYTES && share < LEAK_SHARE`), and only the top 5. The same
   leaking call site reached from a dozen callers, each under 1 MiB and under 10 %, gives no finding at all. Grouping by
   culprit function exists in the table but not in the findings or the series test.
4. **Without names most stacks say nothing.** A snapshot taken without `_NT_SYMBOL_PATH` holds raw addresses; UMDH
   "resolves symbols when you analyze the log" ([Interpreting a UMDH log](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/interpreting-a-umdh-log) [D]).
   Debrief names none of them, and does not say how much of the growth is unnamed.
5. **Times are guessed silently.** Ordering and any rate rest on file modification times, which a copy or a
   download may change; nothing shows that.
6. **No sizes.** A snapshot lists every allocation's size, but only the sum, count and largest survive the reader, so
   "all growth is 422-byte blocks" (the strongest hint of which object leaks) is lost.
7. **No context.** Nothing compares the NT heap's growth with the process's Private Bytes (from a `.blg` or the
   process monitor) or with the logs at the snapshot times, and nothing says when the growth is not on the NT heap at
   all (the .NET GC heap and `VirtualAlloc` are invisible to UMDH; DebugDiag's first step is exactly this split [D]).
8. **Findings do not say what they could not check** (no stack database, unnamed frames, a restart between snapshots,
   unreliable times), so a clean result is not trustworthy.

Known UMDH quirks that the design must handle [D]:

- Stacks exist only with the user-mode stack trace database: `gflags /i ImageName +ust`, and the process restarted
  after it; the database is capped by default at 32 MB (x86) or 64 MB (x64) and can be raised ("Stack Backtrace
  (Megs)") ([Using UMDH](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/using-umdh-to-find-a-user-mode-memory-leak)).
- BSTR caching hides frees unless `OANOCACHE=1` was set before the process started (same page).
- On another machine, the symbol path "must point to the symbols for the version of Windows that was loaded on the
  logging computer … Do not point the symbol path on the analysis computer to a symbol server", or the names are
  wrong ([Analyze UMDH logs](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/analyze-umdh-logs)).
- Output varies by version and mode: `-d` decimal, `-v` verbose, `-l` file and line when analysing logs
  ([Analyze UMDH logs](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/analyze-umdh-logs)), and `-g`
  unreferenced blocks when capturing ([Analyze a running process](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/analyze-a-running-process));
  the documented example of a log entry is aggregated per back trace, `00005320 bytes in 0x14 allocations
  (@ 0x00000428) by: BackTrace00B53` (0x14 allocations of 0x428 bytes each), with frames that are not indented in the
  page's rendering [D]. The reader handles neither (see "Failing tests first").
- `BackTrace` ids are indices into the process's stack trace database: stable within one process lifetime (the
  walkthrough looks up a diff's `BackTrace00B53` in the second snapshot [D]), but after a restart the same id may be a
  different stack. Matching by frames (ADR 0023 §6) stays; the id is a check within one process instance only.
- Stack depth is bounded by the stack database [U: the depth limit is not in the pages read]; truncated stacks must
  not split one call site into several.
- 32-bit processes on 64-bit Windows need the matching UMDH; addresses are 8 hex digits there [U].

## Decision

### D1. A series is the unit; two logs are a series of two

The engine builds one `UmdhSeries` from 2 to `max_series` (default 64, a Setting) snapshots of one process, oldest
first, or from one UMDH diff log (a series of two with UMDH's own numbers). Every view (table, tree, findings, export,
explanation) reads the series, so "compare with" disappears as a separate mode: choosing two logs is a series of two.
The engine checks that the logs belong to **one process instance**, the `ProcessInstance` of the roadmap
proposal's foundation (pid, process start time and main image identity where known; rule 53 and ADR 0068 §1 use the
same triple). A UMDH snapshot records neither pid nor start time [D: the documented log shape has none], so the
instance comes from, in order: the collector's manifest when Debrief or `debrief-agent` took the snapshot; the main
executable's base address and size in each log's module list (a restart under ASLR almost always moves it [J]); else
*unconfirmed*. DLLs loading or unloading between snapshots never split the series: only the main executable's entry is
compared. A changed main-executable base splits the series ("the process restarted between snapshot 3 and 4") and both
parts are offered; an unconfirmed instance gives at most `Low` confidence and a Not checked item. A mix of snapshots and
diff logs is refused with that sentence.

### D2. Identity in three levels, grouping by call site by default

Frames are interned once per series (`FrameId(u32)` into a table of `module!function`, or `module+offset` without a
name); a stack is a `Box<[FrameId]>`. Three identities are computed from it:

- **Call site** (default): the culprit frame plus its `caller_depth` callers (default 2, a Setting), after collapsing
  allocator frames and runs of system and runtime frames to the first one in the user's modules;
- **Function**: the culprit frame alone;
- **Stack**: the full frames, for the drill-down.

The culprit rule stays ADR 0023's, but its lists (allocators, system and runtime modules) become **one classifier
shared with `debrief-profile`'s "just my code"** (`debrief-profile/src/frames.rs:138` and `debrief-dump/src/modules.rs:106`
are two implementations today: rule 55), with the user's own patterns from the same Setting. Truncated stacks (at the
frame cap or the database's depth) are marked and grouped by their deepest common call site, never split.

### D3. A trend per call site, classified, with a rate when the times allow it

For each identity the series gives bytes and count per snapshot. The engine computes, for every call site:

- the **Theil–Sen slope** (median of pairwise slopes; robust to one outlier step) of bytes and of count, per step and
  per hour; least squares for the ranking pass over all call sites, Theil–Sen for the top `trend_candidates` (default
  2,000) only;
- the share of steps that grow, the last steps' change (plateau test) and the largest single step's share of the total
  growth (step test);
- a **class**: *steady growth* (slope > 0, at least `rising_share` 0.7 of steps grow, no plateau), *growth then
  plateau* (cache warming: the last `plateau_steps` 2 steps change less than `plateau_tolerance` 2 % of the total),
  *one step* (one step ≥ 0.8 of the growth), *fluctuating*, *shrinking*, *new*, *gone*.

**Minimum snapshots and precedence** (aligned with ADR 0068 revision 2 `a8bc78a53` §4: two dumps give `Grew` at most,
three or more `Growing`):

| Snapshots | Classes that may be given | Confidence at most |
|---|---|---|
| 2 | *grew*, *shrank*, *no growth*; the check's verdict is never a leak (D7) | `Low` |
| 3 | all classes except *growth then plateau* (needs `plateau_steps` 2 flat steps after growth, so at least 4) | `Low` |
| 4 to 5 | all | `Medium` |
| 6 or more | all | `High` |

Precedence, first that matches: *new* / *gone* (absent at one end), *shrinking* (slope < 0), *one step* (one step
≥ 0.8 of the growth, whatever the others do), *growth then plateau*, *steady growth*, *fluctuating*. So a load step is
never a leak, and a plateau after growth is never a leak.

Ranking is by projected growth (slope × series length) for steady growth, else by net growth. Call sites enter the
Theil–Sen pass by the larger of their rank by least-squares growth and their rank by **number of rising steps**, so a
slow steady leak is not crowded out by a few large one-off steps. All thresholds named
here are Settings in Settings ▸ Dumps ▸ UMDH with these defaults; the constants of `umdh.rs:283-288` become their
defaults.

### D4. Symbols by identity, never by name alone

The ladder, first that works per module:

1. The names UMDH wrote (a frame with `module!function`) are kept, marked *as UMDH wrote them*.
2. A raw address in a listed module gets an identity only from **the same process instance** (D1): a dump of that
   instance open in the project or the bundle (same pid and start time; its module list carries the CodeView GUID and
   age for the module at that path, base and size), or the collector's manifest of that instance when it records the
   module identities. UMDH's own module list has base, size and path only (`umdh_reader.rs:226-234`), with no time stamp
   or GUID, so ADR 0036 §1's check cannot be made from the log alone. A PE file of the same path found in the bundle
   or a symbol folder is used **only** when its size equals the listed size and the instance is confirmed, and its
   names are marked **inferred identity** (the same path and size can still be another build) with a Not checked item.
   Without the same instance, raw frames stay `module+offset` with the item "no identity for these modules: not the
   same process instance as any dump in the project".
3. With an identity, the PDB is found as ADR 0036 does (symbol folders; servers only when the user turned them on).
4. Otherwise the frame stays `module+offset`; the module's status says why (`NoIdentity`, `Missing`, …, ADR 0036 §3).

This amends ADR 0036 §2 ("UMDH is not symbolised from folders"): still never by file name, now by an identity found
elsewhere. For a UMDH **diff** log produced on another machine, the engine cannot check which Windows build named the
system frames; every finding on such a log carries the could-not-check line "system frames were named on the
machine that ran the comparison; they may be wrong if it pointed at a symbol server" [D, the Analyze UMDH logs page].

### D5. Snapshot times with provenance

Each snapshot's time comes from, in order: a time the user set; a date and time in the file name, read by ADR 0066's
time reader (its shapes and zone ladder); the collector's manifest when Debrief or `debrief-agent` took the snapshot;
the file modification time, marked *weak*. The time and its provenance are shown per snapshot. Rates per hour are
shown only when every time is better than *weak*; otherwise growth is per step, and the finding says why. A series
whose times are all equal or missing keeps the order the user chose and says so.

### D6. Sizes per call site

The reader keeps, per back trace, a sparse log2 size histogram (at most 48 buckets) and the three most frequent exact
sizes with their counts (a bounded top-k). An aggregated entry (`… bytes in 0x14 allocations (@ 0x428) …`) gives its
per-allocation size directly (`@ 0x428`), counted 0x14 times. The engine merges them per identity, so a finding can say "94 % of the
growth is 1,064-byte blocks", the hint DebugDiag gives [D]. A diff log has no sizes; the finding says so.

### D7. A check report in ADR 0068's shape

ADR 0068 (Proposed 2026-11-07, `docs/debrief-dump-analysis-adr`, its section 1 and the `CheckReport` type of its
section 2) defines the report every dump check returns. The UMDH series returns the same parts, as a fourth check kind
(**`CheckKind::HeapGrowth`**, the name ADR 0068 revision 2 asks for: a check is named for what it looks at, and a
leak is a verdict it may or may not reach) so the UI, the report block and Pitwall read one shape:

1. **Verdict** from a closed set: `Leak { call_sites }` (steady growth, three or more snapshots only), `CacheWarming`,
   `GrewOnce` (one step then flat: the one name for this verdict, ADR 0068 revision 3 §4 and D13), `Grew { delta }` (two snapshots, never more than that), `NoGrowth`, `NotNtHeap` (D8), `ManagedHeap` (D8),
   and `CannotTell` when the data for a verdict is missing (ADR 0068's rule: no weak verdict from a probe that cannot
   see; ER-33). **Two logs give only `Grew` with Not checked `NeedsMore { of: Snapshots, have: 2, need: 3 }`, `NoGrowth`,
   `NotNtHeap` or `CannotTell`**, the one closed reason ADR 0068 shares with this ADR (`NeedsMore { of: Dumps | Snapshots, have, need }`, ADR 0068 revision 3 `4b91eb4a4` §1 and its D13). Confidence follows D3's table.
2. **Confidence** `High`, `Medium` or `Low` with the reasons that set it (class, number of snapshots, time provenance,
   share of growth named, truncated stacks), computed by rules, never an unexplained score.
3. **Evidence**: typed links: the series, a call site, a snapshot, a size signature (D6), a Private Bytes series, a log
   event around a snapshot time; each is a citation the report and Pitwall can use.
4. **Not checked** from a closed set with a next step (family ADR 0052): `NoStackDatabase` (enable `+ust`, restart),
   `Unnamed(share)` (add the PE or a dump of the same process, D4), `WeakTimes` (set the times), `Restart(at)`,
   `DiffFromAnotherMachine` (D4), `DatabaseMayBeFull` (raise the stack trace database size), `NoPrivateBytes` (record a
   `.blg` or the process monitor), `NeedsMore { of: Snapshots, have, need }` (two logs cannot tell a leak from a cache),
   `InstanceUnconfirmed` (D1), `InferredIdentity(modules)` (D4).
5. **Around**: log events around each snapshot's time.
6. **Left out**: ADR 0065's omissions (stacks over `max_stacks`, frames over the cap, call sites beyond
   `trend_candidates`).

ADR 0068's "Check memory" already points to UMDH for native allocation stacks (its section 4); D8 links back. Today's
`HeapTotal`, `LeakGrowth`, `SteadyGrowth` and `LargeAllocation` findings are replaced (a large single allocation becomes
an evidence item of the verdict); no compatibility shim (Paddock before v1). Where this section and ADR 0068 differ,
ADR 0068 wins and this ADR follows.

### D8. Context: Private Bytes, logs, dumps, .NET

- When the project holds Private Bytes for the same process (a `.blg` or a process monitor capture, matched by pid
  and time), the engine compares its growth over the series' span with the NT heap's: much larger Private Bytes growth
  gives `NotNtHeap` ("about 1.2 GB of the 1.5 GB growth is not on the NT heap: `VirtualAlloc`, the .NET GC heap, or
  mapped files"). Without such data, a could-not-check line.
- When the runtime modules show a .NET process and the culprits sit in the runtime, `ManagedHeap` points to the heap
  snapshot tools Debrief already reads (`.gcdump` is not read, `docs/profiling.md:201`; a `.heapsnapshot` or a dotnet
  heap capture is).
- Each snapshot stays a timeline marker (it is today); a finding's evidence links to the Moment view around each
  snapshot's time and to the dump ADR's memory check for a dump of the same process.

### D9. Report blocks, export and Pitwall

- A finding is a report block (the approved report blocks and citations items): the verdict, the class, a small
  series chart per call site, and its citations, rendered by the engine. The growth table exports to CSV with the
  series' snapshot names and times.
- Pitwall (ADR 0059) explains the series from the computed facts only: the findings, the top `explain_rows` (default
  20) call sites with their classes and numbers, and the could-not-check lines, bounded as `explain.rs` is today.
  It never receives the logs. The engine checks that every call site and number the answer cites exists in the facts
  (rule 46: the LLM explains, the engine computes).

### D10. Where it runs, and its bounds

The reader stays in the dumps plugin (sandboxed), moved to ADR 0064's line loop by SI-11, and keeps per-back-trace
aggregates only (never per-allocation records). The series is built in the engine as a job (ER-17, ER-18: phases
*reading snapshot i of n*, *matching call sites*, *trends*, *symbols*, each with its own count; Cancel between
snapshots and between symbol modules; the old result stays until the new one is complete). Bounds: `max_series` 64
snapshots, `max_stacks` 200,000 per log (as today), interned frames at most 1,000,000 per series, histograms 48
buckets, Theil–Sen only on `trend_candidates`. Every cut is reported with ADR 0065's `Bounded`.

### Parallelism and cost

- **Reading**: one snapshot per plugin call; up to `min(cores - 1, 2)` at once (each may hold a 1 GiB log's line
  buffers in the plugin until SI-11; after SI-11 the plugin holds aggregates only, so the bound becomes cores minus one,
  a Setting with Auto) [J].
- **Matching**: interning turns today's joined `String` keys (up to 64 frames × up to 1,024 characters each, per
  stack, `limits.rs:41-42`) into `u32` slices; the hash join over snapshots is O(total stacks). This is the cheaper
  operation of ER-20: it removes the per-stack key strings that dominate memory today [J; to be measured, see
  Validation].
- **Trends**: least squares is O(stacks × snapshots); Theil–Sen is O(k × n²) for k = 2,000 and n ≤ 64, about 8 M slope
  computations, parallel by call site with rayon at background priority (bulk work, rule 52: utility QoS) [calc].
- **Symbols**: per module, bounded by ADR 0036's `concurrent` Setting; cached by identity.
- **Caching**: the series result is cached per (log ids, Settings hash), replacing the one-pair cache; a Settings change
  re-runs only the trend and findings phases.

### Logs (ER-27)

One line per phase with duration and counts (`umdh.series id=… phase=match stacks=… call_sites=… ms=…`), the reason
for every split or refusal, each module's symbol outcome, and the series' bounds hit. No frame names or paths in logs
by default (they may name the customer's code); module counts and hashes instead.

## Consequences

- An investigator gets a ranked list of call sites with a class, a rate, the block size and a confidence, and knows
  what was not checked; two logs still work but say what they cannot prove.
- The model, the wire (`dumps.ts`) and the UMDH view change together (no shim). The Growth and Tree tabs stay, fed by
  the series (the tree by call site or by stack).
- ADR 0036 §2 changes as D4 says; rule 55 gets one frame classifier for profiles and UMDH.
- The `-g` (unreferenced blocks) output is not read in this ADR [U: its exact shape is not documented in the pages
  read]; it is the strongest leak evidence UMDH gives and is a follow-up. A real sample may only come from a test
  program run in a local VM (ER-14; never a user's log), and the fixtures are then invented in that shape.

## Failing tests first (ER-11; invented files only, ER-14)

All inputs are generated by a new test helper `debrief_dump::testing::umdh_series(spec)` (snapshots as text, from a
spec of call sites with a growth shape: steady, plateau, one step, noise, shrink; sizes; frames named or raw; a
module list), never from real logs.

1. `aggregated_entries_count_allocations`: a snapshot line `00005320 bytes in 0x14 allocations (@ 0x00000428) by:
   BackTrace00B53` must read as count 0x14, 0x5320 bytes and a per-allocation size of 0x428 (largest 0x428, the size
   histogram holding 0x14 × 0x428); today the reader counts it as **one** allocation of 0x5320 bytes
   (`umdh_reader.rs:284-291`) [S].
2. `unindented_frames_are_kept`: frames that follow an entry line without indentation are the stack. Today they are
   dropped (`umdh_reader.rs:300`), so the stack has no frames: no culprit, no names, no grouping by module or function,
   and the key falls back to `#<id>` (`umdh.rs:39-41`). Within one process instance that id is stable, so the
   assertions are about frames, culprit and grouping, not about matching [S].
2b. `same_id_after_a_restart_is_not_merged`: two snapshots of different instances (D1) where `BackTrace00A1` is a
   different stack in each: they must not be merged under one key, whatever their frames' presence.
3. `one_flat_step_is_classified_steady`: 6 snapshots, one call site +5 MiB per step except one flat step. Today
   `LeakGrowth` (critical) still fires, but `SteadyGrowth` does not (`umdh.rs:383`, strict `>`), so the series' shape
   is lost; the series must classify it *steady growth* with a rate.
4. `a_warming_cache_is_not_a_leak`: +10, +10, +0.1, +0.1, +0.1 MiB: *growth then plateau*, verdict `CacheWarming`, not
   `Leak`.
5. `many_callers_one_call_site`: one culprit reached from 12 callers, each +0.5 MiB (under `LEAK_BYTES`) and about
   8 % of the growth (under `LEAK_SHARE`): today no `LeakGrowth` fires (`umdh.rs:340`); grouped by call site it must be
   one call site with 100 % of the growth.
5b. `static_crt_frames_in_the_user_module`: the documented stack (`ntdll!RtlDebugAllocateHeap`, …,
   `MyApp!_heap_alloc_base`, `MyApp!_nh_malloc`, `MyApp!operator new`, `MyApp!DisplayMyGraphics`, `MyApp!main`, from the
   [Using UMDH](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/using-umdh-to-find-a-user-mode-memory-leak)
   page, names invented in that shape): the culprit is `DisplayMyGraphics` (or `operator new` with the function as its
   caller), never `_heap_alloc_base`; the statically linked CRT allocators are recognised inside the user's module.
5c. `two_logs_never_give_a_leak`: any two snapshots give `Grew` with `NeedsMore { of: Snapshots, have: 2, need: 3 }`,
   `NoGrowth`, `NotNtHeap` or `CannotTell` (proptest over random pairs).
6. `weak_times_give_no_rate`: modification times only: no per-hour rate, a could-not-check line.
7. `restart_splits_the_series`: the main executable's base changes between snapshots 3 and 4: two series, and the
   reason. `dll_load_does_not_split`: a DLL appears or moves between snapshots, the executable's base stays: one series.
8a. `inferred_identity_is_marked`: a PE of the same path and size in a symbol folder, instance confirmed: names marked
   *inferred identity*; instance unconfirmed: no names and `InstanceUnconfirmed`.
8. `raw_frames_named_by_identity`: raw addresses plus a synthetic minidump of the same process (from
   `debrief_plugin_dumps::synth`) with matching modules: frames named from a synthetic PDB; without the dump:
   `module+offset` and `Unnamed` with the share.

Properties (proptest): never panics on any text; series results are identical for any input order of the same timed
snapshots and for 1 or N threads; every finding's evidence numbers recompute from the series.

## Validation (rule 48)

Before U2 lands: a results block in `docs/performance.md` with peak memory and time of today's `growth` against the
interned series, on generated series of 2, 8 and 64 snapshots × 20,000 and 200,000 stacks × 32 frames, on a quiet
machine. Acceptance: the 64 × 200,000 series within 1 GiB in the engine and under 30 s on the M4 Pro [J target], or the
bounds are lowered with the numbers.

## Work packages

| # | Package | Days | Depends |
|---|---|---|---|
| U0 | `umdh_series` generator and the failing tests 1–8a (marked) | 2 | - |
| U1 | Reader: aggregated entries (with their `@` size), unindented frames, size histogram and top sizes, process name when present. Lands **with SI-11** on its line loop, or before it with the tests carried into SI-11's change (the reader is the one SI-11 replaces) | 2 | U0; SI-11 |
| U2 | Frame interning, `UmdhSeries`, instance check and restart split (D1, on the shared `ProcessInstance`), cache by ids and Settings; the measurement above | 3 | U0; `ProcessInstance` (roadmap FD-P) |
| U3 | Identity levels and the shared frame classifier with `debrief-profile`: both callers in one commit, or a `docs/known-copies.toml` entry until they are (rule 55) | 2 | U2 |
| U4 | Trends: slopes, classes with D3's minimums and precedence, ranking, Settings page section | 3 | U2 |
| U5 | Times with provenance (file name through ADR 0066's reader, manifest, weak mtime) | 1 | U2; ADR 0066 Z1 |
| U6 | The check report in ADR 0068's shape: verdicts, confidence rules, Not checked list | 2 | U4, ADR 0068's types |
| U7 | Symbols ladder (dump of the same instance, manifest identities, inferred PE, ADR 0036 lookup) | 2 | U2 |
| U8 | Context: Private Bytes comparison, .NET hint, Moment and dump links | 2 | U6 |
| U9 | UI: series header with times and provenance, call-site table with class chips and sparklines, sizes, stack panel, settings; mock backend | 3.5 | U4, U6 |
| U10 | Report block, CSV export, citations | 1.5 | U6; report blocks and citations items |
| U11 | Pitwall explanation over facts with the engine's check | 1 | U6; ADR 0059 |
| U12 | ER-27 log lines, docs (features, ADR 0036 amendment note), user guide | 1 | all |
| | **Total** | **27** [J] | |

The first useful cut is U0–U4 + U6 + U9 (about 18 days): series, call sites, classes and honest findings.

## Decisions for the user

| # | Decision | Recommendation |
|---|---|---|
| D-a | Release | **1.3**, after a large folder opens cleanly (the user's 2026-11-07 priority). In 1.2 only U0 and the two reader fixes (tests 1, 2 and 2b, about 2 days), landing with SI-11 or with their tests carried into it, since they are wrong results today and touch no shared code |
| D-b | Default grouping | Call site (culprit + 2 callers); Function and Stack one click away |
| D-c | Symbols only from the same process instance: a dump or manifest of it, or a PE of the same path and size marked *inferred identity* (D4) | Yes; still never by name alone |
| D-d | Thresholds as Settings with today's constants as defaults | Yes |
| D-e | Private Bytes comparison when a `.blg` or monitor capture of the same pid exists | Yes; it answers "is it even the NT heap" first |
| D-f | `-g` unreferenced-block output | Later; a real sample only from a test program in a local VM, fixtures invented in its shape |
| D-g | Minimum snapshots and caps (D3, D10): no leak verdict from two logs; `Low` at 3, `Medium` at 4–5, `High` from 6; at most 64 snapshots, 200,000 stacks per log, 1,000,000 interned frames | Yes |
| D-h | The process instance check (D1) on the shared `ProcessInstance` of the roadmap's foundation | Yes |
