# DuckDB 2.0, DuckDB extensions and very large imports in Debrief

Date: 2026-10-27. Scope: Debrief's store (`apps/debrief/crates/debrief-store-duck`, `debrief-store-worker`,
`debrief-store-client`), its importers (`debrief-core::ingest`) and the IPC watcher (`debrief-ipc`, `paddock-proc`), on
branch `monorepo/apps` at 355fd94ae. Read with ADR 0014 (database-first engine), ADR 0031 (performance settings), ADR 0051
(streaming import) and ADR 0052 (stage pipelines).

The user asked four questions. Here they are with short answers. Sections 2 to 5 give the evidence, section 6 the plan.

1. **Should Debrief move to DuckDB 2.0?** Yes, but after the first patch release (2.0.1, planned for November). The change is
   smaller for us than for most users, because events live in Parquet files that arrow-rs writes. Only the small catalog
   database uses DuckDB's own storage format. Three things break: the build plumbing (the prebuilt library's
   download names changed), the SQL console's syntax-tree guard (the tree's JSON shape changed), and a database file
   written by 2.0, which DuckDB 1.x cannot open. We can avoid the last one with one setting at open time. We measured
   2.0 on our own segments: text search (`ILIKE`) uses about 2.2x less CPU, most other queries use 5 to 40% less CPU,
   and peak memory is lower.
2. **Which extensions should Debrief bundle?** None today. Our own code already covers what the candidates do
   (Tantivy, Rust decoders, archive reading, the plugins), or the candidates are not built for every platform.
   Bundling is technically clean (section 3.1) if a later feature needs it.
3. **Should we write our own DuckDB extension?** Not a loadable one. It would not speed up imports: our parallel Parquet
   writers are already about 20x faster than any DuckDB write path we measured. It would also move parsers into the
   process that holds the database, against ADR 0017. Two narrower uses are worth having: DuckDB's own readers for
   the formats our importers still read whole (CSV, JSON arrays), and possibly an in-process table function for
   quick looks at files.
4. **How should Debrief handle very large imports, such as the user's 7.6 GB folder?** The pipeline is fast:
   56 million events (8.07 GB) were imported in 36 to 56 s, about 1.3 to 1.55 million events per second. But **on macOS it
   collides with the store worker's 2 GiB memory cap**. A run with 12 parse threads was stopped at 30 s. The default
   run ended at 2,045 MB, 3 MB under the cap. The cause is mostly how memory is measured: the watcher compares the
   resident size with the cap, and the resident size overstates the worker's real memory (its physical footprint) by up
   to 1.4 GB (1,965 MB resident against 535 MB footprint at the end of the import). The real footprint while parsing is
   about 0.9 to 1.05 GB, above ADR 0014's 768 MB import budget. Compressed files are expanded in memory, and the text
   importer uses twice the CPU per byte of the JSON importer.

## 1. How Debrief uses DuckDB today (facts from the code)

- **Events are not in DuckDB tables.** Parse threads write zstd Parquet *segments* with arrow-rs/`parquet` 58 (131,072
  rows, row groups of 65,536, zstd level 1). DuckDB holds a small catalog (`data.duckdb`: segments, sources,
  histograms, field catalogs, analysis caches, the names catalogue). It reads the events through the views `events` and
  `events_wide`, which are `read_parquet([...], union_by_name = true)` (ADR 0014 R2b). This choice was measured: a native
  table with the Arrow appender reached 36 to 60k events/s, Parquet segments 0.97 to 1.7M events/s with 8 writers.
- **Linking**: `duckdb = "=1.10506.1"` (DuckDB 1.5.6), `default-features = false`, plus `appender-arrow`. It links the
  official prebuilt `libduckdb` (never `bundled`), fetched and checked against a SHA-256 by
  `crates/debrief-store-duck/scripts/fetch-libduckdb.sh`, `app/scripts/libduckdb.mjs` and `ci-image/Dockerfile`.
  `build-sidecars.mjs` re-signs the dylib ad hoc (rule 23).
- **Settings at open** (`lib.rs::open_db`): `memory_limit` 192 MB (128 MB in Low memory), `threads` 4,
  `temp_directory` per store, autoload off, `checkpoint_threshold` 1 GB, `allocator_flush_threshold` 32 MiB,
  `allocator_background_threads`, optional `max_temp_directory_size`, and `parquet_metadata_cache`. The SQL console has
  its own locked, sandboxed connection (ADR 0030: no file, network or extension access).
- **Memory cap**: the host watches the worker and stops it above 2 GiB (`DEFAULT_MEMORY_CAP`). It reads the *resident
  size* (`debrief_ipc::process`, `paddock-proc`). On Linux it reads PSS (rule 26). Budgets (ADR 0014/0031): 768 MB
  importing, 300 MB querying.
- **Reading files**: 8 MiB buffered reads; uncompressed text and JSON-lines files are split into ~64 MB chunks for the
  parse pool, which has `min(cores - 1, 8)` threads. **Gzip and zip files are expanded in memory** (`ingest/source.rs`:
  "Archives are expanded in memory", up to `MAX_ENTRY_BYTES` 1 GiB per entry and `MAX_ARCHIVE_BYTES` 4 GiB per archive).
  The "legacy" importers (CSV, plugin text and binary importers, including EVTX in the Windows plugin) receive the
  whole file.

## 2. DuckDB 2.0: what changes for us

DuckDB 2.0.0 "Cinnamon Teal" was released on 2026-10-21. It is an LTS release, supported until at least October 2027.
Support for 1.5 was extended to March 2027, so there is no hurry. DuckDB itself advises mission-critical users to wait
for the first patch release, planned for November
([announcement](https://duckdb.org/2026/10/21/announcing-duckdb-200),
[release calendar](https://duckdb.org/release_calendar.html#lts-releases)).

### 2.1 Rust crate and linking

- **duckdb-rs `1.20000.0`** (crates.io, 2026-10-22) and `libduckdb-sys =1.20000.0` support 2.0. The version follows the
  same scheme as `1.10506.x` = 1.5.6. Its dependency is **`arrow ^60`** (58 today), so `arrow-*` and `parquet` move to 60
  together. Only three crates use Arrow (`debrief-batch`, `debrief-store-client`, `debrief-store-duck`, as one lock
  entry: arrow-array 58.4.0), because ADR 0017 keeps Arrow off the plugin boundary, so the bump stays local. Its MSRV
  is 1.88, below our 1.95.
- **Default features changed**: `libduckdb-sys` now has `default = ["capi-v1", "vcpkg", "pkg-config", "download-lib"]`,
  which downloads a library at build time. We use `default-features = false`, which must stay so the pinned,
  checksummed library is the only one. New features: `capi-v2` and `loadable-extension-v2` (the unstable C API v2).
  `DUCKDB_STATIC=1` links `duckdb_static` instead of the dylib (see the static option below).
- duckdb-rs 2.0 frees an Arrow scan's batch when its registration is dropped (`src/vtab/arrow/mod.rs`). R2b rejected
  "Arrow-scan inserts" because they leaked every batch. That reason is gone, but the measured speed (21 to 25k
  events/s) still rules them out.
- **The prebuilt assets were renamed.** v1.5.6 shipped `libduckdb-<platform>.zip`. v2.0.0 ships
  `duckdb-shared-libs-<platform>.tar.gz` (tar, not zip), `duckdb-static-libs-<platform>.tar.gz`, separate `osx-arm64` and
  `osx-amd64` builds next to `osx-universal`, and musl builds. The old names are gone; `libduckdb-src.zip` is now only
  182 KB, because amalgamation builds are deprecated. So `fetch-libduckdb.sh` and its parser in `libduckdb.mjs`, which
  matches `ASSET=(libduckdb-...zip)`, `ci-image/Dockerfile` and the workflows that cache the library all break. Release API digests:

  | Asset (v2.0.0) | SHA-256 |
  |---|---|
  | duckdb-shared-libs-osx-universal.tar.gz | `14cd63180614cfd961f7959146ecd9087527ff20b67311efdc3fa1c29c18204d` |
  | duckdb-shared-libs-osx-arm64.tar.gz | `c35d60080b31ba5d11a3ce505a6831be784f003b4c575d32a4c2badb3b41cdf0` |
  | duckdb-shared-libs-osx-amd64.tar.gz | `98fef9fcad856555ca0403fe4d9d6680acfd26c67e8c6f004d61a248d6bc0eff` |
  | duckdb-shared-libs-linux-amd64.tar.gz | `993ad89e7dfca66cd6da994c2cad090c2fc160ddf9f550498a098ab4bb13cfb1` |
  | duckdb-shared-libs-linux-arm64.tar.gz | `c60eecf91143ca6bbac55c4f4d40dc22520488b0ac257c86e0fe9a2b451cf2b7` |
  | duckdb-shared-libs-windows-amd64.tar.gz | `2b83169128239de5f45fe21de80f99bf436e53aa4bb24020dd951a1ba8d4b95f` |
  | duckdb-shared-libs-windows-arm64.tar.gz | `159ce60eb587112b8c5f290b8beea56af5f13be4d13679de127ad9bfaafb7447` |

  (Source: `https://api.github.com/repos/duckdb/duckdb/releases/tags/v2.0.0`; the downloaded osx-universal archive matched.)
- **The macOS library** (checked locally): the universal `libduckdb.dylib` grew from 117 MB to 134 MB. It is still signed with
  DuckDB's Developer ID (TeamIdentifier `7NCTQWA3HA`, hardened-runtime flag), exactly like 1.5.6, so the ad-hoc
  re-sign in `build-sidecars.mjs` stays as it is. The archive now also contains `duckdb_v2.h`, `duckdb_extension_v2.h` and
  `duckdb_static_extension.h`. `parquet`, `json` and `icu` are still statically linked: the 2.0 CLI lists them as
  `STATICALLY_LINKED`, and the dylib contains their functions (`read_parquet`, `json_extract`, `icu_sort_key`). The
  announcement says "ICU removed" from the core build, but the `icu` extension keeps time zones in the official
  binaries, so nothing is needed for `epoch_ms`/`TIMESTAMP` use.
- **Options this opens**:
  - Ship the per-architecture macOS library instead of the universal one. The two halves of a universal app need both
    anyway, but per-architecture DMGs would save about 65 MB each.
  - Link statically (`DUCKDB_STATIC=1` with `duckdb-static-libs-*`). This needs no dylib, no re-sign and no
    `DEBRIEF_LIB_DIR`. It costs a larger worker binary and a link step with libc++, and it needs an experiment on the
    Windows MSVC and mingw builds. Worth a spike (WP-D1b), not a requirement.
- **Platforms**: macOS x86_64 is being phased out. Core extensions still build for 2.0, and **2.1 drops it**
  ([announcement](https://duckdb.org/2026/10/21/announcing-duckdb-200)). Debrief's Intel-Mac support then rests on
  staying on 2.0.x LTS, which is supported until October 2027.

### 2.2 Storage format and existing user databases

Facts ([migration guide](https://duckdb.org/docs/2.0/operations_manual/migration.html)), all reproduced locally with the 1.5.6
and 2.0.0 CLIs:

- 2.0 opens, reads and writes 1.5.6 files and **keeps their storage version**. A 1.5.6 catalog that 2.0 had written to
  and checkpointed still opened in 1.5.6.
- A file *created* by 2.0 uses storage version 69. 1.5.6 refuses it: `IO Error: Trying to read a database file with
  version number 69, but we can only read versions between 64 and 68.`
- `storage_compatibility_version` set **at open** (CLI `-storage-version v1.5.6`, `Config::with(...)` in Rust) makes a
  new file 1.5-readable, and so does `ATTACH ... (STORAGE_VERSION 'v1.5.0')`. **`SET storage_compatibility_version`
  after the database is open does not help the main file**: it was already created as version 69.

What this means for Debrief:

- Only `data.duckdb` (the catalog, 176 MB for 56M events in our run) is affected. Parquet segments are written by arrow-rs
  and are unaffected. Tantivy's index is unaffected.
- Users who move to the new Debrief keep working: their catalogs stay in the old format and remain readable by older
  Debrief versions until a migration rewrites them.
- The real risk is **shared projects** (`.debriefz`, ADR 0041). A project created by a 2.0 Debrief would fail to open in an
  older one with DuckDB's raw IO error. Our `FORMAT_VERSION` check never runs in that case, because DuckDB fails before
  `meta` is read.
- Plan:
  1. Open with `storage_compatibility_version = 'v1.5.0'` while 2.0 ships, so every catalog stays readable by 1.x
     Debrief. We lose nothing measurable: the catalog is small, and DICT_FSST and VARIANT storage are not needed there.
  2. Map DuckDB's "version number N" open error to `StoreError::Invalid` with the existing "saved by a newer Debrief"
     message (fail fast with a next step).
  3. Lift the pin later in a release that bumps `FORMAT_VERSION`. The migration then follows our versioned-state rules:
     build a new catalog with `COPY FROM DATABASE` (already used by `project.rs`), then swap. It takes milliseconds to
     seconds for this size and is safe to cancel because the old file stays until the swap.

### 2.3 SQL and API changes, checked against our code

From the [migration guide](https://duckdb.org/docs/2.0/operations_manual/migration.html), with a grep of
`debrief-store-duck`:

| Change | Our code |
|---|---|
| Single-arrow lambdas off by default (`x -> ...`; removed in 2.1) | Already Python-style: `sql.rs:122` `list_filter(..., lambda x: ...)`. |
| `~`/`!~` now partial regex match; `!` binds tighter; `X'..'`/`B'..'` are BLOB/BITSTRING | Not used in generated SQL; regex filters run in Rust or `regexp_matches`. Affects **SQL console users' own queries**: say so in the release notes. |
| Integer `//` and `%` by zero raise an error | `anomalies.rs:174` divides by `layout.bucket.max(1)`: safe. Console users can now see the error. |
| Untyped `NULL` keeps type `NULL` | Every NULL we generate is typed (`NULL::VARCHAR`, `CAST(NULL AS ...)`). |
| `MERGE` errors on several matches; `preserve_identifier_case` is now VARCHAR; removed profiling settings | Not used. |
| PEG parser by default; **`json_serialize_sql` output changed** | See below: this matters for the console guard. |
| `SHOW <name>` prints a setting when no table of that name exists | The console allows `SHOW`. Decide whether settings (paths of `temp_directory`, limits) may be shown, or refuse `SHOW` with a setting name. |

**The console guard** (`console/guard.rs`) walks the tree that `json_serialize_sql` returns. Comparing the same query
in 1.5.6 and 2.0.0 shows these differences:

- table references gain `qualified_name.path` and `query_location_length`;
- function arguments move from `children[]` to `arguments[].expression`;
- `~` serializes as `regexp_matches` instead of `regexp_full_match`;
- a function's own `query_location` can be `18446744073709551615` (u64::MAX).

The guard reads only `type`, `function.function_name` and `table_name`, which are still present, and walks every node
generically, so it still works. But its error position must be clamped (it can be u64::MAX), and its unit tests, which
build 1.5-shaped trees by hand, should also run against trees from the real 2.0 parser. The sandbox
(`enable_external_access = false`, `lock_configuration`) still stands behind it.

**C API v2** (`duckdb_v2.h`) exposes file systems, table, aggregate, cast and copy functions, and streaming results to
extensions. It is explicitly **not yet stable** ([C API overview](https://duckdb.org/docs/2.0/clients/c/overview.html),
[2.0 highlights](https://duckdb.org/2026/08/17/duckdb-20-highlights.html)). We use only the stable v1 surface through
duckdb-rs; nothing to do.

### 2.4 Performance changes that matter to us

From the release material:

- **Asynchronous I/O for Parquet and CSV**: a separate pool of I/O threads with read-ahead governed by the memory limit. On
  local SSD, cold reads were 1.5x faster in DuckDB's TPC-H Q6 test; most of the gain is on S3
  ([async I/O post](https://duckdb.org/2026/07/31/asynchronous-io.html)). **`async_threads` defaults to 4 x cores (56 on
  this 14-core Mac) and does not follow `threads`** (checked: `SET threads = 4` leaves `async_threads = 56`). The
  post also notes that RSS can exceed `memory_limit`, because the allocator keeps freed pages for about a second.
  We must set `async_threads` explicitly (start at `duck_threads`) and re-measure the 300 MB query budget.
- **Aggregates spill to disk** (#24499 in the [highlights](https://duckdb.org/2026/08/17/duckdb-20-highlights.html)).
  ADR 0014's notes record several work-arounds for operators that ran out of memory at 183 MB (summary distinct counts, timeline lanes).
  Some may become unnecessary; re-test before removing any.
- More row-group pruning (zone maps and Bloom filters for `IN` and some functions), and lazy column metadata. Both
  help the id lookups and the source filters on many segments.

**Measured** (synthetic corpus of section 5, 56,152,132 events in 2.6 GB of segments; the 1.5.6 and 2.0.0 CLIs read the
store's own catalog and views read-only, `memory_limit = 192MB`, `threads = 4`, `parquet_metadata_cache = true`, the
same SQL as the store's operations. M4 Pro, 14 cores, shared with other agents: load averages of 20 to 90, so wall
times vary by up to 2x between rounds. User CPU is the steadier figure). Two runs per version, interleaved:

| Query | 1.5.6 wall (s) | 2.0.0 wall (s) | 1.5.6 CPU (s) | 2.0.0 CPU (s) |
|---|---|---|---|---|
| count | 0.12-0.14 | 0.08-0.21 | 0.40-0.43 | 0.22-0.28 |
| level facet | 0.15-0.16 | 0.11-0.34 | 0.53-0.55 | 0.39-0.46 |
| minute x level histogram | 0.34-0.36 | 0.29-1.03 | 1.25-1.30 | 1.08-1.23 |
| first page in time order | 0.47-0.60 | 0.43-1.03 | 1.74-1.85 | 1.65-1.97 |
| facet on an `extra` key | 1.03-1.29 | 0.92-1.54 | 3.96-4.19 | 3.52-4.08 |
| `message ILIKE '%...%'` | 3.52-4.16 | 1.56-2.55 | 13.5-15.5 | **6.2-7.1** |
| `regexp_matches` | 1.53-3.20 | 1.48-2.23 | 5.9-6.5 | 5.9-6.7 |
| row by id | 0.03-0.08 | 0.04-0.05 | 0.03 | 0.05 |
| page at offset 10M | 1.69-3.76 | 1.59-1.85 | 4.2-5.6 | 4.2-4.7 |
| **peak RSS of the process** | 551 / 550 MB | **418 / 395 MB** | | |

So 2.0 never needs more CPU, needs about half for substring search, and its peak is about 150 MB lower than 1.5.6's
with the same limit. At 56M events, several operations exceed the 0.5 s interactive budget in both versions: an
`extra` facet, `ILIKE`, regex and deep pages. Debrief already answers those through promoted columns, Tantivy, the
`ord` table and cached analyses rather than raw view scans, so this table compares engines; it does not measure the app.

### 2.5 Migration risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Behaviour change in generated SQL (parser, NULL types, division) | Low (grep above) | Full store test suite plus the differential tests against the in-memory oracle on `/samples`, on all three OSes. |
| Console users' saved queries change meaning (`~`) | Medium | Release note. Optionally refuse `~` with a hint to use `regexp_full_match`. |
| Worker memory budgets move (async I/O threads, different allocator behaviour) | Medium | Set `async_threads`; run the budget tests (`memory.rs`, worker tests) and the nightly bench before merging. |
| Shared project unreadable in older Debrief | High without the pin | `storage_compatibility_version` at open, plus the mapped error. |
| A 2.0.0 regression | Medium (x.0 release) | Wait for 2.0.1; keep the 1.5.6 pin on a branch until the benchmarks pass. |
| Build breakage in CI image and workflows | Certain | WP-D1 changes every place that names the asset (listed in section 6). |
| Intel Mac support after 2.1 | Certain (later) | Stay on 2.0.x LTS until Intel Macs are dropped by decision. |

## 3. Extensions

### 3.1 Facts about signing and loading in a sandboxed app

- Extensions are checked on every load against signatures. Core extensions are signed with the core key, community
  extensions with the community key, and unsigned ones need `allow_unsigned_extensions` at start-up.
  `allow_community_extensions = false` restricts loading to core-signed extensions, and `lock_configuration` freezes
  this ([securing extensions](https://duckdb.org/docs/2.0/operations_manual/securing_duckdb/securing_extensions)).
- `LOAD '/path/x.duckdb_extension'` loads a local file directly, so **bundling works without a download**: pin the URL
  `http://extensions.duckdb.org/v2.0.0/<platform>/<name>.duckdb_extension.gz` (community:
  `community-extensions.duckdb.org`), check its SHA-256 at build time, decompress it, and ship it next to the library
  ([advanced installation](https://duckdb.org/docs/2.0/extensions/advanced_installation_methods)). The DuckDB signature
  is still verified at load, so `allow_unsigned_extensions` stays off.
- C++ extensions are built for one exact DuckDB version, so each DuckDB bump means a new download. Extensions on the C
  API (most Rust ones) survive minor versions.
- **macOS**: the osx_arm64 v2.0.0 binaries of `fts`, `encodings` and the community `read_lines` are linker-signed
  ad hoc with no Team ID (checked with `codesign -dv`). This fits rule 23's ad-hoc bundle as shipped, so do not re-sign
  them: re-signing rewrites the Mach-O, and DuckDB's own signature would then most likely stop matching. *Not
  verified*: actually loading a downloaded extension (the session's permission rules did not allow running
  third-party binaries), and Gatekeeper behaviour for a dlopen'd ad-hoc extension inside the notarised-later app.
- Sizes are not small: `encodings` is **123 MB** for osx_arm64, `fts` 12 MB and `read_lines` 10 MB.
- 2.0 adds trusted **extension repositories** (`CREATE EXTENSION REPOSITORY`, pinned RSA keys, local paths allowed). A
  Paddock repository could serve our own signed extensions later. The feature is marked work in progress
  ([extension repositories](https://duckdb.org/docs/2.0/extensions/extension_repositories.html)).

### 3.2 Candidates

Lists reviewed: [core extensions](https://duckdb.org/docs/2.0/core_extensions/overview) (29) and the
[community list](https://duckdb.org/community_extensions/list_of_extensions) (about 290). Licences and platforms are taken
from each extension's `description.yml` in
[duckdb/community-extensions](https://github.com/duckdb/community-extensions/tree/main/extensions). Spatial was skipped,
as asked.

| Extension | What it would do for us | Tier / licence | Platforms | Verdict |
|---|---|---|---|---|
| `parquet`, `json`, `icu` (core) | Segment reads, JSON, time zones | Primary, MIT | All, built in | Already in libduckdb. |
| `fts` (core) | BM25 search in SQL | Secondary, MIT | All | **No.** It indexes DuckDB tables, not our Parquet views. It is not incremental unless triggers are used. Its default `ignore` regex `(\\.\|[^a-z])+` drops digits and punctuation, so `GRAPHIC.BUILDER` and error codes are lost ([fts docs](https://duckdb.org/docs/2.0/core_extensions/full_text_search)). Tantivy does better (ADR 0014). |
| `encodings` (core) | Reads CSVs in 1000+ encodings | Secondary, MIT | All | **No.** Our Rust decoders (ADR 0040) cover the encodings we sniff; 123 MB per platform. |
| `excel`, `sqlite` (core) | Spreadsheet/SQLite sources | Secondary, MIT | All | Later, only if users bring such files: a reader plugin is the ADR 0017 way. |
| `read_lines` (teaguesterling) | Lines with numbers and byte offsets, line ranges | MIT, C++ | All incl. Windows | **No.** Same as our `ReaderChunks`; would bypass profiles and limits. |
| `zipfs`, `cozip`, `compression_fs` | Read inside zip, LZ4/XZ/bzip2 | MIT | All | **No.** Our archive layer must stream instead (section 5, L3). DuckDB reading archives would not help the Rust parsers. |
| `duck_hunt` | Parses 110+ CI and test log formats | Apache-2.0 | **No Windows** | **No** for bundling (Windows excluded). Its format list is a good source for parser packs (ADR 0026). |
| `otlp`, `zeek`, `splunk` | OTLP, Zeek and Splunk logs as tables | MIT | `otlp` no mingw/musl | **No.** We have an OTel plugin. New formats are Rust plugins (ADR 0017). |
| `datasketches`, `hashfuncs`, `lsh`, `rapidfuzz`, `marisa` | HLL/KLL sketches, fast hashes, fuzzy match | MIT/Apache | All (lsh no musl) | **Not now.** We compute sketches in Rust while parsing (`stats.rs` HLL) and fuzzy names in Rust (ADR 0053). Revisit `datasketches` only if quantiles are wanted in SQL. |
| `query_condition_cache`, `cache_prewarm` | Cache repeated filters, prewarm blocks | MIT | **No Windows** | **No** (Windows excluded). |
| `tantivy` (alitrack) | BM25 through tantivy in SQL | MIT, Rust | no mingw/musl | **No.** We already embed Tantivy with our tokenizer and index lifecycle. |
| `tick`, `timestral` | Time-interval syntax, forecasting | MIT | `timestral` no mingw/musl | **No.** Our query language and anomaly detection are engine code (AGENTS.md "where logic lives"). |
| `storage_compat` | Read newer-format files in older DuckDB | MIT | **No Windows** | **No**. The pin in 2.2 solves this. |

No community extension reads EVTX. The building blocks are libevtx (LGPL) or Rust's `evtx` crate, which our Windows plugin
already uses for this job.

**Conclusion: bundle nothing now.** Keep the console's "no extension loading" sandbox. If a later feature needs an
extension, use the pinned and verified `LOAD` path from 3.1, loaded only by the store's main connection and never by the console.

## 4. Our own DuckDB extension

Three shapes were considered:

| Shape | How | Signing and distribution | Fit |
|---|---|---|---|
| A. Loadable extension (`.duckdb_extension`) | duckdb-rs `loadable-extension` (C API v1, stable), or v2 (unstable); helpers such as [quack-rs](https://docs.rs/crate/quack-rs/latest) | Ours are unsigned for DuckDB, so `allow_unsigned_extensions = true` at start-up, or a 2.0 extension repository with our key | Only useful if *other* DuckDB users should read our formats. Not Debrief's need. |
| B. In-process table function on the worker's connection | duckdb-rs `vtab` (`register_table_function`, with `set_max_threads` for parallel scans) | None: it is our code in our binary | Possible today, no signing questions. |
| C. Status quo | Rust importers write Parquet segments; DuckDB only reads | n/a | Measured fastest. |

Arguments, with numbers:

- **Write speed favours C.** Every DuckDB write path measured in R2b was slower than our parallel segment writers:
  native table with the Arrow appender 36 to 60k events/s, Arrow-scan insert 21 to 25k, parallel appenders 61 to 75k,
  against Parquet segments at 0.97 to 1.7M events/s. A table function only *produces* rows. DuckDB would then still
  have to write them (a single native writer, 9x larger) or `COPY ... TO parquet`. That is the same Parquet writing we
  already do in parallel, but under DuckDB's memory limit.
- **DuckDB's own readers are fast but need memory and do less work.** On the corpus, DuckDB split the 4.4 GB of
  log4net text into lines with a regex and wrote zstd Parquet in 7.7 s (56 CPU-s, 1.5.6, 8 threads), and the 2.3 GB of
  CLEF in 1.5 s. Our importer took 26 s and 168 CPU-s for the text part, and 8.9 s and 42 CPU-s for CLEF. But DuckDB did
  not join stack traces to their events (41.0M rows against our 37.2M events), parse timestamps, compute levels,
  templates or ids, promote fields or record problems. And it **ran out of memory at `memory_limit = 192MB`** in both
  versions, and on the CLEF part at 512 MB in 2.0. It needed about 1 GB (peak RSS 0.7 to 1.4 GB). Our budget is the
  whole worker at 768 MB.
- **Isolation and robustness favour C.** A table function runs inside DuckDB's threads. A Rust panic crossing the C
  boundary aborts the worker, so every callback would need `guard::catch`. Cancel and progress would have to go
  through DuckDB's interrupt instead of our job events (rules 10 to 12). Plugin importers such as EVTX must stay out of
  process (ADR 0017), so a table function could only proxy them over IPC, which gains nothing.
- **What B is good for** (small, optional):
  - A **quick look** at a file without importing it: `SELECT * FROM debrief_read('x.log') LIMIT 1000` for the parser
    editor or a "peek" preview, reusing the profile parsers.
  - **Quick-fix evaluation and profile re-derivation** directly from `raw`. ADR 0014 §5/§6 wants "re-derive fields
    from the database without the files". A `vscalar`/`vtab` over `raw` lets one `COPY (SELECT ...) TO` write the new
    segment generation inside DuckDB. Today R2c re-reads the files instead.
- **Use DuckDB's readers where our importers are weakest.** The "legacy" CSV and JSON-array importers read whole files.
  `read_csv` (parallel, sniffing) and `read_json` (arrays streamed) are mature. A CSV importer can run
  `COPY (SELECT ... FROM read_csv(?)) TO 'segment.parquet'` with our schema, inside the worker, under a raised per-job
  memory limit. This is a different kind of reuse: DuckDB as a library, not as an extension host.

**Recommendation:** no loadable extension. Consider B for preview and re-derivation once ADR 0014's re-derive feature is
scheduled (effort M, about 3 to 5 days with tests). Consider `read_csv`/`read_json` for the whole-file legacy formats as
part of L6 below.

## 5. Very large data: measurements and recommendations

### 5.1 Method

- **Synthetic data only** (never user logs): `gen_large.py` (deterministic seeds; kept with the scripts in
  `/Volumes/d7sst7/dev-targets/research-data/scripts/`) writes a 7.3 GB folder (8.07 GB uncompressed) in the shapes of
  `/samples`: 8 log4net-pattern text files of 560 MB with stack traces (4.4 GB), 6 CLEF files of 390 MB (2.3 GB),
  600 Serilog-text files of 0.8 MB, and 4 gzip text files of 100 MB uncompressed. Fields are high-cardinality, like real
  services (order ids, customer ids, client IPs, a unique capture id on 1 event in 16).
- **The real path**: the release `debrief` CLI (ADR 0044) with the release `debrief-store-worker` and libduckdb 1.5.6, as the app runs
  them, including the host's 2 GiB watcher. Per run: wall time, the CLI's `elapsedMs`, the worker's RSS every 0.5 s (`ps`)
  and CPU time, and in one run the physical footprint (`top -stats mem`) and `vmmap -summary`.
- Machine: Mac mini M4 Pro, 14 cores, 64 GB, APFS SSD. Shared with other agents (load averages of 20 to 90), so times are
  ±30%, and CPU seconds are the steadier figure. All heavy runs went through the machine's `heavy.sh` slots, one at a time.

### 5.2 Results

| Run | Wall | Events/s | Worker CPU | Worker peak RSS | Result |
|---|---|---|---|---|---|
| Default (8 parse threads), run 1 | 43.1 s (`elapsedMs` 42.4 s) | 1.33M | 248 s | 2,027 MB | ok |
| Default, run 2 (load 75) | 56.4 s | 1.0M | 271 s | **2,045 MB** | ok, 3 MB under the cap |
| Default, run 3 (load 20) | `elapsedMs` 36.2 s | **1.55M** (223 MB/s) | – | 1,965 MB | ok |
| Low memory preset | 66.1 s | 0.85M | 238 s | 1,556 MB | ok |
| 4 parse threads | 59.6 s | 0.94M | 231 s | 1,646 MB | ok |
| 12 parse threads | – | – | – | 2,047 MB | **stopped at 30 s: "used more than its memory cap of 2048 MB"** |
| Text part only (4.4 GB, 37.2M events) | 26.1 s | 1.43M | 168 s | 1,450 MB | ok |
| CLEF part only (2.3 GB, 10.3M) | 8.9 s | 1.15M | 42 s | 1,451 MB | ok |
| 600 small files (0.48 GB, 5.4M) | 4.4 s | 1.2M | 20 s | 933 MB | ok |
| 4 gzip files (0.4 GB unpacked, 52 MB packed) | 4.4 s | 0.74M | 12.5 s | 1,019 MB | ok |

Other figures:

- **I/O is not the bottleneck.** Reading the whole folder with `cat` took 9.5 s (800 MB/s, partly from the page cache).
  The import is CPU-bound: about **4.5 µs of CPU per text event and 4.1 µs per CLEF event**. That is 26 MB of text
  per CPU-second, against 55 MB per CPU-second for CLEF.
- **Store size**: 2.6 GB of segments (3.1x smaller than the input) plus a 176 MB catalog. DuckDB's own zstd Parquet of
  the same content (raw plus parsed fields) was 1.5 GB: our segments keep `raw`, `message` and `template` side by side.
- **RSS vs real memory, macOS** (run 3, sampled together):

  | Seconds | 1 | 5 | 10 | 20 | 29 | 30 | 31 | 37 (end) |
  |---|---|---|---|---|---|---|---|---|
  | RSS (what the watcher checks) | 1,050 MB | 1,299 | 1,573 | 1,609 | 1,621 | 1,626 | 1,854 | **1,965** |
  | Physical footprint | 970 MB | 976 | 633-835 | 883 | 856 | 424 | 443 | **535** |

  `vmmap -summary` at 20 s: RSS 1.61 GB, footprint 907 MB (peak 1.2 GB). The largest region is labelled
  "IOAccelerator": 1.4 GB resident, of which only 795 MB is dirty. That label is VM tag 100, which is **mimalloc's
  default `os_tag` on macOS** (`libmimalloc-sys` `options.c`: `{ 100, ..., MI_OPTION(os_tag) }`; SDK
  `vm_statistics.h`: `#define VM_MEMORY_IOACCELERATOR 100`). So it is our Rust heap, and over 600 MB of it is resident but
  clean, freed pages that macOS has not reclaimed. DuckDB's system-malloc zone was 108 MB. At the end of the import the
  footprint *falls* (segments closed, buffers freed) while RSS *rises* by 340 MB.

### 5.3 What this means

1. **On macOS the watcher stops the worker for memory it is not really using.** The cap is compared with RSS, and RSS
   counts freed mimalloc pages that macOS has not yet reclaimed. The default import of this folder ends within 3 MB of
   the cap, and 12 parse threads (a value a user can enter in Settings > Performance) is stopped. The user's
   7.6 GB case is exactly this. `paddock-proc/src/usage.rs` says "everywhere else the resident size is the best figure
   there is". On macOS that is not true: `proc_pid_rusage(RUSAGE_INFO_V4).ri_phys_footprint` is the kernel's own measure
   (Activity Monitor's "Memory"), and it is what jetsam uses. This is the macOS counterpart of rule 26 (PSS on Linux),
   and a bug: per rule 5 it needs a failing regression test first.
2. **The real footprint during parsing is about 0.9 to 1.05 GB, above the 768 MB budget.** With high-cardinality data it
   does not fall with fewer threads as much as the budget assumes: Low memory still had 1.2 GB RSS, and its footprint
   was not sampled. ADR 0014's budget tests use copies of `/samples`, which have low cardinality and small files. They
   need a large-file, high-cardinality corpus (this generator's shape) to guard the budget. The remaining footprint is
   not yet attributed (per-thread chunk buffers of 64 MB, batches in flight, segment writers with 65,536-row groups,
   per-segment stats, the names catalogue thread): profile it with mimalloc's statistics or Instruments' Allocations.
3. **Compressed files cost memory, not time.** Four 100 MB gzip files lifted the worker to about 1 GB in one second,
   because each is expanded whole. The archive limits allow 1 GiB per entry and 4 GiB per archive, with up to 8
   expanding in parallel, inside a 2 GiB worker. A single large `.gz` log would be enough to stop it.
4. **CPU**: the text path costs twice the JSON path per byte. A regex-only line split in DuckDB managed 79 MB/CPU-s, so
   the text importer (line reading, start-of-event detection, timestamp parsing, `raw`/`message` copies) has
   room. Rule 13's "low CPU" goal is about this path.

### 5.4 Recommendations for large imports

| # | Recommendation | Impact | Effort |
|---|---|---|---|
| L1 | **Watch the physical footprint on macOS** (`ri_phys_footprint`) in `paddock-proc`/`debrief-ipc`, as PSS is used on Linux; keep the cap's meaning "memory the OS charges to the process". Test first: a child that frees a large mimalloc heap must not be stopped. Shared crate per rules 42/43. | **High** (stops false kills of real imports) | S |
| L2 | **Stream compressed input**: `.gz` through `flate2::read::GzDecoder` into the existing `ReaderChunks` (a gzip file is one source, cannot be split, one thread); zip members through the `zip` reader per entry. Bounded memory regardless of size; the 1 GiB/4 GiB expansion caps become a decompression-ratio guard instead of a RAM bound. | High (memory) | M |
| L3 | **Hold the import budget with a realistic corpus**: add the large-file, high-cardinality shape to the corpus generator (`xtask`, synthetic) and to the worker's budget test. Attribute the ~1 GB footprint and cut it: smaller in-flight chunks for split files (64 MB chunks x 8 threads is already 512 MB of text in flight), flushing row groups earlier, a bounded names catalogue during import. | High | M |
| L4 | **Profile and speed up the text importer** (samply/Instruments on the text part). Targets from the profile notes in ADR 0014: per-line allocations, `raw` and `message` copies, timestamp parsing per line. Goal: CLEF-level cost per byte. | High (CPU, wall) | M |
| L5 | **Move `fsync` off the parse threads.** `SegmentWriter::close` calls `sync_all`. On macOS Rust's `sync_all` issues `F_FULLFSYNC`, a full drive-cache flush; ADR 0014 measured fsync at 12% of parse-thread samples on Windows. Sync in the listing step or on a background thread, before the segment is registered. | Medium | S |
| L6 | **Stream the legacy importers**: EVTX is a sequence of independent 64 KiB chunks, so it can be parsed chunk by chunk and in parallel in the plugin. CSV through `csv::Reader` or DuckDB `read_csv` (section 4). JSON arrays through a streaming deserializer or `read_json`. | Medium (memory for big EVTX/CSV) | M-L |
| L7 | **Do less up front for very large imports**: offer the time-window import (ADR 0047, `--since/--until`) when a folder is over a size threshold; defer the full-text index and the names catalogue until the import is done or until first use (the grid and SQL `ILIKE` work meanwhile, ADR 0014). | Medium (time to useful) | S-M |
| L8 | **Tune for many cores only after L1-L3**: 12 threads failed on memory, not CPU; with L1-L3 the derivation in ADR 0031 (one thread per 96 MB of budget, max 8) can be revisited. `mmap` for input files is not recommended: reads are sequential at 800 MB/s already, mapped pages count as resident (the same watcher problem), and a truncated file under live tail raises SIGBUS. | Low-Medium | S |
| L9 | **Smaller segments, later**: store `message` as an offset into `raw` for text formats, or recompress closed segments at zstd 3 in compaction (background, off the import path). Our segments are 1.7x the size of DuckDB's own Parquet of the same content. | Low-Medium (disk, cold reads) | M |
| L10 | **Exploratory: `extra` as Parquet VARIANT.** DuckDB 2.0 reads and writes shredded VARIANT in Parquet ([VARIANT](https://duckdb.org/docs/2.0/sql/data_types/variant.html)), and arrow-rs 60 has it behind `parquet/variant_experimental`. Shredding would type frequent keys per row group automatically, replacing the per-segment `p_<key>` promotion (and its "promoted in 84 of 93 segments" gap). | Potentially high for facets | L (experimental) |

**Settings advice for a 7.6 GB import today** (until L1-L3 land): on macOS keep 8 parse threads or fewer, keep the
cap at 2 GB or raise it to 3 GB in Settings > Performance, and do not raise DuckDB's `memory_limit`. DuckDB is not on the
write path, and its 192 MB only matters for queries. Leave `temp_directory` on the store's disk with
`max_temp_directory_size` set when disk space is short (ADR 0031 already exposes both). Appender, Arrow and `COPY` are
not alternatives worth switching to: segments bypass all three by design (R2b table above).

## 6. Recommendations ranked, and the plan

| Rank | Recommendation | Impact | Effort | When |
|---|---|---|---|---|
| 1 | L1: footprint-based memory watcher on macOS | High | S | Now (bug) |
| 2 | L2: stream gzip and zip members | High | M | Now |
| 3 | L3: realistic budget corpus, cut the import footprint to budget | High | M | Now |
| 4 | L5: fsync off the parse threads | Medium | S | Now |
| 5 | L4: text importer CPU | High | M | Next |
| 6 | DuckDB 2.0 migration (WP-D1..D6) | Medium (CPU of searches, lower peak, LTS to 2027, Arrow 60 unified) | M | After 2.0.1 (November 2026) |
| 7 | L7: defer index and catalogue, suggest the time window for huge folders | Medium | S-M | Next |
| 8 | L6: stream EVTX, CSV, JSON arrays (possibly via `read_csv`/`read_json`) | Medium | M-L | Later |
| 9 | Section 4 B: in-process table function for preview and re-derivation | Medium | M | With ADR 0014's re-derive work |
| 10 | L9, L10: smaller segments, VARIANT for `extra` | Low-High | M-L | Spike after 2.0 |
| – | Bundle extensions, own loadable extension | – | – | Not recommended |

### Work packages

Each one is for a single agent in its own worktree (rule 2). Each runs only the affected tests (rule 3) and starts with
its failing test where it fixes a bug (rule 5). Shared code goes to Paddock first (rule 42).

**Large-data track (independent of DuckDB 2.0):**

- **WP-L1 Memory measure on macOS** (Paddock `paddock-proc` + `debrief-ipc` wiring; S). Use `ri_phys_footprint` from
  `proc_pid_rusage` for the watcher and the process lists on macOS. Keep PSS on Linux. On Windows, check whether
  the working set overstates in the same way (private bytes are the candidate). Tests: a child that allocates and frees 1 GB under
  mimalloc is not stopped at a cap below its RSS; the cap still stops real growth. Lesson to `paddock/docs/lessons.md`,
  and propose extending rule 26 to macOS.
- **WP-L2 Streaming archives** (`debrief-core::ingest::source`, `archive`; M). Gzip as a stream into `ReaderChunks`, zip
  entries one at a time. Peak-memory test: a 1.5 GB-uncompressed `.gz` imports under the import budget. Keep the
  expansion-ratio guard.
- **WP-L3 Budget corpus and footprint** (`xtask` corpus option and `debrief-store-duck` memory tests; M). Add the
  high-cardinality, large-file shape, synthetic. Measure the footprint by phase, attribute it, then cut to 768 MB
  (in-flight chunk bytes, row-group flushing, catalogue thread during import). Record the numbers in ADR 0014's notes.
- **WP-L4 Text importer CPU** (`debrief-core::ingest::text`, `importers`; M). Profile on the text part and fix the top
  items. Target ≤ 2.5 µs/event and output identical to before (differential tests on `/samples`).
- **WP-L5 Segment sync** (`debrief-store-duck::segments`, `writer`; S). Move `sync_all` to the listing step or a
  background syncer. A crash test proves that a listed segment is always durable.
- **WP-L7 Big-folder flow** (store + UI; S-M). Above a size threshold, offer the time window first; defer the
  index and catalogue builds until the import ends; show their phases in the Jobs pane (rule 11).

**DuckDB 2.0 track (start when 2.0.1 is out):**

- **WP-D1 Library plumbing** (S-M): `fetch-libduckdb.sh` (new names, `.tar.gz`, new digests, headers),
  `app/scripts/libduckdb.mjs` and its test (the `ASSET=` regex), `build-sidecars.mjs` (file name unchanged:
  `libduckdb.dylib`/`.so`/`duckdb.dll`), `ci-image/Dockerfile` and `verify.sh`, `.gitea/workflows/debrief.yml` and
  `debrief-bench.yml` cache keys, `scripts/ci/macos-verify-bundle.sh`, `scripts/ci/smoke-linux-docker.sh`.
  **WP-D1b (spike)**: a static link (`DUCKDB_STATIC`) on all three OSes; and per-arch macOS libraries.
- **WP-D2 Crates** (S): `duckdb =1.2000x`, `arrow-*`/`parquet` 60 in the workspace dependencies (store, batch,
  store client); keep `default-features = false`; fix compile changes; `cargo deny`.
- **WP-D3 Store open** (S): `storage_compatibility_version = 'v1.5.0'` in `open_db` and in the export path;
  set `async_threads` explicitly from `duck_threads`; map DuckDB's "newer version" open error to our message. Tests:
  a catalog created by the new build opens in DuckDB 1.5.6 (using the pinned 1.5.6 library in a test fixture is
  heavy; instead, assert the storage version DuckDB reports for the created file), and opening a v69 file gives the friendly error.
- **WP-D4 SQL and console** (S): the full store suite and differential tests on 2.0; guard tests fed by the real 2.0
  `json_serialize_sql`; clamp error positions; decide on `SHOW <setting>`; release-note text on console semantics
  (`~`, `!`, `X''`, division by zero, NULL type, lambdas).
- **WP-D5 Budgets and benchmarks** (M, mostly machine time): worker budget tests on macOS, Linux and Windows; the nightly
  10M bench plus the WP-L3 corpus. Compare against 1.5.6 numbers and record them in ADR 0014. Only then merge.
- **WP-D6 Release, then lift the pin** (later, S): ship 2.0 with the pin; in a later minor release, bump
  `FORMAT_VERSION`, rebuild the catalog with `COPY FROM DATABASE`, swap, and drop the pin, if a 2.0 storage feature is wanted.
- **WP-D7 Decision** (user): Intel Macs after DuckDB 2.1. Stay on 2.0.x LTS (to October 2027) or drop x86_64 macOS.

## Sources

- DuckDB 2.0.0 announcement, 2026-10-21: https://duckdb.org/2026/10/21/announcing-duckdb-200
- Migration guide 2.0: https://duckdb.org/docs/2.0/operations_manual/migration.html
- DuckDB 2.0 highlights (preview), 2026-08-17: https://duckdb.org/2026/08/17/duckdb-20-highlights.html
- Gems of DuckDB 2.0, 2026-10-26: https://duckdb.org/2026/10/26/gems-of-duckdb-200.html
- Asynchronous I/O, 2026-07-31: https://duckdb.org/2026/07/31/asynchronous-io.html
- C API (v2) overview: https://duckdb.org/docs/2.0/clients/c/overview.html
- VARIANT type: https://duckdb.org/docs/2.0/sql/data_types/variant.html
- Core extensions: https://duckdb.org/docs/2.0/core_extensions/overview
- Community extensions list: https://duckdb.org/community_extensions/list_of_extensions and descriptors at
  https://github.com/duckdb/community-extensions/tree/main/extensions
- Full-text search extension: https://duckdb.org/docs/2.0/core_extensions/full_text_search
- Securing extensions: https://duckdb.org/docs/2.0/operations_manual/securing_duckdb/securing_extensions
- Advanced extension installation: https://duckdb.org/docs/2.0/extensions/advanced_installation_methods
- Extension repositories: https://duckdb.org/docs/2.0/extensions/extension_repositories.html
- Release calendar: https://duckdb.org/release_calendar.html
- v2.0.0 release assets: https://github.com/duckdb/duckdb/releases/tag/v2.0.0
- duckdb-rs 1.20000.0: https://crates.io/crates/duckdb/1.20000.0 ; libduckdb-sys: https://crates.io/crates/libduckdb-sys/1.20000.0
- parquet 60 (`variant_experimental`): https://crates.io/crates/parquet/60.0.0
- quack-rs: https://docs.rs/crate/quack-rs/latest
- mimalloc `os_tag` default: `libmimalloc-sys` 0.1.49 `c_src/mimalloc/v3/src/options.c`; macOS SDK `mach/vm_statistics.h`
- Debrief: ADR 0014, 0017, 0030, 0031, 0040, 0041, 0044, 0047, 0051, 0052 in `apps/debrief/docs/adr/`

Measurement scripts, logs and the synthetic corpus: `/Volumes/d7sst7/dev-targets/research-data/` (`scripts/`,
`results/`, `large/`), on the Mac mini; not committed.
