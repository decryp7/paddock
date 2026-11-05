# 63. Binary wire between the engine and the UI: Arrow IPC for tables, MessagePack for everything else

- Status: Proposed
- Date: 2026-11-05
- Related: ADR 0016 (command table, one error contract), ADR 0014 and 0017 (the store worker, Arrow event batches), ADR 0018 (lanes),
  family ADR 0001 and [ipc-wire.md](../../../../docs/ipc-wire.md) (the plugin wire), family ADR 0049 (`wire.toml`, C80);
  [research note on drawing](../../../../docs/research/2026-11-debrief-drawing.md) on `docs/debrief-drawing` (M2 and the lane contract);
  plan packages 2.4, 3.3, 4.1, 3.5, M4, IP1 and, from the drawing note, M0, C1; Paddock rules 42, 48, 49, 51, 54, 55; ER-14, ER-20, ER-27
- Evidence marks: **[M]** measured, in the [results block](../performance.md#2026-11-05-binary-wire-formats-debrief-adr-0063-research); **[S]** read in
  source; **[D]** primary doc or registry data, read 2026-11-05; **[U]** not verified; **[calc]** worked out from measured numbers.
  Code citations are at `origin/debrief/p0-idle` a35ef28c1 and tauri 2.12.4. All sizes in days are judgement.

## Answers

The user decided on 2026-11-05 that the wire goes binary for all commands and events. The questions asked along the way:

| Question | Answer |
|---|---|
| Which format? | **Arrow IPC for anything tabular or numeric. MessagePack for arguments, small replies, errors and events.** A reply can hold both: a MessagePack head plus named Arrow tables. |
| Binary, generic, and read a part without loading all? | **Arrow IPC meets all three** for tables, and that is where size makes it matter. MessagePack is binary and generic but scans. That scan is 0.02 to 0.03 ms to row 137 of a page, and those replies are capped, so scanning is not a reason to reject it. |
| Is Arrow the right lead? | **Yes for tables, measured.** 40 visible rows of a 5,000-row page cost 0.05 ms against 6.7 ms for `JSON.parse`; lane counts 0.003 ms against 3.4 to 4.5 ms, with no copy; half the memory. It is not faster everywhere: see "Where binary is slower". |
| Can Arrow be the one format everywhere? | **No.** A one-row error is 1,608 B against 128 B and 8 us against 1 us. Nested settings with tagged kinds would need Arrow unions by hand. And `arrow-ipc`'s reader panicked on 668 of 20,000 mutated inputs, so it must never read what the webview sends. |
| Does FlatBuffers work? | **Yes as a format, no as Debrief's wire.** It has the fastest lookups measured (under 1 us for one field). But it needs a schema beside the Rust types and hand or generated builders, because it has no serde. For 393 commands that is a second definition or a new generator. It is larger than JSON here, and its builder is 2.6x slower than `serde_json`. |
| Can one generic FlatBuffers schema avoid that? | **It avoids the second definition of each message, and only moves the types to a run-time check.** `Frame { columns: [Column] }` still needs the generator and test that the contract rule asks for. Measured, it reads like Arrow (0.052 ms against 0.054 ms for 40 rows) and gives up the standard, DuckDB's output and the existing readers. It is Arrow reinvented. |
| FlexBuffers? | **No.** The official JS reader throws on any buffer with a 64-bit-wide value, so a single `ts` breaks it. The Rust reader did not finish within 5 s on a mutated 360-byte input, and a longer run hit a 120 s limit. `BTreeMap<Level, _>` (Debrief's `levels`) does not round trip. Full decode is 5 to 14x slower than JSON. |
| Any alternative to `apache-arrow`? | **Flechette** reads the same bytes in 9 KB gzipped against 51 KB, and 1.6x to 3.3x faster when reading every row of 200. It is chosen, with `apache-arrow` as the fallback, since the bytes are a standard. Nothing based on nanoarrow exists on npm, and WebAssembly readers are blocked by our CSP. |
| Other alternatives? | Searched and listed in "What was considered". None meets the criteria better. |

**Plain statement of the gain.** A grid page today is 500 rows (`grid/paged-rows.store.ts:7`), and `JSON.parse` of one costs well under 1 ms [calc from 200 and 5,000]. So decode is
not what limits the grid at today's page size. The gain is real for lanes, for pages near the 5,000 cap, and for memory. For small replies binary is slightly slower, not
faster. The IPC step itself, where the real time may be, has **not been measured**; package W0 does that first.

## Context

**Today, everything is JSON** [S]. 393 commands in `command_table!` (`app/src-tauri/src/lib.rs`), all returning `Serialize` values, which Tauri turns into JSON
(`ipc/mod.rs:181`). `invoke` reads named arguments out of a JSON body (`ipc/command.rs:96`). Rejections are JSON (`ipc/mod.rs:409`). Events take `S: Serialize` and travel as JSON through eval
(`manager/mod.rs:524`); the docs say they are "not suitable for bigger messages" ([calling the frontend](https://v2.tauri.app/develop/calling-frontend/) [D]). About 58 emit sites and 33 event names.

**One place on each side already sees every command** [S]. In Rust, `commands::guard` and `activity::logged` wrap the handler (`lib.rs:619`). In TypeScript, `core/activity/ipc.ts` is the only
importer of Tauri's `invoke`, and a coverage test enforces that. `listenHere` (`windowing/window-events.ts`) is nearly that for events.

**Tauri's binary paths** [S, D]:

- Reply: `tauri::ipc::Response`, read with `response.arrayBuffer()` (`scripts/ipc-protocol.js`); `invoke` resolves to an `ArrayBuffer`. If the custom protocol ever fails, the macOS fallback sends the bytes as a JSON array through eval (`ipc/protocol.rs:390`).
- Arguments: `invoke(cmd, uint8Array, { headers })` gives `InvokeBody::Raw`, readable through `Request` or any `CommandArg` (`ipc/mod.rs:165`). `commands.rs:137` already counts raw bodies against each limit. Named arguments do not work on a raw body.
- A command's `Channel` argument is read by name from JSON too (`ipc/channel.rs:466`). `JavaScriptChannelId::channel_on` (`:273`) is public, so the id can travel inside binary arguments.
- `Channel` carries raw bytes: under 1 KB by eval as a number array, otherwise by a fetch, one more round trip (`channel.rs:36-38`, `:302-312`).
- There is **no generic hook on a result**: the resolver's responder is private (`ipc/mod.rs:286`), as `activity/ipc.rs` notes. So a transport cannot turn all replies binary from the outside; each command's signature has to change.

**Generators** [S]. ts-rs with `generated_typescript_is_current` in 20-plus shared crates and in `generated_types.rs` here. But `api.types.ts` still mirrors 364 types by hand against 142 generated ones
(`generated_types.rs:5`). Serde use in Debrief's wire types, counted by grep: `skip_serializing_if` 105, `tag =` 46, `flatten` 15, `untagged` 4.

**Arrow is already Debrief's own data form** [S]. DuckDB answers through `stream_arrow` (`debrief-store-duck/src/read.rs:167`), events have an Arrow schema (`debrief-batch/src/schema.rs`: fixed columns, `f_*` declared
columns, an `extra` map), and the store worker sends scans as Arrow IPC items (`debrief-store-client/src/arrow_wire.rs`). A grid page leaves the store as Arrow, becomes `EventView` rows, crosses to the
app as postcard, and is then written as JSON. `arrow-ipc`, `arrow-array` and `flatbuffers` are already in `Cargo.lock`.

**The drawing note** recommended typed arrays inside the UI and JSON for everything but new bulk endpoints, with M2 to decide. This ADR follows the user's wider decision and keeps its point that M2 is still owed.

## Decision

### 1. Two encodings, one envelope

- **Arrow IPC stream, read with Flechette and written with `arrow-ipc`,** for every reply that is a table or a typed numeric column: grid pages, events by id, facets, lane buckets, chart series, flame rectangles, thread and module tables, diff rows.
- **MessagePack, `rmp-serde` and `@msgpack/msgpack`,** for arguments, every other reply, errors, events and progress. Struct fields travel by name, so today's serde attributes and ts-rs types still hold (below).
- A MessagePack reply has a **cap of 256 KiB of structure**: over it, a test fails in debug and a warning is logged in release, and the reply should become a table. One long `str` or `bin` value is exempt because it is read whole. The cap is what keeps "scan" honest: at 128 KB a full decode costs 0.4 to 0.65 ms [M].

Both are self-describing, so neither needs a schema compiler in the build.

### 2. The reply

```
offset 0   "DW" magic, u8 wire version, u8 status (0 ok, 1 error)
offset 4   u32 LE head length
offset 8   u64 LE fingerprint of the generated wire types
offset 16  head: MessagePack (the whole reply for most commands; the error when status is 1)
           padding to a multiple of 8
then       zero or more tables: u64 LE length, an Arrow IPC stream, padding to a multiple of 8
```

So `Page` is `{ total, offset, anchorIndex }` in the head and `rows` as a table. The padding keeps Arrow's buffers on 8-byte boundaries, so numeric columns can be views on the reply. Sharing the buffer was measured with the stream at offset 0 only; W2 tests it inside the envelope.

### 3. Rust surface: `crates/paddock-wire`

- `#[wire_command]` replaces `#[tauri::command]` and keeps the function's signature. It separates injected parameters (`CurrentWindow`, `AppHandle`, `State`, `OpId`) from data, generates an arguments struct with camelCase fields, decodes it from the raw body, and returns `tauri::ipc::Response`. TypeScript call sites, which already pass `{ sourceId }`, do not change.
- `#[derive(WireTable)]` on a row struct generates the Arrow schema, the batch builder and the TS side (section 7). `Table<T>` is the field type in a reply. `serde_arrow` was considered: MIT, last pushed 2026-10-03 [D], but one more dependency tied to Arrow's version, not measured. W1 may adopt it if the derive turns out to be more than about 200 lines.
- Serialise with `with_struct_map()` **and `with_human_readable()`**. rmp-serde's default is not human readable (`config.rs:104`), and `paddock_ipc::Json` branches on that flag (`crates/paddock-ipc/src/wire.rs:53`), so without it plugin values would reach the UI as JSON text.

### 4. TypeScript surface: `core/wire/`

- `core/activity/ipc.ts` stays the one edge. A generated set says which commands are binary: their arguments are encoded before `invoke`, and an `ArrayBuffer` reply is decoded after. A status of 1 throws the same `CmdError` as today. Nothing above `invoke` sees bytes. A reply that is already an object (a framework rejection, a shell command) passes through, because Tauri picks by content type.
- A table reaches callers as `RowTable<T>`: `length`, `at(i)`, iteration, and typed column access such as `counts(): Uint32Array`. Rows are built on demand and memoised for the page, so Angular still gets stable object identity. `Page.rows` changes from an array to `RowTable<RecordView>`; about ten callers read `queryPage` (`events-grid.ts`, `export-dialog.ts`, `problems.store.ts`, others).
- Typed arrays stay the UI's contract for lanes and charts, as the drawing note proposed.

### 5. Errors, events, progress, plugins

- **Errors.** A command error is status 1 with `CmdError` in the head. The Guard's refusals and Tauri's own failures (unknown command, ACL) are still rejected as JSON, because `InvokeResponse::Err` is JSON on every path [S]. `fromRejection` stays for those. This is one of the two places JSON survives, and neither is ours to change.
- **Events.** One `wire_subscribe(channel)` per webview and one `Channel<Response>` registered for it. A message is `u16 topic` plus MessagePack. `paddock_wire::emit` and `emit_to` replace `Emitter::emit`; `listenHere` keeps its signature. The registry drops a channel when its window closes. Tauri's native events (window, drag and drop, menu) stay native: that is the second place. Note that a small message costs more than JSON on this path: 128 B of MessagePack becomes 479 characters of eval text against 146 for JSON [calc]. Whether that matters for progress events is part of W0.
- **Progress and progressive results.** `Channel` arguments travel as ids in the binary arguments. A progressive table sends self-contained Arrow streams, as `arrow_wire.rs` does: the 3.4 KB schema repeats, which adds 13 % when 200 rows go in five parts and 0.5 % for 5,000 [M]. Flechette can read a part alone (0.024 ms) or join them (0.105 ms) [M]. Use it only where a slow lane should not hold up the others, because each message over 1 KB costs a round trip.
- **Plugin results.** The plugin wire stays postcard (`paddock-ipc`). Postcard cannot be the UI wire anyway: it refuses `untagged`, internal tags, `flatten`, `skip_serializing_if` and `Value` [M]. The host decodes to Rust types and the command re-encodes them. IP1 is not affected, apart from one more fuzz target.

### 6. Fields of an event

`fields` is the awkward column: per row, heterogeneous. Measured options [M]:

| Form | Rust encode, 200 rows | 40 visible rows | Every row as today's object | Type of a value |
|---|---|---|---|---|
| `Map<Utf8,Utf8>` plus typed `f_*` columns | 0.116 ms | 0.10 to 0.13 ms | 2x JSON | kept for declared fields; `extra` values are text, as in the store |
| JSON text column | 0.082 ms | 0.05 to 0.06 ms | same as JSON | kept, but JSON survives inside the wire |
| One column per field name | 0.340 ms | 0.06 to 0.07 ms | 2 to 3x JSON | text |

**Decision: `Map` plus `f_*`**, because it mirrors the store's schema (`schema.rs`) and allows W10. It is the slowest of the three when every row is read, and no screen does that: the export runs in the engine. The JSON text column is the fallback if the user prefers speed over a pure wire (D3). The numbers above used a `Map` of all fields; the `f_*` columns themselves were not measured apart.

### 7. Contract: one declared definition, checked on both sides

Rust types are the definition, as today. No `.fbs`, no second schema.

- General types: ts-rs, as today, in `generated_types.rs` and `generated_typescript_is_current`. The serde shape is unchanged, so the TS shape is unchanged.
- Tables: `WireTable` generates `core/generated/wire-tables.ts`: column names, types, accessors, fingerprint. The column check is at run time in both Arrow and any generic schema. That is why the generator and the test below are required.
- **Both sides are checked:** the generator writes `wire-fixtures.ts` holding each sample's bytes and its `serde_json` form, as `paddock-settings` already writes `settings-fixtures.ts`. A spec decodes the real bytes with the real readers and compares. Samples include `NaN` (JSON gives `null`, MessagePack `NaN` [M]), `null` against absent, enum map keys and a `u64` above 2^53.
- **Numbers.** Integers above 2^53 lose precision as they do with `JSON.parse` today: `@msgpack/msgpack` matched `JSON.parse` exactly on all samples, and Flechette coerces to `number` by default (`apache-arrow` gives `bigint`) [M]. msgpackr and cbor-x gave `bigint` for `ts`, which is one reason they are not chosen. No regression, no improvement.
- The 364 hand-mirrored types are not required to move for this change, except the ones that become tables. Moving the rest is worthwhile, not part of this ADR.

**Evolution.** UI and engine ship as one binary, so there is no old peer to serve, and a change lands on both sides at once (no shim, no compatibility before v1). The only mismatch is a stale webview in development: a differing fingerprint gives a typed error ("the UI and the engine are from different builds: reload") and a log line. These bytes are new and nothing stores them; files on disk go through their own serde calls [judgement, not audited]. Debrief has no `wire.toml` yet; `ui-engine` does not need ADR 0049's version derivation because no tag serves an older peer. The fixtures take the place of its goldens.

### 8. Hostile input

The engine re-validates everything, as now; `Args<T>` adds the decode step. The UI to engine direction uses only `rmp-serde`: 200,000 mutated inputs, no panic, slowest 13 us [M], a body already limited by the Guard. A fuzz target joins the nightly job of IP1 item 6. **`arrow-ipc` never reads webview input** (668 panics in 20,000 mutations [M]), and the conformance check holds that line. The engine to UI direction is trusted: no JS reader of any format verifies (Flechette, `flatbuffers`: nothing in the published `js/` [S]), and a bad buffer throws at the edge or gives wrong values. The same engine built both ends.

### 9. Logs and dev tools (ER-27)

- UI's activity entry for each command gains: reply bytes, kind (`head`, `head+N tables`), rows, decode ms. On a decode failure: command, version, status, head length, bytes received, the error. Never the body, since the header holds no user content.
- Tauri's "IPC custom protocol failed" fallback is logged once as a warning with the command, because it turns bytes into a JSON array and costs far more.
- `./dev wire decode --app debrief <file|hex>` prints a captured reply as readable JSON and tables, for a person's own use. A debug-build-only toggle logs decoded replies to the DevTools console. Neither writes to log files or diagnostic bundles, so bundles still explain failures from phases and outcomes with no user content.
- DevTools' network view will show binary. That is the honest cost; the toggle and the tool are the answer.

### 10. Mock backend and specs

Mocks sit above `DebriefBackend`, so nothing below that line reaches them: they keep returning plain objects. For tables they return `arrayTable(rows)`, a plain-array `RowTable`, so mocks and specs need no encoder and Arrow stays out of the browser build's mock path. Mocks stay what `AGENTS.md` says: never the reference. The fixtures in section 7 run the real readers, in specs without patches or fixed waits (rule 49). Rust tests that parse `InvokeResponseBody::Json` (`logging.rs:521`, `panel_windows.rs:771`, `profiling.rs:407`, `query_cancel.rs:174`, `ai/folder_setup.rs:124`) move to `paddock_wire::testing::decode`. No UI component changes, so rule 47 is not touched.

## Where binary is slower

The user chose binary knowing this might happen. The numbers, Chromium / WebKit, in ms [M]:

| Reply | JSON | Chosen form | Slower by |
|---|---|---|---|
| Settings overview | 0.036 / 0.037 | MessagePack 0.086 / 0.147 | 2.4x / 4x |
| Facet, every row | 0.070 / 0.093 | Flechette 0.213 / 0.232 | 3x / 2.5x |
| Error | under 0.001 | MessagePack 0.001 | about 1x, 1 us |
| Page, 200, every row | 0.18 / 0.23 | Flechette `Map`, pairs not yet objects, 0.49 / 0.67 | 2.7x / 2.9x |
| Page, 5,000, every row | 6.7 / 6.8 | Flechette `Map`, pairs not yet objects, 14.8 / 15.2 | 2.2x |
| Arrow stream with no rows | none | 3,464 B per reply | |
| Small event through `Channel` | 146 chars | 479 chars of eval text [calc] | 3.3x |

Why: JS strings. `JSON.parse` is one native pass over text, and a decoder written in JS crosses into `TextDecoder` per string. Measured, WebKit pays more than Chromium.

How the design keeps the worst endpoints from regressing:

- **Tables are read lazily, so "every row" does not happen on a screen.** Facet: 50 visible values of 1,000 cost 0.010 ms against 0.09 ms for JSON (Node). Page: 40 rows of 5,000 cost 0.05 to 0.06 ms against 6.7 ms.
- **Dictionaries** on repeated columns (`level`, `sourceName`) save 8 to 9 % of the bytes and some decode time [M].
- **The absolute regressions are tens of microseconds.** Settings is read when a dialog opens; a hundred microseconds does not show.
- What cannot be designed away: small nested replies are 2 to 4x slower than JSON, for a reason of uniformity rather than speed.

What improves [M]: lanes 3.4 to 4.5 ms to 0.003 ms with no copy; a 5,000-row page 6.7 ms to 0.05 ms for what a screen reads; retained memory 5.06 MB to 2.64 MB plus 0.18 MB for 200 touched rows; Rust encode no slower (0.082 against 0.088 ms for 200 rows with JSON text, 0.116 with `Map`); bytes 17 to 24 % fewer on string-heavy replies. Lanes are 4 % larger in bytes than in JSON, since small counts are short as text. Lane counts in MessagePack or JSON are **not** better than an aligned column, and neither is FlexBuffers.

## What was considered

Criteria: binary; generic (no schema step beyond the Rust types); a part can be read without loading all; Rust and JS libraries; licence; health. Health data from crates.io, npm and GitHub, read 2026-11-05 [D].

| Candidate | Generic | Reads a part | Rust / JS | Licence | Health | Verdict |
|---|---|---|---|---|---|---|
| **Arrow IPC + Flechette** | Yes, schema in the stream | Yes: columns by offset, strings per cell | `arrow-ipc` 60.1 (2026-10-28; in the tree) / `@uwdata/flechette` 2.7.1 (2026-10-23) | Apache-2.0 / BSD-3-Clause | arrow-rs large; Flechette 156 stars, 113 of the 128 listed commits from one person | **In: tables** |
| Arrow IPC + `apache-arrow` | Same | Same | Same / 21.2.0 (2026-07-21), repo pushed 2026-11-03 | Apache-2.0 | Good | **Fallback reader**: 51 KB gz, 1.6 to 3.3x slower reading every row of 200, `bigint` by default |
| Arrow IPC file (Feather V2) | Same | Adds a footer for random access to a batch ([spec](https://arrow.apache.org/docs/format/Columnar.html)) | Same | Same | Same | Out: a reply is one batch in memory; a footer adds nothing |
| nanoarrow-based JS | | | C library; no `nanoarrow` on npm; `arrow-js-ffi` needs WebAssembly memory | Apache-2.0 / MIT | | Out: our CSP blocks WebAssembly ([drawing note](../../../../docs/research/2026-11-debrief-drawing.md)) |
| **MessagePack** | Yes | Scan only: headers hold element counts, not byte sizes ([spec](https://github.com/msgpack/msgpack/blob/master/spec.md)). Row 137: 0.02 to 0.03 ms [M] | `rmp-serde` 1.3.1 (2025-12-23) / `@msgpack/msgpack` 3.1.3 (2025-12-26) | MIT / ISC | Mature, 30M downloads a quarter | **In: small replies and arguments** |
| msgpackr | Yes | Scan | Same bytes / 2.1.4 (2026-11-02) | MIT | Active | Out: about as fast as `@msgpack/msgpack` in browsers, 12 KB against 4 KB, `bigint` for `ts`, carries `new Function` for its records mode |
| CBOR | Yes | Scan | `ciborium` 0.2.2 (2024-01) / `cbor-x` 1.6.9 | Apache-2.0 / MIT | ciborium quiet since 2024 | Out: same speed as MessagePack; Chromium logged a CSP error for cbor-x [M]; no reason to prefer it |
| **FlatBuffers, per message** | No: `.fbs` per message | Best: under 1 us | `flatbuffers` 25.12.19 (in the tree) / npm 25.9.23 (lags) | Apache-2.0 | Active | Out: see below |
| FlatBuffers, generic `Frame` | Yes, but a run-time check | Yes | Same | Apache-2.0 | Active | Out: Arrow without the standard |
| FlexBuffers | Yes | Yes: binary search of the key vector ([docs](https://flatbuffers.dev/flexbuffers/)) | `flexbuffers` 25.12.19 / inside `flatbuffers` npm | Apache-2.0 | Lives in the FlatBuffers repo | Out: JS reader broken on 64-bit widths, Rust reader stalls on a mutated input, enum map keys fail [M] |
| Protobuf | No: `.proto`, tags | Scan; fields in any order ([encoding](https://protobuf.dev/programming-guides/encoding/)) | prost / `@bufbuild/protobuf` 2.17.0 (2026-11-05) | Apache-2.0 | Active | Out: schema-first and no in-place read [not measured] |
| Cap'n Proto | No | Yes, lazily validated, traversal limit ([encoding](https://capnproto.org/encoding.html)) | `capnp` 0.27.3 / `capnp-es` 0.0.16 | MIT | JS reader young | Out: schema-first, needs its compiler, no serde [not measured] |
| Postcard | Positional | Scan | In the tree / no JS reader; a hand-written positional one was fast (0.22 ms) | MIT/Apache-2.0 | Active | Out: refuses what Debrief's types use [M] |
| Ion | Yes | Skip scan, by length prefix ([binary](https://amazon-ion.github.io/ion-docs/docs/binary.html)) | `ion-rs` 1.2.0 (2026-10-24) / `ion-js` 5.2.1 (2024-04-30) | Apache-2.0 | JS publish old | Out: 58.8 ms at 5,000 rows on Chromium [M] |
| BSON | Yes | Skip scan, length-prefixed ([spec](https://bsonspec.org/spec.html)); arrays have decimal keys | `bson` 3.1.0 / `bson` 7.3.3 | MIT / Apache-2.0 | Active | Out: largest of the self-describing forms, 15 to 22 ms at 5,000 rows [M] |
| SQLite JSONB | Yes | Skip scan, no index | No JS reader | | | Out: "not intended as an external format" ([docs](https://sqlite.org/jsonb.html)) |
| Smile | Yes | Scan | `serde-smile` 0.3.1 (2025-09) / no maintained JS reader found [U] | MIT/Apache-2.0 | | Out |
| Simple Binary Encoding | No: XML schema | Fixed-layout fields | Rust, no JS listed ([repo](https://github.com/aeron-io/simple-binary-encoding)) | Apache-2.0 | | Out: no JS |
| Bebop | No: schema | No | `bebop` 3.2.3 (2025-09) / npm 3.2.3 | Apache-2.0 | Pushed 2026-02 | Out: schema-first, no in-place read [U] |
| rkyv | Rust types | Yes | Rust only ([site](https://rkyv.org/)) | MIT | Active | Out: no JS |
| Lite³ | Yes | Yes, O(log n) | C only, "the API is unstable", formal spec still open ([repo](https://github.com/fastserial/lite3), pushed 2026-08) | MIT | Young | Out for now: the closest idea to "generic and lookup"; no Rust or JS library |
| Apache Fory row format | Schema order | Yes | Rust; [row format](https://fory.apache.org/docs/specification/row_format_spec) not listed for JS | Apache-2.0 | Active | Out: no JS row reader |
| ORC | File | Stripes | No JS ([docs](https://orc.apache.org/docs/)) | Apache-2.0 | | Out: a file format |
| Vortex | File | Random access claimed by the project | `vortex-array` 0.91 (2026-11-04); JS only as `vortex-web`/WebAssembly, undocumented ([repo](https://github.com/vortex-data/vortex)) | Apache-2.0 | Active, API moving | Out as a wire: blocked by CSP |
| Lance | File | Row ranges ([docs](https://lance.org/format/file/)) | `lance` 14 / native Node binding only [U] | Apache-2.0 | Active | Out as a wire |
| DuckDB's native result | | | No documented external form found [U]; DuckDB exports Arrow | | | Out: Arrow is its interchange |

**FlatBuffers in full, as the user asked.**

- *Lazy access on a grid page:* real and the best measured: 0.043 / 0.059 ms for 40 rows, under 1 us for one field.
- *Builder cost in Rust:* 0.232 ms for 200 rows against 0.088 ms for `serde_json` and 0.082 ms for Arrow.
- *Schema-first against Rust-types-first:* `.fbs` beside the Rust type is the duplicate the contract rule forbids. Generating `.fbs` from Rust is possible, but there is no generator: ts-rs does not do it, and FlatBuffers has no serde, so each type also needs generated builder code. With about 650 `Serialize` derives in the app and core, that generator is a project of its own [calc from grep].
- *`flatc` in CI:* official binaries for Linux, macOS and Windows, 1.4 to 2.7 MB, in the 2026-02-06 release [D]. **I did not run it**: the permission system refused running a downloaded binary, and I wrote the accessors by hand with the runtime's own calls. So generated-code speed and `flatc` integration are not measured. It would be one more pinned tool on every OS.
- *TS runtime:* 1.6 KB gz, the smallest. Npm is on 25.9.23 while the crate is on 25.12.19.
- *Verifier:* Rust has one (`get_root.rs:26`); the TS runtime has none [S]. For a trusted engine that does not matter. For webview to engine it would, and only Rust is on that side.
- *Evolution:* add at the end or use ids, deprecate rather than delete, unions extend at the end ([rules](https://flatbuffers.dev/evolution/)). Good, but not needed here: UI and engine ship together.
- *Unions for errors and events:* work, as `.fbs` unions; the same second definition.
- *Lane buckets:* a vector view, under 1 us, equal in practice to Arrow's 0.003 ms. No advantage.

**Arrow, tested hardest, as the user asked.**

- *Bundle:* `apache-arrow` tree-shaken is 219 KB / 51 KB gz, heavy for a window. Flechette is 25 KB / 9 KB. With `@msgpack/msgpack` (decode and encode) the wire adds about 47 KB raw and 15 KB gz, which goes through C29 and rule 35's budgets. It belongs in the lazy bootstrap chunk rather than `main`.
- *CSP:* both load and decode under Debrief's policy, no violation, Chromium and WebKit [M].
- *Strings:* a cell is one `TextDecoder` call. Reading every string costs about what `JSON.parse` costs in total; reading what the screen shows costs far less.
- *DuckDB output:* `stream_arrow` then an IPC write, SQL included, took 2.71 ms for 200 rows and 5.47 ms for 5,000, against 2.88 and 9.34 ms for a row loop and JSON that left out the `MAP` column. Types: `UInt64`, `Timestamp(µs)`, `Utf8`, `Map`, in 1 and 3 chunks. Flechette and `apache-arrow` read it directly, with `ts` as a millisecond number [M].
- *Build time:* no added crate in Debrief. A full clean harness build, Arrow included, took 93 s at 2 jobs [M]; Debrief pays that today already.
- *Dictionaries:* 9 % smaller, slightly faster to read all rows, decoded upfront (open cost 0.02 ms). Worth it for `level` and `sourceName`.
- *Streaming through `Channel`:* works; see section 5.
- *Non-tabular replies:* one-row batches work for flat errors and are 12x larger; nested settings would need unions or a JSON cell (25,928 B and no gain over JSON). So MessagePack.

**Against the requirement "fast lookup without loading all":** MessagePack was not rejected on it. Scanning to row 137 costs 0.02 to 0.03 ms; to the end of a 3.3 MB page about 1.2 ms [calc]. With a 256 KiB cap that bound is about 0.1 ms, less than a full decode. Tables get true in-place reads.

**On-disk storage** is a separate question, on `docs/debrief-storage-medium`, which has no committed text on `origin` yet. A reply is a bounded buffer already in memory; a store has I/O, pages and indexes. Nothing here depends on that choice: Parquet, Vortex and Lance all hand Arrow batches to the engine, so the wire stays Arrow IPC whichever wins. Wording to align once that branch lands.

## Shared crates and the other apps

**The choice: shared commands stay JSON for now; a compile-time switch moves the family after the tags. Nothing shared changes before the Dyno tag.**

Facts [S]: Debrief's app links no `paddock-*` crate today, so all 393 commands are its own. M4 brings in `paddock-shell` and `paddock-workbench`, and their commands. Shared crates hold about 126 commands and 36 emit sites, used by Dyno and others. The UI of the shared shell takes its `invoke` by injection (`ui/projects/paddock-ui/shell/about/src/backend.ts:20`).

1. `crates/paddock-wire` is new and generic (rule 42). It lands after the Dyno tag, which suits the order: Debrief 1.2 comes after it anyway. If Debrief work starts earlier in a free slot, the crate stays on that branch.
2. When M4 lands, the commands moved to the shell stay JSON inside Debrief, on an allow-list in the conformance check with a delete-by date (rule 55's mechanism). `ipc.ts` already passes such a reply through.
3. Family step after Dyno and Spotter are tagged: shared commands use `Args<T>` and `Reply<T>` with a `binary-wire` Cargo feature of `paddock-wire`. Each app is its own Cargo workspace, so features do not unify across apps. An app turns it on with its own release; the shared UI needs no change because `invoke` is injected. Shared crates are tested with the feature on and off until the last app has moved, and then the feature and JSON path are deleted.

Rejected: run-time negotiation by a header (a shim that never leaves), a breaking change to shared crates before the tag (rule 42 needs all apps updated, and Dyno is near its tag), and Debrief wrapping shared JSON into binary at its own edge (a copy, and it gains nothing since those replies are small).

## Migration

**Per domain, on small merges, with no shim.** Tauri picks by content type and `ipc.ts` needs that pass-through permanently for framework rejections, so mixed states are normal and safe during the move. The allow-list starts with every command and shrinks to the shell's. A single large change would conflict with nearly every open Debrief branch.

| Package | What | Role | Days (judgement) |
|---|---|---|---|
| **W0** | Measure in a real Tauri build: 500 KB, 2 MB, 16 MB and a small reply through `invoke` JSON, `ipc::Response`, `Channel`; main-thread blocking; footprint; small events both ways; WKWebView, WebKitGTK, WebView2. It is the drawing note's M2. Probe first (ER-33). Windows and Linux on their runners, no desktop windows on the user's Mac (ER-14) | researcher | 1 |
| **W1** | `paddock-wire`: envelope, `Args`, `Reply`, `Table`, `#[wire_command]`, `WireTable`, event bus, `testing::decode`, fuzz target, 99 % coverage | designer | 4 |
| **W2** | `core/wire/`: edge in `ipc.ts`, `RowTable`, `arrayTable`, channels, fingerprint, fixtures test, log lines, alignment inside the envelope | designer | 2.5 |
| **W3** | Conformance check and the allow-list | implementer | 1 |
| **W4** | Tables: `query_page`, events, facets, charts, flame, thread and module tables, diff rows, and the callers of `Page.rows` | designer | 4 |
| **W5** | The other commands by `#[wire_command]`, mostly mechanical; after M4 so moved commands are skipped | implementer | 3 |
| **W6** | Events: emit sites and `listenHere` to the bus; existing channels to binary | implementer | 2 |
| **W7** | Plugin results and `Json` values through the human-readable path, with tests; the fuzz target in IP1's nightly job | implementer | 0.75 |
| **W8** | Mocks and specs for `RowTable`; Rust tests that parse replies | implementer | 1.5 |
| **W9** | `./dev wire decode`, the debug toggle, wiki and docs | implementer | 1 |
| **W11** | Full suite once, a rule 48 block on the result, budgets (rule 35), and installers through the app's release workflow (rule 54) | implementer | 1 |
| | **Total** | | **about 22** |
| W10, optional | Carry `EventBatch` from the worker to the UI for grid pages, and skip two conversions. Redaction, `sourceName`, `group` and marker names become derived columns. Only if W0 and W4 show the conversions cost enough | designer | 3 |
| F1, later | The family step above; each app then its own small change | designer | 4 to 6, plus 1 to 3 per app |

**Overlap.**

| Package | Effect |
|---|---|
| M0/M2 (drawing note) | W0 is M2; do once. |
| 3.3 Timeline engine | `lane_buckets` answers as a table, so the drawing note's own raw encoder, header and golden test (part of its +1.5) are not written. W1 must land first; if 3.3 goes first it uses the raw form and W4 moves it. Saves about 1. |
| 2.4 Lane canvas | No change: draws from typed arrays either way. |
| 4.1 Timeline UI | Its typed cache reads `counts()`. No change in size. |
| C1 Chart bound | Columns arrive as Arrow `Float64`. Saves a little. |
| IP1 | Plugin wire unchanged. Its item 6 nightly job takes one more target. |
| M4 | W5 after M4, or it migrates commands about to be deleted. |
| `debrief/stream-text` (3.5) | No wire dependency. It touches `store_pane.rs`, `workspace_commands.rs` and `api.types.ts`, so W4 and W5 land after it. |

Net of overlap, about 20 to 22 days. No release date moves by this ADR until the user places it (D4).

## Parallelism and cost (rule 51, ER-20)

- **Cheaper, which is where the gain is.** Read only what a screen shows; views instead of copies for numbers; dictionaries; a bounded table instead of a large nested reply; W10 removes two conversions per page. All measured except W10.
- **More threads: no.** Encoding runs on the command's async task, 0.08 to 3 ms per reply, far below what a pool could help. JS decode is on the main thread and lazy, at 0.05 ms per screen. A worker would add a copy or a transfer, and `invoke` cannot run in one (the IPC key is in a closure, per the drawing note). So no setting is added and no worker.
- **Bounded.** `MAX_PAGE_ROWS` 5,000, the MessagePack cap, the Guard's limit on arguments.
- **Not repeated.** Rows are memoised per page; a superseded lane request is cancelled as the drawing note describes.

## Conformance

A new check, `C<next>` (the integrator assigns the number), runs over `apps/debrief`:

1. Every command in Debrief's table is declared with `#[wire_command]`; a bare `#[tauri::command]` fails unless on the allow-list, with its delete-by date.
2. No `.emit(`, `.emit_to(` or `.emit_filter(` in Debrief's Rust outside `paddock_wire`, and Clippy `disallowed-methods` says the same at the call site.
3. No `listen(` from `@tauri-apps/api/event` outside `windowing/window-events.ts`, beside the existing rule for `invoke`.
4. No `arrow_ipc::reader` in code reachable from `Args<T>` or a command's arguments.
5. A `MockRuntime` test sends every command in the table an empty raw body and asserts the reply is raw, or a refusal. So a command that slipped back to JSON fails even if it fooled the source check.

## Risks

- **W0 may show the IPC step dominates and the wire barely matters.** That is the largest unknown: every number here starts after the bytes arrive, and none is from a real webview. If so, the fallback below is cheaper.
- **Flechette has one main author** (113 of the 128 listed commits). Mitigation: bytes are standard; `RowTable` is the only importer; swapping to `apache-arrow` costs one file plus 41 KB gz.
- **Lazy rows change an API many callers read** (`Page.rows`). Mitigated by `at()` memoising and `arrayTable` in specs.
- **Small `Channel` messages cost more than JSON** by eval. If W0 shows it matters for progress, small events can go through a fetch-based path or batch.
- **The macOS fallback** turns raw bytes into a number array on protocol failure [S]. It is logged. Whether it fires in practice is part of W0.
- **Conflicts.** 393 signatures change. Small merges and `#[wire_command]` as a one-line edit keep that low.
- **Numbers are from Playwright's browsers,** one run under load, on an M4 Pro. No Windows or Linux figure.

## Fallbacks, ranked

1. **Decision above.**
2. If Flechette fails on a type or stalls: the same bytes with `apache-arrow`.
3. If W0 finds no gain from binary on small replies, or the user wants the 6 or so days back: **tables as Arrow, small replies stay JSON.** W5, most of W6 and part of W8 go away. Small replies then stop being 2 to 4x slower. This contradicts "everywhere", so it is the user's call only.
4. If `#[wire_command]` proves hard: an arguments struct per command by hand, 2 to 4 more days.

## Not measured

The IPC step and full-stack latency; main-thread blocking; webview footprint (rule 26); WKWebView, WebKitGTK, WebView2; Windows and Linux; `flatc`-generated code; Protobuf, Cap'n Proto and Bebop; `serde_arrow`; `f_*` columns apart; `Channel` in any form; alignment inside the envelope.

## Decisions for the user

- **D1.** Accept Arrow IPC for tables and MessagePack for the rest, or say what should differ.
- **D2.** Shared code: JSON in shared commands for now, with a compile-time family move after the Dyno and Spotter tags (recommended), or Debrief alone with the shell's commands on JSON indefinitely.
- **D3.** `fields`: `Map` plus `f_*` (recommended, keeps JSON out and allows W10), or a JSON text cell (faster when every row is read, simpler, but JSON inside the wire).
- **D4.** Where about 22 days goes in the 1.2 schedule. Recommended: W0 right after the Dyno tag, because it can change this ADR, and W1 before 3.3.
- **D5.** Flechette with one main author (recommended), or `apache-arrow` from the start at 41 KB gz more.

## Sources

Read 2026-11-05. Tauri: [calling Rust](https://v2.tauri.app/develop/calling-rust/), [calling the frontend](https://v2.tauri.app/develop/calling-frontend/). Arrow:
[columnar format](https://arrow.apache.org/docs/format/Columnar.html), [versioning](https://arrow.apache.org/docs/format/Versioning.html), [arrow-js](https://github.com/apache/arrow-js), [Flechette](https://github.com/uwdata/flechette),
[nanoarrow](https://github.com/apache/arrow-nanoarrow), [serde_arrow](https://github.com/chmp/serde_arrow). DuckDB: [configuration](https://duckdb.org/docs/configuration/overview). Others as linked in the table. Registries: crates.io, npm, the GitHub API.
