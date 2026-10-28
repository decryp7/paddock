# DuckDB 2.0 server mode (Quack) as Debrief's store architecture

Date: 2026-10-28. Question (user): is DuckDB 2.0's client/server mode, the Quack protocol and `CONNECT`, better for
Debrief than today's design (parse threads write zstd Parquet segments; a supervised store worker owns DuckDB; the UI,
the AI worker and plugins reach it through Debrief's own IPC)? ADR 0010 §"Later: DuckDB 2.0 and its server mode" says
to migrate once 2.0 is released, duckdb-rs supports it and the server mode is stable on all three platforms. This note
checks those conditions and measures the server mode against today's design.

**Answer: don't adopt Quack as the store architecture (today's design stays); revisit at DuckDB 2.1 with the checklist
in §6.** ADR 0010's "migrate to it" should become "do not migrate; revisit when the checklist passes" (a decision for the
user, §7).

Key numbers (3M synthetic events, macOS arm64, release builds, three interleaved rounds, medians):

| | Today (Parquet + worker + IPC) | Quack |
|---|---|---|
| Write path | 4.4M events/s (8 writers), ~0.8 GB | 115k/s (1 writer), 3.4 GB client + server; 4 writers run out of memory at 192 MB |
| Interactive queries (page, facet, histogram, text) | 4 to 110 ms | the same within 1.5 ms |
| Caller memory | IPC client under 20 MB | a DuckDB per caller: 29 to 41 MB for the interactive mix, ~700 MB after a 100k-row fetch |
| Server memory at `memory_limit` 192 MB | worker 312 MB querying | 660 MB querying, 2.0 GB importing |
| Cancel | 1 to 102 ms | client interrupt never reaches the server; server burns CPU after the client is gone |
| Progress | yes (in the worker) | none at the client |
| Local exposure | private Unix socket / owner-only pipe, no TCP | TCP port on 127.0.0.1 for every local account; full SQL to any token holder (a client raised the server's memory limit 10× and wrote files) |
| Rust client | ours | none; duckdb-rs prepared statements fail after `CONNECT` |
| Packaging | one pinned libduckdb | plus `quack` (46 MB) and `httpfs` (20 MB) extensions per platform, not in the prebuilt library |


## 1. What Quack is (official sources only)

Every statement in this section is from the sources in §8, read on 2026-10-28; the "Measured here" column is from §3.

| Topic | What the sources say | Measured here |
|---|---|---|
| What it is | A loadable **extension** (`quack`), "DuckDB-Quack protocol for remote access", support tier **Secondary** ("supported on a best-effort basis"). Introduced in 1.5.3 (per the 2.0.0 announcement) as "an initial implementation" (May 2026 blog); 2.0.0 "ships a polished version ... with substantial performance and stability improvements"; the extension's own README still says "released as a pre-release extension and is currently experimental". `CONNECT`/`DISCONNECT` are new 2.0 statements that route a session to a remote database (Quack, PostgreSQL, MySQL); "in-process local databases ... do not support `CONNECT`". | The prebuilt `libduckdb` 2.0.0 (osx-arm64) Debrief links does **not** contain it: `INSTALL quack` downloaded `quack.duckdb_extension` (46 MB, version `f96420ed46`), and both sides also need `httpfs` (20 MB, `77392a7207`): without it the client fails ("required extension 'httpfs'") and `quack_serve` fails ("read-only crypto module loaded"). |
| Transport | HTTP POSTs to `/quack`, client-initiated request/response ("the server does not initiate interactions"); default port 9494; `quack:host:port` URIs. Messages are DuckDB's internal serialization (MIME `application/duckdb`, "the same primitives used in its Write-Ahead Log"), not Arrow. A query is one round trip; large results stream with follow-up `FETCH` requests, optionally in parallel (`quack_fetch_read_ahead`). No Unix domain socket or named pipe: open issue [#233](https://github.com/duckdb/duckdb-quack/issues/233) "Can't serve on a unix domain socket". | Server start (`quack_serve`) 1.0 to 1.6 ms; client attach 50 to 54 ms (§3.2). |
| Auth | A shared token (`quack_serve(token = ...)`, at least 4 characters, else a random one), checked by an authentication callback (`quack_check_token`); authorization callback per query, default `quack_nop_authorization` (allows everything). Callbacks are SQL macros; the docs say the read-only regex example "is illustrative, not a robust read-only filter" and recommend a read-only attach. Open issue [#325](https://github.com/duckdb/duckdb-quack/issues/325): the callbacks "live in a catalog that authenticated clients can write to", so one valid token can disable authorization. "A Quack server exposes the full SQL surface of the underlying DuckDB instance." | A wrong token is refused ("Authentication failed"). With the token and default settings a client raised the server's `memory_limit` from 183 MiB to 1.8 GiB, read a file with `read_text` and wrote one with `COPY ... TO` in the server's folder (§3.5). |
| TLS | "The server does **not** use TLS for local URIs"; non-local URIs default to HTTPS with a self-signed certificate; production advice is a TLS-terminating reverse proxy. Binds `localhost` only unless `allow_other_hostname = true` ("only meant for testing"). | Bound to `127.0.0.1` with a random port in every run. |
| Concurrency, writers | The motivation is several writers: "multiple separate processes ... modify contents of tables in parallel without locking each other out". Small single-row transactions: 1,038 tps at 1 thread to 5,434 at 8 (vendor benchmark, AWS); "we hit a current limitation of DuckDB itself in concurrent insertions per second into the same table". `quack_server_max_connections` 1024. Open issues: concurrent cursors on one connection cancel each other ([#332](https://github.com/duckdb/duckdb-quack/issues)), a statement run at bind time executes twice ([#282](https://github.com/duckdb/duckdb-quack/issues)). | §3.1: 1 writer 110k to 124k events/s; 4 writers ran out of memory at the worker's 192 MB limit in 2 of 3 rounds. |
| Write paths | `INSERT`, DDL and transactions are forwarded to the server; the README says data "can be copied from client to server". No documented Arrow ingest or remote appender; open feature request [#265](https://github.com/duckdb/duckdb-quack/issues) to register Arrow data on the server. | The C appender on an attached Quack table fails twice: "Payload Too Large" unless flushed per batch, then "zero-length delimited identifier" on a value containing `""` (it ships rows as SQL text). The working path is a local staging table plus `INSERT INTO qk.events SELECT * FROM staging`. |
| Streaming, cancel, progress | Results stream in batches of `quack_target_batch_bytes` (32 MiB); the server buffers up to `quack_server_stream_buffer_bytes` per stream (256 MiB, "at most a quarter of `memory_limit`"). `quack_cancel(catalog, connection_id)` cancels on the server. Open issues: client `interrupt()` does not cancel a query running on the server ([#283](https://github.com/duckdb/duckdb-quack/issues/283)); a query keeps running after its client disconnects, "server burns CPU indefinitely" ([#257](https://github.com/duckdb/duckdb-quack/issues/257), confirmed on 2.0.0 by a commenter); no query-execution timeout. | §3.4: cancel and progress measured. |
| Memory limits | No per-connection limit is documented; `memory_limit` is the server's global setting and any client may `SET` it unless the configuration is locked. | Server peak footprint far above `memory_limit` (§3.1, §3.2). |
| Rust client | No Rust client of the protocol. duckdb-rs 1.20000.0 (DuckDB 2.0.0, 2026-10-22) has no Quack feature: the client is libduckdb itself with the `quack` and `httpfs` extensions loaded. The 1.20000.0 release also added `duckdb-neo` (C API v2). | After `CONNECT qk`, every statement prepared through duckdb-rs fails with "INTERNAL Error: PREPARE succeeded but the prepared statement was not registered", **and the statement still ran on the server** (a `COPY` wrote its file). Usable forms: `quack_query_by_name('qk', '<sql text>')` (the whole statement runs on the server; SQL passed as a string, so no bound parameters) or `qk.events` through the attached catalog (planned on the client). |
| Licence | DuckDB and the `duckdb-quack` repository are MIT; duckdb-rs is MIT. | — |
| Platforms | Extensions are distributed per platform from `extensions.duckdb.org`; the docs pages read do not state Quack's platform list. Windows support for a socket transport is an open question in #233. | Only macOS arm64 measured. |

## 2. Setup of the measurements

- **Machine**: Mac mini M4 Pro, macOS, shared with other agents and CI (every run started at a 1-minute load below 40,
  17 to 40; it rose to about 50 during the last concurrency round). Release builds. One benchmark at a time through the shared `heavy.sh` slot.
- **Data**: synthetic only. `crates/debrief-store-duck/tests/bench.rs` generated its corpus (3,000,000 events, 1,095 MB of
  CLEF) and imported it with Debrief's real store (main, DuckDB 1.5.6): 28 segments, 169 MB. The harness converted those
  segments to an Arrow IPC file (3.3 GB, 8,192-row batches, the batch size of the parse threads), so every write variant
  is fed the same parsed batches (reading them alone: 0.48 s, 6.2M events/s, 83 MB).
- **Harness**: `docs/research/2026-10-28-duckdb-server-mode/harness/` (Rust, duckdb-rs `=1.20000.0` on the pinned
  prebuilt libduckdb 2.0.0; `bench.sh` runs the rounds). One process per variant, so each process reports its own peak
  RSS (`getrusage`), peak physical footprint (`proc_pid_rusage` `ri_lifetime_max_phys_footprint`, the number that
  matters on macOS) and CPU time. DuckDB everywhere with the worker's limits: `memory_limit` 192 MB, `threads` 4
  (Quack clients: 256 MB, 2 threads). Servers bound `127.0.0.1` on a random port and were stopped by pid.
- **"Our IPC"** is a model of `debrief-ipc` (ADR 0017): u32 length plus postcard body on a Unix socket, each result
  batch one Arrow IPC stream (schema and batch), as `debrief-store-client/src/arrow_wire.rs` encodes it. It leaves out
  the handshake, credits and cancel frames, so the real peer costs at least this much.
- **Interleaving**: three rounds; each round runs every variant once in a fixed order (A, B, C, A, B, C, ...). Tables
  give the median of the three rounds and the range.
- **Raw output** of every run: `docs/research/2026-10-28-duckdb-server-mode/results/` (the concurrency and cancel
  phases were run a second time after fixing the harness: the first concurrency run imported for under a second and the
  first cancel query finished before the interrupt; those runs are not used).
- **Quack read path**: `quack_query_by_name('qk', sql)`, so the whole statement runs on the server (§1 explains why not
  `CONNECT`: duckdb-rs prepared statements fail after it).

## 3. Results

### 3.1 Import (3,000,000 events)

The harness variants time the **write path only** (batches already parsed); the first row is Debrief's whole import
(parse, segments, catalog, histograms) for scale.

| Variant | Events/s (median, range) | Peak footprint (MB) | CPU (s) |
|---|---|---|---|
| Debrief's import today (bench.rs, parse + write + catalog, DuckDB 1.5.6) | **301k** (278k to 450k) | RSS 992 to 1,116 (its own sampler) | — |
| Parquet segments, 8 writer threads (today's write path) | **4.38M** (2.21M to 4.54M) | 734 to 814 | 3.4 to 3.6 |
| Parquet segments, 12 writer threads | 4.41M (4.33M to 4.56M) | 899 to 928 | 3.5 to 3.7 |
| DuckDB table, 1 in-process appender | 200k (156k to 215k) | 845 to 892 | 17 to 20 |
| DuckDB table, 4 in-process appenders | 355k (144k to 362k) | 1,828 to 1,852 | 23 to 29 |
| Quack, 1 writer (staged `INSERT ... SELECT`), server at 192 MB | **115k** (110k to 124k) | client 1,337 to 1,370 + server 2,033 to 2,071 | client 16 to 18 + server 24 to 28 |
| Quack, 4 writers, server at 192 MB | **out of memory in 2 of 3 rounds**; 349k in the third | server 1,047 to 2,002 | — |
| Quack, 4 writers, server at 1 GB | 416k (394k to 433k) | client 2,534 to 2,612 + server 2,014 to 2,086 | client 16 to 19 + server 16 to 19 |
| Quack, 8 writers, server at 1 GB | 336k (198k to 404k) | client 4,042 to 4,586 + server 2,301 to 2,456 | client 26 to 34 + server 20 to 31 |
| Quack remote appender (C appender on `qk.events`) | **fails**: "Payload Too Large" without per-batch flushes, then "zero-length delimited identifier" on a value containing `""` | — | — |

Reading: today's Parquet write path is **38× faster** than one Quack writer and about 10× faster than the best Quack
setting, with a quarter or less of the memory of the two Quack processes together (about 0.8 GB against 3.4 to 4.6 GB). One Quack writer (115k/s) would be
slower than Debrief's parse threads (the whole import today is 301k/s), so the store would become the bottleneck.
Parallel Quack writers only scale with a server limit five times the worker's 192 MB, and the server's footprint passes
2 GB at either limit: `memory_limit` bounds DuckDB's buffer manager, not Quack's message buffers or the inserts'
local storage. (The in-process appenders show the same: 1.8 GB with 4 appenders at 192 MB. ADR 0014's bake-off
already rejected a native table for these reasons.)

### 3.2 Query latency (ms, median of three rounds; "cold" = first run in a fresh process, OS file cache warm)

| Query | In process cold / warm | Over Quack cold / warm | Over our IPC (model) cold / warm |
|---|---|---|---|
| Page of 500 rows by id | 13.6 / 12.8 | 15.5 / 14.1 | 16.7 / 14.2 |
| Errors page, first 500 ids (sorted) | 20.2 / 18.9 | 19.7 / 19.1 | 20.2 / 19.0 |
| Errors total | 4.8 / 4.4 | 4.6 / 4.1 | 4.9 / 4.2 |
| Facet `MachineName` (top 50) | 110.0 / 110.3 | 108.8 / 110.4 | 109.6 / 110.3 |
| Histogram, 1000 buckets by level | 34.7 / 33.9 | 33.9 / 33.1 | 35.1 / 34.0 |
| Text total, `ILIKE '%timeout%'` | 33.2 / 32.9 | 33.4 / 32.9 | 33.1 / 33.0 |
| Text, first 100 ids | 6.1 / 5.5 | 6.0 / 6.1 | 5.9 / 5.5 |
| Bulk: 100,000 full rows | 94.1 / 85.3 | 128.4 / 117.6 | 219.9 / 213.1 |
| Connect (attach) | — | 50.8 | < 0.1 (handshake not modelled) |

| Process (query run) | Peak footprint (MB) |
|---|---|
| In process (one process) | 237 to 251 |
| Quack: client (a DuckDB in the caller) + server | **681 to 776 + 651 to 673** |
| Our IPC: client + worker | **17 to 19 + 312 to 314** |

Reading: for the interactive queries the transport does not matter; all three are within about 1.5 ms. Quack is faster
than the IPC model for bulk transfer (118 vs 213 ms per 100k rows; the model sends DuckDB's 2,048-row batches one IPC
stream each), but a Quack caller is a whole DuckDB: its peak footprint was 681 to 776 MB in this run (which includes
the 100k-row fetch) and 29 to 41 MB for the interactive mix alone (§3.3), against 17 to 19 MB and 4 to 8 MB for an IPC
client; the server itself peaked at about 660 MB at a 192 MB `memory_limit`. The UI, the AI worker and each plugin
would each pay that client.

### 3.3 Concurrency: interactive queries while an import runs

The mix (page of 500 rows, errors total, facet) repeated every 100 ms while an import ran. In process and over the IPC
model the import is today's Parquet path with 8 writers, repeated into fresh folders for 18 s (one pass takes under a
second); over Quack it is one writer inserting the same 3M events on the server (the only Quack writer that works at the
192 MB limit). Median of three rounds (range of the p50s in brackets); quiet baseline from §3.2.

| Query (ms) | Quiet | In process, p50 / p95 | Our IPC (model), p50 / p95 | Quack, p50 / p95 |
|---|---|---|---|---|
| Page of 500 rows | 13 to 14 | 25.4 (22 to 27) / 45.9 | 44.7 (33 to 89) / 125.1 | 24.8 (18 to 34) / 42.1 |
| Errors total | 4 | 14.4 (13 to 17) / 37.0 | 18.7 (18 to 26) / 41.1 | 15.8 (6 to 28) / 31.8 |
| Facet `MachineName` | 110 | 220.2 (195 to 230) / 289.8 | 269.7 (236 to 554) / 402.2 | 165.5 (138 to 311) / 415.2 (max 1,411) |
| **Events imported during the run** | — | **54M in 19 s (2.9M/s)** | **39M in 19 s (2.0M/s)** | **3M in 27 s (108k/s)** |
| Import CPU (s) | — | 70 to 110 (one process) | 58 to 109 (worker) | 61 to 75 (server) + 17 (writer) |
| Peak footprint (MB) | — | 955 to 1,047 | worker 996 to 1,058 + client 8 | server 1,976 to 2,086 + writer 1,202 to 1,397 + query client 37 to 41 |

Reading: interactive latency under import is in the same range on all three (roughly 2× the quiet numbers, worse when
the machine's load rose in round 3), but the Parquet paths imported **13 to 18 times as many events** in a shorter window (19 to 27 times
the rate) at about the same CPU: per imported event a Quack import costs about 28 s of CPU per million events against about 2 s on
the Parquet path. The third round ran at load 40 to 50 and widened every range; the IPC model's
higher p50 comes mostly from that round. The real import is slower than this synthetic repeat (301k/s with parsing,
§3.1), so the real contention is lower than shown for the Parquet paths.


### 3.4 Cancellation and progress

A query of a few minutes (`... range(2000) ... ILIKE`) was interrupted after 2 s through duckdb-rs's
`interrupt_handle`, while a second thread polled `progress_handle` every 50 ms. Over Quack the client ran it as
`quack_query_by_name`; afterwards the server's CPU was read over 5 s.

| Round | In process: time to stop after interrupt | In process: progress samples | Quack: time to stop after interrupt | Quack: progress samples | Quack server CPU in the 5 s after the client gave up |
|---|---|---|---|---|---|
| 1 | 8 ms | 37 | **still running after 30 s** | 0 | 19.8 s (about 4 cores) |
| 2 | 102 ms | 37 | **still running after 30 s** | 0 | 4.7 s |
| 3 | 1 ms | 37 | **still running after 30 s** | 0 | 19.4 s |

Reading: in process, DuckDB stops within 1 to 102 ms and reports progress (its estimate for this query stays near 0%,
so ER-18's phase-labelled progress still needs Debrief's own counters). Over Quack the client's interrupt neither returns
nor reaches the server, the client gets no progress, and the server keeps working after the client process has exited
(server CPU totals of 111 to 126 s before it was stopped), exactly as issues #283 and #257 describe. A Quack store
would need its own cancel channel (`quack_cancel` on a second connection) and a server-side deadline that Quack lacks.


### 3.5 Security probe (one authenticated client, server and client in different working folders)

| Statement from a client holding the token | Server with defaults | Server after the ADR 0030 lock |
|---|---|---|
| Wrong token | refused ("Authentication failed") | refused |
| `SET memory_limit = '2GB'` | **accepted: the server's limit went from 183.1 MiB to 1.8 GiB** | refused ("the configuration has been locked") |
| `SELECT current_setting('enable_external_access')` | `true` | `false` |
| `read_text('<a file outside the store>')` | **read** | refused (Permission Error) |
| `COPY (SELECT 42) TO '<relative path>'` | **wrote the file into the server's working folder** | refused |
| `quack_active_connections()` | listed the connections (and each one's last query, per the reference) | the same |
| `duckdb_extensions()` | `httpfs`, `quack`, `parquet`, `json`, `icu`, ... loaded | refused (no file access) |
| Typical reads (`query-quack`) | — | all eight queries work |
| `CONNECT qk` then any prepared statement | internal error on the client, **yet `COPY` ran on the server** (its file appeared) | internal error |
| `ATTACH` without `CONNECT` (`qk.events`) | unqualified statements run in the **client**, not the server | the same |



## 4. Design fit: Quack as Debrief's store architecture

The idea in ADR 0010: the store worker becomes a Quack server; the UI, the AI worker and plugins query it directly, and
Debrief's IPC drops out of the store path.

| Concern | Today (worker + `debrief-ipc`) | With a Quack server | Verdict |
|---|---|---|---|
| **What crosses the boundary** | Typed engine operations (`StoreRequest`: page, facet, timeline, search, analyses ...; `debrief-store-client/src/protocol.rs`); the Rust core computes, the caller renders | SQL text. The UI, the AI worker and plugins would build SQL, or the worker keeps its operations and Quack only replaces the transport | Direct SQL from the UI breaks "the Rust core is the single source of truth" (AGENTS.md, *Where logic lives*); Tantivy, the catalog, paging, caps and the analyses live in the worker anyway. So the worker and its protocol stay, and Quack would only be a second transport. |
| **Client** | `debrief-store-client` links no database ("so the app does not load libduckdb", its Cargo.toml); client footprint < 20 MB | Every caller links libduckdb (65 MB) and loads `quack` (46 MB) and `httpfs` (20 MB); measured client peak footprint 29 to 41 MB for interactive queries, 681 to 776 MB once a 100k-row result is fetched, 1.3 GB for one writer | Worse: the UI process would load DuckDB again, and every caller's memory grows with the results it fetches. |
| **Rust support** | Our code | No Rust client; duckdb-rs through libduckdb. After `CONNECT`, every duckdb-rs prepared statement fails with an internal error **while still executing on the server** (measured: a `COPY` wrote its file). Workable only as `quack_query_by_name('qk', '<sql string>')`: no bound parameters, so every value is spliced into SQL text | Fails ADR 0010's own precondition ("duckdb-rs ... supports it"). |
| **Transport and exposure** | Unix socket in a fresh 0700 folder / owner-only named pipe with `FIRST_PIPE_INSTANCE`, 256-bit per-launch token, peer pid checked, "never TCP" (ADR 0017 §3) | TCP on 127.0.0.1 only (#233: no Unix socket); any local process and **any other local user** can connect to the port and try tokens; no TLS for local URIs, so the token crosses loopback in clear | Worse. Loopback is not readable without root, but the port is reachable by every account on the machine. |
| **Authorization** | The worker answers only its protocol; the SQL console runs in a locked, read-only connection (ADR 0030) | Default: any authenticated client has "the full SQL surface". Measured: a client raised the server's `memory_limit` from 183 MiB to 1.8 GiB, read a file (`read_text`) and wrote one (`COPY ... TO`). The ADR 0030 lock (`allowed_directories`, `enable_external_access = false`, `lock_configuration = true`) applied on the server after `quack_serve` blocks all three (measured) and reads still work, but then the server cannot write Parquet, and the authorization callbacks are SQL objects that an authenticated client can replace (#325) | Worse; the lock is necessary and still not sufficient (DDL and DML on the catalog stay open to every token holder). |
| **Plugin sandbox** | Plugins talk to the store worker over the same private endpoint; their sandbox denies network sockets (ADR 0017 §6: seccomp `AF_INET` → `EACCES` on Linux, `network*` denied on macOS) | A plugin would need TCP to reach the server, so the sandbox would have to allow loopback networking | Worse: widens every plugin's sandbox. |
| **Crash isolation** | Worker process supervised by `debrief_ipc::Supervisor` (restart, backoff, restore, quarantine) | Same: the server is still a child process Debrief must supervise (Quack has no supervisor; `quack_serve` runs inside a host process, foreground mode is a feature request, #206) | Equal at best; the supervisor stays. |
| **Cancellation (ER-17)** | `Cancel` frame → `QueryControl` + DuckDB interrupt in the worker (ADR 0014 bake-off: 6 ms after the interrupt on Parquet) | Client `interrupt()` does not reach the server (#283); a query keeps running after its client disconnects (#257). `quack_cancel(catalog, connection_id)` exists but needs a second connection | Worse (§3.4). |
| **Progress (ER-18)** | duckdb-rs `progress_handle` on the worker's connection (2.0; ADR 0014 follow-up) | The client's progress handle reports nothing for a remote query (§3.4) | Worse. |
| **Memory caps** | One process to cap: `memory_limit` 192 MB plus the host's resident-memory watcher | No per-connection limit; the server's footprint passed 2 GB at a 192 MB limit while importing, ~660 MB while querying; clients add their own | Worse. |
| **Write path** | Parse threads write zstd Parquet in parallel, no database in the hot path | Rows must go through DuckDB inserts on the server: 115k/s with one writer, out of memory with four at 192 MB | Much worse (§3.1). |
| **Platforms and packaging** | One pinned, checksummed prebuilt libduckdb per platform; no extension bundled or loaded (ADR 0030); Windows uses named pipes | Two more native binaries per platform (`quack`, `httpfs`), signed by DuckDB but downloaded per version and platform; Debrief would have to pin, ship and `LOAD` them from a fixed path and allow extension loading in the worker; Windows TCP only | Worse: more to pin, sign-check and ship; contradicts ADR 0030's no-extension stance. |
| **Maturity** | — | Secondary support tier ("best effort"); README "pre-release ... experimental"; 45 open issues, among them cancel, timeouts, auth and correctness (§1) | Not yet production-grade for a local desktop store. |
| **Licence** | DuckDB MIT, duckdb-rs MIT | `duckdb-quack` MIT | Equal (all on `deny.toml`'s allow-list). |

What Quack does better: bulk result transfer (118 ms per 100k rows against 213 ms for the IPC model) and parallel writers
from several processes, a feature Debrief does not need (one importer per store, and the parse threads already write in
parallel without a database).


## 5. Recommendation

**Don't adopt** Quack as the store architecture, and don't adopt it partly either: there is no part of Debrief where it
wins on what matters (write throughput, memory caps, the local security boundary, cancel and progress), and the one
thing it does better, bulk transfer, is cheaper to get inside today's IPC. Ranked:

1. **Keep today's design** (Parquet segments from the parse threads, a supervised store worker, `debrief-ipc`).
   Numbers: 4.4M events/s on the write path against 115k/s for one Quack writer; identical interactive latency (within
   1.5 ms on every typical query); IPC client under 20 MB against about 700 MB for a Quack client; a private socket
   against a TCP port open to every local account.
2. **Fallback, if a remote or multi-machine store is ever wanted** (not a current requirement; Debrief is local-only per
   ADR 0010): a read-only Quack endpoint on a copy or export of a store, behind the ADR 0030 lock, for a separate
   "share this workspace" feature, never as the app's own store path. Re-evaluate only then.
3. **Rejected**: the ADR 0010 plan (worker replaced by DuckDB's server, supervised like `llama-server`). It fails three
   of its own preconditions today: Quack is not stable (secondary tier, pre-release README), duckdb-rs does not support
   it (prepared statements fail after `CONNECT`), and nothing shows it stable on Windows (TCP only; not measured).

Risks of not adopting: low. DuckDB 2.0 itself (async I/O, spill, progress) is adopted on `debrief/duckdb-2`
independently of Quack. If DuckDB later drops features Debrief uses in favour of the server mode (no sign of that in
the sources), the `LogStore` port keeps the change an adapter change, as ADR 0010 says.

## 6. Revisit checklist (DuckDB 2.1 or later)

Re-run `harness/bench.sh` (all phases) when a DuckDB release notes any of these; reconsider only if all pass:

1. Quack serves on a Unix domain socket / named pipe (#233) and is a primary-tier or built-in extension of the
   prebuilt library.
2. duckdb-rs prepared statements work after `CONNECT` (the harness's `probe` with `QH_STYLE=connect`).
3. Client interrupt and client disconnect cancel the server's query (#283, #257): `bench.sh cancel` shows cancel within
   100 ms and no server CPU afterwards; progress reaches the client.
4. A per-connection or per-server memory cap that holds: server peak footprint within 1.5× `memory_limit` during
   `bench.sh import` and `query`.
5. Authorization that clients cannot change (#325) and a built-in read-only or statement-type allowlist.
6. A write path within 2× of today's Parquet path at the 192 MB limit.

## 7. Work packages and decisions

| WP | What | Files | Size | Depends on |
|---|---|---|---|---|
| Q1 | Amend ADR 0010 §"Later: DuckDB 2.0 and its server mode": from "migrate to it" to "do not migrate; revisit with the checklist of this note" | `apps/debrief/docs/adr/0010-persistent-store.md` | XS (docs) | user decision D1 |
| Q2 | Measure the real `debrief-ipc` bulk path (the model needs 213 ms per 100k rows against 118 ms over Quack and 85 ms in process): batch DuckDB's 2,048-row vectors into larger IPC items (one schema per stream, not per batch) and measure `rows()`/export over the real peer | `crates/debrief-store-client/src/arrow_wire.rs`, the worker's scan handler; a bench in `debrief-store-worker` | S (research + small change) | none |
| Q3 | Keep the harness for the revisit: move `harness/` under the research folder as committed (done here), add a line to the DuckDB upgrade checklist to re-run it | this note's folder; `docs/research/2026-10-duckdb-2-and-large-data.md` follow-ups | XS | Q1 |

Decisions for the user:

- **D1**: accept "don't adopt" and amend ADR 0010 (Q1)? Recommended: yes.
- **D2**: is a shared or remote workspace (the fallback in §5.2) a goal at all? If not, the revisit checklist can be
  dropped too.


## 8. Sources (read 2026-10-28)

Official:

- DuckDB blog, "Quack: The DuckDB Client-Server Protocol" (2026-05-12): https://duckdb.org/2026/05/12/quack-remote-protocol.html
- DuckDB blog, "Announcing DuckDB 2.0.0" (2026-10-21): https://duckdb.org/2026/10/21/announcing-duckdb-200
- Quack overview: https://duckdb.org/docs/2.0/quack/overview
- Quack security: https://duckdb.org/docs/2.0/quack/security
- Quack reference (functions, settings, `ATTACH` options): https://duckdb.org/docs/2.0/quack/reference
- Quack troubleshooting: https://duckdb.org/docs/2.0/quack/troubleshooting
- Quack setup overview and deployment: https://duckdb.org/docs/2.0/quack/setup/overview, https://duckdb.org/docs/2.0/quack/setup/deployment
- `quack` core extension page: https://duckdb.org/docs/2.0/core_extensions/quack.html
- Core extensions overview (support tiers): https://duckdb.org/docs/2.0/core_extensions/overview
- `CONNECT` statement: https://duckdb.org/docs/2.0/sql/statements/connect.html
- duckdb-quack repository (licence MIT, README "pre-release ... experimental", 45 open issues): https://github.com/duckdb/duckdb-quack
- Issues read: #233 https://github.com/duckdb/duckdb-quack/issues/233, #257 https://github.com/duckdb/duckdb-quack/issues/257,
  #283 https://github.com/duckdb/duckdb-quack/issues/283, #325 https://github.com/duckdb/duckdb-quack/issues/325; the
  titles of #206, #225, #246, #259, #265, #277, #282, #318, #319, #322, #326, #332 from the issue list
  https://github.com/duckdb/duckdb-quack/issues (titles only, not read in full: **unverified** beyond their titles)
- duckdb-rs releases (1.20000.0, 2026-10-22): https://github.com/duckdb/duckdb-rs/releases

Quotes come from pages fetched through a tool that returns page text and summaries; the quoted sentences were
checked to appear in its output, not against a browser rendering.

Not official, not relied on: MotherDuck's blog post and the third-party Elixir client `quackdb` (which calls the
protocol experimental) appeared in the search; neither is used above.

Debrief and Paddock: ADR 0010 (§"Later: DuckDB 2.0 and its server mode"), ADR 0014 (storage bake-off, worker limits;
the DuckDB 2.0 amendment and feature survey on branch `debrief/duckdb-2`, read with `git show`), ADR 0017 (one IPC,
§3 transport, §6 sandbox), ADR 0030 (console sandbox, no extensions), `docs/research/2026-10-duckdb-2-and-large-data.md`,
`crates/debrief-store-duck/src/{lib.rs,segments.rs}`, `crates/debrief-store-client/src/{arrow_wire.rs,protocol.rs}`,
`crates/debrief-ipc/src/lib.rs`, `crates/debrief-store-duck/tests/bench.rs`.
