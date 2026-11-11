# Debrief 1.2 API design (package 0.5): the foundation

- Status: **Reviewed (0.5 gate passed, 2026-11-07)**; revision 5 folds the confirmation pass's P2s. The user decided Q1,
  Q2, Q15, Q17 and Q27 on 2026-11-07, all as recommended (§17). Revision 3 answers
  the independent review of `b9695e5e5` (P1-1 to P1-7 and its P2s).
- **Revision 6 (2026-11-12, landing):** answers the staleness review of `fff25c189` (P1-1 to P1-5, P2-1 to P2-6, P2-8,
  pre-check 5). It lands this file with the ADRs it reads (Debrief 0064 to 0068, family 0052) and the plan refresh on
  `docs/debrief-1-2-api-landing`; folds the `paddock-limits` and IP1 amendments into §4.1, §4.3, §4.6 and §13.2; reuses
  `paddock-proc`'s start judge in §4.4; applies ADR 0070 (no built-in AI engine) and ADR 0062's trim (no tee); adds
  §14.13 (the restored scope, a stub), §5.10 (`debrief.sniff.v1`) and §17.1 (common practice and cost); four questions
  for the user are open in §17.2 (Q-U1 to Q-U4). Sections not named in §17.3's list are unchanged.
- **Revision 7 (2026-11-12, `docs/debrief-1-2-restored-scope` from `origin/docs/debrief-1-2-api-landing` `765346486`):**
  applies the user's answers of 2026-11-12 to Q-U1 (c), Q-U2 (b2), Q-U3 (b) and Q-U4 (a), and replaces §14.13's stub with
  the design of the restored scope (investigation-folder features, ADR 0068's checks beyond WP-1: 27 packages, 116 raw plus 13 for the `paddock-tools` and `paddock-deps` subset WP-12 needs; round 2's fixes on 2026-11-12, §17.4).
  Q-U5 (the cheaper options, ER-37) is open. What changed: §17.4. Heads read for revision 7, re-read right before its
  commit (ADR 0091's block):

```evidence-heads
read: 2026-11-12 (revision 7, round 2)
origin/docs/debrief-1-2-restored-scope 72417e82c
origin/docs/debrief-1-2-api-landing 765346486
origin/docs/paddock-tools-design-v2 a614485c9
origin/main e2a318345
origin/train/2026-11-11-5 607bb7928
origin/debrief/p0-idle a0fd29185
origin/debrief/1.2-int 852b35ac0
```

  Source read for revision 7 (on `765346486`, this branch's base): `apps/debrief/crates/debrief-core/src/report/`,
  `debrief-ai/src/assistant/{citations,incident}.rs`, `debrief-notes/src/anchor.rs`, `debrief-core/src/{threads,spans/mod}.rs`,
  `app/src-tauri/src/{lib,investigate/incident}.rs`, `app/src/app/{dumps,share,investigate}/*-backend.ts`,
  `crates/paddock-pitwall/src/{lib,client,model,request}.rs`; on `origin/main` `e2a318345`: `crates/paddock-proc/src/lib.rs`,
  `crates/paddock-packs/src/lib.rs` and the shell's `define*` exports under `ui/projects/paddock-ui`; ADRs 0067 and 0068
  as landed on this branch. Round 2 also read: `apps/debrief/crates/debrief-budget/src/lib.rs` on `origin/debrief/p0-idle`
  `a0fd29185` (MEM-2), ADR 0051's work packages on `origin/docs/paddock-tools-design-v2` `a614485c9`, ADR 0028's on
  `origin/main`, `crates/paddock-pitwall/src/wire.rs` (`Provenance`, no serde) and ADR 0031 line 191.
- Scope, by the user's decision of 2026-11-07 (plan §10, "full foundation in 1.2, then one function per release"):
  every package of the plan that §10.3 does not move out has its API here. Revision 2 (same day) extends revision 1,
  which had wrongly limited itself to a narrower foundation, to that scope; §3 to §6 are unchanged.
- The dialogs part is [debrief-1-2-api-dialogs.md](debrief-1-2-api-dialogs.md) (ADR 0026, accepted 2026-10-28), unchanged.
- Base: `docs/debrief-1-2-api` from `origin/docs/debrief-1-2-plan-refresh` `abac43c0c` (which contains
  `origin/debrief/1.2-int` `852b35ac0`), with that branch's `ac85e26c7` (the release split, plan §10) merged in.
  Revision 6 is checked against `origin/train/2026-11-11-5` (the landing branch's base): paths without a branch in
  revision 6's changes are from there. The heads read for revision 6, re-read right before its commit (ADR 0091's block;
  the check that reads it is deferred, so the block is for the reviewer):

```evidence-heads
read: 2026-11-12
origin/train/2026-11-11-5 c5de1b427
origin/main e2a318345
origin/docs/debrief-1-2-api fff25c189
origin/docs/debrief-1-2-plan-refresh ac85e26c7
origin/docs/debrief-streaming-trait ed84adfe6
origin/docs/debrief-bounded-and-timezone df3f58a5b
origin/docs/debrief-folder-import-dialog 0242fcd81
origin/docs/debrief-dump-analysis-adr ad386a9bb
origin/docs/adr-wiring-outcomes-ui fed6b5855
origin/debrief/p0-idle a0fd29185
origin/debrief/1.2-limits-adopt 0d315d649
origin/docs/debrief-analysis-usefulness 95be52908
```

**Read for this design** (all on the named commits; nothing was built or run):

| Input | Where |
|---|---|
| Plan, rows of waves 0-9, §9 refresh | [debrief-1-2.md](debrief-1-2.md) at `abac43c0c` |
| Family ADRs 0001 (platform, wire, streamed calls), 0003 (Debrief's move, protocol-1 mapping), 0031 (patience) | `docs/adr/` on this branch |
| Debrief ADR 0064 (one streaming import trait, Accepted 2026-11-06) | `origin/docs/debrief-streaming-trait` `ed84adfe6` |
| Debrief ADRs 0065 (bounded replies, Accepted r5) and 0066 (how times are read, Accepted r5) | `origin/docs/debrief-bounded-and-timezone` `df3f58a5b` |
| Debrief ADR 0067 (folder import dialog, Accepted r5) | `origin/docs/debrief-folder-import-dialog` `0242fcd81` |
| Debrief ADR 0068 (dump checks, Accepted 2026-11-07; scope split across releases) | `origin/docs/debrief-dump-analysis-adr` `ad386a9bb` (revision 1 read `1ba8e4ecd`) |
| Family ADRs 0020 (shared executor, with its 2026-10-30 amendment) and 0052 (wired, outcome and layout contracts, Accepted) | `docs/adr/0020-shared-executor.md`; `origin/docs/adr-wiring-outcomes-ui` |
| Roadmap proposal, layers A-M, FD-* items (approved) | `origin/docs/debrief-analysis-usefulness`, `apps/debrief/docs/research/2026-11-07-roadmap-proposal.md` |
| Debrief ADR 0063 (binary wire, **Proposed**) | `origin/docs/debrief-binary-wire` |
| Source: `paddock-plugin-api`, `-plugin-host`, `-jobs`, `-proc`; Debrief's `debrief-plugin-api/src/protocol.rs`, `debrief-batch/src/ids.rs`, `debrief-plugin-host/src/`, the command and port tables | this branch (`852b35ac0` for Debrief) |

Marks: **[S]** read in source on this branch; **[A]** decided by the named ADR, restated here only as far as a signature
needs it; **[J]** a design choice made here; **[U]** not verified. Section numbers are what packages cite: "api §3.4".

**Where this file defers.** An accepted ADR stays the authority for its decision. Where this file gives a signature for
something an ADR already shows (ADR 0064's `Importer`, ADR 0065's `Bounded`), the ADR wins on meaning and this file
fills in what it left as `/* ... */`, names, crate paths, test homes and Settings. Where this file had to correct an
ADR, the correction is listed in §17 with a recommendation and is not applied silently.

## Contents

0. [Scope: what is in 1.2](#0-scope-what-is-in-12)
1. [Rules every API here follows](#1-rules-every-api-here-follows)
2. [Ids, ordinals, evidence references, process instances](#2-ids-ordinals-evidence-references-process-instances)
3. [Streaming import (ADR 0064)](#3-streaming-import-adr-0064)
4. [The shared plugin platform](#4-the-shared-plugin-platform)
5. [The `debrief.*` extension namespace](#5-the-debrief-extension-namespace)
6. [Folder open (ADR 0067)](#6-folder-open-adr-0067)
7. [The store](#7-the-store)
8. [Memory bounds](#8-memory-bounds)
9. [Jobs](#9-jobs)
10. [Time model (ADR 0066)](#10-time-model-adr-0066)
11. [Findings: the shared `Finding` from ADR 0068's check report](#11-findings-the-shared-finding-from-adr-0068s-check-report)
12. [Logs (ER-27)](#12-logs-er-27)
13. [Engine-to-UI contract](#13-engine-to-ui-contract)
14. [The rest of 1.2: lists, timeline, shell, UI on foundation types, performance, analyses, correctness](#14-the-rest-of-12-lists-timeline-shell-ui-on-foundation-types-performance-analyses-correctness)
15. [Package index: which section each package cites](#15-package-index-which-section-each-package-cites)
16. [Settings introduced or touched](#16-settings-introduced-or-touched)
17. [Open questions, corrections, recommendations](#17-open-questions-corrections-recommendations)
18. [Completeness](#18-completeness)

## 0. Scope: what is in 1.2

The user decided on 2026-11-07: **the full foundation in 1.2, then one function per release** (plan §10 on
`origin/docs/debrief-1-2-plan-refresh` `ac85e26c7`, merged into this branch). So every package of the plan that §10.3
does not mark as moved is in 1.2 and has its API here.

**Moved out of 1.2, not designed here** (plan §10.3): Y1 AI memory, Y2 semantic search (with X55), N4, N5, N0, N7, Y5
tray popup, Y9's small items, Y13 (format gaps, PML, batch queries), Y14's R5 UI items, ADR 0069's series (the UMDH
redesign). §14.12 names the foundation type each of them will build on, so that nothing in 1.2 has to change when they
come.

**Back in 1.2 by the user's decision of 2026-11-11, designed in §14.13 (revision 7; the user chose Q-U4 (a) on
2026-11-12):** the investigation-folder features plan §10.3 had moved out (the time shapes `+h:mm` and spaced dates,
citations, report blocks, the thread/sequence view, Pitwall report drafting, and the assessment's two small items) and
ADR 0068's checks other than WP-1 (crash, hang, memory, .NET managed state). They are 116 raw days in 25 packages, plus 13 for the two prerequisite packages PT-1 and PT-2
(§14.13.8), built on the foundation types; §2 to §13 change only by the user's answers of 2026-11-12 (§17.2) and §2.5's run split
(owned by FD-P, so the thread view adds no identity of its own).

**Where each part of 1.2 is designed:**

| Part | Sections |
|---|---|
| Ids, references, process instances (FD-U, FD-B, FD-P, FD-E1, FD-E2) | §2 |
| Streaming import (SI-0 to SI-18, 3.4, 3.5, MEM-3, MEM-4, PH4) | §3 |
| Shared plugin platform (1.4 + B1, 2.2, 2.6a to 2.6d, IP1, PH1, SI-4, SI-7, J1, P1, BP1) | §4 |
| The `debrief.*` namespace (PH2a, PH2b, PH3, SI-8) | §5 |
| Folder open (F1 to F12, FM, FR) | §6 |
| Store, memory bounds, jobs, time, findings, logs, engine-to-UI (§7 to §13) | §7 to §13 |
| Lists and the virtual primitive (1.3, 2.1, 2.7, 4.5, 3.7) | §14.1 |
| Timeline and the drawing build (2.4, 3.3, 4.1, M0, the flame and chart fixes), with the canvas vs DOM measurement plan | §14.2 |
| Shell and workbench move (M1 to M5, Y12); dialogs (1.1, 1.2, M2b) by reference to [debrief-1-2-api-dialogs.md](debrief-1-2-api-dialogs.md) | §14.3 |
| UI on foundation types (2.3, B2, 3.2, 4.4, 4.2's Limits group, 3.6's Help page) | §14.4 |
| Store and query performance (ST, Q, Y11, Y4, Y15, SR, IX, P2 to P5, NR1, CPU-1, NB-1, D-0, 4.3, 5.1, 5.2, D-b, B1 bench gates, Y3's rest) | §14.5 |
| Analyses at scale (FD-F, FD-K, FD-M) and correctness (C1 to C5) | §14.6, §14.7 |
| Memory (MEM-5, MEM-6 ADR, FD-G) | §14.8 |
| Names, packs, AI that stays (N1, N2, N3, N6, N8) | §14.9 |
| Family obligations (X1 to X11, Y6, Y7, Y8, Y16, A1's checks) | §14.10 |
| Rule 47 in Debrief's views (Y10) | §14.11 |
| What remains of Y9 and Y14 (the job service on the executor, R0 and R3) | §9.5, §14.10 |
| Restored to 1.2 on 2026-11-11: investigation-folder features (IF-1 to IF-6, C2), ADR 0068's checks other than WP-1 (WP-0, WP-2 to WP-18, WP-RR) | §14.13 |

**Priority order used** (the user's): streaming import and folder open first: §3 to §6 were written to completion
first and are unchanged by the scope correction. §18 says what is complete.

## 1. Rules every API here follows

These are restated once so that each section can be short. They are the binding rules of AGENTS.md, `docs/rules.md` and
ER-17 to ER-20, ER-27, applied to signatures.

### 1.1 Crate placement

| Kind of code | Home | Pattern |
|---|---|---|
| Generic mechanism, no Debrief type | `crates/paddock-*` (shared, rule 42) | Pure core; a `tauri` feature for the adapter; typed `thiserror` errors; no `unwrap` on I/O; ≥ 99 % lines |
| Debrief's plugin contract (domain types on the wire) | `apps/debrief/crates/debrief-plugin-ext` (new, PH2a) | Postcard bodies; depends on `paddock-plugin-api` only; **no** DuckDB, Tauri or zone database |
| Debrief's domain logic | `debrief-core` (pure), `debrief-store-duck` (store adapter), `debrief-dump`, `debrief-profile` | Ports owned by `debrief-core` (`ports/`); adapters never imported by the core |
| Commands | `app/src-tauri/src/<area>.rs` | Thin: parse, call the engine, map errors to `CmdError` |
| UI | `app/src/app/<area>/` | Standalone, OnPush, signals; an abstract `<Area>Backend` class injected by DI, with a Tauri and a mock implementation; never `invoke` outside `core/activity/ipc.ts` |

A `cargo tree` check (PH2a) fails when any `paddock-*` crate depends on a `debrief-*` crate.

### 1.2 Errors

- Every crate has one public error enum per port or module, `#[non_exhaustive]` only where a later minor may add a
  variant, with a `kind()` that maps to a stable code. A limit is never a string: it is `paddock_limits::LimitExceeded`
  (§4.6) inside the error.
- On the plugin wire, an error is `paddock_ipc::RemoteError { code, message }`; contract-level failures that the host
  must act on (a budget, a refusal) are typed **reply variants**, not error strings (§5).
- At the command edge, `CmdError { code: ErrorCode, message, limit?: LimitExceeded, cause_chain }`, generated to
  `core/generated/errors.ts` (exists [S]).

### 1.3 Bounded resources

- Every reply that holds a collection is `paddock_limits::Bounded<T>` or listed in `apps/debrief/bounded-exempt.toml`
  with a reason (ADR 0065 §5) [A].
- Every limit is a registry row with a `LimitId` (§4.6); fixed numbers at call sites are not allowed by the scan
  (ADR 0065 part 3).
- Every queue between threads or processes is bounded; a full queue blocks (back-pressure) or skips with a count, and
  the choice is named in the doc comment.

### 1.4 One wire contract per boundary

| Boundary | Encoding | Single definition | Check |
|---|---|---|---|
| Plugin to app | postcard over `paddock-ipc` frames | `debrief-plugin-ext` (domain), `paddock-plugin-api` (platform) | round-trip test of every body; fuzz target per decoder (IP1) |
| Store worker to app | JSON text today (`debrief-store-client/src/protocol.rs`) [S] | `store_port_table!` | `WIRE_VERSION` bump with every port change |
| Engine to UI | JSON today; MessagePack + Arrow if ADR 0063 is adopted | Rust types with `ts-rs` → `app/src/app/core/generated/*.ts` | generated files checked in CI (`git diff --exit-code` after generation) |

Postcard rules (family ADR 0001, "Versioning"): appending a variant at the end of an enum is a minor change; appending
a field to a struct is not (it fails in both directions). **The escape route is ADR 0001's own:** new fields travel in a
new variant appended at the end of the request or reply enum, which carries them. So this file:

- gives every postcard struct the fields it can name now (`ordinal`, `ts_state`, `time`), with a defined empty value
  until their producers land;
- carries shapes that will grow as **JSON** inside the body, where ADR 0001 makes a new `Option` or `#[serde(default)]`
  field a minor change: the Describe reply and every `*Cap` (§5.2), `FileInfo.times` (§5.3), the profile and document
  option bodies (§5.7) and the view filter (§5.8);
- never relies on appending a field to a postcard struct later.

### 1.5 Work roles and identities

- **Rule 52.** Every long call declares `WorkRole::{Live, Bulk}` (§4.1). Bulk runs at `QOS_CLASS_UTILITY`, nice 10,
  `THREAD_PRIORITY_BELOW_NORMAL`; Live at normal level. Nothing a person waits on runs at a starvable level. A starvable
  level needs a `conformance: background-qos — <why>` comment (C54).
- **Rule 53.** A process is acted on only through a held handle or a `paddock_proc::ProcessId` verified just before
  acting. A *recorded* identity of a process seen in evidence (§2.5, `ProcessInstance`) is never used to act on a
  process.

### 1.6 Jobs and progress (ER-17 to ER-20)

Every operation that can take longer than a frame runs as a job (§9): it returns a `JobId` at once, reports phases each
on its own scale, never goes backwards, takes Cancel in every phase and leaves state consistent (build, then swap). The
UI never times out a job; liveness is the stall judgement (§4.4). Each long or bulk operation's design states its
parallelism bound and the cheaper alternative checked; §3.8, §6.8 and §9.4 do so for the foundation.

### 1.7 What is a Setting

A value is a **Setting** when a person may reasonably need a different value on their machine or data and the engine can
validate it: thread counts (always with "Auto"), memory shares, limits a person can raise,
display choices (the stall judge has one, the patience multiplier `performance.stall.patience`; its per-key thresholds are constants, §4.4). It is a **hard limit** (registry row marked `hard`) when raising it would break a guarantee (frame size,
line limit, entry count). It is a **constant** (not in the registry) when it is a heuristic about data with a test on
each side of the edge (ADR 0066's `BACKWARDS_TOLERANCE`). It is **data** (a TOML pack, rule 21) when it is knowledge about
formats (`importers/memory.toml`, `recommend.toml`, `windows-zones.toml`). All new Settings are listed in §16, each with
its path, default, bounds and owner package.

## 2. Ids, ordinals, evidence references, process instances

Roadmap layer E and B (FD-U, FD-B, FD-P, FD-E1, FD-E2). These come first because the streaming wire (§5) and the folder
and store APIs (§6, §7) carry them, and because a `u32` id built into citations or report blocks would be undone later.

### 2.1 `EventId`, `SourceId`: the declared bound (FD-U)

**Decided by the user, 2026-11-07 (Q2):** ids stay `u64` in Rust and plain `number` in TypeScript, under a declared
bound checked where they are made. ADR 0063 does not change this: under MessagePack, integers above 2^53 lose precision
exactly as with `JSON.parse` today [S: ADR 0063 "Numbers"].

```rust
// debrief-batch/src/ids.rs (exists; gains the bound and a checked constructor)
/// A stable event identity: assigned once by the store's writer, never reused or renumbered.
/// Below `EventId::LIMIT` (2^53) so that it is exact as a JavaScript number; the writer refuses to allocate past it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize), serde(transparent))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, type = "number"))]
pub struct EventId(u64);                           // field private from FD-U on

impl EventId {
    pub const LIMIT: u64 = 1 << 53;                // first id that cannot be made
    pub fn new(v: u64) -> Result<EventId, IdOutOfRange>;
    pub fn get(self) -> u64;
}
#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for EventId { /* through EventId::new: parse, don't validate */ }

pub struct SourceId(pub u32);                      // unchanged: at most 2^32 sources per store (ADR 0051)
```

- **Where the bound is enforced:** the writer's block allocator (`debrief-store-duck/src/segments.rs`, id blocks) asks
  `EventId::new` for the block's last id; past the bound the import stops with `LimitExceeded { store.event_ids }`
  (registry row, `hard`, §4.6). 2^53 is about 9 × 10^15, so the row exists to be tested, not to be met [calc in the
  roadmap].
- **Every DTO carries `EventId`, never `u32`** (FD-U). The list of `u32` sites is the roadmap's layer E list [S there]:
  QL results, anomaly and outlier `sample_ids`, Moment's `record_id`, the store's anomaly ids, report blocks, AI
  citations, `ContextRequest.record_id`. `sample_id()` goes. A test in `debrief-core/tests/it/ids.rs` builds an id of
  `2^53 - 1`, sends it through every port DTO that names an event (serde round trip), and asserts it comes back equal.
- **TypeScript:** `export type EventId = number;` in `core/generated/store.ts`. A spec in
  `app/src/app/core/generated/ids.spec.ts` round-trips `2 ** 53 - 1` through the IPC decode helper.
- **Plugin wire:** plugins never send ids (§5.3): the store assigns them.

### 2.2 Ordinals within a source (FD-B, an ADR 0062 amendment)

A total order inside a source that survives reload and re-time. `(source, line)` already gives it for text sources
[S: roadmap layer B]. Non-text importers declare their own ordinal.

```rust
// debrief-plugin-ext::import (the contract; see §3.2)
/// What orders the events of one source. Declared by every importer (`Importer::ordinal`, no default).
pub enum OrdinalKind {
    /// The 1-based line of the event's first line (text formats).
    Line,
    /// A number the format itself carries (an EVTX record id): declared by the importer, not checked, and it may repeat
    /// (Q-U2 (b2), §2.2: the event id breaks the tie).
    RecordNumber,
    /// The position of the record in the file (a CSV row, an OTLP record index, a video frame): counted by the reader.
    Position,
}
```

- `Importer::ordinal()` has **no default** (§3.2), so an importer does not compile without declaring one; the contract
  test (`debrief-plugin-test`, FD-B) fails a non-text importer (`Access` other than `Lines`) that declares `Line`.
- The event carries `ordinal: Option<u64>` on the wire (`WireEvent`, §5.3) and in `ParsedEvent` (§3.2). `None` under
  `OrdinalKind::Line` means "the line is the ordinal". The store's column is FD-B's decision (a new `ordinal UBIGINT`, or
  `line` widened); the wire carries the field from v1 either way (§1.4).
- **Declared, not checked; may repeat.** Increasing does not imply unique across a split file, and unique does not imply increasing:
  an EVTX log is a circular buffer, so its record numbers rise across the wrap while file order does not [S: `evtx`
  walks the oldest chunk first]. The order inside a source is the ordinal, not the file position.
  - `Line` and `Position`: made by the reader, strictly increasing by construction; equal to the previous is a bug,
    caught by a debug assertion and the contract test;
  - `RecordNumber`: **declared unique by the importer, not checked** (the user's decision of 2026-11-12, Q-U2 (b2)).
    A real log can repeat a record number (a log cleared and refilled, a file stitched from two exports); every such
    event is kept, and the event id, the last part of the order key, breaks the tie. There is no per-source seen-set, no
    `import.ordinal_set_bytes` row and no problem kind for a repeat (revision 6's `DuplicateOrdinal` and
    `OrdinalUnchecked` are gone). Test (FD-B, `debrief-store-duck/tests/ordinal.rs`): two events of one source with the
    same `RecordNumber` are both stored, in id order, and keep that order across a reload.
- **One order key.** FD-B amends ADR 0062's `(ts, source, line, id)` to `(ts, source, ordinal, id)`, where the ordinal
  of a text source is its line, so nothing changes for text; `PageAnchor::Key` (§14.5) and `ViewCursor` (§5.8) use the
  same key.
- Re-time (ADR 0066 §8) moves `ts` and renumbers `pos`, never the ordinal or the id [A: ADR 0066 keeps ids and blocks].

### 2.3 Source identity across reloads

ADR 0051 keeps source ids stable by path and fingerprint [A]. The reference type below needs that identity in a form
that survives a project moved without its logs:

```rust
// debrief-core::evidence
pub struct SourceKey {
    pub id: SourceId,                 // fast path inside one store
    pub root_rel: RelPath,            // path relative to its import root (`RelPrefix` lookup, §6.6); never absolute
    pub fingerprint: Fingerprint,     // ADR 0051's (size and both edges)
}
```

### 2.4 `EvidenceRef` (FD-E1 confirms, FD-E2 builds)

One reference to a piece of evidence, used by notes, report blocks, AI citations and findings (rule 55: three copies
today [S: roadmap layer E]). This section fixes the **shape** so §5, §11 and the store can carry it; FD-E1's ADR decides the
resolution rules and may tighten fields, not add parallel types.

```rust
// debrief-core::evidence  (ts-rs → core/generated/evidence.ts)
pub struct EvidenceRef {
    pub target: EvidenceTarget,
    /// When the target happened, with how the time was read (ADR 0066). `None` for timeless evidence (a module).
    pub time: Option<StampedTime>,               // §10.3
    /// Kept text, at most 240 characters, for "gone" and for the redacted copy; redacted like `raw`.
    pub excerpt: Excerpt,
}

pub enum EvidenceTarget {
    /// An event: located by source and ordinal, checked by hash, found fast by id.
    Event { source: SourceKey, at: EventLocator, hash: ContentHash, hint: Option<EventId> },
    /// A part of an open document (dump, capture, UMDH snapshot): ADR 0068's evidence kinds name `item`.
    Document { doc: DocumentKey, item: DocItem },
}
pub enum EventLocator { Line { line: u32 }, Ordinal { kind: OrdinalKind, n: u64 }, ByteOffset { offset: u64 } }
pub struct ContentHash(u64);                     // xxh3-64 of the event's `raw`, or of its canonical record for `Raw::None`
pub struct DocumentKey { pub file: FileIdentity, pub kind: DocumentKind }   // FileIdentity: ADR 0068 §1's cache identity
pub enum DocItem { Thread { id: u32, frame: Option<u16> }, Module { index: u32 }, Region { va: u64 },
                   Handle { value: u64 }, ManagedType { name_hash: u64 }, CallSite { hash: u64 }, CaptureNode { id: u64 } }

/// Resolution (FD-E2), bounded like every reply (ADR 0065).
pub enum Resolved { Found { id: EventId }, Moved { id: EventId, how: MovedBy }, Gone { why: GoneWhy } }
pub enum MovedBy { Locator, Hash }
pub enum GoneWhy { SourceRemoved, SourceChanged, NotInStore, DocumentNotOpen }
```

- Port (FD-E2): `Inspect::resolve(refs: &[EvidenceRef]) -> Bounded<Vec<Resolved>>`, one row in `store_port_table!`, at
  most 1,000 refs per call (`evidence.resolve_batch`, `hard`); budget from the roadmap: 1,000 refs under 200 ms on the 1B
  store [J target], measured in FD-E2 and recorded under rule 48.
- **Room left for features:** `DocItem` is an enum on JSON wires (UI, store worker), so a variant appended later is
  minor. On the plugin wire it travels only inside a JSON body (`debrief.analysis.v1`, §5.9), never as postcard.
- Tests: `debrief-core/src/evidence/tests.rs` (pure: serde, bounds of `Excerpt`); `debrief-store-duck/tests/evidence.rs`
  (found, moved by locator after a reload, moved by hash after a re-time, gone after removal).

### 2.5 `ProcessInstance` (FD-P)

"The same process" for dumps (ADR 0068 §5), UMDH series (ADR 0069), the thread view and evidence. A **recorded**
identity read from evidence, never a handle (rule 53, §1.5); live processes keep `paddock_proc::ProcessId`.

```rust
// debrief-core::process   (ts-rs → core/generated/process.ts)
pub struct ProcessInstance {
    pub pid: u32,
    /// Process create time as recorded, with provenance (a dump's `ProcessCreateTime` is seconds, UTC).
    pub started: Option<StampedTime>,
    pub image: Option<ImageIdentity>,
    pub machine: Option<MachineKey>,           // host field or system report, when known
    /// Which run of this pid inside one source, when no start time is recorded (a log line rarely has one): runs are
    /// split at a restart marker (a lifecycle pair of `crate::spans`, e.g. "service started"), 0 for the first. `None`
    /// when the source has no restart marker for this pid, or when `started` is known (the start time decides then).
    pub run: Option<u32>,
}
pub struct ImageIdentity { pub path: Option<String>, pub code_id: Option<CodeId> }   // CodeView GUID + age, or build id
pub struct CodeId(Box<str>);

/// Whether two recorded instances are the same process. Pure; every decision has a reason code that is logged.
pub fn same_process(a: &ProcessInstance, b: &ProcessInstance) -> Pairing;
pub enum Pairing {
    Same,
    Unconfirmed { why: UnconfirmedWhy },      // confidence at most Low downstream (ADR 0068 §5)
    Different { why: DifferentWhy },
}
pub enum UnconfirmedWhy { CreateTimeMissing, ImageMissing, MachineUnknown, RunByMarkerOnly }
pub enum DifferentWhy { PidDiffers, CreateTimeDiffers, MainModuleDiffers, MachineDiffers, RunDiffers }
```

- ADR 0068's `Pairing` and its reason codes (`PidDiffers`, `CreateTimeDiffers`, `CreateTimeMissing`,
  `MainModuleDiffers`, `Same`) [A: ADR 0068 §14] **are** this type; WP-6 uses it and does not define another. The
  earliest thread create time as a second witness (ADR 0068 §5) is a feature-side input to `same_process`'s caller.
- **Runs without a start time (revision 7, owned by FD-P; rule 55).** When both instances lack `started`, `same_process`
  compares `run`: the same pid with different runs is `Different { RunDiffers }`; the same run is `Unconfirmed {
  RunByMarkerOnly }` (a marker split runs, nothing proves they are one process); no `run` on either side is `Unconfirmed {
  CreateTimeMissing }` as before. The thread view (§14.13.4) uses this type and this function; it defines no process
  identity of its own. The split is a pure function `debrief_core::process::split_runs(markers: &[(EventId, i64)], pid:
  &str) -> RunMap` over the spans' lifecycle pairs, so FD-P owns it and IF-3 calls it.
- Tests: `debrief-core/src/process/tests.rs`, a table of pairs per reason (including both run cases), and a property test
  that `same_process` is symmetric.

## 3. Streaming import (ADR 0064)

The trait, its inputs and the sink are ADR 0064 §1 [A]. This section fixes the module layout, the types ADR 0064 left
open, registration, the in-process host, tests and Settings. Packages: SI-0 to SI-18, MEM-3, MEM-4, 3.5, PH4, 3.4.

### 3.1 Module layout

```
apps/debrief/crates/debrief-plugin-ext/src/
  lib.rs            re-exports; `pub mod` per file below; no logic
  model.rs          Record, Level, Value, Fields            (moved from debrief-plugin-api/src/model.rs)
  import/
    mod.rs          Importer, DocumentReader, ImportCtx, SniffCtx, Head, ImportEnd, StopReason, ImportError
    input.rs        ImportInput, Access, Lines, Decoded, Bytes, Seekable, ByPath, MemoryInput, WholeReason
    sink.rs         ImportSink, ParsedEvent, Raw, SinkClosed, ProblemKind (re-export of problems)
    budget.rs       RecordBudget, the framer counter
    chunk.rs        ReaderChunks (moved from debrief-core/src/ingest/chunk.rs, SI-2)
    decode.rs       the decode half of encoding.rs (SI-2)
    register.rs     ErasedImporter, ImporterCap, the registration (Input::ACCESS → manifest)
  time.rs           TimestampParser, TimeReader, TimeTally, Stamp, Written (ADR 0066; moved from debrief-plugin-api)
  problems.rs       Problems, Problem, ProblemKind                       (moved)
  wire/             the debrief.* contracts of §5, one file per namespace
  config.rs, annotate.rs, transform.rs, markers.rs, profile.rs, document.rs    domain halves, moved as they are
```

Until PH2a creates the crate, SI-1 puts `import/` in `debrief-plugin-api/src/import/` and PH2a moves the folder whole
(ADR 0064 §1) [A]. `debrief-core` re-exports `debrief_plugin_ext::import` as `debrief_core::import` so built-in and
plugin importers implement one trait.

### 3.2 The types ADR 0064 left open

```rust
// import/mod.rs
pub trait Importer: Send + Sync + 'static { /* ADR 0064 §1, with ADR 0066 D13's amendment (accepted) and: */
    /// ADR 0066 D13: what this importer does with times. **No default**, so a new importer does not compile until it says.
    fn times(&self) -> TimeBasis;
    /// What orders this importer's events (§2.2). **No default** (FD-B).
    fn ordinal(&self) -> OrdinalKind;
    /// ADR 0066 D13: `import` takes `&mut ImportCtx`, so `ctx.times()` can lend the one reader of the call.
    fn import(&self, ctx: &mut ImportCtx, input: Self::Input<'_>, sink: &mut dyn ImportSink) -> Result<ImportEnd, ImportError>;
    /// Whether the pool may split the file and call `import` once per chunk (core text and JSON lines only in 1.2).
    fn splittable(&self) -> bool { false }
    /// ADR 0067's preselection class (`log`, `event-log`, `dump`, `trace`, `config`, `image`, `untimed-table`).
    fn class(&self) -> ImportClass;
}

pub struct ImportCtx<'a> {
    file: &'a FileInfo,                // §5.3: name, root-relative path tail, size, mtime; never an absolute path
    head: &'a Head,                    // the plan's head, ≤ HEAD_BYTES (1 MiB), decoded once for text importers
    plan: &'a TextPlan,                // encoding, data_start
    times: TimeReader,                 // ADR 0066 §1; finish() called by the SDK, never by the importer
    tail: Option<&'a TailStart>,       // live tail only: the file's start (headers)
    progress: &'a dyn ByteProgress,    // §3.6: bytes delivered after the first event
    log: &'a dyn ImportLog,            // §12.2: phase lines, never content
}
impl ImportCtx<'_> {
    pub fn file(&self) -> &FileInfo;
    pub fn head(&self) -> &Head;
    pub fn times(&mut self) -> &mut TimeReader;
    pub fn tail(&self) -> Option<&TailStart>;
}
pub struct SniffCtx<'a> { pub file: &'a FileInfo, pub encoding: Option<&'a str> }
/// ADR 0066 D13 (its "A new importer cannot skip it", 1).
pub enum TimeBasis { Reads, NoTimes { why: NoTimesWhy } }
pub enum NoTimesWhy { FormatHasNone, DocumentTimesOnly, Other(Box<str>) }
pub struct Head { bytes: Box<[u8]>, text: Option<Box<str>>, complete: bool }   // `complete`: the head is the whole file
impl Head { pub fn bytes(&self) -> &[u8]; pub fn text(&self) -> Option<&str>; pub fn is_whole_file(&self) -> bool }

pub enum DeclaredSniff {
    /// Decided from the manifest: extensions (lower-case, no dot) and magic bytes at offsets. No process starts.
    Static { extensions: Vec<Box<str>>, magic: Vec<Magic>, score: u8 },
    /// `Importer::sniff` is called (one batched call per plugin per file, §5.10).
    Dynamic,
}
pub struct Magic { pub offset: u32, pub bytes: Box<[u8]> }         // offset + len ≤ 64 KiB

pub struct RecordBudget(NonZeroU64);                                // bytes; `import.record_bytes` when `None`
pub enum TailSupport { Reread, Appended }

pub enum StopReason { Damaged, CutOff, RecordOverBudget { limit: LimitExceeded }, UnsupportedVersion }

/// Fails the source. Every variant has a cause chain for the logs and a word key for the UI.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("not this format: {0}")]          NotThisFormat(Box<str>),
    #[error("nothing readable")]               NothingReadable,
    #[error("read failed")]                    Read(#[source] std::io::Error),
    #[error("cancelled")]                      Cancelled,
    #[error(transparent)]                      Limit(LimitExceeded),
    #[error("the host could not serve the file")] Host(#[source] HostFileError),
}

// import/sink.rs
pub struct ParsedEvent { pub record: Record, pub raw: Raw, pub ordinal: Option<u64> }
pub enum Raw { None, Text { text: String, byte_offset: u64 } }
pub struct SinkClosed { pub why: SinkClosedWhy }
pub enum SinkClosedWhy { Cancelled, HostGone }
```

`Record.id` is dropped (ADR 0064 §1, "Importers no longer report `Vec`s") [A]; `Record.line` stays `u32`, saturating per
file. `ImportSink` is ADR 0064's `EventSink`, renamed because `paddock_jobs::reporter::EventSink` exists and both are
in scope in the store worker (§17, Q23).

### 3.3 Inputs and registration

```rust
// import/input.rs
/// The input kinds; `Input::ACCESS` fills the manifest's `ImporterCap.access` (ADR 0064 §1, "Registration erases").
pub enum Access { Lines, Decoded, Bytes, Seekable, ByPath }
pub trait ImportInput: Sized { const ACCESS: Access; }
impl ImportInput for Lines<'_>    { const ACCESS: Access = Access::Lines; }
// ... one impl per input type, nothing else implements the trait (sealed).

/// The SDK's in-memory input for tests, the pasted-sample preview and `diag.rs` (ADR 0064, "Callers of the old traits").
/// Capped by the caller's request; never used by an import of a file.
pub struct MemoryInput { bytes: Arc<[u8]> }
impl MemoryInput {
    pub fn new(bytes: impl Into<Arc<[u8]>>, cap: u64) -> Result<MemoryInput, LimitExceeded>;
    pub fn lines(&self, plan: &TextPlan) -> Lines<'_>;
    pub fn decoded(&self, plan: &TextPlan) -> Decoded<'_>;
    pub fn bytes(&self) -> Bytes<'_>;
    pub fn seekable(&self) -> Seekable<'_>;
}

pub enum WholeReason { HeapSnapshot, Pprof, DumpWithoutMemoryList }   // the last goes with ADR 0068 WP-8 (§17, Q3)

// import/register.rs
pub struct ImporterCap {
    pub id: Box<str>,
    pub access: Access,
    pub whole: Option<WholeReason>,
    pub sniff: DeclaredSniff,
    pub tail: TailSupport,
    pub ordinal: OrdinalKind,
    pub times: TimeBasis,
    pub multiline: bool,                       // ADR 0064's `multiline()`
    pub record_budget: Option<u64>,            // ADR 0064's `record_budget()`; None: the Setting
    pub class: ImportClass,
    pub identity_fields: Vec<Box<str>>,
}
// ImporterCap and every other *Cap are serde camelCase and cross inside the JSON Describe reply (§5.2), so a later
// optional field is a minor change (§1.4).
/// Type-erased importer: what the host calls. Built only by `register`, so `access` cannot disagree with the code.
pub struct ErasedImporter { cap: ImporterCap, run: Box<dyn ErasedRun> }
pub fn register<I: Importer>(importer: I) -> ErasedImporter;
impl ErasedImporter {
    pub fn cap(&self) -> &ImporterCap;
    pub fn sniff(&self, ctx: &SniffCtx, head: &Head) -> u8;
    /// The host passes the input matching `cap().access`; a mismatch is a programming error caught by the type of `HostInput`.
    pub fn import(&self, ctx: &mut ImportCtx, input: HostInput<'_>, sink: &mut dyn ImportSink) -> Result<ImportEnd, ImportError>;
}
pub enum HostInput<'a> { Lines(Lines<'a>), Decoded(Decoded<'a>), Bytes(Bytes<'a>), Seekable(Seekable<'a>), ByPath(ByPath<'a>) }
```

- A plugin registers with `Contributions::add_importer(register(MyImporter))` (§4.1); the plugin's `Describe` reply
  lists the `ImporterCap`s inside `debrief.describe.v1` (§5.2), and the host checks them against the verified manifest.
- **`ByPath` refused at install** for a manifest that is not first-party (`spec.home.is_none()` is first-party) [A:
  ADR 0064 §2, "The chosen path"]. The check is `debrief_plugin_ext::register::check_caps(&Manifest, &[ImporterCap])
  -> Result<(), CapRefusal>`, called by Debrief's install path (PH2b) and by `debrief-plugin-test`.

### 3.4 The in-process host: from a source to the sink

The engine side of an import, in `debrief-core::ingest` (store worker process). Only the boundaries are fixed here; the
pipeline (ADR 0051, 0052) is unchanged [A].

```rust
// debrief-core/src/ingest/source.rs (rewritten by SI-1, SI-5, SI-10)
/// One source, from its open handle to its events in `sink`. The only caller of `judge` besides the folder scan (§6.2).
pub(crate) fn import_source(env: &SourceEnv, file: OpenSource, sink: &mut BatchSink) -> SourceOutcome;

/// A source the engine already holds open (rule 53: act on the handle). Made by `open_source` (§6.7's link-refusing open).
pub struct OpenSource { handle: File, meta: FileMeta, member: Option<MemberRef> }

/// How the engine satisfies the declared input (ADR 0064 §3, table "How the host satisfies"). Pure decision.
pub fn satisfy(access: Access, origin: SourceOrigin) -> Satisfy;
pub enum SourceOrigin { PlainFile, GzMember, ZipMember { needs_seek_ok: bool } }
pub enum Satisfy { Read, DecompressInOrder, Spool, Path, SpoolThenPath }

pub enum SourceOutcome {
    Imported { events: u64, end: ImportEnd, time: TimeReport, bytes_read: u64 },
    Refused(FileRefusal),                         // §6.2: same enum as the scan
    Failed { cause: SourceFailure },          // events removed (`abort_source`), listed with Retry
}
pub enum SourceFailure { Import(ImportError), Plugin(PluginFault), Gate(LimitExceeded), Spool(LimitExceeded) }
```

- **Plugin importers** are called through `debrief-plugin-host`'s thin adapter (PH2b, §4.9): `RemoteImporter::import`
  opens a `RemoteStream` on `debrief.import.v1` and hands each item to `BatchSink::write` as it arrives (SI-10, 3.4):
  nothing is collected.
- **Bystander retry** (ADR 0064 §2) [A]: `PluginFault` on a call fails every in-flight call of that process; each other
  file is retried once, serially, on the fresh process, with `begin_source(None)` (a fresh source id, §7.2).

### 3.5 Back-pressure and Cancel

One blocking chain, no added queue [A: ADR 0064 §4]: the store's slot lock blocks `BatchSink::write`; for a plugin, no
`Credit` frame goes back; `ItemSink::send` blocks in the plugin; `ImportSink::event` blocks; the parser stops reading.
Cancel is the import job's `CancelToken` (§9), reaching `RemoteStream::cancel` and `ImportSink::event` →
`Err(SinkClosed { Cancelled })`; every `read` fails with `io::ErrorKind::Other` carrying `ImportError::Cancelled`
(never `Interrupted`, ADR 0064 §1 notes).

### 3.6 Progress

```rust
// debrief-core::ingest::progress (exists as ImportProgress; gains per-input byte progress)
pub trait ByteProgress: Sync {
    /// Bytes delivered to the importer after its first event, cumulative, capped at the file's size. Monotonic.
    fn delivered(&self, bytes: u64);
    /// Before the first event: a growing count of reads ("Preparing <file>").
    fn preparing(&self, reads: u64);
}
```

Import job phases (§9.2): `finding`, `plan`, `spool`, `waiting-memory`, `read`, `corrections` (ADR 0066 §8a),
`maintain`. A `ByPath` importer reports a growing event count, no percentage [A].

### 3.7 Settings and limits

Registry rows from ADR 0064 §5 [A], each with its `limit_case!` in `debrief-core/tests/it/limits/import.rs` (SI-13, 3.6):
`import.record_bytes` (Setting), `import.line_bytes`, `import.event_bytes` (hard), `import.document_bytes` (hard),
`import.dump_whole_bytes` (Setting), `import.spool_bytes` (Setting), `archive.ratio` (Setting), `archive.entries`
(hard), `import.open_files` (hard). Paths in §16.

### 3.8 Parallelism and cost

Unchanged from ADR 0064's section [A]: files on the parse pool (`performance.import.parse_threads`, Auto = cores − 1, at most 8),
`2 × threads` files in flight, lowered on `EMFILE`; plugin calls bounded by files in flight; zip members one cloned
reader each; one spool per file in flight. Cheaper: sniff memo and batch (§5.10), first read grows from 64 KiB, one open
per file, `Vec<Record>` gone.

**The cost of the `CreditPool` (§4.2) in parallelism.** It caps open plugin streams at `limit / EVENT_FRAME_MAX`: at the
default 128 MiB at most 10 plugin files parse at once, at 64 MiB at most 5, against `2 × threads` (16) otherwise
[calc]. In-process files are not affected, so the cost falls on plugin-heavy bundles: an EVTX-heavy folder parses fewer
files at once. 2.6b's benchmark measures it: the same EVTX-heavy invented folder at 64, 128 and 256 MiB, recording
throughput and the store worker's peak footprint, and the default moves only with that result (rule 48).

### 3.9 Tests: where each lives

| What | Where | Package |
|---|---|---|
| Failing tests first (440 MiB UTF-16 `.reg`, 1.43 GiB EVTX, whole-read fake, zip bomb by ratio, budget cases) | `debrief-core/tests/it/stream_*.rs` (in process), `debrief-plugins/tests/it/stream_host_*.rs` (real IPC) | SI-0 |
| Trait, inputs, framer budget per input row of ADR 0064's table | `debrief-plugin-ext/src/import/*/tests.rs` | SI-1 |
| Read boundaries on UTF-16 without BOM, mixed code pages | `debrief-plugin-ext/tests/chunk_boundaries.rs` (moved from `stream_bundle.rs`) | SI-2 |
| Each converted importer against its `/samples` golden | the plugin crate's `tests/golden.rs` | SI-3, SI-11 |
| Flat memory for every importer, positive controls (collecting fake, `read_to_end` fake, raw `ReadFile` fake) | `debrief-plugin-test/tests/importer_memory_is_flat.rs` on `paddock-plugin-test`'s base | SI-14 |
| Static whole-read check | `conformance/src/checks/C<next>-whole-file-read.mjs` and its fixtures | SI-14 |
| Bundle acceptance (14,000 files, 7.6 GiB, 1B nightly) | `debrief-store-worker/tests/it/bundle_acceptance.rs`, results in `performance.md` | SI-15 |

## 4. The shared plugin platform

Packages 2.2, 1.4, IP1, 2.6a, 2.6b, 2.6c, 2.6d, PH1, SI-4, SI-7, J1, P1, BP1. Everything here is domain-free (§1.1)
and is adopted by `shell-demo` or `workbench-demo` in the same change where it has a UI.

**ADR 0070 (accepted 2026-11-08: Pitwall is Debrief's only AI engine) narrows two of them.** PH1 moves the store client,
the plugin host and every other child launch onto `paddock-ipc` and `paddock-proc`, with no AI worker
(`crates/debrief-ai/src/worker/` leaves its file list); LV1 puts the store client's `CALL_TIMEOUT`, `BUSY_WAIT`,
`TAIL_WAIT`, the ready handshake and the in-process watchdog on the judge, and has no AI worker `CONNECT_WINDOW` and no AI
runtimes' `REPORT_TIMEOUT` (ADR 0070's plan table: PH1 −1, LV1 −0.75 raw). No `AiWorker` process role (§4.5) and no
`ai.connect` call key (§4.4).

### 4.1 `paddock-plugin-api` additions (2.6b, SI-7, IP1 item 7)

```rust
// extension.rs
pub trait Contract: 'static {
    const NS: &'static str;
    const MINOR: u16;
    type Request: Serialize + DeserializeOwned;
    type Reply: Serialize + DeserializeOwned;
    /// Rule 52 role of a request (default Live). `serve` runs Bulk at the gentle level, Live at normal level.
    /// Not built yet: IP1 left it to 2.6b (amendment I4).
    fn role(_request: &Self::Request) -> WorkRole { WorkRole::Live }
    /// The minor that introduced `request`'s variant (default 0). Built in IP1 (I4): `Remote::call_within` refuses
    /// with `HostError::Unavailable` before sending to an older peer.
    fn introduced_in(_request: &Self::Request) -> u16 { 0 }
}
pub use paddock_proc::priority::WorkRole;   // defined once in paddock-proc (§4.5), re-exported here

/// A contract whose replies stream: items, then one reply.
pub trait StreamContract: Contract { type Item: Serialize + DeserializeOwned; }

pub trait StreamHandler<C: StreamContract>: Send + Sync + 'static {
    fn handle(&self, ctx: &ExtCtx, request: C::Request, items: &mut ItemSink<C::Item>) -> Result<C::Reply, RemoteError>;
}
/// Blocks while the host holds no credit; fails once the call is cancelled or the host is gone.
pub struct ItemSink<T> { /* the peer's stream writer */ }
impl<T: Serialize> ItemSink<T> {
    pub fn send(&mut self, item: T) -> Result<(), SinkClosed>;
    /// A `Progress` frame without an item (2.6d); the SDK's framer calls it at most once per MiB read (ADR 0064 §1).
    pub fn progress(&mut self) -> Result<(), SinkClosed>;
}

// plugin.rs
impl Contributions {
    pub fn add_extension<C: Contract>(&mut self, handler: impl ExtensionHandler<C>) -> &mut Self;          // exists
    pub fn add_stream_extension<C: StreamContract>(&mut self, handler: impl StreamHandler<C>) -> &mut Self; // new
}

// host_file.rs (SI-7): read_all and read_text are removed
pub struct HostFile { /* host, file ref, position, one 1 MiB read-ahead buffer, sticky fatal error */ }
impl Read for HostFile {}  impl Seek for HostFile {}
impl HostFile {
    pub fn new(host: Arc<dyn Host>, file: HostFileRef, cancelled: CancelCheck) -> HostFile;
    /// The first failure that was not the file's fault (cancel, host, disk). Stays set; every later read fails with it.
    pub fn fatal(&self) -> Option<&HostFileError>;
}
impl RangeRead for HostFile {}     // read_at stays, for document readers

// lib.rs (IP1 item 7, built: I7): what a plugin needs from paddock-ipc, so it depends on paddock-plugin-api alone
pub use paddock_ipc::{codes, RemoteError};
impl ExtCtx<'_> { pub fn cancelled_error(&self) -> RemoteError; }
```

**As built in IP1** (amendments I4 and I7, folded here 2026-11-12): `introduced_in` and the refusal before sending are
in; `role` is 2.6b's. The re-exports and `cancelled_error` are in, but "an example plugin builds with
`paddock-plugin-api` as its only platform dependency" is not proven by a separate example crate; PH2a's first
`debrief-plugin-ext` build is that proof (its `cargo tree` check, §1.1) [J].

`serve` (2.6b): sets the thread level per call from `Contract::role` (`paddock_proc::priority::for_role`, §4.5, the
only caller of `for_role`; jobs go through the executor's `Class`, §9.5); calls
`paddock_sandbox::lockdown` before any request (PH1).

### 4.2 `paddock-plugin-host` additions (2.6a, 2.6b, SI-7)

```rust
// supervisor.rs
impl Supervisor {
    pub fn remote<C: Contract>(&self) -> Result<Remote<C>, HostError>;            // exists
}
impl<C: Contract> Remote<C> {
    pub fn call(&self, request: &C::Request) -> Result<C::Reply, HostError>;      // exists
}
impl<C: StreamContract> Remote<C> {
    /// At most `credit` items in flight. A garbled item, more items than credits or a stall is a fault of the plugin.
    pub fn stream(&self, request: &C::Request, credit: Credit) -> Result<RemoteStream<C>, HostError>;
}
pub struct RemoteStream<C: StreamContract> { /* call id, peer, guard */ }
impl<C: StreamContract> RemoteStream<C> {
    /// Blocks for the next item; returns one credit to the plugin when the caller asks for the item after it.
    pub fn next(&mut self) -> Result<StreamEvent<C>, HostError>;
    pub fn cancel(self);
}
pub enum StreamEvent<C: StreamContract> { Item(C::Item), Done(C::Reply) }
pub struct Credit(NonZeroU32);                         // HostConfig::stream_credit, "Auto" = 4 until 2.6b's benchmark, max 64
/// Bytes of items in flight shared by every stream of one import (§8). Every credit a plugin holds, the initial grant
/// included, is a reservation in the pool, so the import's items in flight never pass the pool's limit.
pub struct CreditPool { /* limit, reserved bytes, FIFO of waiting streams */ }
impl CreditPool {
    /// `limit` is `performance.import.items_in_flight_mb`; at least `streams × EVENT_FRAME_MAX` (see `max_streams`).
    pub fn new(limit_bytes: u64) -> CreditPool;
    /// How many plugin streams may be open at once: `limit / EVENT_FRAME_MAX`. The import lowers its plugin files in
    /// flight to this (in-process files are not affected), so every open stream can always hold one item of any size.
    pub fn max_streams(&self) -> usize;
    pub fn reserved(&self) -> u64;
}
impl<C: StreamContract> Remote<C> {
    /// Opens a stream whose `credit` (its maximum) is granted one credit at a time from `pool`.
    pub fn stream_pooled(&self, request: &C::Request, credit: Credit, pool: &Arc<CreditPool>) -> Result<RemoteStream<C>, HostError>;
}
```

The pool's rules (2.6b's tests, with a fake plugin and no sleeps):

- **Reservation per credit.** Each credit reserves the stream's own item estimate: `max(ITEM_BYTES, the largest item seen on
  this stream)`, at most `EVENT_FRAME_MAX`. "Largest seen" is **per stream**, so one stream with large events does not
  shrink what the others are granted. When an item arrives, its encoded size replaces the reservation; the bytes are
  released when the batcher has taken the item.
- **Initial grant from the pool.** Opening a stream reserves its first credit at `EVENT_FRAME_MAX` (nothing is known yet)
  before the request is sent; a stream that cannot get it waits, FIFO, before it opens. So 16 streams × credit 4 × 12 MiB
  can never be granted up front.
- **Fairness.** Every open stream keeps at least one credit; further credits go one at a time, FIFO among the streams
  waiting for one; a stream never gets a second credit while another open stream has none.
- **No deadlock.** A stream's first credit is reserved before it opens, so an open stream can always make progress; the
  pool holds no lock while a stream waits.

```rust

// services.rs: host-side extension calls
/// The app's handler of one namespace's host side. The namespace is given once, at registration; a call whose `ns` the
/// manifest does not list is `denied` before it reaches the handler.
pub trait HostExtension: Send + Sync {
    fn handle(&self, plugin: &PluginId, minor: u16, body: &[u8], ctx: &HostCallCtx) -> Result<Vec<u8>, RemoteError>;
}
impl HostServices {
    pub fn register_extension(&self, ns: &'static str, handler: Arc<dyn HostExtension>);
}

// files.rs (SI-7)
impl HostFiles {
    /// Lends a handle the engine already holds; no second open (ADR 0064 §2, "Host side").
    pub fn register_open(&self, plugin: &PluginId, file: File, len: u64) -> HostFileRef;
    /// Lends an in-order stream (a decompressing member): reads that do not continue the last one are refused.
    pub fn open_stream(&self, plugin: &PluginId, reader: Box<dyn Read + Send>) -> HostFileRef;
    /// Lends ranges of a held handle for the duration of one call (ADR 0068 §7).
    pub fn lend_ranges(&self, plugin: &PluginId, file: Arc<File>, ranges: Vec<Range<u64>>) -> HostFileRef;
    pub fn release(&self, file: HostFileRef);
}

// grants.rs (2.6a): one memory policy
pub struct ResourceRequest { pub memory_mib: Option<u32>, pub cpu_percent: Option<u8> }   // from manifest `[resources]`
impl Grants { pub fn memory(&self, plugin: &PluginId) -> MemoryPolicy; }                  // §4.5
// One memory path (rule 55): `Plugin::memory_limit()` (paddock-plugin-api/src/plugin.rs:52, sent as
// `Described::memory_limit`) and `Permission::Memory`/`Permission::Cpu` are deleted in 2.6a (ADR 0001, "One memory
// policy"); the manifest's request starts the policy and nothing else sets a cap.

// status.rs (2.6a): the existing `Status` enum is extended, not doubled (rule 55): `Running` splits into Idle and Busy.
pub enum StartPolicy { OnFirstUse, AtStartup }
pub enum Status {
    Stopped,
    Starting,
    Idle { pid: ProcessId, since_tick: u64 },              // was Running
    Busy { pid: ProcessId, in_flight: u16 },               // was Running
    Stopping { pid: ProcessId },                           // new: idle stop or a stop by the user
    Restarting { why: FailureKind, delay: Duration },      // `why` typed (was String)
    Quarantined { why: FailureKind },
    Refused(RefusalText),
    Disabled,
}
pub struct PluginStatus { pub id: PluginId, pub status: Status, pub last_use_tick: Option<u64>,
                          pub sample: Option<ProcessSample>, pub keep_alive: bool }   // ProcessSample from §4.5

// failure (2.6a)
pub enum Failure { Panic, Died, Hang, Garbled, OverMemory { limit: LimitExceeded }, Stall { cause: paddock_proc::StallCause } }
// OverMemory is not counted by paddock-guard's quarantine; simultaneous failures of one process count once.

// sniff.rs (2.6a, new): declarative sniff matcher, pure
pub fn declared_score(decl: &DeclaredSniff, file_name: &str, head: &[u8]) -> Option<u8>;
```

`DeclaredSniff` and `Magic` are defined once in `paddock-plugin-package::manifest` (generic: extensions and magic) and
re-exported by `debrief-plugin-ext` (§3.2), so the platform matches without knowing Debrief.

**Supervisor without containment.** `Supervisor::new` takes `contain: Arc<dyn Contain>`; `Contain::none()` exists only
under `cfg(any(test, feature = "developer"))`. A missing containment is a type error, not a run-time check [J]; the
release checklist item stays (ADR 0001, "OS enforcement").

### 4.3 `paddock-ipc` (IP1, 2.6d)

IP1 is built (`crates/paddock-ipc` on `origin/train/2026-11-11-5`; it reached `origin/main` with the limits adoption).
The block below is the API as built, with the amendments I1 to I9 of 2026-11-07 folded in (they were in
`debrief-1-2-api-limits-amendments.md`, now history); only the frame version 3 is still to build (2.6d).

```rust
pub struct PeerOptions {                // peer.rs; the existing fields plus three (IP1 item 2, I2)
    pub max_concurrent_calls: usize,        // exists: incoming calls handled at once
    pub call_queue: usize,                  // exists: incoming calls waiting; these bound what the OTHER side sends
    pub call_queue_bytes: usize,            // exists
    pub write_queue: usize,                 // frames this side has queued to send (default 256)
    pub write_queue_bytes: usize,           // and their bytes (default 32 MiB); one frame over it enters an empty queue
    pub write_stall: Duration,              // default 30 s on the peer's clock: a sender blocked this long closes the connection
    /* label, cancel_grace, push_queue, clock: unchanged */
}
#[non_exhaustive]
pub enum ProtocolError {                                                             // I3: closes the connection
    RepeatedCallId { id: u64 },                      // only while the first call with that id runs or waits
    ItemWithoutCredit { received: u64, granted: u64 },
    ItemForNonStreamingCall,
}
pub enum IpcError { /* ... */ TooLarge { len, max }, ReplyTooLarge { len, max }, /* ... */ }   // I1: ReplyTooLarge is new
pub trait PeerCredentials { /* peer uid, Unix; fake in tests */ }                    // I5, re-exported
impl Host { pub fn same_account_only(self) -> Host; }                                 // I5: on in launch(), off by default
// frame.rs (2.6d, NOT built): VERSION = 3; Hello and Welcome end with `caps: u32`; bit 0 = progress.
pub const CAP_PROGRESS: u32 = 1 << 0;
```

As built (IP1, amendments I1 to I9):

- **An oversized reply** from a plugin is `IpcError::ReplyTooLarge { len, max }`, not `TooLarge` (which is our own
  request being too large); `Peer::on_skipped` posts it for Reply, Item and Fail frames and the supervisor maps it to
  `FaultKind::Garbled`, the plugin's fault (I1).
- **Write queue and stall (I2).** A sender blocks while the queue is full, for at most `write_stall` or its call's own
  timeout, whichever is smaller. A stall closes the connection ("<label> stopped reading for 30 s"): the queue shuts
  first, every pending call fails with that reason and the sender gets `IpcError::Timeout`, because a dropped reply,
  credit or cancel would leave the two sides disagreeing. A call's own timeout running out fails only that call. The
  reader thread never waits: it sends with `try_push`, and its control frames (refusals, cancels) get a reserve beyond the
  bounds (the calls it runs or queues + 16 frames); only when the reserve is used up does the connection close ("had N
  frames unread when a reply was due"). A cancel that cannot be queued within the grace also closes the connection. It is
  an option, not a constant, so tests drive it with a fake clock.
- **Repeated ids (I3)** are detected only while the first call runs or waits: ids are allocated before the send, so they
  can arrive out of order, and remembering finished ids would be unbounded.
- **Peer credentials (I5).** `Authenticator::authenticate_as` and `Host::same_account_only()` refuse another uid, or an
  unreadable one, before the hello is read. On in `launch()`, **off by default**, because a system service (Dyno's
  helper service) accepts other accounts.
- **Fuzzing (I6):** the `paddock-ipc/fuzz` target `plugin_request` (it depends on `paddock-plugin-api`) and the nightly
  job `shared-fuzz`.
- **Dyno (I8, I9):** Dyno links `paddock-ipc` on `main` by path. `IpcError` gained a variant, so Dyno's exhaustive match
  (`apps/dyno/crates/dyno-plugin-host/src/host.rs`) changed in the same change (rule 39): `ReplyTooLarge` maps like
  `TooLarge` to a protocol failure of the plugin.

**Frame version 3 (2.6d) and the version-2 peers already installed.** `dyno-v1.1.0` ships a helper service
(`apps/dyno/crates/dyno-helper-service`) that speaks `paddock-ipc` frame `VERSION = 2` (`crates/paddock-ipc/src/frame.rs:16`
at that tag). It is installed per machine and outlives an app update, so a Dyno app built after 2.6d meets a version-2
service. So 2.6d:

- keeps ADR 0001's fallback both ways: a version-3 host accepts a version-2 peer without capabilities, and a version-3
  client refused with `version` reconnects once as version 2 (ADR 0031's 2026-10-30 correction to §9);
- **adds a version-2 peer test:** a fixture that speaks the `dyno-v1.1.0` frame bytes (a golden handshake captured from
  that tag's `paddock-ipc`, not written from belief) against the version-3 host and client, and the Dyno app's client
  against a version-2 helper service;
- **updates Dyno in the same change** (rules 39 and 43): every Dyno crate that links `paddock-ipc` builds and runs its
  tests against version 3, and the change runs Dyno's affected tests. A peer without the progress bit is never refused;
  it is judged on CPU time and run state (§4.4).

### 4.4 Patience (2.6c, 2.6d, LV1)

**Reuse what `paddock-proc` already has** (on `origin/main`, ADR 0031's 2026-10-30 amendment "the shared start judge"):
`paddock_proc::{Patience, StartJudge, StartWatch, RunState, run_state, StallCause, StallAction, StallReport}`
(`crates/paddock-proc/src/start.rs`, `runstate.rs`, `stall.rs`). Two consequences for the names of ADR 0031 §5:

- **Starts use the shared start judge as it is.** `plugin.start`, `worker.start` and `window.ready` are starts: a
  `StartWatch` over a `StartProbe` with `paddock_proc::Patience` (spin, idle, hang guard, note, tick, CPU cadence); its
  `StallReport` is what the supervisor and the health chip show. No second start judge.
- **Calls get the call-level policy, renamed so it does not collide.** ADR 0031 §5's trait `Patience` (per-key
  thresholds) would shadow `paddock_proc::Patience` in every crate that uses both, so in `paddock-jobs::patience` it is
  **`CallPatience`**; `RunState` is **`paddock_proc::RunState`** (`Running`, `Stopped`, `Exited`, `Unknown`), not a second
  enum; a stuck verdict carries **`paddock_proc::StallCause`** and is reported as a `StallReport`. The amendment to
  ADR 0031 §5 is one paragraph in 2.6c's change (§17, "Where the ADR notes land").

```rust
// paddock-jobs::patience (2.6c): pure, no I/O, no clock
pub struct CallKey(pub Cow<'static, str>);                 // ADR 0031 §5, unchanged
pub struct Stall { pub blocked_ticks: Option<u32>, pub spin_cpu: Option<Duration> }   // unchanged
/// ADR 0031 §5's `Patience` trait, renamed. `product()` holds LV1's measured values per key; `until_condition()` none.
pub trait CallPatience: Send + Sync {
    fn beat(&self) -> Duration;
    fn stall(&self, key: &CallKey) -> Stall;
}
pub trait TickSource: Send + Sync { fn ticks(&self) -> u64; }                         // unchanged
/// Adapter over `ProcessProbe` (§4.5): the same tick's sample, never a second walk of the process table.
pub trait RunStateProbe: Send + Sync { fn sample(&self, id: &ProcessId) -> Option<RunSample>; }
pub struct RunSample { pub state: paddock_proc::RunState, pub cpu: Duration }
/// What the host owes a call: while any is true, the call gets no blocked ticks (ADR 0031 §2, amended by ADR 0064:
/// waiting for credit is owed by the host).
pub struct Owed { pub host_call: bool, pub file_read: bool, pub credit: bool }
pub struct StallJudge { /* pure state machine */ }
impl StallJudge { pub fn observe(&mut self, key: &CallKey, tick: u64, sample: Option<RunSample>, progress: bool, owed: Owed) -> StallVerdict; }
pub enum StallVerdict { Working, Waiting(WaitWhy), Stuck(paddock_proc::StallReport) }
pub enum WaitWhy { Owed, Paused /* RunState::Stopped */, NotObservable /* RunState::Unknown, inside the hang guard */ }
```

- **Run state.** `RunStateProbe::sample` reads `paddock_proc::run_state(id)`, which is identity-checked (rule 53): a pid
  handed on reads `Exited`, never the new holder's state. That closes §17 Q6 (done; no amendment needed for it).
  `run_state` reads only "stopped" (Windows answers `Unknown` until `IsFrozen` is read, ADR 0031 §4); runnable against
  blocked comes from CPU growth and progress, as the start judge does. ADR 0031 §4's per-thread `Blocked`/`IoWait` read
  is not built; if LV1's measurements show it is needed, it is added to `paddock_proc::RunState`, not as a second enum.
- **`HostTickClock`** as ADR 0031 §5.

Call keys used by the foundation (C measured per key in LV1, recorded under rule 48): `plugin.start`,
`plugin.describe`, `plugin.sniff`, `debrief.import`, `debrief.tail`, `debrief.document`, `debrief.profile`,
`debrief.annotate`, `worker.start`, `worker.call`, `import.busy_wait`, `tail.wait`, `window.ready`. (`ai.connect` is gone
with the built-in AI engine, ADR 0070: Debrief's AI is a `paddock-pitwall` HTTP client, whose waits are external bounds,
ADR 0031 §6.)

**Settings (decided by the user 2026-11-12, Q-U3 (b)).** The per-key thresholds are **constants**: LV1 measures them and
records them under rule 48, in one table `paddock_jobs::patience::product()` reads. One Setting scales them all,
`performance.stall.patience` (§16): `Normal` (×1) or `Patient` (×4), applied by multiplying each key's `blocked_ticks`
and `spin_cpu` when `CallPatience::stall` answers; the hang guard of a start (`paddock_proc::Patience`) takes the same
multiplier. There are no per-key Settings. Tests (2.6c, `paddock-jobs/src/patience/tests.rs`): `Patient` gives four times
the ticks of `Normal` for every key; a value other than the two is refused at the Settings edge.

### 4.5 `paddock-proc` and `paddock-metrics` (2.2, P1, BP1)

```rust
pub enum MemoryPolicy { Cap { bytes: u64 }, Unlimited { machine_floor: u64 } }
impl MemoryPolicy { pub fn ceiling(physical: u64) -> u64; }        // 75 % of physical memory
pub trait ProcessProbe: Send + Sync { fn sample(&self, id: &ProcessId) -> Option<ProcessSample>; }
pub trait SystemProbe: Send + Sync { fn physical(&self) -> u64; fn available(&self) -> u64; }
pub struct ProcessSample { pub footprint: u64, pub cpu: Duration, pub state: Option<RunState> } // footprint per rule 26
// One sampler (rule 55): ADR 0031's `RunStateProbe` is an adapter over `ProcessProbe` (it reads `state` and `cpu` of the
// same tick's sample), not a second walk of the process table.
/// One refresh for all watched pids per tick (P1). Rows by role, versioned camelCase wire type, generated TS.
pub struct ProcessTable { pub version: u32, pub rows: Vec<ProcessRow> }
pub struct ProcessRow { pub role: ProcessRole, pub id: ProcessId, pub footprint: u64, pub cpu_percent: f32 }
pub enum ProcessRole { App, StoreWorker, Plugin { id: Box<str> }, Helper { id: Box<str> } }   // no AiWorker: ADR 0070

pub mod priority {
    /// Defined here, once, and re-exported by paddock-jobs and paddock-plugin-api (`paddock_jobs::exec::Class`,
    /// `paddock_plugin_api::extension::WorkRole`).
    pub enum Class { Interactive, User, Background, Idle }      // ADR 0020's classes, moved here from paddock-jobs::exec
    pub enum WorkRole { Live, Bulk }                            // rule 52's roles (§4.1)
    /// THE table of OS levels per class (ADR 0020's table with its 2026-10-30 amendment): Interactive and User normal;
    /// Background = QOS_CLASS_UTILITY, nice 10 + ioprio best-effort 7, THREAD_PRIORITY_BELOW_NORMAL (the gentle level);
    /// Idle = QOS_CLASS_BACKGROUND, SCHED_IDLE, THREAD_MODE_BACKGROUND_BEGIN (starvable: C54 comment required).
    pub fn apply(class: Class);                      // the executor's workers and child launches use this
    pub fn for_role(role: WorkRole);                 // Live → User, Bulk → Background, through `apply`; plugin serve only
    pub fn threads(kind: WorkKind, class: Class) -> NonZeroUsize;   // ADR 0024 §1
}
```

**New dependency edges (revision 6 correction).** Neither re-exporting crate depends on `paddock-proc` today
(`crates/paddock-jobs/Cargo.toml` and `crates/paddock-plugin-api/Cargo.toml` on the train; revision 5 wrongly said
`paddock-jobs` does). So **2.6c adds `paddock-jobs → paddock-proc`** (with the `patience` module, §4.4, which also uses
`paddock_proc::{RunState, StallCause, StallReport, ProcessId}`) and **2.6b adds `paddock-plugin-api → paddock-proc`**
(for `WorkRole` and `for_role`; it reaches `paddock-proc` today only through `paddock-ipc`). No cycle is possible:
`paddock-proc`'s `[dependencies]` hold no `paddock-*` crate but the workspace hack (`paddock-testkit` is a
dev-dependency only), and it must stay that way; each edge is one line in the owning package's `Cargo.toml`.

### 4.6 `paddock-limits` (1.4 with B1; answers plan D5)

ADR 0019's half (1.4) and ADR 0065's half (B1) in one crate [A: ADR 0065 D1]. A leaf crate: serde, `ts-rs` behind
`ts`, no `paddock-ipc`. **Built** (`crates/paddock-limits` on `origin/main`, from `debrief/1.2-limits` and its adoption
branch `debrief/1.2-limits-adopt` `0d315d649`). The block is the API as built; the amendments L1 to L12 of 2026-11-07
(formerly `debrief-1-2-api-limits-amendments.md`, now history) are folded in and marked with their number.

```rust
pub struct LimitId(&'static str);              // "import.record_bytes"; L5: `parse` validates; `register(&Registry)`
pub struct SettingPath(&'static str);          // L1: dotted parts [a-z][a-zA-Z0-9_]*, ≤ 64 bytes, `const fn new`;
                                               //     the grammar of `paddock_settings::valid_id` (a test keeps them equal)
pub enum Unit { Bytes, Count, Ratio, Millis, Percent }                 // L1: one per Quantity variant
pub enum Quantity { Bytes(u64), Count(u64), Ratio(u32), Millis(u64), Percent(u8) }
pub struct LimitExceeded { pub id: LimitId, pub limit: Quantity, pub reached: Option<Quantity>, pub scope: Scope }  // L9
pub enum Scope { File, Source, Import, Plugin, Query, Process }
pub enum Remedy { RaiseSetting { path: SettingPath }, ShowMore, Narrow, Retry, None }
pub struct LimitRow {
    pub id: LimitId, pub unit: Unit, pub default: Quantity, pub min: Quantity, pub max: Quantity,
    pub value: RowValue,                                   // Setting(SettingPath) | Hard | ByRequest
    pub kind: RowKind,                                     // Stop | Cut | Both  (ADR 0065 §2)
    pub selections: &'static [SelectionKind],              // for Cut rows
    pub remedy: Remedy,
}
impl LimitRow { pub const fn stop(/* id, default, min, max, value, remedy */) -> LimitRow; }   // the usual stop row
pub struct Registry(&'static [LimitRow]);      // `Registry::check`: default, min and max in the row's unit
pub trait SettingLabels { fn label(&self, path: SettingPath) -> Option<(&str, &str)>; }   // L8: (section, field)
impl LimitExceeded {
    pub fn message(&self, r: &Registry) -> String;                             // L8: the dotted path
    pub fn message_with(&self, r: &Registry, labels: &dyn SettingLabels) -> String; // L8: the schema's words, the value
    pub fn setting_path(&self, r: &Registry) -> Option<SettingPath>;           // L8: the structured log field
}

// Bounded, Cut, Total, Selection, SortKey, Next: ADR 0065 §1 [A], as built:
pub struct FieldPath(Cow<'static, str>);       // L2: ≤ 64 bytes, [a-z][a-zA-Z0-9]* parts, one optional [u32] (no leading
                                               //     zero), never starting with `result`; `const fn lit`, `root()`
pub struct CutRow<S>(/* &'static LimitRow */); // L4: `CutRow::<S>::of(&ROW)` is a const fn; in a `const` it fails the build
                                               //     for a stop row, a row without `S`, or a row that does not count items
pub struct RowLimit(u64);                      // L3: only from `CutRow::limit_of(value)`, which checks the row's min and max
pub enum Total { Exact { n: u64 }, AtLeast { n: u64 }, Unknown }
impl Cut {                                     // L10, L11: one constructor per selection; none panics on a number
    pub fn counted_first(row: &CutRow<First>, of: FieldPath, shown: u64, total: u64, from: u64) -> Option<Cut>;
    pub fn page(row: &CutRow<First>, of: FieldPath, offset: u64, shown: u64, total: u64) -> Option<Cut>;   // Next::Page
    pub fn counted_top(row: &CutRow<TopBy>, of: FieldPath, shown: u64, total: u64, key: SortKey, from: u64) -> Option<Cut>;
    pub fn counted_last(row: &CutRow<Last>, of: FieldPath, shown: u64, total: u64) -> Option<Cut>;
    pub fn counted_sample(row: &CutRow<Sample>, of: FieldPath, shown: u64, total: u64) -> Option<Cut>;
    pub fn at_least(row: &CutRow<First>, of: FieldPath, shown: u64, n: u64) -> Option<Cut>;
    pub fn maybe(row: &CutRow<Maybe>, of: FieldPath, shown: u64, selection: Selection) -> Cut;
    pub fn describe(&self) -> String;          // "rows: showing 501 to 503 of 1,204 (test.first)"; sums saturate
    pub const fn words_key(&self) -> &'static str;   // the UI's word key ("first.unknown": a page whose total is unknown)
}
impl<T> Bounded<T> {
    pub fn first<I: IntoIterator<Item = T>>(iter: I, row: &CutRow<First>, of: FieldPath) -> Self;   // L3: the row default
    pub fn first_with<I: IntoIterator<Item = T>>(iter: I, row: &CutRow<First>, limit: RowLimit, of: FieldPath) -> Self;
    pub fn and(self, cut: Option<Cut>) -> Self;   // L6, L7; `None` (nothing left out) adds nothing
}

// Request-side types that plan 2.5 keeps (plan D5: here, not in paddock-view, which pulls in paddock-ipc and postcard):
pub struct Limit(NonZeroU32);
pub struct Page { pub offset: u64, pub limit: Limit }
pub struct Buckets(NonZeroU16);
```

What the code settled that the design had left open (L1 to L12):

- **Ids from the wire (L5).** Ids of a registered registry (`LimitId::register`, plus the built-in `bounded.cuts`) resolve
  to their static text and are never refused; any other key is interned, leaked once per distinct key, **at most 4,096
  unknown keys**, then `BadKey::Full` for those only, and the table logs one ERROR when it fills. The app composition
  calls `LimitId::register` (the shell builder, 1.4b).
- **No panic on a number (L10, L11).** Request and I/O data reach `page`. Numbers that disagree (a stale offset after a
  filter, rows that arrived between the count and the read) give a cut with `Total::Unknown`; a window that does not
  start at 0 is always a cut; `AtLeast` only for the first items from 0. `Cut::maybe` takes a `CutRow<Maybe>` (selection
  kind `Maybe` in the row): an unknown total with a real selection.
- **More than 32 cuts (L6).** The 31 smallest in the **structural order of `FieldPath`** (a path before the paths inside
  it, siblings by name, entries of one list by numeric index), kept sorted, so `.and` does not depend on the order cuts
  are added in; the aggregate is the last of exactly 32 cuts, `of` empty, `First{0}`, `Exact` with more than 31, shown
  31, `Next::None`, and its words say "up to N" because a list counted earlier and added again is counted again.
- **A second cut of the same list (L7)** with the same `(of, limit)` replaces the earlier one in place;
  `TryFrom<BoundedWire>` refuses duplicates.
- **Words (L8).** `message_with` uses the Settings schema's words through `SettingLabels`, implemented by the app over its
  schema, so the crate stays a leaf; measured values (`reached`) are rounded (`1.2 GiB`), limits stay exact.
- **Wire.** `Bounded` serializes by borrowing (no `Clone` bound); JSON and other self-describing formats only (postcard
  fails, so a `Bounded` never crosses the plugin wire, §5); `Quantity` reads its tag through a derived `Kind` and its
  number at the kind's width, round-trip tested for JSON, postcard, MessagePack and bincode; a repeated key is
  `duplicate_field`; `LimitExceeded`'s units must agree; a `Percent` over 100 is refused.
- **Tests.** `limit_case!` and `limit_cases!` (one `#[test]` per case plus a both-ways check against the registry, one
  module per registry) are in `paddock-testkit/src/limits.rs`. Each crate owns its registry rows; Debrief's rows go in
  `debrief-core/src/limits.rs` (3.6 and B3, one file).
- **TypeScript (L12):** §13.2.

**Package 1.4b (built on `debrief/1.2-limits-adopt`, coordinator decisions of 2026-11-07).** `LimitExceeded` is kept in
`ArchiveError::Limit`, `JobError::Limit`, `JobState::Failed` and `WireState::Failed` (with the UI's `SuJobState.limit` and
its parser), `IpcError::limit()` and `HostError::Limit`, and `Cause::from_limit` in `paddock-health`; shared rows with
`limit_cases!` for ipc (51), plugin host (52, 53), guard (54), archive (55), net (56), download (57), updater (58), view
(59), diag (60) and storage (61); conformance check C62 in warning mode (a row without a case, and limit-shaped
constants no registry file mentions). The job wire change updated every consumer in the same change (rules 39 and 43).
Left for later, deliberately: Debrief's, Pitwall's and Spotter's own `JobState` models adopt the limit with their own
registries (Debrief 3.6, B3); row 62 (the UI's IPC call timeout) is TypeScript and goes with 2.3's `LimitNotice`;
`Cause::from_limit`'s first caller is the plugin host's memory kill (2.6a); C62 becomes an error when Debrief's list is
reasoned (ADR 0019's phases).

### 4.7 `paddock-sandbox` (PH1)

The move of `debrief-ipc/src/{sandbox,contain}.rs` as family ADR 0001 lists it [A]: `SandboxSpec`, `FsAccess`,
`Containment::{SelfApplied, Launcher}`, `lockdown()`, `Gap`; `impl paddock_ipc::launch::Contain for Launcher`. Tests per
OS on each runner: a contained plugin reads inside its grant and cannot outside it.

### 4.8 `paddock-archive` member reader (SI-4)

ADR 0064 §3 shows `Members`, `open_member`, `gzip_member` [A]. Added here: the spool helper and its sweep.

```rust
pub struct Spool { dir: PathBuf, owner: OwnerRecord /* ProcessId of this process */ }
impl Spool {
    pub fn create(parent: &Path, me: &ProcessId) -> io::Result<Spool>;
    /// Copies `member` into a private `create_new` file under the cap and the free-disk floor; a phase with progress.
    pub fn spool(&self, member: impl Read, declared: u64, cap: &SpoolCap, ctl: &dyn Control) -> Result<SpooledFile, SpoolError>;
}
pub struct SpooledFile { file: File, path: PathBuf }     // removed on drop, on every path
/// Removes spool folders whose owner process is gone (pid and start time, rule 53). Never by age alone.
pub fn sweep_spools(parent: &Path, probe: &dyn ProcessExists) -> SweepReport;
pub enum SpoolError { Cap(LimitExceeded), Disk { free: u64, need: u64 }, Io(io::Error), Cancelled }
```

### 4.9 What `debrief-plugin-host` keeps (PH2b) and the feature parity list

The plan's risk row asks 0.5 to list every `debrief-plugin-host` feature against the shared host [S: its nine files].

| `debrief-plugin-host` today | Goes to | Package |
|---|---|---|
| `process.rs`: supervised child, start on first use, restart with backoff, quarantine, settings restored after restart | `paddock-plugin-host::Supervisor` (exists), lifecycle (2.6a) | PH2b deletes |
| `process.rs`: `call_lending` with the 5 ms pump | `Remote::stream` + `HostFiles` on the handler thread (no pump) | 2.6b, SI-7 |
| `sandbox.rs` | `paddock-sandbox` | PH1 |
| `installed.rs`: `.debriefplugin` install, update, enable, grant | `paddock-plugin-host::installed` + Debrief's PH3 removal | PH2b, PH3 |
| `output.rs`: saving an elevated process's output streams | `HostCall::Output` + `OutputSink`; captures still land in `<data>/captures/<folder>` (checked in PH2b) | PH2b |
| `builtins.rs`: one bundle binary, one process per built-in, manifest cache, `warm_up` | stays (thin): bundle launch spec; `warm_up` replaced by `StartPolicy` (2.6a, 4.4) | PH2b, 4.4 |
| `remote.rs`: `RemotePlugin` proxies of importers, config parsers, marker kinds, annotators, transforms, tools | stays (thin adapter): one proxy per `debrief.*` namespace over `Remote<C>` | PH2b |
| `profile.rs`, `document.rs`: capture and document importers by ranges | stays (thin adapter) over `debrief.profile.v1`, `debrief.document.v1` | PH2b, SI-11 |
| `lib.rs`: plugin processes look like ordinary `debrief_core::Plugin`s | stays | PH2b |
| Elevated process (second process of the plugin binary, `elevated` role) | stays Debrief's (`ElevatedRunner` port) [A: ADR 0001 notes] | PH2b |
| `View*` host requests under a ticket | `debrief.view.v1` as a `HostExtension` (§5.8) | PH2b |
| Memory watcher on the store worker's plugins | `ProcessProbe` + `MemoryPolicy` | 2.2, PH2b |

No feature of the old host is left without a home.

## 5. The `debrief.*` extension namespace

ADR 0003 asks package 0.5 to "fix the exact request and reply enums of each namespace" [A]. These are the first
versions: nothing has shipped on protocol 2, so each namespace starts at minor 0, with every field it will need (§1.4).
All bodies are postcard; no `untagged`, `flatten`, internal tags or `skip_serializing_if`. Every contract lives in
`debrief-plugin-ext/src/wire/<ns>.rs` with a round-trip test of every variant in the same file and a fuzz target of its
request decoder in `debrief-plugin-ext/fuzz/` (PH2a, SI-8).

### 5.1 The mapping, protocol 1 to protocol 2

| Protocol 1 (`PluginRequest`) [S] | Protocol 2 | Kind | Role | Reply |
|---|---|---|---|---|
| `Manifest` | `PlatformRequest::Describe` + `debrief.describe.v1` `Contributions` | unary | Live | `Described` (JSON) + `DebriefContributions` |
| `ApplySettings { values }` | `PlatformRequest::ApplySettings { json }` | unary | Live | `()` |
| `InvokeTool { tool, op, args, data_dir }` | `PlatformRequest::Invoke { tool, op, args }` | unary | Live | `Result<Json, Fail>`; `data_dir` dropped |
| `StopTask { tool, task, data_dir }` | `PlatformRequest::Stop { task }` | unary | Live | `Result<(), Fail>` |
| `PullOutput { stream }` | removed; the plugin pushes `HostCall::Output` | | | |
| `Sniff { importer, file, head }` | `debrief.sniff.v1` `Score`, target kind `Importer` (batched per plugin per file, §5.10) | unary | Live | `Scores` |
| `Import { importer, file, content }` | `debrief.import.v1` `Import` | stream of `EventItem` | Bulk | `ImportDone` |
| `ImportTail { importer, file, head, tail }` | `debrief.import.v1` `Tail` | stream of `EventItem` | Live | `TailDone` |
| `SniffConfig { parser, file_name, head }` | `debrief.sniff.v1` `Score`, target kind `ConfigParser` | unary | Live | `Scores` |
| `ParseConfig { parser, content, limits }` | `debrief.config.v1` `Parse` | unary | Bulk | `ParseConfigReply` |
| `Annotate { annotator, ticket, since }` | `debrief.annotate.v1` `Annotate` | unary | Bulk | `AnnotateReply` |
| `Transform { transform, source_name, rows }` | `debrief.transform.v1` `Transform` | unary | Bulk | `TransformOut` |
| `SniffProfile { importer, file_name, head }` | `debrief.sniff.v1` `Score`, target kind `Profile` | unary | Live | `Scores` |
| `ImportProfile { importer, file_name, content, limits }` | `debrief.profile.v1` `Import` | stream of `Piece` | Bulk | `ProfileDone` |
| `SniffDocument` | `debrief.sniff.v1` `Score`, target kind `Document` | unary | Live | `Scores` |
| `ImportDocument`, `ResymboliseDocument` | `debrief.document.v1` `Import`, `Resymbolise` | streams | Bulk | `DocumentDone` |

| Protocol 1 (`HostRequest`) [S] | Protocol 2 |
|---|---|
| `ReadFile { id, offset, len }` | `HostCall::ReadFile` (same limits, `READ_CHUNK` 1 MiB) |
| `TaskStarted`, `TaskProgress`, `TaskFinished` | `HostCall::TaskStarted`, `Report`, `TaskFinished` |
| `InvokeElevated { tool, op, args, timeout_ms }` | `HostCall::Elevated` |
| `SaveOutput` | `HostCall::Output { name, bytes, last }` |
| `Log` | `HostCall::Log` |
| `ViewSelect`, `ViewInstances`, `ViewTimeRange` | `HostCall::Extension` `debrief.view.v1` `Select`, `Instances`, `TimeRange` |

Protocol-1 markers had no request of their own: marker kinds are declared in `Contributions` (§5.2) and set per event in
`WireEvent.markers` [S].

### 5.2 `debrief.describe.v1`

```rust
pub struct Describe;
impl Contract for Describe { const NS: &'static str = "debrief.describe.v1"; const MINOR: u16 = 0;
    type Request = DescribeRequest; type Reply = Json; }   // Json = paddock-plugin-api's JSON wrapper of DebriefContributions
pub enum DescribeRequest { Contributions }
/// Crosses as JSON (camelCase), like the platform's `Described`, so an `Option` or `#[serde(default)]` field added later
/// is a minor change (§1.4). Every *Cap below is a JSON struct for the same reason.
pub struct DebriefContributions {
    pub importers: Vec<ImporterCap>,            // §3.3
    pub documents: Vec<DocumentCap>,            // { id, access, whole, sniff, kind: DocumentKind }
    pub profiles: Vec<ProfileCap>,              // { id, access, whole, sniff }
    pub config_parsers: Vec<ConfigParserCap>,   // { id, extensions }
    pub annotators: Vec<AnnotatorCap>,          // { id, reads: AnnotatorReads, writes_markers: Vec<Box<str>> }
    pub transforms: Vec<TransformCap>,          // { id, applies_to: Vec<Box<str>> }
    pub marker_kinds: Vec<MarkerKind>,          // { id, label_key, glyph }
}
```

The host compares it with the verified manifest (`check_caps`, §3.3); a difference refuses the plugin with the
platform's `Refusal`, as `compat::check_described` does for the platform half.

### 5.3 Shared bodies

```rust
pub struct FileInfo {
    pub name: Box<str>,                 // file name only
    pub rel_tail: Box<str>,             // path below the import root, at most 3 segments; never absolute (ADR 0064 §2)
    pub len: u64,                       // LEN_UNKNOWN (u64::MAX) for a decompressing stream
    pub mtime_ms: Option<i64>,
    pub date: Option<Box<str>>,         // file date for time-only formats (ADR 0066 §9)
    pub times: Json,                    // TimeSettings as JSON (assume_timezone etc., `inherit` by default, ADR 0066 §2):
                                        // the settings will grow, so they are not a postcard struct (§1.4)
}
// Protocol 1's `FileInfo.path` (the absolute path) is gone: a plugin sees `rel_tail` only, and a first-party `ByPath`
// importer gets the one chosen path in `WireInput::ChosenPath`. `Permission::OpenedFilePaths` stays in the manifest
// model for tools that are handed paths by the user (ADR 0001); the Windows plugin stops asking for it (ADR 0064 §2).
pub const LEN_UNKNOWN: u64 = u64::MAX;
pub struct TextPlan { pub encoding: Box<str>, pub data_start: u64 }
pub enum WireInput {
    Lent { file: HostFileRef, seekable: bool },
    ChosenPath { path: String, len: u64 },          // first-party ByPath only
}
/// One item: at most MAX_EVENTS_PER_ITEM (8,192) events or about ITEM_BYTES (4 MiB), whichever first.
pub struct EventItem { pub events: Vec<WireEvent> }
pub struct WireEvent {
    pub ts: Option<i64>,
    /// ADR 0066 §6's `ts_state` byte, exactly as the store keeps it: low two bits `TsState` (Parsed 0, Inherited 1,
    /// Untimed 2), then flags WALL 4, GAP 8, FOLD_UNSURE 16, DATE_AMBIG 32, CONTRADICTS 64. **A plugin may set the low two
    /// bits, WALL and DATE_AMBIG**; the host masks the rest and sets GAP, FOLD_UNSURE and CONTRADICTS itself when it
    /// converts wall times. Read through `debrief_batch::TsCode::from_wire(u8)`, the one definition (rule 55).
    pub ts_state: u8,
    pub level: u8,
    pub message: String,
    pub template: Option<String>,
    pub exception: Option<String>,
    pub fields: Vec<(String, WireValue)>,
    pub line: u32,
    pub ordinal: Option<u64>,                       // §2.2; None = the line
    pub markers: u64,
    pub raw: Option<String>,                        // ADR 0064 "Raw text": crosses as text; None for EVTX
    pub byte_offset: Option<u64>,
}
pub const MAX_EVENTS_PER_ITEM: usize = 8192;
pub const ITEM_BYTES: usize = 4 << 20;
pub const EVENT_FRAME_MAX: usize = 12 << 20;        // over it the SDK drops `raw`, then the event, with EventTooLarge
```

`WireEvent` loses `id` (§2.1) and gains `ts_state`, `ordinal`, `raw` and `byte_offset`, in the one wire change ADR 0066
D13 asks for ("`Record.ts` plus basis flags cross in the same wire change"). `TsState` and the flag bits stay defined
once, in `debrief-batch` (`ids.rs`); so that plugins can use them without Arrow, `debrief-batch`'s Arrow dependencies
move behind a default feature `arrow`, and `debrief-plugin-ext` depends on `debrief-batch` with `default-features =
false` (§17, Q25). Until Z1 lands, plugins send flags 0.

### 5.4 `debrief.import.v1`

```rust
pub struct Import;
impl Contract for Import { const NS: &'static str = "debrief.import.v1"; const MINOR: u16 = 0;
    type Request = ImportRequest; type Reply = ImportReply;
    fn role(r: &ImportRequest) -> WorkRole { match r { ImportRequest::Import { .. } => WorkRole::Bulk, ImportRequest::Tail { .. } => WorkRole::Live } } }
impl StreamContract for Import { type Item = EventItem; }

pub enum ImportRequest {                 // revision 6: no Sniff variant; sniffing is `debrief.sniff.v1` (§5.10)
    /// Items: EventItem. Reply: ImportReply::Done. Bulk.
    Import { importer: Box<str>, file: FileInfo, input: WireInput, plan: TextPlan },
    /// Items: EventItem. Reply: ImportReply::TailDone. Live. `head` ≤ HEAD_BYTES, inline.
    Tail { importer: Box<str>, file: FileInfo, head: Vec<u8>, input: WireInput, plan: TextPlan },
}
pub enum ImportReply { Done(ImportDone), TailDone(TailDone) }
pub struct ImportDone {
    pub skipped: u64,
    pub problems: Problems,          // counted, at most 20 samples per kind (existing bound)
    pub end: WireImportEnd,          // Complete | Stopped { why: WireStopReason, at_byte: Option<u64> }
    pub bytes_read: u64,
    pub time: WireTimeReport,        // ADR 0066 §11's importer part; NoTimes for a format without time
}
pub struct TailDone { pub supported: bool, pub skipped: u64, pub problems: Problems, pub time: WireTimeReport }
pub enum WireStopReason { Damaged, CutOff, RecordOverBudget { limit_id: Box<str>, limit: u64 }, UnsupportedVersion }
```

`Arc<str>` in ADR 0064's sketch is `Box<str>` here: postcard encodes both alike, and a body is owned once [J].

- **A typed reply, not a failure,** for everything the host must word: a budget stop is `Done { end: Stopped
  { RecordOverBudget } }`; "not this format" is `RemoteError { code: codes::INVALID }` with the importer's text only in
  the log.
- **Sniffing** is not in this namespace (§5.10): every stream of `debrief.import.v1` is an import or a tail.

### 5.5 `debrief.config.v1`

```rust
pub enum ConfigRequest {                 // sniffing: `debrief.sniff.v1` (§5.10)
    Parse { parser: Box<str>, content: HostFileRef, limits: ConfigLimits },  // Bulk; → Parsed(Result<ConfigTree, ConfigError>)
}
pub enum ConfigReply { Parsed(Result<ConfigTree, ConfigError>) }
pub struct ConfigLimits { pub max_bytes: u64, pub max_depth: u16, pub max_nodes: u32 }    // 64 MiB today [S]
```

Configuration parsers stay whole-reading under their 64 MiB cap (ADR 0064 D1, "leave them out") [A].

### 5.6 `debrief.annotate.v1` and `debrief.transform.v1`

```rust
pub enum AnnotateRequest { Annotate { annotator: Box<str>, ticket: u64, since: Option<ListedSince> } }   // Bulk
pub struct AnnotateReply { pub annotations: Vec<Annotation>, pub dropped: u64, pub listed_through: u64 }   // MAX_ANNOTATIONS
/// "New since the previous run": events in segments listed after this listing sequence (ADR 0062's listing transaction).
/// Always `None` in 1.2 (full runs, Q-U1 (c)); kept so that incremental runs are a minor change later.
pub struct ListedSince { pub listing: u64 }

pub enum TransformRequest { Transform { transform: Box<str>, source_name: Box<str>, rows: Vec<WireEvent> } } // Bulk
pub struct TransformOut { pub rows: Vec<WireEvent>, pub keep: Vec<bool>, pub marks: Vec<(u32, Box<str>)>, pub notes: Vec<Box<str>> }
```

Protocol 1's `RecordCursor { id: u32, ts }` (`debrief-plugin-api/src/annotate.rs:21`) [S] is replaced.

**Full runs in 1.2 (decided by the user 2026-11-12, Q-U1 (c)).** Every annotation run reads all its events. The wire
keeps the fields so that incremental runs can come later as a minor change (§1.4): the host always sends `since: None`
and answers `listed_through: 0`; `ViewRequest::Select` always gets `listed_after: None` (§5.8), and the host refuses a
`Some` from a plugin with `RemoteError { code: codes::INVALID }`. No listing sequence is built in the store, so IX-B1
carries none of it (Q28). The round-trip test of `debrief.annotate.v1` covers both fields; a host test asserts the two
constant values.

**When incremental runs come (a later release, not 1.2).** The design below stays the target. Neither an event id nor a
time-order `ViewCursor` says "new since": ids are allocated in blocks per segment, so an id does not tell when its event
became visible, and events imported later can sort earlier in time. The store already lists segments in one transaction
each when they are ready (ADR 0062) [A]; giving each listing a sequence number [J] makes "new since" exact: the host
answers `listed_through` with each run, the next run sends it back as `since`, and `ViewRequest::Select` takes
`listed_after` (§5.8) to read only those segments. The number is the store's and lives as long as the store; a re-import
starts a new store, and an annotation run never spans one.

**Under rewrites and merges** (for that later release; one test per case in `debrief-store-duck/tests/listing_seq.rs`):

| Case | Sequence of the result | Effect on the next annotation run |
|---|---|---|
| A new segment (import, live-tail append, re-read after undo) | a new sequence, always | its rows are read once |
| Compaction or `maintain` merging segments | the **lowest** sequence of its inputs | nothing already seen is lost: rows the run already read may be read again only if an input was newer than `since`, never missed |
| Re-time rewrite (ADR 0066 §8, clock skew) | the lowest sequence of its inputs | the same rows are not presented as new; their times moved, which the annotator learns only on a full run |
| A source removed | its segments go | nothing to read |

Rule: a rewrite never gives old rows a new sequence, and new rows never share an old one, so "new since" never misses
a row and repeats one only after a merge with newer input. An annotator's results are keyed by event id, so a repeat
replaces rather than doubles them.

### 5.7 `debrief.profile.v1` and `debrief.document.v1`

```rust
pub enum ProfileRequest {                // sniffing: `debrief.sniff.v1` (§5.10)
    Import { importer: Box<str>, file: FileInfo, input: WireInput, limits: Json },  // Bulk, items: Piece → Done(ProfileDone)
}
pub enum DocumentRequest {               // sniffing: `debrief.sniff.v1` (§5.10)
    Import { importer: Box<str>, file: FileInfo, input: WireInput, options: Json },
    Resymbolise { importer: Box<str>, document: HostFileRef, options: Json },
}
pub struct Piece { pub bytes: Vec<u8> }            // ≤ OUTPUT_CHUNK (256 KiB), the library's own encoding
pub struct ProfileDone { pub bytes: u64, pub heap: bool }
pub struct DocumentDone { pub bytes: u64 }
```

The option bodies stay JSON, as in protocol 1 [S: `debrief-plugin-host/src/{profile,document}.rs`], because their
limits grow with every format (§1.4). Their types are defined once, in the domain crates the host and the plugin both
link, and validated at the edge on both sides:

| JSON body | Type | Fields today [S] |
|---|---|---|
| `limits` of `debrief.profile.v1` `Import` | `debrief_profile::Limits` (`limits.rs:7`) | `max_input_bytes`, `max_frames`, `max_stack_nodes`, `max_samples`, `max_threads`, `max_events`, `max_stack_depth`, `max_name_len`, `max_objects`, `max_edges`, `max_nesting` |
| `options` of `debrief.document.v1` | `debrief_dump::ImportOptions` (`import.rs:59`) | `limits: debrief_dump::Limits` (`max_threads`, `max_frames`, `max_stack_bytes`, `max_modules`, `max_handle_types`, `max_stacks`, `max_umdh_frames`, `max_text`, `max_notes`, `max_read_bytes`, `max_encoded_bytes`, and the rest of `limits.rs`), `file_time_ms`, `symbol_folders` |

A new field is `#[serde(default)]`, so an older plugin reads a newer host's body.

### 5.8 `debrief.view.v1` (host side)

```rust
pub enum ViewRequest {
    /// One page; `after` continues from the previous page's `next`. → ViewReply::Rows.
    Select { ticket: u64, filter: Json /* ViewFilter */, limit: u32, after: Option<ViewCursor>, listed_after: Option<u64> },
    Instances { ticket: u64 },                                  // → Instances(Vec<InstanceRow>)
    TimeRange { ticket: u64 },                                  // → Range(Option<(i64, i64)>)
}
pub enum ViewReply {
    /// At most one item's worth (MAX_EVENTS_PER_ITEM or ITEM_BYTES); `next` is `None` at the end or at `limit`.
    Rows { rows: Vec<ViewRow>, next: Option<ViewCursor> },
    Instances(Vec<InstanceRow>),
    Range(Option<(i64, i64)>),
}
/// Protocol 1's `WireViewRecord { event, source, instance }` (`protocol.rs:580`) [S], kept.
pub struct ViewRow { pub event: WireEvent, pub source: Box<str>, pub instance: Box<str> }
/// The page order's key, `(ts, source, ordinal, id)` (§2.2), reversed under `newest_first`.
pub struct ViewCursor { pub ts: Option<i64>, pub source: u32, pub ordinal: u64, pub id: EventId }
```

Served by Debrief as a `HostExtension` (§4.2) during an annotation, under its ticket; a ticket not open is `denied`. A
run reads at most `limit`, capped at `MAX_VIEW_RECORDS` (100,000, `annotate.rs:17`) [S] across all its pages; a unary
reply could not carry that in one item, so the continuation is the design, not an option.

`filter` stays JSON, as protocol 1's `filter: Json` [S], because `ViewFilter` (`annotate.rs:31`) holds `CondValue`, which
is `#[serde(untagged)]` (`condition.rs:22`) and so cannot be postcard (ADR 0001 bans `untagged` on the wire); JSON is
also where a later optional filter field is minor. Its fields [S]: `when: Vec<ConditionCfg { field, op, value:
Option<CondValue> }>` with `CondValue = Text | Number | Bool | List`, `from: Option<i64>`, `to: Option<i64>`,
`newest_first: bool`, all `#[serde(default)]`. The host parses it once at the edge into its typed form and refuses an
unknown `op`.

### 5.9 Room left for features

| Namespace (reserved, not designed) | For | Rule |
|---|---|---|
| `debrief.dumpquery.v1` | ADR 0068 §7's `DumpQuery` range reads: **designed in §14.13.6 (revision 7)** | Its own namespace so `debrief.document.v1` does not change; lends ranges through `HostFiles::lend_ranges` (§4.2) |
| `debrief.managed.v1` | ADR 0068 §9's .NET helper call: **designed in §14.13.6 (revision 7)** | Its own namespace in the .NET plugin; the reply is JSON (schema 1 grows, §1.4) |
| `debrief.analysis.v1` | plugin analysis contributions (roadmap layer C, "later") | Its reply is §11's `AnyFinding` **as a JSON body** (it holds JSON values and `DocItem`, which are not postcard); registered as a `kind = "extension"` contribution |
| a variant appended to any request enum above | a new operation of an existing namespace | Minor; refused before sending to an older peer (`introduced_in`, §4.1) |

### 5.10 `debrief.sniff.v1` (revision 6)

Revision 5 put a `Sniff` variant into each of `debrief.import.v1`, `debrief.config.v1`, `debrief.profile.v1` and
`debrief.document.v1`. Revision 6 moves all four into one namespace of their own:

- a sniff is a unary, Live call with no items; `debrief.import.v1` is a `StreamContract` whose `Item` is `EventItem`, so a
  sniff variant there was a stream contract call that never streams (IP1's `ItemForNonStreamingCall` guards the
  wire, not the contract), and its `role` had to special-case it;
- the four sniffs were four shapes of one question (a score for a head), with different head types (`String` for
  configuration parsers, bytes elsewhere) and only the import sniff batched;
- the folder scan (§6.4) asks every kind for the same file in the same phase: one namespace makes that one call per
  plugin per file, and one memo (SI-6).

```rust
pub struct Sniff;
impl Contract for Sniff { const NS: &'static str = "debrief.sniff.v1"; const MINOR: u16 = 0;
    type Request = SniffRequest; type Reply = SniffReply; }        // unary; role Live (the default)

pub enum SniffKind { Importer, ConfigParser, Profile, Document }
/// One contribution of this plugin to score, by its id in `DebriefContributions` (§5.2) and its kind.
pub struct SniffTarget { pub kind: SniffKind, pub id: Box<str> }
pub enum SniffRequest {
    /// One call per plugin per file: a score per target, in order. `head` ≤ 64 KiB of raw bytes (a configuration parser
    /// decodes it itself, with `encoding` when the host knows it).
    Score { targets: Vec<SniffTarget>, file: FileInfo, head: Vec<u8>, encoding: Option<Box<str>> },
}
pub enum SniffReply { Scores(Vec<u8>) }     // one per target, same order; a length mismatch is `Failure::Garbled`
```

- **Batching and memo (SI-6):** the host memoises the score per file per target and sends one `Score` per plugin per file
  with every target of that plugin whose declared sniff is `Dynamic` and whose parser rules passed (`declared_score`,
  §4.2, answers the rest without a call).
- **Liveness:** call key `plugin.sniff` (§4.4); a plugin crash during a sniff in a folder scan does not count toward
  quarantine (§6.4).
- **Tests:** the round trip of every variant and the fuzz target of the request decoder, as every namespace (§5's
  preamble), plus a host test that a reply of the wrong length is `Garbled`.

## 6. Folder open (ADR 0067)

ADR 0067 [A] decides the behaviour. This section gives the signatures, entry points, the UI backend and the tests.
Packages F1 to F12, FM, FR.

### 6.1 Entry points

| Entry | Today [S] | 1.2 |
|---|---|---|
| Open folder, recent folder, dropped folder, launch path with a folder | `pickAndOpen(true)` → `pick_paths` → `open_paths` | `pick_paths` → **`open_plan`** → `folder_scan_start` for `choose`, `open_paths` for `direct` |
| Single file, single-member zip, `.gz` | `open_paths` | unchanged (`open_plan` returns it in `direct`) |
| `open_paths_live` on a folder | direct | dialog first, live choice carried to `folder_import` |
| Restore, `reload`, `refresh_tail`, watched folder, CLI, tests through `open_paths` | direct | unchanged; the stored `FolderSelection` prunes the walk (§6.6) |

### 6.2 The judge (F1)

```rust
// debrief-core/src/ingest/plan.rs
pub struct JudgeEnv<'a> { pub parsers: &'a ParserSet, pub importers: &'a ImporterRegistry, pub sniff: &'a SniffMemo,
                          pub zone: Option<&'a LogZone>, pub gate_allowance: GateAllowance }
pub struct FileMeta { pub rel: RelPath, pub size: u64, pub mtime_ms: Option<i64>, pub origin: SourceOrigin }
pub fn judge<'a>(env: &JudgeEnv<'a>, file: &FileMeta, head: &Head, whole: bool) -> Result<Judged<'a>, FileRefusal>;
pub struct Judged<'a> { pub choice: Choice<'a>, pub plan: FilePlan, pub limits: Vec<JudgedLimit> }
pub enum JudgedLimit { NoTime, ZoneUnknown, DateFromFileTime, DayOrderUndecided,
                       OverMemory { side: MemorySide, need: u64, can_give: u64, measured: bool }, OverLimit(LimitExceeded) }
pub enum MemorySide { Plugin, StoreWorker }
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FileRefusal {
    Empty, Binary, NoParser, NoImporter, NoFormat,
    PluginStopped { plugin: Box<str> },
    Locked { detail: Box<str> }, Unreadable { detail: Box<str> },
    // archive members (§6.9)
    Encrypted, Damaged, TooManyEntries, TooLarge, Nested,
}
pub enum Status { Pending, Supported, Limited, NotSupported, CantRead }
pub fn status_of(r: &Result<Judged, FileRefusal>) -> Status;          // pure, the table of ADR 0067 §2
```

`FileRefusal`'s `Display` gives today's strings [A]; the UI words come from a key per variant (`refusal.<kebab>`), F9b.
ADR 0067 calls it `Refusal`; it is renamed here because `paddock_plugin_api::compat::Refusal` is in scope in the same
crates (§17, Q22).
`choose` and `plan_with` become `pub(crate)`; F1b's conformance check fails on another whole-file caller.

### 6.3 The table and view, pure (F3)

```rust
// debrief-core/src/folder_scan/table.rs   (no I/O)
pub struct ScanTable { /* flat arrays: parent, name arena offset, kind, status, reason, size, files, selected, check, children */ }
pub struct NodeId(u32);
pub struct Version(u64);
impl ScanTable {
    pub fn from_listing(listing: impl Iterator<Item = ListedEntry>, limit: &CutRow<First>) -> Bounded<ScanTable>;
    pub fn set_verdict(&mut self, node: NodeId, v: RowVerdict) -> Version;
    pub fn check(&mut self, op: CheckOp) -> Result<Version, NodeError>;   // Node(id) | Recommended | AllSupported | None | Range(a, b)
    pub fn totals(&self) -> Totals;
    pub fn cover(&self) -> FolderSelection;                                // smallest rule set; property-tested
}
pub struct ScanView { expanded: BitSet, sort: SortSpec, filter: FilterSpec }
impl ScanView {
    pub fn apply(&mut self, table: &ScanTable, change: ViewChange) -> ViewState;   // count, version, focus index
    pub fn page(&self, table: &ScanTable, offset: u64, limit: Limit) -> Bounded<RowsPage>;   // Bounded::page, of: "rows"
}
pub enum SortKey { Name, Type, Status, Reason, Size, Files }    // natural order shared with pd-table's compareValues
pub struct FilterSpec { pub name: Option<Box<str>>, pub types: Vec<ImportClass>, pub statuses: Vec<Status> }
```

`RowVerdict` is `{ status, class, reason: Option<ReasonKey>, limits }`; `ReasonKey` is a typed enum (FileRefusal kinds, limit
kinds, `LikelyDuplicate { of: NodeId }`, `SameContent { of: NodeId }`, `NoRule`, `ProjectData`).

### 6.4 The scan job (F4)

```rust
// debrief-core/src/folder_scan/run.rs
pub struct ScanSpec { pub roots: Vec<GrantedRoot>, pub threads: ThreadCount, pub head_bytes: u32 }
pub struct ScanHandle { pub scan: ScanId, pub job: JobId }
pub trait ScanPorts: Send + Sync {                  // adapters: fs, plugins, the zone probe; fakes in tests
    fn list(&self, root: &GrantedRoot, sink: &mut dyn FnMut(ListedEntry) -> ControlFlow<()>) -> io::Result<()>;
    fn head(&self, file: &FileMeta) -> Result<Head, FileRefusal>;
    fn sniff(&self, batch: SniffBatch) -> Result<SniffScores, PluginFault>;   // one `debrief.sniff.v1` call per plugin (§5.10)
    fn zone_probe(&self, table: &ScanTable, budget: ProbeBudget) -> Option<ZoneGuess>;
}
/// A job body under ADR 0052 (§9.6): Confirmed after the read-back of the table's totals, Unconfirmed when the listing
/// was cut or a plugin was not asked, Err for a root that cannot be read; a cancel ends the job `Cancelled`.
pub fn run_scan(spec: ScanSpec, ports: &dyn ScanPorts, job: &JobContext, table: &RwLock<ScanState>) -> Result<Outcome, JobFailure>;
```

Phases (job phase ids): `listing` (growing count), `detecting` (done of total), `zone` (budget), `duplicates` (done of
total) [A: ADR 0067 §3]. Live role (rule 52): a person waits on it; normal level on Windows, may be utility elsewhere.
A plugin crash during a sniff does not count toward quarantine; after 3 in one scan that plugin is not asked again (D9)
[A]: `PluginsForScan::mark_stopped(plugin)` is scan-local state, never `paddock-guard`.

### 6.5 Commands (F7)

All take the window and check the scan belongs to it. Paths never cross after `folder_scan_start` [A].

| Command | Kind | Rust | TypeScript (`FolderImportBackend`) |
|---|---|---|---|
| `open_plan` | Query | `fn open_plan(win, paths: Vec<String>) -> CmdResult<OpenPlan>` | `openPlan(paths: string[]): Promise<OpenPlan>` |
| `folder_scan_start` | Mutation | `fn folder_scan_start(win, choose: Vec<ChooseToken>) -> CmdResult<ScanHandle>` | `start(choose: ChooseToken[]): Promise<ScanHandle>` |
| `folder_scan_rows` | Query | `fn folder_scan_rows(win, scan: ScanId, offset: u64, limit: u32) -> CmdResult<Bounded<RowsPage>>` | `rows(scan, offset, limit): Promise<Bounded<RowsPage>>` |
| `folder_scan_view` | Mutation | `fn folder_scan_view(win, scan, change: ViewChange) -> CmdResult<ViewState>` | `view(scan, change): Promise<ViewState>` |
| `folder_scan_check` | Mutation | `fn folder_scan_check(win, scan, op: CheckOp) -> CmdResult<CheckResult>` | `check(scan, op): Promise<CheckResult>` |
| `folder_scan_totals` | Query | `fn folder_scan_totals(win, scan) -> CmdResult<Totals>` | `totals(scan): Promise<Totals>` |
| `folder_scan_retry` | Mutation | `fn folder_scan_retry(win, scan, node: NodeId) -> CmdResult<Version>` | `retry(scan, node): Promise<Version>` |
| `folder_import` | Mutation | `fn folder_import(win, scan) -> CmdResult<JobId>` | `importSelected(scan): Promise<JobId>` |
| `folder_scan_close` | Mutation | `fn folder_scan_close(win, scan) -> CmdResult<()>` | `close(scan): Promise<void>` |

```rust
pub struct OpenPlan { pub direct: Vec<String>, pub choose: Vec<ChooseToken> }   // ChooseToken: an opaque grant-backed id, not a path
pub struct RowsPage { pub rows: Vec<ScanRow>, pub count: u64, pub version: Version }
pub struct ScanRow { pub node: NodeId, pub level: u16, pub posinset: u32, pub setsize: u32, pub parent: Option<NodeId>,
                     pub name: Box<str>, pub kind: NodeKind, pub status: Status, pub class: Option<ImportClass>,
                     pub reason: Option<ReasonKey>, pub size: u64, pub files: u32, pub check: CheckState, pub tags: Vec<RowTag> }
pub struct Totals { pub selected_files: u64, pub selected_bytes: u64, pub disk_free: Option<u64>,
                    pub need_attention: NeedAttention, pub not_selected: Vec<(ReasonKey, u64)>, pub estimate_s: Option<u32> }
```

**Deviation from ADR 0067 [J]:** `folder_scan_start` takes `ChooseToken`s from `open_plan`, not paths, so the second
call cannot name an ungranted path even by mistake; the token resolves to the canonical root `open_request` checked.
§17, Q8.

**Event:** `folder-scan:progress { scan, phase, done, total, version }`, at most 4 Hz, emitted to the owning window
only.

**UI** (`app/src/app/import/folder/`): `FolderImportBackend` (abstract class), `TauriFolderImportBackend` (through
`core/activity/ipc.ts`), `MockFolderImportBackend` (`core/mock/mock-folder-scan.ts`, no judgement, statuses from the
scenario) [A: ADR 0067 §15]; `FolderImportStore` (SignalStore: scan id, window range, focus, version). The dialog id is
`folder-import`, kind `tool` (ADR 0067 D2).

### 6.6 Selection, prefix lookup, the walk (F6)

```rust
// debrief-core/src/paths/prefix.rs (new; one helper for ADR 0066's zone_settings and ADR 0067's rules, rule 55)
pub struct RelPrefix(Box<str>);                   // normalised '/'-separated, no '..', archive members as `a.zip!/m`
pub struct PrefixMap<V> { /* sorted */ }
impl<V> PrefixMap<V> { pub fn insert(&mut self, p: RelPrefix, v: V); pub fn longest(&self, rel: &RelPath) -> Option<&V>; }

// debrief-core/src/api.rs
pub struct FolderSelection { pub root: CanonicalRoot, pub default_new: NewFiles, pub rules: Vec<(RelPrefix, bool)>,
                             pub not_selected: Vec<(ReasonKey, u64)> }
pub enum NewFiles { Recommended, Ask }
pub struct WorkspaceState { /* existing roots, removed, ... */ pub selections: Vec<FolderSelection> }

// debrief-core/src/ingest/discover.rs
pub struct CompiledSelection { /* PrefixMap<bool> per root */ }
impl CompiledSelection { pub fn prunes_dir(&self, rel: &RelPath) -> bool; pub fn selects(&self, rel: &RelPath) -> Option<bool>; }
```

`discover_stream` takes `Option<&CompiledSelection>`; one line in `Walk::list` beside `removed` [A].

### 6.7 The link-refusing open (F7, on the release path)

```rust
// debrief-core/src/fs/open.rs (new; used by the scan and every import)
/// Opens `rel` under `root` without following a link anywhere in the path, and checks where the handle really is.
pub fn open_under(root: &RootHandle, rel: &RelPath) -> Result<File, OpenRefusal>;
pub enum OpenRefusal { IsLink, MovedOutside, Io(io::Error) }
pub struct RootHandle { /* the root's own handle and its final path, got the same way as the file's */ }
```

Unix `O_NOFOLLOW`, Windows `FILE_FLAG_OPEN_REPARSE_POINT` then `FILE_ATTRIBUTE_REPARSE_POINT`; final path by
`F_GETPATH`, `/proc/self/fd`, `GetFinalPathNameByHandleW` on both root and file [A: ADR 0067 §5, "Trust"]. Tests in
`debrief-core/tests/it/open_under.rs` (Unix symlink swap) and the Windows junction test on the Windows runner. Rule 54:
F7's merge batch builds the installers.

### 6.8 The whole-file gate and memory factors (FM)

```rust
// debrief-core/src/ingest/gate.rs (new)
pub trait FootprintSource: Send + Sync { fn worker_footprint(&self) -> u64; fn idle_footprint(&self) -> Option<u64>; }
pub struct GateConfig { pub store_cap: u64, pub headroom: u64, pub whole_file_max: u64, pub idle_allowance: u64 }
pub struct WholeFileGate { /* FIFO queue, ledgers per ProcessId, one mutex; no lock held while waiting */ }
impl WholeFileGate {
    pub fn new(cfg: GateConfig, footprint: Arc<dyn FootprintSource>) -> WholeFileGate;
    /// Most the gate can ever give one input: store_cap − headroom − idle, at most whole_file_max. The dialog uses this.
    pub fn allowance(&self) -> GateAllowance;
    /// Reserves both sides at once or neither; FIFO; returns a guard that frees on drop (success, error, cancel, panic).
    pub fn admit(&self, need: Need, cancel: &CancelToken, waiting: &dyn Fn(&WaitingFor)) -> Result<Reservation, GateError>;
    /// Drops every reservation held for a plugin process that ended (rule 53: keyed by ProcessId).
    pub fn process_ended(&self, id: &ProcessId);
}
pub struct Need { pub worker: u64, pub plugin: Option<(ProcessId, u64)> }
pub enum GateError { NeverFits(LimitExceeded), Cancelled }
pub struct WaitingFor { pub holder: SafePath }       // "Waiting for memory: held by <safe path>"

// debrief-core/src/importers/memory.rs  + importers/memory.toml (data, rule 21)
pub struct MemoryFactor { pub importer: Box<str>, pub access: Access, pub worker: Factor, pub plugin: Factor,
                          pub measured_at: Box<str> /* commit */, pub block: Box<str> /* performance.md anchor */ }
pub enum Factor { Measured(f32), AtLeast(f32) }
pub const WORST_FACTOR: f32 = 22.0;                  // until FM replaces it with shipping-build numbers
pub fn need_for(size: u64, factor: Option<&MemoryFactor>) -> Need;
```

A test ties `memory.toml` rows to the declared inputs: a streaming importer (`Access` other than `Seekable` with
`whole`) must have no row. FM's tests are those ADR 0067 §2 lists, in `debrief-core/src/ingest/gate/tests.rs` (fake
footprint, 8 threads both-or-neither, FIFO, release on every path, recycled pid) [A].

### 6.9 Zips in the dialog (F11)

`paddock_archive::Members::entries()` gives the tree; phase 2 inflates only the head of each member (§4.8). A member's
`RelPath` is `archive!/member` (`archive::ENTRY_SEPARATOR`) [A]. Archive errors map to `FileRefusal::{Encrypted, Damaged,
TooManyEntries, TooLarge, Nested}`.

### 6.10 Queries and removal by folder (F8)

```rust
// debrief-core::query
pub struct FolderRef { pub root: CanonicalRoot, pub prefix: RelPrefix }
pub struct Query { /* existing */ pub source_folders: Vec<FolderRef> }       // at most 256 (`query.source_folders`, hard)
// Store port (Ingest)
fn remove_sources(selector: &SourceSelector) -> RemoveReport;               // one transaction, one maintain, one state write
pub enum SourceSelector { One(SourceId), Many(Vec<SourceId>), Folder(FolderRef) }
pub struct RemoveReport { pub removed: u64, pub journal: JournalEntryId }
```

Undo flips the rules back and starts an import job ("Reading them again"); notes anchored by id fall back to their
anchor (§2.4 makes this the `EvidenceRef` resolution once FD-E2 lands).

### 6.11 Tests

ADR 0067's test plan [A], at these paths: `debrief-core/src/folder_scan/*/tests.rs` (pure), `debrief-plugins/tests/it/
judge_agreement.rs` (agreement over real IPC, with the shared `time_corpus`), `debrief-core/tests/it/folder_scan.rs`
(scan phases, cancel, errors), `app/src-tauri/src/folder_scan/tests.rs` (trust: node out of range, other window, link
swap, path in a request), `debrief-store-duck/tests/folder_selection.rs` (prune, reload, tail, `remove_sources`, undo,
`source_folders` at 256 and 257, `broken_promises`), `app/src/app/import/folder/*.spec.ts` (mock), `app/e2e/
folder-import.e2e.ts` (Playwright, mock), `app/e2e-tauri/folder-open.e2e.ts` (real engine), `debrief-store-worker/
tests/it/folder_open.rs` (F12 acceptance).

## 7. The store

### 7.1 Port changes in 1.2 (one row each in `store_port_table!`)

| Port | Method | Package | Notes |
|---|---|---|---|
| `Ingest` | `remove_sources(selector) -> RemoveReport` | F8 | replaces `remove_source(id)` |
| `Ingest` | `open(paths)`, `restore(state)`, `reload()`, `refresh(changed)` take the compiled selection from `WorkspaceState` | F6 | signature unchanged; state gains `selections` |
| `Inspect` | `resolve(refs) -> Bounded<Vec<Resolved>>` | FD-E2 | §2.4 |
| `Skew` | `source_time(source) -> Bounded<TimeProvenance>`, `set_source_time(req) -> JobId`, `undo_source_time(entry) -> JobId` | Z6, Z5, B3 | ADR 0066 §14 [A] |
| all bounded methods | replies in `Bounded<T>` | B4 to B6, 4.2 | ADR 0065 [A] |

Each change bumps `WIRE_VERSION` (`debrief-store-client/src/port_table.rs`) once per package, in the commit that changes
the port [A: ADR 0065 §7].

### 7.2 Writer contract for the streaming import

```rust
// debrief-store-duck/src/writer.rs (existing BatchSink, SegmentSink)
impl SegmentSink {
    /// A source begins. `reserved: None` for a retry after a plugin fault: a fresh id outside sorted order (ADR 0064 §2).
    pub fn begin_source(&self, meta: &SourceMeta, reserved: Option<SourceId>) -> Result<SourceHandle, StoreError>;
    pub fn abort_source(&self, h: SourceHandle);          // rows stay in segments, never listed (ADR 0062)
    pub fn finish_source(&self, h: SourceHandle, outcome: &SourceSummary) -> Result<(), StoreError>;
}
pub struct SourceSummary { pub end: ImportEnd, pub time: TimeProvenance, pub ordinal: OrdinalKind, pub problems: Problems }
```

- Ids: the block allocator checks `EventId::new` (§2.1); ordinals declared per source, a repeated `RecordNumber` kept and ordered by id (§2.2, Q-U2 (b2)); the source is listed with
  its `source_time` row in the same catalog transaction (ADR 0066 §11) [A].
- `MAX_RESERVED` rises to `MAX_ENTRIES` (ADR 0064 §3) [A].
- `RLIMIT_NOFILE`: the store worker raises its soft limit at start to min(hard, system maximum), logs it; `EMFILE` is
  `LimitExceeded { import.open_files }`, which lowers files in flight [A: ADR 0064].

### 7.3 Format versions

Every schema change takes the next `FORMAT_VERSION` on its own, in landing order (ADR 0065 "Answers", ADR 0066
"Answers") [A]. Expected in 1.2: ADR 0065 instance 12 (`segments.fields_dropped`), ADR 0066 (`zone_settings`,
`import_roots`, `source_time`), FD-B (ordinal column, if chosen), DuckDB 2 storage format (D-a). No migration: older
stores are refused with "open its log files again".

## 8. Memory bounds

Where each bound is enforced, so no package invents another (roadmap layer G).

| Bound | Default | Enforced by | Section |
|---|---|---|---|
| Plugin process cap | 1 GiB, range 64 MiB to 4 GiB; "No cap" with a machine floor | `MemoryPolicy` in grants, `ProcessProbe` watcher, `Failure::OverMemory` | §4.2, §4.5 |
| Store worker import budget | 768 MB (gate for D7) | bench gate, B1 | §9.4 |
| Store worker whole-file gate | cap − headroom − idle, ≤ 1,280 MB | `WholeFileGate` | §6.8 |
| Per record | `import.record_bytes` 8 MiB | framer budget (SDK), host count (SI-10) | §3.2, §3.7 |
| Per item on the wire | 8,192 events or 4 MiB; frame 16 MiB | SDK sink, `paddock-ipc` | §5.3 |
| In flight per file | credit × item (about 16 MiB at credit 4) | `Remote::stream` | §4.2 |
| Spool | 16 GiB per import, 2 GiB disk left | `Spool` | §4.8 |
| Archive expansion | ratio 200, floor 64 MiB | `paddock-archive` guard | §4.8 |
| Scan table | `folder.scan_entries` 500,000 (about 33 MB) | `ScanTable::from_listing` | §6.3 |
| Documents (dumps, captures) | 512 MiB encoded per document; reader 3 GiB | ADR 0023 limits, ADR 0068 §12 | §14.8 |
| Open documents | `MAX_DOCS` 32 by count today | MEM-2's byte budget (a prerequisite, below); MEM-6 widened (FD-G) | §14.8 |
| UI process | no import parsing in the app process (MEM-1) | conformance C<whole-file-read> over `app/src-tauri/src/{profiling,dumps}.rs` | §3.9 |
| **Plugin items in flight, all files of an import** | `performance.import.items_in_flight_mb`, **128 MiB** (registry row `import.items_in_flight`, unit MiB, 64 to 1,024), raised only with 2.6b's and 4.3's recorded measurements | `CreditPool` (§4.2), initial grants included; at 128 MiB at most 10 plugin streams open (128 / 12 MiB). Without it: 16 files × credit 4 × up to about 12 MiB per item (`EVENT_FRAME_MAX`) ≈ 768 MiB, the whole store-worker budget, and more at credit 64 [calc] | §4.2 |
| Analysis results cache (Q-2) | `analysis.results_cache_bytes` 64 MiB (`hard` until measured) | LRU by run id, a superseded run dropped first | §14.5 |
| Trace and request cache (Q-3) | `analysis.trace_cache_bytes` 64 MiB (`hard` until measured) | LRU by `(QueryKey, generation)` | §14.5 |
| Lane buckets per reply | `timeline.bucket_cells_max` 2 Mi cells (8 MiB of `u32`), `hard` | lanes × levels × buckets capped; over it the reply is cut by lanes and the panel asks for the rest. Without it 200 lanes × 6 levels × 11,520 buckets × 4 B ≈ 55 MB, past the 16 MiB frame [calc] | §14.2 |

Footprint is physical memory per rule 26 (macOS footprint, Linux PSS, Windows private bytes, recorded by 2.2).

**Named prerequisite: MEM-2.** The open-documents byte budget is built (`ed8e6005a` "one memory budget for open captures
and documents, released least recently used", on `origin/debrief/p0-idle`), but it is not on `origin/main`,
`origin/train/2026-11-11-5` or `origin/debrief/1.2-int` (`git merge-base --is-ancestor`, 2026-11-12). **Owner: plan package L-0** (the integrator
landing `p0-idle` on `debrief/1.2-int`, plan §9.2). It must land before the MEM-6 ADR's budget API (§14.8) and FD-G
build on it, and before the restored ADR 0068 checks (§14.13) admit held dumps to it (`debrief_budget::MemoryBudget`, WP-RR). Until then this
row's bound is `MAX_DOCS` by count, and nothing in 1.2 may cite MEM-2 as done.

**The store worker's import budget, 768 MB (D7's gate), by part.** The 768 MB is the worker's **total peak physical
footprint** during an import (rule 26), not an increment above idle: that is how ADR 0067 §2 measures it ("the store
worker peaked at 805 and 1,268 MB … a fail against 768") and how the whole-file gate subtracts it from the store cap
[A]. So the idle footprint is inside the 768. The parts must add up; 4.3 measures each and replaces these numbers with
recorded ones (rule 48):

| Part | Bound | MB |
|---|---|---:|
| Read buffers: `2 × threads` files (16) × (up to 8 MiB chunk + 1 MiB head) | `CHUNK_BYTES`, `HEAD_BYTES` | 151 [calc] |
| Plugin items in flight | `CreditPool` | 134 (128 MiB) |
| ~~ADR 0062's tee share (index and names queues)~~ | none in 1.2: ADR 0062's trim of 2026-11-11 deferred WP-4 to WP-8 (the tee); it comes back only if WP-1 misses its target on large10 | 0 |
| Subtotal of bounded import parts | | 285 [calc] |
| Idle footprint allowance (ADR 0067 §2, until measured) | macOS 630, Linux 320, other 300 | [A] |
| Left for all import parts on macOS / Linux / other | 768 − idle | 138 / 448 / 468 [calc] |
| Left after the bounded parts, for batches, the Parquet writer and DuckDB, on macOS / Linux / other | | −147 / 163 / 183 [calc] |

Revision 6 recomputed this table without the tee (revision 5 counted a 240 MB tee share: 525 MB of bounded parts).
So **the budget still does not add up on macOS**: 138 MB remain there for 285 MB of bounded parts. On Linux and other
OSes it now adds up, with 163 and 183 MB left for batches, the Parquet writer and DuckDB, which 4.3 has not measured yet
[calc]. §17 Q27 (decided by the user 2026-11-07: package 4.3 measures the idle footprint and every part first, then
per-OS budgets, smaller parts, or both) stands; only its numbers changed. If WP-1 misses its target and the tee comes
back, its share returns to this table and to §16 with ADR 0062's numbers.

## 9. Jobs

### 9.1 Kinds and queues

Debrief's `JobKind` enum (`app/src-tauri/src/jobs/model.rs`) [S] stays the queue key; M2a maps each to
`paddock_jobs::JobKind` slugs. Added: **`Scan`** (`"scan"`, ADR 0067), one per window, does not wait behind an import.

| Kind | Slug | Concurrency | Role (rule 52) |
|---|---|---|---|
| `Import` | `import` | one per window (ADR 0012) | Bulk |
| `Scan` | `scan` | one per window | Live |
| `Analysis` | `analysis` | cores − 1 shared | Live (a person waits) |
| `Capture` | `capture` | one per tool | Bulk (a person started it and waits for the file) |
| `Maintenance` | `maintenance` | one per store | Idle for compaction and sweeps nobody waits for (C54 comment); Background when a person asked (index repair, Y4) |
| `Export`, `Collection`, `Download`, `Ai`, `Research` | as today | as today | as today |

`Capture` (profiler and dump captures, ETW sessions) and `Maintenance` (compaction, index repair, spool sweeps) are
added so that no long operation runs outside a kind's queue [J].

### 9.2 Phase ids (stable, for tests and i18n)

| Job | Phases |
|---|---|
| Import | `finding` (growing), `plan` (files), `spool` (bytes), `waiting-memory` (indeterminate, names the holder), `read` (bytes), `corrections` (sources), `maintain` (steps) |
| Scan | `listing`, `detecting`, `zone`, `duplicates` |
| Folder import (one job after Enter during a scan) | the scan's phases, then the import's |
| Re-time (ADR 0066 §8) | `planning`, `rewriting` (segments), `reindexing` |
| Remove by folder | `removing` (sources), `maintain` |

### 9.3 Idempotency (J1)

```rust
// paddock-jobs
pub struct IdempotencyKey(u128);                 // hash of kind + canonical request; built by the app
impl JobRegistry {
    pub fn submit_keyed(&self, kind: JobKind, key: IdempotencyKey, run: JobFn) -> Submitted;   // JobFn: §9.5, returns Outcome
}
pub enum Submitted { Started(JobId), Joined(JobId) }    // two identical submits give one id
```

### 9.4 Parallelism and cost of the foundation's long operations (ER-20)

| Operation | Bound | Setting | Cheaper alternative checked |
|---|---|---|---|
| Import parse | cores − 1, at most 8 | `performance.import.parse_threads` (Auto) | §3.8 |
| Scan judge | `performance.import.parse_threads` local, 4 remote | same | raw-byte binary check (F1) cuts about 95 % of judge time [A, calc] |
| Plugin calls | files in flight | none (derived) | sniff memo and batch |
| Spool | one per file in flight | `import.spool_bytes` | sequential members never spool |
| Evidence resolve | one query per 1,000 refs | none | id hint first, locator second, hash last |

### 9.5 The job service on the shared executor (Y9, the part that stays)

Y9's 8-day item stays in 1.2: Debrief's job service (`app/src-tauri/src/jobs/service.rs`) runs on family ADR 0020's
executor, which means ADR 0020's WP-1 to WP-3 and the `jobs/service.rs` part of WP-8 (`Timetable` and `ManualExecutor`
exist on no branch yet) [S: scope plan's Y9 row]. The executor's API is ADR 0020 §"API" [A]; what Debrief adds:

```rust
// app/src-tauri/src/jobs/service.rs, after Y9
pub struct JobService { exec: paddock_jobs::exec::Executor, registry: paddock_jobs::JobRegistry, queues: QueueTable }
impl JobService {
    /// One queue per `JobKind` (§9.1) with its concurrency; every job is an executor task with its `TaskSpec`.
    pub fn submit(&self, kind: JobKind, key: Option<IdempotencyKey>, role: WorkRole, run: JobFn) -> Submitted;
}
/// Rule 52 mapping, the only place a job's class is decided, reading the one level table (`paddock_proc::priority`, §4.5):
/// Live → Class::User (Class::Interactive only for work that blocks the UI); Bulk → Class::Background, which is the
/// gentle level on every OS under ADR 0020's 2026-10-30 amendment (UTILITY, nice 10, BELOW_NORMAL). Class::Idle, the
/// only starvable class, is never returned: a caller names it directly with the C54 comment.
pub fn class_for(role: WorkRole) -> paddock_jobs::exec::Class;
pub type JobFn = Box<dyn FnOnce(&JobContext) -> Result<Outcome, JobFailure> + Send + 'static>;   // ADR 0052 §2.1
```

- Tests run on `ManualExecutor` with a fake clock (no sleeps): `app/src-tauri/src/jobs/tests.rs`.
- ADR 0020's `submit_child(.., cap: paddock_proc::MemoryCap)` predates `MemoryPolicy` (§4.5): §17, Q16.
- The executor is `paddock-jobs`' module `exec` (ADR 0020 §"Decision") [A], so J1's `submit_keyed` (§9.3) and the
  executor live in one crate.

### 9.6 How a job ends (ADR 0052, accepted)

ADR 0052 §2 [A] in `paddock-jobs` (its WP-B1, a dependency of every foundation job package, §15): `JobState::Done {
outcome }`, `Outcome::{Confirmed { summary }, Unconfirmed { why, next_step }}`, `Failed { reason, next_step }`; a body
returns `Result<Outcome, JobFailure>`; `Confirmed` only through `Outcome::confirmed(read_back(..)?, summary)`, where
`read_back` calls the same port the feature reads through; `NextStep` and `Why` defined once there; the executor logs
every end. The UI gets `JobEnd<T>` through `JobRunner` and shows it with `pd-outcome` (ADR 0052 §2.3). The foundation's
jobs map like this [J]:

| Job | Read back through | Confirmed when | Unconfirmed | Failed |
|---|---|---|---|---|
| Import (and folder import) | the store's source listing (`summary.sources`) | every source the job finished is listed ready with its event count | some sources failed or were refused after a Supported verdict: `why = SourcesFailed { n }`, `NextStep::Retry`; a listing that disagrees: `why = ReadBackMismatch`, `NextStep::ShowLog` | nothing could be read (root gone, store refused): `NextStep::Retry` or `OpenSettings` |
| Folder scan | `folder_scan_totals` (§6.5) | totals equal the table's counts and no cut | listing cut at `folder.scan_entries` (`NextStep::OpenSettings(path)`); a plugin stopped for the scan (`NextStep::Retry`) | the root cannot be read |
| Re-time (`set_source_time`) | `source_time(source)` for each moved source | provenance names the new zone or day order | a source failed its swap: `NextStep::Retry` | nothing moved |
| Remove by folder (F8) | the source listing under the prefix | no source left under it and the rules flipped | some sources could not be removed | none removed |
| Undo of a removal | the source listing | sources back with their ids (fingerprints, ADR 0051) | back with new ids: `why = IdsChanged` (notes fall back, §6.10) | |
| Analysis (§14.6) | the run's stored result (Q-3's cache) | result stored under its `AnalysisRunId` | a cut it could not avoid stays in `Bounded`; it is not Unconfirmed | the query failed |
| Maintenance, capture | as their ADRs | | | |

Each row has the engine test ADR 0052 requires, in which the read-back disagrees and the job ends Unconfirmed, beside
the job's other tests. ADR 0068's added `NextStep` variants are its WP-18's (moved out with the checks, except as WP-1
needs them).

## 10. Time model (ADR 0066)

ADR 0066 [A] decides it. Placement and the signatures a plugin or package needs:

### 10.1 Placement

- `debrief-plugin-ext::time`: `TimestampParser` (with `parse_stamp`), `TimeReader`, `TimeTally`, `Stamp`, `Written`,
  `NativeClock`, `EventStamp`, `TimeReport`, `AssumedZone::Inherit`. No zone database (plugins do not link `jiff`).
- `debrief-core::zone` (`model`, `report`, `table` (only file calling `jiff`), `resolve`, `plan`, `detect`, `check`).
- `debrief-store-duck`: `zone_settings`, `import_roots`, `source_time` tables; `Skew` port rows (§7.1).

### 10.2 What importers see

`ctx.times()` (§3.2, on `&mut ImportCtx` as ADR 0066 D13 recommends) returns the one reader of the call; the SDK calls `finish()` after `import` returns and puts the
`TimeReport` into `ImportDone.time` (§5.4). A splittable in-process importer gets one reader per region; tallies merge
in line order [A].

```rust
impl TimeReader {
    pub fn read(&mut self, text: &str, line: u32) -> Option<EventStamp>;
    pub fn absolute(&mut self, ms: i64, line: u32, kind: NativeClock) -> EventStamp;
}
pub struct EventStamp { pub ms: i64, pub ts_state: u8 }     // §5.3's byte: WALL set when digits only (ADR 0066 §2)
pub enum NativeClock { FileTime, UnixMs, UnixNs, Header }
```

### 10.3 Times on documents and references (roadmap layer J)

**One rule [J, from the roadmap]:** every time a foundation type carries outside the event table (a dump's create time,
a UMDH snapshot, a capture start, an evidence reference, a process start) is a `StampedTime`:

```rust
// debrief-core::zone::model
pub struct StampedTime { pub ms: i64, pub basis: StampBasis, pub confidence: Confidence }
pub enum StampBasis { Instant(Written), Wall { zone: ZoneReadingShort } }   // not ADR 0066's importer `TimeBasis` (§3.2)
```

So "logs around the dump" (ADR 0068 §6) and a rate per hour are honest about how the time was read.

### 10.4 Commands

`source_time(source)`, `set_source_time { scope, zone?, day_order? } -> JobId`, `undo_source_time(entry) -> JobId`,
`list_time_zones(query) -> Bounded<Vec<ZoneChoice>>`. The display choice gains `'logs'` beside `'utc' | 'local'`
(`core/format.ts`) and the engine returns the offset table for a range [A: ADR 0066 §10].

## 11. Findings: the shared `Finding` from ADR 0068's check report

Roadmap layer I and FD-I; ADR 0068 WP-1 builds the contract. Four shapes exist today [S: roadmap]; one type replaces
them, and ADR 0068's `CheckReport` is built on it rather than beside it.

```rust
// debrief-core::finding  (ts-rs → core/generated/finding.ts)
/// The common part of every finding: what was concluded, how sure, on what evidence, what could not be looked at.
pub struct Finding<V> {
    pub verdict: V,                            // a closed enum per producer (CrashVerdict, HealthVerdict, ...)
    pub confidence: Confidence,                // { level: High | Medium | Low, because: Vec<ConfidenceReason> }
    pub evidence: Vec<Evidence>,               // at most 64; cut reported in `left_out`
    pub not_checked: Vec<NotChecked>,          // { item: CheckItem, why: MissingReason, next: Option<NextStep> }
    pub left_out: Vec<Cut>,                    // ADR 0065's Cut (ADR 0068 says Vec<Omission>: §17, Q4)
}
pub struct Evidence { pub what: EvidenceRef, pub label: LabelKey, pub role: EvidenceRole }   // §2.4
pub enum EvidenceRole { Primary, Supporting, Contradicting }
pub enum MissingReason { StreamMissing(StreamKind), MemoryNotInDump, NoSymbols(Vec<Box<str>>), NoUnwindInfo(Vec<Box<str>>),
    ToolUnavailable, DacUnavailable, OverBudget, DifferentProcess, NeedsMore { of: NeedsMoreOf, have: u32, need: u32 },
    FileChanged, Unsupported(Box<str>) }                                  // ADR 0068 §1's closed list [A]
pub enum NeedsMoreOf { Dumps, Snapshots }
/// `CannotTell` is every producer's verdict when the data is missing (ADR 0068 §1): a variant each verdict enum must have.
pub trait VerdictKind: Serialize { fn cannot_tell() -> Self; fn is_cannot_tell(&self) -> bool; }

/// The type-erased form for reports, Pitwall's fact bundle and the AI tools.
/// JSON only (engine, store worker, UI, and inside a JSON body on the plugin wire, §5.9); never a postcard type.
pub struct AnyFinding { pub producer: Producer, pub kind: Box<str>, pub verdict: serde_json::Value /* validated */, pub core: Finding<()> }
pub enum Producer { DumpCheck(CheckKind), Health, Profile, Heap, Analysis(Box<str>) }

// debrief-dump::api (ADR 0068 §10), built on it:
pub struct CheckReport {
    pub kind: CheckKind,
    pub docs: Vec<CheckedDoc>,                 // CheckedDoc.process: ProcessInstance (§2.5)
    pub pairing: Option<Pairing>,              // §2.5
    pub finding: Finding<Verdict>,             // ADR 0068's `debrief_dump::api::Verdict` (Crash, Hang, Memory verdicts)
    pub threads: Option<Vec<ThreadClassRow>>,
    pub memory: Option<MemoryComposition>,
    pub around: Option<EventsAround>,
}
```

- **`NextStep`** comes from family ADR 0052's shared enum in `paddock-jobs` when that lands; until then ADR 0068's
  WP-18 adds the variants there (ADR 0068 §10) [A]. §17, Q5.
- FD-I maps the three other producers (`analysis/moment.rs` health findings, `debrief-profile` findings, heap findings)
  onto `Finding<their verdict>` and types the report's `Block::Finding` as `AnyFinding`.
- Tests: `debrief-core/src/finding/tests.rs` (serde of every `MissingReason`, `CannotTell` present in each verdict enum
  through a compile-time test per producer), and the cut of `evidence` at 64.

## 12. Logs (ER-27)

### 12.1 Correlation ids

| Id | Made by | Carried into |
|---|---|---|
| `job` (`JobId`) | job registry | every phase line of that job |
| `import_id` | import job start | per-source lines, plugin call lines, gate lines |
| `scan` (`ScanId`) | `folder_scan_start` | scan lines and the import it leads to (`Scan=7 Job=91`) |
| `call` (`u64`) | `paddock-ipc` | the plugin call's start, end, outcome; stall verdicts |
| `source` (`SourceId`) | writer | per-source lines at debug, failures always |

### 12.2 Lines the foundation adds (fields only; no file content, paths through `privacy::safe_path`)

| Where | Line | Level | Fields |
|---|---|---|---|
| import, per phase | `Import phase` | info | import_id, phase, elapsed_ms, files, bytes |
| import, per source | `Import source` | debug (failure: warn) | import_id, source, importer, access, encoding, bytes_read, events, skipped, end, duration_ms |
| plugin call | `Plugin call` | debug (fault: error) | plugin, ns, minor, role, call, outcome, items, duration_ms, ticks |
| plugin fault | `Plugin fault` | error | plugin, pid+start (ProcessId), failure kind, retried sources |
| gate | `Gate wait` / `Gate admit` / `Gate refuse` | info | import_id, need_worker, need_plugin, footprint, holder (safe path) |
| spool | `Spool` | info | import_id, member (safe path), bytes, duration_ms, outcome |
| stall | `Stall verdict` | warn on Stuck | call key, verdict, why, blocked_ticks, cpu_ms |
| limits | `Limit hit` | info | limit id, scope, value, reached |
| cuts | `Cut` | debug | command, `Cut::describe()` (ids, field paths, counts only) [A: ADR 0065] |
| scan | ADR 0067 §13's nine lines | as there | as there [A] |
| time | ADR 0066 "Logs" | as there | as there [A] |
| dump check | ADR 0068 §14 | as there | as there [A] |
| analysis run (FD-K) | `Analysis run` | info | kind, input query hash, coverage cuts, settings hash, duration_ms |
| plugin process start | `Plugin start` | debug on success, info on failure (ADR 0052 §2.4: spawns log once, through `paddock_proc`'s run helper) | plugin, version, ProcessId (pid and start time), containment kind and `Gap`s, memory policy, outcome, duration_ms; on failure the cause and next step |
| plugin process exit | `Plugin exit` | info (unexpected: error) | plugin, ProcessId, exit code or signal (NTSTATUS on Windows), reason (`idle-stop`, `stop`, `crash`, `over-memory`, `stall`), uptime_ms, calls served |
| job end | `Job end` | info (failed: warn) | job, kind, outcome kind (`confirmed`, `unconfirmed`, `failed`, `cancelled`), why, next step, phase durations, total duration_ms, cause chain (ADR 0052 §2.1: the executor writes it, so no job can skip it) |

Each line has a test that asserts its fields and that it holds no name beyond the privacy path, in the module that emits
it (ADR 0067 §13's rule, applied to all) [J]. A bug the logs could not explain adds its line in the fix.

### 12.3 The store worker's log (WP-SL1)

Its own file, read by the diagnostic bundle; the same line shapes; the worker's `RLIMIT_NOFILE`, gate allowance, idle
footprint and thread counts logged at start.

## 13. Engine-to-UI contract

### 13.1 Command table

One line per command in `command_table!` (`app/src-tauri/src/lib.rs`) [S]: `Kind [windows] (limit <bytes>)? => path`.
New foundation commands: §6.5's nine, §10.4's four, `inspect_resolve` (FD-E2). Every command returns
`CmdResult<T>`; every collection reply is `Bounded<T>` or exempt with a reason (§1.3).

### 13.2 Generated TypeScript

`app/src/app/core/generated/` gains: `limits.ts` (Debrief's registry ids and values, B3 and 3.6), `evidence.ts`,
`process.ts`, `finding.ts`, `folder-scan.ts`, `time.ts`; `store.ts` gets `EventId`. CI regenerates and fails on a diff.

**The shared `Bounded` and `Cut` types, as built (amendment L12, folded 2026-11-12).** `paddock-limits` writes no file
under `ui/`: with `--features ts` it emits declarations only, checked against its golden file
`crates/paddock-limits/tests/golden/limits.ts` (`CutWire` is exported as `Cut`, `BoundedWire<T>` as `Bounded<T>`; counts
are `number`). `Bounded<T>` and `Cut` implement `TS` by delegating to their wire types, so a struct that holds them can
derive `TS`. The UI's one copy, `ui/projects/paddock-ui/limits/generated/limits.ts`, is written by the UI entry point's
package (B2 with 2.3) from those declarations and checked equal to the golden file; apps re-export it, never copy it
(rule 55).

### 13.3 Rules for UI code

- A service calls only its `<Area>Backend`; nothing but `core/activity/ipc.ts` calls Tauri's `invoke`. The check is a
  spec, not a lint: `app/src/app/core/activity/coverage-guard.spec.ts` ("every Tauri command goes through the
  operation-id adapter", with an empty `PENDING_DIRECT_INVOKE`). No ESLint rule enforces it (the app's
  `eslint.config.js` restricts only `patchState`).
- A view of a `Bounded` reply reads `.result` and renders `.cuts` through `<pd-limit-note>` or names its surface in
  `BOUNDED_SURFACES` (ADR 0065 part 5) [A].
- Every state is designed: loading (skeleton), empty, error with Retry, cut.
- Ids are `EventId` (`number`), never array indexes, as row keys (`rowKey`, ADR 0017).

### 13.4 ADR 0063

Nothing in this file depends on ADR 0063's outcome: `Bounded` travels in the MessagePack head under it [A: ADR 0065
§7], ids stay below 2^53 either way (§2.1). The roadmap's advice stands: decide 0063 before 3.3 and 4.5 build their reply
shapes (§17, Q1, decided by the user 2026-11-07: W0 gate early in wave 1).

## 14. The rest of 1.2: lists, timeline, shell, UI on foundation types, performance, analyses, correctness

These packages build on §2 to §13 and add no second mechanism for anything those sections define. Each subsection gives
the entry points, names, Settings and test homes; where a family ADR already fixes an API (ADR 0017's list identity,
ADR 0018's lanes, workbench.md's contributions, ADR 0020's executor, ADR 0026's dialogs), the subsection names the ADR
and adds only what Debrief 1.2 needs from it.

### 14.1 Lists and the virtual primitive (1.3, 2.1, 2.7, 4.5, 3.7, F10b, SI-17)

**List identity (1.3), in `ui/projects/paddock-ui/list/` (new entry point),** as ADR 0017 decisions 1 to 5 [A]:

```ts
export type RowKey = string | number;                       // EventId is a number (§2.1)
export interface KeySelection<K extends RowKey> {
  readonly keys: ReadonlySet<K>; readonly anchor: K | null; readonly focus: K | null;
}
export const KeySelection: {
  empty<K extends RowKey>(): KeySelection<K>;
  only<K extends RowKey>(k: K): KeySelection<K>;
  toggle<K extends RowKey>(s: KeySelection<K>, k: K): KeySelection<K>;
  range<K extends RowKey>(s: KeySelection<K>, to: K, order: (k: K) => number | null): KeySelection<K>;
  retain<K extends RowKey>(s: KeySelection<K>, present: (k: K) => boolean): KeySelection<K>;   // after a refresh
};
export interface ViewportAnchor<K extends RowKey> { readonly key: K; readonly offsetPx: number }
export type ChangeReason = 'query' | 'refresh' | 'append' | 'dialog-result';
export function classifyChange(prev: QueryKey, next: QueryKey, reason: ChangeReason): 'navigate' | 'refresh' | 'append';
export function anchoredViewport<K extends RowKey>(o: {
  keyOf: (i: number) => K | null; indexOf: (k: K) => number | null; neighbourOf: (k: K) => K | null;
}): AnchoredViewport<K>;                                        // capture() and restore(anchor) for a scroller
export interface DataListRegistration { id: string; component: Type<unknown>; contract: 'list' | 'table' | 'tree' | 'grid' }
export const PD_DATA_LIST: InjectionToken<readonly DataListRegistration[]>;  // multi provider
export function provideDataList(r: DataListRegistration): Provider;
```

Contract test (1.3): `ui/projects/paddock-ui/testing/src/list-contract.ts`, `runListContract(harness)` with ADR 0017's
10,000-row script; conformance `conformance/src/data-lists.mjs` (registered count = visited count).

**The virtual primitive (2.1), `ui/projects/paddock-ui/virtual/` (new entry point):**

```ts
/** A window over rows the app holds or fetches. ONE type for 2.1 and ADR 0067's T4 (F10b): rule 55.
 *  Two modes: `indexed` when every row has a position (the folder scan, lists, the grid below the exact-position
 *  threshold), `keyed` when positions are unknown or estimated (the grid above it, §14.5). */
export type PdRowWindow<T> = IndexedWindow<T> | KeyedWindow<T>;
export interface IndexedWindow<T> {
  readonly mode: 'indexed';
  readonly count: number;                       // aria-rowcount
  row(i: number): T | null;                     // null: not loaded; a focusable placeholder is drawn
  place?(i: number): { level: number; posinset: number; setsize: number; expanded?: boolean } | null;
  parent?(i: number): number | null;
}
export interface KeyedWindow<T> {
  readonly mode: 'keyed';
  readonly count: number | null;                // null: unknown, aria-rowcount = -1 and no aria-rowindex
  readonly scroll: ScrollModel;                 // drives the primitive's own scaled scrollbar
  readonly rows: readonly T[];                  // the loaded slice, about one screen each side of the view
  /** Loads `before` and `after` rows around an anchor (a key, a time, the start or the end). */
  around(anchor: RowAnchor, before: number, after: number): Promise<void>;
  /** Where a row sits on the scrollbar (0..1), and which anchor a scrollbar fraction means. */
  fractionOf(row: T): number;
  anchorAt(fraction: number): RowAnchor;
}
export type RowAnchor = { kind: 'key'; key: RowKey } | { kind: 'time'; ms: number } | { kind: 'start' } | { kind: 'end' };
@Component({ selector: 'pd-virtual-list' })  inputs: rows?: readonly T[]; window?: PdRowWindow<T>; rowKey: (r: T) => RowKey;
  height: 'measured' | number; overscan = 1 (screens); selection: KeySelection; anchor: ViewportAnchor | null;
  outputs: rangeChange: { start: number; end: number } (indexed mode only); needRows: { anchor: RowAnchor; before: number;
  after: number } (keyed mode only: the primitive asks, the app calls `around`); anchorChange; selectionChange; activate
// pd-virtual-table and pd-virtual-tree: the same inputs plus columns (as PdTable) and sticky headers
export function boundedSlice<T>(items: readonly T[], limit: LimitValue, of: string): Bounded<T[]>;    // ADR 0065
@Component({ selector: 'pd-scroll-markers' })  input: markers: Bounded<MarkerBuckets>; output: navigate(fraction)
```

- `PdTable` (`table/src/table.ts`) moves onto the primitive; ADR 0067's T1 to T6 inputs (`activation`, `rowSpace`,
  `[pdRowControl]`, `fill`, `rowDescription`) are added there by F10, and its T4 *is* `window` above. **F10b does not
  build a second windowed-rows model: it adopts 2.1's** (roadmap stop-gap table) [A].
- Lints: `paddock/template-no-index-track`, `paddock/collections-virtualized` (2.5), warnings until 2.7 and 4.5 are clean.
- Tests: `ui/e2e/virtual-100k.spec.ts` (frame budget, DOM rows ≤ visible + overscan), each primitive through
  `runListContract`, axe in both themes.

**Rows at 1 billion events** (the user's view rule of 2026-11-05 [mem]): a view holds the visible rows plus about one
screen each side, never a cap on what the view covers; the scrollbar is mapped:

```ts
// core/generated/store.ts (engine type)
export type ScrollModel =
  | { kind: 'position'; total: number }               // exact positions: below the store's exact-position threshold
  | { kind: 'time'; from: number; to: number };       // above it: position estimated from time
```

The engine answers it with every page (§14.5, `PageReply.scroll`). In keyed mode **the scrollbar is the primitive's
own**: a track of the viewport's height whose thumb maps a fraction to an anchor through `anchorAt`, so no element is
ever `rows × row height` tall (1B rows would pass every browser's maximum element height); the content area holds only
the loaded slice. The grid's `scroll-map.ts` becomes the `KeyedWindow` implementation.

**Selection over a range in keyed mode** is a key range, not a set: `KeySelection` gains `{ kind: 'range'; from: RowKey;
to: RowKey }` beside the set form, and actions on it go to the engine as a `SourceSelector`-like `EventRange { from:
PageAnchor, to: PageAnchor }` with `events_count(range) -> u64` and the action commands taking the range, so no list
of a million ids crosses (bounded by the action's own limit row).

**Adoption.** 2.7: view `table`/`tree`, workbench Explorer, assistant turns, jobs list, notifications and settings
lists, each `provideDataList`. 4.5: Debrief's 14 result and analysis lists onto the primitive, each reading
`Bounded<T>.result` and showing its cuts (§13.3); `workspace/virtual-list.ts` deleted; the large mock profile
(`core/mock/large*`) and `app/e2e/large-profile.e2e.ts`. 3.7: the events grid keeps `PagedRowsStore` behind a
`PdRowWindow<EventView>` keyed by `EventId`; `KeySelection` replaces `row-selection.ts`; `ViewportAnchor` restored
through `queryPage({ anchor })`; live tail's follow is `append` with `stickToEnd`; Hotspots keyed and anchored; the
markers gutter fed by §14.2's markers query. SI-17 measures Explorer, source lists and the import report at 14,000
sources on these primitives and fixes only what the measurement shows.

### 14.2 Timeline and the drawing build (3.3, 2.4, 4.1, M0, flame and chart fixes)

The design is ADR 0018 decisions 6 to 8 with its amendment (top N, a catalogue, "All", a Lanes picker) [A] and the
drawing research's "draw pixels, not events" (`origin/docs/debrief-drawing`, `docs/research/2026-11-debrief-drawing.md`,
decided into the plan on 2026-11-05) [A].

**Engine (3.3)**, `debrief-core/src/workspace/timeline.rs` and `debrief-store-duck/src/{lanes,markers}.rs`:

```rust
pub struct LaneKey(Box<str>);                                   // the lane's group key text; stable across refreshes
pub enum TopLanes { N(NonZeroU16 /* 1..=200 */), All }
pub struct LanesRequest { pub query: QueryKey, pub order: LaneOrder, pub top: TopLanes, pub catalogue: Page, pub search: Option<Box<str>> }
pub struct LanesReply {
    pub lanes: Bounded<Vec<LaneHead>>,                          // Bounded::ranked, cut on `timeline.lanes_max` (hard, 500)
    pub catalogue: Bounded<Vec<LaneHead>>,                      // Bounded::page, searchable, every lane
}
pub struct LaneHead { pub key: LaneKey, pub label: Box<str>, pub events: u64, pub errors: u64, pub process: Option<ProcessInstance> }
pub enum LaneOrder { Events, Errors, FirstSeen, Name }

/// Bulk buckets for every visible lane of one panel, on a power-of-two grid, for 3x the visible range.
pub struct LaneBucketsRequest { pub generation: u64, pub query: QueryKey, pub lanes: Vec<LaneKey> /* ≤ 200 */,
                                pub resolution_ms: u64 /* power of two */, pub start: i64, pub end: i64 }
pub fn encode_lane_buckets(reply: &LaneBuckets) -> Vec<u8>;    // layout `LB` v1, little-endian, aligned columns, golden-tested
pub struct LaneBuckets { pub generation: u64, pub resolution_ms: u64, pub start: i64, pub bucket_count: u32,
                         pub lanes: Vec<LaneKey>, pub levels: u8, pub counts: Vec<u32> /* lanes × levels × buckets */,
                         pub markers: MarkerColumns, pub cut: Option<Cut> }
pub struct MarkersRequest { pub query: QueryKey, pub kinds: Vec<MarkerKindId>, pub buckets: Buckets }   // fixed count
pub fn markers(req: &MarkersRequest) -> Bounded<MarkerBuckets>;
```

Commands: `timeline_lanes` (JSON), `timeline_lane_buckets` (raw `ipc::Response`, the one raw endpoint unless the W0 outcome says otherwise; §17, Q15, decided 2026-11-07),
`timeline_markers` (JSON). A newer `generation` supersedes an older request through `query_cancel.rs`. One `MAX_LANES`
for both engines (`timeline.lanes_max`). Tests: `debrief-store-duck/tests/lanes.rs` (60 lanes give 10 plus a catalogue
of 50; paged), a differential test against the in-memory engine on `/samples`, the encoder's golden file
`debrief-core/tests/golden/lane_buckets_v1.bin` read by `ui/projects/paddock-ui/lanes/src/decode.spec.ts`.

**UI (2.4)**, `ui/projects/paddock-ui/lanes/` (new entry point), domain-free:

```ts
export interface LaneBucketsView {                           // typed arrays from the backend edge on; draw loops see nothing else
  generation: number; resolutionMs: number; start: number; bucketCount: number;
  lanes: readonly string[]; levels: number; counts: Uint32Array; markers: MarkerColumns; cut: Cut | null;
}
export function decodeLaneBuckets(buf: ArrayBuffer): LaneBucketsView;
export interface LaneScene { range: { start: number; end: number }; widthPx: number; dpr: number; rows: readonly LaneRow[]; data: LaneBucketsView }
export function drawLanes(ctx: CanvasRenderingContext2D, scene: LaneScene, theme: LaneTheme): void;   // snaps to device pixels
export function hitTest(scene: LaneScene, xPx: number, yPx: number): LaneHit | null;                  // binary search, pure
export type LaneSelection =
  | { mode: 'top'; n: number; pinned: readonly string[]; hidden: readonly string[] }
  | { mode: 'all'; hidden: readonly string[] }
  | { mode: 'pinned'; pinned: readonly string[] };
@Component({ selector: 'pd-lane-canvas' })  inputs: rows (virtual, DOM lane names as buttons), data, range;
  outputs: rangeChange, laneFocus, markerActivate. One viewport-sized canvas per visible panel, aria-hidden;
  the focused lane materialises real marker buttons (the flame chart's pattern).
@Component({ selector: 'pd-lanes-picker' })  inputs: catalogue: PdRowWindow<LaneHeadView>, selection: LaneSelection, badge;
  output: selectionChange. CDK menu with search, Top N, All, Pinned only, sort, pin; fully keyboard-accessible.
export class LaneCache { constructor(maxBytes: number); get(key); put(key, view); }  // key (query, lane, resolution, start)
```

**UI (4.1)**, Debrief's `app/src/app/timeline/`: panels on `pd-lane-canvas` and `pd-lanes-picker`; `LaneSelection` saved
with the project and the layout; restored panels created lazily; a hidden panel marks itself stale and issues no query
(`visible` signal from the dock); `buildLaneVms` per lane, keyed. Tests: 0.2's 60-lane test unmarked; a unit test that a
hidden panel issues no query; 5,000-lane mock: All within the frame budget, fewer than 100 lane rows in the DOM.

**The drawing build's other packages** (drawing research §6): the flame chart fixes (hover on an overlay, no
`canvas.width` reset per draw, visible rows only, DPR change) in `profiling/flame/`; the chart bound (the engine caps
`query_chart` at 2 points per pixel, columns to ECharts' dataset). Their research ids `F1` and `C1` collide with plan
packages F1 (the judge) and C1 (UMDH reader fixes): here they are **FL1** and **CH1** (§17, Q14).

**Canvas vs DOM: the measurement plan (plan row 0.5; ADR 0018 decision 8 says DOM unless profiling shows DOM is the
bottleneck; the drawing research recommends Canvas 2D).** Package M0, before 2.4; its steps are **CV1 to CV4**
(revision 5 called them M1a, M1b, M2, M3, which collided with the shell packages M1 to M3); each result a rule 48 block
in `docs/benchmarks.md` (shared code), machine load at or below 12.

| Step | What | Probe first (ER-33) | Pass |
|---|---|---|---|
| CV1 | A dev-only page in the **built app** draws the worst screen, 52,000 grouped `fillRect`s, over 60 frames, on WKWebView, WebKitGTK, WebView2 with a GPU, WebKitGTK with `LIBGL_ALWAYS_SOFTWARE=1`, and a Windows VM | `requestAnimationFrame` ticks at display rate and pixels change between frames | p95 frame time within one frame with a GPU, at most two in software |
| CV2 | **DOM control**, same scene and webviews: one absolutely positioned element per non-empty bucket run in virtual lane rows | same | recorded; it is the rule 71 control that shows the probe can see a slow renderer |
| CV3 | The wire: 500 KB, 2 MB, 16 MB through `invoke` JSON, `ipc::Response` and `Channel`: main-thread blocking, latency, app footprint | the custom protocol is up on each OS | decides raw against JSON for `timeline_lane_buckets` |
| CV4 | Baseline: pan and zoom on today's timeline at 60 lanes (0.2's fixture) | as CV1 | recorded, so 4.1 has a before |
| Rule | **Canvas** if CV1 passes. If CV1 fails, make it cheaper first (merge equal bars, blit during a pan) and re-run; DOM only if CV2 passes where CV1 failed; WebGL2 only as an opt-in after both fail | | the choice and the numbers recorded in ADR 0018's notes |

Settings: `timeline.lanes_default` (10, 1 to 200, or All), `timeline.cache_mb` (16) (§16).

### 14.3 Shell and workbench move (M1 to M5, Y12); dialogs

The shell and workbench APIs are workbench.md §2 to §4 and the move plan
([2026-10-29-shared-workbench-move.md](2026-10-29-shared-workbench-move.md)) [A]; dialogs are ADR 0026 and
[debrief-1-2-api-dialogs.md](debrief-1-2-api-dialogs.md) [A]. Debrief's side is a set of contributions and a context:

```ts
// apps/debrief/app/src/app/extensions/context.ts (M1)
export interface DebriefContext {                   // CommandContext becomes ShellContext & DebriefContext
  readonly workspace: WorkspaceHandle;              // the window's workspace id, never the store itself
  readonly selection: Signal<readonly EventId[]>;  // M1: today's ids; becomes Signal<KeySelection<EventId>> in 3.7
  readonly timeRange: Signal<TimeRange | null>;
  readonly timeDisplay: Signal<'utc' | 'local' | 'logs'>;   // §10.4
}
```

| Contribution (workbench.md / shell API) | Debrief registers | Package |
|---|---|---|
| `defineCommand` | every Debrief command (today's `DEBRIEF_COMMANDS` contributions); run only through the shell's registry | M1 |
| `defineKeybinding` | Debrief's default keys for those commands | M1 |
| `defineMenuItem` | Debrief's menus (`shell/menu/` moves) | M1, M2c |
| `defineHealthService` | store worker, plugins, Pitwall link, tools (the service-health chip, ADR 0055) | M2a |
| `defineDialog` | Debrief's dialogs as declared contracts (ADR 0026, [debrief-1-2-api-dialogs.md](debrief-1-2-api-dialogs.md)) | M2b |
| UI state checkpoint (not a `define*`: `provideUiCheckpoint` from `@decryp7/paddock-ui/shell/startup` over the `paddock-startup` port, `crates/paddock-startup/src/checkpoint.rs`, ADR 0058) | the window's restorable UI state, replacing Debrief's own `StartupBackend.checkpointUi` | M2a |
| `defineStatusItem` | jobs, health chip, resource meter (`pd-resource-meter`, §14.4), time display | M1, 4.4 |
| `defineSettingsPage` | Performance (with the Limits group, §14.4), Plugins, Time, Storage, AI, Parsers | M1, M2c |
| `defineNotificationKind` | `limit` (`LimitNotice`), `import`, `plugin`, `folder-scan`, `time-zone` | M1, 2.3 |
| `defineTheme` | Debrief's theme packs | M1 |
| `defineSharedState` | selection, time range, lane selection, display zone | M1, M3a |
| `defineView` | events grid, timeline, Explorer views, analyses, profiling views, dumps, Problems, Processes tool window | M3a, 4.4 |
| `definePerspective` | default, Full, profiling | M3a |
| `defineActivityItem`, `defineExplorerProvider` | Sources tree on the shared tree (virtualized, keyed); the totals row "N files not imported" (ADR 0067 §7) | M3b, F8 |
| `definePaletteProvider` | events, fields, settings | M3b |
| `defineAssistantContext` | the window's query and selection; `paddock-assistant` over `paddock-pitwall`, Debrief's tool renderers (ADR 0070: no built-in engine, workbench.md D5's llama.cpp adapter struck) | M3b |

- **Lazy by construction** (M1): a registration file imports no store or command class; the contributions lint fails
  otherwise. That lint is not built (no such rule under `eslint/` or `conformance/src` on the train); it is the move
  plan's §7 item 6, built with Y12's checks, so M1 needs it first or carries the import check as its own spec. Acceptance: **first-paint JS at most 500 kB** (today 1,233 kB), measured by `app/scripts/window-weight.mjs`
  (conformance C29) [J: the target; M1 records the measured value as a rule 48 block].
- **M1 moves `coverage-guard.spec.ts`.** It reads `extensions/command-registry` and `DEBRIEF_COMMANDS`, which M1
  replaces with the shell's registry and `defineCommand`. M1 moves its command half to the shell's registry (every
  registered command logs exactly one activity entry) and keeps its backend and `invoke` halves in Debrief, so the guard
  §13.3 relies on never lapses; the move is in M1's file list.
- **Rust (M4):** the app registers the `paddock-shell` and `paddock-workbench` Tauri plugins; Debrief's command-table
  rows for moved commands are deleted; `debrief://` events are renamed to the shell's names; capabilities grant what the
  shell sets; the files the plan lists are deleted.
- **Ports Debrief keeps:** the windowing unsaved-state port, the updater's single endpoint (Paddock `debrief-latest`,
  the family key), `paddock-download` for its segmented downloader and `paddock-pitwall` for its Pitwall client (M2c).
- **Y12** adds the move plan's 0a, the baseline retake, Phase 1, 2b, 2c and Phase 3 (about 15 days), with no API of
  its own beyond the contributions above; its conformance checks are the move plan's §7.
- Tests: the **WG0** screenshot baseline (`app/e2e/shell-baseline.e2e.ts`; the plan's M rows call it "W0 shots", renamed
  here because W0 is ADR 0063's binary-wire gate, §13.4), the e2e specs each M row lists, the contributions lint, a
  `MockRuntime` test that registers every command (M4).

### 14.4 UI on foundation types (2.3, B2, 3.2, 4.4, 4.2's Limits group, 3.6's Help page)

```ts
// ui/projects/paddock-ui/shell/notify/src/limit-notice.ts (2.3)
export type RaiseAction = { kind: 'setting'; path: SettingPath; preset?: unknown } | { kind: 'oneClick'; to: Quantity };
export interface LimitNoticeData { limit: LimitExceeded; raise: RaiseAction | null; retry: (() => Promise<void>) | null }
@Component({ selector: 'pd-limit-notice' })  input: data: LimitNoticeData   // rendered by NotificationHost, kind 'limit'
// ui/projects/paddock-ui/shell/settings/src/deep-link.ts (2.3)
export function openSettingsAt(path: SettingPath, preset?: unknown): Promise<void>;   // focuses the field
// ui/projects/paddock-ui/limit-note/ (B2)
@Component({ selector: 'pd-limit-note' })  input: cuts: readonly Cut[]; outputs: showMore(cut), raise(cut)
export function formatCut(cut: Cut, i18n: I18n): string;      // one ICU message per LimitId × Selection (ADR 0065 §4)
// ui/projects/paddock-ui/image/ (2.3)
@Component({ selector: 'pd-image-viewer' })  inputs: src, alt; fits before first paint, never upscales
```

- **Words** for every `LimitId` and `Selection` come from i18n keys the registry generator lists; the i18n check fails on
  a missing one.
- **3.2 cap UI:** command `performance_ceiling() -> MemoryCeiling { physical, ceiling, machine_floor }`; Settings ▸
  Performance shows the ceiling; Plugins ▸ Memory cap offers "No cap" with a warning icon and the floor; an import's
  `LimitExceeded { plugin.memory }` notice presets Raise it to the suggested value.
- **4.4 plugin manager UI:** commands `plugins_status() -> Bounded<Vec<PluginStatus>>` (§4.2), `process_table() ->
  ProcessTable` (§4.5); event `processes:tick` at 1 Hz while a consumer is visible, none otherwise. Shared
  `<pd-resource-meter [table]>` and `<pd-process-table [table]>` in `ui/projects/paddock-ui/shell/processes/` (2.6a);
  Debrief contributes the meter as a status item and Processes as a tool window. Settings "Start plugins" and "Stop idle
  plugins after N minutes" (§16).
- **4.2's Limits group** is generated: every registry row whose value is a Setting becomes a field of Settings ▸
  Performance ▸ Limits, with unit, bounds and default from the row; no hand-written list. The registry test resolves each
  path against the schema (ADR 0065 §3) [A].
- **3.6's Help ▸ Limits** is generated by `xtask limits-help` into `app/src/app/help/limits.generated.ts` from the
  registry rows and their message keys.
- **Rules 44 and 45 (2.3):** `paddock-settings`' test that every registered section has a setting (plugin-fallback
  exemption); Debrief's Video page moves to its view or gains a setting (M2c).
- Tests: shell-demo Playwright per `LimitId` kind (text, Raise it opens the field, Retry re-runs); a planted empty
  section fails the section test; the viewer opens a 4000 × 3000 image whole and never upscales a small one.

### 14.5 Store and query performance (ST, Q, Y11, Y4, Y15, SR, IX, P2 to P5, NR1, CPU-1, NB-1, D-0, 4.3, 5.1, 5.2, D-b, bench gates, Y3)

Most of this is inside `debrief-store-duck` and changes no port. The parts that do:

```rust
// debrief-core::ports::dto (ST-2/ST-3 with Q-1: keyset paging replaces deep OFFSET)
pub struct PageQuery { pub query: QueryKey, pub anchor: PageAnchor, pub before: u32, pub after: u32 }   // before + after ≤ 512
pub enum PageAnchor {
    Start, End,
    /// The order key `(sort?, ts, source, ordinal, id)` (§2.2, FD-B's amendment of ADR 0062). `sort` carries the value
    /// of a column sort and is allowed only under `ScrollModel::Position`.
    Key { sort: Option<SortValue>, ts: Option<i64>, source: SourceId, ordinal: u64, id: EventId },
    Position(u64),                                                         // only below the exact-position threshold
    Time(i64),
}
pub struct PageReply { pub rows: Bounded<Vec<EventView>>, pub scroll: ScrollModel, pub generation: u64 }
pub enum ScrollModel { Position { total: u64 }, Time { from: i64, to: i64 } }
pub enum SortValue { Text(Box<str>), Int(i64), Float(f64), Null }
```

- **Sorting at 1 billion events in 1.2:** time order (`ts, source, ordinal, id`) at any size; a sort by another column
  only below `store.exact_positions_max`. Above it the column sort is not offered: the header says why, with the
  threshold (a `Cut` with `Next::Narrow`). ST-5 measures a column sort at 1B and may lift this in a later release with
  its numbers [J].

- **ST-1:** segment pruning by the histogram table (it passed the mixed-segment gate; min/max zone maps did not) [mem];
  internal `trait SegmentPruner { fn select(&self, p: &PrunePredicate) -> SegmentSet }` with the histogram as its one
  implementation. **ST-5:** the 1 billion event bench (`debrief-bench.yml`, a quiet window), including a sort by a
  non-time column. **ST-6:** its content was not found in the sources read [U]; its owner names it before it starts.
- **Exact positions below a threshold:** `store.exact_positions_max`, 100M by the user's decision of 2026-11-05 [mem],
  kept at 100M as a `hard` registry row (decided by the user, 2026-11-07, Q17), revisited with ST-5's numbers (plan §9.6 D7).
- **Q-2 pageable analysis results:** `AnalysisRunId` (§14.6) plus `Page`; results kept per run until superseded (Q-3's
  cache), `Bounded<T>` per page. **Q-3:** traces and requests cached by `(QueryKey, generation)`; Moment does not wait
  for anomalies. **Q-4:** a `rows_read` counter on the store under `cfg(feature = "test-stats")` and a test per view that
  rows read ≤ k × rows shown (k per view, recorded).
- **P3:** `catalogue_status() -> CatalogueStatus { built: bool, generation: u64, sources_pending: u32 }`, blocking until
  a commit changes it (no polling), one row in the port table. **NR1:** the names rebuild is a job keyed on the catalogue
  generation through J1 (§9.3); requests never rebuild. **P2, P5:** no polling loops (store loops; the plugin serve loop
  blocks on its read and the host's liveness, §4.4).
- **Index and catalogue thread Settings** (§16): index reader threads, index writer threads, catalogue workers (Auto, within cores − 1 beside
  the parse threads). No tee memory share in 1.2: ADR 0062's trim (2026-11-11) deferred WP-4 to WP-8 (in the plan's
  terms IX-B2, P4's tee tally, IX-C, IX-S and IX-M); WP-1 (SR1, parallel readers after the import) and WP-2 and WP-3
  (IX-B1) stay [A: ADR 0062's status line and its mapping to the plan]. The thread Settings above are owned by SR1 and
  SR3 (SR3's index read threads Setting is built on `debrief/import-search`, plan §9.1), not by the deferred IX-S.
- **Y11:** first page of 10M events under 0.5 s (a bench gate); "sticky field promotion" was not found in the sources
  read [U]: its owner confirms the scope first. **Y4:** the search budget (`search.budget_ms`, a registry row), the index
  repair path as `MaintenanceAction::RepairIndex` (existing enum), the kept-profile quit and the two waits on the judge
  (§4.4). **Y15:** the Overview freeze measured, then fixed, with its block in `performance.md`.
- **Bench gates (B1 of plan §2, not ADR 0065's B1):** `apps/debrief/bench/budgets.toml` holds D7's budgets as rows
  `{ id, metric, unit, max, corpus }`; `paddock-bench check` fails on a planted 2× regression (X5's positive control).
- **4.3 corpus:** `cargo xtask corpus --shape <large10|bundle|planted> --out <tmp>`; invented data only (ER-14).
- **CPU-1, NB-1, D-0, 5.1, 5.2, D-b, SR1, SR3, SR4, SR5, IX-B1, Y3's rest:** no API change (IX-B2, IX-C, IX-S and IX-M are
  deferred with the tee); their results are rule 48 blocks; 5.2's deferral uses the import job's phases (§9.2).

### 14.6 Analyses at scale (FD-F, FD-K, FD-M)

FD-F is an ADR; this fixes the API it must fill, so that 1.3's builds have one shape:

```rust
// debrief-core::analysis::scale
pub enum AnalysisClass {
    /// Accumulated during the import (as ADR 0062 does for the catalogue), finished per query.
    ImportAccumulated,
    /// Computed in the store over all rows (counts, SQL).
    StoreSide,
    /// Over a sample, with the coverage as an `analysis.events` cut (ADR 0065).
    Sampled { limit: LimitId },
}
pub trait Analysis: Send + Sync + 'static {
    const KIND: &'static str;                    // "patterns", "exception_groups", "known_issues", ...
    const CLASS: AnalysisClass;
    type Request: Serialize + DeserializeOwned;
    type Result: Serialize;
    fn run(&self, req: &Self::Request, ctx: &AnalysisCtx) -> Result<AnalysisRun<Self::Result>, AnalysisError>;
}
pub struct AnalysisRunId(u64);
pub struct AnalysisRun<T> { pub id: AnalysisRunId, pub result: Bounded<T>, pub findings: Vec<AnyFinding> }   // §11
```

- Every analysis runs as an `Analysis` job (§9.1); the old result stays until the new one is complete.
- The query language's refusal over 2M matches (`qlang.rs:66-77`) becomes a `Bounded` cut or a store-side count, as
  FD-F decides per stage; known issues and playbook checks move to store-side counts.
- **FD-K:** the `Analysis run` log line (§12.2), emitted by `AnalysisCtx` for every run, so no analysis can skip it.
- **FD-M:** `cargo xtask corpus --shape planted --incidents <spec.toml>` writes the corpus and `expected-findings.toml`
  (`{ incident, analysis, finding_kind, window }`); `debrief-core/tests/it/recall.rs` (nightly) fails when a planted
  incident has no matching finding.

### 14.7 Correctness (C1 to C5)

| # | API change | Test home |
|---|---|---|
| C1 | UMDH reader on SI-11's `Lines` loop: `UmdhReader::read(input: Lines<'_>, sink: &mut dyn UmdhSink, limits: &UmdhLimits) -> Result<UmdhEnd, ImportError>`; aggregated entries counted with their `@` size; unindented frames kept; the same id after a restart not merged (uses `ProcessInstance`, §2.5) | `debrief-plugin-dumps/tests/umdh_reader.rs` (ADR 0069 tests 1, 2, 2b) |
| C2 | `TimestampParser::parse_offset` accepts `+h:mm`; dates with spaces in the template set; both through ADR 0066's corpus shapes | `debrief-plugin-ext/src/time/tests.rs`, `debrief-plugins/tests/it/time_corpus.rs` |
| C3 | `KnownIssuesRequest { query, window: Option<TimeRange> }` and the playbook run take the report's or view's window | `debrief-core/src/known_issues/tests.rs` |
| C4 | `debrief analyze --profile <name-or-path>` in `debrief-cli`; an unknown name fails with the list of profiles | `debrief-cli/tests/it/profile.rs` |
| C5 | `pub fn benjamini_hochberg(p: &[f64], q: f64) -> Vec<bool>` (pure) in `analysis/outliers.rs`, applied over the tests of one run; `q` is the constant `OUTLIER_FDR = 0.05` [J], a Setting only in 1.3 (analysis thresholds as Settings moved there) | `debrief-core/src/analysis/outliers/tests.rs`, 6,000 tests with no planted effect |

### 14.8 Memory (MEM-5, MEM-6 ADR, FD-G)

- **MEM-5:** the PDB cache by bytes: `SymbolCache::new(budget_bytes: u64)`, LRU by identity; Setting
  `performance.symbols.cache_mb` (§16); a limit row `symbols.cache_bytes`.
- **MEM-6 ADR, widened by FD-G:** decides file-backed models (Parquet for samples and stacks, a rebuildable
  memory-mapped cache for heap graphs) [mem] and a byte budget for **all** open documents (dumps, UMDH, profiles),
  replacing `MAX_DOCS = 32` (`debrief-dump/src/store.rs:13`). The API it must fill:

```rust
// debrief-core::documents (MEM-2's budget, built on `debrief/p0-idle` and landed by L-0 first (§8), generalised)
pub struct DocumentBudget { pub bytes: u64 }                      // Setting performance.documents.budget_mb
pub trait OpenDocuments: Send + Sync {
    fn admit(&self, key: DocumentKey, bytes: u64) -> Result<DocLease, LimitExceeded>;   // evicts LRU first
    fn touch(&self, key: &DocumentKey);
}
pub struct DocLease { /* frees on drop */ }
```

  The 8 to 12 day build stays the user's call after the ADR's numbers [mem].

### 14.9 Names, packs and the AI that stays (N1, N2, N3, N6, N8)

- **N3:** `PackKind::Names` on `paddock-packs` (hot reload, user `packs/`, a health line); built-in packs empty or a few
  dozen entries; schema `{ alias, canonical, how = "pack", evidence }`.
- **N1, N2, N6** change no API beyond removals (`schedule_alias_job`, the `name_aliases` setting) and the abbreviation
  rule (`How::Abbreviation`); N6's confirmation is a decision dialog in the registry.
- **N8:** docs only.

### 14.10 Family obligations and checks (X1 to X11, Y6, Y7, Y8, Y16, A1, Y14's R0 and R3)

Each has its API in its own ADR; this file only routes:

| Package | API owner | Debrief's side |
|---|---|---|
| X1 bundle privacy | `paddock-diag`'s `Redactor` | logs and child logs through it; screenshots off by default with a per-report checkbox |
| X2 per-machine install | ADR 0053 | guard, start check, notes |
| X3, X4 | ADR 0052 (wired, outcome, layout), ADR 0054 A-1 | adoption |
| X5 | rule 71 | a positive control per bench gate and VM row (§14.5) |
| X6 logging | the family logging ADR (F3) | §12's lines on its API |
| X7 waivers | `paddock.waivers.toml` | C36, C39, C53, C54 sites by 2026-12-19 |
| X8 copies and the driver | `docs/known-copies.toml`, the driver ADR (`paddock-chromium`) | every `delete_by = debrief-v1.2.0` copy deleted |
| X9, X10, X11a, X11b | release path, chain hygiene | no API |
| Y16 | ADR 0028 (`paddock-about`, `paddock-deps`) | About and notices |
| Y6 | new conformance check `ports-adapters` | `debrief-core` depends on no adapter crate (`debrief-store-duck`, Tauri, DuckDB); a Cargo-graph check with a seeded allow-list |
| Y7 | `paddock-e2e` `stabilize(page)` (fonts ready, animations off, frozen clock, fixed DPR) | Debrief's screenshot specs call it first |
| Y8 | Debrief's test architecture | `data-testid="<area>.<thing>"`, page objects in `app/e2e/pages/`, the flake list file |
| A1's checks | ADR 0024 (C40 to C42) | Debrief's instances listed, then fixed |
| Y14 R0 | the redesign's check: security S1 (restart arguments) and S2 (path grants), deadlocks, UI render cost [mem] | verification, no new API |
| Y14 R3 | "split" from the 2026-10 redesign notes; its scope was not found in the sources read [U] | its owner confirms before starting (§17, Q18) |

### 14.11 Rule 47 in Debrief's views (Y10)

Material first (rule 47) in Debrief's own views, start by 2026-12-01 (plan §10.4): the audit's 72 `<select>` become
`mat-select`, 63 checkboxes `mat-checkbox`, 19 radios `mat-radio-group`, 22 hand-rolled tabs `mat-tab-group`; all themed
through `--pd-*` tokens. No new API. A rule `paddock/material-first` in the **shared `eslint/` preset** (rule 42: rule 47 binds every app; warning for the
other apps until each has its own pass), an error for Debrief at Y10's
end) flags native `select`, `input[type=checkbox|radio]` and `role="tab"` outside Material; sub-packages start with
views that 2.7 and 4.5 do not rewrite. Tests: each touched view's spec on CDK component harnesses (rule 38); axe both
themes; new screenshots with an independent review (ER-23).

### 14.12 Room left for what moved out of 1.2

| Moved function | Builds on, unchanged |
|---|---|
| ADR 0069's UMDH series | C1's reader (§14.7), `ProcessInstance`, `CheckKind::HeapGrowth` |
| Y1 AI memory, Y2 semantic search, N4, N5, N0, N7 | the assistant contribution (§14.3); N3's pack (§14.9) |
| Y13 formats, PML, batch queries | the `Importer` trait (§3), `Seekable` for PML |
| Plugin analysis contributions | `debrief.analysis.v1` (§5.9) returning `AnyFinding` |

### 14.13 Restored to 1.2: investigation-folder features and ADR 0068's checks (revision 7)

**Status: designed (revision 7, 2026-11-12), replacing revision 6's stub.** On 2026-11-11 the user put back into 1.2
what plan §10.3 had moved out [mem: decisions 2026-11-11, "Scope answers"]; on 2026-11-12 the user chose Q-U4 (a):
design all of it in this file now (§17.2). The scope, from the user's decisions of 2026-11-07 [mem: investigation-folder
decisions; ADR 0068 decisions] and 2026-11-11:

- **Investigation-folder features:** the time shapes `+h:mm` and dates with spaces (C2, §14.7, widened here); citations
  as `path!/member:line` with the written and the zoned time, in reports, exports and AI; report blocks (hypotheses, open
  points, glossary, SVG figures, two audience versions, analysis results); the per-thread sequence view with stall gaps
  and a sequence-diagram export; Pitwall drafting of a report section by section within about 8k tokens from engine facts
  with checked citations (rule 46). Two small items from the same assessment that no plan row carried: a duplicate rule for
  a zip next to its own extraction, and the `ctime;` event-log text exports (ADR 0066's gap 4: `apps/debrief/docs/adr/
  0066-how-debrief-reads-times.md` line 38, corpus row `evt_ctime` line 417); **the user confirmed both on 2026-11-12**
  [user, 2026-11-12, relayed by the coordinator]; they are packages IF-5 and IF-6 here.
- **ADR 0068's checks other than WP-1:** WP-0, WP-2 to WP-18 and WP-RR, with .NET managed state (the approved ".NET dump
  stacks" are WP-0 and WP-12). ADR 0068 decides the behaviour [A]; this section gives each work package its signatures,
  homes, Settings, limits and test homes, on the foundation types, and the corrections the foundation forces.

**Rules for this section.** No second mechanism for anything §2 to §13 defines: evidence is `EvidenceRef` (§2.4), findings
are `Finding<V>` and `AnyFinding` (§11), "the same process" is `ProcessInstance` and `same_process` (§2.5), times outside
the event table are `StampedTime` (§10.3), every collection reply is `paddock_limits::Bounded<T>` with its rows in a
registry (§4.6), every long operation is a job ending in ADR 0052's `Outcome` (§9.6), child processes go through
`paddock_proc` (rule 53, §1.5), knowledge lists are `paddock_packs` packs (rule 21), the UI registers through the shell's
`define*` (§14.3), and Debrief's AI goes through `paddock_pitwall` (ADR 0070).

**Prerequisites that are not built** (ADR 0096; each checked on `origin/main` `e2a318345` and every remote branch,
2026-11-12, with `git ls-tree` and `git grep`):

| Prerequisite | State on 2026-11-12 | Needed by | Owner |
|---|---|---|---|
| ADR 0052 WP-B1: `NextStep`, `Why`, `Outcome` in `paddock-jobs` | no `enum NextStep` in `crates/paddock-jobs` on any branch | every job here; WP-18 adds variants to it | ADR 0052's package (§15, shared dependency) |
| ADR 0051's crate `paddock-tools` (pins, install on demand, Authenticode check) | no `crates/paddock-tools` on any branch; ADR 0051 (Accepted 2026-11-04, `origin/docs/paddock-tools-design-v2` `a614485c9`) prices the whole crate at 42 to 57 days, scheduled after the releases | WP-12 (the helper's pin, the DAC signature check) | **priced here as PT-1, the subset WP-12 needs** (§14.13.8; Q-U6 offers making .NET conditional instead) |
| ADR 0028's `paddock-deps` (notices for downloaded tools) | no `crates/paddock-deps` on any branch | WP-12's notices | Y16 (§14.10; ADR 0028's DP1 and DP4 are inside Y16's 15 days, wave 7), plus **PT-2**, ADR 0051's WP-12 (the "downloaded on request" class), priced here |
| MEM-2's `debrief_budget::MemoryBudget` and `Ticket` (not FD-G's `OpenDocuments`, which is unpriced and undecided) | on `origin/debrief/p0-idle` `a0fd29185` only (`ed8e6005a`), not on `origin/main` or `origin/debrief/1.2-int` | WP-RR (the held dump's ticket) | L-0 (§8) |
| FD-E2, FD-I, FD-B, FD-P, Z3, Z6 | 1.2 packages, not built | IF-1 to IF-4, WP-6, WP-9 | their rows (plan §10.1, ADR 0066) |
| `framehop` | an external crate (MIT or Apache-2.0), not a dependency yet | WP-3 | WP-3 adds it (licence allow-list as for every dependency) |

Built on `origin/main` and used as they are: `paddock-limits`, `paddock-proc` (`Supervised`, `MemoryCap`, `run_bounded`,
`ProcessId`), `paddock-pitwall` (`Link`, `ChatRequest`, `Client::count_tokens`, `CallOptions` with `CancelToken`,
`AiState`), `paddock-packs` (`PackKind`, `PackLoader`), and the shell's `defineView`, `defineCommand`, `defineMenuItem`,
`defineSettingsPage`, `defineNotificationKind` and `defineAssistantContext` (`ui/projects/paddock-ui`).

**Order inside 1.2.** None of this is on the streaming and folder-open chain (§9.3 of the plan); in §2 to §13 only §2.5
gains a field and two reasons for it (FD-P). It starts by capacity once its dependencies are in: the engine halves of ADR 0068 (WP-0, WP-1's
follow-ups, WP-2, WP-11) can start in wave 2; the dump UI (WP-13) waits for M3a (`defineView`) and the shell's job end
(ADR 0052 WP-B1); the sequence view's UI (IF-3) waits for M3b, as the roadmap note §2.3 says ("after M3b for its UI"):
its row-menu entry and palette provider are M3b's `defineMenuItem` and `definePaletteProvider` wiring of the grid, not
only `defineView`; IF-1 to IF-4 wait for FD-E2 and FD-I. §14.13.8 lists every package; plan §9.4 places them in waves.

#### 14.13.1 Time shapes and the assessment's two small items (C2 widened, IF-5, IF-6)

- **C2, widened (no new days; it stays 1.5).** `+h:mm` (a one-digit offset hour) is accepted in all three places where an
  offset is read today [mem: investigation-folder decisions]: the line regex of the shipped `generic.toml` pack, the
  numeric-date pattern (`numeric_date_re`) and `TimestampParser::parse_offset`; a date with spaces (`yyyy MM dd,`) gets a
  template entry. All three go through ADR 0066's `TimeReader` (§10.2) once Z1 lands, so C2 adds the shapes to Z1's corpus
  shapes (`debrief-plugins/tests/it/time_corpus.rs`) rather than to the old parser only. Test homes as §14.7.
- **IF-5, a zip next to its own extraction (ADR 0067's duplicate rules, F5).** A folder holding `x.zip` beside a folder
  `x/` whose files match the zip's members by relative name and declared size (from the central directory,
  `paddock_archive::Members::entries()`, SI-4, no inflation) is a duplicate set: the extracted folder stays checked and
  the zip is flagged. ADR 0067 keeps duplicates as rules in its pack (`[[duplicates.*]]`, "Duplicates are only flags"
  [A]), so this is one new rule kind, `[[duplicates.extracted-copy]]` (`scope = "sibling"`, `prefer = "folder"`,
  `min_match = 0.95` of members), and one variant appended to §6.3's `ReasonKey`, `ExtractedCopy { of: NodeId }`,
  computed in the scan's `duplicates` phase (§9.2) by a pure function `debrief_core::folder_scan::recommend::
  extracted_copy(zip: &[MemberEntry], dir: &ScanTable, at: NodeId) -> Option<f32>`, comparing at most
  `folder.scan_entries` names (no new limit). Test: ADR 0067's `folder_scan/recommend` tests gain an invented zip and its
  extraction, and the negative cases (a member missing past the threshold, a size differing, a renamed member).
- **IF-6, `ctime;` event-log text exports.** Lines of the shape `Mon Jun  1 10:00:00 2026;0x12;Info;Svc;msg` (ADR 0066
  corpus row `evt_ctime`) are read by a template entry in the shipped delimited-text pack (`importers/*.toml`, rule 21)
  with the `ctime` stamp format added to `TimestampParser` (`%a %b %e %H:%M:%S %Y`, a wall time, `WALL` set) and the
  `;` fields named by position. No new importer and no new trait: the core text importer (§3.2) with a pack entry. Tests:
  ADR 0066's corpus row turns from "gap 4" to read, in `debrief-plugins/tests/it/time_corpus.rs`; a sniff test that the
  shape wins over the generic CSV.

#### 14.13.2 Citations (IF-1)

A citation is how a person sees an `EvidenceRef` (§2.4): where it is, and when it happened, both as the file wrote it and
in the report's display zone. FD-E2 gives notes, report blocks and AI citations one `EvidenceRef` and its resolver; IF-1
gives them one rendering, used by reports, exports, the AI chat and drafting. One formatter, no per-surface copies
(rule 55).

```rust
// debrief-core::evidence::cite   (pure; ts-rs → core/generated/evidence.ts)
/// Where a citation points, in the form people quote: `logs/app.zip!/svc/app.log:1234`.
pub enum CiteLocation {
    /// A text source: its root-relative path (`RelPath`, archive members as `a.zip!/m`, §6.6) and 1-based line.
    Line { file: RelPath, line: u32 },
    /// A non-text source: path and the importer's declared ordinal (§2.2), shown as `#<n>` (an EVTX record number).
    Ordinal { file: RelPath, kind: OrdinalKind, n: u64 },
    /// A part of a document (§2.4's `DocItem`): `dumps/app.dmp#thread 1234 frame 3`.
    Document { file: RelPath, item: DocItem },
}
/// The time as the file wrote it: the wall clock read in the source's own zone, and how Debrief knows that zone.
pub struct WrittenTime { pub wall: CivilTime, pub offset: Option<OffsetMinutes>, pub how: WrittenHow }
pub enum WrittenHow {
    /// The line carried an offset (ADR 0066 `Written::Offset` or `OffsetZero`); shown with it.
    Offset,
    /// A wall time under the zone applied to its folder (ADR 0066 `source_time`); shown with the zone's name.
    ZoneApplied { zone: ZoneName },
    /// A wall time whose zone is not known (`AsWritten`): shown as written, marked "zone unknown".
    AsWritten,
}
/// The same instant in the report's display zone (§10.4: UTC, local or the logs' zone), with its confidence.
pub struct ZonedTime { pub ms: i64, pub display: TimeDisplay, pub confidence: Confidence }
pub struct Citation {
    pub reference: EvidenceRef,
    pub resolved: Resolved,                       // §2.4: Found, Moved or Gone (Gone keeps the excerpt)
    pub at: CiteLocation,
    pub written: Option<WrittenTime>,             // None for timeless evidence
    pub zoned: Option<ZonedTime>,
}
pub enum CiteStyle { Short, Full, Plain }         // Short: in text; Full: a footnote; Plain: an export column, no markup
pub fn cite(r: &EvidenceRef, resolved: &Resolved, zones: &dyn SourceZones, display: TimeDisplay) -> Citation;
pub fn format_citation(c: &Citation, style: CiteStyle, words: &dyn CiteWords) -> String;
// "logs/app.zip!/svc/app.log:1234 · 10:00:00.120 +9:00 as written · 01:00:00.120 UTC"
```

- **Written time, reconstructed honestly.** The store keeps `ts` (UTC ms) and the `ts_state` byte, not the offset each
  line carried (§5.3). `debrief-core::zone::resolve` gains `fn written_time(ts: i64, ts_state: u8, prov: &TimeProvenance,
  table: &OffsetTable) -> WrittenTime`: an event with `WALL` clear and a source whose provenance records one written
  offset is `Offset`; a `WALL` event under an applied zone is `ZoneApplied` with that zone's offset at the instant (the
  offset table ADR 0066 §10 already returns for the logs' display); `AsWritten` otherwise. Sources whose lines carry
  differing offsets give `Offset` with the provenance's offset at that instant and the citation says "offset from the
  source's provenance". `SourceZones` is the port the store's `Skew::source_time` (§7.1) already serves. [J]
- **Where citations appear.** Report export (HTML, Markdown, PDF): every events block row and every inline citation in
  text renders `Short` in place and `Full` as a numbered note at the end of its section. Event exports
  (`debrief-core/src/export.rs`, CSV and JSON): one column `citation` (`Plain`), on by default, off by an export option.
  AI chat and drafting (§14.13.5): links carry the `EvidenceRef`; the hover shows `Full`; a `Gone` citation shows the
  kept excerpt and the word "gone". Copy: a command "Copy citation" on a row (`defineCommand`, `debrief.evidence.copyCitation`)
  puts `Full` on the clipboard.
- **Redaction.** The location is a path, so the redacted copy and a redacted export run it through the redactor's path
  rules (`debrief-core/src/redact`), as `raw` is; the excerpt is already redacted (§2.4).
- **Commands.** `evidence_cite(refs: Vec<EvidenceRef>, display: TimeDisplay) -> Bounded<Vec<Citation>>`, at most
  `evidence.resolve_batch` (1,000, §2.4) per call: one row in `command_table!` and in `store_port_table!` only through
  `Inspect::resolve` (no new port). TypeScript: `core/generated/evidence.ts` gains `Citation`, `CiteLocation`,
  `WrittenTime`, `ZonedTime`; `core/format.ts` gains `formatCitation(c, style)` which only lays out the engine's fields
  (the engine computed them; §13.3).
- **Tests.** `debrief-core/src/evidence/cite/tests.rs` (pure: every `CiteLocation` and `WrittenHow`, a zip member path, a
  `Gone` citation, a DST fold under `ZoneApplied`, redaction of the path); `debrief-core/tests/it/cite_export.rs` (a CSV
  export has the column and a re-time moves the zoned time, not the written one); a spec
  `app/src/app/core/format.citation.spec.ts`; a Playwright spec that a report's note links back to the cited row
  (plan 0.2's citation regression).

#### 14.13.3 Report blocks and audiences (IF-2)

The report stays one versioned JSON document (`debrief-core/src/report/mod.rs` [S]: `Report`, `Section`, `StoredBlock`
keeping unknown blocks verbatim, `Block` tagged by `type`). IF-2 appends block kinds and an audience per block; the
engine renders every kind (HTML, Markdown, PDF); the UI only edits and lays out.

```rust
// debrief-core::report  (REPORT_VERSION 2; a newer block kind in an older Debrief stays `StoredBlock::Unknown`)
pub enum Block {
    Text { markdown: String, generated: Option<DraftProvenance> },   // inline citations as `[[cite:<n>]]` tokens; `generated`: IF-4
    Events { caption: String, refs: Vec<EvidenceRef> },     // FD-E2 replaces `ids: Vec<u32>`
    Chart { .. }, Timeline { .. }, Annotations { .. },       // today
    Table { caption: String, query: String, dialect: QueryDialect, columns: Vec<String>, rows: Vec<Vec<Value>> },
    Finding { finding: AnyFinding },                         // FD-I (§11) replaces the three strings
    // appended by IF-2:
    Hypothesis { statement: String, status: HypothesisStatus, evidence: Vec<BlockEvidence> },
    OpenPoints { items: Vec<OpenPoint> },
    Glossary { entries: Vec<GlossaryEntry> },
    Figure { caption: String, svg: SafeSvg, from: FigureSource },
    AnalysisResult { caption: String, run: AnalysisRunId, kind: Box<str>, snapshot: Bounded<Value>, findings: Vec<AnyFinding> },
}
pub enum QueryDialect { Ql, Sql }                            // the SQL result block (roadmap §2.3) is a Table with `Sql`
pub enum HypothesisStatus { Open, Supported, Refuted, CannotTell }
/// §11's `Evidence` with a person's note instead of an engine label key (same `EvidenceRef`, same `EvidenceRole`).
pub struct BlockEvidence { pub what: EvidenceRef, pub role: EvidenceRole, pub note: Option<String> }
pub struct OpenPoint { pub text: String, pub done: bool, pub evidence: Vec<BlockEvidence> }
pub struct GlossaryEntry { pub term: String, pub meaning: String }
pub enum FigureSource { Timeline { from: i64, to: i64 }, Chart { spec: ChartDef }, Sequence { request: SequenceRequest }, Imported }
/// SVG checked by the existing sanitiser (`debrief-core/src/svg.rs`) at the parse edge; never rendered unchecked.
pub struct SafeSvg(String);
/// Who drafted a text block (IF-4): a stored copy of `paddock_pitwall::Provenance`, which has no serde. Built in the
/// adapter from the reply's headers; bounded strings (64 chars each).
pub struct DraftProvenance { pub model: Option<Box<str>>, pub engine: Option<Box<str>>, pub chosen_by: Option<Box<str>>,
                             pub cached: bool, pub made_at_ms: i64 }
pub struct BlockItem { pub id: String, pub audience: AudienceSet, pub block: Block }   // `audience` defaults to both
pub enum Audience { Technical, Summary }
pub struct AudienceSet { pub technical: bool, pub summary: bool }

impl ResolvedReport {
    /// One audience's version: blocks not for it are left out; in `Summary`, evidence lists collapse to their
    /// citations, analysis snapshots to their findings, and terms used in the kept text that the glossary defines
    /// are listed at the end. Pure; the export calls it.
    pub fn for_audience(&self, a: Audience) -> ResolvedReport;
}
```

- **Limits.** Today's constants (`MAX_SECTIONS` 100, `MAX_BLOCKS` 200, `MAX_EVENT_IDS` 1,000, `MAX_TABLE_ROWS` 1,000,
  `MAX_TEXT` 256 KiB [S]) become registry rows (ADR 0065 part 3; §4.6) in `debrief-core/src/limits.rs`, `hard`:
  `report.sections`, `report.blocks`, `report.evidence` (per block, 1,000), `report.table_rows`, `report.text_bytes`, and
  new `report.glossary_entries` (500), `report.open_points` (500), `report.figure_bytes` (2 MiB of SVG, the sanitiser's
  input cap). `AnalysisResult.snapshot` is cut by the analysis's own `CutRow` (its `Bounded` as returned, §14.6).
- **Engine operations.** `report_add_analysis(run: AnalysisRunId, section: SectionId) -> BlockItem` (snapshot of a stored
  run, Q-2's cache, §14.5); `report_add_figure(source: FigureSource, svg: Option<String>) -> BlockItem`. **Timeline and chart
  figures stay UI-rendered and engine-sanitised:** the UI's existing serialisers make the SVG, as today [S: `Block::Chart`
  and `Timeline` hold "the SVG the UI rendered"], and the engine accepts it only as `SafeSvg` (the sanitiser at the parse
  edge). Chosen over porting the TypeScript serialisers to Rust (about 2 to 3 days [J], not priced) because the figure is
  then exactly what the person saw, the sanitiser is the security boundary either way, and no second renderer of the
  timeline exists to keep equal to the first (rule 55). Only the sequence figure is engine-rendered (§14.13.4's
  `render_sequence`, priced in IF-3), because its export needs the same drawing without a view open; `report_glossary_suggest(report) ->
  Bounded<Vec<GlossaryEntry>>` (terms from the report's findings' label keys and the names pack, N3, no AI); export takes
  `audience: Audience` (`share::export_report`'s request gains it, one field).
- **UI.** `app/src/app/share/report/blocks/` gains one editor component per kind (`hypothesis-block.ts`,
  `open-points-block.ts`, `glossary-block.ts`, `figure-block.ts`, `analysis-block.ts`), Material first (rule 47); the
  report menu gains "Add hypothesis", "Add open points", "Add glossary", "Add figure"; an "Audience" toggle per block;
  the export dialog (`report-export-dialog.ts`) a "Version: Technical / Summary" choice. `ShareBackend.exportReport`'s
  request gains `audience`; `ReportBackend` (new, split from `ShareBackend` by interface segregation) has
  `addAnalysis`, `addFigure`, `suggestGlossary`.
- **Tests.** `debrief-core/src/report/tests.rs`: each kind round-trips; a newer kind stays verbatim in an older reader
  (today's `StoredBlock::Unknown` test extended); `for_audience` property tests (Summary never contains a
  Technical-only block; every citation of a kept block survives); the sanitiser refuses script and external references in
  a `Figure`; each limit's `limit_case!`. Export goldens in `debrief-core/tests/it/report_export.rs` (HTML and Markdown
  per audience). UI: one spec per editor on CDK harnesses (rule 38), the export dialog's Playwright spec.

#### 14.13.4 Thread/sequence view (IF-3)

What happened on each thread of a process, in order, where it stalled, and a sequence diagram of it. Built on the order
key `(ts, source, ordinal, id)` (§2.2), `ProcessInstance` (§2.5), the lane canvas and Lanes picker (§14.2) and keyset
paging (§14.5). A thread id means something only inside one process instance, so the lane key is both.

```rust
// debrief-core::sequence   (pure core + one store query; ts-rs → core/generated/sequence.ts)
/// Which fields name a process and a thread, per source format. Data, not code (rule 21): `[sequence]` in the shipped
/// profile (`default_profile.toml`) and user profiles: `process_fields = ["pid", "ProcessId", "ProcessID"]`,
/// `thread_fields = ["tid", "thread", "ThreadId", "ThreadID", "thread_id"]`, first match wins, at most 16 names each.
pub struct ThreadRoles { pub process: Vec<FieldName>, pub thread: Vec<FieldName> }
/// A lane: one thread of one process. `instance` is §2.5's `ProcessInstance` (`pid` from the process field, `started:
/// None` unless the source records a start time, `run` from FD-P's `split_runs`); two lanes are one process only when
/// `same_process` says `Same`, and the lane shows an `Unconfirmed` reason when it says so. No second identity type.
pub struct ThreadKey { pub instance: ProcessInstance, pub source: SourceId, pub thread: Box<str> }
pub struct ThreadSummary { pub key: ThreadKey, pub events: u64, pub first: i64, pub last: i64, pub stalls: u32, pub pairing: Option<UnconfirmedWhy> }

pub struct SequenceRequest {
    pub query: QueryKey,                        // the view's filter (§14.5)
    pub threads: ThreadPick,                    // TopByEvents(n) | Keys(Vec<ThreadKey>) | ForEvent(EventId)
    pub range: Option<TimeRange>,
    pub anchor: PageAnchor,                     // §14.5's keyset paging, `before + after ≤ 512`
}
pub struct SequenceReply {
    pub lanes: Bounded<Vec<ThreadSummary>>,     // cut by `timeline.lanes_max` (§14.2), TopBy events
    pub rows: Bounded<Vec<SeqRow>>,             // events of the picked lanes in order, one page
    pub gaps: Bounded<Vec<StallGap>>,           // in the page's span; TopBy duration
    pub generation: u64,
}
pub struct SeqRow { pub id: EventId, pub lane: u16, pub ts: Option<i64>, pub level: Level, pub text: Excerpt, pub link: Option<Handoff> }
/// A silence on one thread longer than both the stall floor and `GAP_FACTOR` × that thread's median interval.
pub struct StallGap { pub lane: u16, pub from: EventId, pub to: EventId, pub ms: u64, pub median_ms: u64 }
/// An edge to another lane: the same correlation value (a trace, request or activity id, from the profile's existing
/// correlation fields of `analysis/trace.rs`) appearing next on another thread. Never inferred from time alone.
pub struct Handoff { pub to_lane: u16, pub to: EventId, pub by: FieldName }
pub const GAP_FACTOR: u32 = 20;                  // a heuristic constant with a test on each side of it (§1.7)

pub fn thread_key(record: &RecordView, roles: &ThreadRoles, source: SourceId) -> Option<ThreadKey>;   // pure
pub fn stall_gaps(lane_times: &[(EventId, i64)], floor_ms: u64) -> Vec<StallGap>;                      // pure

/// The export: Mermaid and PlantUML text (lifelines = lanes, an event = a note on its lifeline, a handoff = an arrow,
/// a gap = a delay), and SVG drawn by the engine (`debrief-core/src/svg.rs`) for report figures.
pub enum SequenceFormat { Mermaid, PlantUml, Svg }
pub struct SequenceExport { pub format: SequenceFormat, pub text: String, pub cut: Option<Cut> }
pub fn render_sequence(rows: &[SeqRow], lanes: &[ThreadSummary], gaps: &[StallGap], f: SequenceFormat, cap: RowLimit) -> SequenceExport;
```

- **Store side.** One port row: `Query::sequence(req: &SequenceRequest) -> SequenceReply` in `store_port_table!`, a
  `WIRE_VERSION` bump in IF-3's commit (§7.1). The store reads the role fields through the catalogue's field index and
  pages in the order key; it never loads a thread whole. Stall gaps need each lane's median interval: computed over the
  page window plus a bounded look-back (`sequence.median_window`, 10,000 events per lane, `hard`), stated in the reply's
  `gaps` cut when the window was short.
- **Commands.** `sequence_threads(query) -> Bounded<Vec<ThreadSummary>>`, `sequence_page(req) -> SequenceReply` (both
  Query, `Analysis` queue when over the exact-position threshold), `sequence_export(req, format) -> SequenceExport`
  (at most `sequence.export_events`, 2,000 events, `hard`, a `Cut` beyond; Mermaid's own renderer slows on large
  diagrams, so the cap is about readability, not memory [J]).
- **Settings.** `sequence.stall_floor_ms` (§16): the floor below which no silence is a stall, default 1,000 ms.
- **UI.** `app/src/app/sequence/`: `SequenceBackend` (abstract; Tauri and mock), `sequence.store.ts` (SignalStore),
  `sequence-view.ts` on the shared lane canvas (2.4) with one lane per thread, rows as ticks, gaps shaded and labelled
  with their length and "×N the usual interval", handoffs as arrows; registered with `defineView({ id: 'debrief.sequence'
  })`; commands `debrief.sequence.openForEvent` (grid row menu, `defineMenuItem`) and `debrief.sequence.export`
  (Mermaid, PlantUML, SVG through the shell's save dialog); "Add to report" makes a `Figure { from: Sequence }` (IF-2).
- **Tests.** `debrief-core/src/sequence/tests.rs` (roles from the pack, a pid reused across a restart marker gives two
  lanes through `same_process`'s `RunDiffers`, `stall_gaps` on each side of `GAP_FACTOR` and the floor, renderer goldens for Mermaid and PlantUML, the
  export cut); `debrief-store-duck/tests/sequence.rs` (paging over 1M invented events of 50 threads keeps order and reads
  at most k × rows shown, Q-4's counter); `app/src/app/sequence/*.spec.ts` against the mock; one Playwright spec.

#### 14.13.5 Pitwall drafting (IF-4)

Pitwall drafts a report section from engine facts; the engine checks every citation the draft makes; the person accepts
or discards each section. One request per section within the section budget (rule 46: no LLM over the large job; the
model words facts the engine computed and never decides a verdict, as ADR 0068 §15 says of dumps). It replaces
`incident_narrative` (`app/src-tauri/src/investigate/incident.rs`, `debrief-ai`'s `assistant/incident.rs` [S]): the
narrative becomes "draft the Summary section", one mechanism (rule 55).

```rust
// debrief-core::report::draft   (pure: no I/O, no clock, no model)
/// The facts a section may use, each with a short handle the model cites as `[F12]`.
pub struct FactBundle { pub facts: Bounded<Vec<Fact>>, pub glossary: Vec<GlossaryEntry>, pub audience: Audience }
pub struct Fact { pub handle: FactHandle, pub kind: FactKind, pub text: String, pub cites: Vec<EvidenceRef> }
pub struct FactHandle(u16);                                   // "F1".."F<n>", unique within one bundle
pub enum FactKind { Finding(Producer), Count, Event, Hypothesis, OpenPoint, TimeNote }
/// Builds the bundle from the section's blocks and the report's findings, ranked (findings by confidence, then evidence
/// count), redacted when the report is. Pure: no counting here.
pub fn build_bundle(section: &ResolvedSection, report: &ResolvedReport) -> FactBundle;
/// Fits the assembled prompt to `budget`: **one** count of the whole prompt; if over, facts are dropped from the lowest
/// rank by their byte share of the prompt (the measured tokens-per-byte of that count) and the prompt is counted once more;
/// a second miss drops by the second ratio without a third count. At most two counts per section; the cut is a `Cut` on
/// `ai.draft.facts`. Checks `cancel` before each count.
pub fn fit_bundle(bundle: FactBundle, budget: u32, count: &dyn TokenCount, cancel: &CancelCheck) -> Result<FactBundle, DraftError>;
pub fn draft_prompt(bundle: &FactBundle, instructions: Option<&str>) -> Vec<paddock_pitwall::Message>;
/// Every sentence of the reply: kept with its verified handles, or marked. Unknown handles never become links.
pub fn check_draft(reply: &str, bundle: &FactBundle) -> CheckedDraft;
pub struct CheckedDraft { pub markdown: String, pub sentences: u32, pub cited: u32, pub uncited: u32, pub unknown: Vec<Box<str>> }
/// Adapter over `paddock_pitwall::Client::count_tokens` (`POST /x/v1/tokens/count`); one call per assembled prompt.
pub trait TokenCount { fn tokens(&self, text: &str) -> Result<u32, DraftError>; }
```

- **The call (adapter, `app/src-tauri/src/investigate/draft.rs`).** A job of kind `Ai` (§9.1), role Live, phases
  `facts` (blocks read), `drafting` (sections), `checking`; one `paddock_pitwall::ChatRequest` per section through the
  window's `Link::chat_with` with `CallOptions::default().with_priority(Priority::Interactive).with_cancel(token)`, where
  `token` is the job's cancel token bridged to `paddock_pitwall::CancelToken`, also passed to `fit_bundle`; Cancel stops
  between counts and sections and aborts the stream. Pitwall missing or stopped: the job does not start; the UI shows the shared `<pd-needs-ai>` from
  `AiState` (ADR 0070). The draft is stored as a **proposal** beside the report (`DraftProposal { section, blocks:
  Vec<BlockItem>, check: CheckedDraft, provenance: DraftProvenance }`, all serde, IF-2's stored type), never written into
  the report until the person accepts it; an accepted text block keeps `generated: Some(DraftProvenance)` and a
  "drafted" mark in the editor and the export.
- **Citations in the draft** become IF-1 citations (`[[cite:n]]` tokens into the block's evidence); an uncited sentence is
  kept with a "not backed by a fact" mark in the editor; a sentence citing only unknown handles is shown struck through
  with "cites a fact that was not given" and is left out on accept unless the person keeps it.
- **Budget.** `ai.draft.section_tokens` (§16), default 8,000, the prompt plus the expected answer; `build_bundle` cuts
  facts by rank to fit and reports the cut (`ai.draft.facts`, 200 facts, `hard`, a `CutRow<TopBy>`).
- **Job end (§9.6).** Read back through the stored proposal: `Confirmed` when every requested section has a proposal;
  `Unconfirmed { why: SectionsFailed { n } }` when Pitwall failed some (`NextStep::Retry`); `Failed` when none.
- **Commands.** `report_draft(req: DraftRequest { report: ReportId, sections: Vec<SectionId>, audience, instructions:
  Option<String> }) -> JobId` (instructions at most 2,000 characters), `report_draft_proposals(report) ->
  Bounded<Vec<DraftProposal>>`, `report_draft_accept(report, section, keep_marked: bool) -> Report`,
  `report_draft_discard(report, section)`. UI: `investigate/incident/` becomes `share/report/draft/` with
  `DraftBackend` (abstract; Tauri and mock: the mock returns fixed drafts and computes no facts); "Draft with Pitwall" on a
  section and on the report; the assistant context (`defineAssistantContext`) offers the same facts to Pitwall's chat.
- **Tests.** `debrief-core/src/report/draft/tests.rs` (bundle ranking and cut against a fake counter; `check_draft` on
  every sentence class; handles never collide; a redacted report's bundle holds no unredacted text);
  `app/src-tauri/src/investigate/draft/tests.rs` against `paddock_pitwall::testing`'s fake Pitwall (no real Pitwall, its
  port or the credential store, as that crate requires): a section drafted, Cancel between sections, Pitwall gone mid-job
  gives `Unconfirmed`; a spec that accept writes the block and discard leaves the report unchanged.

#### 14.13.6 ADR 0068's checks: where each work package's API lives

ADR 0068 §10 shows `CheckReport` with its own `verdict`, `confidence`, `evidence`, `not_checked` and `left_out`. §11 builds
`CheckReport` on the shared `Finding<Verdict>` (WP-1, in 1.2 already), so every work package below fills
`finding: Finding<Verdict>` rather than five parallel fields, `left_out` is `Vec<Cut>` (§17 Q4), `pairing` is §2.5's
`Pairing`, `CheckedDoc.process` is `ProcessInstance`, times are `StampedTime`, and evidence is `EvidenceRef` with
`EvidenceTarget::Document { doc: DocumentKey, item: DocItem }` (§2.4: `Thread`, `Module`, `Region`, `Handle`,
`ManagedType` already listed). The table names the home of each API; the blocks below give the signatures ADR 0068 left
as prose.

| WP | API home | New public surface |
|---|---|---|
| WP-2 reader | `debrief-plugin-dumps/src/dump_reader.rs`, `synth.rs`; model in `debrief-dump/src/model.rs` | `DumpCapabilities` (ADR 0068 §10), `MemoryIndex`, `ThreadTimes`, `HandleObject`, `RegionInfo`, `UnloadedRange`, `ProcessTimes` in the model |
| WP-RR, WP-8 | `debrief-plugin-ext/src/wire/dumpquery.rs` (§5.9's reserved namespace); `app/src-tauri/src/dumps.rs` (held handle), `paths.rs` (re-grant) | `debrief.dumpquery.v1` below; `HeldDocument`; WP-8 removes `WholeReason::DumpWithoutMemoryList` (§3.3, §17 Q3) |
| WP-3 unwinder | `debrief-plugin-dumps/src/unwind.rs` (new), `pdb_symbols.rs`, `debrief-symbols` | `UnwindProvider: minidump_unwind::SymbolProvider` (its `walk_frame`), `FrameTrust` |
| WP-4 to WP-7 | `debrief-dump/src/checks/{crash,hang,series,memory}.rs` | `fn check_*(docs: &[&DumpDoc], ctx: &CheckCtx) -> CheckReport`, pure over the model plus planned range reads |
| WP-9 | `debrief-dump/src/checks/around.rs` (plan), `debrief-core` (query) | `EventsAround` below |
| WP-10 | `app/src-tauri/src/dumps.rs`, `command_table!` | the check job, its cache, four commands |
| WP-11 | `debrief-plugin-dumps/src/capture.rs`, `plugin.rs` | `CaptureSeries`, the privacy option |
| WP-0, WP-12 | `apps/debrief/tools/debrief-clr-inspect/` (C#); plugin crate `debrief-plugin-dumps-dotnet` (new); `debrief-dump/src/managed.rs` | `debrief.managed.v1` below; schema 1 as Rust types |
| WP-13 | `app/src/app/dumps/checks/` | `DumpsBackend` additions below |
| WP-16 | `debrief-dump/src/packs/injectors.rs`; pack files under `packs/` | `InjectorPack: paddock_packs::PackKind` |
| WP-17 | `debrief-plugin-windows/src/minidump_import.rs` | none: the importer calls `debrief-dump`'s reader; its parser is deleted |
| WP-18 | `crates/paddock-jobs` (integrator) | `NextStep::{TakeDump, AddSymbolFolder, TurnOnSymbolServers, AddDacFolder, OpenAgain, RecordUmdh}` appended to ADR 0052's enum |

**Range reads after open (WP-RR), the namespace §5.9 reserved.**

```rust
// debrief-plugin-ext::wire::dumpquery   (postcard; round trip and fuzz target as every namespace, §5)
pub struct DumpQuery;
impl Contract for DumpQuery { const NS: &'static str = "debrief.dumpquery.v1"; const MINOR: u16 = 0;
    type Request = DumpQueryRequest; type Reply = DumpQueryReply; }          // unary; role Live (a person waits on a check)
pub enum DumpQueryRequest {
    /// Decode the named ranges of the lent dump. `index` is the slice of the document's memory index the ranges need, so
    /// a restarted plugin answers the same (ADR 0068 §7: stateless for correctness).
    Read { file: HostFileRef, doc: DocKeyWire, index: Vec<IndexEntry>, ranges: Vec<VaRange>, kind: DecodeKind },
}
pub struct IndexEntry { pub va: u64, pub len: u64, pub file_offset: u64 }   // 24 bytes, ADR 0068 §7
pub struct VaRange { pub va: u64, pub len: u32 }
pub enum DecodeKind { CriticalSection, Peb, Teb, UnwindTables { module: u32 }, Exports { module: u32 }, Raw }
pub enum DumpQueryReply { Pieces { pieces: Vec<Piece>, missing: Vec<(VaRange, MissingWire)> } }
pub enum Piece { CriticalSection { va: u64, owner_tid: Option<u32>, recursion: i32, lock_count: i32 },
                 Teb { va: u64, tid: u32, stack_base: u64, stack_limit: u64 }, Peb { va: u64, being_debugged: bool },
                 Unwind { module: u32, pdata: Vec<u8>, xdata: Vec<(u32, Vec<u8>)> }, Exports { module: u32, names: Vec<(u32, Box<str>)> },
                 Raw { va: u64, bytes: Vec<u8> } }
pub enum MissingWire { MemoryNotInDump, OverBudget, Unreadable }
```

- **The file.** The host keeps the handle it opened for `dumps_open` for the document's lifetime (rule 53), opened with
  share-read and share-delete (ADR 0068 Consequences (4)):

  ```rust
  // app/src-tauri/src/dumps.rs
  pub struct HeldDocument {
      file: File,                          // held, never re-opened by path while the document is open
      identity: FileIdentity,              // ADR 0068 §1's cache identity, checked before every lend
      header_hash: u64,
      ticket: debrief_budget::Ticket,      // MEM-2 as built (`origin/debrief/p0-idle`, `debrief-budget/src/lib.rs`)
  }
  ```

  **How it counts against the budget.** MEM-2's `MemoryBudget::admit(kind, name, bytes, release) -> Result<Ticket,
  OverBudget>` [S: `debrief-budget/src/lib.rs:199`, `Ticket` `:232`] already admits the dump's decoded model (kind
  `dump`, the model's estimated bytes, including the memory index of ADR 0068 §12); WP-RR keeps that one ticket in
  `HeldDocument` instead of beside it. The held handle itself adds **no bytes**: it is a kernel object, not process
  memory, and the plugin's parsed-prefix LRU counts against the reader process's own cap (ADR 0068 §7, §12), not the
  app's budget. When the budget releases the dump (its `Release` callback drops the model), the handle stays open, so the
  next use re-reads from the same held file with `Ticket::readmit` (rule 53: hold the name, never re-open by path); the
  handle closes only with the document. The number of held handles is bounded by the open dumps, a registry row
  `dumps.held_documents` (32, today's `MAX_DOCS`, `hard`), so nothing here depends on FD-G or the MEM-6 ADR, which may later
  replace the count with bytes. An `OverBudget` on open is the open's `LimitExceeded` with MEM-2's words and Settings path.
  For each query the host lends the handle by ranges through `HostFiles::lend_ranges` (§4.2) for the call only. A restored session re-opens by path only under a re-confirmed grant
  (`paths.rs`) and re-verifies `FileIdentity` and the header hash; a mismatch is `MissingReason::FileChanged`.
- **Limits** (registry rows in `debrief-dump/src/limits.rs`, `hard`, each with `limit_case!`): `dumps.query_ranges`
  4,096 and `dumps.query_bytes` 16 MiB per query; `dumps.check_queries` 64 and `dumps.check_bytes` 256 MiB per check;
  `dumps.unwind_table_bytes` 16 MiB per module; `dumps.memory_index_entries` 4 Mi; `dumps.prefix_cache_docs` 2. `Raw` is
  capped at 4 KiB per piece and refused outside `cfg(test)` builds' callers by the host.
- **WP-8.** `source.rs`'s prefix-plus-ranges path covers the 32-bit `MemoryListStream`; the 1 GiB refusal and
  `WholeReason::DumpWithoutMemoryList` go in WP-8's change with ADR 0064's amendment (§17 Q3).

**The unwinder (WP-3).**

```rust
// debrief-plugin-dumps::unwind
/// The CFI seam of `minidump-unwind` (`SymbolProvider::walk_frame`), fed from bytes: image pages read by range,
/// binaries from the symbol folders by code identity (`<name>/<TIMESTAMP><SIZE>/<name>`), the host's fetch when symbol
/// servers are on, dynamic function tables (stream 13). Never opens a path recorded in the dump.
pub struct UnwindProvider { modules: UnwindCache /* parsed once per module identity, bounded by `symbols.cache_bytes` */ }
impl minidump_unwind::SymbolProvider for UnwindProvider { /* walk_frame over framehop; fill_symbol from PDBs as today */ }
pub enum FrameTrust { Context, Cfi, FramePointer, Scan }      // every frame says how it was found; Scan is "candidate"
pub fn rewalk(doc: &mut DumpDoc, improvable: &[u32], provider: &UnwindProvider, threads: usize, cancel: &CancelCheck) -> RewalkReport;
```

- `framehop` is pinned and wrapped (ADR 0068 Consequences (2)): only `unwind.rs` names it. A re-walk runs threads in
  parallel (`performance.dumps.walk_threads`, Auto = cores − 1, §16), results in thread order (deterministic). Test:
  synthetic x64 frames with `.pdata`/`.xdata` in `synth.rs`; a dump whose module path points at a planted file must not
  read it.

**The checks (WP-4 to WP-7) and the events around (WP-9).**

```rust
// debrief-dump::checks
pub struct CheckCtx<'a> { pub limits: &'a Limits, pub reads: &'a dyn RangeReads, pub managed: Option<&'a ManagedState>,
                          pub symbols_generation: u64, pub cancel: &'a CancelCheck, pub phase: &'a dyn PhaseSink }
/// Port the shell implements over `debrief.dumpquery.v1`; the checks plan reads and interpret the typed pieces.
pub trait RangeReads { fn read(&self, doc: DocId, ranges: &[VaRange], kind: DecodeKind) -> Result<ReadOutcome, ReadFault>; }
pub fn check_crash(docs: &[&DumpDoc], ctx: &CheckCtx) -> CheckReport;   // WP-4
pub fn check_hang(docs: &[&DumpDoc], ctx: &CheckCtx) -> CheckReport;    // WP-5, WP-6 (series)
pub fn check_memory(docs: &[&DumpDoc], ctx: &CheckCtx) -> CheckReport;  // WP-7
pub fn pair_candidates(doc: &DumpDoc, others: &[&DumpDoc]) -> PairCandidates;   // WP-6: same process / same program / other
pub struct PairCandidates { pub same_process: Vec<(DocId, Pairing)>, pub same_program: Vec<DocId>, pub other: Vec<DocId> }
/// The cache key of ADR 0068 §1: document identities, kind, ordered set, symbol generation, the helper's pin.
pub struct CheckKey(u128);
pub fn check_key(kind: CheckKind, docs: &[&DumpDoc], symbols_generation: u64, helper_pin: Option<&str>) -> CheckKey;

// debrief-core::dumps_around  (WP-9: a store query, bounded like every reply)
pub struct EventsAround { pub at: StampedTime, pub window: TimeRange, pub same_machine: bool,
                          pub counts_by_level: Vec<(Level, u64)>, pub first: Bounded<Vec<AroundRow>>, pub query: QueryKey }
pub struct AroundRow { pub reference: EvidenceRef, pub level: Level, pub excerpt: Excerpt }
pub fn events_around(store: &dyn Query, at: &StampedTime, window: AroundWindow, machine: Option<&MachineKey>) -> Result<EventsAround, QueryError>;
```

- Verdict enums (`CrashVerdict`, `HangVerdict`, `MemoryVerdict`, ADR 0068 §2 to §4) each implement §11's `VerdictKind`
  (`CannotTell` present, a compile-time test per producer). `ThreadClass` replaces `api::ThreadState` in WP-5 (ADR 0068
  §1), no shim. `GrewOnce` and `NeedsMore { of, have, need }` are the shared words of ADR 0068 D13 (`NeedsMoreOf` is
  §11's).
- The window is two Settings, `dumps.around_before_s` and `dumps.around_after_s` (§16), with ADR 0068 §6's defaults;
  the first rows are a `CutRow<TopBy>` (`dumps.around_rows`, 50, `hard`).

**The shell (WP-10) and capture (WP-11).**

```rust
// app/src-tauri/src/dumps.rs (commands; one line each in `command_table!`)
#[tauri::command] async fn dumps_check(req: CheckRequest) -> CmdResult<CheckStarted>;          // { job: JobId } or { cached: CheckKey }
#[tauri::command] async fn dumps_check_result(key: CheckKey) -> CmdResult<Option<CheckReport>>;
#[tauri::command] async fn dumps_pair_candidates(doc: DocId) -> CmdResult<PairCandidates>;
#[tauri::command] async fn dumps_events_around(doc: DocId) -> CmdResult<EventsAround>;
pub struct CheckRequest { pub kind: CheckKind, pub docs: Vec<DocId> }   // 1..=`dumps.check_docs` (16, hard); the engine orders them

// debrief-plugin-dumps::capture (WP-11); the existing `create` grows, no new tool
pub struct CaptureSeries { pub pid: u32, pub kind: DumpKind, pub folder: FolderGrant, pub count: u8 /* 1..=5 */, pub interval_s: u16 /* 1..=600 */,
                           pub include_process_details: bool /* Heap and Full only; refused for Minidump (D9) */ }
```

- A check is an `Analysis` job (§9.1) with ADR 0068 §11's phases; phase ids (§9.2) `thread-details`, `reading-memory`,
  `walking`, `lock-owners`, `managed`, `comparing`, `events-around`. Its end (§9.6): read back through
  `dumps_check_result(key)`; `Confirmed` when the cached report is there; `Unconfirmed` is never used for a weak dump
  (that is the report's `CannotTell` with its Not checked list, ADR 0068 §1); `Failed` when the reader failed.
- A series capture is a `Capture` job (§9.1) holding the process handle across the series through `paddock_proc`
  (rule 53), phases `capturing` (dumps, count of `count`) and `opening`; read back: the files exist with the expected
  names and open as one group.

**.NET managed state (WP-0, WP-12).**

```rust
// debrief-plugin-ext::wire::managed   (unary; role Live; the reply crosses as JSON: schema 1 grows, §1.4)
pub struct Managed;
impl Contract for Managed { const NS: &'static str = "debrief.managed.v1"; const MINOR: u16 = 0;
    type Request = ManagedRequest; type Reply = Json; }   // Json of debrief_dump::managed::ManagedState (schema 1)
pub enum ManagedRequest {
    /// The host grants the dump's canonical path for this call only, after checking the held handle's identity
    /// (ADR 0068 §9); `dac` is a path the host chose and signature-checked; `sections` and `budget` bound the run.
    Inspect { dump: ChosenPath, dac: ChosenPath, sections: Vec<ManagedSection>, budget: ManagedBudget },
}
pub enum ManagedSection { Threads, Exceptions, Stacks, HeapStats, SyncBlocks, ThreadPool, Finalizer }
pub struct ManagedBudget { pub heap_objects: u64, pub types: u32, pub frames: u16, pub text: u32, pub wall_ms: u64 }

// debrief-dump::managed: schema 1 as Rust types, the JSON Schema generated from them (WP-12's check against the helper)
pub struct ManagedState { pub schema: u32, pub runtime: RuntimeId, pub sections: Vec<SectionResult> }
pub enum SectionStatus { Ok, Failed { reason: Box<str> }, Cut { limit: LimitId } }

// app/src-tauri/src/dumps/dac.rs
pub enum DacSource { UserFolder, LocalRuntime, SymbolServer }
pub fn find_dac(runtime: &RuntimeId, folders: &[PathBuf], servers_on: bool) -> Result<(PathBuf, DacSource), DacUnavailable>;
```

- **The helper run.** The plugin `net.decryptology.dumps.dotnet` (`RunPrograms`, `ReadFolders`, no network, ADR 0068 D2)
  starts the pinned helper through `paddock_proc::Supervised` with a memory cap (the job object on Windows) of
  `performance.dumps.dotnet_memory_mb`, reads stdout bounded by `dumps.dotnet_output_bytes` (64 MiB, `hard`), and ends it
  by held handle on Cancel (rule 53). At most `performance.dumps.dotnet_helpers` runs at once (§16). The memory type is
  §4.5's `MemoryPolicy` once 2.2 lands (§17 Q16), not a second cap type.
- **The pin and the signature check** are `paddock-tools`' (ADR 0051 and its `ci-artifact` amendment in WP-12); the
  signer policy's roots are a `paddock_packs` pack (`packs/microsoft-code-signing-roots.toml`, rule 21). Neither is
  built (prerequisites table above); WP-12 does not start its plugin half before `paddock-tools` exists.
- **Bounds inside the helper** are ADR 0068 §9's; the outer ones are registry rows (`dumps.dotnet_types` 10,000,
  `dumps.dotnet_frames` 256, `hard`; `dumps.dotnet_heap_objects`, a Setting, default 200 million).
- **WP-0 exit criteria** are ADR 0068 §9's, unchanged; WP-0 records its numbers under rule 48 before WP-12 starts.

**The UI (WP-13).**

```ts
// app/src/app/dumps/dumps-backend.ts (additions)
abstract check(request: CheckRequest): Promise<CheckStarted>;
abstract checkResult(key: CheckKey): Promise<CheckReport | null>;
abstract pairCandidates(doc: number): Promise<PairCandidates>;
abstract eventsAround(doc: number): Promise<EventsAround>;
// app/src/app/dumps/checks/: checks-strip.ts (three mat-buttons, the primary from `suggested_check`), check-report-view.ts
// (verdict, confidence reasons, evidence links into Threads/Modules/regions, Not checked with each NextStep, Around,
// cuts through <pd-limit-note>), pair-picker.ts, capability-line.ts. The capture dialog gains series and the privacy option.
```

- The report view's "Add to report" makes a `Finding { finding: AnyFinding }` block (IF-2), so a check reaches a report
  through the same block as every other finding; "Explain with AI" is IF-4's drafting over that one finding's facts,
  not a second AI path (ADR 0068 §15, rule 55).

#### 14.13.7 Settings, limits, logs and jobs added

- **Settings** (all in §16): `sequence.stall_floor_ms`, `ai.draft.section_tokens`, `dumps.around_before_s`,
  `dumps.around_after_s`, `dumps.dac_folders`, `dumps.dotnet_heap_objects`, `performance.dumps.walk_threads`,
  `performance.dumps.dotnet_helpers`, `performance.dumps.dotnet_memory_mb`. The symbol-server switch that gates binaries
  and DACs (ADR 0068 D4, D6) is today's symbols switch, not a new one.
- **Registry rows without a Setting (`hard`)**, each with `limit_case!`: `report.*` (§14.13.3), `sequence.export_events`,
  `sequence.median_window`, `ai.draft.facts`, `dumps.check_docs`, `dumps.query_ranges`, `dumps.query_bytes`,
  `dumps.check_queries`, `dumps.check_bytes`, `dumps.unwind_table_bytes`, `dumps.memory_index_entries`,
  `dumps.prefix_cache_docs`, `dumps.memory_regions` (65,536), `dumps.handle_objects` (65,536), `dumps.around_rows`,
  `dumps.dotnet_types`, `dumps.dotnet_frames`, `dumps.dotnet_output_bytes`. Dump limits live in `debrief_dump::Limits`
  today (`limits.rs`, carried as JSON in `debrief.document.v1`'s options, §5.7); WP-1 ("model, wire, limits") gives them a
  registry in `debrief-dump/src/limits.rs`, and the rows here are added to it, with the `Limits` fields read from the rows
  (one source).
- **Log lines** (§12.2's rules: fields only, a test per line in its emitting module): `Dump check` (ADR 0068 §14's
  fields, as there [A]); `Range read` (queries, ranges, bytes, missing, `FileChanged` field); `DAC lookup` (step,
  outcome, `SignatureFault`, HRESULT); `Managed helper` (pin, exit code, duration, peak job memory, section statuses, the
  dump path as a hash of its file name, last 20 stderr lines); `Draft section` (report id hash, section index, facts,
  tokens, cut, sentences cited/uncited/unknown, Pitwall provenance, duration; never text); `Sequence export` (format,
  lanes, events, cut).
- **Jobs** (§9.1, §9.2): checks are `Analysis`; drafting is `Ai` with phases `facts`, `drafting`, `checking`; series
  capture is `Capture` with `capturing`, `opening`. No new job kind.

#### 14.13.8 Work packages

Raw agent-days, judgement-sized [J] unless marked [P] (a size from ADR 0068's table or the roadmap note §2.3). ADR 0068's
packages keep their names and its sizes; "Accept" is the failing-test-first condition, from ADR 0068's test plan where
it has one. Every package that ends a job depends on ADR 0052 WP-B1 (§15's shared dependency).

| # | Package | Role | Depends | Days | Accept (failing test first) |
|---|---|---|---|---|---|
| C2 | Time shapes, widened to the three places (§14.13.1) | implementer | Z1 | (1.5, in §10.2) | `+9:00` in the line regex, the numeric date and `parse_offset`, and a spaced date, parse to the right instant |
| IF-1 | Citations: `cite`, `format_citation`, `written_time`, report, export and AI surfaces, Copy citation | implementer | FD-E2, Z3, Z6 | 3 [P] | a cited line in a zip member shows `a.zip!/m:12` with written and zoned times; a re-time moves the zoned time only; a removed source shows "gone" with the excerpt |
| IF-2 | Report blocks and audiences: the five kinds, `Table` dialect, `for_audience`, limits as rows, engine-rendered figures, editors | designer | FD-I, FD-E2, IF-1, Q-2 | 5.5 [P: 5 + the SQL block 0.5] | a Summary export of a report with a Technical-only block omits it and keeps its citations elsewhere; an SVG figure with a script is refused |
| IF-3 | Thread/sequence view: roles pack, `ThreadKey` on `ProcessInstance`, the store query, gaps, handoffs, the view, the three exports | designer | FD-B, FD-P (with its run split), 2.4, 3.3, Q-1, M3b | 6.5 [P: 7, less the run split moved to FD-P] | two threads with the same tid in two runs of one pid stay two lanes; a 30 s silence on a thread whose median is 50 ms is a gap and a 30 ms one is not; the Mermaid golden matches |
| IF-4 | Pitwall drafting: bundle, prompt, check, proposals, accept, replacing `incident_narrative` | designer | IF-2, FD-I, IF-1 | 4 [P] | against the fake Pitwall, a draft citing an unknown handle shows that sentence struck through and leaves it out on accept; a bundle over budget is cut with its `Cut` |
| IF-5 | Zip next to its extraction: the `extracted-copy` rule kind and `ExtractedCopy` reason | implementer | F5, SI-4 | 1 [J; user, 2026-11-12] | an invented zip and its extracted folder form one group with the folder preselected |
| IF-6 | `ctime;` event-log exports (ADR 0066's gap 4): pack entry and stamp format | implementer | Z1, SI-3 | 1.5 [J; user, 2026-11-12] | ADR 0066's `evt_ctime` row reads 07:00:00 UTC under its zone instead of "gap 4" |
| WP-0 | .NET helper spike against ADR 0068 §9's exit criteria; numbers under rule 48 | researcher | — | 3 [P] | as ADR 0068 §9 (1) to (6) |
| WP-2 | Reader: new streams, memory index, `DumpCapabilities` | implementer | WP-1 | 4 [P] | a synthetic dump without stream 17 reports `thread_info: false` and every check that needs it adds its Not checked item |
| WP-RR | `debrief.dumpquery.v1`, held handle, `lend_ranges`, re-grant, `FileChanged`, prefix LRU | designer | WP-1, WP-2, PH2a, 2.6b, MEM-2 (L-0) | 8 [P] | a range outside the index is `missing` with `MemoryNotInDump`; a changed file gives `FileChanged`; a restarted plugin answers the same query |
| WP-8 | Heap dumps by ranges; ADR 0064 amendment; `WholeReason::DumpWithoutMemoryList` removed | implementer | WP-RR | 3 [P] | an invented 1.2 GiB heap dump without a Memory64 list opens within the reader cap |
| WP-3 | Unwinder over framehop; re-walk | designer | WP-RR, WP-8 | 12 [P] | a synthetic x64 stack walks by CFI past a frame that scanning misses; a planted module path is never opened |
| WP-4 | Check crash, break-in | implementer | WP-1, WP-2 | 3 [P] | a breakpoint on a `DbgUiRemoteBreakin` stack gives `DebuggerBreakIn` |
| WP-5 | Check hang, one dump; `ThreadState` removed | designer | WP-3, WP-RR, WP-8 | 8 [P] | a two-critical-section deadlock gives `Deadlock` with High; without its handle stream the same dump gives `CannotTell` (the ER-33 probe) |
| WP-6 | Series: pairing on §2.5, picker groups, progress, spin ranges, growth rule | implementer | WP-4, WP-5, FD-P | 4 [P] | proptest: no series of two yields `Growing`; a pid reused with another create time is `Different` |
| WP-7 | Check memory | implementer | WP-1, WP-6 | 4 [P] | three dumps growing every step give `Growing` with a rate; one step gives `GrewOnce` |
| WP-9 | Events around (on `StampedTime`, `EvidenceRef`, `Bounded`) | implementer | WP-1, FD-E2, Z6 | 3 [P] | events of another machine are not mixed in when a host field matches; the window follows the Settings |
| WP-10 | Shell: job, cache, commands, logs | implementer | WP-1, WP-RR, WP-18 | 3 [P] | pressing a check twice starts one job (cache by `CheckKey`); Cancel caches nothing |
| WP-11 | Capture series, presets, D9 flags with sizes measured | implementer | — | 3 [P] | a Minidump capture never sets 0x100 or 0x40000; series names follow ADR 0068 §5 |
| WP-12 | .NET: helper, CI build, pin, plugin, DAC lookup and signer policy, per-query grant, notices, decoder and fuzz target | designer (integrator for `.gitea/`) | WP-0, WP-1, WP-RR, PT-1, PT-2 (and Y16's DP1, DP4 for the notices step) | 15 [P] | an unsigned DAC gives `DacUnavailable` "unsigned or wrong signer"; the helper's JSON validates against the generated schema; offline with no runtime the managed part is Not checked with its next step |
| WP-13 | UI: checks strip, report view, pair picker, capability line, capture options | designer | WP-10, WP-18, M3a, IF-2 | 6 [P] | one Playwright spec per check against the mock; axe in both themes |
| WP-14 | Tests outside a package: command-driven Windows fixtures, named-build mutant tests, properties, first measurements | implementer | all of ADR 0068 | 8 [P] | as ADR 0068's test plan |
| WP-15 | Docs and ADR amendments (0023, Languages rule D3, ADR 0068 scope line), report screens reviewed (ER-23) | implementer, then reviewer | WP-13 | 2 [P] | the user guide's "which dump for which question" page; the review's P1s fixed |
| WP-16 | Unexpected-modules pack on `paddock-packs` | implementer | WP-2 | 2 [P] | a user pack adds an injector and the finding names it; a broken pack keeps the last good one |
| WP-17 | One minidump reader (D10); copy entry removed | implementer | WP-2, SI-12 | 2.5 [P: 3, less 0.5 shared with SI-12's minidump importer] | the Windows plugin's minidump import goes through `debrief-dump`'s reader; its parser file is gone |
| WP-18 | `NextStep` variants (shared change, all users updated) | integrator | ADR 0052 WP-B1 | 1 [P] | each variant round-trips and has its words |
| PT-1 | **`paddock-tools`, the subset WP-12 needs** (ADR 0051 WP-1 scaffold, `ToolPack`, `ToolLayout`; WP-5 download and verify by size and SHA-256; WP-6's verify half only: Authenticode through `WinVerifyTrust` with the signer and root policy, no privileged helper or installer; WP-10 `./dev tools pin` with the `ci-artifact` evidence kind), plus an independent security review of the Authenticode verify (ER-23's reader, a reviewer) | designer; reviewer for the review | T (shared code, rule 1); ADR 0051 accepted | 10.5 [P: 3 + 1.5 + 2.5 (half of WP-6's 4 to 6) + 2.5; review 1 [J]; range 9 to 13] | a pinned artifact with a wrong SHA-256 is refused; a test-signed DAC fails the Microsoft signer policy with a typed `SignatureFault`; `./dev tools pin` writes a `ci-artifact` record that the check reads back; the review's P1s fixed |
| PT-2 | ADR 0051 WP-12's "downloaded on request" class in `paddock-deps`, so the helper's runtime and ClrMD notices reach About | implementer | PT-1, Y16's DP1 and DP4 | 2.5 [P: 2 to 3] | the helper's notices appear in About only once it is installed |
| | **Investigation-folder total** (IF-1 to IF-6) | | | **21.5** | C2 stays in §10.2; IF-3's run split (0.5) moved to FD-P |
| | **ADR 0068 remainder** (99 [P] less WP-1's 4, less 0.5 netted in WP-17) | | | **94.5** | 76.5 without WP-0 and WP-12 |
| | **Restored scope** (IF and ADR 0068) | | | **116** | [calc] |
| | **Its prerequisites priced here** (PT-1, PT-2) | | | **13** | [calc]; Q-U6 offers making .NET conditional instead |
| | **Total added to 1.2** | | | **129** | [calc], plus FD-P's +0.5 in plan §10.1 |

**Test homes of ADR 0068's packages** (crate paths under `apps/debrief/crates/` unless named; each file holds that
package's failing tests first):

| WP | Test file | WP | Test file |
|---|---|---|---|
| WP-0 | `apps/debrief/tools/debrief-clr-inspect/tests/ExitCriteriaTests.cs`, numbers in `apps/debrief/docs/performance.md` | WP-9 | `debrief-core/tests/it/dumps_around.rs` |
| WP-2 | `debrief-plugin-dumps/tests/reader_streams.rs` | WP-10 | `app/src-tauri/src/dumps/tests/check_job.rs` |
| WP-RR | `debrief-plugin-dumps/tests/dumpquery.rs`, `app/src-tauri/src/dumps/tests/held.rs`, round trip in `debrief-plugin-ext/src/wire/dumpquery.rs`, fuzz `debrief-plugin-ext/fuzz/fuzz_targets/dumpquery_request.rs` | WP-11 | `debrief-plugin-dumps/tests/capture_series.rs` (Windows runner) |
| WP-8 | `debrief-plugin-dumps/tests/heap_by_ranges.rs` | WP-12 | `debrief-dump/src/managed/tests.rs` (decoder on committed JSON), fuzz `debrief-dump/fuzz/fuzz_targets/managed_json.rs`, `app/src-tauri/src/dumps/dac/tests.rs`, `apps/debrief/tools/debrief-clr-inspect/tests/SchemaTests.cs` |
| WP-3 | `debrief-plugin-dumps/src/unwind/tests.rs`, `debrief-plugin-dumps/tests/unwind_synth.rs` | WP-13 | `app/src/app/dumps/checks/*.spec.ts`, `app/e2e/dump-checks.e2e.ts` |
| WP-4 | `debrief-dump/src/checks/crash/tests.rs` | WP-14 | `debrief-plugins/tests/it/dumps_windows.rs` (command-driven fixtures), `debrief-plugin-dumps/tests/mutant_builds.rs` |
| WP-5 | `debrief-dump/src/checks/hang/tests.rs` | WP-15 | none (docs; the link check and the review) |
| WP-6 | `debrief-dump/src/checks/series/tests.rs` (proptest of the two-versus-three rule) | WP-16 | `debrief-dump/src/packs/injectors/tests.rs` |
| WP-7 | `debrief-dump/src/checks/memory/tests.rs` | WP-17 | `debrief-plugin-windows/tests/minidump_import.rs` |
| PT-1 | `crates/paddock-tools/tests/it/{pack,download,authenticode,pin}.rs` | WP-18, PT-2 | the `NextStep` tests beside ADR 0052 WP-B1's enum in `crates/paddock-jobs` [U: that file is WP-B1's]; `crates/paddock-tools/tests/it/notices.rs` |

#### 14.13.9 Parallelism and cost (ER-20)

| Operation | Bound | Setting | Cheaper alternative checked |
|---|---|---|---|
| Citations | one resolve per 1,000 refs (§2.4) | none | written time reconstructed from provenance instead of storing each line's offset (a column per event) |
| Report export per audience | one pass over the resolved report | none | `for_audience` filters the resolved report; no second resolve |
| Sequence page | the store's readers (§14.5) | none | keyset pages; never a whole thread; median over a bounded look-back |
| Drafting | one section at a time per window (a person reads each) | `ai.draft.section_tokens` | facts ranked and cut before the call; no model over raw events (rule 46) |
| Re-walk | cores − 1 across threads | `performance.dumps.walk_threads` (Auto) | only improvable threads; unwind tables parsed once per module |
| Range reads | sequential per query | none | only addresses a waiting frame or module names; prefix LRU of 2 |
| .NET helper | one at a time (memory-heavy) | `performance.dumps.dotnet_helpers` | one run per dump with all sections, cached by `CheckKey` |

Priority (rule 52): checks, sequence pages and drafting are Live (a person waits); capture series is Bulk at the
gentle level; nothing here uses a starvable level.

#### 14.13.10 Common practice and cost (ER-37)

Debrief is a personal tool, so ER-37's default is common practice; each row names the cheaper common-practice option,
which §17.2 Q-U5 offers as a decision. Sources were read on 2026-11-12.

| Function | Common practice, with sources | Cost here (raw) | Cheaper common-practice option: its cost, what it saves |
|---|---|---|---|
| Citations | Tools quote a place as `file:line`: "Error messages from compilers should look like this: sourcefile:lineno: message" ([GNU Coding Standards](https://www.gnu.org/prep/standards/html_node/Errors.html)); an entry inside an archive as `jar:<url>!/{entry}` ([Java `JarURLConnection`](https://docs.oracle.com/javase/8/docs/api/java/net/JarURLConnection.html)); a time with its numeric offset ([RFC 3339](https://www.rfc-editor.org/rfc/rfc3339)) | IF-1 3 | `path!/member:line` plus one UTC time, no written time: about 1, saves about 2 |
| Report blocks, audiences | Postmortems have fixed sections, "Summary", "Impact", "Root Causes", "Trigger", "Resolution", "Detection", "Action Items", "Lessons Learned", "Timeline" ([Google SRE example postmortem](https://sre.google/sre-book/example-postmortem/)); a second, external version "is a summarized and sanitized version of the information used for the internal postmortem" ([PagerDuty postmortem guide](https://docs.pagerduty.com/ops-guides/postmortem-best-practices/step-by-step)); notebooks mix text with computed outputs ([Jupyter Notebook](https://jupyter-notebook.readthedocs.io/en/stable/notebook.html)) | IF-2 5.5 | hypotheses, open points and glossary as headings in today's text blocks, one audience; only `AnalysisResult` and `Figure` added: about 3, saves about 2.5 |
| Thread/sequence view | One lane per thread: Visual Studio's Threads view has "A channel for each thread in the process" with executing and blocked segments ([Concurrency Visualizer](https://learn.microsoft.com/en-us/visualstudio/profiling/threads-view-parallel-performance)); Perfetto filters tracks by process or thread ([Perfetto UI](https://perfetto.dev/docs/visualization/perfetto-ui)); sequence diagrams "are defined entirely in text" ([PlantUML](https://plantuml.com/sequence-diagram); also [Mermaid](https://mermaid.js.org/syntax/sequenceDiagram.html)) | IF-3 7 | a grid filter "thread = X" plus timeline lanes by the thread field, and a Mermaid text export only, no gaps or handoffs: about 4, saves about 3 |
| Pitwall drafting | Grounded generation returns its sources: Amazon Bedrock Knowledge Bases' retrieve-and-generate says "The responses are returned with citations to the original source data" ([AWS documentation](https://docs.aws.amazon.com/bedrock/latest/userguide/kb-test-retrieve-generate.html), current, read 2026-11-12). Checking each cited handle against the facts given is this design's addition [J] | IF-4 4 | keep today's one-request incident narrative, marked as generated, with no per-section facts or citation check: about 1 (moving it onto Pitwall), saves about 3 |
| Time shapes, small items | — (bug fixes and pack entries) | C2 1.5, IF-5 1, IF-6 1.5 | none cheaper |
| Dump checks, native | Microsoft's own tools answer the same three questions: DebugDiag "is designed to assist in troubleshooting issues such as hangs, slow performance, memory leaks … and crashes" ([download page](https://www.microsoft.com/en-us/download/details.aspx?id=103453)); WinDbg's `!analyze` "displays information about the current exception" and `-hang` "Generates !analyze hung-application output" ([`!analyze`](https://learn.microsoft.com/en-us/windows-hardware/drivers/debuggercmds/-analyze)); Visual Studio shows a "Minidump File Summary" ([dump files](https://learn.microsoft.com/en-us/visualstudio/debugger/using-dump-files)) | 76.5 (WP-2 to WP-11, WP-13 to WP-18) | (a) "Open in WinDbg or Visual Studio" from the dump view, today's view kept: about 1.5, saves about 75; (b) the checks without Debrief's own unwinder, scanned frames labelled (saves WP-3's 12, with weaker hang verdicts) |
| .NET managed state | `dotnet-dump analyze` runs SOS commands such as `clrstack`, `dumpheap`, `syncblk` ([dotnet-dump](https://learn.microsoft.com/en-us/dotnet/core/diagnostics/dotnet-dump)); Visual Studio's .NET Diagnostic Analyzers report thread-pool starvation on a managed dump ([Microsoft Learn](https://learn.microsoft.com/en-us/visualstudio/debugger/how-to-debug-managed-memory-dump)) | WP-0 3 + WP-12 15 = 18, plus PT-1 and PT-2's 13 for the `paddock-tools` and `paddock-deps` parts it needs | "Open in Visual Studio" or a copied `dotnet-dump analyze` command line, with the managed part shown as Not checked: about 0.5, saves about 17.5. ADR 0068 D1 rejected parsing `dotnet-dump`'s text as a stop-gap; linking out is not parsing |

**Why this design goes further.** The user decided each of these with its size: ADR 0068 D1 to D13 on 2026-11-07 (99
raw, "all of it in 1.2"), the four investigation-folder features on 2026-11-07, and their return to 1.2 on 2026-11-11.
The design adds nothing beyond those decisions; the cheaper options are offered once more because ER-37 asks for it.

## 15. Package index: which section each package cites

Every 1.2 package of the plan at `ac85e26c7`, except those §10.3 moves out.

**Shared dependency of every job package below:** ADR 0052 WP-B1 (`Outcome`, `ReadBack`, `NextStep`, `Why` in
`paddock-jobs`), §9.6. It is ADR 0052's to schedule ("after Spotter 1.0, before Pitcrew 1.1"); the Debrief packages that
end jobs (F4, F7, F8, 3.4, Z5, Y9, J1, FD-F's builds) land after it or with it.

| Package | Section | | Package | Section |
|---|---|---|---|---|
| 0.1, 0.2, 0.3 | §3.9, §14.1, §14.2 (the failing tests they write) | | SI-0, SI-1, SI-2 | §3.1 to §3.3, §3.9 |
| 0.4 | §1.1 | | SI-3, SI-11 | §3.2, §3.3, §5.7 |
| 1.1, 1.2, M2b | dialogs part | | SI-4, SI-5 | §4.8, §3.4, §6.9 |
| 1.3 | §14.1 | | SI-6 | §5.10, §3.8 |
| 1.4 + B1 (ADR 0065) | §4.6 | | SI-7 | §4.1, §4.2 |
| 2.1, 2.7, 4.5, 3.7 | §14.1 | | SI-8 | §5.3, §5.4, §5.7 |
| 2.2, P1, BP1 | §4.5 | | SI-9, PH4 | §5.4, §3.3 |
| 2.3, B2, 3.2, 4.4 | §14.4 (UI), §4.2 and §4.5 (data) | | SI-10, 3.4 | §3.4, §3.5, §4.2 |
| 2.4, 3.3, 4.1, M0, FL1, CH1 | §14.2 | | SI-12, MEM-3, MEM-4 | §3.3, §5.4 |
| 2.5 (rest) | §4.6, §14.1 (lints) | | SI-13, 3.6 | §3.7, §4.6, §14.4 |
| 2.6a | §4.2 | | SI-14 | §3.9 |
| 2.6b | §4.1, §4.2 | | SI-15, SI-16 | §3.9, §3.3 |
| 2.6c | §4.4 | | SI-17, SI-18 | §14.1, §3.8 |
| 2.6d, LV1, LV2, LV3 | §4.3, §4.4 | | F1, F1b, F2 | §6.2 |
| IP1 | §4.3, §4.1 | | F3 | §6.3 |
| PH1 | §4.7, §4.9 | | F4 | §6.4, §9 |
| PH2a | §3.1, §5 | | F5, F5b | §6.3, §3.2 |
| PH2b | §4.9, §5.8, §3.4 | | F6 | §6.6 |
| PH3 | §4.9 | | F7 | §6.5, §6.7 |
| J1 | §9.3 | | F8 | §6.10 |
| 4.2 | §7.1, §4.6, §14.4 | | F9, F9b | §6.5, §14.1 |
| 3.5 | §3.3, §4.8 | | F10, F10b, F11 | §14.1, §6.9 |
| B3 to B8, B7b (ADR 0065) | §4.6, §13.3 | | F12, FM, FR | §6.11, §6.8 |
| Z0 to Z13, Z5b, Z8b, Z12a, Z12b | §10 | | D-a, D-b | §7.3, §14.5 |
| FD-U | §2.1 | | M1, M2a, M2c, M3a, M3b, M4, M5, Y12 | §14.3 |
| FD-B | §2.2, §7.2 | | ST-1, ST-2, ST-3, ST-5, ST-6, Q-1 to Q-4 | §14.5 |
| FD-P | §2.5 | | SR1, SR3, SR4, SR5, IX-B1 (IX-B2, IX-C, IX-S, IX-M deferred: ADR 0062's trim) | §14.5 |
| FD-E1, FD-E2 | §2.3, §2.4 | | P0, P2, P3, P4, P5, NR1, CPU-1, NB-1, D-0 | §14.5 |
| WP-1 (ADR 0068), FD-I | §11 | | 4.3, 5.1, 5.2, B1 bench gates, X5 | §14.5 |
| FD-F, FD-K, FD-M | §14.6, §12.2 | | Y3 rest, Y4, Y11, Y15 | §14.5 |
| FD-G, MEM-5, MEM-6 ADR | §14.8 | | C1 to C5 | §14.7 |
| WP-SL1 | §12.3 | | N1, N2, N3, N6, N8 | §14.9 |
| Y9 (job service) | §9.5 | | X1 to X11, Y6, Y7, Y8, Y14 R0 and R3, Y16, A1 | §14.10 |
| Y10 | §14.11 | | 5.4, M5, R, F (fix buffer), L-0 | no API (review, safety net, release, landing) |
| W0 (ADR 0063's binary-wire gate) | §13.4, §17 Q1 and Q15 | | MEM-1, MEM-2 (built on `debrief/p0-idle`, not landed; L-0 lands them, §8) | §8, §14.8 |
| WG0 (the shell screenshot baseline, called W0 in the plan's M rows) | §14.3 | | IF-1 citations | §14.13.2, §2.4, §10.3 |
| IF-2 report blocks and audiences | §14.13.3, §11 | | IF-3 thread/sequence view | §14.13.4, §14.2, §14.5 |
| IF-4 Pitwall drafting | §14.13.5 | | IF-5, IF-6, C2 (widened) | §14.13.1, §6.3 |
| WP-2, WP-RR, WP-8 (ADR 0068) | §14.13.6, §5.9, §4.2, §14.8 | | WP-3 (ADR 0068) | §14.13.6 |
| WP-4 to WP-7, WP-9 (ADR 0068) | §14.13.6, §11, §2.5 | | WP-10, WP-11, WP-13 (ADR 0068) | §14.13.6, §9 |
| WP-0, WP-12 (ADR 0068) | §14.13.6 (.NET), prerequisites table | | WP-14 to WP-18 (ADR 0068) | §14.13.6, §14.13.8 |

## 16. Settings introduced or touched

All under Settings ▸ Performance unless named; each validated in the engine; Auto where a count. **One scheme:**
`performance.<area>.<name>` for performance values, `<area>.<name>` for the others (`folder.`, `timeline.`, `time.`).
ADR 0067 names two of them differently (`performance.plugin_memory_mb`, `performance.import_whole_file_mb`); F packages
use the names here (§17, Q24).

| Path | Default | Bounds | Owner |
|---|---|---|---|
| `performance.import.parse_threads` | Auto (cores − 1, ≤ 8) | 1 to cores | exists |
| `performance.import.record_bytes` | 8 MiB | 1 to 256 MiB | SI-13 |
| `performance.import.dump_whole_bytes` | 1 GiB | exists | exists |
| `performance.import.spool_bytes` | 16 GiB | 1 GiB to disk | SI-13 |
| `performance.import.archive_ratio` | 200 (floor 64 MiB) | 10 to 2,000 | SI-13 |
| `performance.import.whole_file_mb` | store cap − import budget (1,280) | 0 to store cap | FM |
| `performance.import.headroom_mb` | 256 | 64 to 1,024 | FM |
| `performance.plugins.memory_mb` | 1,024, or No cap | 64 to 4,096 | 2.6a, 3.2 |
| `performance.plugins.start` | On first use | On first use, At startup | 2.6a, 4.4 |
| `performance.plugins.idle_stop_min` | 10 [J] | 1 to 120, Never | 2.6a, 4.4 |
| `performance.plugins.stream_credit` | Auto (4) | 1 to 64 | 2.6b |
| `performance.stall.patience` (decided 2026-11-12, Q-U3 (b); the per-key values are LV1's constants, §4.4) | Normal (×1) | Normal, Patient (×4) | 2.6c, LV1 |
| `performance.index.read_threads`, `.write_threads`, `performance.catalogue.workers` | Auto (within cores − 1 beside parse threads; capped at 4 until measured) | 1 to cores | SR1, SR3 |
| `performance.symbols.cache_mb` | from MEM-5's measurement | 64 to 4,096 | MEM-5 |
| `performance.documents.budget_mb` | from the MEM-6 ADR | as there | MEM-6, FD-G |
| `performance.import.huge_threshold_gb` | from 5.2's measurement | 1 to 1,000 | 5.2 |
| `performance.import.items_in_flight_mb` | 128 | 64 to 1,024 | 2.6b, 3.4 |
| `folder.scan_entries` | 500,000 | 10,000 to 5,000,000 | F4 |
| `folder.scan_threads` | Auto (`parse_threads` local, 4 remote) | 1 to cores | F4 |
| `timeline.lanes_default` | 10 | 1 to 200, or All | 3.3 |
| `timeline.cache_mb` | 16 | 4 to 256 | 4.1 |
| `time.display` | UTC | UTC, Local, Logs' time zone | Z-packages |
| `time.zone_probe_budget_ms` | ADR 0066 D3 | as there | Z8 |
| `sequence.stall_floor_ms` | 1,000 | 10 to 600,000 | IF-3 |
| `ai.draft.section_tokens` | 8,000 | 2,000 to 32,000 | IF-4 |
| `dumps.around_before_s`, `dumps.around_after_s` | 120, 10 (ADR 0068 §6) | 1 to 3,600; 0 to 600 | WP-9 |
| `dumps.dac_folders` | none | up to 16 folders | WP-12 |
| `dumps.dotnet_heap_objects` | 200,000,000 (ADR 0068 §9) | 1,000,000 to 2,000,000,000 | WP-12 |
| `performance.dumps.walk_threads` | Auto (cores − 1) | 1 to cores | WP-3 |
| `performance.dumps.dotnet_helpers` | 1 | 1 to 4 | WP-12 |
| `performance.dumps.dotnet_memory_mb` | Auto (half of physical memory, at most 8,192; 4,096 when unknown) | 512 to 16,384 | WP-12 |

Limits that are registry rows without a Setting (`hard`) are listed with their sections: `store.event_ids` (§2.1),
`evidence.resolve_batch` (§2.4), `query.source_folders` (§6.10), `timeline.lanes_max` (§14.2),
`store.exact_positions_max` (§14.5), `search.budget_ms` (§14.5), `symbols.cache_bytes` (§14.8), and the restored
scope's rows (§14.13.7).

## 17. Open questions, corrections, recommendations

| # | Question | Recommendation |
|---|---|---|
| Q1 | **Decided by the user, 2026-11-07, as recommended.** ADR 0063 is still Proposed; FD-U, 3.3 and 4.5 shape replies | **Measure first: ADR 0063's W0 gate early in wave 1, and decide 0063 before 3.3 and 4.5 build their reply shapes** (roadmap R8). FD-U does not wait: the declared bound (§2.1) holds under JSON and under MessagePack alike |
| Q2 | **Decided by the user, 2026-11-07, as recommended.** Ids above 2^53 | **Declared bound** (§2.1), the cheapest of string, bigint or bound; a registry row `store.event_ids` (`hard`) and a test at `2^53 − 1` |
| Q3 | ADR 0068 §12 drops `WholeReason::DumpWithoutMemoryList` (WP-8), which ADR 0064 lists | Keep it in the enum until WP-8 lands, then remove it in WP-8's change with ADR 0064's amendment. (Revision 6: WP-8 is back in 1.2 with ADR 0068's checks, §14.13, so the reason is removed within 1.2, in WP-8's change) |
| Q4 | ADR 0068 §10 names `left_out: Vec<Omission>`; ADR 0065 has no `Omission` | **Use `Vec<Cut>`** (§11); a one-word amendment to ADR 0068 in WP-1's change |
| Q5 | `NextStep` lives in family ADR 0052's `paddock-jobs` enum, not on main | **Closed (revision 7) in ADR 0068's favour:** no local `NextStep`, not even a registered copy. WP-1 to WP-9 do not need it; WP-10 and WP-13 wait for ADR 0052 WP-B1 and WP-18 (ADR 0068 "Work packages", last paragraph) |
| Q6 | **Closed (revision 6): done by `paddock_proc::run_state(ProcessId)`.** ADR 0031's `RunStateProbe::sample(pid: u32)` took a bare pid (rule 53) | `run_state` on `origin/main` takes a `ProcessId` and is identity-checked (a pid handed on reads `Exited`, `runstate.rs`); `RunStateProbe::sample(&ProcessId)` reads it (§4.4). Nothing left to build; ADR 0031 §5's text is corrected with 2.6c's rename note |
| Q7 | (Revised.) The time basis on the wire. Under ADR 0001 appending an enum variant is a minor change and only struct fields freeze, so the issue was never the variant: it was that one `ts_kind` enum could not carry ADR 0066's model (low two bits `TsState`, separate flags) | **`ts_state: u8` with ADR 0066 §6's exact bit layout** (§5.3), the bits a plugin may set named, read through `debrief_batch::TsCode`; plugins send flags 0 until Z1 |
| Q8 | §6.5's `ChooseToken` instead of paths in `folder_scan_start` deviates from ADR 0067 | Accept the token (no path crosses twice); a one-line note in ADR 0067 by F7 |
| Q9 | `Supervisor` without containment as a type error (§4.2) instead of ADR 0001's run-time refusal | Type error plus the existing test; the run-time refusal stays for the `developer` feature path |
| Q10 | (Resolved.) `ViewFilter`, the profile limits and the document options | Listed from source and carried as JSON (§5.7, §5.8); `CondValue`'s `untagged` stays legal because it never becomes postcard |
| Q11 | `FileInfo.rel_tail` caps the path at 3 segments [J] | Accept; importers that read a folder name from the path (a machine folder) get it; the full path stays host-side |
| Q12 | Plan D5 (`Limit`/`Page` home) | **`paddock-limits`** (§4.6), with ADR 0065's types, not `paddock-view` |
| Q13 | `performance.plugins.idle_stop_min` default | 10 minutes [J]; 4.4 measures plugin start cost and may change it with a recorded result |
| Q14 | The drawing research's package ids `F1` (flame fixes) and `C1` (chart bound) collide with the plan's F1 (the judge) and C1 (UMDH reader fixes) | **Rename them FL1 and CH1** in the plan's next edit; this file uses those names |
| Q15 | **Decided by the user, 2026-11-07, as recommended.** `timeline_lane_buckets` answers raw bytes (drawing research) while ADR 0063 would make every reply binary | **Lane buckets follow the W0 outcome (Q1). One binary mechanism:** if 0063 is adopted, lane buckets are one of its tables and the `LB` layout is not built; if 0063 is rejected, `LB` v1 is the only raw reply, recorded as a short ADR 0018 amendment. Decide with Q1, before 3.3 |
| Q16 | (Revised.) ADR 0020's executor takes `paddock_proc::MemoryCap` in `submit_child`, while §4.5 has `MemoryPolicy` | One memory type: `submit_child` takes `MemoryPolicy` (Y9 amends ADR 0020 in its change). On classes there is no conflict: under ADR 0020's 2026-10-30 amendment `Background` is the gentle level on every OS and only `Idle` is starvable, so `class_for` maps Bulk to `Background` (§9.5) |
| Q17 | **Decided by the user, 2026-11-07, as recommended.** Exact positions below 100M cost 905 MiB of `ord` at 100M against the 768 MB budget (plan §9.6 D7) | Keep `store.exact_positions_max` a `hard` row at 100M and let `ScrollModel` switch to time above it; bring the threshold back to the user with ST-5's numbers, as D7 says |
| Q18 | ST-6, Y11's "sticky field promotion" and Y14's "R3 split" were not found in the sources read | Each owner writes a one-paragraph scope in the plan row before starting; none changes a foundation API here unless it adds a port row |
| Q19 | Canvas vs DOM: ADR 0018 decision 8 defaults to DOM, the drawing research to canvas | The rule in §14.2 (canvas if CV1 passes; DOM only if CV2 passes where canvas failed), recorded as ADR 0018's note by M0 |
| Q20 | F10b (ADR 0067's windowed rows) and 2.1 both define a window over rows | **One type, `PdRowWindow`, owned by 2.1**; F10b adopts it, which pulls 1.3 → 2.1 ahead of F10 (roadmap stop-gap table) |
| Q21 | ADR 0066 D13 asks ADR 0064 for an amendment (`times()` with no default, `&mut ImportCtx`, `ImportDone.time`, the wire flags) | **Taken here as D13 recommends** (§3.2, §5.3, §5.4); ADR 0064's author records the amendment in SI-1's change. No `RefCell` deviation remains |
| Q22 | `FileRefusal` (§6.2) renames ADR 0067's `Refusal`, which collides with `paddock_plugin_api::compat::Refusal` | Accept the rename; a one-line note in ADR 0067 by F1 |
| Q23 | `ImportSink` renames ADR 0064's `EventSink`, which collides with `paddock_jobs::reporter::EventSink` | Accept; a one-line note in ADR 0064 by SI-1 |
| Q24 | Setting paths: ADR 0067 used `performance.plugin_memory_mb` and `performance.import_whole_file_mb` | Use §16's one scheme (`performance.plugins.memory_mb`, `performance.import.whole_file_mb`); FM and 2.6a use these |
| Q25 | `TsState` lives in `debrief-batch`, which pulls Arrow; plugins must not | `debrief-batch`'s Arrow dependencies behind a default feature `arrow`; `debrief-plugin-ext` depends on it with `default-features = false` (one definition, rule 55); a `cargo tree` check in CI (beside PH2a's) fails when a plugin binary (`debrief-plugins`, `debrief-plugin-*`) links an `arrow-*` crate. Alternative: move ids and `TsState` to a dependency-free crate; not chosen, one more crate for two types |
| Q26 | Plugin items in flight across an import add up to the store worker's whole budget (§8) | **A shared `CreditPool` by bytes** (§4.2), initial grants included, at most `limit / EVENT_FRAME_MAX` plugin streams, FIFO fairness; `performance.import.items_in_flight_mb` 128 MiB, raised only with 2.6b's and 4.3's recorded results |
| Q27 | **Decided by the user, 2026-11-07, as recommended.** D7's 768 MB is the worker's total peak footprint (§8). With ADR 0067's idle allowances, 138 MB remain on macOS and 448 MB on Linux for import parts whose bounded share alone is 285 MB (read buffers 151, credit pool 134; revision 6: no tee, ADR 0062's trim; revision 5 said 525 with a 240 MB tee), before batches, the writer and DuckDB: short by 147 MB on macOS, 163 MB left on Linux | 4.3 measures the idle footprint and every part first (the allowances are not measurements). Then choose: a per-OS budget (macOS's footprint counts more than Linux's PSS), smaller parts (chunk size, credit pool, at a measured throughput cost), or both. Bring back with 4.3's numbers, beside plan §9.6 D1 |
| Q28 | Annotation "new since" needs a listing sequence the store does not have (§5.6) | **Closed by Q-U1 (c), 2026-11-12:** full runs in 1.2, no listing sequence; IX-B1 builds none of it. The rewrite rules of §5.6 wait for the release that adds incremental runs |

**Where the ADR notes land.** Q21's amendment of ADR 0064 and Q23's note land in SI-1's first commit; Q22's note in
ADR 0067 lands in F1's first commit; Q8 and Q24's notes in ADR 0067 with F7 and FM; §4.4's rename (`CallPatience`, `paddock_proc::RunState`, the `ProcessId` probe) in ADR 0031 §5 with 2.6c; Q-U3's note in ADR 0031 at line 191 ("Both are tunable in Settings under rule 51" becomes: constants per key, one multiplier `performance.stall.patience`) with 2.6c; Q4 in
ADR 0068 with WP-1; Q16 in ADR 0020 with Y9.

### 17.1 Common practice and cost (ER-37, revision 6)

How comparable, well-known software handles the main mechanisms of this design, what it costs here in raw agent-days
(plan §9.2's sizes; [calc] sums, [J] where no plan row exists), and the cheaper common-practice option. Debrief is a
personal tool (memory "user-personal-use"), so ER-37's default is common practice; every row below that goes further
says why, and its cheaper option is a decision the user can still take.

| Mechanism here | Common practice, with sources | Cost here | Cheaper common-practice option | Why this design goes further |
|---|---|---|---|---|
| Plugins out of process, sandboxed, supervised (§4, PH1 to PH4, IP1, 2.6a to 2.6d) | Browsers run untrusted parsing and rendering in sandboxed target processes under a privileged broker that outlives them (Chromium, [sandbox design](https://chromium.googlesource.com/chromium/src/+/main/docs/design/sandbox.md)); VS Code runs extensions in an extension host apart from the UI ([extension host](https://code.visualstudio.com/api/advanced-topics/extension-host)) | IP1 4 (built), 2.6a 4.5, 2.6b 3.5, 2.6c 4.5, 2.6d 4, PH1 2.5 (3.5 − 1, ADR 0070), PH2a 2, PH2b 3.5, PH3 0.5, PH4 3.5: about 32.5 [calc] | none cheaper that keeps the 1.1 behaviour: Debrief 1.1 already hosts plugins out of process; the cost is moving onto the shared host (ADR 0003, accepted), not isolation itself | the family decision (ADR 0001, 0003): one host for every app |
| Credit-based back-pressure on streamed items (§4.2 `CreditPool`) | HTTP/2's receiver-controlled, credit-based flow control with `WINDOW_UPDATE` ([RFC 9113 §5.2](https://www.rfc-editor.org/rfc/rfc9113#section-5.2)) | inside 2.6b (3.5) | a fixed per-stream credit without a shared pool | 16 files × credit 4 × 12 MiB items is the whole 768 MB budget (§8, Q26) |
| Bounded replies that say what they left out (§4.6, ADR 0065) | Elasticsearch counts hits exactly up to `track_total_hits` (default 10,000) and reports `hits.total.relation: "gte"` beyond it, a lower bound ([search API](https://www.elastic.co/docs/solutions/search/the-search-api)) | 1.4 + B1 5 (built); B2 to B8 per ADR 0065 | a plain cap with a "results truncated" flag | ADR 0065 (accepted 2026-11-06, full scope): Debrief's lists are evidence, and an unmarked cut misleads an investigation |
| Stall judgement instead of timeouts (§4.4, 2.6c, 2.6d, LV1, LV2) | Windows calls a window not responding after an internal 5 s without pumping messages, not configurable and "subject to change" ([`IsHungAppWindow`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-ishungappwindow)); the OS shows the state and the user decides | 2.6c 4.5, 2.6d 4, LV1 2.25 (3 − 0.75), LV2 1: about 11.75 [calc], on the start judge that is already built | fixed timeouts per call with a "still working" note, no per-key Settings (Q-U3 (c)) | ADR 0031 (accepted): a paused or starved Mac (App Tamer, CI 7096) failed fixed timeouts. **Decided 2026-11-12, Q-U3 (b):** constants plus one Setting |
| Virtual lists and the keyed window (§14.1) | Angular CDK virtual scrolling renders only the visible items ([CDK scrolling](https://material.angular.dev/cdk/scrolling/overview)), which rule 47 already prefers | 2.1 4 (+1 to 2 for columns), 2.7 2.5, 4.5 4, 3.7 3: about 13.5 to 15.5 [calc] | CDK `cdk-virtual-scroll-viewport` with fixed row heights and an index window only, no keyed mode | 1 billion events: an index window cannot place rows whose positions are unknown above 100M (§14.5, Q17) |
| `define*` contributions, lazy by construction (§14.3, M1 to M5) | VS Code declares contribution points in the extension manifest and activates an extension when one of its activation events fires ([contribution points](https://code.visualstudio.com/api/references/contribution-points), [activation events](https://code.visualstudio.com/api/references/activation-events)); JupyterLab plugins provide and require tokens ([extension guide](https://jupyterlab.readthedocs.io/en/stable/extension/extension_dev.html)) | the M packages of the shell move (plan, not repriced here) | keep Debrief's own registries (no move) | the family workbench (ADR 0013 and workbench.md, accepted): one shell for every app |
| Restored scope: citations, report blocks, sequence view, drafting, ADR 0068's checks and .NET (§14.13) | per function in §14.13.10, with sources | 116 + 13 (PT-1, PT-2) [calc] (§14.13.8) | per function in §14.13.10 (Q-U5) | the user's decisions of 2026-11-07 (ADR 0068 D1 to D13; the four features) and 2026-11-11 (back in 1.2) |
| Incremental annotation (§5.6, the listing sequence) | analysers that rerun over the whole data set on demand are the usual pattern [J: no vendor document found that standardises "new since"] | about 1 to 1.5 in IX-B1 [J] | full runs only (Q-U1 (b) or (c)) | **Decided 2026-11-12, Q-U1 (c):** full runs in 1.2, the wire fields kept and always empty; nothing built |

**Summary.** The plugin, back-pressure, bounded-reply and list mechanisms are accepted ADRs with common practice behind
them; this revision does not reopen them. The stall Settings and incremental annotation were decided on 2026-11-12 with
the cheaper option (Q-U3 (b), Q-U1 (c)). The restored scope goes beyond common practice by the user's earlier decisions;
its cheaper options are §14.13.10's, offered as Q-U5.

### 17.2 Questions for the user (revision 6, decided 2026-11-12; revision 7 adds Q-U5)

**Decided by the user on 2026-11-12** (answers relayed in revision 7's brief); each is applied in the section named.

| # | Question | Decision (2026-11-12) | Applied in |
|---|---|---|---|
| Q-U1 | **Incremental annotation.** A store listing sequence so an annotator reads only what is "new since" its last run | **(c)**: full runs in 1.2; `since` and `listed_through` stay on the wire, always `None` and 0, so incremental runs can come later as a minor change. No listing sequence is built (Q28 closed) | §5.6, §17 Q28, §17.1 |
| Q-U2 | **Duplicate EVTX record ids.** A real log can repeat a record number | **(b2)**: keep every event, the event id as the tie-break, no check. The `RecordNumber` seen-set, the `import.ordinal_set_bytes` row and the problem kinds `DuplicateOrdinal` and `OrdinalUnchecked` are removed | §2.2, §7.2, plan FD-B row |
| Q-U3 | **Stall Settings.** Two Settings per call key, about 26 fields | **(b)**: the per-key values are constants measured by LV1 (rule 48); one Setting `performance.stall.patience` (Normal, Patient = ×4) | §4.4, §16, §17.1 |
| Q-U4 | **The restored scope** | **(a)**: design all of it now, in this file: the investigation-folder features and ADR 0068's checks beyond WP-1, with APIs, packages, days and ER-37's comparison | §0, §14.13, §15, §16, plan §10.6 |

Options that were offered, for the record: Q-U1 (a) build the listing sequence, (b) drop the fields, (d) per-source
generation; Q-U2 (a) skip duplicates, (b1) keep and count, (c) declare EVTX `Position`; Q-U3 (a) per-key Settings, (c) no
Setting; Q-U4 (b) design before each first package, (c) move it back out.

**Open (revision 7).** Not decided here; the user decides.

| # | Question | Options | Recommendation |
|---|---|---|---|
| Q-U5 | **Cheaper options for the restored scope** (ER-37, §14.13.10). Each function has a common-practice option that costs less | (a) build as designed, 116 raw plus 13 for PT-1 and PT-2; (b) take one or more of §14.13.10's cheaper options: citations with one UTC time (saves about 2), report blocks as headings in text (about 2.5), sequence view as a thread filter plus Mermaid text (about 3), keep today's one-request narrative (about 3), dump checks without Debrief's own unwinder (12), "Open in WinDbg or Visual Studio" instead of native checks (about 75), link out to Visual Studio or `dotnet-dump` for .NET (about 17.5) | **(a)**: the user decided each function with its size on 2026-11-07 and brought all of it back on 2026-11-11; if the end date must move, the first candidate is the unwinder (WP-3, 12 days), whose loss weakens hang verdicts on unsymbolised dumps but leaves every check working |
| Q-U6 | **.NET and `paddock-tools`.** WP-12 needs a part of ADR 0051's `paddock-tools` (pins, download and verify, Authenticode), which ADR 0051 prices at 42 to 57 days for the whole crate, after the releases, and which exists on no branch | (a) build the subset in 1.2 as PT-1 and PT-2: **+13 raw** (about 15.6 with the buffer, about 3 working days later), .NET ships in 1.2; (b) **.NET conditional**: 1.2 keeps WP-0 (3) only; WP-12 starts only if `paddock-tools` lands on its own schedule by a date the user sets (suggested 2027-03-31, about T+100 [J]); otherwise WP-12 moves to the next release, and 1.2's checks show the managed part as Not checked with "needs the .NET helper (next release)". Saves 13 raw if the crate lands on its own schedule, 28 (13 + WP-12's 15) if it does not | **(a)**, matching the user's "build everything" (Q-U5 (a)); (b) is the cheaper option, and the right one if ADR 0051's own schedule is expected to deliver the crate before the date |

### 17.3 What revision 6 changed

Header (revision line, evidence heads); §0 (restored scope, index row); §4 preamble (ADR 0070 on PH1 and LV1); §4.1
(I4, I7 folded); §4.2 (`Failure::Stall` carries `StallCause`); §4.3 (rewritten as built, I1 to I9; frame v3 and Dyno
1.1.0's version-2 helper service); §4.4 (rewritten on `paddock-proc`; `CallPatience`; no `ai.connect`); §4.5 (no
`AiWorker`); §4.6 (rewritten as built, L1 to L11, 1.4b); §5.1, §5.4, §5.5, §5.7, §3.2's and §3.8's references, §6.4's
port comment (sniffs moved); §5.10 (new, `debrief.sniff.v1`); §8 (no tee, recomputed; MEM-2 prerequisite); §13.2 (L12);
§13.3 (the coverage guard, not a lint); §14.2 (CV1 to CV4); §14.3 (`define*` rows, M1 selection type, 500 kB target,
contributions lint not built, `coverage-guard.spec.ts` moves, WG0, assistant row); §14.5 (no tee, IX packages);
§14.8 (MEM-2); §14.12 (restored rows moved); §14.13 (new stub); §15 (rows); §16 (tee row gone, stall row, thread
Settings owners); §17 (Q3, Q6, Q27, ADR notes; §17.1 to §17.3 new). After round 2: §4.5's dependency edges, Q-U1's
option (d), Q-U2's (b1)/(b2) split, §17.1's `define*` row. The plan `debrief-1-2.md` landed with this file
carries a landing note on its totals.

### 17.4 What revision 7 changed

**Round 2 (same revision, after the independent review of `72417e82c`):** WP-RR on MEM-2's `Ticket` (no FD-G type);
PT-1 and PT-2 priced (the `paddock-tools` and `paddock-deps` subsets) and Q-U6; IF-3 on §2.5's `ProcessInstance` with the
run split moved into §2.5 (FD-P; `run`, `RunByMarkerOnly`, `RunDiffers`); §1.7, §2.2's wording, §5.9's rows, Q5 closed,
the ADR 0031 note; IF-2's `DraftProvenance`, `generated` and the SVG choice; IF-4's two-count fit and cancel token;
IF-5 and IF-6 confirmed by the user; IF-3 after M3b; ADR 0068's test homes; ER-37's drafting source. Plan: §9.4's waves,
§10.1's WP-1 and FD-P rows, §10.6.

Revision 7, first round: header (revision line, evidence heads); §0 (restored scope designed, index row); §2.2 (Q-U2 (b2)); §4.4 (Q-U3 (b)); §5.6
and its `ListedSince` comment (Q-U1 (c)); §7.2 (ordinals); §14.13 (the stub replaced by §14.13.1 to §14.13.10); §15
(rows); §16 (stall row, nine new Settings, the restored rows' pointer); §17 (Q28 closed; §17.1 rows and summary; §17.2
decided and Q-U5; this list); §18. The plan `debrief-1-2.md` gains §10.6 and new totals.

## 18. Completeness

**Complete** (signatures, entry points, names, Settings, test homes): §2 ids and references, §3 streaming import, §4
the shared platform, §5 the `debrief.*` namespace (Q10 resolved), §6 folder open, §9.6 job ends, §14.1 lists,
§14.2 timeline with the measurement plan, §14.4 UI on foundation types, §14.6 analyses at scale (the API FD-F's ADR
fills), §14.7 correctness, §16 Settings, §15 the package index.

**Complete at the level of placement and contract, deferring to the accepted ADR or plan for detail:** §7 store, §8
memory bounds, §9 jobs (with §9.5's executor), §10 time model, §11 findings, §12 logs, §13 engine-to-UI contract, §14.3
the shell move (workbench.md and the move plan own the API), §14.5 store performance (internal; port changes given),
§14.8 memory (the MEM-6 ADR fills the budget API), §14.9 names, §14.10 family obligations (each ADR owns its API),
§14.11 rule 47.

**Scope not found in the sources read, so not designed:** ST-6, Y11's sticky field promotion, Y14's R3 split (Q18).

**Restored to 1.2, designed in revision 7:** §14.13 (signatures, homes, Settings, limits, logs, jobs, test homes, work
packages with days and acceptance, ER-37's comparison). Its prerequisites that are not built are listed in its table:
ADR 0052 WP-B1, MEM-2's landing (L-0), and the parts of ADR 0051's `paddock-tools` and ADR 0028's `paddock-deps` that
WP-12 needs, priced as PT-1 and PT-2 (Q-U6 offers the cheaper conditional option).

**Not verified:** nothing was compiled; signatures were checked against the source files named in the header, not
built. The listing sequence of §5.6 is not built in 1.2 (Q-U1 (c)). The drawing research's numbers are from
Playwright's browsers, not the real webviews; CV1 settles that.
