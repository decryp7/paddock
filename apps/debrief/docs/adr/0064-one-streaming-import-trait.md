# 64. One streaming import trait: no importer holds a whole file just to read it

- Status: Accepted (2026-11-06, by the user; D7 with option (c)). Revision 2 answers the independent review of `0f6e482f5`. Revision 3 answers the confirmation review of `570be48a1`
- Amendment 2026-11-12 (user, cut review of 2026-11-12; Tier A of the Debrief 1.2 plan, [section 10.7](../../../../docs/plans/debrief-1-2.md#107-cut-review-tier-a-applied-user-2026-11-12)): less apparatus around the trait, which stays: SI-17 and SI-18 are cut; SI-14 (a static check and a generator for every format) becomes one flat-memory test per input kind; SI-15 drops the nightly 1 billion run; SI-16 deletes old callers as they move. IPC hardening (IP1) is lighter: items 1, 2, 3, 7 and 8 only, with no peer-uid check, no per-variant versions and no nightly all-crate fuzz. Earlier text is history.
- Date: 2026-11-05
- Related: ADR 0014 (database-first engine, `raw`), ADR 0017 (one IPC, host files), ADR 0023 (dumps), ADR 0027 (plugin SDK), ADR 0040 (encodings), ADR 0047 (time
  window), ADR 0051 (streaming discovery), ADR 0052 (stage pipelines), ADR 0062 (index and catalogue during the import), ADR
  0063 (binary wire to the UI, not this wire); family ADRs [0001](../../../../docs/adr/0001-plugin-platform-crates.md) (plugin
  platform), [0003](../../../../docs/adr/0003-debrief-migration-and-users.md) (Debrief's move),
  [0018](../../../../docs/adr/0018-bounded-loading-everywhere.md) (decision 4, "imports stream"),
  [0019](../../../../docs/adr/0019-limits-explain-themselves.md) (limits),
  [0031](../../../../docs/adr/0031-patience-not-wall-clocks.md) (stall judgement), [0034](../../../../docs/adr/0034-one-implementation-per-mechanism.md);
  plan packages 2.6b, 2.6d, 3.4, 3.5, 3.6, PH2a, PH2b, PH4, MEM-1, MEM-3, MEM-4; Paddock rules 10-14, 40, 42, 48, 51, 52, 53, 55, 71; ER-17, ER-18, ER-19,
  ER-20, ER-21, ER-27, ER-33
- Amends: family ADR 0018 decision 4 ("archives extract to a temporary file": only members that must seek), family ADR
  0003's mapping table (`Import`, `ImportProfile`, `ImportDocument` take a typed input), family ADR 0031 §2 (a call waiting for credit is owed by the host)
- Input: [parser research note](../../../../docs/research/2026-11-debrief-parser-streaming.md) on
  `docs/debrief-parser-streaming` `61c2e22a5`; the working streaming CSV on `debrief/stream-text` `3168b6145`
- Evidence marks: **[M]** measured, in a results block of [performance.md](../performance.md#2026-11-05-streaming-trait-measurements-debrief-adr-0064)
  (or the block named); **[S]** read in source at `3168b6145` unless a commit is named; **[R]** from the research note, not re-read;
  **[J]** judgement; **[calc]** worked out from measured numbers; **[U]** not verified. Days are judgement.

## Answers

- **The rule.** No importer holds a whole file in memory just to read it (user, 2026-11-05). Four exceptions, closed:
  heap snapshots and pprof; dumps without a full-memory list, up to 1 GiB; `.etl` and `.blg`, by path; a compressed member
  that must seek, spooled to a private capped temp file and removed afterward. The native plugin gets only the chosen path for
  `.etl`. The minidump upstream bug is not reported.
- **The shape.** One record trait, `Importer::import(ctx, input, sink)`. The importer names its input as a type (`Lines`,
  `Decoded`, `Bytes`, `Seekable` or `ByPath`), the registration turns that type into the manifest's declaration, and the host satisfies
  it. A second small trait takes the same inputs for formats whose result is one document. Output is pushed into a blocking
  `EventSink`; input is pulled. A pull interface for records is not needed and is not in the trait.
- **The mechanism against the next whole read (ER-21).** The old traits and `read_all` go, with every shipped caller decided in [Callers of the old traits](#callers-of-the-old-traits). A whole read needs a reason declared in the manifest, with its cap from the limit registry. A record budget is counted at the framer in the SDK, and a coarser count on the host catches raw `ReadFile`. A completeness test checks flat memory for every registered importer, and a static check covers the source.
- **What the findings got right, and three corrections** are in [Findings checked](#findings-checked). The largest correction: `raw` does not
  become an offset and length. It stays text in the store, and crosses the plugin wire as text.
- **Debrief 1.2's folder of about 14,000 files and 7.6 GiB:** the reading side already handles a text-only folder of that shape, in 41.9 s at 192 MB peak RSS, 83.7 M events, none failed [M, counting sink, no store writer]. The known stops are in the plugin path: its EVTX and `.reg` files, and sniff calls per file. Nothing here has run the store on that folder, so more stops may exist; SI-0 runs it first.
- **A billion events:** the import trait keeps no per-event state, so it is not the ceiling. The store, the index and the queries at that
  size are outside this ADR ([what this does not decide](#what-this-does-not-decide)).

## Context

Debrief imports through eight traits in four families and one adapter. `TextImporter::import(ctx, content: &str)` and `BinaryImporter::import(ctx, bytes: &[u8])`
exist twice (`debrief-plugin-api/src/importer.rs:83,115`, `debrief-core/src/importers/mod.rs:38,76`), `ProfileImporter` and `CaptureImporter` take
`&[u8]`, and only `DocumentImporter` reads by range (`RangeRead`). All of them return collected results (`Imported { records: Vec<Record> }`) except
the three that already stream in the engine: `ingest/text.rs`, `ingest/json.rs` (lines only) and, on `debrief/stream-text`, `ingest/delimited.rs`.
`ingest/legacy.rs` adapts the rest. [S]

Whole-file reading is a recurring class, so this is an ER-21 redesign rather than another instance. ADR 0014 named it on 2026-10-20 (an 8 GB folder took
31 GB). Family ADR 0018 decision 4 repeated it on 2026-10-27. Since then it has come back as the Event Viewer plugin crash (family ADR 0019), the 3 GiB CSV at 17,399 MB, whole `.gz` and zip
expansion (`archive.rs:96,410`), and captures in the UI process (`app/src-tauri/src/profiling.rs:113,196`). Each instance was found by a user or a test on a large file.

What the whole read costs, measured on `debrief/stream-text`
([block](../performance.md#2026-11-05-streaming-csv-import-branch-debriefstream-text-plan-package-35-brought-forward)) [M]:

| 300 MiB CSV | Whole-file read | Streaming |
|---|---|---|
| Peak RSS | 3,535 MB | 68 MB (66-72 over 3 runs) |
| Events per second, one thread | 762,619 | 966,794 to 999,548 |

Where the plugin path stands today [S]:

1. The engine reads the whole file (`ingest/source.rs:335` for binary, `:368` for legacy text) and decodes it.
2. `RemoteImporter` lends that buffer (`remote.rs:192`, `:268`), so the plugin wire only ever sees `content` whole.
3. The plugin copies it again (`HostFile::read_all`, `read_text`, cap `MAX_IMPORT_BYTES` 2 GiB, `serve.rs:25,127-133`), and EVTX a third time
   (`EvtxParser::from_buffer(bytes.to_vec())`, `evtx_import.rs:57`).
4. Items stream with credit, then the host collects them into a `Vec` (`records.extend`, `remote.rs:163`) and only then feeds the batcher (`legacy.rs:34-54`).
5. The whole call has a deadline, 108 s of a 120 s budget (`guard.rs:153`, `remote.rs:36`), so a long honest file is killed.

## Findings checked

| Finding given to this ADR | Checked | Result |
|---|---|---|
| About 45 implementers in four families | Counted with grep over `impl ... for` [S] | **32 format implementers** (4 core text, 14 plugin text, 3 plugin binary, 8 captures behind one adapter, `.etl`, 2 documents), 1 example plugin, 2 test plugins, 7 configuration parsers, 10 proxies and wrappers (9 for importers, 1 for configuration parsers, counting the 2 in `app/src-tauri`), 28 test fakes. The families hold |
| `Vec<Record>` was about 86% of the CSV peak | `performance.md` table [M] | 2,851 of 3,482 MB is 82% of RSS [calc]; I could not reproduce the block's 86%. The conclusion holds |
| No pull interface is needed | `import_text` and `import_delimited` are flat at 62-68 MB [M]; `ProcessTrace` and serde visitors can only push [S, R] | Confirmed. Importers pull input and push output |
| `raw` becomes an offset and length | Read `debrief-store-duck/src/context.rs`, `fts.rs`, ADR 0014 | **Corrected.** `raw` stays in the store. The context view checks the file against it and falls back on it when the file is gone, changed, compressed or in an archive (`debrief-store-duck/src/context.rs:98-137`), timestamp diagnostics read it (`diag.rs:24`), and the redacted project copy is written from it (`redaction.rs:130`). Only the wire question is open, and the [measurement](#raw-text) answers it |
| A blocking `EventSink` extended with credit and cancel | Read `ingest/sink.rs`, family ADR 0001 | **Narrowed.** Credit and cancel need no new method: `event()` blocks and returns an error. They live in `ItemSink` under it |
| `Input` as Sequential, Seekable or Path, declared and satisfied | Read every importer | **Kept, with two refinements.** Text needs two decoded forms, because a line reader skips an over-long line, which would break XML or JSON on one line. And the declaration is a type, so it cannot disagree with the code |
| A record budget is needed | `csv`, `quick-xml` and `serde_json` have no size limit [R; `csv` buffer growth re-read in `delimited.rs`] | Confirmed. It also becomes the runtime guard against whole reads |
| EVTX wraps `from_buffer`, the crate can take a reader | `evtx_import.rs:57`; `evtx-0.15.1/src/evtx_parser.rs:511` [S]; run over a counting reader [M] | Confirmed. Peak 4 MB at 256 MiB and at 1,464 MiB [M], with a cost at the start of each file ([below](#evtx-over-a-lent-file)) |
| The plugin wire is the ceiling for plugin text formats | `remote.rs:178`, `serve.rs:127` [S] | Confirmed, with a second ceiling beside it: the 108 s deadline |

Four things the findings did not name, each changing the design:

- **Streaming over today's protocol 1 would be slow by construction.** `call_lending` answers `ReadFile` from a loop that waits up to `PUMP_POLL`, 5 ms, for items
  (`debrief-plugin-host/src/process.rs:41,684-693`) [S]. A read can wait 5 ms, so the 23,412 reads `oldest_chunk` makes on a 1.43 GiB log [M] could take up to about 117 s before the first event [calc], past the 108 s deadline.
  The shared host answers `ReadFile` on the peer's handler thread with no pump (`paddock-plugin-host/src/services.rs:128`) [S], so this ADR builds on PH2b and never on protocol 1. That also removes the research note's risk of reads waiting on a segment flush.
- **Sniffing becomes the largest per-file cost once plugins are in.** `format_score` asks every importer, for every parser whose rules pass (`parser/set.rs:247-251,308-343`), and each remote sniff is an IPC call: 17 per file measured, 238,000 for the folder [M, calc].
- **Each file zeroes 8 MiB before its first read** (`ReaderChunks::fill`, `chunk.rs:184`) [S]. Growing that read saves about 20% of the per-file time [M].
- **ADR 0031's "the host owes the call" lists callbacks and lent-file reads, not credit** (§2) [S]. With back-pressure reaching the parser, a plugin waiting for credit while the host flushes a segment looks blocked. This ADR adds credit to the list; without it a slow disk fails a healthy import.

## Decision

### 1. The trait and its types

These live in `debrief-plugin-ext` (created by PH2a), re-exported by `debrief-core`, so built-in and plugin importers implement one trait. Until PH2a lands they sit in a new file of `debrief-plugin-api`, moved whole.

```rust
/// Reads one format into events. Holds no whole file and no per-event state (the flat-memory test checks it).
pub trait Importer: Send + Sync + 'static {
    /// What it reads. The registration turns this into the manifest's declaration, so they cannot disagree.
    type Input<'a>: ImportInput;
    fn id(&self) -> &str;
    /// 0..=100 from the file's start. Only called when the declared sniff does not decide.
    fn sniff(&self, ctx: &SniffCtx, head: &Head) -> u8;
    fn declared_sniff(&self) -> DeclaredSniff { DeclaredSniff::Dynamic }
    /// Bytes it may consume between two boundaries; `None` takes the Setting. HAR raises it: bodies it skips are not held.
    fn record_budget(&self) -> Option<RecordBudget> { None }
    /// `Some` only for the closed list of exceptions; goes into the manifest. `read_whole` refuses any other reason.
    fn whole_reason(&self) -> Option<WholeReason> { None }
    fn import(&self, ctx: &ImportCtx, input: Self::Input<'_>, sink: &mut dyn EventSink) -> Result<ImportEnd, ImportError>;
    /// Live tail: the input holds the appended bytes, `ctx.tail` the file's start. The default re-reads the file.
    fn tail(&self) -> TailSupport { TailSupport::Reread }
    fn multiline(&self) -> bool { true }
    fn identity_fields(&self) -> Vec<String> { Vec::new() }
}

/// A format whose result is one document (ADR 0023). Same inputs, bounded output, no event sink.
pub trait DocumentReader: Send + Sync + 'static {
    type Input<'a>: ImportInput;
    fn id(&self) -> &str;
    fn sniff(&self, file_name: &str, head: &[u8]) -> u8;
    fn read(&self, ctx: &ImportCtx, input: Self::Input<'_>, options: &DocumentOptions) -> Vec<u8>; // encoded result, as today
}

/// Sequential: file order only, never spooled.
pub struct Lines<'a>   { /* decoded whole lines with number and byte offset: today's LineReader */ }
pub struct Decoded<'a> { /* impl BufRead: UTF-8, decoded in chunks, line breaks kept, one line may be any length */ }
pub struct Bytes<'a>   { /* impl Read: the file's own bytes */ }
/// Read + Seek on the file's own bytes; a compressed member is spooled to satisfy it.
pub struct Seekable<'a> { /* impl Read + Seek */ }
/// One path, the chosen file's or a private spool's. First-party only, refused at install for anyone else.
pub struct ByPath<'a>  { /* path(), len(), cancel() */ }

impl Seekable<'_> {
    /// The one way to hold a whole input. The reason must be the one the manifest declares. The cap comes from the
    /// limit registry by reason, so the importer cannot choose it.
    pub fn read_whole(&mut self, why: WholeReason) -> Result<Vec<u8>, ImportError>;
    /// The first failure that was not the file's fault (cancel, disk, host). Stays set, see the notes.
    pub fn fatal(&self) -> Option<&ImportError>;
}
// `Decoded`, `Bytes` and `Seekable` also have `boundary()`: a framer says a frame ended, with or without an event.
pub enum WholeReason { HeapSnapshot, Pprof, DumpWithoutMemoryList }

pub trait EventSink {
    /// Blocks while the host holds no credit; fails once the import is cancelled, and the importer returns that error.
    fn event(&mut self, ev: ParsedEvent) -> Result<(), SinkClosed>;
    /// One bad record: counted and sampled (`MAX_SAMPLES`), the import goes on. Ends a frame, as `event` does.
    fn problem(&mut self, kind: ProblemKind, line: u32);
    /// Ends a frame, as `event` does.
    fn skipped(&mut self, n: u64);
}

pub struct ParsedEvent { pub record: Record, pub raw: Raw }
/// An event's original text; `None` for a format with no text of its own (EVTX).
pub enum Raw { None, Text { text: String, byte_offset: u64 } }

pub enum ImportEnd {
    /// Read to the end.
    Complete,
    /// The file is damaged or cut off, or a record went over its budget: what was read stays, with this problem and where.
    Stopped { why: StopReason, at_byte: Option<u64> },
}
```

Notes on the types, all [J] unless marked:

- **Registration erases.** `add_importer<I: Importer>` stores a boxed erased form, and `I::Input::ACCESS` fills the manifest's `ImporterCap { access, content }`, replacing `binary: bool` (`manifest.rs:57`). An importer cannot declare one input and read another. GATs keep the trait out of `dyn`, and the one erasure lives in registration.
- **The record budget is counted at the framer, not at the file read,** so read-ahead and healthy files never trip it. The first version counted file reads since the last event, which would have stopped three kinds of healthy file: `ReaderChunks` reads 8 MiB ahead, exactly the budget; a long stretch of comments or unparsed lines has no event; and XML can hold megabytes outside any row. Over the budget, `read` fails with a typed error, which `csv`, `quick-xml` and `serde_json` all pass up as an ordinary error [R].

  | Input | What counts | What resets it |
  |---|---|---|
  | `Lines` | No byte count. The line limit (4 MiB) and event limit (8 MiB) already bound it, and it has no `Read`, so a whole read cannot be written | n/a |
  | `Decoded` | Bytes the parser consumed (`BufRead::consume`, or bytes `read` returned), not bytes buffered | `event`, `problem`, `skipped`, `boundary()` |
  | `Bytes` | Bytes `read` returned | The same |
  | `Seekable` | Bytes `read` returned | The same, and a seek, because `evtx` reads chunk headers across the file first (`evtx_parser.rs:666-690`) [S] |
  | `ByPath` | Nothing: the OS reads it. A tool's output comes back through `Lines` | n/a |
  | `DocumentReader`, framed (nettrace block, Chrome trace element) | As `Bytes`, per block or element | `boundary()` |
  | `DocumentReader`, one typed document (`from_reader`) | Total bytes, up to `import.document_bytes`. The model's own `Limits` bound the memory; the flat-memory test is what catches a copy of the input | Never |

- **A count on the host too, enforced only from SI-10.** A plugin can call `HostCall::ReadFile` directly and skip the SDK. `HostFiles` counts contiguous bytes served per call since the last `Progress` frame, item or non-contiguous read. Over the budget plus 2 pieces, or over the registry's cap for a declared reason, it refuses further reads with the typed limit. **The SDK sends a `Progress` frame at most once per MiB read, after any event, line, `problem`, `skipped`, `boundary()` or seek,** because items alone would stop healthy files: a sparse EVTX log at 4 records per 64 KiB fills an 8,192-event item only every 128 MiB [calc], 200 empty chunks and 20 MiB of comments send no item at all. Those frames come from 2.6d, so SI-7 only logs the count and SI-10 enforces it. Hopping between offsets avoids this count; the plugin's memory cap still applies, so this catches mistakes and is not a security boundary [J].
- **A fatal read error stays fatal.** `evtx` turns any read error into `IncompleteChunk`, a bad chunk it may step past, and `oldest_chunk` stops on a read error without reporting it (`evtx_parser.rs:582-584,672-676`) [S]. So `Seekable` keeps the first error that was not the file's fault (cancel, disk, host) and fails every later read. The EVTX loop checks `input.fatal()` per record and after the loop, so a cancel or a dead disk is never reported as damaged chunks.
- **Errors from `read` are never `ErrorKind::Interrupted`.** `read_exact`, `read_to_end` and our own `ReaderChunks::fill` retry on it (`chunk.rs:193`) [S], so a cancel carried that way would loop forever. Cancel and budget errors use `ErrorKind::Other` with a typed payload.
- **The head.** `ctx.head` is the plan's decoded head, at most `HEAD_BYTES` 1 MiB (`plan.rs:13`). It replaces the importers that scan the whole `content` for a header or sample, such as Procmon's day order (`procmon.rs:31`), which then decides from what the head holds.
- **Core importers keep `read_split`.** An importer may answer `splittable()`. The pool then calls `import` once per chunk, each with `Lines` over that byte range and fresh state, and merges `BatchedSource` as today (`parallel.rs`). Only core text and JSON lines do this in 1.2; plugin importers read one stream per file.
- **Importers no longer report `Vec`s.** `Imported` goes. `skipped` and problems reach the sink. `Record.id` is dropped from the wire: the store assigns ids (`writer.rs:570`).

### Every implementer

"Today" is what it does at `3168b6145` [S]. "Size" is the conversion, S under half a day, M up to two.

| Implementer | Crate | Today | Input | Exception | How it converts | Size |
|---|---|---|---|---|---|---|
| `TextImporter` (generic) | core | streams (`ingest/text.rs`), plus an in-memory copy | `Lines` | | Moves onto the trait; the in-memory form is deleted, `/samples` goldens replace the differential test | S |
| `JsonImporter` becomes `JsonLinesImporter` | core | lines stream (`ingest/json.rs`) | `Lines` | | Split, because one importer has one input type. The file plan's `JsonShape` picks between the two, and `[format] kind = "json"` still names both | S |
| `JsonDocImporter` (new, from the same) | core | arrays and single objects whole (`ingest/json.rs:107`) | `Decoded` | | Framer finds element bounds, `from_slice` each under the budget, `boundary()` per element. A single object is whole only under 1 MiB already (`plan.rs:216`), so it fits the budget | M |
| `CsvImporter` | core | streams on the branch; in-memory form kept for the test | `Lines` | | Delete the in-memory form; same goldens | S |
| `VideoFramesImporter` | core | whole | `Lines` | | Line loop, keeps `MAX_FRAME_LINES` | S |
| `SyslogImporter` | plugin-syslog | whole | `Lines` | | Line loop; continuation lines append to a held event, sent when the next begins | S |
| `JournaldImporter` | plugin-syslog | whole | `Lines` | | Line loop | S |
| `AccessLogImporter`, `W3cImporter` | plugin-web | whole | `Lines` | | Line loop; W3C's `#Fields` stays loop state | S each |
| `HarImporter` | plugin-web | `from_str::<Value>` | `Decoded`, budget raised | | Visitor over a typed `Entry`, bodies through `IgnoredAny`, test with a large body and a `data:` URL [R]. `Raw::Text` is the event's message, because the redacted copy and context view read `raw` | M |
| `DockerJsonImporter`, `CriImporter` | plugin-formats | whole, has tail | `Lines` | | Line loop; tail through `ctx.tail` | S each |
| `SqlErrorLogImporter` | plugin-formats | whole, has tail | `Lines` | | Line loop with its head; held event for continuations | S |
| `Log4jXmlImporter` | plugin-formats | regex frames over `content`, has tail | `Decoded` | | Framer over `BufRead`: buffer to the next start tag under the budget, then the existing `parse_event`. Cut-off handling stays | M |
| `RegistryImporter` | plugin-registry | `parse_reg` over `content` | `Lines` | | Line loop holding the current key and one pending continuation (`logical_lines`), UTF-16 from the file plan. The 440 MiB case | M |
| `OtlpImporter` | plugin-otel | lines, or one document whole | `Decoded` | | Lines as they are. A document: framer over `resourceLogs`/`resourceSpans` elements, each under a raised budget, so a resource may follow its records inside an element. One element over it stops with "export as JSON lines" (D3) | M |
| `PerfmonCsvImporter` | plugin-windows | `csv` over `content` | `Decoded` | | `csv::Reader::from_reader`, reusable `ByteRecord`, budget | S |
| `ProcmonCsvImporter` | plugin-windows | all rows, then records | `Decoded` | | One row at a time; day order from `ctx.head` | M |
| `NfoImporter` | plugin-windows | `Reader::from_str`, usually UTF-16 [U] | `Decoded` | | `read_event_into` with a reused buffer; depth cap stays; line from counted newlines | M |
| `EvtxImporter` | plugin-windows | `from_buffer(to_vec)` | `Seekable` | Spool in an archive | `from_read_seek`, one event per record. A bad record is a problem; damage after events is `Stopped` | S; PH4 |
| `BlgImporter` | plugin-windows | `relog`, then `fs::read` of the CSV | `ByPath` | `.blg` by path | `relog` on the path; the temp CSV goes through the Perfmon reader, streamed. Cancel ends the child | S |
| `MinidumpImporter` (summary) | plugin-windows | whole | `Seekable` | Dumps without a full list, to 1 GiB | Through `debrief-plugin-dumps::source::load`, moved to a library both link [R] | S |
| `EtlImporter` | plugin-native | writes lent bytes to temp, reads converted NetTrace whole | `ByPath` | `.etl` by path | `scan` and `convert` on the path, no temp copy of the input; the converted NetTrace goes through the block reader | S |
| `MinidumpImporter` (document) | plugin-dumps | ranges, prefix plus stacks, whole only without a full list (`source.rs:48,52`) | `Seekable` | As above | `Ranges` becomes `Seekable` | S |
| `UmdhImporter` | plugin-dumps | `read_exact(0, len)` | `Lines` | | Line loop; output bounded by `max_stacks` | S |
| `NettraceImporter` | plugin-profiling | `Cursor` over a slice | `Bytes` | | One length-prefixed block at a time, block size capped. Sequential walk not confirmed for V6 [R, U]; if it needs a seek, `Seekable` | M |
| `HeapSnapshotImporter` | plugin-profiling | `from_slice` | `Seekable` | Heap snapshots | `read_whole(HeapSnapshot)`, existing message | S |
| `PprofImporter` | plugin-profiling | `decode` over bytes, gunzip | `Seekable` | pprof | `read_whole(Pprof)`, then as today | S |
| `TraceEventImporter` | plugin-profiling | whole | `Decoded` | | Framer over the array or `traceEvents`; model bounded by `Limits` | S |
| `CpuProfileImporter`, `SpeedscopeImporter` | plugin-profiling | whole | `Decoded`, budget off, model capped | | `serde_json::from_reader` into typed structs: no copy of the input, model bounded by `Limits`. The research note kept these whole; the user's list does not, so this is the conversion, measured against D3 | S each |
| `PerfScriptImporter`, `FoldedImporter` | plugin-profiling | whole | `Lines` | | Line loop; input decoded from `Bytes` as today | S each |
| `KvLines` (example), `Fake`, `Fixture` | examples, tests | whole | `Lines` | | Rewritten with the SDK docs; one streaming example is what plugin authors copy | S |
| `RemoteImporter`, `RemoteBinaryImporter`, `RemoteCaptureImporter`, `RemoteDocumentImporter` | plugin-host | proxies | follow the manifest | | One remote form replaces them, erased by the manifest's `access` | PH2b, 3.4 |
| `GuardedImporter`, `GuardedBinaryImporter`, profiling `Adapter`, `GuardedImporter` in `app/src-tauri/src/{dumps,profiling}.rs` | core, plugin-profiling, app | wrappers | same | | One guarded wrapper; the adapter goes with `CaptureImporter` | S |
| `Json`, `Xml`, `Yaml`, `Ini`, `Toml`, `RegConfig`, `NfoConfig` parsers | core, plugins | `parse(&str)` up to `Limits::max_bytes` 64 MiB | not importers | Not covered (D1) | Stay as they are if D1 says so: they read one configuration file for a diff, not an import | none |

**Planned formats** use the same trait: Process Monitor PML is `Seekable` (tables at the end [R, U]), `.tar` is a container (below).

### 2. The plugin ABI

**Where it lives.** `debrief.import.v1`, `debrief.profile.v1` and `debrief.document.v1` (family ADR 0003) are defined this way from their first version. None has shipped, so nothing changes version twice: the break is the one PH2 already makes, Debrief protocol 1.1 to platform protocol 2.0. Old plugins are refused with `Refusal::Legacy` (PH3) and there is no shim. `PLATFORM_PROTOCOL` stays 2.0, because `ReadFile` keeps its shape [S: `crates/paddock-plugin-api/src/protocol.rs:20`]. After release, an added request is a minor and a changed struct is a new `.v2`, by family ADR 0001's rule. Family ADR 0003's table is amended in the same change.

```rust
// debrief.import.v1. Body types of the contract, postcard.
pub enum ImportRequest {
    /// → Vec<u8>: one score per importer, in order. One call per plugin per file. `head` is raw bytes; the SDK decodes
    /// it once with `encoding` for text importers, because they sniff decoded text today (`importer.rs:89`).
    Sniff { importers: Vec<Arc<str>>, file: FileInfo, head: Vec<u8> /* ≤ 64 KiB */, encoding: Option<Arc<str>> },
    /// Items: Vec<WireEvent>, at most 8,192 events or about 4 MiB. → ImportDone. Bulk.
    Import { importer: Arc<str>, file: FileInfo, input: WireInput, plan: TextPlan },
    /// Items as above. → TailDone. Live. `head` is at most HEAD_BYTES, sent inline.
    Tail { importer: Arc<str>, file: FileInfo, head: Vec<u8>, input: WireInput, plan: TextPlan },
}
pub enum WireInput {
    /// Read with HostCall::ReadFile. `file.len` is LEN_UNKNOWN for a decompressing stream, read only in order.
    Lent { file: HostFileRef, seekable: bool },
    /// The one file the user chose, or a private spool. Only for an importer whose manifest declares ByPath.
    ChosenPath { path: String, len: u64 },
}
pub struct TextPlan { encoding: Arc<str>, data_start: u64 }
pub struct ImportDone { skipped: u64, problems: Vec<Problem>, end: ImportEnd, bytes_read: u64 }
```

`WireEvent` loses `id` and gains `raw` and `byte_offset` (ADR 0017 §3 lists them; the code never had them, `protocol.rs:501`) [S]. See [Raw text](#raw-text).

**An event never makes an oversized frame.** Frames are 16 MiB by default and 64 MiB at most (`paddock-ipc/src/frame.rs:18,20`) [S], and an oversized one counts against the plugin. With an 8 MiB `raw` plus message, exception and fields, one event can pass that. So the SDK's sink measures each encoded event before it joins an item. Over 12 MiB [J], it drops `raw` and adds `EventTooLarge`. If still over, it skips the event with the same problem.

**`HostFile: Read + Seek`** (package 2.6b, `crates/paddock-plugin-api/src/host_file.rs`). `read_all` and `read_text` are removed; `read_at` stays for `RangeRead`, which `Seekable` wraps. A plugin reads in pieces of at most `READ_CHUNK`, 1 MiB. **Read-ahead only on sequential reads**, one 1 MiB buffer per input: a read that continues the last one asks for a whole piece, and a read after a seek asks only for what it needs. [Measured](#evtx-over-a-lent-file) over `evtx`'s real access pattern. The cancel check runs before each round trip.

**Host side** (package 3.4). `HostFiles` gains `register_open(File)`, so the handle the engine already holds is lent with no second open, and `open_stream(Box<dyn Read + Send>)`, which answers in-order reads and refuses others, for a decompressing member. On `RemoteStream::next`, each item goes straight to the batcher: nothing is collected.

**Decoding.** The plugin decodes, on its side of the wire, with the file plan's encoding. `chunk.rs` and the decode half of `encoding.rs` move to `debrief-plugin-ext`, which adds `encoding_rs` to the plugin bundle; `chardetng` stays in the host, because the plan is made there. The host never lends decoded text, because decoded offsets cannot be mapped back to file offsets for UTF-16 [J]. Today's `content` "decoded text (UTF-8)" (`protocol.rs:109`) goes away.

**What a crash or hang does:**

| What happens | How it is seen | This file | Other files on that process | The plugin |
|---|---|---|---|---|
| A panic in the importer | `Fail { PANIC }` | Fails with the cause, its events removed (`abort_source`), listed with Retry | Retried once each, one at a time, on the fresh process, with a fresh source id | Restarted; counts towards quarantine, simultaneous failures once (2.6a) |
| Abort, stack overflow | `IpcError::Died` | Same | Same | Same |
| Over its memory cap | `LimitExceeded { plugin.memory }` | Same, with Raise it and Retry | Same | Restarted; not counted (2.6a) |
| Stall (no progress, not owed anything, per ADR 0031) | Stall judgement (2.6d) | Same, "stopped responding" | Same | Stopped and restarted; counted |
| Garbled item, more items than credit | Fault in `paddock-plugin-host` | Same | Same | Stopped; counted |
| A host read fails (file gone, disk) | `ReadFile` error → `io::Error` | `Err` after no events; `Stopped` after some | Not affected | Fine |
| One bad record | `sink.problem` | Goes on | Not affected | Fine |
| Damaged or cut-off file | `ImportEnd::Stopped` | Keeps what was read, with a problem | Not affected | Fine |
| Cancel | `Cancel` frame; `event()` and `read` fail | "not imported", events removed | Same | Fine, or stopped after `stop_grace` |
| Quarantine reached | `Health` refuses | "plugin is quarantined", Retry after re-enabling | Remaining files of its importers fail at once, not one by one | Stays quarantined until the user acts |

**Bystanders are retried [J].** With 8 files in flight in one process, one poison record kills 8 calls. Retrying each once, serially, costs at most one more read of those files, and leaves the failure on the file that caused it. Without it, a folder of 14,000 files can lose unrelated files to one bad one.

**A retry takes a fresh source id.** `begin_source` with the same reserved id clears the aborted flag (`writer.rs:541-552`) [S], so rows of the failed attempt, already in segments, would become visible beside the new ones. A retried file therefore calls `begin_source` with no reservation. Its id falls outside sorted order, a cost only in a run that had a crash; a later reload keeps it by path (ADR 0051). SI-0 has the failing test.

**Limits of the wire**, all existing: items at most 8,192 events or about 4 MiB, credit 4 and "Auto", frame 16 MiB, `READ_CHUNK` 1 MiB, 64 concurrent calls (family ADR 0001). Plugin process cap 1 GiB by default (`sandbox.rs:14`). In flight per file, on the wire: at most about 16 MiB of events [family ADR 0001]. What leaves: `MAX_IMPORT_BYTES`, `READ_TIMEOUT` 60 s (`host_file.rs:10`), the import budget and deadline. A plugin's total is bounded by concurrent bulk calls, which the host limits to the import's files in flight.

**The chosen path.** `ByPath` is declared per importer. The host sends `ChosenPath` for that importer only, and only when the manifest is first-party (`spec.home.is_none()`); a third-party manifest declaring it is refused at install, matching `sandbox_for_third_party`, which never grants paths (`sandbox.rs:91`). Consequently:

- **The native plugin gets only the chosen `.etl` path**, and asks for no `OpenedFilePaths`. It learns no other imported file's path.
- **The Windows plugin drops `OpenedFilePaths`**, so EVTX and CSV calls see only the path's tail. `.blg` still gets its path per call.
- **"Only the chosen path" holds in the contract and not in the OS, for both plugins.** The native plugin's sandbox is `FsAccess::Unrestricted` today, because it holds `WriteFolders` for captures (`sandbox.rs:42-45`, `debrief-plugin-native/src/plugin.rs:60`) [S]. The Windows plugin's is too, because it holds `RunPrograms` for `relog` (`sandbox.rs:37-41`) [S]. Either plugin could read other files. Narrowing that is a change of their rights, not part of this ADR, and it must not be claimed as a sandbox property.

### 3. Containers and archives

**`paddock-archive` gets a member reader** (rule 42; shared, so it lands after the Dyno tag). The crate today only extracts to a folder (`extract`, `readers.rs`); its zip and flate2 versions match Debrief's (`crates/paddock-archive/Cargo.toml:12-15`) [S].

```rust
pub struct Members { /* one File, read by position; central directory by ranges, count refused from the EOCD first */ }
impl Members {
    pub fn open(file: File, limits: &Limits) -> Result<Members>;
    pub fn entries(&self) -> &[MemberInfo];              // name, declared sizes, renamed_from
    pub fn open_member(&self, i: usize, ctl: &dyn Control) -> Result<Member>;   // Read, through the guard
}
pub fn gzip_member<R: Read>(reader: R, archive_len: u64, limits: &Limits, ctl: &dyn Control) -> GzMember<R>;
```

- **Moved from `debrief-core/src/archive.rs`:** `central_records` and `check_ranges`, rewritten to read the central directory and the 30-byte local headers by range instead of over a slice, with the overlap check kept; and the repeated-name rule (`Namer`, user decision 2026-10-29). Debrief's copy is deleted in the same change (rule 42), not registered as a copy.
- **Refuse by the EOCD count before `ZipArchive::new`**, which builds its map first [R]. The same check replaces `source_slots`' full parse (`parallel.rs:123`).
- **`read_entry` and the in-memory context view stop expanding the archive** (`archive.rs:402`, `workspace/context.rs:27-33`): one member, read in order. The store's context view needs none of this; it falls back on `raw`.
- **Formats.** `.gz` through `MultiGzDecoder` (sequential). `ZipArchive` is `Clone` with shared metadata when its reader is `Clone` [S: `zip-9.0.5/src/read/zip_archive.rs:120`]. `File` is not, and `try_clone` would share one cursor, so `Members` wraps an `Arc<File>` in a small reader that keeps its own position and reads with `read_at` or `seek_read`. Members of one zip then import in parallel on one handle, with no `unsafe` (`unsafe_new_with_metadata` not used), and source ids stay in order. **`MAX_RESERVED` rises from 10,000 to `MAX_ENTRIES`** (`parallel.rs:120`), because the bundle as a zip has over 14,000 members and those past the reservation would get ids out of order [S]. `.tar` when the roadmap adds it. No nesting, as today: a zip inside a zip is reported as binary [S].

**The ratio guard replaces the fixed caps.** Today: entry 1 GiB, archive 4 GiB (`archive.rs:12,14`), so a zip of the 7.6 GiB bundle is refused [S, calc]. New: bytes out at most `max(compressed bytes × ratio, floor)`, the rule `paddock-archive` already enforces while writing (`writer.rs:64-71`) [S]. Default ratio 200, floor 64 MiB, a Setting with a hard maximum of 2,000 (deflate cannot pass about 1,032:1 [D: zlib technical details, not re-read]). For a whole-archive total, the guard counts across members. Hitting it stops that source with `archive.ratio` and Raise it. Entries stay at `MAX_ENTRIES` 65,536.

**Free space is checked before spooling.** The central directory declares each member's size, untrusted but useful. A member that must seek and declares more than free disk minus 2 GiB fails at once with its numbers, rather than after writing most of it. The guard still counts real bytes.

**How the host satisfies the declared input:**

| Importer declares | Plain file | `.gz` or a zip member |
|---|---|---|
| `Lines`, `Decoded`, `Bytes` | Reads the file (in process) or lends it | Decompresses in the host: on the parse thread in process, or on the peer's handler thread that answers `ReadFile` for a plugin. Never spools |
| `Seekable` | Reads or lends the file | Spools the member, then as a file |
| `ByPath` | The path | Spools the member, then the spool's path |

`head` is read once from a sequential member, then chained in front of the rest, so nothing is decompressed twice.

**The spool** (exception 4). A private folder under the store worker's temp folder, each member created with `create_new`, owner-only. A cap is checked while writing (`import.spool_bytes`, below); so is free disk, which fails typed. If the import-wide cap is reached while other members are spooled or being read, this one waits for them; it fails only when it is alone and still does not fit. Spooling is a phase with progress and cancel (`paddock_archive::Control`). It is removed when the source ends on every path. **A later start sweeps what a crash left, by held identity (rule 53):** the folder holds an owner record, a `paddock_proc::ProcessId` with pid and start time, and is removed only when that process is gone. `sweep_stale` cannot do this as it stands: it matches `.<dest>.paddock-staging-` style names beside a destination and goes by age alone (`stage.rs:106-127`) [S], and another running Debrief may own a spool. SI-4 adds `sweep_spools`. While the source is read the engine holds the handle, and a plugin gets a path only under `ByPath` [J]. It holds user content on disk for the length of the read; that is the cost the user accepted.

### 4. Back-pressure, Cancel, progress, errors, encoding

**Back-pressure is one blocking chain, with no queue added.** The store's slot lock blocks the parse thread in `BatchSink::write`; for a plugin importer that thread is the one taking items, so no `Credit` frame goes back; `ItemSink::send` blocks in the plugin; `sink.event` blocks; the parser stops reading; `ReadFile` stops. In process, the chain is the same without the wire. Bounded per file: one read buffer, one partial item, credit × item, one batch of 8,192 rows. IP1's bounded writer queue is required.

**Cancel.** `ImportProgress`'s flag reaches the `RemoteStream` (`Cancel` frame) and the in-process sink. `sink.event` and every input `read` fail; importers need no polling, so `ctx.cancelled()` goes. `.etl` returns false from the callback (`etl.rs:41`) [S]; `relog` is ended through its held child (rule 53); the spool phase asks `Control`. State stays consistent because a source is listed only when ready (ADR 0062, `writer.rs:589`): `abort_source` leaves garbage the view hides. A cancel after the plugin's grace stops the process.

**Progress (ER-18).** Today `file_done` moves it at the end of a file, so a 1.43 GiB log shows nothing for its whole read [S: `parallel.rs:217`, `pipeline.rs:145,179`]. Each input now adds to `ImportProgress` as it reads:

- Bytes delivered after the first event, cumulative, capped at the file size. Not the file position: `evtx` walks every chunk header first and a wrapped log starts in the middle, and progress never goes backwards.
- Before the first event, phase "Preparing \<file\>", with a growing count of reads.
- A zip reports compressed position, which is monotonic (`Counting`, `readers.rs:67`) [S].
- `ByPath`: `.etl` and `.blg` report phase and a growing event count, no percentage.
- Spooling is its own phase on its own scale.

**Errors, one rule for all importers.**

| Scope | What the importer does | What the user sees |
|---|---|---|
| One bad record, line, chunk | `sink.problem(kind, line)`, goes on | Counted in the source's problems, first 20 sampled |
| An over-long line or event | Existing: `LineTooLong`, `EventTooLarge`, skipped | Same |
| A record over its budget | `read` fails; importer returns `Stopped` | Events kept, "read up to byte N: one record is larger than the limit", Raise it and Retry |
| Damaged or cut-off file after events | `Stopped { at_byte }` | Events kept, one problem |
| Nothing readable; not this format | `Err` | Source aborted, "Needs attention", Retry |
| I/O failure, crash, stall, quarantine | `Err` or fault | Source aborted, with the cause. D2 asks whether a crash keeps what was read |

A failure stays in its file. Every file already runs inside `guard::catch` (`source.rs:246`); plugin faults arrive as `Crashed`. **OOM in the engine is the uncaught case** [R], so `csv`, `flate2` and `zip` in `debrief-core` read under the budget and the guard before they read from disk.

**Encoding in chunks.** `Lines` is `ReaderChunks`: it splits on the encoding's own newline, keeps exact byte offsets, and decodes per line, so a surrogate pair or a multi-byte character is never cut. A test crosses every read boundary, on UTF-16 with no BOM, mixed code pages and stray bytes (`tests/stream_bundle.rs`) [S; passes on the branch]. `Decoded` uses `encoding_rs::Decoder::decode_to_utf8`, which keeps state across calls [U: API read in docs, not run here], so no line limit applies to it. Stateful encodings stay excluded (ADR 0040). A 440 MiB UTF-16 log with a BOM reads at 636,347 to 636,787 events/s on one thread, against 751,145 to 756,655 for the same lines in UTF-8, at 134-141 MB peak RSS [M]; the `.reg` importer itself cannot be measured until it is converted. Per-line decoding allocates (`utf16` in `decode_into`, `chunk.rs:96`), so a cheaper form is in [Parallelism and cost](#parallelism-and-cost-er-20).

### Per-file costs at 14,000 files

Measured on invented files, reading side only (a counting sink, no store writer) [M]:

| 14,000 invented files of 2 KiB, reading side | 1 thread | 4 | 8 |
|---|---|---|---|
| Import, seconds (today) | 2.37-2.45 | 0.87-0.89 | 0.60-0.62 |
| Import, seconds (first read grows from 64 KiB) | 1.91-1.96 | 0.70-0.73 | 0.45 |
| Thread time per file, µs (today) | 169-175 | 247-253 | 343-356 |
| Most file handles seen at a write | 5 | 11 | 18-19 |
| Walk of the tree | 0.03 s | | |

Peak RSS across the run: 174-178 MB today, 112-116 MB when the first read grows. A sniff call counted per file: **14 text plus 3 binary, 17 in all, once each**, with fake importers standing in for today's 14 plugin text and 3 plugin binary importers; these `.log` files matched one parser. A file that several parsers' rules select repeats them (`parser/set.rs:247-251`) [S].

So on the reading side per-file cost is small: about 170 µs, or 2.4 s of thread time for the folder. Plugins change that. 238,000 sniff calls [calc] with up to 64 KiB each, at the [measured](../../../../docs/benchmarks.md#2026-11-05-paddock-ipc-round-trip-debrief-adr-0064) 153 µs per 64 KiB call, is about 36 s of thread time, 15 times the rest [calc].

Per file, in order [S]: walker stat; `source_slots` opens the file and reads 4 bytes; `fingerprint` opens it and reads up to 128 KiB; `open_with_retry`; head up to 1 MiB; encoding detection; `score_text`; `parsers.choose` over 16 built-in parsers; `plan_with`; batcher allocations; reads. The store lists sources when segments register, so no catalogue commit is made per source (`writer.rs:539`, comment) [S]. So three opens per file become one with this ADR, sniff calls fall from 17 to at most 6 per file, and the first read stops zeroing 8 MiB.

**The fixes, in the order they save:**

1. **Sniff at most once per file per importer, in one call per plugin.** Memoise `format_score` by importer id for the file's duration, and send `Sniff { importers, head }`. With 2.6a's declared sniff (extensions, magic), EVTX, BLG and dumps need no call, and at most the 6 plugins with content-sniffing importers are asked [S]: about 13 s of thread time instead of 36 [calc]. Whether importers need the full 64 KiB is checked in SI-6: at 4 KiB a call costs 39 µs [M].
2. **The first read grows.** `ReaderChunks` starts at 64 KiB and doubles to `CHUNK_BYTES`: about 20% less time per file, about 35% less peak RSS, interleaved A/B [M].
3. **One open per file.** `source_slots` looks at magic from the walker's stat pass, and `fingerprint` shares the file handle and head.
4. **Lazy plugin starts.** Plugin processes start on the first dynamic sniff their importer needs (2.6a).

**Open handles** [S unless marked]. Each file in flight holds one source handle, plus, with `register_open`, none for lending. Today's shared `HostFiles::register_file` would open a second (`files.rs:65`). A split text file holds one handle per chunk, at most `threads`; a spool one more; a walker one while listing, at most 16; the writer a Parquet file per slot. In flight is `2 × threads`, at most 16 by default, so the import holds about 16 to 50. Measured peak 18-19 at 8 threads [M, counted at each write, so a brief peak could be missed]. `stream_bundle.rs` asserts `threads × 8 + 16` on 3,000 files. No code raises `RLIMIT_NOFILE`, and a macOS GUI process has a soft limit of 256 by default [U]. So: the store worker raises its soft limit at start to the lesser of its hard limit and the system's per-process maximum, because macOS refuses `RLIM_INFINITY` [U], logs the value, and `EMFILE` becomes a typed limit (`import.open_files`) that lowers the files in flight for that import instead of failing files [J].

### EVTX over a lent file

| Over a simulated lent file, 256 MiB | Round trips to first event | Bytes to first event | Round trips, total | Bytes, total | Wire time [calc] |
|---|---|---|---|---|---|
| No read-ahead | 4,101 | 2 MiB | 20,481 | 258 MiB | over 1 s |
| Read-ahead on every read | 257 | 257 MiB | 512 | 512 MiB | 1.0 s |
| **Read-ahead on sequential reads only** | 4,084 | 4 MiB | 4,339 | 258 MiB | 0.6 s |

The chosen policy on a 1,464 MiB log (the bundle's 1.43 GiB): 23,412 round trips and 13 MiB to the first event, 24,875 round trips and 1,476 MiB in all, about 3.6 s of wire time [calc]. Peak RSS of the parser process is 4 MB at both 256 and 1,464 MiB, so it is flat [M]. The parser ran at 1,949 MiB/s and 124,731 records/s on one thread [M], but the invented log repeats the MIT fixture's chunk, which holds only 4 records, so records per second on a dense log, the cost of `to_record` and the worst case per chunk were not measured. **SI-0 remeasures with a dense chunk and `to_record`, in footprint;** the review's `evtx_probe.rs` has a `dense_chunk` generator to start from. Until then, read these as the reader's access pattern, which the density does not change, and not as a throughput.

`evtx` always walks every chunk's header before its first event (`chunks()` → `oldest_chunk()`, `evtx_parser.rs:666-700`) [S]. With sequential-only read-ahead that is one round trip per chunk, then one per 1 MiB. At the [measured round trip](../../../../docs/benchmarks.md#2026-11-05-paddock-ipc-round-trip-debrief-adr-0064) of 33 µs, about 0.8 s before the first event on a 1.43 GiB log [calc]. That is not worth an upstream change or a passed handle. Read-ahead on every read would transfer the whole file once just to find the start, which is why the policy is what it is. This answers the research note's open question. The round trip is in one process over the private endpoint, on macOS; 2.6b's benchmark gives the figure against a real child on each OS.

### Raw text

**`raw` stays a store column,** for the three readers above. Two things ADR 0014 says about it are no longer what the code does: the index reads message, exception and template, not `raw` (`fts.rs:436-453`), and a profile change re-imports from the files (`profiles.rs:62`) [S]. So fewer features depend on `raw` than ADR 0014 suggests. Its disk cost is real: 190 of 215 MB of text in ADR 0014's table, and at a billion events it matters. Changing it is a separate decision (D5).

**On the plugin wire, two ways were measured**, the same events through `paddock-ipc` items at credit 4 [M]:

| Mode, 4,000,000 events, 3 rounds | Events per second | Wire bytes |
|---|---|---|
| Raw text in `WireEvent` | 1,936,823 to 2,019,105 | 815 MiB |
| Offset and length; the host reads the item's range and cuts | 2,356,065 to 2,395,779 | 456 MiB |

The span is about 20% faster on the wire alone. Against a plugin parser's rate it matters much less: at 800,000 events per second of parsing, about 5% end to end [calc]. **Decision: raw text crosses the wire, one path for every input.** A span needs a host re-read, which can differ from what the plugin read if the file is being written; a window or a fallback for a decompressing member, whose bytes the host no longer has; and a second decode for UTF-16 and legacy encodings. That is three new failure paths for 5%, on plugin text formats only, because built-in text, JSON and CSV are in process and EVTX has no raw text. This corrects the finding that `raw` becomes an offset and length, and the research note's "text must not cross the wire twice": today it crosses as `content` and then is cut by the host, and after this it crosses once, beside the message. If a later profile shows the wire dominating, send `message` as a range of `raw` first; that saves the same bytes with no re-read [J].

In process nothing changes: the importer already has the text.

### 5. Limit registry (package 3.6)

Rows are in family ADR 0019's form, implemented by 3.6, with one `limit_case!` each. A new or changed row carries a test that triggers it.

| `LimitId` | Unit | Default | Bounds | Setting or hard | When it fires, and the remedy |
|---|---|---|---|---|---|
| `import.record_bytes` (new) | bytes consumed between two frames ([table](#every-implementer)) | 8 MiB, as `MAX_EVENT_BYTES` | 1 MiB to 256 MiB | Performance ▸ Import | "One record of \<file\> is larger than N; read up to byte M." Raise it, Retry |
| `import.line_bytes` (row 29) | bytes | 4 MiB | | hard | A line skipped, counted |
| `import.event_bytes` (row 30) | bytes | 8 MiB | | hard | Continuation lines dropped, counted |
| `import.document_bytes` | bytes | 1 GiB (`max_input_bytes`) | to the plugin's cap | hard | "Open a smaller capture" (existing) |
| `import.dump_whole_bytes` | bytes | 1 GiB (`max_read_bytes`) | | Setting exists | Existing message (`source.rs:144-149`) |
| `import.spool_bytes` (new) | bytes | 16 GiB per import, and always 2 GiB of disk left | 1 GiB to the disk | Performance ▸ Import | "Needs N of temporary space for a member of \<archive\>." Raise it, Retry |
| `archive.ratio` (replaces rows 34-35) | ratio | 200, floor 64 MiB | 10 to 2,000 | Performance ▸ Import | "\<archive\> expands more than 200 times." Raise it, Retry |
| `archive.entries` | count | 65,536 | | hard | Refused from the EOCD, before the directory is read |
| `import.open_files` (new) | handles | soft limit minus a reserve | | hard | Files in flight lowered, one notice |

**Retired:** `MAX_IMPORT_BYTES` 2 GiB (row 23), `MAX_ENTRY_BYTES`, `MAX_ARCHIVE_BYTES`, `plugin.call_deadline.import`, `READ_TIMEOUT` 60 s. Removing the deadline and the read timeout is 2.6d's; ER-19 is met only when those land, so 3.4 must not ship ahead of 2.6d (rule 12).

### Rules met

- **ER-17, rule 10.** Importers run on the parse pool in the store worker or in a plugin process. The UI thread sees job events only. MEM-1 takes captures off the app process's own read path.
- **ER-18, rule 11.** Progress as in §4. Cancel in every phase. Progress never goes backwards.
- **ER-19, rule 12.** No wall-clock limit on a running import after 2.6d. The stall judgement treats items, answered reads and waiting for credit as not stuck.
- **ER-20, rule 51.** [Parallelism and cost](#parallelism-and-cost-er-20).
- **ER-27, Paddock's logging rule.** An import logs, with `import_id` and source id: phases with durations (`finding`, `plan`, `spool`, `read`); per source at debug, per import at info, per failure always: importer, input kind, encoding, bytes read, events, skipped, end, duration. The plugin call logs start, end and outcome. A failure logs the cause chain and the next step. A stopped or crashed plugin logs its exit and which sources were retried. No file content, and paths go through `paddock-diag`'s redactor (X1). The legacy phase logs on `debrief/stream-text` (`legacy.rs:24-53`) are removed with it. A bug this log could not explain adds the line in the fix.
- **Rule 52.** Bulk calls run at `QOS_CLASS_UTILITY`, nice 10 or below normal; `Tail` is Live.
- **Rule 53.** Spool files are held by handle, a spool is swept by owner identity, and `relog` is ended through the held child.
- **Rule 71, ER-33.** The flat-memory test has a positive control: a collecting fake must fail it.
- **Rules 48 and 26, not yet met in full.** The harnesses are committed as text under `docs/measurements/adr-0064/`, with exact commands in the results blocks. Memory in those blocks is RSS, labelled, not footprint; SI-0 re-runs them in footprint.

## Parallelism and cost (ER-20)

**More threads.** Files parse on the existing pool (Setting, Auto: cores minus one, at most 8). Measured scaling from `debrief/stream-text` [M, same block as the table above]: CSV, 8 files of 37.5 MiB, 948,841 events/s on 1 thread to 4,522,732 on 8; text, 1 file of 300 MiB split in chunks, 844,321 to 2,050,586. One CSV file does not scale, because rows cannot be cut without the quote state.

| Work | Bound | Note |
|---|---|---|
| Parsing, in process | Pool threads | Existing |
| Plugin parsing | One thread per concurrent call, calls limited to files in flight; no pool of its own (family ADR 0001) | `evtx` with `multithreading`, keeping `2 × threads` chunks in flight [R], only if the single-thread rate proves the bottleneck on a dense log. Not planned for 1.2: 1,949 MiB/s on the sparse sample, and many files already parallelise [M, U] |
| Files in flight | `2 × threads`, lowered by handles | Existing gate (`pipeline.rs`), new lowering |
| Reads from a slow disk | The pool is sized for CPU, not the device. Rule 13 asks 1-2 for an HDD, an open gap for a bundle on a USB disk [U] | SI-18 |
| Zip members | Pool threads, one cloned reader each | New |
| Spooling | One per file in flight, total capped by `import.spool_bytes` | New |

**Cheaper, each measured or counted where marked:**

- The whole-read copies go: file bytes, decoded string, `to_vec`, the host's `Vec`, the `.etl` temp copy [M for CSV; [calc] elsewhere].
- `Vec<Record>` goes: 27% faster on CSV even before anything else [M].
- Sniff memo and batch: 17 calls per file to at most 6, about 23 s of thread time on the folder [calc].
- Small first read: about 20% of per-file time, about 35% of peak RSS on small files [M].
- Two extra file opens per file go.
- A typed HAR `Entry` skips bodies; the JSON framer avoids `from_reader`'s byte-at-a-time reads [R].
- UTF-16: about 16% slower per event than UTF-8 [M]. Decoding a line into the chunk's `String` with `encoding_rs` avoids the per-line allocation; SI-2 measures it and keeps it if it pays.
- `WireEvent` drops `id`.

**Skipped work:** a file the window rules out is not opened further (ADR 0047). Sequential members never spool. Stored zip members could use `by_index_seek` instead of a spool [R]; not needed yet.

**Memory.** Measured on the text-only bundle shape: 192 MB peak RSS at 8 threads, reading side [M]. The store worker's budget is 768 MB and `2 × threads` files bound it; `import.record_bytes` is what a hostile file can add per file.

**Benchmarks to record (rule 48):** SI-0's store run and footprint re-runs, SI-15's acceptance block, 2.6b's per-call overhead on each OS, and a nightly 8 GiB run (4.3).

## Migration, with no shim

Nothing old ships. Within the branch sequence `legacy.rs` carries what is not yet converted, as it does today; it is registered in `docs/known-copies.toml` with `delete_by = debrief-v1.2.0`. C58 does not enforce `delete_by` on main yet (the release scope plan: by hand), so the tag check by hand covers it until then. There are no compatibility paths for old plugins, old settings or old trait names (`mem:no-backward-compat-before-v1`). In-memory `Importer::import` forms are deleted with their conversion, with goldens from `/samples` taken first (ER-11: the golden run must pass before and after).

**Order, ranked by bytes and memory saved.** "Saves" uses the bundle's facts as given (7.6 GiB, 1.43 GiB EVTX, 440 MiB UTF-16 `.reg`) and measured peaks. The other shares of the bundle were not read for this ADR [U].

| Rank | What | Why here | Saves |
|---|---|---|---|
| 1 | EVTX | 1.43 GiB in the bundle; today linear at 2,892 MB per 256 MB [family research, measured], over any cap, and the user's original crash | From about 16 GiB [calc, not run] to tens of MB [J]: 4 MB for the parser alone [M], plus items and buffers |
| 2 | `.reg` and other plugin line formats | 440 MiB UTF-16: bytes, string, `Vec<Record>`, then the host's copy. Half as many characters per byte as the CSV | About 3 GiB [calc from the CSV's ratio, not run] to about 140 MB, the core text path's figure on UTF-16 [M] |
| 3 | Archives (`.gz`, zip) | Engine process: an OOM here is uncaught, and a zipped bundle is refused today | Up to 4 GiB in the engine, to one chunk per member |
| 4 | Core JSON arrays, video frames | Engine process, whole | Up to file size |
| 5 | Plugin CSV and XML: Perfmon, Procmon, `.nfo`, log4j, HAR, OTLP | Usually smaller [U] | File size × about 12 (CSV's ratio) |
| 6 | Captures: nettrace, Chrome trace, cpuprofile, speedscope, UMDH, perf, folded, `.etl`, `.blg`; MEM-1 | In the UI process today; bounded by `Limits` | Input copies, 1 GiB at worst, ×2 in the app |
| 7 | Minidump summary, example plugin, test plugins | Small or exempt | Little; needed to delete the old traits |

### Work packages

Days are raw, without the plan's +20%, and ±50% [J]. "New" adds to the plan's total. "Plan" names packages in the release scope plan's 427 that this work uses. Owners follow AGENTS.md: shared integration points by the integrator. No measurement runs before the Dyno tag frees the queue, so everything marked "waits" is unmeasured today.

| WP | Scope | Depends | Lands | New | Plan |
|---|---|---|---|---|---|
| SI-0 | Failing tests (ER-11): 440 MiB UTF-16 `.reg`, 1.43 GiB EVTX, whole-read fake, zip bomb by ratio, 7.6 GiB zip, a plugin waiting for credit not judged stuck, a retry showing each row once. Budget cases: 20 MiB of comments before the first event, 20 MiB of unparsed lines, a read-ahead boundary on a large file, 20 MiB of XML outside rows, a 50 MiB HAR body, 200 empty EVTX chunks, a sparse EVTX log at 4 records per chunk, a nettrace with large metadata, a 100 MiB `.cpuprofile`, each in process and through the real host path with a fake plugin over real IPC. The host-path ones stay red until SI-10. **A real store run on the invented folder,** then the footprint and dense EVTX re-runs (waits) | this ADR | App only, now; runs after the queue frees | 3.5 | |
| SI-1 | The trait and types, inputs, framer budget, `ImportEnd`, `splittable`. Core importers on it, JSON split and framer, in-memory forms deleted | SI-0 | App only | 3.5 | 3.5: 1.5 |
| SI-2 | `chunk.rs` and decode to the SDK crate, `Decoded`, UTF-16 cost measured and fixed if it pays (waits) | SI-1 | App only | 1.5 | |
| SI-3 | The 14 plugin text importers on the trait, each over an in-memory input in tests, on today's serve loop while the wire waits: 8 line formats 3 (the registry is one), Perfmon and Procmon 1, `.nfo` and log4j 1.5, HAR and OTLP 1 | SI-1, SI-2 | App only | 6.5 | |
| SI-4 | `paddock-archive` `Members`, ratio guard, spool helper, `sweep_spools`, free-space preflight, fuzz target | 1.4 | **After the Dyno tag** | 3 | 3.5: 2 |
| SI-5 | Debrief on it: `.gz`, zip, spool phase and its wait, parallel members, `MAX_RESERVED`, `read_entry`, `source_slots` | SI-1, SI-4 | After SI-4 | 2.5 | 3.5: 0.5 |
| SI-6 | Per-file costs: sniff memo, batch with encoding, first read, one open, `RLIMIT_NOFILE`, `import.open_files`, head size (waits) | SI-1 | App only; the batch's wire half with SI-8 | 2.5 | 2.6a's declared sniff stays there |
| SI-7 | `HostFile: Read + Seek` with sequential read-ahead, sticky fatal error, `HostFiles::{register_open, open_stream}`, the host-side count in log-only mode | IP1, 2.6b | **After the Dyno tag** | 1 | 2.6b: 3.5 |
| SI-8 | `debrief.import.v1`, `.profile.v1`, `.document.v1` with `WireInput`, frame rule; family ADR 0003's table | SI-1, 2.6b, PH2a | **After the Dyno tag** (needs 2.6b) | 1 | PH2a: 2 |
| SI-9 | EVTX streamed in the plugin, with `fatal()` checks. Needs no 2.6d: at Checkpoint B the plugin's memory is flat, but the host still reads the whole file and collects every record until SI-10, and the 108 s deadline stands until 2.6d | SI-8, PH2b | After SI-8 | 0.5 | PH4: 3.5 |
| SI-10 | Host side: items to the batcher, no collection, bystander retry with a fresh id, credit counted as owed, the host-side count enforced, `.reg` and line formats live on the wire | PH2b, SI-8, 2.6d | **After the Dyno tag** | 2 | 3.4: 3 |
| SI-11 | Documents: `DocumentReader`, nettrace by block, Chrome framer, cpuprofile and speedscope `from_reader` (D3; waits), UMDH, perf, folded, `read_whole` with its declared reason | SI-1 | App only | 3 | |
| SI-12 | `.etl`, `.blg`, minidump summary. Windows runner | SI-8, SI-11 | After SI-8 | 1 | |
| SI-13 | Limits rows, Settings, ER-27 lines | 1.4, 2.3, PH2b, SI-5 | **After the Dyno tag** | 1 | 3.6: 3.5 |
| SI-14 | Static check over the wider scope 1. Flat-memory completeness and positive control 1. **Generators, since `grow` does not exist:** line and CSV repeat helper 0.5, JSON, XML, HAR and OTLP wrappers 1, profile formats 1, `ByPath` fakes 0.5 | SI-3, SI-11, 2.6b | **After the Dyno tag** (`conformance/`, `paddock-plugin-test`) | 5 | 2.6b's memory test is the base |
| SI-15 | Bundle acceptance: invented 14,000 files, 7.6 GiB, 1.43 GiB EVTX, 440 MiB UTF-16, 1 billion events nightly on the counting sink plus a store run; results block. `docs/plugin-sdk.md`, `docs/plugins.md`, `docs/ipc-wire.md`, the example, and the stale drawings (`c4-3-engine-ingest`, `uml-activity-ingest-*`, `uml-class-plugin-*`) with ER-23's independent review | all | Last | 3 | 4.3's nightly stays there |
| SI-16 | Delete the old traits: every row of [Callers of the old traits](#callers-of-the-old-traits), 28 test fakes 1.5, 25 of 46 fuzz targets 1, plugin-test harness 0.5, `Workspace` and agent 2, in-memory input 0.25, deletion and known-copies row 0.75 | all | Last | 6 | |
| SI-17 | UI at 14,000 sources: Explorer, source lists, the import report and Needs attention with thousands of rows, measured then fixed (waits). Not read or sized from code [U] | SI-15's folder | App only | 2 | |
| SI-18 | HDD gap (rule 13): parse pool limited by device kind for reads, Auto Setting, invented slow-reader test | SI-1 | After `paddock_proc::threads` work, **after the Dyno tag** | 1.5 | |
| | **Total** | | | **50** | **19.5** |

**Store-side fixes are an allowance, not a package.** SI-0's store run may find stops in the writer, DuckDB or the index. They are unknown today, so up to 5 raw days are held for them outside the 50 [J]; anything larger comes back as its own decision.

So this ADR adds about **50 raw days, about 60 with +20%**, of which SI-17 and SI-18 are conditional, 3.5 days together, and uses 19.5 days the plan counts: 3.5 4, 2.6b 3.5, PH2a 2, PH4 3.5, 3.4 3, 3.6 3.5. The first estimate of 29.5 left out the shipped callers, the generators and the store run. MEM-1 1.5, MEM-3 1 and MEM-4 1.5 were approved on 2026-11-05 and are **not** in the plan's 427 either, so they add 4 more: **54 raw, about 65 with +20%, about 15 working days at the plan's 4.25 agent-days a day [calc]**. SI-3 and SI-16's fakes are parallel and mechanical.

**Overlaps.**

- **`debrief/p0-idle`** (the base of this branch, moved on to `7b0c2421e`): no overlap in `ingest/`, `importers/` or `plugin-host`; only `debrief-plugins/tests/it/evtx_memory.rs` and its neighbours, which SI-0 and SI-9 use rather than replace [S, by `git diff --name-only`]. Stack SI-1 on `debrief/stream-text` after that branch lands on it.
- **`debrief/import-search`** `60f814bb4`: no overlap with `debrief-core` `ingest/`, `importers/`, `archive.rs`, the store's `writer.rs` and `segments.rs`, or the plugin crates [S, same check]. ADR 0062's `observe` hook sits below `BatchSink`, which this ADR leaves unchanged, so the two can land in either order.
- **MEM-1** lends captures by range and drops the copies at `profiling.rs:113,196`. SI-11 changes the same call's types, so **one agent does both, SI-11 first**. MEM-1's release of the lock during import stays MEM-1's.
- **MEM-2** (byte budget for open documents, LRU) is independent: it bounds output, this bounds input. Land in either order.
- **MEM-3, MEM-4** are SI-12: count them once.
- **MEM-5** (PDB budget) reads symbol folders, not imported files, and stays separate.
- **ADR 0063** changes the wire to the UI, not this one. **Y13's PML and format gaps** build on SI-1.
- **Checkpoint B has streamed EVTX in the plugin, and does not open the 1.43 GiB log.** Plan §2 puts the Event Viewer fix at B, through PH4. SI-9 lands with PH4 and passes its 256 MB test with a flat plugin. But until SI-10 the host still reads the whole file (`ingest/source.rs:335`) and collects every record (`debrief-plugin-host/src/remote.rs:163`), and SI-10 needs 2.6d. So **the 1.43 GiB log opens only after 2.6c, 2.6d, 3.4 and SI-10**, and the whole folder also needs SI-3 and SI-6 (D7).
- **Checkpoint B's slack is used up [calc].** Plan §2 has a 16.5-day serial chain that must start by about T+8 for B at about T+25. SI-7, SI-8 and SI-9 add 2.5 serial days to it. SI-0 and SI-1 must finish before SI-8: 7 days, app only, on a second agent from T, or B moves day for day. So B is about T+27.5 from a T+8 chain start, or T+25 only if the chain starts by about T+5.5.
- **Shared packages** wait for the Dyno tag, and each says so in the table.

### Callers of the old traits

"The old traits go" needs a decision for every caller. Checked with grep at `3168b6145` [S]; decisions are [J].

| Caller | What it does | Decision |
|---|---|---|
| `debrief-core/src/workspace/mod.rs:1143,1254,1262`, `read_with_retry` `:1313` | The in-memory `Workspace` reads a whole file, then imports it | `import_file` goes through `ingest::import_path` into a `BatchSink` that keeps records. `read_with_retry` goes. `Workspace` still keeps records in memory by design (D8) |
| `debrief-core/src/workspace/mod.rs:1264`, `tail/files.rs:182,299`, `tail/mod.rs:735` | Live tail on `Workspace` | Same, through `Importer::tail` and `read_appended` |
| `debrief-agent/src/trigger.rs:202` | The shipped headless agent opens with `start_at_end` (`workspace/mod.rs:474-500`). `skip_history` seeds each file from its end by range; a file it cannot seed, because its importer has no tail, goes to `import_file` and is read whole. Polls go through `refresh_tail`, by range; a gzip is read whole (`tail/files.rs:182,241`) [S, flow traced, not run] | Stays on `Workspace`'s tail. The whole-read cases go with the `import_file` row above and SI-5's gzip reader |
| `debrief-core/src/workspace/insights.rs:269` | Preview of a pasted sample, already in memory | `MemoryInput`, a small in-memory input in the SDK, capped by the request |
| `debrief-store-duck/src/diag.rs:253` | Re-runs an importer on one stored event's `raw` | `MemoryInput` and a collecting sink, at most the limit of rows it already has |
| `debrief-core/src/guard.rs:576-648` | Guarded calls | One wrapper, no `import_owned` |
| `debrief-store-duck/src/tail.rs`, `ingest/region.rs`, `ingest/plan.rs:147,166`, `parser/set.rs:304-325`, `debrief-store-duck/src/read.rs:399` | Tail, selection, sniffing | Follow the trait; no whole read today except through `legacy.rs` |
| `app/src-tauri/src/profiling.rs:113,196`, `dumps.rs:100,323,346` | App wrappers; `profiling.rs:196` reads a capture whole in the UI process | SI-11 with MEM-1 |
| `app/src-tauri/src/share.rs:667`, `settings.rs:940,1443`, `ai/commands.rs:64` | Build a `Workspace` for its registry or an empty one, no file | No change beyond the type |
| `debrief-plugin-test` `import_file`, `golden.rs`; `debrief-plugin-e2e` | Plugin test harness | Rewritten on the trait, `MemoryInput` and files |
| `debrief-plugin-host/src/bin/fake_plugin.rs`, `debrief-plugin-test/src/bin/fixture.rs`, example plugin | Test plugins, SDK example | Rewritten; the example is what authors copy |
| 28 test fakes, in `debrief-core/tests/it`, `src` test modules, `debrief-profile`, `debrief-dump`, `app/src-tauri` | Fixed-result importers | Mechanical, onto `MemoryInput` and a sink |
| 25 of 46 fuzz targets | Call parsers on bytes | Over `MemoryInput`; the parsers' internal entry points stay where they are `pub` |
| `debrief-core/src/config/`, `ConfigParser` | Configuration diff | No change (D1) |

### The conformance check that stops the next whole read (ER-21)

**Pattern:** an importer holds a whole file because the interface hands it one, or because a helper makes it easy. **Mechanism:** that interface no longer exists. **Check,** four layers, each failing on its own:

1. **Types and manifest.** No trait takes `&str` or `&[u8]` of a whole input. `read_all`, `read_text` and `Imported.records` are deleted. The only whole read is `Seekable::read_whole(WholeReason)`. The reason is a closed enum, declared in the manifest (`ImporterCap.whole`), and the cap comes from the limit registry by reason: `HeapSnapshot` and `Pprof` use `import.document_bytes`, `DumpWithoutMemoryList` uses `import.dump_whole_bytes`. An importer cannot pick a larger cap or a reason it did not declare.
2. **Runtime, on both sides.** The framer budget fails `read_to_end` past it, in shipped builds, in any importer written against the SDK. The host's count fails a plugin that calls `ReadFile` itself. `limit_case!` covers both.
3. **Completeness test: flat memory for every importer.** `debrief-plugin-test`'s `importer_memory_is_flat` (on 2.6b's `paddock-plugin-test` base) imports each importer's sample scaled to 1× and 64×, in a child with a counting allocator, and asserts peak heap grows by less than 1.5× [J: to tune]. **Minimum sizes, so buffers cannot hide a copy: 1× at least 1 MiB, 64× at least 64 MiB, and both must produce at least 1,000 events** (documents: the model's limit). Counting allocations, not RSS, keeps it deterministic (ER-12). A sibling test lists every importer of every first-party plugin and of core from `Contributions`, and fails on one without a case. So a new importer has a test or the suite is red. Exception importers assert their registry cap instead. **Positive control (rule 71):** a fake that collects, a fake that calls `read_to_end` and a fake that calls `ReadFile` directly must each fail. Generators per format do not exist yet; SI-14 sizes and builds them.
4. **Static check, `C<next free>` `whole-file-read`,** in `conformance/src/checks/`, in the shape of C54 (`C54-background-qos.mjs`). Over `apps/debrief/crates/debrief-plugin-*/src`, `debrief-core/src/{ingest,importers,workspace}`, `debrief-core/src/archive.rs`, `debrief-profile/src`, `debrief-dump/src`, `debrief-etw/src`, `app/src-tauri/src/{profiling,dumps}.rs`, tests excluded: `read_to_end`, `read_to_string`, every `read_exact` and `read_at`, `fs::read`, `fs::read_to_string`, `io::copy` into a `Vec`, `.bytes().collect`, `from_buffer`, `read_whole`, `Reader::from_str`. A text check cannot tell a whole-file length from a bounded one, so every `read_exact` and `read_at` needs the site comment. It finds the common shapes and not every one; layers 2 and 3 catch the rest. A site carries `conformance: whole-read — <WholeReason, or the bound that makes it not whole>` (a `.take(cap)` under the budget is fine and says so). Warning mode until SI-16, then an error. Its own fixture tests fail and pass one case each. Clippy's `disallowed-methods` was considered and not used: `clippy.toml` is per workspace, and the store, app and tool crates read files whole for good reasons.

## Options rejected

| Option | Why not | Basis |
|---|---|---|
| Larger caps, keep the traits | 3,535 MB for 300 MiB, 17,399 MB for 3 GiB. No cap fits 7.6 GiB | [M] |
| A record pull in the trait (`next_record`) | `ProcessTrace` and serde visitors would need a thread each. Pull parsers just loop, and push is flat at 62-68 MB | [M, R] |
| Async streams | The IPC is synchronous, on threads, with no runtime (family ADR 0001) | [S] |
| One trait for documents too | A heap graph or dump is one model. Sharing the input and keeping a small second trait is honest; a record sink would be a fake | [J, R] |
| Keep six traits, add a streaming method to each | Two forms of everything, and whole reads stay available. ADR 0014's "adapter, until it streams" is still there | [S] |
| Runtime `Input` enum, declared in the manifest separately | Can disagree with the code; a mismatch is found only when a file arrives. The type makes it impossible | [J] |
| Whole read as a free function, reasons in comments | Comments do not close a set. A closed enum, declared in the manifest, with its cap from the registry does | [J] |
| Streaming over protocol 1 first | A 5 ms pump on every read, and PH2 throws the work away | [S, calc] |
| `raw` as an offset in the store | Breaks the self-contained project (ADR 0014), the redacted copy, timestamp diagnostics and the context fallback for any file that moved or was in an archive | [S] |
| Plugin reads by path for every format | Needs `OpenedFilePaths` and `FsAccess`; third-party plugins never get them. Only for what must have a path | [S, R] |
| Host passes an open handle | A new mechanism per OS, for a round trip measured at 1.9 ms per MiB, 525 MiB/s [M]. Not needed; reconsider on 2.6b's numbers | [M, R] |
| mmap for minidump | `HostFile` cannot back `Deref`; a truncated file raises SIGBUS, which no `Result` catches. `source::load` already gives the bound | [R] |
| Host lends decoded text | Decoded offsets do not map to file offsets for UTF-16, and it decodes in the store worker for no gain | [J] |
| Extract every member to disk (family ADR 0018 decision 4) | Extra write and disk for what can be read in order. Now only for members that must seek | [J] |
| Arrow across the plugin boundary | Refused by ADR 0017 §3. Importers parse before the pipeline | [S] |
| Replace `evtx`, `minidump`, `csv` | None is needed; `evtx` reads flat over a reader | [M, R] |
| Report the minidump bug upstream | User, 2026-11-05: not reported. The guard stays on `fix/debrief-fuzz-2026-11-05` | user |
| Keep `MAX_ENTRY_BYTES` and `MAX_ARCHIVE_BYTES` beside the ratio | They refuse the bundle as a zip, and were only there because members were held in memory | [S] |

## What this does not decide

- **The store, the index and queries at 1 billion events.** This ADR gives bounded per-import state. Checked at the importer's edge: `EventId` is `u64`, `SourceId` is `u32`, `Record.line` is `u32` and saturates per file, `ContextRequest.record_id` is `u32` [S], which is fine at 1 billion but worth watching. Reading side at 8 threads 2.0 M events/s [M], so 8 minutes for 1 billion at best [calc]. **There is no measured time for 1 billion.** ADR 0062's `large10` baseline is 56.2 M events: import 39.7 s, search ready 208.3 s, catalogue 376.4 s. Scaled linearly that is about 12, 62 and 112 minutes [calc], but that is a floor for a person's wait and not a forecast: the end-of-import position sort, segments, compaction, disk and DuckDB memory do not scale linearly by any evidence here. `raw`'s disk cost at that size is D5.
- **Hot paths in the store writer.** Not measured here; every number above uses a counting sink.
- **PDB budgets** (MEM-5), **document budgets** (MEM-2), **file-backed models** (MEM-6).

## Consequences

- One way to write an importer; the SDK docs and the example become shorter. Memory no longer depends on the file's size, except the four exceptions.
- The plugin bundle gains `encoding_rs`, and the store worker keeps the detector.
- A raised budget or spool is a Setting the user may meet, each with a message and Raise it.
- A file retried after a crash has a source id out of sorted order.
- `.reg`, `.evtx` and archive imports report damage and keep what was read, a behaviour change from "any failure removes the file". Events from a damaged file are now shown, with a problem.
- Temporary disk is used for seek-needing members, cleaned on every path and swept after a crash.
- Risks: regressions in `raw` and line numbers where importers derived them from the whole content, covered by `/samples` goldens (SI-1, SI-3) and 3.5's differential tests. A slower `from_reader` for cpuprofile and speedscope (D3). `evtx` is moving fast, so its test pins behaviour and not a version [R]. The stall judgement must land before 3.4 (ER-19). Unknown: worst-case `evtx` output per 64 KiB chunk [R].

## Decisions for the user

| # | Question | Recommendation |
|---|---|---|
| D1 | Configuration parsers (`parse(&str)`, 64 MiB cap, used for configuration diff) read whole. They are not importers, but "all file types" could include them | Leave them out: small files, a cap and a typed message exist, and `toml`, `yaml` and `ini` crates take a string |
| D2 | A plugin crash mid-file: remove what was read (as today), or keep it with a problem? | Remove it for 1.2, as today: a crash says less about what was read than a damaged-file report does, and Retry exists. Revisit on real crashes |
| D3 | `.cpuprofile`, speedscope and the OTLP pretty document stream under this ADR, though the research note left them whole. If `from_reader` is much slower (SI-11 measures), add them as a fifth exception? | Yes, only if it is over 2× slower on a 100 MiB invented file. Otherwise no |
| D4 | The ratio guard replaces 1 GiB per entry and 4 GiB per archive: default 200:1, floor 64 MiB, a Setting | Accept. A highly repetitive log can pass 200:1 and gets Raise it |
| D5 | `raw` stays in the store, which is most of the stored text (ADR 0014's table). Only the context fallback, timestamp diagnostics and the redacted copy read it. At 1 billion events, is a later ADR on `raw` wanted? | Yes, after SI-15's run gives the disk figure. Not in 1.2 |
| D6 | SI-0 to SI-16 need 46.5 raw days. SI-17 and SI-18 are conditional, 3.5 days together, and the 50 includes them. MEM-1, 3 and 4 add 4, which makes 54 raw, about 65 with +20%, on top of the plan's 427. Up to 5 more are held for store-side fixes SI-0 may find. The end moves from about T+103 to about T+118, about 2027-04-22 instead of 2027-04-01, or about 1.5 working days later with the full allowance [calc]. Does this go into Debrief 1.2? | Yes for SI-0 to SI-16, which the folder requirement needs. SI-17 and SI-18 follow what SI-0 and SI-15 find |
| D7 | When does the 1.43 GiB log open? Checkpoint B has EVTX streaming only in the plugin. (a) Move 2.6c, 2.6d, 3.4 and SI-10 into B: B itself moves to about T+41. (b) Leave them in plan order: B stays at about T+27.5 with the 256 MB fix, and the large log comes as late as about T+55. (c) Run them first in phase 3: B stays at about T+27.5, and the large log opens at about T+41 with no added days [calc: 13.5 serial days, one agent; T+39 with B at T+25] | (c). It costs only reordering, because the plan's limit is capacity and not the critical path. These four have no known shortcut unless the stall judgement, which ER-19 needs, is dropped. The whole folder also needs SI-3 and SI-6, which can run beside them |
| D8 | The in-memory `Workspace` still keeps every record, in `store.rs:309` and the agent. ADR 0014 said it would be removed. Remove it in 1.2? | Not in 1.2. Route it through the trait (SI-16) and take it up after, because removing it reaches about 15 app files [S, count of files naming it] |
