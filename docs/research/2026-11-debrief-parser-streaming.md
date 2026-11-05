# Debrief: which parsers can stream, and what to do about the others

Date: 2026-11-05. Input to the ADR for one streaming importer trait (`debrief/stream-text`). Debrief source read at
`origin/debrief/p0-idle` b4bf8cb6d, versions from its `apps/debrief/Cargo.lock`. Paths below `apps/debrief/crates/` are
written `debrief-x/src/...`, and short forms mean: `ingest/`, `importers/`, `workspace/`, `archive.rs` in `debrief-core`;
`nettrace/`, `pprof/`, `heapsnapshot.rs`, `util.rs` in `debrief-plugin-profiling`; `blg.rs`, `procmon.rs` in
`debrief-plugin-windows`. `docs/roadmap.md` and `docs/profiling.md` are under `apps/debrief/`. Third-party paths are
`crate-version/src/...` in the cargo registry, or named by file once the crate is clear.

Labels: **[source]** read from source; **[measured]** a number from an earlier recorded measurement (no new measurement
here: the brief asked for no builds); **[judgement]** my reasoning, not checked by running it; **[meta]** crates.io or
GitHub API, fetched 2026-11-05.

## Answer

"Streaming for all file types" can be kept as: **no importer holds a whole file in memory just to read it**. For every
format that yields records (logs, EVTX, CSV, JSON, XML, HAR, PML), memory can be flat in the input size. It cannot be kept
literally in four places, and the ADR should say so rather than promise it:

1. **Formats whose result is one in-memory model.** Heap snapshots (a graph analysed whole) and pprof (protobuf decoded
   whole). The input can stop being copied, but memory grows with the model, within `Limits`. Same message as today.
2. **`minidump` needs one `&[u8]`.** Full dumps already avoid the whole file (prefix plus stacks, at most 768 MiB).
   Dumps without a full-memory list stay whole up to 1 GiB, refused above that with a message that exists.
3. **`.etl` and `.blg` are read by Windows, by path.** That is streaming, done by the OS, but the host cannot lend them, so
   the plugin needs the path. On other systems they stay unsupported with the existing messages.
4. **A compressed member for a format that must seek** (`.evtx` in a zip or `.gz`): spooled to a private temporary file,
   capped. It is not in memory, but it does use disk.

Nothing needs replacing before 1.2. One small upstream report is worth making (`minidump`), and one is worth considering
after a measurement (`evtx`).

## Verdicts

"Whole file at" names who forces the whole file today. The host forces it for every plugin: `read_to_end` in
`debrief-core/src/ingest/source.rs:333-335,356-358` and `HostFile::read_all` in `debrief-plugin-api/src/serve.rs:128,131`,
so a plugin holds at most one copy plus its results, capped at `MAX_IMPORT_BYTES` 2 GiB (`serve.rs:25`). The table does not
repeat that.

### Formats that yield records

| Format | Parser | API | Whole file at (beyond the host) | Verdict |
| --- | --- | --- | --- | --- |
| `.evtx` | `evtx` 0.15.1 | `from_read_seek`, iterator of records, chunk by chunk | wrapper: `from_buffer(bytes.to_vec())`, a second copy (`debrief-plugin-windows/src/evtx_import.rs:57`), and `Vec<Record>` | **Stream as is** over bounded `Read + Seek`. Plan PH4 already says so. See the seek cost below |
| Text lines (syslog, journald, access, W3C, Docker, CRI, ERRORLOG, `.reg`) | none, in-house | built-in `LineReader` exists (`ingest/source.rs:343-348`) | `TextImporter::import(&str)` | **Stream**: move each to a line loop. Bounded by `MAX_LINE_LEN` 4 MiB |
| CSV (generic, PerfMon, Procmon) | `csv` 1.4.0 | `Reader::from_reader(Read)`, reusable `ByteRecord` | wrapper: `from_reader(content.as_bytes())` (`debrief-core/src/importers/csv.rs:136-141`), rows collected | **Stream, with a record budget**: the crate has no size limit (below) |
| XML (`.nfo`, log4j) | `quick-xml` 0.42.0 | `Reader::from_reader(BufRead)` + `read_event_into` | wrapper: `from_str` over the content (`debrief-plugin-windows/src/nfo.rs`, `debrief-plugin-formats/src/log4j.rs`) | **Stream, with a record budget**; keep the depth cap (`nfo.rs:13`) |
| JSON lines | `serde_json` 1.0.154, per line | lines | none: already streams | **As is** |
| JSON arrays | `serde_json` 1.0.154 | `from_str::<Vec<Value>>` today; no array iterator | wrapper (`ingest/json.rs:107-117`) | **Wrap**: a small framer finds element bounds, then `from_slice` per element, capped. Not `from_reader` (below) |
| HAR | `serde_json` 1.0.154 | as above | wrapper: `from_str::<Value>` (`debrief-plugin-web/src/har.rs:27`) | **Wrap, differently**: a visitor over a typed `Entry` with `from_reader(BufReader)`. Bodies are skipped unbuffered (below). Fallback: `struson` |
| OTLP JSON | `serde_json` 1.0.154 | lines, or one document | document path only (`debrief-plugin-otel/src/lib.rs:76-85`) | **Lines stream**. A pretty-printed document stays whole up to a cap: `resource` may follow `scopeLogs`, so order cannot be assumed, and such exports are small [judgement]. Message: "this file is one JSON document of N MB; export as JSON lines" |
| Process Monitor PML | **no crate** on crates.io (searched `procmon`, `process monitor`, `pml`) [meta] | to write, roadmap only (`docs/roadmap.md:44`) | not built | **Write in-house over `Read + Seek`**, format reference `procmon-parser` (below) |
| Registry hives | none in use; only `.reg` is imported | candidates load the whole file | not built | **Not on the roadmap. If added: whole under a cap**, or in-house (below) |

### Formats read by a tool, by path

| Format | Reader | API | Whole file at | Verdict |
| --- | --- | --- | --- | --- |
| `.etl` | own bindings, `OpenTraceW` + `ProcessTrace` (`debrief-etw/src/sys.rs:164-186`) | **push**: an OS callback per event; two passes (`scan`, `convert`) | wrapper writes the lent bytes to a temp file (`debrief-plugin-native/src/importer.rs:95`), then reads the converted NetTrace whole (`:114`) | **Cannot be lent. Give the plugin the path**; spool only for archive members. Non-Windows: "ETL import needs Windows" (`importer.rs:52`) stays |
| `.blg` | `relog.exe` (plugin holds `OpenedFilePaths`, `debrief-plugin-windows/src/lib.rs:107`) | tool writes CSV to a temp file | `fs::read` of that CSV (`blg.rs:67`) | **Stream the temp CSV** through the PerfMon reader. Non-Windows message stays (`blg.rs:14`) |

### Formats whose result is one document

These do not yield records, so a record sink does not fit them. Only their input can stream.

| Format | Parser | API | Whole file at | Verdict |
| --- | --- | --- | --- | --- |
| Minidump, summary | `minidump` 0.27.0 | `Minidump::read(T: Deref<Target=[u8]>)` | `read_minidump(bytes)` (`debrief-plugin-windows/src/minidump_import.rs:73`) | **Wrap: use `debrief-plugin-dumps::source::load`**. A second minidump path that skips it |
| Minidump, document | same | same | `load` already reads prefix and stacks (`debrief-plugin-dumps/src/source.rs`) | **Keep the wrap**: bounded by `MAX_PREFIX` 512 MiB + `MAX_KEPT` 256 MiB (`:48,52`). Without a full-memory list: whole, up to `max_read_bytes` 1 GiB, with a message (`:144-149`). **Report the handle bug** |
| PDB | `pdb` 0.8.0 | `PDB::open(Source)`; default `Source` for `Read + Seek`; one `Vec` per stream | none (`File`, `debrief-plugin-dumps/src/pdb_symbols.rs:95-96`) | **Wrap: own `pdb::Source` that refuses a view above a cap**. Not lent: read from symbol folders. Add `fuzz_pdb`. `pdb2` is the later swap |
| UMDH | none, in-house | lines | `read_exact(src, 0, len)` (`debrief-plugin-dumps/src/umdh_reader.rs:78`) | **Stream**: output is already bounded by `max_stacks` |
| `.nettrace` | none, in-house | `Cursor` over a slice; length-prefixed blocks (`nettrace/format.rs:196,259`, `v6.rs:58`) | wrapper | **Change the in-house reader to one block at a time**, block size capped [judgement: the walk is sequential; confirm in V6] |
| Chrome trace | `serde_json` | JSON array or `{traceEvents}` | wrapper | **Stream input with the framer**; model bounded by `Limits` |
| `.cpuprofile`, speedscope | `serde_json` | one document | wrapper | **Whole, up to `max_input_bytes` 1 GiB**, existing message. Usually small [judgement] |
| `.heapsnapshot` | `serde_json` into flat `Vec`s (`heapsnapshot.rs:50-59`) | one document | wrapper; model is the graph | **Truly whole model.** `from_reader` could drop the raw copy; measure, as it may be slower. Limit and message exist: "open a smaller capture" |
| pprof | `prost` 0.14.4, `decode(impl Buf)` | one message | wrapper plus inflate (`pprof/mod.rs`, `util.rs:17-32`) | **Truly whole model.** Stay within `max_input_bytes`; existing message |
| folded, `perf script` | none, in-house | lines | wrapper | **Stream** |
| `.gcdump` | **none**: TraceEvent's FastSerialization, documented only by its source (`docs/profiling.md:201`) | not read | not built | **Not read today, and no change.** No crate found [meta] |
| Live .NET EventPipe | own (`debrief-plugin-dotnet/src/eventpipe`) | live stream, not a file | n/a | **No change** |

### Containers and internal formats

| Format | Parser | API | Whole file at | Verdict |
| --- | --- | --- | --- | --- |
| `.gz` | `flate2` 1.1.10 | `MultiGzDecoder<R: Read>: Read` (`gz/read.rs:264-307`) | `read_capped` to `Vec` (`debrief-core/src/archive.rs:96,410-412`) | **Stream as is**. The bomb guard is Debrief's own, and plan 3.5 already changes it. Sequential only |
| `.zip` | `zip` 9.0.5 | `ZipArchive::new(Read + Seek)`; members are `Read` | wrapper: slice functions `central_records` and `check_ranges` (`archive.rs:215,282`), and `read_entry` re-expands everything (`:402-408`, used by `workspace/context.rs:27`) | **Stream as is; patch the wrapper** to read the central directory and the 30-byte local headers by range. Refuse by the EOCD count before `ZipArchive::new`, which builds its map first (`read/zip_archive.rs:82`) |
| `.tar` | `tar` 0.4.46 (roadmap, `docs/roadmap.md:36`) | sequential `entries()` (`tar-0.4.46/src/archive.rs:82`), `entries_with_seek` (`:189`) | not built | **Stream as is** when added |
| `.xz`, `.zst`, `.bz2` | not import formats | `lzma-rs` 0.3 is push-only into `Write` (`lzma-rs-0.3.0/src/lib.rs:43`) | not built | **No decision needed.** If `.xz` is added, `lzma-rs` needs a thread, so look for a `Read` decoder first |
| Parquet | `parquet` 60.0.0 | `ChunkReader` by range | internal segments only (`debrief-store-duck`) | **Not an import format; no change** |

## Findings behind the verdicts

### `evtx` 0.15.1 streams, with one surprise [source]

- `from_read_seek` reads the header and asks for the length (`evtx_parser.rs:511-530`). Each chunk is one 64 KiB `Vec`
  (`:570-583`).
- Debrief builds with `default-features = false`, so no `multithreading`: `chunk_pipeline` runs inline with one recycled
  arena (`:757-764`, single-thread branch). Records are collected per chunk (`serialized_records`, `:841`).
- Across chunks, compiled programs are capped at 64 MiB (`binxml/compiled.rs:86`) and per chunk at 16 MiB (`:83`). Element
  depth is capped at 512 (`binxml/ir.rs:265`). So memory should not grow with the file. [measured] today's peak is linear,
  2,892 MB for a 256 MB log, from the two copies and `Vec<Record>`
  ([2026-10 memory research](2026-10-plugins-memory-and-manager.md), table at line 88). Its 50-70 MB for the streamed form
  is an estimate, not a measurement; PH4's test is what checks it.
- **The surprise:** `chunks()` first calls `oldest_chunk()`, which seeks to every chunk and reads its header
  (`evtx_parser.rs:670-690`) before the first event. Over a lent file with naive read-ahead, that transfers the whole file
  once just to find the start. A 4 GiB log has 65,536 chunks, so this is 65,536 round trips without read-ahead, or 4 GiB
  with it. `HostFile`'s `Read + Seek` (plan 2.6b) should read ahead only on sequential reads. Measure this with the 2.6b
  overhead benchmark. If it hurts, a setting for file order is a small upstream PR, since Debrief does not rely on
  `.evtx` chunk order [judgement: confirm against the batcher].
- A hostile chunk's expansion into JSON has no output cap I could find. **Unknown:** the worst case per 64 KiB chunk. A
  `fuzz_evtx` target exists; adding the memory conformance from 2.6b to it would show this.

### `minidump` 0.27.0 cannot take a reader [source]

- `Minidump<T: Deref<Target = [u8]>>` (`minidump.rs:5495-5503`) borrows from the slice, so nothing short of a slice or a
  map works. `MmapMinidump::read_path` exists (`:5456-5467`).
- **Why not mmap.** `HostFile` cannot back a `Deref`, so it needs a path or a passed handle. A file truncated under a
  map raises SIGBUS (Unix) or an in-page error (Windows) [judgement], which is a crash no `Result` catches. Tolerable in a
  plugin process, never in the engine. `source::load` already gives the bound without any of that, so keep it.
- **The bug is still on upstream `main`** (fetched today, `minidump/src/minidump.rs:1943-1954`).
  `MinidumpHandleDataStream::read` passes the file's `size_of_descriptor` to `ensure_count_in_bound` (`:1946`), so size 0
  accepts any count, and `Vec::with_capacity(number_of_entries)` (`:1956`) reserves for it. The other callers are safe:
  `read_list` uses the type's size (`:1307`), and `read_ex_list` refuses a declared size that differs (`:1371`).
  Debrief's guard is on `fix/debrief-fuzz-2026-11-05` (`d1f67d311`). **Worth reporting:** a one-line fix, either refuse a
  size below the descriptor's or bound the capacity by `bytes.len() / size`. My issue search hit the API limit, so check for
  an existing report first.

### `pdb` 0.8.0: seeks, but a view is sized before it is read [source]

- Each view does `Vec::with_capacity(len)` and `resize(len, 0)` before `read_exact` (`source.rs:88-90`), and a stream's
  size comes from the file (`msf/mod.rs`, `look_up_stream`), up to `u32`. So a small file can ask for a very large zeroed
  buffer [judgement: not reproduced]. An own `Source` with a cap closes this and is about 30 lines. There is no `fuzz_pdb`
  among the 46 fuzz targets, though PDBs can come from symbol servers.
- Memory per stream is the stream's size. Debrief reads modules one at a time, so the peak is the largest stream plus the
  result table (`MAX_SYMBOLS` 4 million, `pdb_symbols.rs:19`).
- Health: last release 2022-06-08 [meta]. `pdb2` 0.11.0 (2026-10-20) has the same `Source` shape (`pdb2-0.11.0/src/source.rs:49`),
  so a later swap touches one file [judgement].

### No size limit in `csv`, `quick-xml` or `serde_json` [source]

- `csv`: `OutputFull` doubles the record with no limit (`reader.rs:1655-1657`, `byte_record.rs:530-532`). One unterminated
  quote makes the rest of the file one field.
- `quick-xml`: `read_event_into` extends the caller's buffer until the event ends (`reader/buffered_reader.rs:80`), and the
  open-tag names accumulate (`reader/state.rs:48-51`) unless the caller stops at a depth.
- `serde_json`: strings grow `scratch` (`read.rs:222-235`). Depth is capped at 128 by default (`de.rs:63`).

**One mechanism for all three: a record budget on the input.** A reader that counts bytes since the importer last said
"record boundary", and fails with an `io::Error` above the limit. All three crates return that as an ordinary error. Debrief
already has the numbers: `MAX_LINE_LEN` 4 MiB and `MAX_EVENT_BYTES` 8 MiB (`ingest/mod.rs:58`).

### JSON: a framer for arrays, a visitor for HAR [source + judgement]

- `from_reader` reads through `io::Bytes`, one byte per call (`read.rs:149-153`), and its docs warn it is usually slower than
  reading the file and using `from_slice` (`de.rs:2534`). So for arrays of small items, a framer (track string and depth, cut
  at the top-level comma, `from_slice` each, about 50 lines) is both bounded and cheaper. It needs a cap per element.
- HAR is the opposite: bodies are base64 and can be many MB, and Debrief uses only a few fields (`har.rs:54-110`). With
  `IgnoredAny`, `serde_json` skips strings without buffering (`ignore_str`, `read.rs:352`), so a typed `Entry` holds no body at
  all. A framer would buffer the body first. Visitors are **push**. A retained `url` can still grow; a `data:` URL is the
  case to test. If that proves awkward, the candidate is `struson` 0.7.2 (MIT OR Apache-2.0, a pull reader); I read only its
  metadata.

### `zip` and `flate2` [source]

- A `zip` member is `Read`, not seekable once deflated [judgement: `by_index_seek`, `read/zip_archive.rs:631`, not
  read in depth]. Central-directory memory grows with entries, so `MAX_ENTRIES` 65,536 must be checked first. `zip` has a guard
  against a lying count (`:247`) but does not limit expansion: bombs are the caller's job, and Debrief's `check_ranges`
  (overlapping entries) must survive the rewrite.
- `flate2` caps its header buffer at 64 KiB (`gz/mod.rs:23`). Output is whatever you read, so the caller bounds it.

### PML and hives [source + meta]

- PML, from `procmon-parser`'s [format notes](https://github.com/eronnen/procmon-parser/blob/master/docs/PML%20Format.md)
  (MIT, pushed 2026-10-25) [unverified against a real PML]: the header gives offsets to the events, event offsets, processes,
  strings, and hosts and ports. Tables lie at the end, so **seek is required**. Events can then be walked sequentially, each
  with a fixed header plus stack and detail sizes. The tables need caps or an LRU, or memory grows with distinct strings.
- Hives: `notatin` 1.0.1 takes `ReadSeek` but reads all of it (`file_info.rs:35-41`). `regf` 0.2.0 does too (`hive.rs:35-37`).
  `nt-hive` is GPL-2.0-or-later, so it needs an ADR and is out. `frnsc-hive` was not read.

### Health and licence [meta]

| Crate | In use | Latest, date | Licence | Note |
| --- | --- | --- | --- | --- |
| evtx | 0.15.1 | 0.15.1, 2026-11-01 | MIT/Apache-2.0 | Active: pushed 2026-11-03 |
| minidump | 0.27.0 | 0.27.0, 2026-08-11 | MIT | Active, 92 open issues |
| pdb | 0.8.0 | 0.8.0, 2022-06-08 | MIT OR Apache-2.0 | **Stale.** `pdb2` 0.11.0 and `ms-pdb` 0.1.22 are alternatives |
| quick-xml | 0.42.0 | 0.42.0, 2026-08-22 | MIT | Active |
| serde_json | 1.0.154 | 1.0.154, 2026-10-11 | MIT OR Apache-2.0 | Active |
| csv | 1.4.0 | 1.4.0, 2025-10-17 | Unlicense/MIT | Stable, slow by design |
| flate2 | 1.1.10 | 1.1.10, 2026-11-02 | MIT OR Apache-2.0 | Active |
| zip | 9.0.5 | 9.3.0, 2026-11-03 | MIT | Active, frequent majors: 9.0.5 is behind |
| prost | 0.14.4 | 0.14.4, 2026-06-07 | Apache-2.0 | Active |
| parquet | 60.0.0 | 60.1.0, 2026-10-28 | Apache-2.0 | Tied to duckdb-rs's Arrow |

Panics: I counted `unwrap`, `expect` and `panic!` by grep, and the counts include doc examples, so they prove nothing either
way and are not quoted. What holds in Debrief: third-party parsers run in plugin processes under a cap (1 GiB default; dump
readers 3 GiB, `debrief-plugin-dumps/src/plugin.rs:27`), so a panic or an abort is contained and reported as a crash.
In-process core importers run under `catch_unwind` (`debrief-core/src/guard.rs:70`). **Gaps:** an OOM in the engine is not
caught, so `csv`, `flate2` and `zip` in `debrief-core` need the budget before they read from disk. And of the formats above,
PDB has no fuzz target (`.nfo` is covered by `fuzz_perfmon_csv`).

## What the trait must support

| Need | Forced by | Consequence |
| --- | --- | --- |
| **Record push into a blocking sink** | `ProcessTrace` callback, serde visitors. These cannot be turned into a pull without a thread | The trait is `import(ctx, input, sink)`. Every pull parser (`evtx`, `csv`, `quick-xml`, lines) just loops. Plan 2.6b's `ItemSink::send` (blocks without credit, fails on cancel) already has this shape |
| Record pull | Nothing requires it | **Do not put it in the trait.** Offer it only as a test helper |
| **Seek on input** | `evtx`, zip, PML, minidump `load`, hives; rewinding for multi-pass and sampling (`DAY_ORDER_SAMPLES` in `procmon.rs:31`) | Input is `Read + Seek` when the importer asks |
| **A declared need** | `.gz` cannot seek; `.etl`/`.blg` need a path | The importer declares `Sequential`, `Seekable` or `Path`, in the manifest. The host satisfies it, spooling a compressed member to a capped private temp file. Sequential formats never spool |
| Path | `OpenTraceW`, `relog` | `Input::path()` for a plugin holding the grant |
| **A shared input, separate outputs** | Dumps and profiles yield one document | "One streaming trait" covers records. Document importers take the same `Input` and keep producing a bounded document. `ProfileImporter::import_encoded(&[u8])` is the one to change; `DocumentImporter` already takes `RangeRead` |
| Record boundary | The budget above | `input.record_boundary()`, or the sink's `send` counts as one. The latter is simpler; HAR's skipped bodies must not count |
| Raw text and offset | `ParsedEvent { raw, byte_offset }`; today `legacy.rs` cuts `raw` from the whole content by line number; `csv` positions are off at CRLF (`importers/csv.rs:142`) | `raw` must be optional, or an offset and length the host cuts from the file. Text must not cross the wire twice |
| Partial results | A failure mid-file; `evtx` yields per-chunk errors and goes on | Define it: a record error counts as skipped and goes on; a failure after events were sent either keeps them with a problem or aborts the source (`abort_source` exists). Decide one for all |
| Progress (ER-18) | Reading goes through the host's input | Bytes consumed gives progress with no help from the importer. `.etl` and `.blg` give only a growing count |
| Cancel | Long parse loops | `send` and `read` fail on cancel, so importers need no polling; `ProcessTrace` returns false from the callback (`etl.rs:39-42`) |

## The plugin wire

Today: the plugin asks with `ReadFile` (at most 1 MiB, `protocol.rs:37`). The host answers **on the calling thread**
(ADR 0017 §4; `debrief-plugin-host/src/process.rs:349-370`). It can already lend by range (`lent.ranges`, `:362`), but the import path
lends a `Vec` (`remote.rs:268-277`).

| Option | Bounded | Parsers served | Sandbox | Cost per read | Effort |
| --- | --- | --- | --- | --- | --- |
| **A. Host lends ranges (package 3.4)** | Yes | All `Read + Seek`; not `Deref<[u8]>`, not OS paths | Unchanged: no path, no rights | A round trip per read; 1 MiB pieces make sequential reads cheap | In the plan |
| B. Plugin reads by path | Yes | All, plus `.etl`, `relog`, mmap | Needs `OpenedFilePaths` and an `FsAccess` grant; not for third-party packages by default | None | Small; exists for Windows |
| C. Host passes an open handle | Yes | All, plus mmap; not tools that want a path | Keeps "no path"; handles opened before lockdown are usually unaffected [judgement] | None | New platform mechanism per OS; not needed yet |

**Recommendation: A by default; B only for importers that declare `Path`, first-party only (`.etl`, `.blg`); C only if the 2.6b
benchmark shows round trips dominate.** The trait hides this: an importer sees `Input`.

**A risk for 3.4.** The pump answers `ReadFile` on the thread that will now also hand items to the batcher. If a segment flush
blocks that thread, the plugin's read waits on `READ_TIMEOUT` 60 s (`host_file.rs:10`) and the import fails with "cannot read the
file". With a parser blocked in `send` this does not happen; with read-ahead on another thread it can. **Answer reads off the
batcher's thread**, and treat that 60 s under ER-19.

## Parallelism and cost (ER-20)

- **Cheaper first:** dropping `to_vec`, the host's `read_to_end` and the `.etl` temp copy removes work outright. A typed HAR
  `Entry` skips bodies. The framer avoids byte-at-a-time JSON.
- **More threads, later:** `evtx`'s `multithreading` feature keeps at most `2 * num_threads` chunks in flight (`evtx_parser.rs:757`),
  so memory stays bounded. It uses rayon's pool, so it needs sizing through `paddock_proc::threads`. Measure after streaming lands.
- **Parallel files stay as they are** (ADR 0051). Spooling is serial per member.
- Not measured here: every speed claim in this note is from reading source.

## Work packages

Sizes, not estimates (firm numbers come with the ADR). Already planned: **2.6b** `HostFile: Read + Seek`, **PH4** evtx, **3.5** gz,
zip, CSV, JSON arrays, **3.4** host side. The rest appear not to be covered:

| WP | Scope | Size |
| --- | --- | --- |
| S1 | `Input` with declared need, record budget, spool to a capped temp file; a failing test first: an unterminated CSV quote and an unclosed XML text node fail with a message and stay under the budget | M, inside the trait work |
| S2 | Line importers in `debrief-plugin-{formats,syslog,web,windows,registry,otel}` on a line loop; PerfMon and Procmon CSV; `.nfo`, log4j on `from_reader` | M |
| S3 | HAR visitor, including a test with a large body and a `data:` URL | S |
| S4 | Minidump summary through `source::load`; land the handle guard | S |
| S5 | `.etl`: path grant, no temp copy, NetTrace read through S7. `.blg`: stream the temp CSV. Needs the Windows runner | S |
| S6 | Capped `pdb::Source`; `fuzz_pdb` | S |
| S7 | `.nettrace` by block; Chrome trace framer; UMDH, folded and perf by line; `ProfileImporter` takes `Input` | M |
| S8 | `paddock-plugin-test`'s "memory does not grow with input" over every record importer, and a truncating and failing fake `Read` | S |
| Later | `evtx` file-order setting if measured; PML importer; `pdb2` swap | - |

## Risks

- **Temp spool** writes user content to disk. It must use a private folder, cap size, delete on every exit path, and leave nothing
  on a crash that a later start does not sweep.
- **Regressions in raw text and line numbers** for importers that derive them from the whole content; `/samples` differential tests
  (plan 3.5) must cover them.
- **`evtx` is moving fast** (0.12 to 0.15 in the registry here); PH4's memory test should pin behaviour, not version.
- **A slower `from_reader`** for heap snapshots could make things worse for no real saving: measure first.

## Unknown

- `oldest_chunk`'s real cost over `ReadFile`; the round-trip time comes from the 2.6b benchmark.
- Worst-case `evtx` output per chunk.
- Whether anyone has already reported the handle bug to rust-minidump.
- Whether a `.nettrace` V6 walk is fully sequential.
- How `debrief/stream-text` shapes its trait: it had no commits past `b4bf8cb6d` at the time of writing, and its working tree was
  not read beyond its file list.
- `paddock_sandbox`'s effect on handles passed after lockdown, on each OS (option C only).
