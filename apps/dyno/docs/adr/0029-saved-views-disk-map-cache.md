# 29. Saved views: the disk map (and slow lists) are kept in a cache database, shown at once with their age

- Status: Proposed (2026-11-03)
- Date: 2026-11-03
- Revision 2 (2026-11-03): answers the independent review of `488a62e44` (P1: kept scans against `evict`, the wire style
  and the missing contract tests; P2: parser bounds, like-for-like figures, staleness wording, privacy of freed pages and
  the cache folder, saving from a finished snapshot, the estimate). The reviewer's answers to D1 to D8 are recorded with
  each decision; the user decides after the delta review.
- Related: ADR [0008](0008-storage-sqlite-and-retention.md) (SQLite in the agent, single writer; this record adds a second,
  rebuildable file beside `dyno.db`), ADR [0021](0021-monitoring-only.md) (read-only), `dyno-core/src/disk/scan.rs`
  (the scan as a job), ADR 0027 on branch `docs/dyno-helper-1-1` (the 1.1 helper, for the Windows change journal later), family ADR 0041 on `origin/docs/redesign-adrs-2-4-6` (proposed rule 60, tests control
  time); rules 10 to 12 (ER-17 to ER-19), 26, 48, 51 (ER-20), 52; ER-14, ER-27.
- Trigger: the user, 2026-11-03: "Cache UI data (performance counters, disk map) so reopening doesn't rescan." Decided by
  the user: SQLite in the agent, not DuckDB.

## Context

What is kept today, read in `origin/main` `a18e18801` (paths under `apps/dyno/crates/`):

| View | Where it lives now | Survives a closed window | Survives an agent restart (login, update, crash) |
| --- | --- | --- | --- |
| System history (CPU, memory, disk, network, apps, events) | `dyno.db` tiers, `dyno-agent/src/store.rs` (ADR 0008) | yes | **yes** (already persisted) |
| Disk map scans | `DiskCentre.scans` in memory, `dyno-agent/src/disk/centre.rs:275`; at most `MAX_KEPT = 3` scans and `MAX_KEPT_NODES = 8,000,000` nodes; the UI adopts them through `DiskScans` (`app/src/app/state/disk-map.store.ts`, `restore()`) | yes (the agent keeps running) | **no**: every scan is lost and the volume is walked again (minutes on a full SSD, much longer on a share or HDD) |
| Counter catalog (the objects list) | `CounterCentre.catalog`, in memory, `CATALOG_TTL` 5 min, `dyno-agent/src/counters.rs:19` | for 5 min | no (up to `CATALOG_DEADLINE` 10 s to enumerate again on Windows) |
| Counter history | rings in memory, `MAX_RINGS = 48`, at most 60 min (`counters.rs:27-31`) | yes | no |
| Installed software (Extras) | in memory, `SOFTWARE_TTL` 10 min, `dyno-agent/src/extras/mod.rs:23` | for 10 min | no |
| Hardware inventory | in memory, warmed at start, `dyno-agent/src/inventory.rs` | yes | no (warmed again in the background) |
| Facts, unusual-startup list | in memory, 5 min (`agent.rs:44`, `unusual.rs:20`) | for 5 min | no (cheap) |

So "reopening rescans" is, precisely: the window itself already reattaches to what the agent kept, but the agent keeps
nothing of the disk map, the counter catalog or the software list across its own restart, and the disk map keeps only the
three newest scans. The disk map is the expensive one; the others cost seconds.

The history store is the wrong home for a disk tree. `Store` is one `rusqlite::Connection` inside `Core`, behind the agent's
core lock (`agent.rs:151-153`); a 100 MB tree written there would hold the lock the sampler takes every tick. A tree is also
rebuildable data, while `dyno.db` is the user's history that ADR 0008 moves aside rather than deletes.

A disk scan's index (`dyno-core/src/disk/index.rs`) is an arena: about 80 bytes per node plus the name bytes, at most
`ScanLimits::max_nodes` (default 2,000,000, hard cap `HARD_MAX_NODES` 5,000,000) per scan. Sizes add up from the leaves at
every moment, so a cancelled or failed scan is still a consistent (partial) tree, and each node knows whether it is
complete.

## Decision

### 1. What is saved

1. **Disk map scans**, per target (a volume by id, or a folder by path): the newest pass only, with its index, its status
   (state, completeness: `truncated`, unreadable folders, `needs_full_disk_access`, `hidden_bytes`), the limits it ran
   with, `scanned_at`, the volume's identity and used bytes at that time. Done scans and, marked as partial, cancelled or
   failed ones (D4). A scan is saved when its pass ends (`WalkHooks::ended`, `centre.rs`), never while it runs.
2. **Small slow lists** as "last known" documents (D7): the counter catalog and the installed-software list, each one
   JSON document with its format number and time. They are shown at once and refreshed in the background as now; the
   in-memory TTLs stay.
3. **Not saved:** system history (already in `dyno.db`), counter rings (60 minutes of live readings; the pinned counters
   are polled in the background anyway), facts and the startup list (cheap), the hardware inventory (warmed at start).

### 2. The store: a second SQLite file, `cache.db`, owned by one writer thread

- **Crate:** `rusqlite` 0.40 with `bundled`, the one Dyno already uses (`dyno-agent/Cargo.toml:30`; `libsqlite3-sys`
  0.38.2 in `apps/dyno/Cargo.lock`). No new licence, no system SQLite, statically linked. Same pragmas as ADR 0008 (WAL,
  `synchronous = NORMAL`) plus `auto_vacuum = INCREMENTAL` set at creation, so eviction can give the space back.
- **Where:** the OS cache folder, not the data folder: a new `Dirs::cache_db()` (`dyno-agent/src/paths.rs`) =
  `~/Library/Caches/Dyno/cache.db` (macOS), `$XDG_CACHE_HOME/dyno/cache.db` or `~/.cache/dyno/cache.db` (Linux),
  `%LOCALAPPDATA%\Dyno\Cache\cache.db` (Windows, not roamed). This keeps it out of backups: `~/Library/Caches` is
  left out of Time Machine by default (the reviewer's statement, not checked here), and WP-2 also sets the item's backup
  exclusion on macOS so that does not depend on it. With `DYNO_DATA_DIR` set (tests, ER-14) the cache is
  `<DYNO_DATA_DIR>/cache/cache.db`. Unix: the folder `0700`, the file `0600`.
- **Single writer:** a `dyno-cache` thread in the agent owns the only write connection and takes save, forget and evict
  commands over a bounded channel (capacity 4; a save that finds the channel full drops the older save of the same target,
  never blocks the walk). It runs at the background level: nobody waits for a save (rule 52 allows it; a *load* is waited
  for and runs on the requesting thread with a read-only connection, WAL lets it read beside the writer). The app never
  opens the file (ADR 0008's rule).
- **Schema** (`meta.schema` = 1, the same version handling as `Store::try_open`):

  ```sql
  CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
  CREATE TABLE disk_saved (
    id INTEGER PRIMARY KEY,
    target_key TEXT NOT NULL UNIQUE,     -- 'volume:<id>' or 'folder:<path>' (user data, section 6)
    target TEXT NOT NULL,                -- DiskTarget as JSON
    volume_identity TEXT,                -- the volume's UUID / serial where the OS gives one (WP-3 checks each OS)
    status TEXT NOT NULL,                -- DiskScanStatus at the end of the pass, JSON (serde defaults = forward-compatible)
    limits TEXT NOT NULL,                -- ScanLimits as JSON
    scanned_at INTEGER NOT NULL,         -- wall ms, when the pass ended
    saved_at INTEGER NOT NULL,           -- wall ms, when this row was written
    used_bytes_then INTEGER,             -- the volume's used bytes when the pass ended (whole-volume scans)
    record_format INTEGER NOT NULL,      -- the node record layout (section 3)
    nodes INTEGER NOT NULL, stored_bytes INTEGER NOT NULL,
    last_shown_at INTEGER NOT NULL       -- for eviction
  );
  CREATE TABLE disk_chunks (saved INTEGER NOT NULL REFERENCES disk_saved(id) ON DELETE CASCADE,
                            seq INTEGER NOT NULL, data BLOB NOT NULL, PRIMARY KEY (saved, seq)) WITHOUT ROWID;
  CREATE TABLE view_saved (key TEXT PRIMARY KEY, format INTEGER NOT NULL, saved_at INTEGER NOT NULL, data TEXT NOT NULL);
  ```

- **Build new, then swap (rule 11):** a save inserts the new `disk_saved` row and all its chunks and deletes the old row of
  the same `target_key` in **one transaction**. A crash or a full disk leaves the old saved map intact.
- **Migration:** numbered forward steps in one transaction, as ADR 0008. Because the file holds only rebuildable data, a
  schema **newer** than known (an older Dyno after a downgrade) or a file that is not a usable database is **deleted and
  recreated** together with its `-wal` and `-shm` files, not moved aside (unlike `dyno.db`); the agent logs it (ER-27: cause and next step) and adds one activity
  entry ("Saved disk maps could not be read and were cleared; scan again to see a map"). Busy or locked is not damage
  (`store.rs` `is_busy`): caching is switched off for this run and the file is left alone. A data folder that is not
  writable means no caching, said once in the activity log; the disk map works as in 1.0.
- **Recoverable per row:** a saved map whose chunks do not parse (section 3) is dropped alone, logged with its row id,
  never with its path.

### 3. The tree layout: packed, versioned records in 4 MiB chunks

The index is written as one byte stream: a header (format number, node count, name bytes, limits, totals), then one
fixed-size little-endian record per node in arena order (parent, kind, flags, name length, the size and count fields,
`newest`; `first_child` / `next_sibling` / `open` are rebuilt, not stored), then the names. The stream is cut into chunks of at
most 4 MiB of uncompressed bytes each (D3: compressed with `flate2` level 1 if WP-5 confirms; `flate2` 1.1.10 is already in
Dyno's lockfile, with both the `miniz_oxide` and `zlib-rs` back ends, and `zip` uses it with `zlib-rs`; WP-5 measures the
back end Dyno would get).

- **Parse, don't validate:** `DiskIndex::from_records` in `dyno-core` (pure) turns the bytes into an index or a typed
  error, never panics on any input, and checks every bound: node count within `HARD_MAX_NODES` and the stored limits,
  every parent earlier than its child, name ranges inside the name block, name lengths within `MAX_NAME`, kinds known,
  and the stored totals equal to the totals it recomputes. `to_records` is its inverse.
- **Bounds before allocation:** the header's node count and name bytes are checked against the row's `nodes` and
  `stored_bytes` columns and against `HARD_MAX_NODES` **before** anything is allocated from them; buffers grow from what
  was actually read, never reserved from a header. Each chunk inflates into at most 4 MiB (a chunk that would inflate
  further is an error), the whole stream into at most the row's `stored_bytes`, and the expansion ratio of any chunk is
  limited to 64:1 (a zip-bomb guard; real records compress far less). Every limit has a broken-stream test.
- **Why not one row per node:** measured on a synthetic 1M-node tree ([performance.md](../performance.md#2026-11-03-a-saved-disk-map-tree-of-1m-nodes-in-sqlite-rows-against-packed-chunks-docsdyno-cached-views)):
  like for like (read and decode to node values), chunks take 0.135 s (0.042 s read plus 0.093 s decode) against 0.89 s for
  rows, about 6.5x, and write in 0.51 s (with packing) against 1.24 s; Python numbers, repeated in Rust by WP-5. Rows would
  allow answering a level straight from SQL without loading the tree, but every other operation (rings, folding,
  retry, paths for Reveal) works on the in-memory index, and a second query engine over SQL would duplicate them (D2).
- **Loaded lazily:** at agent start only the `disk_saved` rows are read (a few rows of JSON) and offered as kept scans;
  the tree is loaded when the UI first asks for a level of it. Saved maps that are not loaded cost no memory.
- **Kept scans and eviction (changes `DiskCentre::evict`, `centre.rs:650` on `a18e18801`):** today `evict` removes whole
  entries once there are `MAX_KEPT` (3) or more, so up to 16 saved maps would be dropped at once and their scan ids would
  answer "no longer kept". An `Entry` therefore holds its tree as `Loaded(Arc<Walk>)` or `Saved(saved row id)`;
  `MAX_KEPT` and `MAX_KEPT_NODES` count **loaded** trees only; `evict` turns the oldest finished entry that has a saved
  row back into `Saved` (unloads it, keeps its id and status) and removes only entries that have no saved row (a scan
  too big to keep, or caching off). A request on an unloaded entry loads it again (a load that fails, for example a
  damaged row, answers the existing "Scan again" error and drops the row). Scan ids of saved maps are assigned at
  agent start from the same counter as live scans.
- **Saving reads a finished snapshot, never under the index lock:** when a pass ends, the walk's index is final and
  shared read-only (`Arc`); the writer thread serialises from that `Arc` in bounded steps (a chunk at a time), never
  inside `Walk::with_index` across up to 5M nodes, so queries on the same scan are not held up. A retry or refresh that
  starts while a save is pending produces a new index; the pending save keeps the old `Arc` and is superseded by the
  next one for the same target.

### 4. Bounded size and eviction

- **One saved map per target**, at most **16 targets** (D6).
- **A total cap**, Setting `disk.savedMaps.maxMb`, default **1024 MB** (Auto), bounded 64 MB to 8 GB. Before a save, the
  least recently shown saved maps (`last_shown_at`) are evicted until the new one fits; then `PRAGMA incremental_vacuum`
  runs in the writer thread in small steps. A map larger than the cap on its own is not saved, and its status says so
  ("Too big to keep; raise the limit in Settings").
- **Free space:** no save when the data volume would keep less than 2 GB or 5 % free afterwards.
- **Off switch and clearing:** Setting `disk.savedMaps.enabled` (default on). "Forget saved maps" in Settings and
  "Forget this map" on the page are agent commands; turning the setting off forgets them all. (Adopting
  `paddock-storage` for a family Storage page is separate work; Dyno does not use it yet.)

### 5. Staleness: show at once with its age, refresh on demand, swap when done

- At agent start, each saved map is a kept scan with `origin: saved`. The UI's existing `restore()` shows it at once with
  the line it already draws from `scannedAt` ("Scanned 3 days ago"), plus "saved" and, for a whole-volume scan, the **net
  change in used space** since the scan (`changedBytes` = used now minus `used_bytes_then`, computed in the agent; a net
  figure: 10 GB deleted and 10 GB written show as no change, and the UI says "net"). Folder scans show only their age:
  there is no cheap used-space figure for a folder. The
  engine computes these; the UI only formats them.
- A saved map whose volume identity differs from the volume now mounted under that id is not offered ("A different disk
  is mounted here"); it stays until evicted, in case the other disk returns.
- **Refresh** (the page's Rescan) starts a new scan of the same target as a replacement: the saved map stays on screen,
  readable, with a phase-labelled progress strip (rule 11); when the new pass is **Done** the page swaps to it and the
  save replaces the old row. Cancel keeps the old map on screen and saves nothing; a failed refresh keeps the old map and
  shows the failure. This changes today's Rescan, which switches the page to the growing tree at once; the growing tree
  stays one click away ("Show progress") for those who want to watch it. This is a UX change for the user to decide (D9).
- **No background refresh by default** (D5). Scanning a volume is the heaviest thing Dyno does and it is a monitor; the
  `changedBytes` hint tells the user when a refresh is worth it. Setting `disk.savedMaps.refreshOnOpen` (off | older than
  1 day | older than 7 days) for those who want it; when on, the refresh starts at the first look at the page, never on
  a timer in the background.

#### Incremental refresh, honestly scoped

| Approach | What it gives | Why not in 1.1 |
| --- | --- | --- |
| Folder mtime checks | Skips re-listing folders whose own mtime is unchanged | A folder's mtime changes when an entry is added, removed or renamed, **not** when a file inside grows (logs, VM images, databases), so sizes would be wrong; checking every file's size again costs what a scan costs (a scan is listing and stat). Rejected. |
| macOS FSEvents history (`FSEventStreamCreateRelativeToDevice` with `sinceWhen` = the event id stored with the scan, checked against `FSEventsCopyUUIDForDevice`) | The folders changed since the scan, also across reboots; re-read only those, patch the index, propagate sizes up | Needs a new index operation (replace one folder's listing), a binding (`fsevent-sys` has `FSEventsCopyUUIDForDevice` commented out, per docs.rs), and it misses changes made while the volume was mounted by another OS (reported on Stack Overflow, unverified). Planned as WP-7 (1.2), falling back to a full scan when the UUID changed, history was purged or `MustScanSubDirs` arrives. |
| Windows NTFS/ReFS change journal (`FSCTL_QUERY_USN_JOURNAL` / `FSCTL_READ_USN_JOURNAL`) | Changed files since a stored USN and journal id | Microsoft documents that change-journal operations need administrator rights; the agent never runs elevated (ADR 0006), so only through the 1.1 helper as a new read request. WP-8, after the helper lands; full scan when the journal id changed or the USN was overwritten. |
| Live watchers (`ReadDirectoryChangesW`, inotify, `notify` / `paddock-watch`) | Changes while the agent runs | Nothing across an agent restart, which is the case this record solves; a recursive watch over a volume is costly (inotify needs one watch per folder, RDCW drops events on overflow). Rejected. |
| Linux | — | Full scan only (no unprivileged persistent journal). |

### 6. Privacy (ER-27, ADR 0008 §6)

- Paths and names in `cache.db` are the user's data: kept on this machine only, never uploaded, never in the diagnostics
  bundle (`dyno-agent/src/diagnostics.rs` collects named items; `cache.db` is not added), the file `0600` on Unix, kept in
  the cache folder and so out of backups (section 2).
- Forgetting leaves no path in freed pages: the write connection runs with `PRAGMA secure_delete = ON` (freed pages are
  overwritten with zeros) and checkpoints the WAL with `wal_checkpoint(TRUNCATE)` after a forget or an eviction, so old
  pages do not linger in `-wal`. "Forget saved maps" and turning the setting off close the connection and delete
  `cache.db`, `-wal` and `-shm` outright, then start an empty file if caching stays on. A test forgets a map with
  distinctive invented names and asserts the bytes of `cache.db` and `-wal` no longer contain them.
- Logs carry the saved row id, scan id, target kind (volume or folder, never the path or volume name), node count, bytes,
  durations and outcomes; this matches `centre.rs`'s `state_name` rule. A test captures the log of a save, load, evict and
  damaged-file run over a tree with invented, distinctive names and asserts none of them appears.

### 7. Wire changes (mirrored to TypeScript in the same change)

- `DiskScanStatus` gains `origin: ScanOrigin`, in the house style of `ScanState` (a unit variant is a string, a variant
  with fields an object): TypeScript `origin: 'live' | { saved: { savedAt: Timestamp } }`, Rust
  `enum ScanOrigin { Live, Saved { saved_at: Timestamp } }` with the same serde attributes as `ScanState`, `changedBytes:
  number | null` and `replaces: number | null` (the scan a refresh will replace). `scannedAt` keeps its meaning.
- New requests: `ForgetSavedDiskMap { scan }`, `ForgetSavedDiskMaps`; `StartDiskScan` gains `replaces: Option<u64>`.
- `view_saved` documents add `savedAt` to the counter catalog and software list replies (`CounterObjectList`,
  `SoftwareInventory`): `savedAt: Timestamp | null` (null when read just now).
- The disk map is mirrored through the existing contract test: `dyno-core/tests/disk_wire.rs` writes
  `app/src/app/core/disk-map.wire.json`, `app/src/app/core/disk-map.contract.spec.ts` checks it against `api.types.ts`.
  There is **no** counters or extras contract test on main (only `disk_wire.rs` and `jobs_wire.rs` in `dyno-core/tests/`):
  WP-6 creates `counters_wire.rs` and `extras_wire.rs` with their `.wire.json` files and contract specs in the same shape
  before adding `savedAt`. The mock backend gains a saved map.

### 8. Performance (ER-20, rule 51)

- Parallelism: saving and loading are single-threaded by nature (one SQLite connection, one stream); the cost lever is
  the cheaper format (section 3), measured. A load is waited for, so it runs at the live level (rule 52); a save at the
  background level.
- Budgets (asserted by the WP-5 bench in release builds): start-up listing of saved maps < 20 ms; first level of a 1M-node
  saved map < 300 ms from the request; saving 1M nodes < 2 s off the walk and sampling threads; footprint of a loaded
  map within 10 % of a live one of the same size (rule 26).

## Consequences

- After a reboot or an update the disk map opens on the last map at once, with its age; rescans happen when the user asks.
- A second database file to look after, but one that may always be deleted.
- `DiskIndex` gets a stable on-disk record format with its own number; a change to `Node` must bump it (a test pins the
  record size and field list).
- The page's Rescan changes behaviour (old map stays until the new one is done), which the user guide and features.md
  describe.

## Tests

All time comes from the centre's `Clock` (`centre.rs:79`) and a `FakeClock` for the cache's ages and eviction order;
nothing sleeps or asserts elapsed time (proposed rule 60, family ADR 0041). Databases are in temp folders or in memory.

- `dyno-core`: round trip `to_records` / `from_records` on generated trees (property test) gives identical `children`,
  `rings` and `path_components` answers; partial, truncated and refused-folder trees keep their completeness flags; a
  fuzz target and a table of broken streams (cut short, parent after child, name out of range, count over the cap, a
  header count larger than the row's `nodes`, a chunk that inflates past 4 MiB or past 64:1, totals that disagree) each give a typed error, never a panic; a test pins the record size.
- `dyno-agent` cache: creates, migrates, refuses-and-recreates a newer schema, recreates a file of garbage bytes, drops a
  single damaged row, leaves a busy file alone; the swap keeps the old map when the save transaction fails half-way
  (injected error); eviction by `last_shown_at` on the fake clock and by the size cap; no save under the free-space floor
  (a fake free-space port); the writer drops an older pending save of the same target.
- Centre: a saved map is offered at start without loading its tree, loaded on the first level request, unloaded under
  `MAX_KEPT_NODES`; **16 saved maps** at start are all listed by `DiskScans`, every one answers a level request in turn
  (each load unloads the oldest loaded one, never removes it), and no id ever answers "no longer kept"; a save of a
  5M-node fake index does not hold `with_index` (a query on the same scan completes while the save is parked at a test
  `Gate`); a refresh keeps the saved map until Done, Cancel keeps it, failure keeps it; a different volume
  identity hides it; the privacy log test (section 6).
- UI (Vitest, rule 49): the page shows a saved map with "saved" and its age at once, keeps it during a refresh and swaps
  on Done, keeps it on Cancel, Forget removes it; the contract specs for the new fields.

## Work packages

| WP | What | Owner paths | Agent-days |
| --- | --- | --- | --- |
| WP-1 | Record format: `DiskIndex::to_records` / `from_records`, validation, property tests, fuzz target | `dyno-core/src/disk/index*`, `dyno-core/tests/`, `dyno-core/fuzz/` | 1.5 |
| WP-2 | `cache.db`: schema, open/migrate/recreate, the writer thread, cap, eviction, incremental vacuum, free-space floor, `0600`, FakeClock tests | `dyno-agent/src/cache/` (new), `paths.rs` | 2 |
| WP-3 | Centre integration: `Loaded`/`Saved` entries and the new `evict`, save from the finished snapshot, offer at start, lazy load, refresh-as-replacement, forget, privacy log test | `dyno-agent/src/disk/centre.rs`, `disk/mod.rs` | 2.5 |
| WP-3b | Volume identity on three OS (APFS/HFS volume UUID, Windows volume serial or GUID path, Linux filesystem UUID), each tested on its own runner, with the "different disk" state | `disk/volumes.rs`, `dyno-platform` | 1.5 |
| WP-4 | Wire, TS and UI: `origin`, `changedBytes`, `replaces`, forget requests, Settings keys, the Rescan UX (old map until Done, "Show progress", Cancel keeps it), mock, contract specs, user guide | `dyno-core/src/disk/scan.rs`, `protocol.rs`, `settings`, `app/src/app/...disk-map*`, `docs/user-guide.md` | 2 |
| WP-5 | Rust bench (1M and 5M nodes with `Node`'s real fields, `reason`, `unread`, `child_count` included, realistic name lengths, raw and `flate2` on the back end Dyno gets, footprint), budgets asserted, results in performance.md; settles D3 | `dyno-agent/tests/disk_cache_bench.rs`, `docs/performance.md` | 0.5 |
| WP-6 | New counters and extras contract tests (`counters_wire.rs`, `extras_wire.rs`, wire JSON, specs), then the saved counter catalog and software list (`view_saved`), `savedAt` on the wire and in the UI | `counters.rs`, `extras/mod.rs`, `dyno-core/tests/`, their stores and specs | 1.5 |
| WP-7 (1.2) | macOS FSEvents incremental refresh with the fallbacks of section 5 | `dyno-platform/src/macos/`, `disk/` | 4 |
| WP-8 (after the helper) | Windows change journal through the helper | `dyno-helper`, `dyno-platform/src/windows/` | 3 |

1.1 scope (WP-1 to WP-6, with WP-3b): about **10.5 agent-days** (10 to 11; revision 1 said 8.5 and left out volume identity
  on three OS, the Rescan UX and the contract tests), plus an independent review of this record and of WP-3's diff. WP-1,
WP-2 and WP-5 can run in parallel; WP-3 needs WP-1 and WP-2; WP-4 needs WP-3's wire shape (fixed by this record, so the
UI can start on the mock).

## Decisions for the user (recommendation first)

The independent reviewer (revision 1) agreed with every recommendation below, D1 to D8, as recorded; D9 was added at the
reviewer's request. The user decides after the delta review.

- **D1. Where:** a separate `cache.db` with its own writer thread (**recommended**: no load on the core lock, damage and
  size isolated, may be deleted) / tables in `dyno.db`.
- **D2. Tree layout:** packed versioned records in chunks (**recommended**: about 6.5x faster to read and decode like for like in the synthetic Python measurement; WP-5
  repeats it in Rust) / one row per
  node (queryable from SQL, slower, a second query path).
- **D3. Compression:** `flate2` level 1 if WP-5 shows at least 2x smaller on realistic names at < 0.15 s per 1M nodes
  (**recommended**) / none.
- **D4. What is saved:** done scans and partial ones marked as partial (**recommended**: a cancelled whole-disk scan is
  still most of the answer) / done scans only.
- **D5. Refresh:** on demand, with the "net change in used space" hint and an opt-in "refresh on open when older than" Setting
  (**recommended**) / automatic on open when older than a day.
- **D6. Bounds:** one map per target, 16 targets, 1 GB total by default (**recommended**) / other values.
- **D7. Small lists in 1.1:** save the counter catalog and the software list (WP-6) (**recommended**) / disk map only.
- **D8. Incremental:** none in 1.1; FSEvents in 1.2; the change journal after the helper; no live watchers
  (**recommended**).
- **D9. Rescan behaviour (a UX change):** Rescan keeps the old map on screen until the new pass is Done, with progress
  and "Show progress" for the growing tree, and Cancel keeps the old map (**recommended**) / today's behaviour (the page
  switches to the growing tree at once; the saved map is replaced only when the new pass is Done).

## Sources (read 2026-11-03)

- FSEvents persistent history, event ids with the device UUID, `FSEventStreamCreateRelativeToDevice`: Apple's
  `FSEvents.h` comments as reproduced in <https://gist.github.com/tjfontaine/7594be6bc86139fafeee> (secondary copy of the
  header; to be checked against the current SDK in WP-7); Rust signatures:
  <https://docs.rs/fsevent-sys/latest/src/fsevent_sys/fsevent.rs.html>,
  <https://docs.rs/objc2-core-services/latest/objc2_core_services/fn.FSEventStreamCreateRelativeToDevice.html>.
- Changes made from another OS not reported (unverified, secondary):
  <https://kiwix.lema.org/stackoverflow.com_en_all_2023-11/questions/12239848/fsevents-treatment-of-disk-changed-by-other-os>.
- Windows change journal rights and journal id: <https://learn.microsoft.com/en-us/windows/win32/fileio/using-the-change-journal-identifier>,
  <https://learn.microsoft.com/en-us/windows/win32/fileio/walking-a-buffer-of-change-journal-records>.
- Measurement: [performance.md, 2026-11-03](../performance.md#2026-11-03-a-saved-disk-map-tree-of-1m-nodes-in-sqlite-rows-against-packed-chunks-docsdyno-cached-views).
