# 66. How Debrief reads times: provenance per source, a zone per bundle, contradictions raised

- Status: Accepted 2026-11-06 (user), revision 5: D1 to D14 as recommended, full scope. Revision 5 fixes the last diff review of `e447e4753`; revision 4 answered the third review of `4e1fc4afe`, revision 3 the second of `fd622064a`, revision 2 the first of `2f289b4c3`
- Date: 2026-11-05, revised 2026-11-06
- Related: ADR 0006 and 0037 (clock skew on the stored events; live tail), 0027 (plugin SDK, semver), 0032 (zone evidence for generated parsers), 0047 (time-window import), 0048 (watched folders), 0051 (stable source ids), 0062 (index and catalogue during the import), 0065 (bounded replies, revision 4 on this branch, `7cd3a23a6`), 0064 (one streaming import trait, **Accepted 2026-11-06** with option (c) of its D7, revision 3 `c12864566` on `docs/debrief-streaming-trait`), 0067 (folder import dialog, **Accepted 2026-11-06**, D1 to D9 as recommended, revision 5 `d6af93f3b` on `docs/debrief-folder-import-dialog`); [bundle shapes note](../../../../docs/research/2026-11-debrief-bundle-shapes.md) on `docs/debrief-bundle-shapes` `fc6bdf0b7`; Paddock rules 10, 11, 13, 21, 42, 48, 51, 52, 55; ER-19, ER-27
- Evidence marks: **[M]** measured on invented data, in the [results block](../performance.md#2026-11-05-time-zone-libraries-and-re-timing-debrief-adr-0066), harnesses in `docs/measurements/adr-0066/`; **[S]** read in source at `origin/debrief/1.2-int` 852b35ac0, paths under `apps/debrief/`; a short path such as `profile.rs` means under `crates/debrief-core/src/`, and any other crate is named; **[D]** primary doc or registry data, read 2026-11-05; **[N]** from the bundle shapes note, not re-run; **[calc]** arithmetic on numbers marked elsewhere; **[J]** judgement. Sizes in days are judgement.

The user decided on 2026-11-05: a time zone per bundle, with daylight saving handled, using a time zone database. Later that day they widened it into an ER-21 redesign of how Debrief reads times, because silent wrong times keep coming back. This ADR is the design and the details left to decide.

## Answers

| Question | Answer |
|---|---|
| Which library? | **`jiff`** with its bundled database, used only to build a plain table. `chrono` stays for parsing. |
| Where does the zone come from? | A ladder: the user; then a named zone in a system report or registry export, checked against numeric offsets on neighbouring logs; then those neighbours alone; then weak hints. **A `Z`, `UTC` or epoch stamp is not evidence of the machine's zone**, `+00:00` is weak evidence that never outranks a report, and a report that strong neighbours contradict drops to Low and is never applied on its own. Extends ADR 0032's `zone::detect`, which today only serves "Generate with AI". |
| What if the guess is wrong? | The zone, how sure it is and what it rests on are always shown at the root of the logs. Low confidence is never applied on its own. One click changes it; Undo restores the old times exactly. |
| Time in the DST gap or fold? | **Gap:** the offset before the gap (RFC 5545), flagged. **Fold:** first pass until the clock runs back, then second pass, per file; flagged and counted when the file does not show which. |
| Re-time without re-import? | **Yes.** The event's digits can be recovered from `ts`, so a zone change is a list of `(from, to, delta)` moves on the same segment rewrite that clock skew uses. Undo is the same operation the other way. No new event column. |
| Corrections found at the end of an import? | A phase of the same import job between the sink's `finish` and `maintain`, under the import's own guard and write lock, before compaction and ordering. The sources it corrects get no `sources` row until it swaps, so nobody sees a provisional time and compaction does not drop them (section 8a). |
| Old stores, projects and parsers? | **No migration code** (user, 2026-11-06). The store's `FORMAT_VERSION` takes the next number for this change alone (3 to 4, or 4 to 5 if ADR 0065's change lands first), and both project versions bump (`package.rs:94` for `project.toml`, 1 to 2; `project.rs:20` for `.debriefproj`, 2 to 3). The existing checks refuse a newer file in an older build ("saved by a newer Debrief … update Debrief") and an older store in a newer build ("saved by an older preview of Debrief … open its log files again"), `crates/debrief-store-duck/src/lib.rs:913-923`, `package.rs:162` [S]. An old generated parser with `timezone_source` is refused by name (section 11). |
| Does this fix the 777 files? | **Not by itself.** 60 need the zone. 713 lose an offset written in the line, which `fix/debrief-cap-stop-and-offset` already fixes; 2 more need Z2. The last 2 are the `ctime` exports, another gap. See below. |
| What stops this coming back? | **Four things together:** every time goes through one `TimeReader`, which cannot hand one out without keeping count; each source persists how its times were read; anything that contradicts that raises a typed problem, bounded on any file size; and a corpus of invented shapes checks every importer through the real import path, with a build error for a new importer that states nothing. |
| Why did the day order flip within a file? | `day_first.unwrap_or(first > 12)` (`crates/debrief-plugin-api/src/time.rs:297`). When the sample decides nothing, `None` means **each row decides for itself**: `03/04/2026` is March, `13/04/2026` is April, in one file. The sample is 2,000 rows of a 1 MiB head at most (`importers/csv.rs:271` on `origin/debrief/stream-text`; `ingest/plan.rs:170` for text). |

## Context

**The pattern (ER-21, rule 40).** A decision about how to read a time is made once, from a small part of the file, or not made at all, and then forgotten. What it rested on is not recorded, later rows are not compared with it, and nobody can see or correct it. Three shapes so far, each of which looked like its own bug: local time read as UTC; an offset lost to a regex; day order flipping within a file. Fixing shapes one at a time leaves the fourth to be found by a person noticing a wrong time, if they notice.

**What is lost today** [N], [S]. A timestamp with no offset goes through `TimestampParser::localize` (`crates/debrief-plugin-api/src/time.rs:404-410`) with the parser's `assume_timezone`, which is `"UTC"` in `crates/debrief-core/parsers/generic.toml:77`. The result is an `i64`, and that it had no offset is forgotten. Nothing can tell a local time read as UTC from a real UTC time.

**The 777 files split up** [N]:

| Files | Bytes | Families | What is wrong | What fixes it |
|---:|---:|---|---|---|
| 60 | 106,573,738 (101.6 MiB) | 12, 17, 23, 27, 33, 38, 43, 47 | Digits right, no offset, read as UTC | **The zone** |
| 713 | about 20 MB | 19, 21, 32 | `+offset,` : the regex backtracks, the offset lands in the message (`crates/debrief-core/parsers/generic.toml:64`) | `fix/debrief-cap-stop-and-offset` `6480771b8` |
| 2 | small | 27 | `UTC=+h:mm` is left in the message | A captured offset, no zone needed |
| 2 | 328 MB | 4 | `ctime;...` exports become CSV with no time | Gap 4 of the note, not this ADR |

On the same bundle, **1,950 files carry an offset** in every line (families 8, 9, 10, 16), so neighbours are strong evidence there, provided the offset is numeric and not `Z` (section 4). The system report (family 26) and the 439 MiB registry export (family 5) exist but are refused or fail today, so detection cannot wait for them to be imported.

**What exists** [S]:

- `parser/verify/zone.rs` (ADR 0032): the ladder of offsets in the text, same machine, modification time, text hints, ask. `ZoneGuess { offset, source, confidence, detail }`. Fixed offsets only, used only by "Generate with AI", and `machine_offsets` is filled by no import path.
- `clock_offsets(path, offset_ms)` and `DuckStore::apply_offsets` (`crates/debrief-store-duck/src/skew.rs:126`): the segment rewrite in one catalog transaction, keeping ids and id blocks, renumbering positions, re-indexing (ADR 0037, 0062). It takes one constant delta per source. It has no progress or Cancel, and it updates `sources.ts_from`/`ts_to` by the delta. **It refuses while an import runs** (`self.idle()?` at `skew.rs:127`, which answers `Busy` when `importing` is set, `crates/debrief-store-duck/src/lib.rs:1107-1110`) and holds `write_lock` for the whole rewrite (`skew.rs:130`).
- The import holds the same `write_lock` for its whole length (`crates/debrief-store-duck/src/lib.rs:708`), the `Importing` guard clears `importing` when the job ends however it ends (`lib.rs:1087-1104`), and a source becomes ready, so visible, as soon as its segments are listed, during the import (`crates/debrief-store-duck/src/writer.rs:14`, `:443-453`).
- `SegmentSink` applies an offset to sources begun later (`crates/debrief-store-duck/src/writer.rs:549`).
- `AssumedZone::{Utc, Local, Fixed}` in `debrief-plugin-api`. `Local` means the *analyst's* machine, which is wrong for someone else's bundle and differs by machine.
- `ts_state` is a `UInt8` column holding `TsState` 0 to 2 (`crates/debrief-batch/src/ids.rs:32`); a reader that meets another value fails the batch (`crates/debrief-batch/src/batch.rs:184`). `ctx.file_date` is the file's modification time as a UTC date (`ingest/region.rs:88`). The UI's display choice is `'utc' | 'local'` (`app/src/app/core/format.ts:8`).
- `TimestampIssue` (`timestamp_issue/mod.rs`) flags sources with many untimed events, and `Problems` aggregates counts with at most 20 sample lines per kind (`crates/debrief-plugin-api/src/problems.rs`). Nothing looks at a time that parsed.
- Importers that yield times: about 20 in the core and plugins (`impl ... Importer`, listed in ADR 0064's table), and 16 TOML parsers in `crates/debrief-core/parsers/` [S]. No single place sees all their times: plugin importers call `ctx.timestamps` or `TimestampParser::parse` themselves (`crates/debrief-plugin-syslog/src/syslog.rs:74`), and some use `chrono` directly.
- No "source details" view exists [S]: the Explorer row has badges, a tooltip and **Adjust clock**, which opens `skew/clock-section`.
- "Bundle" is already a word for `.debriefz` shares (`bundle.rs`). In code this ADR says **import root**: a path given to one `open_paths` (`app/src-tauri/src/workspace_commands.rs:219`), a folder the dialog of ADR 0067 opened, or a watched folder (ADR 0048).

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

Under ADR 0064's trait the SDK lends the reader through `ImportCtx` and calls `finish` itself after `import` returns, `Complete` or `Stopped`, so an importer cannot forget it (section "A new importer cannot skip it"). `read` takes `&mut self`, so a reader belongs to one thread: `ctx.times()` gives `&mut TimeReader` from the context of one call. A plugin import is one call on one thread, so it has one reader. A splittable in-process importer parses regions on several threads, and each region's context owns its own reader, seeded with the same head decision; their tallies merge in line order as below, before `finish` builds the one `TimeReport`.

**`TimeTally` merges in line order.** It has a fixed size whatever the file's length (section 12). A region's tally holds its counters, its first and last `(line, wall, instant)`, the first line of each kind of event, and, per fold range, the first run-back and the wall extremes at its edges. Merging two adjacent regions adds their counters and compares A's last row with B's first, which is the one "went backwards" or run-back test the boundary needs. Regions finish in any order on parse threads, so the merger keeps finished regions sorted by first line and merges a region into its neighbours as soon as they are adjacent; what waits is bounded by the regions in flight, about the parse threads plus the queue (rule 13). Sample lines are the lowest line numbers, so the result is the same for every thread count and every order. A proptest checks it (check 5).

**`TimestampParser::parse` stays public.** Revision 1 said it would stop being public outside `plugin-api` and, a section later, that its callers are unchanged; both cannot hold. `debrief-plugin-api` is the SDK and follows semver (ADR 0027 section 1), first-party plugins call `parse` today (`crates/debrief-plugin-syslog/src/syslog.rs:74`), and so do non-import callers in the core (`parser/verify/timestamps.rs`, `timestamp_issue/suggest.rs`, `related/time.rs`). So `parse` keeps its signature and meaning, the reader is the documented way for importers, and what keeps importers on it is the source scan (check 4) and the provenance-against-store test (check 3), not visibility.

### 2. Keep the wall clock apart from the instant

Parsing answers one of two things:

```rust
pub enum Stamp {
    /// The text says which instant.
    Instant { ms: i64, written: Written },
    /// Digits only, kept as if they were UTC.
    Wall { ms: i64 },
}
/// What the text said about the instant, and so what it says about the machine's zone (section 4).
pub enum Written {
    /// `+09:00`, `-0500`, `UTC=+9:00`. A non-zero offset is strong evidence of the writer's zone at that instant.
    Offset { minutes: i16 },
    /// `+00:00` or `+0000` written as a number: weak evidence, as it is also what a logger set to UTC prints.
    OffsetZero,
    /// `Z`, `UTC`, `GMT`, and `-00:00`, which RFC 3339 section 4.3 defines as "local offset unknown": the logger chose
    /// UTC or did not know. Not evidence of the machine's zone.
    UtcDesignator,
    /// Epoch, uptime and native clocks. Not evidence of the machine's zone.
    Clock,
}
```

`TimestampParser` gains `parse_stamp`, and `parse` becomes `parse_stamp` plus `localize`, so other callers are unchanged. `plugin-api` learns no zone names: a new `AssumedZone::Inherit` reads digits and answers `Wall`, so **plugin binaries do not link the database**.

`assume_timezone` gains `"inherit"`, which becomes the value in `generic.toml` and `FormatCfg::default`. A parser may name a zone (`"Asia/Tokyo"`, resolved by the host). `"local"` stays for a person reading their own machine's logs. `parse_offset` also accepts `+h:mm`. **Generated parsers** (ADR 0032, "Generate with AI") write `"inherit"` unless the person confirmed a zone in that dialog; its `timezone_source` and `timezone_confidence` go (section 11).

**Order of authority, first match wins:**

1. An offset written in the line (`Stamp::Instant`).
2. A zone the **user confirmed** for the path (section 3, origin `User`).
3. A parser's explicit `assume_timezone` (`"UTC"`, `"+08:00"`, `"Asia/Tokyo"`, `"local"`).
4. A detected zone for the path, when applied (section 5).
5. As written.

So a person who says "these logs are Tokyo" is obeyed even where a shipped or generated parser said `"UTC"`, and only a written offset outranks them, because it records what the machine did. A parser that wants to win over a detected zone still does. **When it does, it says so:** the zone reading is `ZoneSource::Parser` (a new value beside `SystemReport` and `Neighbours`), and where detection found something else the chip reads "the parser says UTC" with the detected zone offered as a choice. Whether a parser's explicit `"UTC"` should outrank detection at all is D12.

### 3. The zone, by path

```rust
pub enum LogZone {
    /// Unknown. Digits are read as UTC, and it says so everywhere.
    AsWritten,
    Fixed { minutes: i16 },
    Named { name: ZoneName },   // parsed once at the edge, checked against the database
}
```

`CREATE TABLE zone_settings (path VARCHAR PRIMARY KEY, zone VARCHAR NOT NULL, origin VARCHAR NOT NULL, confidence VARCHAR NOT NULL, tzdb VARCHAR NOT NULL, set_ms BIGINT NOT NULL)`, plus `import_roots(path)`, both in the schema of this change's format version.

**The longest matching path prefix wins.** The import root gives the bundle's zone; a folder gives one machine's zone in a bundle of several; a file gives "per source when needed". There is no special case, and it follows `clock_offsets` in being keyed by path, stable across reloads. The lookup is ADR 0067's one helper over its `RelPrefix` type, shared with that ADR's selection rules (rule 55). It travels in both files that carry `clock_offsets` today, as a new key beside it: `project.toml` of a self-contained project (`package.rs:130`; its `PROJECT_VERSION` at `package.rs:94` goes 1 to 2) and the `.debriefproj` workspace file (`project.rs`, which holds `ClockOffsets`; its `PROJECT_VERSION` at `project.rs:20` goes 2 to 3). An older Debrief refuses either instead of opening it with the zones silently dropped (`package.rs:162`; `project.rs` refuses newer files, its line 11). A file of the old version simply has no zones; reading an absent key is not a migration.

### 4. Where the zone comes from

`zone::detect` grows and stays pure: the evidence is gathered by the caller. New `ZoneSource` values: `SystemReport`, `Neighbours`, `Parser` (section 2). `Confidence` becomes an enum (`Unknown`, `Low`, `Medium`, `High`, `Confirmed`) for the zone; a number could not say which behaviour applies. `detail` becomes typed evidence items so the UI words them in i18n.

**Which neighbours count.** Only `Written::Offset` (non-zero) is **strong** evidence: it is what the machine's own clock and zone produced at that instant. `Written::OffsetZero` is **weak**: right for a machine in UTC, in Iceland or in UK winter, and also what a logger configured for UTC prints. `UtcDesignator` and `Clock` are **not evidence**: they say how the logger was set, not where the machine was. They are counted in provenance and shown as "N sources write UTC (not evidence)".

| # | Evidence | Reads | Result | Confidence |
|---:|---|---|---|---|
| 1 | The user | Dialog | Whatever was chosen | Confirmed |
| 2 | Registry export `...\Control\TimeZoneInformation\TimeZoneKeyName`, or the system report's `Time Zone` line, **and strong neighbours that agree** | Bounded probe, below; names through a CLDR table | Named | High |
| 3 | The same with **no strong neighbours** (none at all, only weak ones, or only `Z` and epoch) | Same | Named | Medium: it says what the machine was set to when the report was made, not when the logs were written. Weak neighbours are listed as weak and are not counted against it |
| 4 | The same, and **strong neighbours disagree** at the same instants | Same | Both candidates | **Low**: never applied on its own; shown as a conflict with both candidates and their evidence |
| 5 | Strong neighbour offsets fit exactly one class of zones over the logs' time range, with at least two different offsets | The tally, below | Named (representative of the class) | High |
| 6 | Strong neighbour offsets fit several classes, or all strong samples share one offset | The tally | Fixed offset | Medium |
| 7 | Only weak neighbours (`+00:00`) | The tally | Fixed 0 | Low |
| 8 | Modification time against the last line, the same quarter hour in several files (ADR 0032) | `zone.rs` | Fixed offset | Low: a copy resets file times [N] |
| 9 | Text hints such as `JST` | `zone.rs` | Fixed offset | Low |
| 10 | Nothing | | `AsWritten` | Unknown |

**Conflict.** Revision 1 let the neighbours win a conflict at Medium, so a guess would have been applied against a report. It is now rung 4: the person chooses, with both shown. Offsets that differ by folder at the same instant mean several machines: per-folder zones, shown as such, or a question when folders do not separate them. Rung 4 is decided per folder in that case, so one machine's agreement is not undone by another's disagreement. **Where it is recorded.** A *folder* here is a first path level under the import root; files directly in the root belong to the root. When every folder that has evidence of its own agrees, the detected zone goes on the root, so folders with no evidence inherit it through the prefix lookup. Each folder whose evidence disagrees gets its own `zone_settings` row, an explicit `AsWritten` with the conflict as a suggestion, so it does not inherit the root's zone.

**A report found after neighbours were applied** (the probe's later scan, D3) is checked the same way: if it disagrees with a zone already applied from neighbours, it raises `ZoneEvidenceConflict` and offers its zone; it does not move anything on its own. The later scan is a bulk job of its own, package Z8b.

**Neighbours.** The reader keeps, per source, the offsets it saw with their first and last instant and a count (`offsets` in section 11), from `Stamp::Instant`, tagged strong or weak. Candidates are all database zones whose offset fits every strong `(instant, offset)` sample, grouped by having equal transitions over the logs' range: the choice within a class changes no time at all. Cost is no concern: loading all 598 zones took 2.5 ms and testing them against two samples 16 µs. Two samples, +2 in winter and +3 in summer, fit 26 zones, and one sample of +9 fits 12 [M]. How many classes those form was not measured, so rung 5 against rung 6 is decided by the code at run time, not assumed here.

**The probe.** It runs as **phase 3 of ADR 0067's scan**, "Looking for the time zone", which picks its files from the scan's table instead of walking again and never holds up the other phases (0067 section 3). An open that does not go through the dialog (a dropped file, the CLI, a launch path, a watched folder) runs the same probe on `discover`'s listing of the root before parsing. Either way it reads only files by name and kind (`*.reg`, `*.nfo`, `msinfo*`, `systeminfo*`), for one key, and a head of 200 lines from up to 20 small text files for offsets, within the D3 budget. The offsets are read with the new `TimeReader`, which gives each one with its instant and its `Written` class, not with today's `zone::offsets_in`, which keeps no instant and reads `Z`, `UTC` and `GMT` as an offset of 0 (`parser/verify/zone.rs:86-89`, `:168-172`) [S], the very confusion section 4 removes. `offsets_in` stays for ADR 0032's generator until Z8 moves it onto the reader. UTF-16 is expected; a full registry export has the key deep in the file, so it streams to the budget and gives up with `not measured` rather than a wrong answer. It is a bulk job phase with a label and Cancel (rule 52: utility level, as a person may wait for it). The import does not wait beyond the budget. When the probe answers after parsing began, its zone applies to files not yet read, and the files already read are moved in the import's own end-of-import phase (section 8a); until then they are visible "as written". An answer after the import ended is a move as one job (section 8).

**Windows names.** Windows gives `Tokyo Standard Time`, not `Asia/Tokyo`. Jiff has this table privately (`src/tz/system/windows/windows_zones.rs` in jiff 0.2.39) [S]. Debrief ships `windows-zones.toml` as a pack (rule 21), generated by an `xtask` from CLDR `windowsZones.xml`, territory 001, about 142 rows [D] ([source](https://raw.githubusercontent.com/unicode-org/cldr/main/common/supplemental/windowsZones.xml)). A test checks that every target exists in the database. A display name that is localized or absent from the table falls back to the `(UTC+09:00)` text as a fixed offset, Medium. `Bias`, `StandardBias`, `DaylightBias` in the export (UTC = local + bias, per [Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/timezoneapi/ns-timezoneapi-dynamic_time_zone_information) [D]) serve as a cross-check. The display names on real localized systems were not verified.

### 5. Applying a guess, and showing a wrong one

| Confidence | Applied? | Shown |
|---|---|---|
| High | Yes | Quiet chip on the root row: "Asia/Tokyo · from system report". Undo in the notice |
| Medium | Yes | Chip with "check": the notice says what it rests on |
| Low, including a conflict (rung 4) | **No**, offered as a suggestion, with both candidates for a conflict | Home "Needs attention" |
| Unknown | No | Chip: "Time zone unknown: times as written", with Choose |

Choosing opens the Clock dialog (`skew/clock-section`, a real window, rule 17), which gains a Time zone section: scope (these logs, this folder, this file), an autocomplete over `list_time_zones` from the engine, with fixed offsets and "as written"; the evidence list; and a preview with the count of events that would move and any problems under each candidate.

**A wrong zone shows itself** in checks kept per source and listed in the same dialog, all pure over counts:

- Events in a gap: a machine on this zone could not have written them, so these are evidence against the zone.
- Neighbours in the same folder with a written **non-zero** offset (`Written::Offset`) disagree with the zone's offset at the same instant. `OffsetZero` is listed as weak evidence and never counted against a zone; `UtcDesignator` and `Clock` are not evidence at all. This is the one `+00:00` rule, used by section 4, this list, `OffsetDisagreesWithZone` and the corpus.
- Events later than the plausibility bound of ADR 0032 (15 h after the file's modification time).
- A fold passed with no clock running back in a busy file.

Any of these lowers the shown confidence and adds a `Problem` to the source. **It never un-applies a zone:** a High or Medium zone that was applied stays applied until the person chooses otherwise; the chip changes to "check" and the problem offers Correct…. Only the person's choice, or a later probe answer before anything was shown, changes the times. An event's details show "written 02:30:00, read as Asia/Tokyo (+09:00)". The grid shows no per-row mark in the normal case, as a chip at the root is what a person can act on [J].

### 6. Gaps and folds, defined

Mapping is a pure function on wall-clock ms:

- **Normal.** One offset.
- **Gap** (`2026-03-29 02:30` where clocks go from 02:00 to 03:00): use the offset *before* the gap, as [RFC 5545 section 3.3.5](https://www.rfc-editor.org/rfc/rfc5545#section-3.3.5) says, the same result as jiff's `compatible` [D]. Sets `GAP`. Counted as a problem: it suggests a wrong zone.
- **Fold** (`02:30` occurs twice): RFC 5545 says first occurrence, which for one line alone is all one can do. A log has more to go on: lines are in order. Per file, in `line` order, among lines in the fold's wall range: first offset until the clock runs back by at least half the fold length, then second offset. Small jumps from threads writing out of order do not count. Only jumps with both lines inside the range count, so a circular log's wrap does not.
- **No run-back seen** (the file starts inside the fold, or holds only one pass): first offset, `FOLD_UNSURE` set, counted, and shown as "12 events in the repeated hour could be an hour later".

Flags share `ts_state`, which has room: low two bits `TsState`, `4` `WALL`, `8` `GAP`, `16` `FOLD_UNSURE`, `32` `DATE_AMBIG` (section 7), `64` `CONTRADICTS` (section 7). `TsState::from_code` masks; `crates/debrief-store-duck/src/diag.rs:24`'s `ts_state = ?` and the batch check (`crates/debrief-batch/src/batch.rs:184`) change with it. **No new column, one format bump of its own:** an older build would fail its batch check on the new values, so the store's `FORMAT_VERSION` takes the next number (3 to 4, or 4 to 5 after ADR 0065's instance 12) and the existing check refuses either side (section "Answers"). Each schema change takes its own number; whichever lands second takes the next.

**At import** the conversion is stateless per row: one table lookup, first offset in a fold. So it is the same whichever parse thread reads which region (rule 13). A source with rows in a fold range, or with a late day order (section 7), gets its correction at the end of the import (section 8a).

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

- **A disambiguating value comes after the sample, and no opposite one exists.** The earlier `DATE_AMBIG` rows were read under the default. At the end of the import a move corrects them (section 8a), and `DayOrderLate` (Info) records the line. This is the reported CSV bug, now right and said out loud.
- **Both kinds of value exist.** The file contradicts itself, and no order is right for all of it. The order with more votes wins. **On equal votes, the order of the first disambiguating row in line order wins**: the tally keeps the first line of each kind, so the answer does not depend on threads or merge order. A row over 12 the other way is read in its only legal order and set `CONTRADICTS`; `DayOrderContradicted` (Warning) gives counts and sample lines, and the day order's confidence is Low. Today such a row becomes untimed or flips silently.
- **Nothing disambiguates.** The sequence decides if it can: per source, the tally counts how often time ran backwards under each order. Logs run forwards, so if one order has almost none and the other many, that order is used, with source `Sequence` and confidence Medium. Otherwise, the default with `DayOrderUndecided` (Warning) and confidence Low. This is two prior values and two comparisons per ambiguous row, nothing for other rows. A log covering 12 days or fewer of one month runs forward under both orders, so it stays undecided: order alone cannot tell, and the problem says so.

`detect_day_first` and `day_order` stay as the head step. Procmon's own use (`crates/debrief-plugin-windows/src/procmon.rs:277`) moves to the reader.

### 8. Re-time without re-import

**The rule.** Let `s` be the source's clock offset, `Z` the zone applied. For a `WALL` row, `instant = ts - s`, and the digits are `instant + Z.offset_at(instant)`, or with `GAP`, `instant + Z.offset_before(instant)`. Going from instant to digits is a function even in a fold, so **the digits can always be recovered from `ts`, `s`, `Z` and the flag.** Non-`WALL` rows never move.

So changing `Z_old` to `Z_new` is a composition of two piecewise-constant maps, which is again piecewise constant. A day-order correction adds a third map on the digits: each ambiguous calendar day goes to its swapped day, which is at most 132 ranges a year, limited by the source's range, applied only where `DATE_AMBIG` is set.

```rust
pub struct TimeMove { from: i64, to: i64, delta: i64, only: FlagMask, flags: FlagChange, line_lt: Option<u32> }
pub fn plan(old: &TimeReading, new: &TimeReading, range: (i64, i64), cuts: &[FoldCut]) -> Vec<TimeMove>;
// TimeReading = the zone table and the day order
```

This is plain data, pure, and testable with no database. For a source with a range of a few weeks it is usually one to three moves. Clock skew is then a single move over all time.

`apply_offsets` becomes `apply_moves`: one mechanism for clock skew, zone and day order, not a second rewrite beside it. `shifted_columns` renders moves as nested `CASE` for the store; `WorkspaceStore`, the in-memory reference, applies them per record. What already holds carries over: ids and id blocks stay, the swap is one transaction, a reader sees old or new, positions are renumbered, the full-text documents are replaced (`skew.rs` module docs). Three things change: `ts_state` is rewritten with `ts`; `sources.ts_from`/`ts_to` are read back rather than shifted; the histogram comes from the rewrite (`write_existing` already recomputes stats [S]).

**Clock skew stays separate.** `s` stays on top of the zone. If a source has an offset that is a whole number of quarter hours and matches what the zone would now add, the dialog offers to clear it: a person may have worked around this bug with "Adjust clock" [J].

**Undo.** `time_history(seq, path, old_reading, new_reading, origin, tzdb, moved, at_ms)` records each change. Undo is `apply_moves` with the old reading. Nothing is snapshotted: old segments are deleted as today. It is exact because digits are recoverable; the check is a property test that A to B to A gives identical `ts` and `ts_state`, for zones and day orders. Undo is offered in the notice and in the dialog's history, for the last change per path.

**As a job** (rules 10 to 12, ER-19). The zone command returns at once. Phases: "Planning", "Moving events n of m segments", "Ordering", then the index catches up as it does today. Cancel works until the swap, since nothing is visible before it: new files are removed, the old ones stay. This also gives clock skew progress and Cancel, which it lacks. **A change asked for during an import waits for it** ("Waiting for the import to finish" in the Jobs pane) instead of failing with `Busy`, since the public `apply_moves` keeps today's `idle()` check.

**What re-time cannot do**, and says so:

- Events a time window dropped (ADR 0047) are gone. The reply counts sources read under a window: "read again to include events outside the window" (and section 8b).
- A source whose date came from the file's time (section 9) moves wrong when the zone changes its date. Those sources are read again instead, listed in the reply.
- A wrong **format** is not a move: the digits were read wrong. That correction is a parser override and a re-read, the existing `DiagnosticFix` path, offered from the same place.
- A time filter in the UI holds instants. It is cleared with a note when its source moves [J]. Bookmarks and notes hold ids and are unaffected.

### 8a. Corrections at the end of an import

Fold run-backs (section 6) and a late day order (section 7) are known only when a source's regions have merged, after its rows were written; so is a zone the probe found after parsing began (section 4). Today's store path cannot apply them then, for six reasons read in the code at 852b35ac0 [S]:

- `apply_offsets` refuses while `importing` is set (`skew.rs:127`, `lib.rs:1107-1110`) and wants `write_lock`, which the import holds for its whole length (`lib.rs:708`).
- It finds its sources with `sources WHERE path = ? AND ready` (`skew.rs:138`) and rewrites with `WHERE {READY}` (`skew.rs:96`), so a source that is not ready is invisible to it.
- A source has **no `sources` row** until it is ready: `write_ready` appends the row with its diagnostics when the source is listed (`writer.rs:176-190`). So "not ready" cannot be a flag on a row; it is a row not yet written.
- `SegmentSink::finish(self)` consumes the sink (`writer.rs:492`), so reports it held back would be lost with it.
- `maintain` runs compaction before ordering (`lib.rs:416-450`), and compaction keeps only `READY` rows (`compact.rs`, `live_rows` and `rewrite ... WHERE {READY}`, `:115`): rows of a held source would look like an aborted import's and be dropped.
- `apply_offsets` orders the events and bumps the data version itself (`skew.rs:236-238`), which `maintain` and the import do again.

And `Importing` is freely constructible (`Importing(pub(crate) Arc<Inner>)`, built directly in `tail.rs:492`), and `write_lock` is a `Mutex<()>` with no type for "holding it". So the design:

1. **Held sources.** `SegmentSink::completed` (`writer.rs:443-453`) holds back a source whose `TimeReport` says a correction is pending: its segments are listed, its `sources` row and diagnostics are not written. `finish` returns what it held: `fn finish(self) -> Result<Finished>` with `#[must_use] Finished { held: Vec<SourceReport> }`. A sink is built with a hold policy: the import's sink holds; the tail's sinks (built at `tail.rs:308` for `append_regions` and `:632` for `import_at_end`, finished at `:394` and `:675`) are built with `Hold::Never`, as section 8b corrects a tail by a job after the append, so they never hold a source. **The zone is captured once per source**, when the batcher begins it, from the prefix lookup at that moment, and goes into its `TimeReport`; so a probe that answers while a file is mid-read does not split the file between two zones, and the old reading of any later move is always the one in the source's stored provenance, never a fresh lookup.
2. **The phase.** After `finish` and before `maintain`, under the store's `heavy` lock and the import's memory limit as compaction and ordering are, the import runs "Correcting times n of m segments" for the held sources and for sources a late probe answer moves. It calls `apply_moves_in_import(&Importing, &Writing, &moves, &held)`: the rewrite core of `apply_moves` without `idle()`, without ordering and without the version bump. Its sources are the held ids plus ready ones, looked up by id from the sink's reports, not by `AND ready`; its rewrite predicate is `{READY} OR source IN (<held ids>)`.
3. **The swap.** One catalog transaction swaps the rewritten segments, inserts the held sources' `sources` and `diagnostics` rows (what `write_ready` would have written, with the corrected `ts_from`/`ts_to`), writes their `source_time` rows and updates those of moved ready sources. From then on they are ready like any other.
4. **Then `maintain`.** `heavy` is a non-re-entrant `Mutex<()>` (`lib.rs:321`, made at `:971`) that `maintain` takes itself at `lib.rs:426`, so the phase takes it through a `Heavy<'_>` guard and hands that guard to `maintain`, which gains a `Heavy<'_>` parameter in place of its own `lock` (other callers take the guard first). Otherwise unchanged: compaction now sees every row as `READY`, ordering runs once, on corrected times, and the import bumps the version once. The import's `ImportReport` (`lib.rs:744-752`) takes each source's `from` and `to` after the phase, not the sink's.
5. **Guards as types.** Both live in a `guard` module, are `#[doc(hidden)] pub`, and have **named private fields** (`Importing { inner }`, `Writing { guard }`), so code outside the module cannot build them. A `trybuild` UI test with a `.stderr` snapshot shows that a struct literal outside the module is refused because the field is private (stable rustdoc's `compile_fail` does not check error codes, so it is not used for this). `Importing::begin(&Arc<Inner>) -> Option<Importing>` swaps the flag and `Importing::begin_bulk` also sets `bulk_import`, replacing `Importing::bulk` (`lib.rs:1093`, used at `:1135` and `tail.rs:581`); they are the only constructors. `tail.rs`, `open` and `reload` use them, and `Importing` is passed into `Inner::import`. `Writing<'a>` wraps the `MutexGuard` that the import takes at `lib.rs:708`, built only by `Inner::writing(&self)`; the `Mutex<()>` is not re-entrant, so the phase uses the import's own `Writing` and never takes the lock again. `apply_moves_in_import` cannot be called without both.
6. **Cancel and errors.** Today a cancelled import keeps the sources it finished usable: `maintain` skips compaction but still orders (`lib.rs:414-415`). The phase keeps that promise (D14). A cancel before the phase starts, a cancel during it, or an error in it (I/O, a full disk) discards the phase's new files and **makes the held sources ready uncorrected**: one catalog transaction writes their `sources`, `diagnostics` and `source_time` rows as read, with `FOLD_UNSURE` kept on the fold rows, and Correct… offered. A day order that was decided but whose correction did not run keeps the decided order and its first line in `source_time` and raises `DayOrderNotApplied` (section 12), so the earlier `DATE_AMBIG` rows are known to be read under the default. Ready sources a late probe answer would have moved keep `source_time` at `AsWritten`, which is what their rows are. An error is logged with its cause chain and the step it failed at (ER-27), and the import's notice says the times of N sources were not corrected and why. If even that transaction fails, the held sources are reported in `unrecognized` with `NOT_IMPORTED`, so the next import reads them (`lib.rs:1143`), and not in `report.sources`, which today takes every `Outcome::Imported` (`lib.rs:746-750`). A kill during the phase leaves unlisted segment files, which the next open of the store removes (`recover`, `lib.rs:826-828`), and held sources with no row: their rows in listed segments stay hidden by the view until compaction rewrites those segments, as an aborted source's do today (`writer.rs:20-21`). After reopening, the store tells the person which sources are missing ("N files were not finished: read them again"), and the next import reads them; ready sources a late probe answer would have moved are offered the found zone as a suggestion.

**What a person sees meanwhile.** A held source is shown as still reading in the Explorer, like any source not finished, and no query, export or search answers with its provisional times. A ready source a late probe answer will move is visible "as written" with the chip "Time zone unknown: times as written" until the phase swaps it.

**The index (ADR 0062).** At 852b35ac0 there is no tee: the sink gets `None` for its observer (`lib.rs:709-715`) and the indexer starts only when the import releases the lock (0062, Context). So today the phase needs nothing from the index: the rewritten segments are unindexed like any new ones. When the tee lands (0062 WP-5, plan package **IX-B2**), the tee may have indexed held rows; they stay hidden by the live filter because their source is not ready, and the phase deletes the moved blocks' documents through the tee's writer and marks the segments unindexed. That part of Z4 waits for IX-B2, or IX-B2's brief carries it if it lands later.

**Fold cuts for a zone not known at import.** When the zone arrives later (a late probe answer in the phase, or a person's choice), the reader never tallied fold run-backs under it. `plan` then gets its `FoldCut`s from a store search: per source, the rows in each fold range of the new zone in line order, looking for the run-back, the query measured at 0.05 s for one fold over 5,000,000 rows [M].

**Cost.** Only segments that hold rows of the corrected sources are rewritten. A fold comes once a year per zone (the autumn fall-back; spring is a gap), and late day orders are rare, so most imports have no such phase [J]. Its worst case is the segment question of "Parallelism and cost".

**Tests.** With compaction on: one import of a fold-crossing file, a `df_late` file and ordinary files, so that mixed segments hold ready and held sources. No reader sees the held rows during the phase (a query through a crash point); after it, every event of every source is present once, in order, and equals a re-import with the order and zone given. A cancel before the phase, a cancel in it and an injected write error each leave the held sources ready and uncorrected, flagged, with the cause in the log and nothing half-corrected. A probe answer injected while a file is mid-read gives that file one zone, the one it began with. The tail's sinks never hold. A test that `Importing` and `Writing` cannot be built outside their constructors (a `compile_fail` doc test).

### 8b. Live tail, watched folders and the time window

- **Live tail (ADR 0037).** Appended lines are read with the source's stored reading: its zone and day order from `source_time`, not decided again. Their tally merges into the source's provenance. `source_time` keeps, for each fold range the source has rows in, whether the run-back was seen and at which line, so a tail can continue a fold it is in the middle of. A row in a fold with no run-back yet takes the first offset with `FOLD_UNSURE`; when a later append shows the run-back, the earlier rows of that fold get a move as one job after the append (the tail holds `Importing` while it appends, `tail.rs:492`). A row that contradicts the stored day order is read in its only legal order with `CONTRADICTS` and a problem; the order is not decided again during a tail. Live tail ignores the time window, as ADR 0047 says.
- **Watched folders (ADR 0048).** The watched folder is recorded in `import_roots` when watching starts, so each file it opens one at a time takes that folder's zone by the prefix lookup. The probe runs for the folder when watching starts, and again when files of a probed kind (section 4) appear in a folder whose zone is still Unknown, such as one that was empty at the start.
- **The time window (ADR 0047).** `RecordBatcher::with_window` drops events outside the window before the pipeline [S]. A `WALL` row read while its path's zone is not applied (`AsWritten`, or a Low guess) may be off by up to 14 hours, so for those rows the window is widened by 14 hours on each side. A `DATE_AMBIG` row is kept if either day order puts it inside the (widened) window, since a late day order may still swap it. Both are counted (`outside_window_kept`, `ambiguous_window_kept`) and the reply says "N events near the window's edges were kept because the logs' time zone or day order was not known". After the correction they are where they belong, some just outside the window; they are kept, as 0047 changes nothing already stored. Rows with an offset, or under an applied zone and a decided day order, use the window as today.

### 9. Time-only logs and `UTC=+h:mm`

**`UTC=+h:mm`.** A named group `offset` beside `timestamp` joins the two captures, removes the field from the message and yields `Stamp::Instant` with `Written::Offset` (or `OffsetZero`). Today "a regex cannot join two captures" [N]. A shipped pattern in `generic.toml` covers `yyyy.MM.dd HH:mm:ss.fff UTC=+h:mm`. These lines also feed the neighbour tally. Same package: the `,?\s+` regex fix for `+offset,`.

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
    counts: TimeCounts,            // timed, inherited, untimed, offset (strong), offset zero (weak), UTC designator, clock, wall, gap, fold unsure, ambiguous, contradicting, went backwards
    offsets: Vec<OffsetSeen>,      // offset, strong or weak, first and last instant, count; at most 16
    folds: Vec<FoldSeen>,          // per fold range with rows: run-back seen, at which line; at most 64 (live tail, section 8b)
    sampled: Sampled,              // lines the plan's head covered, and lines the reader went on to read
    confidence: Confidence,        // the lowest of the parts
}
```

- **Kept** in `source_time(source PRIMARY KEY, provenance VARCHAR)`, as versioned JSON, in the schema of this change's format version, written in the same catalog transaction as the source becoming ready. A missing or unreadable row reads as "not recorded", with Read again as its action. A `version` newer than the build reads is "made by a newer Debrief"; there is no conversion of older versions. It is rewritten by every correction, so it always tells the truth about the stored events. It travels with a re-read; a project reopens through the store.
- **Replaces** `[format] timezone_source` and `timezone_confidence`, which were informational and written only by AI generation. `[format]` has `deny_unknown_fields` (`parser/format.rs:9`), so a parser generated by an older Debrief no longer loads. No conversion: it is refused, listed in the parser list and in Problems as "this parser was generated by an older Debrief: `timezone_source` is no longer read; generate it again, or delete the `timezone_source` and `timezone_confidence` lines".
- **A bounded reply** (ADR 0065). The stored provenance keeps at most 16 offsets, 8 evidence items, 64 fold ranges and a 120-character pattern, each with a count of what it left out. Until ADR 0065's registry exists (its B3), the `source_time` query answers a plain `TimeProvenance`, listed in `bounded-exempt.toml` with the kind `per-source` ("one record of fixed maximum size for the one source asked about; its caps are counted inside it"); 0065's B7 seeds that row, and Z6 does not wait for 0065. 0065's B3 then wraps it: it answers `Bounded<TimeProvenance>` with a cut for each that was hit (`of: "offsets"`, `"zone.evidence"`, `"folds"`, `"format.pattern"`, `Total::Exact` from those counts), on registry rows `time.offsets_kept`, `time.evidence_kept`, `time.folds_kept`, `time.pattern_chars`. `timeNote` in `summary` is a fixed shape and goes in `bounded-exempt.toml` as `fixed-set`. The wrap and the four `time.*` rows are part of 0065's B3 (half a day there), which owns that edit to `zone/report.rs` and `api.rs`.
- **Size.** At most about 2 KiB per source. It is not in `summary`, which already costs 4.4 MB on the sample bundle [N]: `SourceInfo` gets only a compact `timeNote { confidence, zoneShort, problems }`, and a new `source_time(source)` query returns the rest. A roll-up by import root counts sources by confidence.
- **The importer's part** is `TimeReport`, from `TimeReader::finish`. In-process it is carried in `Imported`; on ADR 0064's plugin wire in `ImportDone` (section "Fit with work in progress"). A format with no times says `TimeReport::NoTimes`.

### 12. Contradictions raise a problem

New `ProblemKind`s: part of the plugin contract, so plugins report them through the reader rather than by hand.

| Kind | Severity | Raised when | Explained away when | Action |
|---|---|---|---|---|
| `DayOrderContradicted` | Warning | Rows over 12 both ways | | Choose the order |
| `DayOrderLate` | Info | Decided after the sampled head; earlier rows corrected | | See line; change |
| `DayOrderNotApplied` | Warning | A day order was decided after the head, but the end-of-import correction was cancelled or failed (section 8a): earlier ambiguous rows are read under the default | | Correct… |
| `DayOrderUndecided` | Warning | Nothing decides, default used | Sequence decided it | Choose the order |
| `OffsetDisagreesWithZone` | Warning | A written **non-zero** offset is not the zone's offset at that instant, for the source's zone or a neighbour's in its folder. Checked at the end from `offsets`. Never for `+00:00`, `Z` or a clock (section 5) | | Choose the zone |
| `ZoneEvidenceConflict` | Warning | Rung 4: a report and strong neighbours disagree | | Choose the zone |
| `TimeBasisMixed` | Info | Some rows have an offset, others do not, in one source | | See how read |
| `TimeWentBackwards` | Warning above `BACKWARDS_SHARE` (1 in 100 rows) or a jump over `BACKWARDS_JUMP` (1 h); else no problem, only the count | Backward jump beyond `BACKWARDS_TOLERANCE` (5 s, ADR 0032's tolerance), by line | Within the tolerance; inside a fold; a file where most jumps go backwards (reversed: noted in provenance); exactly one large jump with the file's ends meeting (a circular log: `Info`, noted as wrapped) | See line; Adjust clock |
| `TimeInGap` | Warning | `GAP` rows | | Choose the zone |
| `TimeFoldUnsure` | Info | `FOLD_UNSURE` rows | | See line |
| `TimeImplausible` | Warning | Before 1990, over 10 years before the file time, or over 15 h after it (ADR 0032's bounds) | | Choose the zone or date |
| `DateFromFileTime` | Warning | A time-only or yearless log took its date from the file's time | | Choose the date; Read again |

**Thresholds are engine constants**, `BACKWARDS_TOLERANCE`, `BACKWARDS_SHARE`, `BACKWARDS_JUMP` and the plausibility bounds in `zone/check.rs`, with doc comments and a test on each side of each edge. They are heuristics about logs, not limits on work, so they are not rows of ADR 0065's registry, and they are not Settings until someone finds them noisy (D9).

"Rows past the sample" is not one kind: whichever of these it is, the problem carries the first line and `sampled`, so it reads "first seen at line 3,104,552; the first 2,000 rows were sampled". `UnmatchedLine` and `TimestampUnparsed` stay as they are, for what did not parse.

**Bounded on huge files:**

- Per file: `TimeTally` is fixed-size counters, two previous values and at most 16 offsets, then an `others` count; regions merge in line order (section 1). The normal row costs a few comparisons and allocates nothing. Z1 measures this against text import throughput, target under 2 % [J]; its result goes to `performance.md`.
- Per problem: `Problems` already keeps a count and at most 20 sample lines per kind. Nothing is per row. Samples are the lowest lines, so which are kept does not depend on threads.
- Per store: rows in `diagnostics` are at most kinds times sources, as today. The Problems panel and any notice roll up by kind and root first ("`TimeWentBackwards` in 1,211 files"), with `Bounded` replies (ADR 0065) if the list is cut.
- Checks over the tally run at the end of a source, and over the root at the end of the import, cost proportional to sources, not events.

### 13. Where a person sees it, and the one action

- **A "How times were read" section** at the top of the Clock dialog (`skew/clock-section`, a real window, rule 17), opened from the Explorer row. Since no source-details view exists, this becomes it; the row's button reads **Times…**. It says, in sentences: format; zone, confidence and what it rests on, with evidence that is not evidence named as such; day order and its counts; where the date came from; rows read and rows sampled; what looks wrong. The clock offset is below it.
- **Explorer row and root** carry the chip from section 5, shown only when confidence is under High or a problem exists, so a healthy bundle is quiet. Words, never colour alone. ADR 0067's dialog shows the same chip on its path line.
- **Problems** lists the kinds above in the existing panel (`app/src/app/problems/`). `DiagnosticRow.fix` gains a time form: a row's one action is **Correct…**, which opens the dialog at the right field with the suggested value and a preview count.
- **Event details** say "written 03/04/2026 02:30:00, read as 3 April, Asia/Tokyo (+09:00)".
- **One action, one mechanism.** Save in the dialog is `set_source_time { scope, zone?, day_order? }`: the section 8 job, with Undo in its notice. A format fix goes through the existing parser override and re-read, from the same dialog.

### 14. Pure core

`debrief-core/src/zone/` (moves `parser/verify/zone.rs` here, with a re-export):

- `model.rs`: `Stamp`, `Written`, `LogZone`, `ZoneName`, `Confidence`, `TimeProvenance`.
- `TimeReader` and `TimeTally` sit in `crates/debrief-plugin-api/src/time.rs` beside the parser, without a zone database, since plugins use them.
- `report.rs`: from tally to provenance and problems.
- `table.rs`: `ZoneTable`, a sorted `Vec` of `(wall_from, wall_to, Kind, before, after)` for a range. **Only this file calls jiff.** It builds from `TimeZone::following` and hands out plain numbers; everything else takes a table, so tests can write one by hand.
- `resolve.rs`: wall to instant and back; `FoldCut` from `(line, wall)` in order.
- `plan.rs`: moves, for zone and day order.
- `detect.rs`: the ladder, evidence strength and candidates.
- `check.rs`: problems and their constants.

The adapters are thin: `store-duck` renders SQL and owns tables, the sink looks up by source, and the shell has commands. Port: `Skew` gains `source_time`, `set_source_time`, `undo_source_time`, one row each in `store_port_table!`, `long`-kind or a job. New port rows change the worker wire, so `WIRE_VERSION` (`crates/debrief-store-client/src/port_table.rs:24`) bumps with them.

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

**Which binaries.** `debrief-core` links it, so the app, CLI and store worker do. The measured 0.52 MiB is an upper bound [M, a small test binary, not the `dist` profile]: it was built with `tz-fat`, which Debrief does not need, and `jiff` with `jiff-tzdb-platform` is already in Debrief's lock through `evtx` and `serde_with`, so binaries that link those already carry part of it (on Windows the bundled data) [S, `Cargo.lock`]. Plugins do not link it through `plugin-api` (section 2).

## The check

### The corpus

`crates/debrief-plugins/tests/it/time_corpus/` (that crate sees every built-in plugin over real IPC), driven by `shapes.toml`, which ADR 0067's `judge_agreement.rs` shares. Each row: an id, the shape's source, invented file text or a small generator, the zone set, the **expected UTC of each event**, expected provenance fields, expected problems. Each goes through `import_path`, the app's path, into `DuckStore`, and the test reads events back from the store, so a loss anywhere on the way shows. No row holds real data; lines below are invented, and rules come through POSIX TZ strings so they do not depend on tzdata.

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
| ts11a | header `=== Verbose logging started: 6/1/2026 ...`, then `MSI (s) (1C:2F) [10:00:00:123]: msg` | 07:00:00.123 | wall, date from header | Z10 |
| ts12, resource_blocks | `Date,2026.06.01,10:00:00` then rows | each row 07:00:00 | wall, inherited | gap 11 |
| ts13, day_hour_tool_log | `[ Day=1 Hour=10 Min=0 Sec=0 ]` | untimed, `TimestampUnparsed` | no month: says so | gap 11 |
| dmp1 | invented minidump header | the header's time | native, absolute | |
| evtx | invented small EVTX | FILETIME | native, absolute | |

Time shapes the note did not list, for this redesign:

| Id | File | Expected | Problem |
|---|---|---|---|
| df_late | CSV, over 1 MiB of `03/04/2026` style rows, then `13/04/2026` | All rows day-first | `DayOrderLate`, its line |
| df_late_text | Same, as plain text | Same | Same |
| df_contradicts | `25/11/2026`, later `12/25/2026`, and ambiguous rows | One vote each: the tie goes to the first disambiguating row, so day first; `12/25/2026` read in its only order (25 December) with `CONTRADICTS`; ambiguous rows day first; day order Low | `DayOrderContradicted`, 1 row, its line |
| df_sequence | `03/05`, `04/02`, `04/09`, `05/01`, `06/11` of 2026 | Month first, source `Sequence`, as the other order runs back | |
| df_sequence_day | `05/03`, `02/04`, `09/04`, `01/05`, `11/06` of 2026 | Day first, source `Sequence` | |
| df_undecided | Two rows, `03/04` and `05/06` in rising order under both | Month first | `DayOrderUndecided` |
| df_late_cancelled | `df_late`, with the import cancelled in the end-of-import phase | Rows read as written, the source ready; the decided order and its line kept in `source_time` | `DayOrderNotApplied` |
| off_disagree | `+09:00` rows beside a zone set to XST | By the written offset | `OffsetDisagreesWithZone` |
| off_mixed | Half with offset, half without | Each by its own basis | `TimeBasisMixed` |
| bk_jitter | Threads, 2 s out of order | As written | none |
| bk_reset | A jump of 3 h back outside any fold | As written | `TimeWentBackwards` |
| bk_wrap | A circular log with one wrap | As written, noted wrapped | Info |
| bk_reversed | Newest line first | As written, noted reversed | none |
| plaus_old | Year 1980 | As written | `TimeImplausible` |
| time_only_filetime | Time-only, no date anywhere | Date from file time, in zone | `DateFromFileTime` |
| time_only_midnight | Time-only over midnight on a gap day | Right date, right offset | none |
| window_wall_unknown | Wall rows 13 h either side of a window, zone not known | Kept in the widened band, counted | none; the reply's note |
| window_date_ambig | `03/04/2026` rows with a window on 3 April, day order not yet known | Kept, counted; after a late day order, the rows of 4 March stay, outside the window | none; the reply's note |
| tail_fold | A followed file whose run-back arrives in a later append | Earlier fold rows moved after that append | `TimeFoldUnsure` until then |

**Zone evidence**, through the probe and the tally (rung numbers from section 4); report lines are invented in each format:

| Id | Report says | Neighbours write | Expected zone | Confidence, applied? | Problem |
|---|---|---|---|---|---|
| zr_agree | `Tokyo Standard Time` | `+09:00` | Asia/Tokyo | High, applied | |
| zr_alone | `Tokyo Standard Time` | nothing | Asia/Tokyo | Medium, applied | |
| zr_neighbours_z | `Tokyo Standard Time` | `...Z` on every line, 3 sources | Asia/Tokyo; the `Z` sources listed as not evidence | Medium, applied (rung 3) | |
| zr_neighbours_utc0 | `Tokyo Standard Time` | `+00:00` on every line, 3 sources | Asia/Tokyo; the `+00:00` sources listed as weak, not counted | Medium, applied (rung 3) | **none**: no `OffsetDisagreesWithZone`, no conflict |
| zr_conflict | `Tokyo Standard Time` | `+02:00` and `+03:00` at the same instants | Both candidates offered | Low, **not applied** | `ZoneEvidenceConflict` |
| zr_utc0_only | none | `+00:00` only | Fixed 0 | Low, not applied | |
| zr_parser_utc | `Tokyo Standard Time` | nothing; the parser says `assume_timezone = "UTC"` | UTC, `ZoneSource::Parser`; Asia/Tokyo offered | As the parser says; chip "the parser says UTC" | |
| zr_late_probe | `Tokyo Standard Time`, found after parsing began | nothing | Asia/Tokyo, applied in the end-of-import phase; "as written" until then | Medium | |
| zr_two_machines | none | folder A `+09:00`, folder B `-05:00`, one offset each | Fixed +9 for A, Fixed −5 for B | Medium per folder, applied (rung 6) | |
| zr_report_two_folders | `Tokyo Standard Time` | folder A `+09:00` and `+09:00` across the range; folder B `-05:00`; folder C no offsets | Asia/Tokyo on the root, so A and C read Tokyo (C through the root); B's prefix an explicit `AsWritten`, with Tokyo and Fixed −5 offered | Root High, applied; B Low, not applied, and B does not inherit Tokyo | `ZoneEvidenceConflict` on B |

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

1. **Build.** ADR 0064 (Accepted) has `import` return `ImportEnd`, an enum (`Complete`, `Stopped { why, at_byte }`), and its wire answer is `ImportDone { skipped, problems, end, bytes_read }` [S, 0064 r3]. A required field cannot go into that enum, so this ADR asks 0064 for an amendment instead: the `Importer` trait gains `fn times(&self) -> TimeBasis` **with no default** (`Reads` or `NoTimes { why }`), so a new importer does not compile until it says what it does. A required trait method is a semver break of the SDK (ADR 0027), acceptable only because it rides 0064's own trait break, which already changes every importer and the protocol (0064, "Where it lives"); added any later, it would need its own major version; the SDK lends the reader through `ImportCtx` and finishes it after `import` returns; and `ImportDone` gains `time: TimeReport`. `ImportEnd` is unchanged. Until that lands, `Imported.time: Option<TimeReport>`, and the next check treats `None` as a failure.
2. **Completeness test** in `debrief-core/tests/it` for the core's importers and parsers (Z12a) and in `debrief-plugins/tests/it` for plugins (Z12b), like 0064's: it lists every registered importer and every shipped parser from the registries themselves and fails for any without a corpus row or a `no-times` entry with a reason.
3. **Provenance matches reality.** For each corpus file: `counts.timed` equals the events with a time in the store, and `wall` equals the rows flagged. A report that says one thing while rows say another fails. This is what catches an importer that declares `Reads` and then reads times another way.
4. **Source scan**, a Debrief test: `.and_utc()`, `Local.`, `from_local_datetime` and `TimestampParser::parse` in `crates/debrief-plugin-*/src`, `importers/` and `ingest/` fail unless marked `// time: <why>`. It reads the real files, in the style of `paddock-conformance`; if it grows to a second app it moves to `./dev`. Clippy's shared `lint/clippy.toml` is left alone, as it binds other apps.
5. **Properties (proptest).** Any table and rows: A to B to A identical, for zones and day orders; `plan` covers every instant exactly once; a move never touches a non-`WALL` row, and a day swap never touches a row without `DATE_AMBIG`; `TimeTally` split into any regions and merged in any order gives the same report as one pass; `resolve` never panics at `i64` extremes; offsets stay within ±14 h.
6. **Store.** `set_source_time` over mixed segments; a reader during the swap sees old or new only; cancel before the swap changes nothing; failure injection on the swap, as in `skew.rs`; the end-of-import phase of section 8a; a project round trip, and a version 2 project refused by a version 1 reader; a format-3 store refused with "open its log files again"; `WorkspaceStore` and `DuckStore` agree on `/samples` (differential, as 3.3 uses).
7. **Detection.** A table of evidence to result for every rung, and each conflict, including the zone evidence table above; a report in UTF-16 with an invented key name; a registry key past the budget gives no answer, not a wrong one.
8. **UI.** The dialog against the mock; Undo; chip per confidence; a conflict with both candidates; Correct… from a problem; the mock is never the reference.

**Red tests first** (rule 5): `local_read_as_utc` on a bracket log; `utc_field_is_read`; `df_late` on `origin/debrief/stream-text`, failing today at the row past the head. Reviewers run the mutation check (rule 5).

**What still gets through:** an importer that passes its corpus row but has another, unsampled path to a time; and a shape nobody has invented yet. The problems of section 12 are what catches those at run time.

## Fit with work in progress

**`fix/debrief-cap-stop-and-offset`** (`6480771b8`, on origin, not merged). It adds a regex to `generic.toml` in front of the generic one and `tests/it/offset_before_comma.rs`. It stands: it lands first, and its comma cases become corpus rows, with its test kept. Z2 shrinks to the `offset` group and `h:mm`. Z1 changes only the `assume_timezone` line in that file, so no conflict [S]. Its regex wants a date and time separated by a space or `T`, so `date,time+offset` stays with `log4net-csv`, as it says.

**`origin/debrief/stream-text` and `importers/csv.rs`.** The flip's cause is in `TimestampParser::parse` and is not specific to CSV. The fix is in the reader, so `ingest/delimited.rs` (on that branch only) and `importers/text.rs` get it through one call. In `csv.rs`, only `with_day_first(detect_day_first(...))` at `:271` changes, to seeding the reader from the head; `CsvLayout::fold` calls the reader instead of the parser. **Order:** `debrief/stream-text` lands first, since it is further along, and Z1 goes on top. A red test on that branch is written now.

**ADR 0064** (Accepted 2026-11-06, revision 3 `c12864566`). They reinforce each other. As 0064 is accepted, what this ADR needs from it is an amendment for its author, not a change made here:

| 0064 says | This ADR asks | Amendment to 0064 |
|---|---|---|
| `import` returns `ImportEnd`, an enum; `ImportDone { skipped, problems, end, bytes_read }` | Each importer states its times, and the report reaches the host | `fn times(&self) -> TimeBasis` with no default, inside 0064's own trait break (an SDK major either way); the SDK lends one reader per call or region and finishes it; `ImportDone` gains `time: TimeReport`; its completeness test links the corpus. `ImportEnd` is unchanged |
| `ImportCtx` is passed as `&ImportCtx` | `ctx.times()` hands out `&mut TimeReader`, as `read` takes `&mut self` | Either `import` takes `&mut ImportCtx`, or the context holds the reader behind a `RefCell` (it is used on one thread per call or region); the first is plainer and is part of the same trait break |
| `EventSink::problem(kind, line)` | The reader's problems flush through it at `finish` | None; it fits |
| `WireEvent` loses `id` and gains `raw` and `byte_offset` | `Record.ts` plus basis flags cross in the same wire change, so there is one plugin wire version, not two | A line in 0064's wire section; Z11 rides on 0064's plugin package |
| `ctx.head` at most 1 MiB; "Procmon's day order from `ctx.head`" | The head **seeds** the reader; it is not the decision | That one line of 0064's table, and `CsvImporter`'s |

If this ADR lands first, the `Option` form holds until 0064's amendment, at about half a day in its package. Neither blocks the other [J]. When the event log opens under 0064's D7 option (c) changes nothing here.

**ADR 0067** (Accepted 2026-11-06). Its scan runs this ADR's probe as phase 3, its path line shows this ADR's chip, it records the dialog's root in `import_roots`, its `RelPrefix` helper is the `zone_settings` lookup, and its F2 moves onto `TimeReader` and `TimeTally` when Z1 lands (0067 sections 2 and 3) [S, 0067 r5]. Nothing here changes 0067.

**ADR 0065** (revision 4). Two touch points, neither a wait. `source_time` answers a plain `TimeProvenance` until 0065's B3 and is wrapped in `Bounded` afterwards (section 11), and the Problems roll-ups use `Bounded` once it exists. Both Z5 and 0065's B5 (instance 3) change `crates/debrief-core/src/skew/store.rs`: whichever lands first lands as it is and the other rebases onto it, and **the owner of `skew/` for both is Z5's agent**, who reviews B5's change there. `TimeReport` is not a bounded reply.

## Packages

Rough days; one agent; failing test first.

| # | Package | Owns | Days | Waits for |
|---|---|---|---|---|
| Z0 | Corpus skeleton, `shapes.toml`, `import_path` runner, the note's shapes with today's verdicts, red tests | `debrief-plugins/tests/it/time_corpus` | 1.5 | |
| Z1 | `TimeReader`, `TimeTally` with the line-order merge, `Stamp`, `Written`, `AssumedZone::Inherit`, flags in `ts_state`, one day order per source with its tie-break, sequence evidence, batcher keeps it, throughput measured | `crates/debrief-plugin-api/src/time.rs`, `crates/debrief-batch/src/ids.rs`, `ingest/batcher.rs`, `importers/{text,csv,lines}.rs`, `ingest/delimited.rs`, `parser/format.rs` | 3.5 | stream-text landing |
| Z2 | `offset` group, `h:mm` (the comma is on the fix branch); the comments that say "read as UTC" corrected to "inherit" in `android-logcat.toml:4`, `apache-error.toml:4`, `log4net-pattern.toml:2`, `nlog.toml:2`; in `generic.toml`, the comment at `:76` and the `timezone_source` and `timezone_confidence` comment block at `:78-81` | `crates/debrief-core/parsers/`, `importers/text.rs` | 0.5 | fix branch, Z1 |
| Z3 | `zone/` core, `jiff`, `ZoneTable`, resolve, plan with day swaps | `debrief-core/src/zone`, `Cargo.toml` by the integrating agent | 2.5 | |
| Z4 | Conversion at import, the order of authority, `zone_settings`, `import_roots`, this change's `FORMAT_VERSION`, both project versions; the end-of-import phase of section 8a (held reports from `finish`, the `READY OR held` predicate, the swap that writes the held rows, `Importing::begin` and `Writing` in a `guard` module, the hold policy, cancel and errors made ready uncorrected, the compaction test); the widened window of 8b with `DATE_AMBIG` | `ingest/batcher.rs`, `crates/debrief-store-duck/src/{writer,schema,lib,skew,tail}.rs`, `package.rs`, `project.rs` | 6 | Z1, Z3; its tee part waits for plan IX-B2 (ADR 0062 WP-5), or IX-B2 carries it |
| Z5 | `apply_moves`, job with phases and Cancel, waiting behind an import, Undo, `time_history`, port rows and the `WIRE_VERSION` bump, CLI `--time-zone`; **first, the segment gate** below | `crates/debrief-store-duck/src/skew.rs`, `skew/`, `crates/debrief-store-client/src/port_table.rs` | 3.5 | Z4. Shares `skew/store.rs` with ADR 0065's B5: whichever is second rebases; Z5's agent owns `skew/` |
| Z5b | Wall sources in segments of their own, if the gate says so | `crates/debrief-store-duck/src/{writer,segments}.rs` | 2.5 | the gate |
| Z6 | Provenance: model with fold run-backs, `source_time`, `timeNote` in summary, the query as a plain `TimeProvenance` (0065's B7 seeds its exempt row, 0065's B3 wraps it), root roll-up; the old-parser refusal | `zone/model.rs`, `zone/report.rs`, `crates/debrief-store-duck/src/schema.rs`, `api.rs` | 2 | Z1 |
| Z7 | Problem kinds, severities and constants, end-of-source and end-of-root checks, `DiagnosticRow.fix` time form | `crates/debrief-plugin-api/src/problems.rs`, `zone/{report,check}.rs`, `crates/debrief-store-duck/src/diag.rs` | 2 | Z1, Z6 |
| Z8 | Detection: ladder with evidence strength and conflicts, `ZoneSource::Parser`, the probe as 0067's phase 3 and on `discover`'s listing, the watched-folder re-probe, candidates, Windows pack | `zone/detect.rs`, `packs/`, `ingest/discover.rs`, `folder_scan/run.rs` (with 0067's F4), `watch.rs` | 3.5 | Z6; 0067 F4 for the dialog path |
| Z8b | The probe's later scan past its budget as a bulk job, and a late report checked against an applied zone (`ZoneEvidenceConflict`) | `zone/detect.rs`, `ingest/discover.rs`, `app/src-tauri/src/workspace_commands.rs` (the job's start) | 1 | Z8 |
| Z9 | UI: "How times were read", chips, conflict view, Correct… from Problems, picker, Undo notice, event details, display choice, mock | `skew/`, `shell/sidebar.ts`, `problems/`, `core/format.ts`, `settings/general-settings.ts` | 4 | Z5, Z7 |
| Z10 | Time-only: date sources, time-only regex, an installer parser | `importers/lines.rs`, `profile.rs`, `crates/debrief-core/parsers/` | 1.5 | Z1 |
| Z11 | Every plugin importer through the reader; `times()` and the basis flags on the plugin wire | `crates/debrief-plugin-api`, `-plugin-host`, plugins | 2.5 | Z1; 0064's amendment and plugin package |
| Z12a | Corpus complete for the core's importers and the 16 parsers, their completeness test, provenance-against-store test, source scan on core paths, errors on | `debrief-core/tests/it`, `debrief-plugins/tests/it/time_corpus` (core rows) | 1.5 | Z1, Z7 |
| Z12b | The same for every plugin importer, over real IPC, and the source scan on `crates/debrief-plugin-*` | `debrief-plugins/tests/it` | 1.5 | Z11, Z12a |
| Z13 | Live tail and watched folders (section 8b) | `crates/debrief-store-duck/src/tail.rs`, `watch.rs`, `workspace/tail` | 1.5 | Z4, Z5 |

**About 41 days raw with Z5b, 38.5 if the gate rules it out**, against 39.5 in revision 3, 36.5 in revision 2, 29.5 in revision 1, about 17.5 before the widening and the note's rough 3 for gap 6. Revision 4 adds the cancel and error path, the hold policy and the guard module (Z4 +0.5) and the later scan (Z8b, 1). With the 20 % that ADR 0064 used: about 49.2, or 46.2 without Z5b. **By concern**, adding to 41: zone with DST and re-time 20.5 (Z3 2.5, Z4 6, Z5 3.5, Z5b 2.5, Z8 3.5, Z8b 1, Z13 1.5); provenance and problems 4 (Z6, Z7); UI 4 (Z9); corpus and checks 4.5 (Z0, Z12a, Z12b); reading and shapes 5.5 (Z1, Z2, Z10); plugin importers 2.5 (Z11).

**Order.** The comma fix lands on its own branch regardless. Z0 first, as it shows everything red. Then Z1 and Z2, which stop the flip and the loss of offsets for every text and CSV file: about 5.5 days, and this alone ends the silent flips. **A smaller first cut** is Z0 to Z7 with Z5b if the gate wants it, Z12a and the dialog part of Z9 (about 27.5 days with Z5b, 25 without): zones chosen by hand, with provenance, problems and the check for the core's importers and parsers; no detection. Outside itself it waits only for `debrief/stream-text` (Z1) and the comma fix branch (Z2), both Debrief-only; nothing of ADR 0065 and nothing behind the Dyno tag. Z8, Z10, Z11, Z12b and Z13 follow; until Z12b, plugin rows are listed as pending.

Nothing here touches shared crates, and the two touch points with ADR 0065 are decoupled (Z6's plain reply until B3; the rebase rule for `skew/store.rs`), so nothing waits for the Dyno tag, apart from Debrief's own `Cargo.toml` and lock. The `Bounded` wrap of `source_time` follows 0065's B3, which does wait for the tag.

**Overlap.** Gaps 3, 6 and part of 10 in the note. `related/time.rs:44` reads `Local` for its own purpose and should take the source's zone in a later change, a known second reader of `assume_timezone`.

## Parallelism and cost (rule 51)

**More workers.** Each segment rewrites alone, so today's serial loop (`for l in &affected`) could run a few at a time (SSD I/O, rule 13), within DuckDB's memory limit. Not measured at more than one worker in the store; Z5 measures it before adding any, and any count is a Setting with Auto. A rewrite runs at the bulk level (rule 52), and it should not hold the write lock against readers beyond the swap: the code holds `write_lock` through the whole rewrite today [S], which blocks imports and the indexer, not queries.

**Cheaper.** The rule 51 question is the one that pays:

- The probe sets the zone before parsing, so the usual open writes each event once, right the first time.
- Zones with no transition in a source's range collapse to one constant move, today's path.
- Moves are per source's own range, with one shared `CASE` per zone rather than one per source.
- Sources with no `WALL` row are skipped entirely: the histogram names affected segments, and a tally names wall sources. In the sample bundle, wall rows are 1.3 % of bytes [N].
- Per-event conversion at import is a table lookup with a remembered position, not a library call: under 1 ns against 29 ns [M, the table loop has no date arithmetic in it and may be flattered by the compiler]. A 30-year table for one zone has 121 rows and builds in 11 µs [M].

**What a re-time costs at scale.** Measured on 5,000,000 invented rows, one Parquet file, 4 threads [M]: a plain copy 0.69 s; one zone with 7 transitions over 800 sources 1.30 s; the same with fold cuts 0.76 s, as the source test applies only inside a fold; a run-back search in one fold 0.05 s. So the moves themselves are cheap next to reading and writing. A second run on 2026-11-06 was faster on every row (the zone row 0.49 s) and peaked at 1.02 GiB of physical footprint for the whole script [M], so the rates below are the slower end of a range. **Today's shape is the costly one** in both runs: one `WHEN source = N` per source took 2.41 s (1.63 s in the second run) at 800 sources, so `shifted_columns` should test a set per reading rather than a branch per source, and skew gains from that too. Index rebuild and ordering were not measured here. Carried to Debrief's targets, linearly and with every segment holding some wall rows, which mixed segments make likely [calc, J]:

| Events in the store | Rewrite at 3.85 to 10.19 M rows/s | Plain copy at 7.24 to 10.84 M rows/s | Index catch-up at 0.31 to 0.35 M events/s |
|---:|---:|---:|---:|
| 100,000,000 | about 10 to 26 s | about 9 to 14 s | about 5 min |
| 1,000,000,000 | about 1.6 to 4.3 min | about 1.5 to 2.3 min | about 48 to 54 min |

The index rate is the plan's own baseline (56.2M events indexed 160 to 183 s after an import of 39.7 to 47.6 s, `docs/plans/debrief-1-2.md`, "Search-ready packages", from this file's "Import of an 8 GB folder until search is ready") [calc]. With wall sources in segments of their own and wall rows at 1.3 % (of bytes, not rows [N]), a re-time of 1B events would rewrite about 13M rows, about 3.4 s, and re-index them in under a minute [calc]. That is the case for Z5b, against more and smaller segments for the writer.

**The segment gate (Z5, first step).** Measure `apply_moves` through the real store path on 10M invented events in mixed segments, with `raw` and field columns, at 1 and 4 workers, with memory (physical footprint, rule 26), and the index catch-up after it; record the block in `performance.md`. If the projection for a full re-time of a 100M store is over 60 s of rewrite, or the index catch-up at 1B over 15 minutes, Z5b is done before Z5 continues; otherwise Z5b is dropped and the reason recorded. By the [calc] above the index part trips the gate, so the plan counts Z5b in; the gate can only remove it.

## Logs (ER-27)

Each decision logs: root, zone, origin, confidence, the kinds of evidence with counts, strong, weak and not evidence apart, and `tzdb`. Each re-time and each end-of-import correction logs a correlation id, phases with durations, segments and rows moved, `GAP` and `FOLD_UNSURE` counts, and the outcome, including cancelled. The probe logs which files it read, by kind and size, its budget and result, and "gave up at budget". A wait behind an import logs its start and end. No file contents or paths beyond what today's source logging already writes.

## Consequences

- Local times are read as local, across DST, and a person can see on what grounds. A bundle with one machine usually needs no action.
- A logger set to UTC no longer argues with a system report, and a real disagreement is put to the person instead of decided for them.
- "Time zone unknown" is said out loud where today it is silent. Some people will see a new chip on every folder with no evidence.
- A zone change costs a rewrite of the segments holding those sources, as a large clock-skew change does, and the index catches up after it; at a billion events that is the reason for Z5b.
- A source corrected at the end of its import becomes visible a little later than the others.
- `debrief-core` gains a direct dependency on a crate already in its tree; the app, CLI and worker binaries each grow by at most about 0.5 MiB [M], less where `jiff` is already linked.
- `Local` remains a parser option, for own-machine logs.
- The display gets a third choice, which needs an engine command to feed it.
- `ts_state` stops being a plain enum on disk. With no migration, this change's store format refuses older stores ("open its log files again"), and older builds refuse its stores, `project.toml` version 2 and `.debriefproj` version 3 ("made by a newer Debrief"); old generated parsers with `timezone_source` are refused by name.
- Every importer changes a little, and every future one has one more thing to state.
- Files that were silently wrong now raise problems. Expect the Problems panel to grow on old-style bundles, which is the point, and the roll-up is what keeps it readable.
- A day order can no longer differ row by row; a mixed file shows as contradicted rather than half right.

## Alternatives considered

- **Store digits in a new column.** Simple and always right, but it costs a column in every event, and recovery from `ts` plus one flag is equally exact.
- **Re-parse `raw` to re-time.** `raw` is there, but re-parsing means running parser, pipeline and regexes again, per event, for no gain.
- **Apply zones at query time.** Rejected in ADR 0037 for the same reasons.
- **Per-source zone only.** 700 sources, one click each. The prefix makes a bundle one click and a file still possible.
- **Fixed offset for all, no database.** Wrong across DST by an hour, which is what the user ruled out.
- **Always ask first.** Blocks the import on a dialog; the chip plus Undo gives the same control with no wait [J].
- **Use the system database.** Results would change by machine; Windows has none in this format.
- **Neighbours win a conflict (revision 1).** A logger set to another zone than the machine would silently decide the bundle's times.
- **Correct at the end of the import with the public `apply_moves`, after the import ends.** People would see and export provisional times in between, and the move would contend with the indexer the import just released.
- **Make `TimestampParser::parse` private.** A semver break of the SDK for something the source scan and the store test already check.

## Decisions (accepted by the user on 2026-11-06, as recommended)

- **D1. Applying guesses.** High and Medium automatically, with chip and Undo; Low, including a report-against-neighbours conflict, only as a suggestion (recommended). Or: only High automatically.
- **D2. Scope for 1.2.** All packages, about 41 days raw with Z5b (38.5 if the gate drops it); or Z0 to Z7, Z12a and the dialog (about 27.5 days with Z5b), which waits for nothing of ADR 0065 or the Dyno tag, with detection, plugin importers, tail and watch after (recommended if 1.2 must shrink: it ends silent wrong times, and a person chooses the zone). The first 5.5 days (Z0 to Z2) are worth doing in any case.
- **D3. Probe budget.** Proposed 512 MiB of files and 2 s of wall time per root, then import proceeds and detection may finish later with a re-time. A large registry export may exceed it: if so, a later scan as a bulk job fills it in.
- **D4. Third display choice.** Add "Logs' time zone" (recommended) and whether it is the default for a store with a known zone. Recommendation: yes, as it matches the digits in `message`; UTC and Local remain.
- **D5. Old stores and projects.** Decided by the user on 2026-11-06: no migration; refused with "open its log files again" or "made by a newer Debrief".
- **D6. Unknown zone stays read as UTC**, labelled "as written" (recommended), rather than refusing or using the analyst's zone.
- **D7. Default day order when nothing decides.** Month first, as today's fallback, with a problem (recommended: no change of behaviour on old files, and it says so). Or a Setting, or by locale of the bundle's system report.
- **D8. Late day-order correction during import.** Correct the earlier rows in the end-of-import phase and say so (recommended). Or stop at a problem and let the person press Correct….
- **D9. `TimeWentBackwards` thresholds.** 5 s tolerance, Warning above 1 in 100 rows or a jump over 1 h, as engine constants with edge tests (recommended); a Setting only if they prove noisy.
- **D10. The segment gate.** 60 s of rewrite at 100M or 15 minutes of index catch-up at 1B (recommended, as above), or other thresholds; or decide now to build Z5b and skip the gate.
- **D11. Window edges with an unknown zone or day order.** Keep wall rows within 14 h of the window, and `DATE_AMBIG` rows that either order puts inside, and say so (recommended), or drop them as today and offer "read again" once the zone is set.
- **D12. A hand-written parser's explicit `"UTC"` against detection.** Of the 16 shipped parsers only `generic.toml:77` writes `assume_timezone = "UTC"`; the other 15 take `FormatCfg::default`, also `"UTC"` (`parser/format.rs:47`) [S]. Both become `"inherit"` (section 2), so after Z1 no shipped parser says `"UTC"`, and this decision concerns only parsers a person wrote (or edited) that say `"UTC"`. Keep their word, with the chip "the parser says UTC" and the detected zone offered (recommended: an explicit value was written on purpose, and the disagreement is shown). Or let a High detection outrank it.
- **D13. Amend the accepted ADR 0064.** Ask its author for the amendment of "Fit with work in progress": a required `fn times()` with no default (an SDK major, acceptable only inside 0064's own trait break), `&mut ImportCtx` so `ctx.times()` can lend a reader, `ImportDone.time`, the wire flags and the head line (recommended). Or keep 0064 as accepted and carry `Imported.time: Option<TimeReport>`, checked only by the completeness test.
- **D14. A cancel or error before the end-of-import corrections finish.** Make the held sources ready uncorrected, flagged, with Correct… offered (recommended: a cancelled import keeps what it read, as today). Or run the phase to the end even after a cancel, which makes Cancel wait; or drop them as not imported.
