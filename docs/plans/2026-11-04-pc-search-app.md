# Recce (working name): draft plan for a PC-wide search, launcher and answers app (2026-11-04)

Status: **draft for the user's decisions.** No ADRs exist yet, so every size here is rough: agent-days for one agent, with a
wide range; firm estimates come once the ADRs have work packages.
Research, sources and the reasoning behind each choice:
[research/2026-11-04-pc-search-app.md](../research/2026-11-04-pc-search-app.md). The feature catalogue (what users love and
miss in comparable tools, with a stage, effort and privacy note per feature):
[research/2026-11-04-recce-features.md](../research/2026-11-04-recce-features.md); feature ids below (Q4, A1, C16 …) refer
to it.

**Release order.** The app starts after Dyno 1.1, Spotter 1.0, Pitwall 1.1, Pitcrew 1.1, Debrief 1.2, Pacenotes and Sighter
(the user's order of 2026-11-04, ER-1). Two kinds of work may run earlier, and only while load and CI capacity allow:
- spikes S1 to S6, which touch no shared code;
- the shared crates this app needs that another app is building anyway (`paddock-embed` X55, `paddock-walk` IG14).

## 0. Shape

- **Process model:**
  - `recce-agent`: per user, a login item through `paddock-service`; it owns the index, feeds, connectors, MCP and plugins;
  - the app: a pre-created hidden launcher window plus a main window on the shared shell;
  - guarded extraction workers and an embed worker;
  - optionally `recce-helper` (Windows journal, read-only) and Pitwall (answers).
- **Stores:**
  - the in-memory name table, with an mmap snapshot;
  - `catalog.sqlite` (sources, manifest, cursors, jobs, clipboard and snippets);
  - Tantivy (content), one index per source shard;
  - vectors per shard: binary codes for search plus int8 codes for re-scoring, in a flat file or a USearch index served
    from disk (section 1; spike S6 decides);
  - encrypted blobs for remote text.
- **Extension tiers:** built-in; TOML packs (quicklinks, web searches, snippets, workflows); Rust out-of-process plugins
  (new `SearchSource` and `ResultProvider` contributions).
- **Every long operation** is a `paddock-jobs` job with phases, a growing count, Cancel, and build-then-swap (rules 10
  to 14). Bulk work runs at utility QoS; queries are live work (rule 52).
- **Logs** follow ER-27: phases, durations, ids, counts and outcomes; never names, paths, queries or text. A test with
  invented, distinctive names asserts they never reach a log or a diagnostics bundle (the Dyno ADR 0029 §6 pattern).

## 1. Scale design (added 2026-11-04)

The user asked (2026-11-04): "what if the information is a lot?" The design target is a heavy single-user machine:
**10 M files, 5 M emails and 20 M passages** (the S6 corpus). Every size below is arithmetic or an estimate until S6
measures it; nothing here is a benchmark (rule 48).

### 1.1 Tiered index: not every item gets every layer

| Tier | What | Which items | Cost driver |
|---|---|---|---|
| **Names** | name, path, size, times, type, source | **every** file, mail (subject, sender, date) and app | the name table and `catalog.sqlite`; about 45 to 100 B per entry **(Everything's vendor figures, see research §5)** |
| **Keyword** | full text in Tantivy, per passage, no stored text (D5) | **text-bearing types only**: documents, PDFs, mail bodies, text and code, notes; attachments through `paddock-extract` | extraction CPU; the Tantivy index, roughly 25 to 50 % of the extracted text **(estimate; S6 measures)** |
| **Meaning** | Model2Vec vectors | **selected passages only**, in priority order, within the vector budget (1.2) | embedding is cheap (6,365 chunks/s on one thread, Pacenotes ADR 0001); storage and index build are not |

- Non-text types (images, media, binaries, archives, disk images) stay **names-only** unless the user opts a folder or a
  type in (OCR and media tags are confirmed bulk jobs, D13).
- Old archives (mail and folders not changed or opened for two years, PST/mbox archives) are **keyword-only by default**
  (decision D18); "Add meaning search" is one switch per source or folder.
- Every result says which tiers covered it, so a missing meaning hit is explained ("keyword only: archive") instead of
  silently absent.

### 1.2 Vector cost control

Bytes per passage at 256 dimensions (arithmetic):

| Layout | Per passage | 1 M passages | 20 M passages |
|---|---|---|---|
| f32 | 1,024 B | 1.0 GB | 20.5 GB |
| f16 | 512 B | 0.5 GB | 10.2 GB |
| int8 | 256 B | 0.26 GB | 5.1 GB |
| binary (1 bit per dimension) | 32 B | 32 MB | 0.64 GB |
| HNSW graph, M = 16 (about 32 level-0 links of 4 B plus keys) | about 144 B **(estimate; USearch's layout differs)** | 0.14 GB | 2.9 GB |
| **Default: binary search codes + int8 re-score codes, flat** | **288 B** | **0.29 GB** | **5.8 GB** |
| Default plus an HNSW graph over the binary codes | about 432 B | 0.43 GB | 8.6 GB |

The rules:
1. **256 dimensions.** Model2Vec potion models at 256 dims (potion-base-8M, potion-code-16M-v2; Pacenotes ADR 0001).
   The 512-dim potion-retrieval-32M and the 128M multilingual model are opt-in per source, never the default, because
   they double storage for little or no measured gain (Pacenotes ADR 0001 table).
2. **Quantise, then re-score.** Candidates come from binary codes (Hamming distance, popcount), the top k (default 200,
   a Setting) are re-scored with int8 codes read from an mmapped file, and the top 50 go to RRF (and to the optional
   Qwen re-rank through Pitwall, WP 2.3). f32 vectors are never stored: Model2Vec is deterministic, so a model change or a
   precision question is answered by re-embedding, not by keeping 20 GB of floats. S6 measures recall@10 of binary-only,
   binary + int8 and int8-only against exact f32 brute force; if binary + int8 misses the bar, f16 re-score codes
   (512 B) are the fallback.
3. **Disk-resident, mmapped.** Vector files are read through mmap, so the OS pages them in for a query and out again;
   idle memory does not grow with the corpus. Candidates (S6 decides): **(a) a flat binary scan** (pure Rust, no index
   to build or repair; 640 MB of codes at 20 M passages, so a parallel scan is plausibly tens of milliseconds warm
   **(estimate)**); **(b) USearch** with `i8` or `b1x8` codes served with `view` ("view large indexes from disk without
   loading into RAM", README, read 2026-11-04); a viewed index is assumed read-only **(verify)**, so each shard keeps
   an immutable base, a small in-memory delta for new passages and a tombstone set, and a background job rebuilds and
   swaps (ER-18, build then swap); **(c) LanceDB** IVF_RQ / IVF_PQ on disk with `refine_factor`; **(d)** a DiskANN-style
   graph on SSD (`diskann-rs`, or Microsoft's DiskANN now in Rust) as a stretch in S6 only if a maintained crate is
   published.
4. **Prioritise.** Passages are embedded in order of a priority score: opened or launched through Recce (frecency),
   recently changed, sources and folders marked important, mail of the last 12 months, then the rest. When the budget is
   full, the lowest-priority vectors are dropped first (those passages stay keyword-searchable) and the ledger says so.
5. **Budgets as Settings (rule 51).** Per source: a disk budget for vectors and for the keyword index, and a CPU budget
   (worker threads), each with **Auto**. Proposed Auto for the vector layer (decision D17): **min(4 GB, 2 % of the index
   volume) in total**, which holds about **13.9 M passages flat or about 9.3 M with an HNSW graph** at the default layout
   (arithmetic: 4 GB / 288 B and 4 GB / 432 B). CPU Auto: cores − 1 when idle on mains power, 1 thread while the user is
   active, 0 on battery (1.5).
6. **Content fingerprints.** File level first: the manifest (size, mtime, ctime, file id, then content hash; Pacenotes
   D1/D3) skips unchanged files before extraction. Passage level second: a 128-bit hash of the normalised passage text
   plus the model id keys the vector, so an edit re-embeds only changed passages, a copied file re-embeds nothing, and a
   rename or move costs a manifest update only. 16 B per passage: 320 MB of keys at 20 M passages (arithmetic).

### 1.3 Per-source shards

- **One shard per source:** a drive or root folder, a mailbox, a SharePoint site or OneDrive, an IMAP account. Each
  shard has its own Tantivy index, vector files and manifest rows, and a large mailbox splits into yearly segments so
  old years can be dropped to keyword-only (D18) without a rebuild.
- **Queries fan out in parallel** over a bounded pool (live work, normal priority, rule 52) and merge:
  - names: one in-memory table across shards (it is small per entry);
  - keyword: per-shard BM25 lists. BM25 scores are not comparable across shards (each has its own IDF), so either the
    shards share global statistics through Tantivy's `Bm25StatisticsProvider` trait (exists on docs.rs, read
    2026-11-04; the wiring is **unverified**) or the lists are merged by RRF; S6 compares both on the stored query set;
  - meaning: cosine scores are comparable across shards (same model), so these lists merge by score;
  - the three layer lists then fuse by RRF (k = 60, as X55).
- A shard that answers late does not hold the list: results show at once and the late shard's hits merge in with a
  "still searching 1 source" line (ER-19: no timeout that hides results).
- **Rebuild or forget per source:** a rebuild builds a new shard beside the old one and swaps (ER-18); Forget deletes
  the shard's files and catalog rows and shows the space freed. Small sources share one "small sources" shard so a
  query never opens hundreds of indexes (cap of shards per query a Setting, Auto 32 **(estimate)**).

### 1.4 Parallelism and cost at scale (rule 51, ER-20)

| Work | Bound (Auto, a Setting) | Cheaper operation |
|---|---|---|
| Embedding 20 M passages | cores − 1 threads | at 55,499 chunks/s on 13 threads (Pacenotes ADR 0001) that is about 6 minutes; at one thread about 52 minutes (arithmetic). Not the bottleneck: fingerprints make re-runs nearly free |
| Extraction | cores − 1 guarded workers | the bottleneck at this scale (S2 gives the rate); tier rules skip non-text types; caps on size and time |
| Vector index build | one builder per shard, at most 2 at once | flat layout has no build; HNSW builds per shard at background priority, beside the old one |
| Tantivy writes | one writer per shard, 64 MB budget each, at most 2 shards writing | batch commits; merge policy for many small updates |
| Query fan-out | min(shards, cores − 1) threads, live | per-shard top-k only; binary scan before any int8 read |

### 1.5 Background indexing

- Bulk work runs at `QOS_CLASS_UTILITY`, nice 10 or `THREAD_PRIORITY_BELOW_NORMAL` (rule 52: work a person waits for
  never runs at a starvable level; nobody waits on a backlog, but the coverage view is waited on, so not `BACKGROUND`).
- **Pauses:** on battery (a Setting, default pause), while the user is active (input in the last 2 minutes: drop to one
  thread, never to zero, so an always-busy user still converges), on low disk (below the budget's floor), and on thermal
  pressure where the OS reports it. Each pause shows its reason and a "Resume now" button.
- **Live, phased progress from the first moment (ER-18):** List → Extract → Keyword index → Embed → Vector index →
  Swap, each phase on its own scale, a growing count where the total is unknown; Cancel in every phase leaves the old
  shard in use.
- **Coverage, honestly:** per source and tier, for example "Names 100 %; keyword 84 % (12,034 files left); meaning 41 %
  (budget reached, recent first)". Never a single percent that mixes tiers (catalogue C16, V3).
- The UI never times out the job (ER-19); a backlog that will take hours says so with a rate and a rough remaining time
  from the measured throughput, labelled as an estimate.

### 1.6 A future, optional "index server" mode

For data beyond one machine (a NAS archive, a family share, tens of millions of passages across devices), a separate,
optional **index server** may come later: the agent becomes a client of a server process that holds shards for the
sources assigned to it. **Postgres with pgvector** is the candidate there (`halfvec`, `bit` with "re-rank by the
original vectors", HNSW up to 4,000 dims for `halfvec`; [pgvector README](https://github.com/pgvector/pgvector), read
2026-11-04; licence: PostgreSQL License **(from memory, not re-read)**). It is never the default: it needs a database
service, sign-in, TLS and backups, and it moves text off the machine. It gets its own ADR (R10), only when a user asks
for it, and it is not in stages 1 to 4.

### 1.7 The embedded store: choice and why

Recommended default: **USearch (or the flat binary layout, S6 decides) + Tantivy + SQLite**, all embedded, per shard.

| Option | Facts (source, read 2026-11-04) | For | Against | Verdict |
|---|---|---|---|---|
| **USearch** 2.26.6 + Tantivy + SQLite | Apache-2.0; `i8` "only valid for cosine-like metrics"; `b1x8` binary; "view large indexes from disk … without loading into RAM" ([README](https://github.com/unum-cloud/usearch)) | Mmapped, quantised, small; Tantivy already in Debrief; SQLite already in Pacenotes and Dyno | C++ core built per OS; a viewed index is read-only **(verify)**, so updates need delta + rebuild | **Default**, unless the flat binary layout meets the budgets (then no ANN index at all) |
| Flat binary + int8 files (own, pure Rust) | arithmetic: 288 B per passage | Nothing to build or repair; exact over binary codes; deterministic | Linear scan cost; unmeasured above 1 M | **Baseline in S6**; default if it meets the p95 budget at 20 M |
| DuckDB-VSS (HNSW) | Persistence needs `SET hnsw_enable_experimental_persistence`; "the index itself is not buffer managed and must be able to fit into RAM"; "WAL recovery is not yet properly implemented for custom indexes", so a crash can lose data or corrupt the index; deletes are marked and need `PRAGMA hnsw_compact_index` ([DuckDB 2.0 VSS docs](https://duckdb.org/docs/2.0/core_extensions/vss.html)) | Debrief knows DuckDB; SQL over vectors and metadata | RAM-resident index (20 GB of f32 at 20 M); experimental persistence; crash safety | **No** as the default; measured in S6 at 5 M for comparison only |
| LanceDB 0.41 (`lancedb` crate) | Apache-2.0; IVF_RQ (RaBitQ, "around 1/32 of raw size"), IVF_PQ, IVF_HNSW_SQ ("best recall/latency trade-off"); `refine_factor` "reads and reranks additional candidates … to recover recall lost to quantization" ([docs](https://docs.lancedb.com/indexing/vector-index)) | Disk-native format, quantisation and re-scoring built in | Heavy (Arrow, DataFusion) for one app (research §2.4); larger binary and build time **(measure in S6)** | **Runner-up**; the fallback if USearch's update path is too costly |
| `sqlite-vec` | MIT/Apache; "float, int8, and binary vectors in `vec0`"; "pre-v1, so expect breaking changes"; IVF and DiskANN source files exist but the README does not document them ([repo](https://github.com/asg017/sqlite-vec)) | Inside the catalog | Brute force as documented; pre-v1 | **No** |
| DiskANN-style | `diskann-rs` 0.5.0: Vamana, mmap, f16 and int8 (docs.rs, via search; its benchmark is 200 k × 128 only, **secondary, unverified**); Microsoft DiskANN's `main` is now Rust ("DiskANN3"), MIT, C++ kept on `cpp_main`; no published crate seen ([repo](https://github.com/microsoft/DiskANN)) | Built for indexes larger than RAM on SSD | Young crates; maintainers and releases unclear | **Watch**; a stretch row in S6 if a crate is published |
| Postgres + pgvector | see 1.6 | Proven at large scale | A server | **Index server mode only** (1.6) |

## 2. Spikes first (before any ADR is accepted)

Each spike runs on synthetic data only (ER-14). Each records its numbers in `apps/recce/docs/performance.md` (rule 48),
uses a private `CARGO_TARGET_DIR`, and runs as a heavy job through the semaphore.

| Spike | Question | Output | Size |
|---|---|---|---|
| S1 | Name table: memory and per-keystroke latency for 0.5M, 2M, 5M and 10M generated paths (10M added for scale, section 1); own table against a Tantivy n-gram field; `paddock-names` matching cost; resident footprint with the snapshot mmapped | A results block, and the choice for ADR R2 | 2.5 to 3.5 |
| S2 | Extraction: `pdfium-render`, `pdf_oxide` and `lopdf` on a generated PDF corpus; kreuzberg (features trimmed) against our own thin adapters on OOXML, PDF, eml and PST; binary size, `cargo deny`, peak RSS per file, throughput per worker | Library choice for `paddock-extract` | 3 to 4 |
| S3 | Change feeds: macOS FSEvents history across a reboot, with a UUID check; Windows journal through an elevated test process in the VM (local only, never CI); whether the "unprivileged" journal read exists and what it returns; Linux inotify cost at 100k folders | Feed design for ADR R3 | 3 |
| S4 | macOS: does Full Disk Access survive an ad-hoc re-signed update? (local VM, lessons.md) | Yes or no, and the Repair flow | 1 |
| S5 | Graph: PKCE loopback sign-in with a personal account and a test tenant; delta for drive and mail; `410` and `syncStateNotFound` paths with a fake server | Connector design for ADR R5 | 2 to 3 |
| S6 | Scale: 10 M files, 5 M emails, 20 M passages (synthetic); first index, incremental update, disk per layer, footprint, query p50/p95 per layer; int8 against binary; USearch, DuckDB-VSS, LanceDB and a flat binary baseline | Store and layout choice, budgets and Auto defaults for ADR R4; D17 confirmed or changed | 6 to 9 |

Spikes in total: **about 17 to 24 agent-days** (S1 to S6).

### S6: the scale test

**Question.** At the design target (section 1), which embedded store and quantisation meet the budgets below, and what
do the tiers, shards and fingerprints cost and save?

**Harness design.**
- **Generator** (`apps/recce/bench/gen-scale`, Rust, deterministic from a printed seed): invented data only (ER-14), no
  real names, paths or mail. It writes a stream through a fake `SearchSource`, not a real folder tree, so the spike
  measures the index layers rather than the file system (the walk is S1 and S3's job):
  - 10 M file entries: paths with a realistic depth and fan-out distribution, sizes and times from fixed distributions,
    20 % text-bearing;
  - 5 M emails in 8 synthetic mailboxes (sender, subject, date over 15 years, threads, bodies);
  - 20 M passages of about 1 KB from a Zipfian vocabulary of invented words plus a small English word list, so BM25
    and Model2Vec both see plausible statistics;
  - a stored query set of 1,000 queries with planted answers (name, keyword, meaning and mixed), used for recall and
    for the shard-merge comparison (1.3).
- **Vectors:** real Model2Vec (potion-base-8M, 256 dims) over the synthetic passages, embedded once and cached as an
  f32 file outside `~` (20.5 GB), so every store is fed the same vectors and random vectors (which are unrealistically
  easy or hard for ANN) are never used. Exact f32 brute-force top-10 for the 1,000 queries is the ground truth, computed
  once and cached.
- **Adapters** behind one `VectorStore` trait in the spike: flat binary + int8 (own), USearch (`i8`, `b1x8`, viewed
  from disk), DuckDB-VSS (HNSW, experimental persistence; at **5 M only**, because its index must fit in RAM, then
  extrapolated and labelled so), LanceDB (IVF_RQ and IVF_PQ with `refine_factor`); `diskann-rs` as a stretch row only
  if time allows. Tantivy and the name table run once (not per store).
- **Shards:** the corpus is split into 24 shards (8 mailboxes, 12 drives or roots, 4 SharePoint-like sites) and also
  run as one index, so the cost of sharding is measured, not assumed.
- **Scenarios:** (1) first index from empty; (2) incremental: 10,000 files changed (5 % of passages edited, the rest
  touched only), 1,000 moved, 1,000 deleted, 20,000 new mails; (3) a model id change (rebuild beside and swap);
  (4) Forget one mailbox shard.
- **Measures:** wall time per phase; CPU seconds; disk per layer (names, catalog, Tantivy, binary codes, int8 codes,
  graph); physical footprint (rule 26) peak during indexing and idle after 5 minutes, both with a cold and warm page
  cache; query p50/p95/p99 per layer (name, keyword, vector, hybrid) over the 1,000 queries, warm and after a page-cache
  purge; recall@10 against the f32 ground truth; the number of embed calls in scenario 2 (proves fingerprints).
- **Running it:** one heavy job through `scripts/heavy.sh` and the semaphore, `CARGO_BUILD_JOBS=2`, build before the
  measuring window, data and targets on `/Volumes/d7sst7` (never under `~`), about 100 GB free disk needed
  **(estimate)**; only when the 1-minute load is below 12 and no release is building (docs/benchmarks.md); indexing at the
  product's utility QoS, queries at normal level. Never during a release (ER-1).

**Budgets to beat** (targets, first set here; S6 confirms or moves them with a results block):

| Measure (S6 corpus) | Budget |
|---|---|
| Name query per keystroke, 10 M entries | ≤ 30 ms p95 |
| Keyword (BM25), 20 M passages, 24 shards | ≤ 150 ms p95 warm |
| Vector, top 10 after re-score | ≤ 50 ms p95 warm; ≤ 300 ms p95 cold |
| Hybrid (names + keyword + vector, RRF) | ≤ 250 ms p95 warm; first results ≤ 1 s cold |
| Recall@10 against exact f32 | ≥ 0.95 int8; ≥ 0.92 binary + int8 re-score |
| Disk, vector layer, default layout | ≤ 300 B per passage flat; ≤ 450 B with a graph |
| Disk, keyword layer | ≤ 50 % of the extracted text |
| Idle footprint of the agent | ≤ 300 MB with the name snapshot mmapped (≤ 1 GB if the 10 M name table must be resident; S1 decides) |
| Peak footprint while indexing | ≤ 4 GB including workers |
| Incremental scenario 2 | names visible ≤ 2 s; keyword ≤ 60 s; vectors ≤ 5 min; embed calls only for edited passages |
| First index (excluding extraction, which S2 measures) | ≤ 2 h for keyword and vectors at Auto on the Mac mini |

**Results block template** (rule 48: once `apps/recce/docs/performance.md` exists, the block goes there, newest first;
until then nothing is quoted):

```markdown
### YYYY-MM-DD, S6 scale test: <store> <layout> (<branch>)

- **Commit:** <short sha>
- **Machine:** <host, CPU, cores, RAM, OS and version>; disk <model, free space>
- **Load:** 1-minute load <min–max>; <other jobs, if any>
- **Build:** release; allocator; usearch | duckdb + vss | lancedb <versions>; tantivy <version>; potion-base-8M
- **Corpus:** `gen-scale --seed <n> --files 10000000 --mails 5000000 --passages 20000000 --shards 24`; synthetic
- **Command:** `<exact command, with every env var>`
- **Runs:** <n> runs per scenario; median and range; p50/p95/p99 over 1,000 queries
- **Numbers:**

  | Measure | int8 | binary + int8 | Budget | Verdict |
  |---|---|---|---|---|
  | First index, per phase (min) | | | ≤ 2 h total | |
  | Incremental scenario 2 (s), embed calls | | | see S6 budgets | |
  | Disk: names / catalog / Tantivy / binary / int8 / graph (GB) | | | | |
  | Footprint peak / idle (MB) | | | ≤ 4,096 / ≤ 300 | |
  | Name / keyword / vector / hybrid p50, p95 warm (ms) | | | 30 / 150 / 50 / 250 p95 | |
  | Vector / hybrid p95 cold (ms) | | | 300 / 1,000 | |
  | Recall@10 against f32 | | | 0.95 / 0.92 | |
  | Shards against one index: hybrid p95, recall@10 | | | | |

- **Memory measure:** physical footprint (rule 26)
- **Notes:** extrapolations (DuckDB-VSS at 5 M), what is not measured, where the raw output is kept
```

**Size:** 6 to 9 agent-days: generator and query set 1.5 to 2; harness, adapters and the flat baseline 2.5 to 3.5;
runs (long, several layouts, cold and warm) 1 to 2; write-up and the R4 input 1 to 1.5.

## 3. ADRs to write (app numbering, `apps/recce/docs/adr/`)

| ADR | Decision | After |
|---|---|---|
| R1 | Architecture: agent, app, workers, helper; ports and adapters; the IPC contract | user decisions D1 to D3 |
| R2 | Name table and query model (kinds, filters, ranking, frecency) | S1 |
| R3 | Change feeds and the manifest (shared `paddock-watch` `ChangeFeed`; shared `paddock-walk`) | S3, Pacenotes ADR 0012 |
| R4 | Content and meaning index at scale (section 1): tiers, Tantivy schema, chunking, `paddock-embed`, vector store and quantisation, shards and merge, budgets with Auto, fingerprints, RRF | S2, S6, X55, D17, D18 |
| R5 | Connectors: Graph, IMAP, local mail; secrets; permission handling | S5 |
| R6 | Privacy: storage, encryption, exclusions, MCP exposure, logs | D5 to D8 |
| R7 | Launcher: hotkey per OS, app catalogue, calculator, clipboard, snippets, system actions | D9 |
| R8 | Extensions: packs and workflows, plugin contributions | D10 |
| R9 | Answers: retrieval, prompt, citation check, Pitwall preset | stage 4 |
| R10 | Index server mode (optional, section 1.6): Postgres + pgvector, client protocol, sign-in, what leaves the machine | only when a user asks; not in stages 1 to 4 |

Family ADRs needed (shared code):
- an amendment to ADR 0001 for the new contribution kinds;
- one ADR for `paddock-extract`;
- an amendment to ADR 0007 if MCP needs new shared limits.

## 4. Stages and work packages

Sizes are agent-days and rough; ranges are wide on purpose. The "Shared" column names the shared work each package depends
on or creates (rules 42, 43 and 55: a shared change lands with its adoption).

### Stage 1: launcher and file name search (about 39 to 58 agent-days)

The first useful release: hotkey, apps, files by name, calculator, actions, plus the agent and the shell.

| WP | Scope | Shared | Acceptance | Size |
|---|---|---|---|---|
| 1.1 | App skeleton from the template (`scripts/new-app.mjs`): shell, settings, jobs pane, `paddock.ci.json`, workflow, CODEOWNERS | shell (ADR 0012), composition contract (ADR 0037) | Conformance green; the app starts and quits on three OS | 3 to 4 |
| 1.2 | `recce-agent` as a login item; IPC contract with generated TypeScript; supervisor; health | `paddock-service`, `paddock-ipc`, `paddock-health` | Agent restarts after a crash; app shows Repair (rule 19) | 4 to 6 |
| 1.3 | Name table (from S1): build, query, mmap snapshot, frecency; sized for 10 M entries (section 1) | `paddock-names` | S1 budgets met in `paddock-bench`, including 10 M entries at ≤ 30 ms p95 and the idle footprint budget | 6 to 8 |
| 1.4 | First listing and the manifest walk with stat fields; roots and exclusions (built-in pack plus settings); network shares by scheduled rescan; cloud placeholders never read | `paddock-walk` (IG14, with Pacenotes), `paddock-packs` | 1M generated files indexed within the budget on SSD; progress and Cancel per phase | 5 to 7 |
| 1.5 | Change feeds: FSEvents history (macOS); RDCW plus manifest (Windows, no helper); inotify roots (Linux); cursors in the catalog | `paddock-watch` `ChangeFeed` (new, with Dyno ADR 0029 WP-8) | Create, rename and delete show within 2 s; a dropped-events path triggers a rescan; tests use fakes (ADR 0041) | 5 to 7 |
| 1.6 | Launcher window: pre-created and hidden, shown on the hotkey; results as you type; keyboard only; preview pane (file metadata, Quick Look-style preview where the OS offers one **(verify)**); actions (open, reveal, copy path, open with) | `global-hotkey`, `paddock-files`, `paddock-window` | ≤ 50 ms to paint (budget); accessible (CDK listbox, rule 47) | 5 to 7 |
| 1.7 | App catalogue per OS, including Store apps and Settings pages (L2); calculator (`fend-core`); system commands with confirmation | none | The listed apps of a VM image are found; destructive actions confirm | 3 to 5 |
| 1.8 | Query and action model (catalogue Q2 to Q7, Q12, A1 to A4, L5 to L10, L16, E1 in part): filter syntax and chips, saved searches and scopes as a TOML pack, recent searches; action panel on every result, file buffer, Alt+1..9, run as admin; quicklinks and keyword web searches with `{query}` placeholders and keyword-only suggestions (moved here from 4.3); fallbacks, aliases, Home with pinned and recent items, compact mode, `recce://` command links, hotkey conflict check | `paddock-packs`, `paddock-siblings` (deep links) | Syntax and chips give the same query (property test); a plain query never reaches the network (test with a fake egress); every action reachable by keyboard with its shortcut shown | 5 to 8 |
| 1.9 | Coverage and source health for local sources (C16, F4, V6): counts, last change, unreadable folders and why (no Full Disk Access, excluded, locked) with Repair; rebuild one source; pause indexing from the tray | `paddock-health`, `paddock-window` | A denied folder in the VM shows its reason and a working Repair; the invented-names test covers the view's logs | 3 to 4 |

### Stage 2: content and meaning search (about 49 to 77 agent-days)

| WP | Scope | Shared | Acceptance | Size |
|---|---|---|---|---|
| 2.1 | `paddock-extract`: ports, guarded worker protocol, limits (size, time, memory, zip ratio), quarantine by content hash; OOXML (moved from Pacenotes' reader), PDF (S2 choice), text, HTML, RTF | **new shared crate**; `paddock-guard`, `paddock-proc`, `paddock-archive`; safe XML from Pacenotes ADR 0010 | Fuzz targets; a hostile corpus never takes the agent down; Pacenotes adopts it in the same change (rule 55) | 8 to 12 |
| 2.2 | Tantivy content index, one per source shard (section 1.3), with the shard merge S6 chose (shared BM25 statistics or RRF), late shards merged in; schema, chunking, passage re-read from the source, deletion on change; content operators (phrase, same line or section, fields) (Q8); preview with hit highlighting and open at the match (P2, P3); "why this result" (Q11) | none | BM25 query ≤ 100 ms p95 at 1M generated documents and ≤ 150 ms p95 at 20 M passages in 24 shards (S6 budget); a stored synthetic corpus finds every planted hit (Q15) | 8 to 12 |
| 2.3 | Embeddings through `paddock-embed` (Model2Vec default at 256 dims; multilingual model a Setting per source); the vector store and layout S6 chose (binary search codes, int8 re-score of the top k, mmapped; USearch base plus delta and tombstones with rebuild-and-swap, or flat); RRF; Qwen re-rank of the top 50 through Pitwall when it runs | **X55 `paddock-embed`** (Debrief and Pacenotes build it first) | hit@5 against a stored synthetic query set (the bar set in R4); recall@10 against exact f32 meets the S6 bar; the index record lets a model change rebuild beside and swap | 7 to 11 |
| 2.4 | Content job: pipeline walk → extract → chunk → index → embed → vector index → swap, with stage threads as Settings (Auto); utility QoS; resumable; passage fingerprints (text hash plus model id) so unchanged content is never re-embedded; the priority queue of section 1.2 | `paddock-jobs` | Phases, counts and Cancel; idle CPU ≈ 0 after catch-up; a touch-only or move-only change makes zero embed calls (test with a counting fake embedder) | 5 to 7 |
| 2.5 | Windows helper (journal and MFT listing, read-only) | **`paddock-helper`** extracted from Dyno's helper (ADR 0034 copies), or an app helper with a registered copy | Callers checked; installer and uninstall paths in the VM; rule 54 bundle build | 8 to 12 |
| 2.6 | Privacy storage: no stored local text; encrypted store for remote text and clipboard; FDE check at first run; Forget a source; the privacy ledger (V3: per source what is indexed, where, its size; index size and memory cap, F3) | `paddock-secrets`, `paddock-storage` | The invented-names test passes on index, logs and diagnostics | 5 to 8 |
| 2.7 | Deterministic natural-language filters shown as chips (Q10); browser bookmarks (C10); EPUB (C2); "Open in Debrief" for logs and bundles (G3) | `paddock-siblings` | The filter parser has a table test of phrases to chips and never calls a model | 3 to 5 |
| 2.8 | Scale controls (section 1): tier rules (names for all, keyword for text-bearing types, meaning for selected passages; archives keyword-only by default, D18); per-source disk and CPU budgets as Settings with Auto (D17); priority order and eviction to keyword-only when a budget is full; pause on battery, user activity (one thread floor), low disk and thermal pressure (F2); coverage per source and tier in the ledger | `paddock-jobs`, `paddock-health` | A budget change evicts lowest-priority vectors first and the ledger shows it; each pause shows its reason and Resume; coverage never mixes tiers into one percent | 4 to 6 |

### Stage 3: email and SharePoint/OneDrive (about 32 to 48 agent-days)

| WP | Scope | Shared | Acceptance | Size |
|---|---|---|---|---|
| 3.1 | Local mail: eml, mbox (`mail-parser`), PST (`outlook-pst`), msg; attachments through `paddock-extract`; Apple Mail `.emlx` and Thunderbird folders; one shard per mailbox in yearly segments, old years keyword-only by default (D18) | `paddock-mail` if a second app needs it, else `recce-mail` | Generated mailboxes (never real mail, ER-14); threads and attachments searchable; dropping an old year to keyword-only frees its vectors without a rebuild | 7 to 10 |
| 3.2 | Graph sign-in: PKCE loopback (127.0.0.1), tokens in the keychain, BYO client id, consent errors explained | `paddock-secrets`, `paddock-net`; `paddock-graph` on a second user | Personal account and test tenant (S5); a blocked consent shows the next step | 5 to 7 |
| 3.3 | OneDrive and SharePoint: drive delta, `token=latest`, `410` resync, content download under caps with hash-while-writing; access re-check on open and in a weekly job | `paddock-download` | A fake Graph server covers delta, duplicates, `410` and `429` with `Retry-After` | 6 to 9 |
| 3.4 | Outlook mail through Graph: message delta per folder; `syncStateNotFound` resync; attachments on demand; mail preview with headers, thread and attachments (P4); optional Touch ID / Windows Hello lock for mail (V4) | same | Fake server tests; throttling honoured | 6 to 9 |
| 3.5 | IMAP: UIDVALIDITY plus UID; CONDSTORE/QRESYNC where offered; OAuth for Gmail and Outlook.com **(verify)** | `paddock-net` (TLS policy) | A local test IMAP server in a container we create (ER-14) | 5 to 8 |

### Stage 4: answers with sources, MCP, plugins and workflows (about 36 to 54 agent-days)

| WP | Scope | Shared | Acceptance | Size |
|---|---|---|---|---|
| 4.1 | Answers: hybrid retrieval, token budget, numbered sources, citation check in Rust, `recce-answer` preset in Pitwall, streaming, Cancel, "Still working"; "no supported answer" instead of an uncited one (K2); source cards with open-at-passage (K3); scoped answers (K4); follow-up chat with local, deletable history (K6); summarise one result (K7) | `paddock-pitwall`, `paddock-assistant` (if it fits) | Answers without citations are flagged or refused; a fake LLM provider drives the tests | 8 to 12 |
| 4.2 | MCP server: `search`, `get_passage`, `list_sources` (and `answer` if enabled); per-source visibility; audit log view | `paddock-mcp` (spec update is its own item) | Off by default; caps and paging tested; the audit log has no text | 4 to 6 |
| 4.3 | Packs: snippets and workflows (closed step set; chained actions A5; run-command only from user packs after confirmation); quicklinks and web searches moved to 1.8 | `paddock-packs` | Invalid packs refused whole with file and line; hot reload | 3 to 5 |
| 4.4 | Plugin contributions `SearchSource` and `ResultProvider`; host wiring; a sample plugin (an Obsidian vault source) | `paddock-plugin-api`, `-host`, `-test` (ADR 0001 amendment) | Conformance checks; a slow provider is skipped within its budget; quarantine | 8 to 12 |
| 4.5 | Clipboard history (off by default; concealed markers; password apps ignored by default; app exclusions; encrypted; retention choices; "clear last 5 or 15 minutes") and snippets in the launcher; deterministic secret filter for clipboard and MCP passages (V5, V7); paste into the front app (A7) | none | Privacy tests with invented secrets | 5 to 7 |
| 4.6 | Family command registry (G1, family ADR on `paddock-siblings`): each Paddock app declares its commands and Recce lists them; Dyno hand-offs and summary results (G2); "Open in Pacenotes" (G4) | `paddock-siblings` (shared change with its adoption, rule 43) | A sibling's declared command appears and runs its deep link; Recce never performs another app's action itself | 4 to 6 |

### Stage 5 (optional, later): index server mode

Only when a user asks for it (section 1.6, ADR R10): a server process holding shards in Postgres + pgvector, the agent as
its client, sign-in and TLS, and an honest view of what leaves the machine. Rough size 15 to 25 agent-days; **not in the
total below** and not estimated firmly before R10.

**Total, rough:** about 156 to 237 agent-days after the spikes (about 17 to 24, including S6), before review rounds and
releases; the scale design added about 10 to 14 agent-days to stages 1 to 3. Firm figures
come with the ADRs' work packages.

## 5. Dependencies on other apps' shared work

| Needs | From | Status (2026-11-04) |
|---|---|---|
| `paddock-embed` (X55) | Debrief 1.2 and Pacenotes stage 2 | Planned ([shared-next X55](shared-next.md)) |
| `paddock-walk` stat fields and Windows enumeration (IG14) | Pacenotes ADR 0012 | Accepted 2026-11-04 on `docs/pacenotes-fast-ingest` |
| `ChangeFeed` with cursors in `paddock-watch` | Dyno ADR 0029 WP-8 (journal through the helper) and Pacenotes | Not started; propose as shared from the start |
| Privileged helper pattern | Dyno 1.1 (ADR 0027) | In the Dyno 1.1 release |
| Safe XML and OOXML reading | Pacenotes ADRs 0009 and 0010 | Accepted; code in Pacenotes stage work |
| Shell, dialogs, jobs pane, settings | ADRs 0012, 0026, 0037 | Built |
| `paddock-mcp` spec update (2025-11-25 and later) | family | Not planned; raise separately |

## 6. Open decisions for the user (recommended default first)

| # | Decision | Recommended default | Alternatives |
|---|---|---|---|
| D1 | Working name | **Recce** | Marshal; Sweep |
| D2 | Process model | **Per-user agent (login item) plus app**; the index is warm all the time | Index inside the app only (no updates while closed) |
| D3 | Windows privileged helper | **Yes, in stage 2, read-only (journal and MFT)**, shared with Dyno's pattern | None: RDCW plus rescans only (slower first index, may miss changes while the agent is off) |
| D4 | Name index | **Own in-memory table** (S1 confirms) | Tantivy n-gram field |
| D5 | Stored text | **Store no local file text; remote text encrypted** with a keychain key; FDE check at first run | Encrypt everything, including a custom Tantivy directory (spike first); store plain text (fastest previews) |
| D6 | Clipboard history | **Off by default**; concealed markers honoured | On by default with a 7-day retention |
| D7 | Microsoft 365 app registration | **A family app registration (multi-tenant plus personal accounts) and a BYO client id field** | BYO only; personal accounts only |
| D8 | MCP | **Off until enabled; mail and M365 hidden from MCP by default; no paths unless allowed** | Everything visible once MCP is on |
| D9 | Wayland hotkey | **Portal where available, otherwise a documented desktop shortcut to `recce --toggle`** | X11 only |
| D10 | Plugins and workflows | **Three tiers (Dyno ADR 0007): built-in, TOML packs with a closed step set, Rust plugins for sources and providers**; run-command only from the user's own packs after confirmation | Allow scripts in any pack; no workflows |
| D11 | Extraction toolkit | **Decide after S2**: own thin adapters (pdfium-render, calamine, mail-parser, outlook-pst) in `paddock-extract`, or kreuzberg trimmed inside the worker | Tika through extractous (stale; not recommended) |
| D12 | macOS sources | **Our own index everywhere**; Spotlight only as an optional extra source | Use Spotlight for names on macOS |
| D13 | OCR | **Off by default**; platform OCR as a confirmed bulk job per folder | On for images and scanned PDFs |
| D14 | Timing | **After Pacenotes and Sighter; spikes S1 to S6 earlier only while load is low (S6 only when no release is building)** | Start after Debrief 1.2 |
| D15 | Web search suggestions | **On by default, only after an engine keyword (`g term`)**, never for plain queries; a switch per engine | Off by default; suggestions for every query (rejected: sends keystrokes off the machine) |
| D16 | Family command registry | **A shared declaration in `paddock-siblings` that every app adopts** (catalogue G1) | Recce hard-codes each sibling's commands |
| D17 | Default vector budget (section 1.2) | **Auto = min(4 GB, 2 % of the index volume) for the vector layer in total**, binary search codes plus int8 re-score codes: about 13.9 M passages flat or 9.3 M with a graph (arithmetic); beyond it, the lowest-priority passages stay keyword-only and the ledger says so. S6 confirms the bytes per passage | 1 GB (about 3.5 M passages flat); 10 GB; unlimited; no vectors unless opted in |
| D18 | Old archives | **Keyword-only by default** (names and full text, no vectors) for mail and folders not changed or opened for two years and for PST/mbox archives; one switch per source or folder adds meaning search | Meaning search for everything within the budget, by priority only; names-only for archives |
| D19 | Very large data | **No index server in stages 1 to 4**; an optional, separate index server mode (Postgres + pgvector, ADR R10) only when a user asks | Plan it now as stage 5; never |

## 7. Risks and their owners

The top risks are listed in research section 6. Each ADR records its own risks. Two items need a check before stage 1 is
committed:
- whether Full Disk Access survives an update (S4);
- whether `outlook-pst` reads OST files (S2).

Scale risks (section 1):
- **The vector layer outgrows the disk or the budget.** Mitigation: tiers, D17's budget with eviction to keyword-only, and
  quantisation; S6 measures bytes per passage.
- **Quantisation loses recall.** Mitigation: int8 re-score of the top k; f16 re-score codes as the fallback; S6 measures
  recall@10 against exact f32.
- **USearch's disk-served index cannot be updated in place (verify).** Mitigation: base plus delta plus tombstones with
  rebuild-and-swap; the flat binary layout or LanceDB as fallbacks (S6).
- **Per-shard BM25 ranks disagree.** Mitigation: shared statistics or RRF, chosen on the stored query set in S6.
- **First index takes days on a large archive.** Mitigation: priority order, honest coverage per tier, names first;
  extraction rate from S2 sets the user-facing estimate.
