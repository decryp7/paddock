# 66. How Debrief reads times: provenance per source, a zone per bundle, contradictions raised

- Status: Proposed
- Date: 2026-11-05
- Related: ADR 0006 and 0037 (clock skew on the stored events), 0032 (zone evidence for generated parsers), 0047 (time-window import), 0051 (stable source ids), 0062 (full-text index holds times),
  0065 (bounded replies), 0064 (one streaming import trait, in draft on `docs/debrief-streaming-trait`); [bundle shapes note](../../../../docs/research/2026-11-debrief-bundle-shapes.md) on `docs/debrief-bundle-shapes` `fc6bdf0b7`; Paddock rules 10, 11, 13, 21, 42, 48, 51, 55; ER-27
- Evidence marks: **[M]** measured on invented data, in the [results block](../performance.md#2026-11-05-time-zone-libraries-and-re-timing-debrief-adr-0066); **[S]** read in source at `origin/debrief/1.2-int` 852b35ac0, paths under `apps/debrief/`, and a short path such as `profile.rs` means under `crates/debrief-core/src/` unless a crate is named; **[D]** primary doc or registry data, read 2026-11-05; **[N]** from the bundle shapes note, not re-run; **[J]** judgement. Sizes in days are judgement.

The user decided on 2026-11-05: a time zone per bundle, with daylight saving handled, using a time zone database. Later that day they widened it into an ER-21 redesign of how Debrief reads times, because silent wrong times keep coming back. This ADR is the design and the details left to decide.

## Answers

| Question | Answer |
|---|---|
| Which library? | **`jiff`** with its bundled database, used only to build a plain table. `chrono` stays for parsing. |
| Where does the zone come from? | A ladder: the user; then a named zone in a system report or registry export, checked against offsets on neighbouring logs; then the neighbours alone; then weak hints. Extends ADR 0032's `zone::detect`, which today only serves "Generate with AI". |
| What if the guess is wrong? | The zone, how sure it is and what it rests on are always shown at the root of the logs. Low confidence is never applied on its own. One click changes it; Undo restores the old times exactly. |
| Time in the DST gap or fold? | **Gap:** the offset before the gap (RFC 5545), flagged. **Fold:** first pass until the clock runs back, then second pass, per file; flagged and counted when the file does not show which. |
| Re-time without re-import? | **Yes.** The event's digits can be recovered from `ts`, so a zone change is a list of `(from, to, delta)` moves on the same segment rewrite that clock skew uses. Undo is the same operation the other way. No new event column. |
| Does this fix the 777 files? | **Not by itself.** 60 need the zone. 713 lose an offset written in the line, which `fix/debrief-cap-stop-and-offset` already fixes; 2 more need Z2. The last 2 are the `ctime` exports, another gap. See below. |
| What stops this coming back? | **Four things together:** every time goes through one `TimeReader`, which cannot hand one out without keeping count; each source persists how its times were read; anything that contradicts that raises a typed problem, bounded on any file size; and a corpus of invented shapes checks every importer through the real import path, with a build error for a new importer that states nothing. |
| Why did the day order flip within a file? | `day_first.unwrap_or(first > 12)` (`debrief-plugin-api/src/time.rs:297`). When the sample decides nothing, `None` means **each row decides for itself**: `03/04/2026` is March, `13/04/2026` is April, in one file. The sample is 2,000 rows of a 1 MiB head at most (`importers/csv.rs:271` on `origin/debrief/stream-text`; `plan.rs:170` for text). |

## Context

**The pattern (ER-21, rule 40).** A decision about how to read a time is made once, from a small part of the file, or not made at all, and then forgotten. What it rested on is not recorded, later rows are not compared with it, and nobody can see or correct it. Three shapes so far, each of which looked like its own bug: local time read as UTC; an offset lost to a regex; day order flipping within a file. Fixing shapes one at a time leaves the fourth to be found by a person noticing a wrong time, if they notice.

**What is lost today** [N], [S]. A timestamp with no offset goes through `TimestampParser::localize` (`crates/debrief-plugin-api/src/time.rs:405-411`) with the parser's `assume_timezone`, which is `"UTC"` in `parsers/generic.toml:77`. The result is an `i64`, and that it had no offset is forgotten. Nothing can tell a local time read as UTC from a real UTC time.

**The 777 files split up** [N]:

| Files | Bytes | Families | What is wrong | What fixes it |
|---:|---:|---|---|---|
| 60 | 106,573,738 (101.6 MiB) | 12, 17, 23, 27, 33, 38, 43, 47 | Digits right, no offset, read as UTC | **The zone** |
| 713 | about 20 MB | 19, 21, 32 | `+offset,` : the regex backtracks, the offset lands in the message (`generic.toml:64`) | `fix/debrief-cap-stop-and-offset` `6480771b8` |
| 2 | small | 27 | `UTC=+h:mm` is left in the message | A captured offset, no zone needed |
| 2 | 328 MB | 4 | `ctime;...` exports become CSV with no time | Gap 4 of the note, not this ADR |

On the same bundle, **1,950 files carry an offset** in every line (families 8, 9, 10, 16), so neighbours are strong evidence there. The system report (family 26) and the 439 MiB registry export (family 5) exist but are refused or fail today, so detection cannot wait for them to be imported.

**What exists** [S]:

- `parser/verify/zone.rs` (ADR 0032): the ladder of offsets in the text, same machine, modification time, text hints, ask. `ZoneGuess { offset, source, confidence, detail }`. Fixed offsets only, used only by "Generate with AI", and `machine_offsets` is filled by no import path.
- `clock_offsets(path, offset_ms)` and `DuckStore::apply_offsets` (`crates/debrief-store-duck/src/skew.rs:126`): the segment rewrite in one catalog transaction, keeping ids and id blocks, renumbering positions, re-indexing (ADR 0037, 0062). It takes one constant delta per source. It has no progress or Cancel, and it updates `sources.ts_from`/`ts_to` by the delta.
- `SegmentSink` applies an offset to sources begun later (`writer.rs:549`).
- `AssumedZone::{Utc, Local, Fixed}` in `debrief-plugin-api`. `Local` means the *analyst's* machine, which is wrong for someone else's bundle and differs by machine.
- `ts_state` is a `UInt8` column holding `TsState` 0 to 2 (`crates/debrief-batch/src/ids.rs:32`). `ctx.file_date` is the file's modification time as a UTC date (`ingest/region.rs:88`). The UI's display choice is `'utc' | 'local'` (`app/src/app/core/format.ts:8`).
- `TimestampIssue` (`timestamp_issue/mod.rs`) flags sources with many untimed events, and `Problems` aggregates counts with at most 20 sample lines per kind (`debrief-plugin-api/src/problems.rs`). Nothing looks at a time that parsed.
- Importers that yield times: about 20 in the core and plugins (`impl ... Importer`, listed in ADR 0064's table), and 16 TOML parsers in `crates/debrief-core/parsers/` [S]. No single place sees all their times: plugin importers call `ctx.timestamps`, and some use `chrono` themselves.
- No "source details" view exists [S]: the Explorer row has badges, a tooltip and **Adjust clock**, which opens `skew/clock-section`.
- "Bundle" is already a word for `.debriefz` shares (`debrief-core/src/bundle.rs`). In code this ADR says **import root**: a path given to one `open_paths` (`app/src-tauri/src/workspace_commands.rs:219`).

## Decision

### 1. One reader, which keeps count

Every importer reads times through `TimeReader`, a thin owner of the `TimestampParser` and a `TimeTally`:

```rust
impl TimeReader {
    /// Reads one stamp, and counts what it saw. No allocation in the normal case.
    pub fn read(&mut self, text: &str, line: u32) -> Option<EventStamp>;
    /// A time the format itself defines (FILETIME, a header, nanoseconds in a protocol).
    pub fn absolute(&mut self, ms: i64, line: u32, kind: NativeClock) -> EventStamp;
    pub fn finish(self) -> TimeReport;
}
```

`TimeTally` has a fixed size whatever the file's length (section 11) and merges, so regions read on different threads give the same answer in any order. It is where every check of section 12 gets its numbers. `TimestampParser::parse` stops being public outside `plugin-api`, so importers have no other way to get a time from text.

### 2. Keep the wall clock apart from the instant

Parsing answers one of two things:

```rust
pub enum Stamp {
    /// The text says which instant: an offset, a `Z`, an epoch or uptime clock. `offset_min` feeds neighbour evidence.
    Instant { ms: i64, offset_min: Option<i16> },
    /// Digits only, kept as if they were UTC.
    Wall { ms: i64 },
}
```

`TimestampParser` gains `parse_stamp`, and `parse` becomes `parse_stamp` plus `localize`, so other callers are unchanged. `plugin-api` learns no zone names: a new `AssumedZone::Inherit` reads digits and answers `Wall`, so **plugin binaries do not link the database**.

`assume_timezone` gains `"inherit"`, which becomes the value in `generic.toml` and `FormatCfg::default`. A parser that says `"UTC"` or `"+08:00"` explicitly keeps its word. A parser may also name a zone (`"Asia/Tokyo"`, resolved by the host). `"local"` stays for a person reading their own machine's logs. `parse_offset` also accepts `+h:mm`.

Order of authority, first match wins: an offset in the line; a parser's explicit `assume_timezone`; a zone set by path (section 3); as written.

### 3. The zone, by path

```rust
pub enum LogZone {
    /// Unknown. Digits are read as UTC, and it says so everywhere.
    AsWritten,
    Fixed { minutes: i16 },
    Named { name: ZoneName },   // parsed once at the edge, checked against the database
}
```

`CREATE TABLE zone_settings (path VARCHAR PRIMARY KEY, zone VARCHAR NOT NULL, origin VARCHAR NOT NULL, confidence VARCHAR NOT NULL, tzdb VARCHAR NOT NULL, set_ms BIGINT NOT NULL)`, plus `import_roots(path)`.

**The longest matching path prefix wins.** The import root gives the bundle's zone; a folder gives one machine's zone in a bundle of several; a file gives "per source when needed". There is no special case, and it follows `clock_offsets` in being keyed by path, stable across reloads. It travels in `project.toml` as a new optional key beside `clock_offsets` (`package.rs:130`).

### 4. Where the zone comes from

`zone::detect` grows and stays pure: the evidence is gathered by the caller. New `ZoneSource` values: `SystemReport`, `Neighbours`. `Confidence` becomes an enum (`Unknown`, `Low`, `Medium`, `High`, `Confirmed`) for the zone; a number could not say which behaviour applies. `detail` becomes typed evidence items so the UI words them in i18n.

| # | Evidence | Reads | Result | Confidence |
|---:|---|---|---|---|
| 1 | The user | Dialog | Whatever was chosen | Confirmed |
| 2 | Registry export `...\Control\TimeZoneInformation\TimeZoneKeyName`, or the system report's `Time Zone` line, plus neighbours that agree | Bounded scan, below; names through a CLDR table | Named | High |
| 3 | The same with no neighbours to compare | Same | Named | Medium: it says what the machine was set to when the report was made, not when the logs were written |
| 4 | Neighbour offsets fit exactly one class of zones over the logs' time range | The tally, below | Named (representative of the class) | High |
| 5 | Neighbour offsets fit several classes, or all samples share one offset | The tally | Fixed offset | Medium |
| 6 | Modification time against the last line, the same quarter hour in several files (ADR 0032) | `zone.rs` | Fixed offset | Low: a copy resets file times [N] |
| 7 | Text hints such as `JST` | `zone.rs` | Fixed offset | Low |
| 8 | Nothing | | `AsWritten` | Unknown |

**Conflict.** A report says one thing and neighbours another: the neighbours' offsets are what the machine really wrote, so they win for the range they cover, confidence drops to Medium, and the conflict is shown. Offsets that differ by folder at the same instant mean several machines: per-folder zones, shown as such, or a question when folders do not separate them.

**Neighbours.** The reader keeps, per source, the offsets it saw with their first and last instant and a count (`offsets` in section 11), from `Stamp::Instant`. Candidates are all database zones whose offset fits every `(instant, offset)` sample, grouped by having equal transitions over the logs' range: the choice within a class changes no time at all. Cost is no concern: loading all 598 zones took 2.5 ms and testing them against two samples 16 µs. Two samples, +2 in winter and +3 in summer, fit 26 zones, and one sample of +9 fits 12 [M]. How many classes those form was not measured, so rung 4 against rung 5 is decided by the code at run time, not assumed here.

**The probe.** Runs when opening a root, before parsing, so that in the common case no re-time follows. Bounded (D3 budget): only files by name and kind (`*.reg`, `*.nfo`, `msinfo*`, `systeminfo*`), reading for one key, and a head of 200 lines from up to 20 small text files for offsets (`zone::offsets_in` exists). UTF-16 is expected; a full registry export has the key deep in the file, so it streams to the budget and gives up with `not measured` rather than a wrong answer. It is a bulk job with a phase label and Cancel. The import does not wait beyond the budget.

**Windows names.** Windows gives `Tokyo Standard Time`, not `Asia/Tokyo`. Jiff has this table privately (`src/tz/system/windows/mod.rs`) [S]. Debrief ships `windows-zones.toml` as a pack (rule 21), generated by an `xtask` from CLDR `windowsZones.xml`, territory 001, about 142 rows [D] ([source](https://raw.githubusercontent.com/unicode-org/cldr/main/common/supplemental/windowsZones.xml)). A test checks that every target exists in the database. A display name that is localized or absent from the table falls back to the `(UTC+09:00)` text as a fixed offset, Medium. `Bias`, `StandardBias`, `DaylightBias` in the export (UTC = local + bias, per [Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/timezoneapi/ns-timezoneapi-dynamic_time_zone_information) [D]) serve as a cross-check. The display names on real localized systems were not verified.

### 5. Applying a guess, and showing a wrong one

| Confidence | Applied? | Shown |
|---|---|---|
| High | Yes | Quiet chip on the root row: "Asia/Tokyo · from system report". Undo in the notice |
| Medium | Yes | Chip with "check": the notice says what it rests on |
| Low | **No**, offered as a suggestion | Home "Needs attention" |
| Unknown | No | Chip: "Time zone unknown: times as written", with Choose |

Choosing opens the Clock dialog (`skew/clock-section`, a real window, rule 17), which gains a Time zone section: scope (these logs, this folder, this file), an autocomplete over `list_time_zones` from the engine, with fixed offsets and "as written"; the evidence list; and a preview with the count of events that would move and any problems under each candidate.

**A wrong zone shows itself** in checks kept per source and listed in the same dialog, all pure over counts:

- Events in a gap: a machine on this zone could not have written them, so these are evidence against the zone.
- Neighbours in the same folder disagree with the zone's offset at the same instant.
- Events later than the plausibility bound of ADR 0032 (15 h after the file's modification time).
- A fold passed with no clock running back in a busy file.

Any of these lowers the shown confidence and adds a `Problem` to the source. An event's details show "written 02:30:00, read as Asia/Tokyo (+09:00)". The grid shows no per-row mark in the normal case, as a chip at the root is what a person can act on [J].

### 6. Gaps and folds, defined

Mapping is a pure function on wall-clock ms:

- **Normal.** One offset.
- **Gap** (`2026-03-29 02:30` where clocks go from 02:00 to 03:00): use the offset *before* the gap, as [RFC 5545 section 3.3.5](https://www.rfc-editor.org/rfc/rfc5545#section-3.3.5) says, the same result as jiff's `compatible` [D]. Sets `GAP`. Counted as a problem: it suggests a wrong zone.
- **Fold** (`02:30` occurs twice): RFC 5545 says first occurrence, which for one line alone is all one can do. A log has more to go on: lines are in order. Per file, in `line` order, among lines in the fold's wall range: first offset until the clock runs back by at least half the fold length, then second offset. Small jumps from threads writing out of order do not count. Only jumps with both lines inside the range count, so a circular log's wrap does not.
- **No run-back seen** (the file starts inside the fold, or holds only one pass): first offset, `FOLD_UNSURE` set, counted, and shown as "12 events in the repeated hour could be an hour later".

Flags share `ts_state`, which has room: low two bits `TsState`, `4` `WALL`, `8` `GAP`, `16` `FOLD_UNSURE`, `32` `DATE_AMBIG` (section 7), `64` `CONTRADICTS` (section 7). `TsState::from_code` masks; `diag.rs:24`'s `ts_state = ?` and the batch check (`batch.rs:184`) change with it. **No new column and no format bump:** old segments hold 0 to 2, meaning "not known to be wall".

**At import** the conversion is stateless per row: one table lookup, first offset in a fold. So it is the same whichever parse thread reads which region (rule 13). A source with rows in a fold range gets the fold step at the end of the import: find the run-back, then a move (section 8). This is a phase with its own progress. Rare, and it costs nothing when no file crosses a fold.

### 7. Day order is decided once per source, and checked to the last row

The per-row fallback goes: **a source has one day order.** A date with a 4-digit year first has no order to decide. For `a/b/yyyy`:

| What the row says | Tally |
|---|---|
| `a > 12` | `first_over_12 += 1`, first line kept |
| `b > 12` | `second_over_12 += 1`, first line kept |
| both at most 12, `a != b` | `ambiguous += 1`, `DATE_AMBIG` set on the row |
| `a == b` | Reads the same either way |

The order and its source, strongest first: the user; the parser or profile; a value above 12 in the sampled head; a value above 12 **anywhere in the file**; the sequence (below); the default, which is month-first as today's fallback gives for a value of 12 or less.

**Past the sample.** The head still decides early, so most rows are read right the first time and nothing waits. Reading continues to count. The cases:

- **A disambiguating value comes after the sample, and no opposite one exists.** The earlier `DATE_AMBIG` rows were read under the default. At the end of the import a move corrects them (section 8), and `DayOrderLate` (Info) records the line. This is the reported CSV bug, now right and said out loud.
- **Both kinds of value exist.** The file contradicts itself, and no order is right for all of it. The order with more votes wins; a row over 12 the other way is read in its only legal order and set `CONTRADICTS`; `DayOrderContradicted` (Warning) gives counts and sample lines. Today such a row becomes untimed or flips silently.
- **Nothing disambiguates.** The sequence decides if it can: per source, the tally counts how often time ran backwards under each order. Logs run forwards, so if one order has almost none and the other many, that order is used, with source `Sequence` and confidence Medium. Otherwise, the default with `DayOrderUndecided` (Warning) and confidence Low. This is two prior values and two comparisons per ambiguous row, nothing for other rows. A log covering 12 days or fewer of one month runs forward under both orders, so it stays undecided: order alone cannot tell, and the problem says so.

`detect_day_first` and `day_order` stay as the head step. Procmon's own use (`procmon.rs:277`) moves to the reader.

### 8. Re-time without re-import

**The rule.** Let `s` be the source's clock offset, `Z` the zone applied. For a `WALL` row, `instant = ts - s`, and the digits are `instant + Z.offset_at(instant)`, or with `GAP`, `instant + Z.offset_before(instant)`. Going from instant to digits is a function even in a fold, so **the digits can always be recovered from `ts`, `s`, `Z` and the flag.** Non-`WALL` rows never move.

So changing `Z_old` to `Z_new` is a composition of two piecewise-constant maps, which is again piecewise constant. A day-order correction adds a third map on the digits: each ambiguous calendar day goes to its swapped day, which is at most 132 ranges a year, limited by the source's range, applied only where `DATE_AMBIG` is set.

```rust
pub struct TimeMove { from: i64, to: i64, delta: i64, only: FlagMask, flags: FlagChange, line_lt: Option<u32> }
pub fn plan(old: &TimeReading, new: &TimeReading, range: (i64, i64), cuts: &[FoldCut]) -> Vec<TimeMove>;
// TimeReading = the zone table and the day order
```

This is plain data, pure, and testable with no database. For a source with a range of a few weeks it is usually one to three moves. Clock skew is then a single move over all time.

`apply_offsets` becomes `apply_moves`: **one mechanism, no copy** (rule 55). `shifted_columns` renders moves as nested `CASE` for the store; `WorkspaceStore`, the in-memory reference, applies them per record. What already holds carries over: ids and id blocks stay, the swap is one transaction, a reader sees old or new, positions are renumbered, the full-text documents are replaced (`skew.rs` module docs). Three things change: `ts_state` is rewritten with `ts`; `sources.ts_from`/`ts_to` are read back rather than shifted; the histogram comes from the rewrite (`write_existing` already recomputes stats [S]).

**Clock skew stays separate.** `s` stays on top of the zone. If a source has an offset that is a whole number of quarter hours and matches what the zone would now add, the dialog offers to clear it: a person may have worked around this bug with "Adjust clock" [J].

**Undo.** `time_history(seq, path, old_reading, new_reading, origin, tzdb, moved, at_ms)` records each change. Undo is `apply_moves` with the old reading. Nothing is snapshotted: old segments are deleted as today. It is exact because digits are recoverable; the check is a property test that A to B to A gives identical `ts` and `ts_state`, for zones and day orders. Undo is offered in the notice and in the dialog's history, for the last change per path.

**As a job** (rules 10 to 12). The zone command returns at once. Phases: "Planning", "Moving events n of m segments", "Ordering", then the index catches up as it does today. Cancel works until the swap, since nothing is visible before it: new files are removed, the old ones stay. This also gives clock skew progress and Cancel, which it lacks.

**What re-time cannot do**, and says so:

- Events a time window dropped (ADR 0047) are gone. The reply counts sources read under a window: "read again to include events outside the window".
- A source whose date came from the file's time (section 9) moves wrong when the zone changes its date. Those sources are read again instead, listed in the reply.
- Stores from before this ADR have no flags. `source_time` has no row for such a source, so it is offered as "read these files again" rather than moved.
- A wrong **format** is not a move: the digits were read wrong. That correction is a parser override and a re-read, the existing `DiagnosticFix` path, offered from the same place.
- A time filter in the UI holds instants. It is cleared with a note when its source moves [J]. Bookmarks and notes hold ids and are unaffected.

### 9. Time-only logs and `UTC=+h:mm`

**`UTC=+h:mm`.** A named group `offset` beside `timestamp` joins the two captures, removes the field from the message and yields `Stamp::Instant`. Today "a regex cannot join two captures" [N]. A shipped pattern in `generic.toml` covers `yyyy.MM.dd HH:mm:ss.fff UTC=+h:mm`. These lines also feed the neighbour tally. Same package: the `,?\s+` regex fix for `+offset,`.

**Time-only.** The date comes from, in order: a date on an earlier line of the file (`Clock.date` exists, `importers/lines.rs:183`); a `date_header` regex in the parser, new, for example the installer log's "logging started" line; a date in the file name; and last the file's modification time, in the zone, not in UTC as today. The last one sets `date_from_file_time` on the source with a `Problem`: a copied bundle gives the copy date [N]. Midnight roll-over stays in the wall domain, where 24 h is correct, and conversion comes after it, so a roll-over on a DST day is right. A regex may mark itself time-only, today only templates can (`profile.rs:475`). Formats without a year (syslog) use the same `reference` and the same flag.

### 10. Showing times

A third display choice, **Logs' time zone**, beside UTC and Local (D4). With it, a person reading a bundle from elsewhere sees the same digits as the log. The engine returns the offset table for the range; the UI does not ask the browser, since `Intl` zone data differs by webview and the engine is the source of truth. Several zones in one store: each row in its source's zone, and the column header says "mixed".

### 11. Provenance per source

Each source records how its times were read. Engine only, one definition, generated to TypeScript (ts-rs).

```rust
pub struct TimeProvenance {
    version: u32,
    format: TimeFormat,            // Template | Regex | CsvColumn | JsonKey | Native(NativeClock), parser id, pattern up to 120 chars
    zone: ZoneReading,             // LogZone, ZoneSource, Confidence, up to 8 typed evidence items
    day_order: DayReading,         // NotApplicable | DayFirst | MonthFirst | Undecided; DayOrderSource; the three counts; first line of each
    date: DateSource,              // InLine | EarlierLine | Header | FileName | FileTime
    counts: TimeCounts,            // timed, inherited, untimed, with offset, wall, gap, fold unsure, ambiguous, contradicting, went backwards
    offsets: Vec<OffsetSeen>,      // offset, first and last instant, count; at most 16
    sampled: Sampled,              // lines the plan's head covered, and lines the reader went on to read
    confidence: Confidence,        // the lowest of the parts
}
```

- **Kept** in `source_time(source PRIMARY KEY, provenance VARCHAR)`, as versioned JSON, in `ADDED_DDL`, written in the same catalog transaction as the source becoming ready. A missing or unreadable row reads as "not recorded", with Read again as its action. It is rewritten by every correction, so it always tells the truth about the stored events. It travels with a re-read; a project reopens through the store.
- **Replaces** `[format] timezone_source` and `timezone_confidence`, which were informational and written only by AI generation.
- **Size.** At most about 2 KiB per source. It is not in `summary`, which already costs 4.4 MB on the sample bundle [N]: `SourceInfo` gets only a compact `timeNote { confidence, zoneShort, problems }`, and a new `source_time(source)` query returns the rest. A roll-up by import root counts sources by confidence.
- **The importer's part** is `TimeReport`, from `TimeReader::finish`, carried in `Imported` and, once ADR 0064 lands, in `ImportEnd`. A format with no times says `TimeReport::NoTimes`.

### 12. Contradictions raise a problem

New `ProblemKind`s: part of the plugin contract, so plugins report them through the reader rather than by hand.

| Kind | Severity | Raised when | Explained away when | Action |
|---|---|---|---|---|
| `DayOrderContradicted` | Warning | Rows over 12 both ways | | Choose the order |
| `DayOrderLate` | Info | Decided after the sampled head; earlier rows corrected | | See line; change |
| `DayOrderUndecided` | Warning | Nothing decides, default used | Sequence decided it | Choose the order |
| `OffsetDisagreesWithZone` | Warning | A written offset is not the zone's offset at that instant, for the source's zone or a neighbour's in its folder. Checked at the end from `offsets` | | Choose the zone |
| `TimeBasisMixed` | Info | Some rows have an offset, others do not, in one source | | See how read |
| `TimeWentBackwards` | Warning above 1 in 100 rows or a jump over 1 h; else no problem, only the count | Backward jump beyond the tolerance, by line | Within 5 s (ADR 0032's tolerance); inside a fold; a file where most jumps go backwards (reversed: noted in provenance); exactly one large jump with the file's ends meeting (a circular log: `Info`, noted as wrapped) | See line; Adjust clock |
| `TimeInGap` | Warning | `GAP` rows | | Choose the zone |
| `TimeFoldUnsure` | Info | `FOLD_UNSURE` rows | | See line |
| `TimeImplausible` | Warning | Before 1990, over 10 years before the file time, or over 15 h after it (ADR 0032's bounds) | | Choose the zone or date |
| `DateFromFileTime` | Warning | A time-only or yearless log took its date from the file's time | | Choose the date; Read again |

"Rows past the sample" is not one kind: whichever of these it is, the problem carries the first line and `sampled`, so it reads "first seen at line 3,104,552; the first 2,000 rows were sampled". `UnmatchedLine` and `TimestampUnparsed` stay as they are, for what did not parse.

**Bounded on huge files:**

- Per file: `TimeTally` is fixed-size counters, two previous values and at most 16 offsets, then an `others` count. The normal row costs a few comparisons and allocates nothing. Z1 measures this against text import throughput, target under 2 % [J]; its result goes to `performance.md`.
- Per problem: `Problems` already keeps a count and at most 20 sample lines per kind. Nothing is per row. The regions merge by line, so which samples are kept does not depend on threads.
- Per store: rows in `diagnostics` are at most kinds times sources, as today. The Problems panel and any notice roll up by kind and root first ("`TimeWentBackwards` in 1,211 files"), with `Bounded` replies (ADR 0065) if the list is cut.
- Checks over the tally run at the end of a source, and over the root at the end of the import, cost proportional to sources, not events.

### 13. Where a person sees it, and the one action

- **A "How times were read" section** at the top of the Clock dialog (`skew/clock-section`, a real window, rule 17), opened from the Explorer row. Since no source-details view exists, this becomes it; the row's button reads **Times…**. It says, in sentences: format; zone, confidence and what it rests on; day order and its counts; where the date came from; rows read and rows sampled; what looks wrong. The clock offset is below it.
- **Explorer row and root** carry the chip from section 5, shown only when confidence is under High or a problem exists, so a healthy bundle is quiet. Words, never colour alone.
- **Problems** lists the kinds above in the existing panel (`app/src/app/problems/`). `DiagnosticRow.fix` gains a time form: a row's one action is **Correct…**, which opens the dialog at the right field with the suggested value and a preview count.
- **Event details** say "written 03/04/2026 02:30:00, read as 3 April, Asia/Tokyo (+09:00)".
- **One action, one mechanism.** Save in the dialog is `set_source_time { scope, zone?, day_order? }`: the section 8 job, with Undo in its notice. A format fix goes through the existing parser override and re-read, from the same dialog.

### 14. Pure core

`debrief-core/src/zone/` (moves `parser/verify/zone.rs` here, with a re-export):

- `model.rs`: `Stamp`, `LogZone`, `ZoneName`, `Confidence`, `TimeProvenance`.
- `TimeReader` and `TimeTally` sit in `debrief-plugin-api/src/time.rs` beside the parser, without a zone database, since plugins use them.
- `report.rs`: from tally to provenance and problems.
- `table.rs`: `ZoneTable`, a sorted `Vec` of `(wall_from, wall_to, Kind, before, after)` for a range. **Only this file calls jiff.** It builds from `TimeZone::following` and hands out plain numbers; everything else takes a table, so tests can write one by hand.
- `resolve.rs`: wall to instant and back; `FoldCut` from `(line, wall)` in order.
- `plan.rs`: moves, for zone and day order.
- `detect.rs`: the ladder and candidates.
- `check.rs`: problems.

The adapters are thin: `store-duck` renders SQL and owns tables, the sink looks up by source, and the shell has commands. Port: `Skew` gains `source_time`, `set_source_time`, `undo_source_time`, one row each in `store_port_table!`, `long`-kind or a job.

### 15. Dependency

Read 2026-11-05 [D]; sizes and speed [M].

| | **`jiff`** + `jiff-tzdb` | `chrono-tz` | `tzdb` / `tz-rs` | None (hand-written, OS database) |
|---|---|---|---|---|
| Licence | Unlicense OR MIT; both on `deny.toml` | MIT OR Apache-2.0 | Apache-2.0; MIT OR Apache-2.0 | |
| Last release | 0.2.39, 2026-11-02; tzdb crate 0.1.9, 2026-10-06 | 0.10.4, **2025-07-11** | 0.7.3, 2025-12-11; 0.7.3, 2026-01-29 | |
| Database inside | 2026e, 206,788 B file [S] | **2025b** [M] | | Windows has none in TZif form |
| Added to a stripped binary | **+0.52 MiB** [M] | +1.17 MiB [M] | | |
| One wall time to instant | 29 ns [M] | 19 ns [M] | | |
| Latest IANA | 2026f, 2026-10-27 ([IANA](https://www.iana.org/time-zones)) | | | |
| Gap detail | `AmbiguousOffset::Gap { before, after }` [M] | `LocalResult::None`, no offsets ([jiff's comparison](https://docs.rs/jiff/latest/jiff/_documentation/comparison/index.html); [M]) | | |
| List transitions | `following`, `preceding` | None public [S] | | |
| Invented zones in tests | `TimeZone::posix`, and the table below was checked with it [M] | No | | |
| Already in Debrief | Yes: in `Cargo.lock` through `evtx` and `serde_with` | No | No | |

**Chosen: `jiff`**, `default-features = false`, features `std`, `tzdb-bundle-always`. Reasons in order: it reports gaps and lists transitions, which the design needs; invented zones make tests independent of tzdata; it is already in the tree (0.2.38 in `Cargo.lock`; 0.2.39 was measured) and its data is fresh; it is smaller. `chrono-tz` is 10 ns faster per call, which does not matter since calls happen only to build the table.

**Which database.** Only `TimeZoneDatabase::bundled()` (`src/tz/db/mod.rs:472`) is used, **not** `TimeZone::get`, which prefers the system copy ([features](https://docs.rs/jiff/latest/jiff/#crate-features) [D]). Cargo's feature unification could switch on `tzdb-zoneinfo` through another crate, so asking by name would then depend on the machine. The same bundle must give the same times on every machine and in every test. A test checks that the zone's source is bundled.

**Fallback.** `chrono-tz`, probing around each wall time for gaps. It would need a separate way to list transitions, and Debrief would stop being able to test with invented rules.

**Update path.** Renovate bumps `jiff-tzdb`; tzdata arrives with an app release. A zone row records `tzdb`. A newer database does not silently move stored events; on re-read they follow it. Data is about past logs, so a late rule change touches few of them [J]. The release checklist reads `jiff_tzdb::VERSION` next to IANA's latest, as a warning.

**Which binaries.** `debrief-core` links it, so the app, CLI and store worker do, at about 0.5 MiB each [M, in a small test binary, not the `dist` profile]. Plugins do not (section 2).

## The check

### The corpus

`crates/debrief-plugins/tests/it/time_corpus/` (that crate sees every built-in plugin over real IPC), driven by `shapes.toml`. Each row: an id, the shape's source, invented file text or a small generator, the zone set, the **expected UTC of each event**, expected provenance fields, expected problems. Each goes through `import_path`, the app's path, into `DuckStore`, and the test reads events back from the store, so a loss anywhere on the way shows. No row holds real data; lines below are invented, and rules come through POSIX TZ strings so they do not depend on tzdata.

**A row's `today` is a ratchet.** A shape not read yet is listed with its pending package and what it does now (`refused`, say); the test asserts exactly that, so when a parser lands the row must flip, and a shape cannot regress quietly.

Zone in the examples: `XST-2XDT,M3.5.0,M10.5.0/3`, so +3 in June.

| Id (from the note) | Line | Expected UTC | Provenance | Pending |
|---|---|---|---|---|
| ts01, app_csv_quoted | `2026/06/01,10:00:00.123+09:00,...` | 01:00:00.123 | offset | |
| app_csv_dst_cross | the same with +02:00 then +03:00 | each by its own | offset; two `offsets` | |
| ts02, ts03, hist_csv_offset, srv_csv_fraction_offset_cp1252 | `2026/06/01 10:00:00.123 +09:00,msg` | 01:00:00.123, offset not in message | offset | fix branch |
| ts04, iso_install_log, ope_cp1252_dump_blocks | `2026/06/01 10:00:00 msg` | 07:00:00 | wall, XST | Z4 |
| ts05, colon_millis_circular | `2026/06/01 10:00:00:123` | 07:00:00.123 | wall | gap 11 |
| ts06, dot_date_utc_field_bom | `2026.06.01 10:00:00.123 UTC=+9:00 msg` | 01:00:00.123, field not in message | offset | Z2 |
| ts07 | `2026-06-01 10:00:00 msg` | 07:00:00 | wall | Z4 |
| ts08, bracket_micros_exception | `[2026-06-01 10:00:00,123456] msg` | 07:00:00.123 | wall | Z4 |
| ts09a, month_first_frames | `06/25/2026 10:00:00` | 2026-06-25 07:00:00 | wall, month first, over 12 | gap 11 |
| ts09b | `06/01/2026 10:00:00` alone | 2026-06-01 07:00:00 | month first, default, Low | gap 11 |
| ts10 | `Mon Jun  1 10:00:00 2026` | 07:00:00 | wall | gap 4 |
| evt_ctime | `Mon Jun  1 10:00:00 2026;0x12;Info;Svc;msg` | 07:00:00 | wall | gap 4 |
| ts11a | header `=== Verbose logging started: 6/1/2026 ...`, then `MSI (s) (1C:2F) [10:00:00:123]: msg` | 07:00:00.123 | wall, date from header | Z8 |
| ts12, resource_blocks | `Date,2026.06.01,10:00:00` then rows | each row 07:00:00 | wall, inherited | gap 11 |
| ts13, day_hour_tool_log | `[ Day=1 Hour=10 Min=0 Sec=0 ]` | untimed, `TimestampUnparsed` | no month: says so | gap 11 |
| dmp1 | invented minidump header | the header's time | native, absolute | |
| evtx | invented small EVTX | FILETIME | native, absolute | |

Time shapes the note did not list, for this redesign:

| Id | File | Expected | Problem |
|---|---|---|---|
| df_late | CSV, over 1 MiB of `03/04/2026` style rows, then `13/04/2026` | All rows day-first | `DayOrderLate`, its line |
| df_late_text | Same, as plain text | Same | Same |
| df_contradicts | `25/11/2026`, later `12/25/2026` | Each in its only order | `DayOrderContradicted`, 1 row, its line |
| df_sequence | `03/05`, `04/02`, `04/09`, `05/01`, `06/11` of 2026 | Month first, source `Sequence`, as the other order runs back | |
| df_sequence_day | `05/03`, `02/04`, `09/04`, `01/05`, `11/06` of 2026 | Day first, source `Sequence` | |
| df_undecided | Two rows, `03/04` and `05/06` in rising order under both | Month first | `DayOrderUndecided` |
| off_disagree | `+09:00` rows beside a zone set to XST | By the written offset | `OffsetDisagreesWithZone` |
| off_mixed | Half with offset, half without | Each by its own basis | `TimeBasisMixed` |
| bk_jitter | Threads, 2 s out of order | As written | none |
| bk_reset | A jump of 3 h back outside any fold | As written | `TimeWentBackwards` |
| bk_wrap | A circular log with one wrap | As written, noted wrapped | Info |
| bk_reversed | Newest line first | As written, noted reversed | none |
| plaus_old | Year 1980 | As written | `TimeImplausible` |
| time_only_filetime | Time-only, no date anywhere | Date from file time, in zone | `DateFromFileTime` |
| time_only_midnight | Time-only over midnight on a gap day | Right date, right offset | none |

**Gaps and folds**, by hand:

| Zone | Wall | Expected UTC | Flags |
|---|---|---|---|
| XST | 2026-03-29 01:59:59 | 2026-03-28 23:59:59 | |
| XST | 2026-03-29 02:30:00 | 2026-03-29 00:30:00 | `GAP`, `TimeInGap` |
| XST | 2026-03-29 03:00:00 | 2026-03-29 00:00:00 | |
| XST | 2026-10-25 02:30:00, clock runs back later | 2026-10-24 23:30:00 | |
| XST | the same text after the run-back | 2026-10-25 00:30:00 | |
| XST | 2026-10-25 02:30:00, file starts here | 2026-10-24 23:30:00 | `FOLD_UNSURE` |
| `LHX-10:30LHY-11,M10.1.0,M4.1.0` (half-hour shift, south) | 2026-10-04 02:15:00 | 2026-10-03 15:45:00 | `GAP` |
| same | 2026-04-05 01:45:00, then again | 2026-04-04 14:45:00, then 15:15:00 | |
| `QQT+3:30QQS,M4.1.0/0,M9.2.0/0` (west, gap at midnight) | 2026-04-05 00:30:00 | 2026-04-05 04:00:00 | `GAP` |
| same | 2026-09-12 23:30:00, then again | 2026-09-13 02:00:00, then 03:00:00 | |

Also a circular file wrapped inside a fold, and threads with 2 s of disorder across a fold.

**Every importer.** One small file per importer that yields times, core and plugin, with an expected instant: text, CSV, JSON, CLEF, `errorlog`, log4j XML, syslog, journald, W3C, access, HAR, OTLP, Docker, CRI, Procmon, Perfmon, EVTX, minidump, `.blg` where a tool exists; and each of the 16 TOML parsers through `parsers/samples/`. Captures and dump documents state their clock class. **All through real IPC** where the importer is a plugin. Which of these carry a wall time or are absolute by format was read from the code only in part; the corpus settles it [J].

### A new importer cannot skip it

1. **Build.** `TimeReport` has no `Default`. In `ImportEnd` (ADR 0064) it is a required field, so a new importer does not compile until it says what it did, including `NoTimes`. Until 0064 lands, `Imported.time: Option<TimeReport>`, and the next check treats `None` as a failure.
2. **Completeness test** in `debrief-plugins/tests/it`, like 0064's: it lists every registered importer and every shipped parser from the registries themselves and fails for any without a corpus row or a `no-times` entry with a reason.
3. **Provenance matches reality.** For each corpus file: `counts.timed` equals the events with a time in the store, and `wall` equals the rows flagged. A report that says one thing while rows say another fails.
4. **Source scan**, a Debrief test: `.and_utc()`, `Local.`, `from_local_datetime` and `TimestampParser::parse` in `crates/debrief-plugin-*/src`, `importers/` and `ingest/` fail unless marked `// time: <why>`. It reads the real files, in the style of `paddock-conformance`; if it grows to a second app it moves to `./dev`. Clippy's shared `lint/clippy.toml` is left alone, as it binds other apps.
5. **Properties (proptest).** Any table and rows: A to B to A identical, for zones and day orders; `plan` covers every instant exactly once; a move never touches a non-`WALL` row, and a day swap never touches a row without `DATE_AMBIG`; `TimeTally` merged in any order gives the same report; `resolve` never panics at `i64` extremes; offsets stay within ±14 h.
6. **Store.** `set_source_time` over mixed segments; a reader during the swap sees old or new only; cancel before the swap changes nothing; failure injection on the swap, as in `skew.rs`; a project round trip; `WorkspaceStore` and `DuckStore` agree on `/samples` (differential, as 3.3 uses).
7. **Detection.** A table of evidence to result for every rung, and each conflict; a report in UTF-16 with an invented key name; a registry key past the budget gives no answer, not a wrong one.
8. **UI.** The dialog against the mock; Undo; chip per confidence; Correct… from a problem; the mock is never the reference.

**Red tests first** (rule 5): `local_read_as_utc` on a bracket log; `utc_field_is_read`; `df_late` on `origin/debrief/stream-text`, failing today at the row past the head. Reviewers run the mutation check (rule 5).

**What still gets through:** an importer that passes its corpus row but has another, unsampled path to a time; and a shape nobody has invented yet. The problems of section 12 are what catches those at run time.

## Fit with work in progress

**`fix/debrief-cap-stop-and-offset`** (`6480771b8`, on origin, not merged). It adds a regex to `generic.toml` in front of the generic one and `tests/it/offset_before_comma.rs`. It stands: it lands first, and its comma cases become corpus rows, with its test kept. Z2 shrinks to the `offset` group and `h:mm`. Z1 changes only the `assume_timezone` line in that file, so no conflict [S]. Its regex wants a date and time separated by a space or `T`, so `date,time+offset` stays with `log4net-csv`, as it says.

**`origin/debrief/stream-text` and `importers/csv.rs`.** The flip's cause is in `TimestampParser::parse` and is not specific to CSV. The fix is in the reader, so `delimited.rs` and `text.rs` get it through one call. In `csv.rs`, only `with_day_first(detect_day_first(...))` at `:271` changes, to seeding the reader from the head; `CsvLayout::fold` calls the reader instead of the parser. **Order:** `debrief/stream-text` lands first, since it is further along, and Z1 goes on top. A red test on that branch is written now.

**ADR 0064** (in review, `origin/docs/debrief-streaming-trait` `0f6e482f5`). They reinforce each other, with four points to agree before either is accepted:

| 0064 says | This ADR asks | Who changes |
|---|---|---|
| `ImportEnd` is what an importer returns | A required `time: TimeReport` | 0064's `ImportEnd`, a line, and its completeness test gains the corpus link |
| `EventSink::problem(kind, line)` | The reader's problems flush through it at `finish` | None; it fits |
| `Record.id` goes from the wire | `Record.ts` plus basis flags cross in the same wire change, so there is one plugin wire bump, not two | Z9 rides on 0064's plugin WP |
| `ctx.head` at most 1 MiB; "Procmon's day order from `ctx.head`" | The head **seeds** the reader; it is not the decision | That one line of 0064's table, and `CsvImporter`'s |

If this ADR lands first, the `Option` form holds until 0064 flips it, at about half a day in its WP. If 0064 lands first, Z1 starts from `ImportEnd`. Neither blocks the other [J]. I did not edit 0064; its author should read this table.

**ADR 0065.** `Bounded` for Problems roll-ups. `TimeReport` is not a bounded reply. Independent.

## Packages

Rough days; one agent; failing test first.

| # | Package | Owns | Days | Waits for |
|---|---|---|---|---|
| Z0 | Corpus skeleton, `shapes.toml`, `import_path` runner, the note's shapes with today's verdicts, red tests | `debrief-plugins/tests/it/time_corpus` | 1.5 | |
| Z1 | `TimeReader`, `TimeTally`, `Stamp`, `AssumedZone::Inherit`, flags in `ts_state`, one day order per source, sequence evidence, batcher keeps it, throughput measured | `plugin-api/time.rs`, `debrief-batch/ids.rs`, `ingest/batcher.rs`, `importers/{text,csv,lines}.rs`, `ingest/delimited.rs`, `parser/format.rs` | 3 | stream-text landing |
| Z2 | `offset` group, `h:mm` (the comma is on the fix branch) | `parsers/generic.toml`, `importers/text.rs` | 0.5 | fix branch, Z1 |
| Z3 | `zone/` core, `jiff`, `ZoneTable`, resolve, plan with day swaps | `debrief-core/src/zone`, `Cargo.toml` by the integrating agent | 2.5 | |
| Z4 | Conversion at import, end-of-import phase for fold and late day order, `zone_settings`, `import_roots` | `ingest/batcher.rs`, `store-duck/writer.rs`, `schema.rs` | 2.5 | Z1, Z3 |
| Z5 | `apply_moves`, job with phases and Cancel, Undo, `time_history`, project key, port rows, CLI `--time-zone` | `store-duck/skew.rs`, `core/skew`, `package.rs`, `port_table.rs` | 3 | Z4 |
| Z6 | Provenance: model, `source_time`, `timeNote` in summary, query, root roll-up | `zone/model.rs`, `zone/report.rs`, `store-duck/schema.rs`, `api.rs` | 1.5 | Z1 |
| Z7 | Problem kinds and severities, end-of-source and end-of-root checks, `DiagnosticRow.fix` time form | `plugin-api/problems.rs`, `zone/report.rs`, `store-duck/diag.rs` | 2 | Z1, Z6 |
| Z8 | Detection: ladder, probe, candidates, Windows pack | `zone/detect.rs`, `packs/`, `ingest/discover.rs` | 2.5 | Z6 |
| Z9 | UI: "How times were read", chips, Correct… from Problems, picker, Undo notice, event details, display choice, mock | `skew/`, `shell/sidebar.ts`, `problems/`, `core/format.ts`, `settings/general-settings.ts` | 4 | Z5, Z7 |
| Z10 | Time-only: date sources, time-only regex, an installer parser | `importers/lines.rs`, `profile.rs`, `parsers/` | 1.5 | Z1 |
| Z11 | Every plugin importer through the reader; the basis flags on the plugin wire | `plugin-api`, `plugin-host`, plugins | 2.5 | Z1; rides on 0064's plugin WP, else its own wire bump |
| Z12 | Corpus complete for every importer and parser, completeness test, provenance-against-store test, source scan, errors on | `debrief-plugins/tests/it`, `debrief-core/tests/it` | 2.5 | Z11 |

**About 29.5 days raw**, against about 17.5 before the widening and the note's rough 3 for gap 6, which predated any ADR and counted neither re-time, detection nor UI. About 35 with the 20 % that ADR 0064 used. **New sizes by concern:** zone with DST and re-time 13; provenance and problems 8; corpus and checks 4; shapes (time-only, `offset`) 2; plugin importers 2.5.

**Order.** The comma fix lands on its own branch regardless. Z0 first, as it shows everything red. Then Z1 and Z2, which stop the flip and the loss of offsets for every text and CSV file: about 5 days, and this alone ends the silent flips. **A smaller first cut** is Z0 to Z7, Z12 and the dialog part of Z9 (about 21 days): zones chosen by hand, with provenance, problems and the check; no detection. Z8, Z10, Z11 follow. The check needs Z11 to be complete for plugins; until then plugin rows are listed as pending.

Nothing here touches shared crates, so nothing waits for the Dyno tag, apart from Debrief's own `Cargo.toml` and lock.

**Overlap.** Gaps 3, 6 and part of 10 in the note. `related/time.rs:44` reads `Local` for its own purpose and should take the source's zone in a later change, a known second reader of `assume_timezone`.

## Parallelism and cost (rule 51)

**More workers.** Each segment rewrites alone, so today's serial loop (`for l in &affected`) could run a few at a time (SSD I/O, rule 13), within DuckDB's memory limit. Not measured at more than one worker in the store; Z5 measures it before adding any, and any count is a Setting with Auto. A rewrite runs at the bulk level (rule 52), and it does not hold the write lock against readers beyond the swap: the code holds `write_lock` through the whole rewrite today [S], which blocks imports, not queries.

**Cheaper.** The rule 51 question is the one that pays:

- The probe sets the zone before parsing, so the usual open writes each event once, right the first time.
- Zones with no transition in a source's range collapse to one constant move, today's path.
- Moves are per source's own range, with one shared `CASE` per zone rather than one per source.
- Sources with no `WALL` row are skipped entirely: the histogram names affected segments, and a tally names wall sources. In the sample bundle, wall rows are 1.3 % of bytes [N].
- Per-event conversion at import is a table lookup with a remembered position, not a library call: under 1 ns against 29 ns [M, the table loop has no date arithmetic in it and may be flattered by the compiler]. A 30-year table for one zone has 121 rows and builds in 11 µs [M].

**Not done yet.** Mixed segments mean that one wall row rewrites a whole segment and re-indexes it. Keeping wall sources in segments of their own would limit a re-time to them, at the cost of more small segments. What a rewrite costs on 5,000,000 invented rows, one Parquet file, 4 threads [M]: a plain copy 0.69 s; one zone with 7 transitions over 800 sources 1.30 s; the same with fold cuts 0.76 s, as the source test applies only inside a fold. So the moves themselves are cheap compared with reading and writing. **Today's shape is the costly one:** one `WHEN source = N` per source took 2.41 s at 800 sources, so `shifted_columns` should test a set per reading rather than a branch per source, and skew gains from that too. Finding a run-back in one fold took 0.05 s. Index rebuild and ordering were not measured. Decide on separate segments from real numbers in Z5, not now.

## Logs (ER-27)

Each decision logs: root, zone, origin, confidence, the kinds of evidence with counts, and `tzdb`. Each re-time logs a correlation id, phases with durations, segments and rows moved, `GAP` and `FOLD_UNSURE` counts, and the outcome, including cancelled. The probe logs which files it read, by kind and size, its budget and result, and "gave up at budget". No file contents or paths beyond what today's source logging already writes.

## Consequences

- Local times are read as local, across DST, and a person can see on what grounds. A bundle with one machine usually needs no action.
- "Time zone unknown" is said out loud where today it is silent. Some people will see a new chip on every folder with no evidence.
- A zone change costs a rewrite of the segments holding those sources, as a large clock-skew change does.
- `debrief-core` gains a direct dependency on a crate already in its tree; the app, CLI and worker binaries each grow by about 0.5 MiB [M].
- `Local` remains a parser option, for own-machine logs.
- The display gets a third choice, which needs an engine command to feed it.
- `ts_state` stops being a plain enum on disk.
- Every importer changes a little, and every future one has one more thing to state.
- Files that were silently wrong now raise problems. Expect the Problems panel to grow on old-style bundles, which is the point, and the roll-up is what keeps it readable.
- A day order can no longer differ row by row; a mixed file shows as contradicted rather than half right.

## Alternatives considered

- **Store digits in a new column.** Simple and always right, but it costs a column in every event and a format bump, and recovery from `ts` plus one flag is equally exact.
- **Re-parse `raw` to re-time.** `raw` is there, but re-parsing means running parser, pipeline and regexes again, per event, for no gain.
- **Apply zones at query time.** Rejected in ADR 0037 for the same reasons.
- **Per-source zone only.** 700 sources, one click each. The prefix makes a bundle one click and a file still possible.
- **Fixed offset for all, no database.** Wrong across DST by an hour, which is what the user ruled out.
- **Always ask first.** Blocks the import on a dialog; the chip plus Undo gives the same control with no wait [J].
- **Use the system database.** Results would change by machine; Windows has none in this format.

## Decisions for the user

- **D1. Applying guesses.** High and Medium automatically, with chip and Undo; Low only as a suggestion (recommended). Or: only High automatically.
- **D2. Scope for 1.2.** All packages, about 29.5 days raw; or Z0 to Z7, Z12 and the dialog (about 21 days), with detection and plugin importers after (recommended if 1.2 must shrink: it ends silent wrong times, and a person chooses the zone). The first 5 days (Z0 to Z2) are worth doing in any case.
- **D3. Probe budget.** Proposed 512 MiB of files and 2 s of wall time per root, then import proceeds and detection may finish later with a re-time. A large registry export may exceed it: if so, a later scan as a bulk job fills it in.
- **D4. Third display choice.** Add "Logs' time zone" (recommended) and whether it is the default for a store with a known zone. Recommendation: yes, as it matches the digits in `message`; UTC and Local remain.
- **D5. Old stores.** Offer "read these files again" (recommended), no migration.
- **D6. Unknown zone stays read as UTC**, labelled "as written" (recommended), rather than refusing or using the analyst's zone.
- **D7. Default day order when nothing decides.** Month first, as today's fallback, with a problem (recommended: no change of behaviour on old files, and it says so). Or a Setting, or by locale of the bundle's system report.
- **D8. Late day-order correction during import.** Correct the earlier rows automatically and say so (recommended). Or stop at a problem and let the person press Correct….
- **D9. `TimeWentBackwards` thresholds.** 5 s tolerance, Warning above 1 in 100 rows or a jump over 1 h. Proposed as engine constants registered with ADR 0065's limits; a Setting if they prove noisy.
