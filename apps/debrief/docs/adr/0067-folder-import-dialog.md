# 67. The folder import dialog: a quick scan, one judge, a tree grid and a selection by folder

- Status: Proposed, revision 2: answers the independent review of `e2402491e` (three P1s, the P2s)
- Date: 2026-11-05
- Related: ADR 0014 (database-first engine), ADR 0047 (time window), ADR 0048 (watched folders), ADR 0050 and 0054 (dialog registry and kinds), ADR 0051 (streaming discovery, stable source ids), ADR 0063 (binary wire); three proposed ADRs on branches, not yet on this one: ADR 0064 (one streaming import trait, revision 3, `docs/debrief-streaming-trait` `c12864566`), ADR 0065 (bounded replies, `docs/debrief-bounded-and-timezone` `1eaa14850`), ADR 0066 (how Debrief reads times, same branch, `2f289b4c3`); family ADRs [0018](../../../../docs/adr/0018-bounded-loading-everywhere.md), [0019](../../../../docs/adr/0019-limits-explain-themselves.md), [0043](../../../../docs/adr/0043-dialogs-size-to-content.md); Paddock rules 17, 18, 21, 39, 40, 42, 47, 48, 51, 52; ER-17, ER-18, ER-19, ER-20, ER-21, ER-23, ER-27; [bundle shapes note](../../../../docs/research/2026-11-debrief-bundle-shapes.md) (`origin/docs/debrief-bundle-shapes` `fc6bdf0b7`)
- Layout: [contact sheet](../images/folder-import-dialog/contact-sheet.png) and one image per state and theme in the same folder; the drawing's review is separate (ER-23)
- Evidence marks: **[M]** measured, in [this results block](../performance.md#2026-11-05-folder-scan-on-invented-files-adr-0067); **[S]** read in source at `origin/debrief/1.2-int` `852b35ac0`, paths below `apps/debrief/`; **[N]** from the bundle shapes note, not re-run; **[R]** from the folder-open reproduction on `docs/debrief-folder-repro` `875827467` (its results block "2026-11-05, opening a 7.6 GiB diagnostic-bundle folder"), not re-run; **[J]** judgement; **[U]** not verified. Days are judgement.

## Answers

| Question | Answer |
|---|---|
| What does opening a folder do today? | Imports everything at once, with no question. Each refusal becomes its own row. Nothing can be chosen beforehand ([what exists today](#what-exists-today)). |
| What can be extended? | The walk (`discover.rs`), the detection steps inside `import_source`, the job and progress machinery, `fingerprint`, the shared journal and `pd-table`. No existing dialog picks files. |
| How can the dialog never promise what the import refuses? | The import and the scan call one new function, `ingest::judge`, on the same bytes. Only the failures that happen after the head can differ, and those are listed, worded honestly and reported when they occur. |
| Is the scan quick enough? | **Not yet with plugins.** In-process, 14,000 invented files are listed in about 23 ms and judged in 4.4 to 4.5 s at 8 threads, warm [M]. Plugin sniffs add about 4.5 s at 8 threads today, or about 1.6 s after ADR 0064's SI-6, so 6.0 to 6.1 s even then [calc]. 95% of the in-process judge is deciding that binary files are binary, which F1 makes cheaper. The budget (section 3) is per machine class and holds only once **both** F1's pre-check and SI-6 land and are measured; neither is measured yet. |
| Which component? | The shared `pd-table`, which is already a Material tree grid with CDK virtual scroll. Six precise changes, after the Dyno tag ([gaps](#pd-table-what-fits-and-what-is-missing)). No new component. |
| How is `MAX_QUERY_SOURCES` avoided? | Three ways: the UI sends a scan id and node ids, never paths; the selection is stored as path prefixes (9 rules for the invented bundle's recommended set, with a stand-in rule set); a query can name a folder instead of its source ids. |
| Removal and undo by folder? | One bulk command, one `maintain` at its end, one journal entry. Undo reads the files again, and says so. |
| Does it open the 7.6 GiB bundle? | **Only once memory is judged, not size, and reserved across files.** The reproduction stopped the 2,048 MB store worker on a 1.43 GiB EVTX and on a 230 MB CSV export; with the caps lifted, an EVTX needed about 8.6 MB of worker memory and 10.4 MB of plugin memory per MB, and the registry export about 20 and 22 [R]. So the dialog judges each file's need from measured factors, and a [shared whole-file gate](#the-whole-file-gate) reserves that need before any whole-file input starts, so parallel files cannot add up past the worker's cap. Files that cannot fit start unchecked. F12's acceptance is the reproduction's own worker tests, run on exactly the recommended set: Enter on the invented 14,255-file bundle imports and the worker does not stop. |
| Does a zip show the same tree? | Yes, built from its central directory without expanding it. A bundle in a zip is a folder in a file, so a single dropped zip with more than one member also opens the dialog: this bends the user's "a single dropped file skips it" and is decision D1. |
| What did this ADR find that was not asked? | (1) `rowActivate` fires on click as well as Enter, so binding Enter to import would import on a row click. (2) Memory, not size, decides whether a file fits: the importer's measured memory per input byte against both the plugin cap and the store worker's budget, which shrinks to nothing as ADR 0064 makes inputs stream. (3) 95% of the judge's time goes into deciding that binary files are binary, in code the import shares, so one fix speeds up both ([section 4](#4-parallelism-and-cost-er-20-rule-51)). (4) Modeless `tool` is the only kind that fits a large user-sized window, and rule 17 asks pickers to be modal: decision D2. |

## What exists today

Read at `852b35ac0` [S] unless marked.

**The path of a folder open.** `pickAndOpen(true)` (`app/src/app/core/workspace/workspace-data.store.ts:219`) asks `pick_paths` (`app/src-tauri/src/paths.rs:533`, which grants each chosen path to the window), then `open_paths` (`workspace_commands.rs:219`). `open_request` (`:197`) caps a request at `MAX_OPEN_PATHS`, 1,024 paths, and checks each one against the window's grants or the recent list. `start_open_at` starts an import job; `open_in_job` diverts diagnostics reports (`activity/report/open.rs:450`) and calls `store.open`, which canonicalises, pushes the paths to `s.roots` and runs `ingest::import_stream`. The walk (`ingest/discover.rs`) streams files in sorted order, applies the profile's `include` and `exclude` globs, `removed` and `known`, and calls a probe that opens each file to count zip entries (`parallel.rs:123`). It does not follow symbolic links: neither `is_dir` nor `is_file` holds for a link from `DirEntry::file_type`. So nothing is ever asked, and everything except `.exe`, `.dll`, dot files and Debrief's own files is read and judged (`default_profile.toml:22-23`).

**Where the verdict is made today.** Inside `import_source` (`ingest/source.rs:195` onward), in this order: empty head; `score_text_with`; `ParserSet::choose`; `plan_with`; then `begin_source` and the read. Reasons are plain strings. The head is at most `HEAD_BYTES`, 1 MiB (`plan.rs:13`). Encoding detection looks at 64 KiB (`encoding.rs:17`), the parser score at 256 KiB (`plan.rs:126`), importer choice at 64 KiB of characters (`plan.rs:19`), and the text format at 200 lines of what the head holds (`importers/text.rs:15`).

**Other open entry points.** The recent list (`shell/menu/recent-opener.ts:17`), drops (`core/dropped-files.ts`), launch paths (`lib.rs:583`), `open_paths_live` (`lib.rs:193`), project restore (`debrief-store-duck/src/lib.rs:1270`) and `reload` all reach the same import.

**Other walkers.** `watch::scan_folder` (`watch.rs:301`, its own limits, 50,000 entries and depth 16), the tail's new-file search (`workspace/tail/mod.rs:358,412`) and `config/snapshot.rs:127`. This ADR adds no fifth: the scan uses `discover`'s listing.

**Existing dialogs near this.** `import-range` (a time range, ADR 0047), `open-ask` (which window), `open-live`, `folder-setup` (an AI wizard that surveys what was already imported), `drop`, `paste`. None lists files or selects them. Nothing to extend, but the new dialog must offer `import-range` as a next step rather than replace it.

**Removal.** `remove_source` (`debrief-store-duck/src/lib.rs:1217`) takes one id, runs `maintain` each time and adds one path to `s.removed`. At 61 ms each [N], removing 2,793 sources one by one would take about 170 s [calc]. There is no bulk remove and no undo. `WorkspaceState` (`debrief-core/src/api.rs:493`) persists `roots` and `removed`.

**`MAX_QUERY_SOURCES`.** 10,000 (`crates/debrief-core/src/query.rs:92`), checked at `:140`. It guards a request from the webview. The `Expr::Sources` set behind it has no limit of its own (`:188`).

**Bundle shapes [N].** 14,255 files, 8.17 GB. 2,793 import, 11,458 are refused and each becomes a row; the summary carries 4.4 MB of JSON on each update. Three over-cap EVTX quarantine the plugin, and the next dump then reads `binary file`.

**Numbering.** 0063 is on `docs/debrief-binary-wire`, 0064 on `docs/debrief-streaming-trait`, 0065 and 0066 on `docs/debrief-bounded-and-timezone`. This ADR takes **0067**. Should another ADR take 0067 first, renaming this file, its heading and the results block's title is the whole fix.

## Decision

### 1. When the dialog appears

| Entry point | Behaviour |
|---|---|
| Open folder (menu, Home, Explorer), a recent folder, a dropped folder, a folder on the command line | Dialog, always. Enter imports the recommended set, so the open stays one step. |
| A single dropped or picked file | No dialog |
| A single dropped or picked zip with more than one member | Dialog (D1) |
| Several paths with at least one folder | One dialog with one top row per folder or zip; plain files in the request import at once, as today |
| Project restore, `reload`, `refresh_tail`, a watched folder | No dialog: they use the stored selection ([section 8](#8-the-selection-and-the-import)) |
| `open_paths_live` on a folder | Dialog first, with the live choice carried through to the import. [J] |
| `debrief-cli`, tests through `open_paths` | No dialog. `open_paths` stays a direct import; the dialog is an explicit first step in the UI's open flow. |

The check "is this a folder or a multi-member zip" is the engine's: a new command `open_plan(paths)` returns `{ direct, choose }` after `open_request`'s checks, and the store acts on it. The UI never decides from a name.

### 2. One judge

A new function in `ingest/plan.rs` holds the steps now inside `import_source`, without changing them:

```rust
/// What the import would decide from a file's first bytes. The import and the folder scan both call this; nothing else
/// calls `ParserSet::choose` or `plan_with` for a whole file.
pub fn judge(env: &JudgeEnv, file: &FileMeta, head: &[u8], whole: bool) -> Result<Judged, Refusal>;

pub struct Judged<'a> { pub choice: Choice<'a>, pub plan: FilePlan, pub limits: Limits }

pub enum Refusal {
    Empty, Binary, NoParser, NoImporter, NoFormat,
    PluginStopped { plugin: String },   // not "binary file" after a quarantine: bundle note, "Misleading reason"
    Locked { detail: String }, Unreadable { detail: String }, Encrypted, // the last for an archive member
}
```

`Refusal`'s `Display` gives today's strings, so reasons shown after an import change only for `PluginStopped`. `import_source` becomes: read the head, `judge`, then read. `choose` and `plan_with` become `pub(crate)`, and a conformance check fails on a new whole-file caller outside `judge` and `ingest/region.rs` (which plans a region, not a file).

**The status a row shows** is computed by the engine from `judge` plus the file's size:

| Status | Words and glyph | Meaning | From |
|---|---|---|---|
| Pending | "Scanning…", three dots | Not judged yet | The table |
| Supported | "Supported", check in a circle | A parser reads the head, with time | `Ok`, no limits |
| Limited | "Limited", triangle, with the reason | It imports, with a stated loss | `Ok` with `Limits` |
| Not supported | "Not supported", circle with a slash, with the reason | The import would refuse it | `Err(Refusal::{Empty, Binary, NoParser, ...})` |
| Can't read | "Can't read", exclamation in a circle, with the reason and Retry | The OS refused | `Err(Refusal::{Locked, Unreadable})` |

Status is never colour alone (AGENTS.md): each has its own glyph. Folders roll up counts per status, for example `2 / 3 / 1` with the three glyphs, and the full words in the tooltip and accessible name.

**Limits** are computed from the head and the manifest, never from a list of names:

| Limit | How it is known | Words |
|---|---|---|
| No time | The plan's format has no time, or an untimed CSV shape | "Imports without time" |
| Time zone unknown | ADR 0066: the head gives `Stamp::Wall` and the root's `LogZone` is `AsWritten`, or its confidence is Low so it is not applied. **Not a limit once the zone is known** | "Time zone unknown: times as written", ADR 0066's own words. The row's action is Choose |
| Date from the file time | A time-only format, ADR 0066 section 9 | "No date in the file: date taken from the file time" |
| Day order undecided | ADR 0066 section 7: `a/b/yyyy` stamps in the head that do not decide. A later row may still decide it | "Day order not decided from the start of the file" |
| Over a memory limit | `size` times the importer's memory factor, on each side, is over what that side can ever give one file: the plugin process cap (`performance.plugin_memory_mb`, 1,024 MB by default) or the [whole-file gate](#the-whole-file-gate)'s capacity in the store worker (1,280 MB by default) | "Needs 10 GiB, limit 1 GiB", with the side (plugin or store worker) named in the tooltip; "Needs too much memory" in the totals |
| Over another limit | Each limit in the registry of family ADR 0019 that a size can decide (`MAX_ENTRY_BYTES` until ADR 0064 retires it) | The limit's own message |

#### Memory per input byte

Revision 1 compared a file's size with the plugin cap. The cost is a multiple of the size, on two sides, and the runs that were stopped only show a lower bound [R]:

| File (invented, same shape) | Side | Peak | Per input MB |
|---|---|---:|---:|
| 1.43 GiB EVTX, default caps | Store worker (whole read, then the guarded call's copy) | 2,914 MB, stopped at the 2,048 MB cap | at least 1.9 |
| 1.43 GiB EVTX, caps lifted (stream-text, memory-mapped binary files, no guard copy) | Store worker | 13,140 MB | about 8.6 |
| Same run | Plugin | 16,013 MB | about 10.4 |
| 460 MB UTF-16 registry export, caps lifted | Store worker | 9,453 MB | about 20.5 |
| Same run | Plugin | 10,088 MB | about 21.9 |
| 230 MB semicolon export, default caps | Store worker (whole-file CSV adapter) | 3,102 MB, stopped | at least 13.5 |
| Same file on `debrief/stream-text` | Store worker | 820 MB | about 3.6 |
| Three 296 MiB dumps, in parallel | Store worker | 1,923 MB together | about 2.1 [calc] |

So each importer declares, in a data file `importers/memory.toml` (rule 21), its measured peak per input byte on the worker side and the plugin side, keyed by importer id and input kind, with the commit and results block it came from (rule 48). A factor from a stopped run is marked `at_least` and is never used as if it were the cost. An importer whose input streams (ADR 0064: `Lines`, `Decoded`, `Bytes`, `Seekable` without `read_whole`) is bounded by the record budget and has no row. **A whole-file input with no measurement gets the worst measured factor, 22, on both sides.** With the gate's 1,280 MB that means such a file over about 58 MB starts unchecked, so an unmeasured format cannot stop the worker. The tooltip then says "not measured". Each ADR 0064 package that makes an input stream deletes its row in the same change, and a test checks the rows against the declared input kinds.

A file that does not fit is **Limited and starts unchecked**, with the reason. A person can still check it; the cost strip then warns "Needs 10 GiB in the plugin: the plugin will be stopped", and the file is refused at the gate rather than run.

#### The whole-file gate

This is the memory gate the folder-open reproduction asked for, and **this ADR defines it**; ADR 0064 has no such gate at revision 3, and its next revision should reference this section for its `read_whole` exceptions rather than define another one.

- One shared byte budget in the store worker, `performance.import_whole_file_mb`, by default `store_cap_mb - import_budget_mb` = 2,048 - 768 = 1,280 MB. `import_budget_mb` (768 MB, ADR 0014's tested import budget) stays what streaming inputs use; the gate is the rest.
- Before a whole-file input starts (a `read_whole`, a legacy whole-file importer, an archive member that must be spooled), the dispatcher reserves `size x worker factor` from the gate. If it does not fit, that file waits in a queue and the next streaming file goes ahead, so the pool stays busy. The reservation is released when the input ends, by success, failure or cancel.
- A need larger than the whole gate is refused at once, with the Limited reason, never queued forever.
- Each plugin process has the same gate on its own cap, keyed by process.
- Progress names the wait: "Waiting for memory: 2 large files".
- The dialog checks the **sum**: the total reserved need of the checked whole-file inputs is shown when it is over the gate ("3 large files will be read one at a time"), and it lengthens the time estimate. It refuses nothing, since the gate queues.

The claim that the default set imports without stopping the worker is **not made until F12 runs exactly the recommended set**: the reduced folder of the reproduction already peaked at 1,830 MB of 2,048 with no gate [R], and its three dumps alone took 1,923 MB.

**Reading times on the head uses ADR 0066's `TimeReader` and `TimeTally`, not a second parser**, so "imports without time" and "times as written" are that ADR's facts on fewer lines. After ADR 0066 lands, the row's Limited reasons and the import's `TimeProvenance` come from one type. Until then the scan reads `plan.kind` and the parser's `assume_timezone`, and F2 moves over when ADR 0066's Z packages land. If 0066 is not accepted, the row "Time zone unknown" reads "Local clock read as UTC" and nothing else changes.

**What a quick scan cannot know, and what it says.** Nothing here is a guess from a file name: each item below is a fact about the rest of the file that the head does not hold.

| Cannot know from the head | Example | What the dialog does |
|---|---|---|
| A header past the sniff window | The note's 329-line state header (`st05a`): the import refuses it too | Same verdict as the import. The reason says what was looked at: "No parser reads the first 1 MiB" |
| Encoding further in | CP932 after 64 KiB of ASCII (`enc5c`) | Supported. The row's tooltip and the Status filter help text: "Checked from the start of the file." |
| A time format that changes later, or a later row that contradicts the head | A log whose stamps change after a restart; a day order the head left open | Supported. The import reports it: `UnmatchedLine` today, ADR 0066's typed problems and **Times…** later. A time problem is not a broken promise: the file imported |
| Events exist at all | A CSV header with no rows: "importer found no events" | Supported until read. Reported as a [broken promise](#when-the-judge-and-the-import-disagree) |
| A plugin runs out of memory or crashes on content | A corrupt EVTX | Supported until read; reported the same way |
| A file changes between scan and import | Still being written | The import judges again from its own head, and the import result wins |

The words are the same everywhere: the dialog's header reads "Checked from the start of each file. Some problems only show during the import." This line sits in the help popover of the Status filter and in the import result, not on every row. "Supported" means "a parser reads its start", and the status tooltip says exactly that. The UI never says "will import".

#### When the judge and the import disagree

A file marked Supported or Limited that fails is, by the user's rule, a bug. Two things follow from it.

**Test.** `crates/debrief-plugins/tests/it/judge_agreement.rs`, the crate that sees every built-in plugin over real IPC and where ADR 0066 puts its `time_corpus` and `shapes.toml`, so one list of invented shapes serves both. It runs over all `samples/` files, that corpus (the bundle note's `st*`, `ts*`, `enc*`) and generated files for each importer. For each file it runs the scan path and then `import_path` and asserts:

- Not supported or Can't read: the import refuses with the *same* `Refusal`.
- Supported or Limited: the import succeeds, or fails with a kind from a closed list `AfterHead { NoEvents, PluginStopped, Crashed, Budget }`, and the case is listed by name in the test's expectations. A new case fails the test until someone names it, so the honest list above stays complete.
- Limited for time: the stated loss holds. "Imports without time" means no timed event; "times as written" means `wall` rows in ADR 0066's `TimeProvenance.counts`, or stamps with no offset until that lands.

A property test in the fuzz harness feeds random heads and checks the same implication without plugins. Because both paths call one function, the test watches the *inputs* (same head length, same `whole`, same env), which is where they could drift. It must also fail when the two are made to differ, which is checked once by hand and recorded (rule 71, ER-33).

**At run time.** The scan's verdict for a file goes with it to the import. A file that reads `Outcome::Unrecognized` after a Supported or Limited verdict increments `ImportReport.broken_promises` and is kept with its verdict and actual reason. The import's result then says "2,458 files selected: 2,456 imported, 2 failed although the scan could read their start" with a link to the two and their reasons, and each is logged at warning level (section 13). A failure of the same `Refusal` kind as the verdict, or a file that changed since (`size`, `mtime`), is not counted as broken.

### 3. The scan

A job of a new kind (`JobKind::Scan`), off the UI thread, visible in the Jobs pane, cancellable in every phase, never timed out by the UI ("Still working" after the stall judgement of family ADR 0031).

| Phase | Label | Scale | What it does | Tree meanwhile |
|---|---|---|---|---|
| 1 | "Listing folders" | Growing count, no total | `discover`'s walkers: `read_dir`, one `stat` per file, no file opened | Folders and files appear, with size and file count |
| 2 | "Detecting file types" | `done` of `total` files, per its own scale | Up to `HEAD_BYTES` per file, then `judge`, on a bounded pool; one open per file | Type, status and check fill in, by folder order, and also the part in view first |
| 3 | "Looking for the time zone" | Budget, per ADR 0066 | ADR 0066's probe, using this table to pick its files instead of walking again. It gives up with `not measured` and never holds up the rest | Zone chip on the path line; "Time zone unknown" rows change |
| 4 | "Checking duplicates" | `done` of `total` candidates | The rule pack over the table, and `fingerprint` only where a rule needs it and sizes are equal | Flags appear |

Rows are usable from the first listing: expand, sort, filter and check all work. "Rows in view first" means the pool takes the next files from what the view's window shows when that changes, so a person looks at a folder and sees it finish first. A person's check on a Pending row is kept: a verdict later does not overwrite it, unless the file turns out to be Not supported, when the row becomes unchecked with the reason.

**Zone.** The path line carries ADR 0066's chip once its probe has an answer, in that ADR's words and rules: quiet at High, "check" at Medium, Choose at Low or Unknown. `folder_import` records the dialog's root in `import_roots`. A root is whatever the dialog opened, so a choice of subfolders does not change which zone applies, and the longest-prefix lookup of `zone_settings` and of this ADR's selection rules are one helper over one `RelPrefix` type, not two (rule 55).

**Enter during the scan.** "Import selected" stays enabled; the footer says "Import selected starts when the scan ends". The dialog closes and the job goes on: scanning, then importing, one job with both phases. It stays one step, and the file count of the recommended set is settled before the first source id, which also keeps ids in sorted order (ADR 0051).

**Bounds.**

- `folder.scan_entries`, a Setting and a row in ADR 0065's `debrief_core::limits`, default 500,000 entries: about 33 MB of table at the measured cost per node. The listing is a `Bounded::first`: above the limit it stops, the `Cut` says `shown`, `Total::AtLeast` and `Next::Narrow`, and `<pd-limit-note>` draws the [over-the-limit state](../images/folder-import-dialog/cut-dark.png) in its fixed slot ("Looked at the first 500,000 of more than 500,000 entries"), with this dialog's own line that what is not listed is not imported and a button to open a subfolder. It never counts on just to print a number (ADR 0065).
- No depth limit in the listing, as `discover` has none today; `watch.rs`'s 16 stays its own.
- At most `2 x threads` heads in memory.
- Symbolic links are listed by count ("3 links not followed") and never followed, same as the import.

**Target and measurement.** On the invented 14,000-file corpus of the [results block](../performance.md#2026-11-05-folder-scan-on-invented-files-adr-0067), on a loaded Mac mini (external SSD, warm cache, in-process importers only):

| 14,000 files, 7.56 GiB, 1 MiB heads | 1 thread | 4 | 8 | 13 |
|---|---:|---:|---:|---:|
| Listing and stat, serial | 22-23 ms (417 ms cold) | | | |
| Heads read, no judging | 438-443 ms (19.0 s cold) | 152-184 ms | 105-111 ms | 125-129 ms |
| Heads read and judged | 33.9-34.2 s | 8.6-9.3 s | 4.40-4.51 s | 3.05-3.26 s |
| Judge per file, p50 and p99 | 3.37-3.44, 4.60-4.63 ms | 3.44-3.45, 4.64-4.70 ms | 3.48-3.49, 4.74-4.95 ms | 3.66-3.68, 7.05-10.6 ms |

The verdicts were the same in every run: 1,953 CSV logs, 773 text logs, 7,458 binary (EVTX, dumps and zips among them, as no plugin and no archive step ran), 3,770 with no parser, 46 empty [M].

**Budgets, per machine class**, for 14,000 files on a local SSD, warm (rule 16). The thread count is `ingest::default_threads`: CPU threads minus one, clamped to at most 8 (`ingest/parallel.rs:30-35`) [S], so a 4-core laptop gets about 3.

| Machine class | Judge threads | First rows | Listing | All verdicts | Today, in-process [M] | Today, with plugins [calc] | After F1 and SI-6 [calc] |
|---|---:|---:|---:|---:|---:|---:|---:|
| Desktop, 8 or more cores | 8 | 250 ms | 1 s | 5 s | 4.4-4.5 s | about 9 s | about 2 s |
| Laptop, 4 cores | 3 | 250 ms | 1 s | 10 s | about 11-12 s (4 threads: 8.6-9.3 s) | about 23 s | about 5 s |

The last column assumes F1 removes most of the 31.5 s of binary decoding, so the in-process judge drops to about 3 s of thread time (`choose` 0.2 s, `plan_with` 1.3 s, the text files' scoring 1.6 s [M, step split]), and SI-6 brings plugin sniffs to about 13 s of thread time [ADR 0064, calc]. **Neither is measured.** So the budgets are a target that F1 must meet before F4 closes: F1 measures its saving with an A/B on this corpus, and F12 re-runs this table at 3, 4 and 8 threads with plugins. If either misses, the budget is restated in this ADR rather than met by a smaller head. Numbers for a cold cache, a spinning disk and a network share are **not measured** [U]: each file costs one `open` and one read, so a share at 2 ms per round trip adds about 7 s for the opens alone at the default pool (network: few per host, ER-20, so `Auto` picks 4 for a remote root). Progress keeps moving, and there is no timeout, so slow is honest rather than broken.

### 4. Parallelism and cost (ER-20, rule 51)

**More threads.**

| Work | Kind | Bound | Priority |
|---|---|---|---|
| Listing | Waits on the disk | `discover`'s walkers: `(threads x 2).clamp(4, 16)` as today | `QOS_CLASS_UTILITY`, nice 10 or `THREAD_PRIORITY_BELOW_NORMAL` (bulk, rule 52) |
| Heads and `judge` | Mostly small reads, CPU for the judge | `import.parse_threads` when local, 4 when the root is remote; a Setting with "Auto" | Live work: a person waits on it. Normal on Windows, may be `UTILITY` elsewhere (rule 52) |
| Plugin sniffs | IPC | ADR 0064: one call per plugin per file, one head sent once | As above |

The judge scales with threads up to 8 (34 s, 8.9 s, 4.5 s) and less beyond (3.2 s at 13, where p99 rises to 7-11 ms) [M]. Its mean is 2.4 ms a file (33.3 s over 14,000) and its p50 3.4 ms, because more than half the files are binary and pay the decoding [M]. So `Auto` for a local root is `parse_threads`, at most cores minus one, and nothing is gained by more walkers: listing is 23 ms.

**Cheaper.**

- **No second open per file.** `source_slots` opens each file for 4 bytes and `fingerprint` opens it again (ADR 0064 section "Per-file costs at 14,000 files", fix 3). The scan does one open and takes the zip check from the head.
- **Deciding "binary" is the cost, and it is cheap to fix.** 95% of the judge's thread time is `score_text_with` on the 7,458 binary files: 4.3 ms each at p50, 31.5 of 33.1 s [M, step split]. It runs encoding detection and decodes up to 256 KiB before `looks_binary` can say no. `choose` takes 0.2 s for the whole folder, and `plan_with` 1.3 s. A test of the raw bytes that decides "binary" before detection, and falls through to today's path whenever it is not sure, cuts most of those 31.5 s, **for the import as much as for the scan**, as both run this code. Package F1 does it behind the agreement test, which must show the same verdicts on the whole corpus. The saving is a [calc] until F1 measures it.
- **Head size: the import's own, `HEAD_BYTES`.** A 64 KiB head read 631 MiB instead of 3.09 GiB and finished in 3.5 s instead of 4.4 s at 8 threads, with the same verdicts on this corpus [M]. But the import plans from 1 MiB, so a smaller scan head could differ on a file whose text format shows only past 64 KiB, which is the kind of promise this design rules out. The 0.9 s are not worth a second source of truth. Reading is 105 ms of the 4.4 s warm; cold it was 2.0 ms a file at one thread [M], which more threads hide on an SSD.
- **Sniff once.** `format_score` runs every importer's sniff for every parser whose rules pass (`parser/set.rs:247-251,308-343`). ADR 0064's memo and declared sniff (SI-6) apply here unchanged, and plugin processes start only for a dynamic sniff. ADR 0064 measured what plugins add: 17 sniff calls per text file, 153 µs per 64 KiB call, so about 36 s of thread time at 14,000 files today and about 13 s after its fix, or at 8 threads about 4.5 s and 1.6 s [ADR 0064 section "Per-file costs at 14,000 files", its M and calc; here an upper bound, as a file that is not text gets the 3 binary sniffs only]. **So no budget above holds with plugins until both F1's pre-check and SI-6 have landed and been measured.**
- **No parsing of whole files, no decoding past the head, no store access.** The scan writes nothing to DuckDB.
- **No verdict cache in this ADR.** A re-open scans again. A cache keyed on size, time and the parser set's version would save seconds locally, and it is worth adding only if a share measures badly. [J]
- **Rows.** A page is at most 500 rows: about 80 KB of JSON. Status changes are batched in events of counts at most 4 times a second; a row's data is fetched only when it is in the window.

### 5. The table and the view, in the engine

`debrief-core/src/folder_scan/` is pure and has no I/O: `ScanTable` holds nodes in flat arrays (parent, name offset into one arena, kind, status, reason, size, file count, selected counts, check state, the children's range). The view adds the expanded set, the sort and the filter, and produces the visible order. Each operation is a pure function of the table, so it is unit-tested without a disk. Measured [M]:

| 14,152 nodes | Measured [M] |
|---|---|
| Table size | 66 bytes per node, 0.9 MB |
| Walk, stat, sort, build | 37-44 ms warm, 559 ms cold |
| Full roll-up | 0.05-0.07 ms |
| Toggle one file, and a folder of 2,723 files | 0.2 µs, 0.072 ms |
| Visible order, all expanded, by name and by size per folder | 0.01 ms, 0.13 ms |
| Name and status filter with the visible order | 0.25 ms |
| A page of 500 rows as JSON | 80,389 bytes |

So every view change is answered well inside a frame, and the engine never needs an index for the view at this size. At 500,000 entries the same work is about 35 times larger [calc], still under 10 ms.

A person's expanded set, sort and filter are interaction state, which AGENTS.md leaves to the UI, but they decide a paged, engine-computed order. They are kept with the scan in the engine and sent on each change, like a query. The UI owns the focus, the scroll position and the column choice.

**Commands** (`Mutation` or `Query` by rule of the command table; bodies are thin; each takes a window and checks the scan belongs to it). Paths never cross again after `folder_scan_start`: everything else names a `scan` and `node` ids.

| Command | Takes | Returns |
|---|---|---|
| `open_plan` | `paths` (checked by `open_request`) | `{ direct, choose }` |
| `folder_scan_start` | `paths` from `choose` | job id and scan id at once |
| `folder_scan_rows` | `scan`, `offset`, `limit` at most 500 | `Bounded<RowsPage>` (`Bounded::counted`, `Next::Page`): rows with `level`, `posinset`, `setsize`, `parent`, plus `count` and `version` |
| `folder_scan_view` | `scan`, `expand`, `collapse`, `sort`, `filter` | `count`, `version`; the focus row's new index |
| `folder_scan_check` | `scan`, `node`, or a bulk kind (`recommended`, `all_supported`, `none`) | `Totals`, `version` |
| `folder_scan_totals` | `scan` | `Totals`: selected files and bytes, disk free for the store's volume, warnings, not selected by reason |
| `folder_scan_retry` | `scan`, `node` | Judges the node again |
| `folder_import` | `scan` | Import job id. The scan's selection is the request; no path list |
| `folder_scan_close` | `scan` | Drops the table. A closed window drops it too |

**Events.** `folder-scan:progress` (phase, done, total, version) at most 4 Hz. A changed `version` tells the UI to refetch the window it shows. Long folders stay smooth since a change touches only the rows in view.

**Trust.** `folder_scan_start` passes `open_request`, so a path must be granted to the window, canonicalised. Everything after is relative to that root and comes from the walk: there is no input the webview can use to read elsewhere. Node ids are bounds-checked, a stale `version` is answered with the current one, and `folder_import` re-resolves each selected path against the canonical root. A link or junction that appears between scan and import must also be refused, and today nothing would refuse it: `open_with_retry` is a plain `File::open` (`crates/debrief-core/src/workspace/mod.rs:1331`) [S]. So F7 adds an open that refuses links and checks where the handle really is. On Unix it opens with `O_NOFOLLOW`. On Windows, `FILE_FLAG_OPEN_REPARSE_POINT` opens the reparse point itself rather than refusing it, so the open then reads `FILE_ATTRIBUTE_REPARSE_POINT` with `GetFileInformationByHandleEx` and refuses such a handle. Then the handle's own path must lie under the root, and **both are got the same way**: `F_GETPATH` on macOS (which also resolves firmlinks), `/proc/self/fd` on Linux, `GetFinalPathNameByHandleW` on Windows with the same flags for the root's handle, so `\\?\` prefixes, volume GUID paths and drive letters compare like with like. A file whose handle is outside is refused as "Moved outside the folder". This covers links in the middle of the path as well. **Hard links cannot be told apart** from ordinary files this way: a hard link inside the folder to a file elsewhere is read. Making one needs write access to the folder, which is the same as copying the file in, so this is accepted and stated. **A picked folder or file, or a launch path, that is itself a link** is resolved once when it is granted (`open_request` canonicalises it), and its target becomes the root; inside the root no link is followed. The import uses the same open (rule 53: act on the handle, not on a name read earlier). Tests: a symbolic link swapped in on Unix, and a directory junction on Windows (CI's Windows runner).

**Text.** File names, folder names and reasons are bound as text, never through `[innerHTML]` (AGENTS.md), and reasons are words from typed `Refusal` and limit kinds rendered in the UI's i18n, so nothing from a file or an OS error is ever markup.

**Sorting** is per folder: children of a folder sort among themselves, folders first when sorting by name, and a folder's size and count are its roll-up. Sort keys: name (natural order, one implementation shared with `pd-table`'s `compareValues` semantics: `Intl.Collator numeric` is mirrored by the engine's comparison and tested on a table of cases), type, status (worst first), reason, size, files. The default sort is by name, ascending.

**Filter.** A name filter (substring, case-insensitive, whole words split on spaces, as `filterRows` reads), a Type multi-select and a Status multi-select. A folder shows when any descendant matches, with roll-ups that count only the matching rows (the count near the filter says `1,100 of 14,000 files`). The engine computes it.

### 6. Preselection, as data

`crates/debrief-core/src/folder_scan/recommend.toml` is a built-in pack, hot-reloaded and overridable from the user's config like other knowledge packs. No file name of any vendor or site is in it. The inputs are engine facts:

```toml
# Checked unless a later rule says otherwise.
[[rule]]
id = "reads-as-logs"
when = { status = ["supported", "limited"], class = ["log", "event-log", "dump", "trace"] }
check = true

# Readable, but it is project data rather than a record of what happened: unchecked, with the reason.
[[rule]]
id = "project-data"
when = { status = ["supported", "limited"], class = ["config", "image", "untimed-table"] }
check = false
reason = "Project or configuration data"

[[rule]]
id = "over-a-limit"
when = { limit = ["memory", "entry-size"] }
check = false

[[duplicates.same-stem-other-form]]
# The same logs in two forms: the richer form stays checked.
prefer = ["event-log"]
over = ["text-log"]
scope = "same-folder-or-sibling"
min_score = 2                         # stem match, then time span or line count from the head

[[duplicates.series-names]]
# Two series side by side, where one names a summary.
tokens = [["all", "full", "complete"], ["summary", "brief"]]
scope = "sibling"
```

`class` comes from the chosen importer and parser (each parser and importer declares one, in the TOML or the manifest; a new format is one entry), so it is never a list of extensions. The result of rules is `check` plus `reason`, and the rule id goes to the logs.

**Duplicates are only flags.** Both stay listed. The preferred one is checked, the other is unchecked with the tag "Likely duplicate" and "of `<name>`", and a notice above the tree says how many sets there are, with "Show them", which sets a filter. The note could not measure whether `Summary` is a subset of `All` [N], and neither does this: the scan compares names and heads, and the wording stays "likely". Files with an identical `fingerprint` (size and both edges, 128 KiB read per pair of equal size) are stated as "Same content as `<name>`". A file with no rule falls to `check = false` and reason "No rule recommends it", so an unknown class is never imported by default. [J]

### 7. Cost before commit

The strip below the tree shows selected files and bytes, disk free on the store's volume, and "Not selected" with a popover of totals by reason ([image](../images/folder-import-dialog/reasons-dark.png)). All values come from `folder_scan_totals`. Warnings:

- **Need attention** is defined once: the checked-or-recommended files that will not import as they stand and that a person can do something about, which are "Can't read" (Retry) and "Needs too much memory" (the gate refuses them). Duplicates, Not supported and empty files are not in it; they are in the totals by reason. The strip says "3 files need attention" with "Show them", which filters to exactly those, and its tooltip splits the count ("2 need too much memory, 1 can't be read").
- **One order of reasons** everywhere (the popover, the Status filter, the logs): Can't read, Needs too much memory, Likely duplicate, No parser reads this file, Binary file, Empty file, then any other `Refusal` kind. Actionable reasons come first.
- Disk: when selected bytes exceed free space, the strip warns; below `store_cap` headroom it also says so. The store's size per source byte varies by format and no measured ratio exists for this mix, so the dialog gives no estimate of what the import will use. [J, U]
- Unreadable: "1 file could not be read" with Retry.
- Estimated import time: "About 7 s to import", from the selected bytes and the import rate this machine last measured, falling back to the 5.3 to 5.4 s per GB recorded for the Mac mini in `performance.md` (import budget row). Shown as an estimate, rounded, and only for more than 1 GB; it never blocks anything. [J]

**Totals by reason** replace per-file rows. After the import, the Explorer has a single row "11,542 files not imported" with the same popover and "Choose files…", which reopens the dialog. `WorkspaceSummary.unrecognized` then holds only what the import tried and failed, which fixes the note's 4.4 MB summary (gap 7) for this case; plan 4.2 bounds the rest.

### 8. The selection and the import

Selection is a set of **rules, not paths**: `(relative prefix, on | off)`, most specific wins, with one default per root. The engine reduces the checked tree to its smallest cover: a fully checked folder is one rule. For the invented bundle's recommended set, chosen by a stand-in for the rule pack, this is 9 rules [M]. A folder with mixed files needs one rule per differing file, so the worst case is the number of files; `WorkspaceState` keeps it in a compact sorted form that is bounded by `folder.scan_entries`.

```rust
pub struct FolderSelection { pub root: PathBuf, pub default_new: NewFiles, pub rules: Vec<(RelPrefix, bool)> } // sorted
pub enum NewFiles { Recommended, Ask }
```

- `WorkspaceState` gains `selections: Vec<FolderSelection>`. `roots` remains the root list; `removed` stays for a single source a person removed. Before v1 no compatibility is kept (memory: no backward compatibility before v1).
- `discover_stream` gains one predicate, the compiled selection, which prunes a whole unselected folder without listing it. So `import`, `reload`, `restore` and `refresh_tail` get it for free. It is one line in `Walk::list` next to `removed`.
- `folder_import` passes no paths and is not subject to `MAX_OPEN_PATHS`. The job's `retry_with` carries the root and its rules rather than a list.
- Unselected files never become sources or Explorer rows. Only the totals by reason remain, stored with the selection.

**Source ids** follow the sorted stream as before (ADR 0051), so selection only skips ids.

### 9. Queries and removal by folder

- **`Query::source_folders: Vec<FolderRef>`**, at most 256, beside `sources`. A `FolderRef` is a root and a relative prefix. The engine resolves it once per query against the `sources` table (`path` is sorted; a range scan), into the existing `Expr::Sources` set. An Explorer group or folder row then sends a folder, not thousands of ids, and `MAX_QUERY_SOURCES` stays as the guard on hand-picked lists. The Explorer's group "Filter to these" moves to it.
- **`remove_sources(selector)`** in the `Store` port: one id, a list, or a `FolderRef`. One transaction, one `maintain` at the end, one write of the state. The rules flip to off for the folder rather than adding its paths to `removed`. Time at 2,793 sources is measured in F8, against today's about 170 s [calc].
- **Undo.** The shared journal (`paddock_jobs::journal`, `shell/undo`) records one entry: "Removed the folder Logs (2,723 sources)". Undo flips the rules back and starts an import job for those files, so it is a re-read with progress and Cancel, not instant, and the toast says "Reading them again". Source ids come back through the fingerprints (ADR 0051), when the files have not changed. **Notes anchored to an event by id are not restored by this** because event ids are not reproducible between imports (ADR 0062); they fall back to their anchor's time, source and excerpt. [S for ADR 0062; the fallback's behaviour here is U]
- A soft window that hides rows and postpones compaction would make undo instant. It would also keep bytes on disk with no way to tell the user. Not in this ADR. [J]

### 10. Re-opening, remembering, later additions

**Remembered per folder, in the workspace or project** (keyed by canonical root): the selection rules, the totals by reason, `NewFiles`, and the set of rows the person changed by hand. Remembered per user by the dialog shell as for any tool: window size and offset, hidden columns. Not remembered: the expanded set, the sort and the filter, since a reopened tree starts clear. A new workspace starts from the recommendations (D5).

**Re-open.** "Choose files…" in the Explorer's folder menu and on the totals row, and `file.addFromFolder` in the menu. It scans again (a cheap, honest view of the folder now) and shows:

- A file already imported: status "Imported", checked and locked. Removal stays in the Explorer's Remove, so a checkbox cannot delete data as a side effect (D4).
- A new file since the last scan: tagged "New", with the rules applied.
- A file that is gone: not listed. Its source stays with the Explorer's existing "file gone" state.
- A person's own choices stay, recommendations do not override them.

The primary button reads "Import 12 more", disabled at zero. Its action is additive.

**Live watch and later additions.** `refresh_tail` finds new files under opened folders and imports them; `watch.rs` does so for watched folders. Both pass new files through the compiled selection and `judge`:

- Under an off prefix: ignored, and counted in the totals.
- Under an on prefix, or at the default: `NewFiles::Recommended` applies the rule pack with no dialog; a file not recommended is counted, not imported. A notice "3 new files were not imported" offers "Choose files…", at most once per batch.

A file under a live tail is not probed for duplicates. This is the only path that imports without the person seeing the row, and only for files the same rules would have checked (D3).

### 11. Zipped bundles

A zip row is a folder row with its own glyph, and a single zip with more than one member opens the dialog with its members as the tree (D1). `paddock_archive::Members::entries()` (ADR 0064 section 3, package SI-4, after the Dyno tag) gives names and declared sizes from the central directory, read by range, without expanding anything; until then `archive.rs`'s `central_records` gives the same over a whole read of the directory. Phase 2 opens each member through `Members::open_member` and inflates only its first `HEAD_BYTES`, or stops at the first deflate block that passes it, so it never holds the whole archive, and judges the member as a file. A selected member is `archive!/member` (`archive::ENTRY_SEPARATOR`), which `removed` and the import already use, and a prefix rule works on a member path just as on a folder.

- Above 4 GiB: user decision; ADR 0064 retires `MAX_ARCHIVE_BYTES`. Until then, a zip over the limit reads "Too large to expand: 4 GiB limit", Limited and unchecked, and members are still listed.
- `gzip`: one member, no tree.
- A zip inside a zip or inside a folder's zip: listed as one file, "Nested archive: open it separately". Not supported, and not expanded. [J]
- Encrypted member: "Encrypted", Not supported.
- Malformed central directory, or any other archive error: the zip row reads "Can't read: the archive is damaged". Archive errors map to typed `Refusal` kinds (`Damaged`, `Encrypted`, `TooManyEntries`, `TooLarge`) with words in the UI's i18n; the error's own text goes to the logs only, never to the row.
- `MAX_ENTRIES` 65,536 stays and reports through a `Cut`.
- Cost at many small members: one `inflate` of up to 1 MiB per member. It is CPU, bounded like the parse pool. Not measured here [U]; F11 measures the 14,000-member case.

### 12. Errors: degrade, don't fail

| What happens | The dialog shows | The logs say |
|---|---|---|
| The root cannot be read | The [scan error state](../images/folder-import-dialog/error-dark.png): reason, Try again, Choose another folder | Error with the OS code |
| A folder cannot be listed | Its row: "Can't read", the reason, Retry in its "..." menu; siblings go on | Warning, count per reason |
| A file is locked or unreadable | Its row: same, unchecked, disabled; retried once automatically (`open_with_retry`, as the import) | Warning, the first 20 then a count |
| The folder disappears during a scan | The rows it had stay, marked "Gone"; the header says so; Try again | Warning |
| A file disappears or changes after its verdict | Nothing until the import, which reports it with the rest | Info |
| A plugin crashes on a sniff | Its rows: "Can't check: plugin stopped", Retry. The crash stays contained. By D9's recommendation it does **not** count toward quarantine; after 3 crashes in one scan that plugin is not asked again for the rest of the scan, and its rows read "Not checked: plugin stopped for this scan" | Error with the plugin |
| A plugin is quarantined | Rows: "Plugin stopped: <name>", with a link to Plugins. Never `binary file` (bundle note, "Misleading reason") | Warning once |
| Disk full when writing the state | The import's error applies; the scan writes nothing | |
| Scan cancelled | Dialog closes. The state is unchanged | Info with counts |
| Nothing importable | The [state](../images/folder-import-dialog/nothing-dark.png): reasons, Set up a format, Open another folder. "Import selected" disabled, Close | Info |

### 13. What the logs show (ER-27)

Counts and reasons only; paths through `privacy::safe_path`, never contents. One correlation id per scan, given to the import it leads to.

```
FolderScan started    Scan=7 Roots=1 Threads=8 Remote=false HeadLimit=1048576
FolderScan phase      Scan=7 Phase=listing   Files=14000 Folders=<n> ElapsedMs=<n>
FolderScan phase      Scan=7 Phase=detecting Files=14000 BytesRead=<n> ElapsedMs=<n> Supported=<n> Limited=<n> NotSupported=<n> CantRead=<n>
FolderScan refused    Scan=7 Refusal=binary Count=7429  (one line per reason, always)
FolderScan unreadable Scan=7 File=<safe path> Reason=locked  (first 20, then More=n)
FolderScan rules      Scan=7 PackVersion=3 Rule=reads-as-logs Count=2430 ...  Duplicates=2
FolderScan import     Scan=7 Selected=<n> Bytes=<n> Rules=<n> ByHand=<n> Job=<id>
FolderScan broken     Scan=7 Job=91 File=<safe path> Verdict=supported Actual=no-events (warn, first 20, then More=n)
FolderScan finished   Scan=7 Outcome=imported|cancelled|closed|failed ElapsedMs=...
```

So a diagnostic bundle alone answers why a file was or was not imported: which rule unchecked it, or which verdict the import contradicted. Each line is covered by a test that asserts its fields and that it holds no file name beyond the privacy path.

### 14. The dialog in the UI

**Kind.** `folder-import`, kind `tool`, 960 x 680 by default, user-sized, remembered, in its own window (rule 17). Modeless: decision D2, which is where this bends rule 17's "pickers that feed the next step" are modal. Family ADR 0043 fixes `tool` as the only kind with a user size, and says a dialog needing something else is a new kind, which is a shared change that cannot land before the Dyno tag. A modeless window is also honest for this job: on a slow share the scan takes a while, and the main window stays usable, and a re-open sits beside the Explorer. One `folder-import` per main window; a second open focuses it. If an import is already running, "Import selected" stays enabled and queues the job, since `store.open` answers `Busy` at once today and ER-19 says a person never waits on a refusal. Commands are declared in the contract (`check-dialog-commands.mjs`).

**Layout** (see the images; Standard density, tokens only, `--pd-*`):

- A path line: folder glyph, `pd-path-text` (rule 18, truncated at segments, full path in its tooltip), and at the right "14,000 files, 7.56 GiB. Scan finished."
- A toolbar projected into `pd-table`'s `[pdTableToolbar]` slot: name filter, Type and Status as `mat-select multiple`, "Select" as `mat-menu` (Recommended, All supported, None, Expand all, Collapse all), Columns.
- A notice band for duplicates, the over-limit `Cut`, or nothing importable; or the progress row while scanning (`mat-progress-bar`, phase label, count).
- The tree grid: Name, Type, Status, Reason, Size, Files, and the row action column.
- The cost strip.
- The frame's footer, drawn from declared actions: Cancel, and "Import selected" as the Enter target, the one primary.

**Tables that size to the window.** The grid takes all the space left in the window; `pd-table` needs a [fill mode](#pd-table-what-fits-and-what-is-missing). Columns drop by `hideBelow` with no horizontal scroll (rule 18): Reason below 760 px, where the reason moves to the Status cell's tooltip and accessible name; Type below 640 px; Files below 480 px. The [narrow image](../images/folder-import-dialog/narrow-dark.png) is 600 x 560, below 640, so Type and Reason have dropped and Files stays. Size column values carry their own units since they run from `0 B` to `1 GiB`; the design guide asks for units in the header, which a human-sized column cannot do. This is a deliberate exception and a review point. [J]

**Row actions** in the "..." cluster and right-click, per the family pattern, keyboard reachable: Check only this, Retry (a row that can't be read), Set up a format… (opens `format-wizard` with this file, then the row is judged again; a row with no parser), Parse with…, Reveal in the file manager. A folder: Check only supported files, Expand all below.

**Checkbox semantics.**

- `mat-checkbox` with `[indeterminate]` and `clickAction: 'noop'` (`MAT_CHECKBOX_DEFAULT_OPTIONS`), so the visual state always comes from the engine.
- A folder's click: mixed or off becomes on, meaning every *selectable* descendant, on becomes off. "Selectable" leaves out Not supported and Can't read rows, which have no checkbox (below) and their reason beside them; a folder with no selectable descendant has none either. Such rows show no checkbox at all, only a dash in `--pd-text-subtle` in its place (5.8:1 on `--pd-surface-1` in dark, 5.7:1 in light [M, computed from the tokens]), so nothing looks clickable that is not; the accessible description says "cannot be selected" and the reason.
- A filter only hides rows and never changes the selection. A folder's check under a filter applies to matching rows only, and the footer's message says that the selection is unaffected by what is hidden.
- "Recommended" in the Select menu goes back to what the rules say, and forgets choices made by hand.

#### `pd-table`: what fits and what is missing

Rule 42 and rule 47: this is `ui/projects/paddock-ui/table/src/table.ts`, Material's `mat-table` and `matSort` on CDK's `cdk-virtual-scroll-viewport`, used by Dyno's processes page and the shell demo, with e2e coverage in `ui/e2e/data.spec.ts:373`.

**Fits today, no change:**

| Need | Where |
|---|---|
| `treegrid`, `aria-level`, `aria-expanded`, `aria-posinset`, `aria-setsize`, `aria-rowcount`, `aria-rowindex` | `table.ts:189`, `:287-292`, `tree` input |
| Right unfolds or enters, Left folds or goes up, Up, Down, Home, End, PageUp, PageDown | `treeKey`, `nextIndex` |
| Fold state owned by the app | `rowToggle` output; `order="source"` and `tree` skip its sort and filter |
| Engine-side sort | `sort` is a two-way `model`, `matSort` draws the arrow and `aria-sort`, and a tree grid does not sort locally (`ordered`) |
| Rows filling in during a scan | `rowKey` keeps identity, focus survives redraws (`pendingFocus`, `restoreLostFocus`), `trackLine` |
| Tri-state cell, status cell | `pdCell` custom cell templates, `mat-checkbox` is themed in `_material.scss:159-166` |
| Row actions, their keys and shared tooltip | `rowActions`, `PdActionTip` |
| Columns by width, user-hidden columns | `hideBelow`, `hiddenColumns` |
| Toolbar slot, own filter | `[pdTableToolbar]`, `filterable=false`; tree grids already have no filter field (design guide) |
| Error and empty states | `state`, `errorText`, `retry`, `[pdTableEmpty]` |

**Per-change cost at 14,000 rows** [M]: 3.7-5.5 ms per change of `rows`, 0.15-0.25 ms per arrow key; at 100,000 rows 26-37 ms and 3-11 ms. This is the model's own work (`treePlaces` and the maps), run in Node without Angular, so it is not a measure of drawing. So the `rows` path is fine at this size without changes to `treePlaces`.

**Missing: six changes, one package, after the Dyno tag (F10).** Each ships with a spec and a gallery row, each is a `CHANGELOG` line, and existing users see no difference while the new inputs are unset.

| # | Gap | What exists | Change |
|---|---|---|---|
| T1 | Enter and click cannot be separated from `rowActivate`. Both go through `open` (`:781-787`, `:861`), so binding Enter to import would also import on a click | One path | `activation = input<'row' \| 'none'>('row')`. With `none`, a click sets focus only, Enter is not consumed and bubbles to the frame, whose Enter target runs |
| T2 | Space is the same as Enter (`:857`). A tree grid with checks needs Space to toggle | Nothing | `rowSpace = output<T>()`; when bound, Space emits it. Group rows are unchanged |
| T3 | Right reaches only `.pd-icon-btn` (`rowButtons`), so a checkbox in a cell is not reachable by the grid's arrows | Icon buttons | The same list also takes `[pdRowControl]`. Right from a leaf, or from an unfolded row with no child in view, moves to the checkbox, then to the actions. Tree rows keep Right for folding first |
| T4 | The table needs every visible row in memory. `rows` is a whole array; `treePlaces` and `parentIndex` compute from it, and a placeholder has no index or focus | `state='loading'` skeleton for the whole table only | `window = input<PdRowWindow<T> \| null>(null)`: `count`, `row(i): T \| null`, `place(i)`, `parent(i)`, and output `rangeChange` from the viewport. An unloaded line is a focusable placeholder with its index. `aria-rowcount` is from `count`. The app fetches on `rangeChange`. See below |
| T5 | Height is a number: `min(lines x rowPx, maxHeight)` (`:617`, default 480). The storage page passes 640 and the problems view passes 2000 | Fixed `maxHeight` | `fill = input(false)`: the body takes the host's remaining height from a `ResizeObserver` (one is already there for the width). Page size for PageUp and PageDown follows it. A number-based `maxHeight` stays for the others |
| T6 | A tri-state row has no announced check state: `aria-checked` is not allowed on `row` in ARIA 1.2, and `aria-selected` is two-state | `aria-selected` for the opened row | `rowDescription = input<(row: T) => string>()`: a visually hidden text at the end of the first cell, read with the row. The dialog gives "partly selected, 612 of 1,340 files". Screen reader results are checked in F12 and are not known yet [U] |

Why T4 rather than passing a growing array: refreshing 14,000 rows at 4 Hz during a scan is about 9 MB of JSON a second, while a window of about 60 rows is under 1% of that, and the same mechanism holds at 500,000 rows. Row data that fits in memory can still use `rows`; T4 is a second, separate input.

**Shared?** Everything above is generic and goes in `@decryp7/paddock-ui/table`. Domain rules stay in Debrief's engine. The dialog component itself is Debrief's. Nothing shared lands before the Dyno tag (memory: "no shared changes before the Dyno tag"). **Debrief 1.2's own path:** it consumes `ui/` in the same repository (rule 43), so F10 merges to `debrief/1.2-int` through `main` after the tag. If the tag slips past F9's start, F9 builds on a local branch of F10 and waits with it.

**Debrief's hand-built tree grids**, compared for rule 42:

| View | What it is | Should it move? |
|---|---|---|
| `profiling/heap/heap-dominators.ts` | `<table>`, level-by-level load, no virtual scroll; its own keys | **Yes, next to no risk**: `tree`, `rowToggle`, `spans` for "pending" rows. About 0.5 day, after T-free tests. No new gap |
| `profiling/tree/call-tree-view.ts` | `<table>`, paged children ("more" rows), its own keymap with `*` for hot path and Enter for callers | **Yes**, needs a key extension point like T2 and T1 (`activation: none`, a `rowKey` output for `*`). About 1 day. Its `tree-model.ts` stays |
| `config-diff/config-diff-view.ts` | Its own windowing with a scaled physical height and `aria-activedescendant`, split or unified columns | **Not now.** T4 covers its paging, but its split layout and scaling past the browser's maximum element height are not generic until a second user needs them. Review after T4 has shipped |

These moves are listed so they are not lost. They are not in this ADR's packages, and each would be its own change under rule 42.

#### Keyboard and accessibility

| Key | On a row | Elsewhere |
|---|---|---|
| Enter | Imports (the frame's Enter target) | In the filter box it moves focus to the first matching row and does not import, so a filter's Enter cannot start an import by accident. On a sort header it sorts; on a menu it picks |
| Space | Toggles the check | Normal |
| Right, Left | Unfold or enter; fold or go to parent | |
| Up, Down, Home, End, PageUp, PageDown | Moves focus; loads the window if needed | Up from row 0 goes to the sort header |
| Shift+Space | Toggles every row from the anchor to this one in the view, with one engine call. [J] | |
| `*` | Expands all below (the ARIA tree pattern) | |
| Ctrl/Cmd+F | Moves to the filter box | |
| Esc | Closes the dialog, or the open menu, as the frame and Material do. When the filter box has text, the first Esc clears it | |
| Tab | Filter, Type, Status, Select, Columns, the grid's active row (one stop), Retry or links in the strip, Cancel, Import selected | |

- **Row announcement** at focus: name, level and position (from ARIA), the description from T6 ("partly selected, 612 of 1,340 files"), Status with its words (never the glyph alone), Reason, Size, Files.
- **Live region.** Scan phases and totals go in one polite `aria-live` region, at most once every 5 seconds during a scan, and when it ends. Rows do not announce when they fill.
- **First focus.** The grid's first row: the frame's `autofocus` hook in place of the Enter target, because Enter works from there anyway and arrow keys need to work at once.
- **Contrast.** All colours are tokens and the token test covers them. Measured from the renders in [the results block](../performance.md#2026-11-05-folder-scan-on-invented-files-adr-0067) is not contrast; F12's axe run in both themes and the token test are the checks.
- **Reduced motion.** The progress bar and skeletons follow `data-motion`.
- **High contrast and density.** Rows follow the density token (`pd-table` already does) and the checkbox's hit area follows `--pd-hit-min`.

### 15. The mock backend

`app/src/app/core/mock/mock-folder-scan.ts` implements the same commands over a generated in-memory tree of invented names, as `mock-import.ts` does, for browser development and specs only. It can simulate a slow scan, a locked file, a cut and nothing importable. It has **no** judgement of its own: statuses are stamped by the generator and come with the scenario. Rule pack, roll-ups, sort and filter are exercised on the Rust side only; the mock returns canned pages for the states. A real-engine e2e is what makes it "done" (AGENTS.md).

## Fit with the other ADRs in progress

- **ADR 0064 (streaming trait, revision 3, `c12864566`).** `judge` uses its `DeclaredSniff`, sniff memo and `Sniff` batch (SI-6) unchanged. `Head` is the plan's head. `Input::ACCESS` says which importers stream and so need no row in `memory.toml`; `whole_reason` names the ones that do. `paddock_archive::Members::entries()` and `open_member` (SI-4, after the Dyno tag) feed zip rows. SI-9 streams EVTX inside the plugin, so it removes the plugin side of EVTX's row; the host still reads the whole file until SI-10, so the worker side stays until then. Its fix 3 ("one open per file") applies to the scan and the import alike. Both ADRs change `import_source`; F1 lands on either side, moving the same lines. Its measured sniff cost is the dependency in section 4.
- **ADR 0065 (bounded replies, `1eaa14850`).** `folder_scan_rows` returns `Bounded<RowsPage>` from `Bounded::counted`, with `Total::Exact` and `Next::Page`. As with the events grid, the tree's scrollbar is that display, so `BOUNDED_SURFACES` lists it with that reason and no note is drawn for paging. The listing's limit is a `Bounded::first` cut with `Total::AtLeast` and `Next::Narrow`, and `folder.scan_entries` is a registry row with its `limit_case!`. The UI uses `<pd-limit-note [cuts]>`, and the logs use `Cut::describe()`. Totals by reason are a fixed set of `Refusal` kinds and go in `bounded-exempt.toml` as `fixed-set`. That ADR adds no Settings; this one adds this single one, under its rule that a limit with a Setting says so. If D1 of that ADR puts the types in `paddock-bounded`, they arrive after the Dyno tag with F10.
- **ADR 0066 (how times are read, `2f289b4c3`).** Limits use its `TimeReader` and tally; the scan hosts its probe; the chip and its words are its own; `import_roots` is written by `folder_import`; `zone_settings` and the selection share prefix lookup; the agreement test shares its corpus. The two import-root notions agree: its "a path given to one `open_paths`" becomes "a path given to one `open_paths` or one dialog". Its order of work does not depend on this one: a scan without the probe has no chip and its rows stay "times as written".
- **ADR 0063 (binary wire).** Pages are small JSON now, and a later move of `folder_scan_rows` to `#[wire_command]` changes nothing here. Not needed at 500 rows.
- **Plan 2.7, M3b (Explorer virtualized).** The dialog makes the Explorer's refused rows vanish; virtualization remains for imported sources. The totals row is a new Explorer row kind.

## Test plan

| Layer | What | Where |
|---|---|---|
| Rust, pure | `ScanTable`: rollups, tri-state, cover (property: cover selects exactly the checked set), filter, per-folder sort, paging, `version`, stale requests, bounds; never panics on odd names (long, control characters, invalid UTF-8) | `debrief-core/src/folder_scan/*` |
| Rust, agreement | [`judge_agreement`](#when-the-judge-and-the-import-disagree); a mutation run showing it fails when the head length differs | `tests/it/` |
| Rust, rules | Each rule and duplicate kind on invented shapes; a user pack; a broken pack falls back to the built-in with a notice | `folder_scan/recommend` |
| Rust, scan | Phases and monotonic progress (ER-18), cancel in each phase, a locked file, an unreadable folder, a link not followed, a file that vanishes, the cut, a plugin crash and a quarantine, a zip with a damaged directory | `tests/it/folder_scan.rs` |
| Rust, trust | A node id out of range, a scan from another window, a link swapped in between scan and import, a request naming a path | `app/src-tauri` tests |
| Rust, import | Selection prunes without listing; reload and restore keep it; tail applies it; `remove_sources` of a folder, undo; `Query::source_folders` at 256 and over; `broken_promises` | `debrief-store-duck/tests` |
| Benchmark | The scan at 14,000 and 500,000 invented files, with a budget assertion at 14,000 (rule 48, `performance.md`) | `debrief-store-duck` bench |
| Angular | Dialog against the mock: every state, Enter in each focus place, Space, filter Enter, row click does not import, each key in the table above; specs wait on conditions (rule 49); `pd-table`'s spec for T1 to T6 and CDK harnesses (rule 38) | `app/src/app/import/folder/*`, `ui/.../table.spec.ts` |
| Playwright, mock | Smooth scroll at 14,000 expanded rows: long tasks over 50 ms counted during a scripted fling, zero allowed at a warm start. Both themes, 1440 x 900 and the narrow size; axe; dialog matrix entry (ADR 0043) | `app/e2e`, `ui/e2e/data.spec.ts` |
| Real engine | `e2e-tauri`: open the invented 14,000-file bundle, press Enter, assert sources and totals. The unattended smoke asserts no dialog on restore | `app/e2e-tauri` |
| Manual | VoiceOver, NVDA and Narrator on the tri-state announcement (T6); the bundle the user has, by the user | `docs/testing-manual.md` |

## Packages

Rough days, before implementation design. Order: F1 to F4 can start now on `debrief/1.2-int`; F2's budget needs ADR 0064's sniff fix.

| # | Package | Paths | Days | Needs |
|---|---|---|---|---|
| F1 | `judge`, typed `Refusal`, import calls it; `PluginStopped` wording; the raw-byte binary check before encoding detection, with its A/B measurement on this corpus, **which must meet the budget's precondition before F4 closes** | `debrief-core/src/ingest/{plan,source,encoding}.rs` | 2.5 | |
| F1b | The `judge` caller check in conformance: a new whole-file caller of `choose` or `plan_with` fails | `conformance/`, `paddock.conformance.json` | 0.5 | F1 |
| FM | Memory per input byte: `importers/memory.toml` with `at_least`, the two-sided check, the worst-factor default, the test that ties rows to declared input kinds; **the whole-file gate** in the dispatcher and per plugin process, with its queue, refusal and progress; the reproduction's worker tests (`debrief-store-worker/tests/it/folder_open.rs`) made the measuring harness | `debrief-core/src/importers/`, `debrief-core/src/ingest/pipeline.rs`, `debrief-plugin-host`, `debrief-store-worker/tests/it/` | 4 | F1 |
| F2 | Limits from the head and manifest; agreement test on the shared corpus; `broken_promises` in the report | `ingest/`, `debrief-plugins/tests/it/` | 2 | F1; ADR 0064's sniff memo for the budget; moves onto ADR 0066's reader when that lands |
| F3 | `ScanTable` and view, pure, with tests | `debrief-core/src/folder_scan/` | 2.5 | |
| F4 | The scan job: phases, pool, priority, window-first order, cancel, errors, logs, the limit | `folder_scan/run.rs`, `app/src-tauri/src/folder_scan.rs` | 2.5 | F1, F3 |
| F5 | Rule pack, `class` in parser TOML, duplicates, hot reload | `folder_scan/recommend*`, `crates/debrief-core/parsers/` | 2.5 | F3 |
| F5b | `class` in the plugin manifest: the field, each first-party plugin's value, the host reading it, refusal of an unknown value | `debrief-plugin-api` manifest, `crates/debrief-plugin-*` | 1 | F5 |
| F6 | Selection in the walk and state; reload, restore, tail, watch | `ingest/discover.rs`, `debrief-store-duck/src/lib.rs`, `api.rs`, `watch.rs`, `workspace/tail` | 2.5 | F3 |
| F7 | Commands, grants, `open_plan`, dialog contract, events, generated types; the link-refusing open with its Unix and Windows junction tests | `app/src-tauri`, `debrief-core/src/workspace/mod.rs`, `dialog-registry.ts`, `generated` | 2.5 | F4, F6; **on Debrief 1.2's release path**: it changes how every import opens files, so it lands before the release and its merge batch builds the installers (rule 54) |
| F8 | `source_folders`, `remove_sources`, undo, Explorer totals row | `query.rs`, `ports/`, `debrief-store-duck`, `shell/sidebar.ts`, journal | 3 | F6 |
| F9 | The dialog, store, cost strip with the time estimate, popover, re-open; the mock | `app/src/app/import/folder/`, `core/mock/` | 3.5 | F7, F10 (or its branch) |
| F9b | i18n: every new string with its id, plurals for counts, the catalogs regenerated, the reasons' words for each `Refusal` and limit kind | `app/src/locale/`, `ui/projects/paddock-ui/i18n` for T6 | 1 | F9 |
| F10 | `pd-table` T1, T2, T3, T5, T6, gallery, specs, changelog; Dyno's whole e2e suite, the shell demo's and `ui/e2e` on both engines | `ui/projects/paddock-ui/table/`, `ui/projects/gallery`, `ui/e2e` | 2.5 | **after the Dyno tag** |
| F10b | `pd-table` T4, windowed rows, alone; the same suites | same | 1.5 | F10 |
| F11 | Zip rows, member heads, 14,000-member measurement | `folder_scan/`, `paddock-archive`'s member reader (ADR 0064) | 1.5 | ADR 0064 member reader, after the Dyno tag |
| F12 | Acceptance: the reproduction's `folder_open` worker tests un-ignored and passing, and Enter on the invented 14,255-file bundle (`scripts/perf/bundle/gen_bundle.py`, `examples/folder_repro.rs`) importing the recommended set without the worker stopping; real-engine e2e; the budget table re-run at 3, 4 and 8 threads with plugins; axe, screen readers, screenshots, independent review, docs | `app/e2e*`, `debrief-store-worker/tests/it/`, `performance.md`, `user-guide.md` | 3 | all |
| FR | Rule 17's amendment for D2, if the user agrees: one line, a lesson, the dialog matrix entry | `docs/rules.md`, `docs/lessons.md` | 0.25 | D2 |

**Total about 36.5 days of raw work** (revision 1 said 29; the first review expected 32 to 35, before the gate). Bundle note gaps closed by it: 7 (3 days there) and part of 8 (3 days there, by choosing, not by deduplicating content) and the misleading-reason half of gap 2. What it leaves: the time zone, `ctime` and other shape gaps stay as the note's own rows; they change a row from Not supported to Supported, with no change here.

## Decisions for the user

| # | Decision | Recommendation |
|---|---|---|
| D1 | A single dropped zip with more than one member: tree, or skip like a single file? Your rule says a single dropped file skips it; a bundle in a zip is a folder in a file. | **Tree.** Enter still imports the recommended set in one step. A one-member zip and a `.gz` skip. |
| D2 | The dialog's kind: modeless `tool`, which fits the shared kind table, or modal as rule 17's "pickers that feed the next step" reads, which needs a new kind? | **Modeless `tool`**, and add a line to rule 17 that a picker with a long scan may be a tool. A modal large window would be a shared kind change, and a slow scan would block the main window. |
| D3 | New files under a selected folder in live tail and reload: import what the rules recommend without asking, or always ask? | **Rules, with a notice for anything held back.** Asking on each new log file would break live tail. |
| D4 | In a re-opened dialog: does unchecking an imported file remove it, or is removal only in the Explorer? | **Explorer only.** A checkbox should not delete data; the dialog stays additive. |
| D5 | The same folder in a new workspace: start from the recommendations, or from your last choices there? | **Recommendations.** Choices are stored in the workspace or project. Anything more is a preference file to keep and explain. |
| D6 | Not supported rows: disabled, or checkable so the import can try and then report? | **Disabled.** It cannot import, and offering it breaks the "no promises" rule. "Set up a format…" is the way forward, and a new parser re-judges the row. |
| D7 | A file whose measured memory need cannot fit (over the plugin cap, or over the store worker's whole-file gate of 1,280 MB): unchecked by default, or checked with a warning? And an unmeasured whole-file format: the worst measured factor (22), or a plain size limit? | **Unchecked**, with the need and the limit shown; checked anyway, the gate refuses it instead of letting it stop the worker. **Unmeasured formats use the worst factor, 22**, which starts them unchecked above about 58 MB. The rows go away as ADR 0064 makes inputs stream. |
| D9 | Should a plugin crash during a scan's sniff count toward that plugin's quarantine, as an import's crash does? Browsing a folder could then disable a plugin before anything is imported. | **No.** The scan marks the rows "Can't check", logs the crash and, after 3 crashes in one scan, stops asking that plugin until the scan ends. Only crashes during an import count toward quarantine. |
| D8 | Enter during a scan: import the recommended set once the scan ends, or wait until it ends to allow Enter? | **Import when it ends.** One step is the point, and the job shows both phases. |

## Alternatives considered

- **Scan with file extensions only.** Cheap, and a second source of truth: it would promise what the import refuses. Rejected by the user's rule.
- **A separate quick detector with a small window (for example 4 KiB).** Faster in theory. It decides differently from the import for exactly the files that matter, such as a long header. Rejected: [measured](#4-parallelism-and-cost-er-20-rule-51) reading is not where the time goes.
- **Keep paths as the selection, raise `MAX_QUERY_SOURCES` and `MAX_OPEN_PATHS`.** Moves the same problem up: a 14,000-path IPC reply, a state file growing by path, no way to say "this folder". Rejected.
- **A new tree grid or `MatTree` with columns.** Rule 42 and 47: `pd-table` has the Material base. `MatTree` has no columns. Rejected.
- **The UI computes rollups from loaded rows.** Violates "the engine is the source of truth" and fails with paged rows.
- **Filter-first, no tree, as a flat list with folder facets.** Simpler, loses the tree the user asked for.
- **Import everything, then let the person remove.** Today's approach with bulk remove. It spends the whole import on what the person did not want, and leaves the note's 11,458 rows.

## Risks

- **The scan's verdict cost with plugins is taken from ADR 0064's measurement, not run on this corpus.** F12's benchmark checks it, and if it misses, the budget and this ADR's first answer change.
- **Files named alike across many folders** can flag false duplicates. The flag only changes a default, and it says "likely".
- **Rule pack judgement.** A wrong default in the pack costs a person one click, or a missing file they do not notice. The totals by reason and the rule ids in the logs are the guard. Its first version needs the user's bundle to tune (by the user, never in the repo).
- **`pd-table` is shared, and widely.** 37 tracked files outside specs and Markdown use it at `852b35ac0`, 25 of them in Dyno [S, counted with `git grep`]. T1 to T6 are opt-in inputs, but F10 must run Dyno's whole e2e suite (`apps/dyno/app/e2e/`, including `tables.spec.ts`), the shell demo's, `ui/e2e/data.spec.ts` and the table's 1,187-line spec, on both engines. T4 (windowed rows) touches the table's line model and is its own package, F10b, so it can be reviewed and reverted alone. Nothing lands before the tag.
- **A mixed folder gives per-file rules**, so a worst-case selection is one rule per file. Measured for the mix, not for an adversarial one. Bounded by `folder.scan_entries`.
- **One dialog per folder open adds a step a person might not need** on small folders; Enter keeps it to one key, and the user asked for it on every folder.

## What this ADR did not verify

- The scan through plugins: the probe runs the in-process importers only, without EVTX, dump or registry sniffs over IPC; section 4 uses ADR 0064's measured cost.
- ADR 0066's probe inside the scan, and the head tally's cost: neither exists yet.
- The memory factors: taken from the reproduction's results [R], not re-run here, and measured for EVTX, the semicolon export and dumps only; every other whole-file importer gets the conservative default until FM measures it.
- The link-refusing open: designed, not built; its Windows half is untested until F7.
- F1's saving and the budgets' last column: both are [calc].
- Any cold cache, spinning disk or network share. All numbers are warm, on one loaded machine with an external SSD.
- `pd-table` with 14,000 tree rows in a real browser: the JS costs are the model's, without Angular's change detection or layout. Smoothness is F12's check.
- Screen reader announcements (T6).
- That `Summary` is a subset of `All` in a real bundle, and how the rule pack does on one.
- Whether re-reading restores source ids after a folder removal in every case, and how anchored notes behave after undo.
- The dialog on Windows and Linux: the mockups are macOS-framed drawings, not renders of the component.
