# Debrief 1.2 API design (package 0.5): the foundation

- Status: **Proposed**, 2026-11-07, for an independent review before wave 1 starts (plan row 0.5).
- Scope, by the user's decision of 2026-11-07 (relayed by the coordinator): **Debrief 1.2 is the foundation release**;
  features come one by one in later releases. This file designs the **foundation APIs fully** and, for feature packages,
  names only the **extension points** they will need, without designing their APIs. Section 0 lists which plan packages
  were treated as feature.
- The dialogs part is [debrief-1-2-api-dialogs.md](debrief-1-2-api-dialogs.md) (ADR 0026, accepted 2026-10-28), unchanged.
- Base: `docs/debrief-1-2-api` from `origin/docs/debrief-1-2-plan-refresh` `abac43c0c` (which contains
  `origin/debrief/1.2-int` `852b35ac0`). No `origin/docs/debrief-1-2-api` existed before this branch.

**Read for this design** (all on the named commits; nothing was built or run):

| Input | Where |
|---|---|
| Plan, rows of waves 0-9, §9 refresh | [debrief-1-2.md](debrief-1-2.md) at `abac43c0c` |
| Family ADRs 0001 (platform, wire, streamed calls), 0003 (Debrief's move, protocol-1 mapping), 0031 (patience) | `docs/adr/` on this branch |
| Debrief ADR 0064 (one streaming import trait, Accepted 2026-11-06) | `origin/docs/debrief-streaming-trait` `ed84adfe6` |
| Debrief ADRs 0065 (bounded replies, Accepted r5) and 0066 (how times are read, Accepted r5) | `origin/docs/debrief-bounded-and-timezone` `df3f58a5b` |
| Debrief ADR 0067 (folder import dialog, Accepted r5) | `origin/docs/debrief-folder-import-dialog` `0242fcd81` |
| Debrief ADR 0068 (dump checks, Accepted 2026-11-07) | `origin/docs/debrief-dump-analysis-adr` `1ba8e4ecd` |
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

0. [Scope: foundation and feature packages](#0-scope-foundation-and-feature-packages)
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
14. [Extension points for feature packages](#14-extension-points-for-feature-packages)
15. [Package index: which section each package cites](#15-package-index-which-section-each-package-cites)
16. [Settings introduced or touched](#16-settings-introduced-or-touched)
17. [Open questions, corrections, recommendations](#17-open-questions-corrections-recommendations)
18. [Completeness](#18-completeness)

## 0. Scope: foundation and feature packages

**Foundation** is what later features stand on and what would be expensive to change after release: the import path,
the plugin platform and its wire, folder open, the store's identities, memory bounds, jobs, the time model, ids and
references, the finding type, logs and the engine-to-UI contract (the roadmap's layers A to E and G to L). These are
designed in full in §2 to §13.

**Feature** is a user-facing capability built on that foundation. For these, §14 names the extension point (a reserved
namespace, a port method shape, a type it must use) and nothing more.

| Treated as **feature** (extension point only, §14) | Why |
|---|---|
| 2.1, 2.7 virtual primitive and its adoption; 1.3 list identity; 4.5 Debrief's lists; 3.7 grid and Hotspots | UI components; they consume `Bounded` (§13.3) and keyed rows (§2.1) |
| 2.4 lane canvas and Lanes picker; 3.3 timeline engine; 4.1 timeline UI; the drawing build | Timeline feature; §14.1 gives the canvas vs DOM measurement plan the plan row asks for |
| M1 to M5, Y12 (shell and workbench move) | Presentation move; the shell contract is ADR 0026 and workbench.md, not this file |
| 1.1, 1.2, M2b (dialogs) | Designed in [debrief-1-2-api-dialogs.md](debrief-1-2-api-dialogs.md) |
| 2.3 `LimitNotice` UI, B2 `pd-limit-note` | UI of a foundation type; the types are §4.6; the components are named in §14.4 |
| 3.2 cap UI, 4.4 plugin manager UI (its UI half) | UI over §4.2's status and §4.5's process table |
| ADR 0068 dump checks (WP-0 to WP-18 except WP-1's contract), ADR 0069 UMDH series | Features; their shared type is §11 and their wire room is §5.9 |
| Citations, report blocks, Pitwall drafting, thread/sequence view, analyses at scale (FD-F build), N0 to N8, Y1 to Y16, MEM-6 build, ST-*, Q-*, IX-*, SR-*, P2 to P5, NR1, CPU-1, NB-1 | Features or performance work that uses foundation types unchanged; §14 lists what each must use |
| 4.2's Limits group in Settings, 3.6's Help ▸ Limits page | UI; the registry is §4.6 |

**Foundation packages this file serves** (each cites its section in §15): 0.4, 1.4 + B1, 2.2, 2.5 (rest), 2.6a to 2.6d,
IP1, PH1, PH2a, PH2b, PH3, PH4, 3.4, 3.5, 3.6 (registry rows), 4.2 (port shapes), J1, BP1, LV1, LV2, P1, SI-0 to SI-18,
MEM-3, MEM-4, F1 to F12, FM, FR, B3 to B8, Z0 to Z13, FD-U, FD-B, FD-P, FD-E1, FD-E2, FD-I, FD-K, WP-SL1, D-a, D-b,
ADR 0068 WP-1 (the contract half).

**Priority order used** (the user's): streaming import and folder open first: §3, §4, §5, §6 were written to completion
before §7 to §13. §18 says what is complete.

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

Postcard rules (family ADR 0001, "Versioning"): only a variant appended at the end of an enum is a minor change; a field
appended to a struct is a new `.vN` namespace. **So every field a foundation struct may ever need must be in its first
version**, which is why §5's bodies carry fields whose producers land later (`ordinal`, `time_report`); they are
`Option` or have a defined empty value until then.

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
validate it: thread counts (always with "Auto"), memory shares, limits a person can raise, stall thresholds per key,
display choices. It is a **hard limit** (registry row marked `hard`) when raising it would break a guarantee (frame size,
line limit, entry count). It is a **constant** (not in the registry) when it is a heuristic about data with a test on
each side of the edge (ADR 0066's `BACKWARDS_TOLERANCE`). It is **data** (a TOML pack, rule 21) when it is knowledge about
formats (`importers/memory.toml`, `recommend.toml`, `windows-zones.toml`). All new Settings are listed in §16, each with
its path, default, bounds and owner package.

## 2. Ids, ordinals, evidence references, process instances

Roadmap layer E and B (FD-U, FD-B, FD-P, FD-E1, FD-E2). These come first because the streaming wire (§5) and the folder
and store APIs (§6, §7) carry them, and because a `u32` id built into citations or report blocks would be undone later.

### 2.1 `EventId`, `SourceId`: the declared bound (FD-U)

**Decision recommended [J], pending ADR 0063:** ids stay `u64` in Rust and plain `number` in TypeScript, under a declared
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
/// What orders the events of one source. Declared by the importer; `Line` is the default.
pub enum OrdinalKind {
    /// The 1-based line of the event's first line (text formats).
    Line,
    /// A number the format itself carries and that is unique per source (an EVTX record id).
    RecordNumber,
    /// The position of the record in the file (a CSV row, an OTLP record index, a video frame): counted by the reader.
    Position,
}
```

- The event carries `ordinal: Option<u64>` on the wire (`WireEvent`, §5.3) and in `ParsedEvent` (§3.2). `None` under
  `OrdinalKind::Line` means "the line is the ordinal". The store's column is FD-B's decision (a new `ordinal UBIGINT`, or
  `line` widened); the wire carries the field from v1 either way (§1.4).
- **Uniqueness is checked at the sink**, per source, on a rolling window: an ordinal that is not greater than the last one
  of the same source is a `ProblemKind::OrdinalNotIncreasing` (counted, sampled), never an error. A format whose
  record numbers may repeat (a wrapped EVTX log restarts numbering only on clear [U]) declares `Position` instead.
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
- **Room left for features:** `DocItem` is an enum on JSON wires only (UI, store worker), so a variant appended later is
  minor. It never crosses the plugin wire.
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
pub enum UnconfirmedWhy { CreateTimeMissing, ImageMissing, MachineUnknown }
pub enum DifferentWhy { PidDiffers, CreateTimeDiffers, MainModuleDiffers, MachineDiffers }
```

- ADR 0068's `Pairing` and its reason codes (`PidDiffers`, `CreateTimeDiffers`, `CreateTimeMissing`,
  `MainModuleDiffers`, `Same`) [A: ADR 0068 §14] **are** this type; WP-6 uses it and does not define another. The
  earliest thread create time as a second witness (ADR 0068 §5) is a feature-side input to `same_process`'s caller.
- Tests: `debrief-core/src/process/tests.rs`, a table of pairs per reason, and a property test that `same_process` is
  symmetric.

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
    sink.rs         EventSink, ParsedEvent, Raw, SinkClosed, ProblemKind (re-export of problems)
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
pub trait Importer: Send + Sync + 'static { /* exactly ADR 0064 §1, plus: */
    /// What orders this importer's events (§2.2). Default `Line`.
    fn ordinal(&self) -> OrdinalKind { OrdinalKind::Line }
    /// Whether the pool may split the file and call `import` once per chunk (core text and JSON lines only in 1.2).
    fn splittable(&self) -> bool { false }
    /// ADR 0067's preselection class (`log`, `event-log`, `dump`, `trace`, `config`, `image`, `untimed-table`).
    fn class(&self) -> ImportClass;
}

pub struct ImportCtx<'a> {
    file: &'a FileInfo,                // §5.3: name, root-relative path tail, size, mtime; never an absolute path
    head: &'a Head,                    // the plan's head, ≤ HEAD_BYTES (1 MiB), decoded once for text importers
    plan: &'a TextPlan,                // encoding, data_start
    times: RefCell<TimeReader>,        // ADR 0066 §1; finish() called by the SDK, never by the importer
    tail: Option<&'a TailStart>,       // live tail only: the file's start (headers)
    progress: &'a dyn ByteProgress,    // §3.6: bytes delivered after the first event
    log: &'a dyn ImportLog,            // §12.2: phase lines, never content
}
impl ImportCtx<'_> {
    pub fn file(&self) -> &FileInfo;
    pub fn head(&self) -> &Head;
    pub fn times(&self) -> RefMut<'_, TimeReader>;
    pub fn tail(&self) -> Option<&TailStart>;
}
pub struct SniffCtx<'a> { pub file: &'a FileInfo, pub encoding: Option<&'a str> }
pub struct Head { bytes: Box<[u8]>, text: Option<Box<str>>, complete: bool }   // `complete`: the head is the whole file
impl Head { pub fn bytes(&self) -> &[u8]; pub fn text(&self) -> Option<&str>; pub fn is_whole_file(&self) -> bool }

pub enum DeclaredSniff {
    /// Decided from the manifest: extensions (lower-case, no dot) and magic bytes at offsets. No process starts.
    Static { extensions: Vec<Box<str>>, magic: Vec<Magic>, score: u8 },
    /// `Importer::sniff` is called (one batched call per plugin per file, §5.4).
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
file.

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
    pub class: ImportClass,
    pub identity_fields: Vec<Box<str>>,
}
/// Type-erased importer: what the host calls. Built only by `register`, so `access` cannot disagree with the code.
pub struct ErasedImporter { cap: ImporterCap, run: Box<dyn ErasedRun> }
pub fn register<I: Importer>(importer: I) -> ErasedImporter;
impl ErasedImporter {
    pub fn cap(&self) -> &ImporterCap;
    pub fn sniff(&self, ctx: &SniffCtx, head: &Head) -> u8;
    /// The host passes the input matching `cap().access`; a mismatch is a programming error caught by the type of `HostInput`.
    pub fn import(&self, ctx: &ImportCtx, input: HostInput<'_>, sink: &mut dyn EventSink) -> Result<ImportEnd, ImportError>;
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
    Refused(Refusal),                         // §6.2: same enum as the scan
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
`Credit` frame goes back; `ItemSink::send` blocks in the plugin; `EventSink::event` blocks; the parser stops reading.
Cancel is the import job's `CancelToken` (§9), reaching `RemoteStream::cancel` and `EventSink::event` →
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

Unchanged from ADR 0064's section [A]: files on the parse pool (`import.parse_threads`, Auto = cores − 1, at most 8),
`2 × threads` files in flight, lowered on `EMFILE`; plugin calls bounded by files in flight; zip members one cloned
reader each; one spool per file in flight. Cheaper: sniff memo and batch (§5.4), first read grows from 64 KiB, one open
per file, `Vec<Record>` gone.

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

### 4.1 `paddock-plugin-api` additions (2.6b, SI-7, IP1 item 7)

```rust
// extension.rs
pub trait Contract: 'static {
    const NS: &'static str;
    const MINOR: u16;
    type Request: Serialize + DeserializeOwned;
    type Reply: Serialize + DeserializeOwned;
    /// Rule 52 role of a request (default Live). `serve` runs Bulk at the gentle level, Live at normal level.
    fn role(_request: &Self::Request) -> WorkRole { WorkRole::Live }
    /// The minor that introduced `request`'s variant; the host refuses to send it to an older peer (IP1 item 4).
    fn introduced_in(_request: &Self::Request) -> u16 { 0 }
}
pub enum WorkRole { Live, Bulk }

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

// lib.rs (IP1 item 7): what a plugin needs from paddock-ipc, so it depends on paddock-plugin-api alone
pub use paddock_ipc::{codes, RemoteError};
impl ExtCtx<'_> { pub fn cancelled_error(&self) -> RemoteError; }
```

`serve` (2.6b): sets the thread level per call from `Contract::role` (`paddock_proc::priority::for_role`, §4.5); calls
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
pub struct Credit(NonZeroU32);                         // HostConfig::stream_credit, "Auto" = 4 until 2.6b's benchmark

// services.rs: host-side extension calls
pub trait HostExtension: Send + Sync {
    const NS: &'static str;  // checked against the manifest's declared namespaces
    fn handle(&self, plugin: &PluginId, minor: u16, body: &[u8], ctx: &HostCallCtx) -> Result<Vec<u8>, RemoteError>;
}
impl HostServices {
    pub fn register_extension(&self, ns: &'static str, handler: Arc<dyn HostExtensionDyn>);
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

// status.rs (2.6a): lifecycle
pub enum StartPolicy { OnFirstUse, AtStartup }
pub enum Lifecycle {
    Stopped,
    Starting { since_tick: u64 },
    Idle { pid: ProcessId, since_tick: u64 },
    Busy { pid: ProcessId, in_flight: u16 },
    Stopping { pid: ProcessId },
    Quarantined { until: QuarantineUntil },
}
pub struct PluginStatus { pub id: PluginId, pub lifecycle: Lifecycle, pub last_use: Option<u64>,
                          pub memory: Option<MemorySample>, pub cpu: Option<CpuSample>, pub keep_alive: bool }

// failure (2.6a)
pub enum Failure { Panic, Died, Hang, Garbled, OverMemory { limit: LimitExceeded }, Stall { why: StallWhy } }
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

```rust
pub struct PeerOptions {
    pub max_concurrent_calls: u16,          // exists, 64
    pub max_queued_frames: NonZeroU32,      // IP1 item 2: the writer queue bound; a full queue blocks the sender
    pub max_queued_bytes: NonZeroU64,       // and its byte bound
}
pub enum ProtocolError { RepeatedCallId { id: u64 }, ItemWithoutCredit, /* ... */ }  // IP1 item 3: closes the connection
pub trait PeerCredentials { fn peer_uid(&self) -> Option<u32>; }                     // IP1 item 5, Unix; fake in tests
// frame.rs (2.6d): VERSION = 3; Hello and Welcome end with `caps: u32`; bit 0 = progress. Version-2 fallback as ADR 0001.
pub const CAP_PROGRESS: u32 = 1 << 0;
```

An oversized reply from a plugin is the plugin's fault (`Failure::Garbled`), not the host's request (IP1 item 1).

### 4.4 Patience (2.6c, 2.6d, LV1)

`paddock-jobs::patience` exactly as family ADR 0031 §5 shows (`CallKey`, `Stall`, `Patience`, `TickSource`,
`RunStateProbe`, `RunSample`, `RunState`, `StallJudge`, `HostTickClock`) [A], with two additions:

```rust
/// What the host owes a call: while any is true, the call gets no blocked ticks (ADR 0031 §2, amended by ADR 0064:
/// waiting for credit is owed by the host).
pub struct Owed { pub host_call: bool, pub file_read: bool, pub credit: bool }
impl StallJudge { pub fn observe(&mut self, key: &CallKey, tick: u64, sample: Option<RunSample>, progress: bool, owed: Owed) -> Verdict; }
pub enum Verdict { Working, Waiting(WaitWhy), Stuck(StuckWhy) }
```

Call keys used by the foundation (C measured per key in LV1, recorded under rule 48): `plugin.start`,
`plugin.describe`, `plugin.sniff`, `debrief.import`, `debrief.tail`, `debrief.document`, `debrief.profile`,
`debrief.annotate`, `worker.start`, `worker.call`, `ai.connect`, `import.busy_wait`, `tail.wait`, `window.ready`.
Settings: `performance.stall.<key>.blocked_ticks` and `.spin_cpu_s`, Auto (§16).

`RunStateProbe::sample(pid: u32)` takes a bare pid; see §17, Q6.

### 4.5 `paddock-proc` and `paddock-metrics` (2.2, P1, BP1)

```rust
pub enum MemoryPolicy { Cap { bytes: u64 }, Unlimited { machine_floor: u64 } }
impl MemoryPolicy { pub fn ceiling(physical: u64) -> u64; }        // 75 % of physical memory
pub trait ProcessProbe: Send + Sync { fn sample(&self, id: &ProcessId) -> Option<ProcessSample>; }
pub trait SystemProbe: Send + Sync { fn physical(&self) -> u64; fn available(&self) -> u64; }
pub struct ProcessSample { pub footprint: u64, pub cpu: Duration, pub state: Option<RunState> } // footprint per rule 26
/// One refresh for all watched pids per tick (P1). Rows by role, versioned camelCase wire type, generated TS.
pub struct ProcessTable { pub version: u32, pub rows: Vec<ProcessRow> }
pub struct ProcessRow { pub role: ProcessRole, pub id: ProcessId, pub footprint: u64, pub cpu_percent: f32 }
pub enum ProcessRole { App, StoreWorker, AiWorker, Plugin { id: Box<str> }, Helper { id: Box<str> } }

pub mod priority {
    pub fn for_role(role: WorkRole);                 // sets the calling thread's level (rule 52)
    pub fn threads(kind: WorkKind, class: Class) -> NonZeroUsize;   // ADR 0024 §1
}
```

### 4.6 `paddock-limits` (1.4 with B1; answers plan D5)

ADR 0019's half (1.4) and ADR 0065's half (B1) in one crate [A: ADR 0065 D1]. A leaf crate: serde, `ts-rs` behind
`ts`, no `paddock-ipc`.

```rust
pub struct LimitId(&'static str);                          // "import.record_bytes"; dotted, lower-case
pub enum Quantity { Bytes(u64), Count(u64), Ratio(u32), Millis(u64), Percent(u8) }
pub struct LimitExceeded { pub id: LimitId, pub limit: Quantity, pub reached: Option<Quantity>, pub scope: Scope }
pub enum Scope { File, Source, Import, Plugin, Query, Process }
pub enum Remedy { RaiseSetting { path: SettingPath }, ShowMore, Narrow, Retry, None }
pub struct LimitRow {
    pub id: LimitId, pub unit: Unit, pub default: Quantity, pub min: Quantity, pub max: Quantity,
    pub value: RowValue,                                   // Setting(SettingPath) | Hard | ByRequest
    pub kind: RowKind,                                     // Stop | Cut | Both  (ADR 0065 §2)
    pub selections: &'static [SelectionKind],              // for Cut rows
    pub remedy: Remedy,
}
pub struct Registry(&'static [LimitRow]);
// Bounded, Cut, Total, Selection, SortKey, FieldPath, Next, CutRow<S>: exactly ADR 0065 §1 [A].

// Request-side types that plan 2.5 keeps (plan D5: here, not in paddock-view, which pulls in paddock-ipc and postcard):
pub struct Limit(NonZeroU32);
pub struct Page { pub offset: u64, pub limit: Limit }
pub struct Buckets(NonZeroU16);
```

`limit_case!` (stop and cut forms) in `paddock-testkit/src/limits.rs`; Debrief's rows in `debrief-core/src/limits.rs`
(3.6 and B3, one file); `core/generated/limits.ts` from the registry.

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
| `Sniff { importer, file, head }` | `debrief.import.v1` `Sniff` (batched per plugin per file) | unary | Live | `SniffScores` |
| `Import { importer, file, content }` | `debrief.import.v1` `Import` | stream of `EventItem` | Bulk | `ImportDone` |
| `ImportTail { importer, file, head, tail }` | `debrief.import.v1` `Tail` | stream of `EventItem` | Live | `TailDone` |
| `SniffConfig { parser, file_name, head }` | `debrief.config.v1` `Sniff` | unary | Live | `u8` |
| `ParseConfig { parser, content, limits }` | `debrief.config.v1` `Parse` | unary | Bulk | `ParseConfigReply` |
| `Annotate { annotator, ticket, since }` | `debrief.annotate.v1` `Annotate` | unary | Bulk | `AnnotateReply` |
| `Transform { transform, source_name, rows }` | `debrief.transform.v1` `Transform` | unary | Bulk | `TransformOut` |
| `SniffProfile { importer, file_name, head }` | `debrief.profile.v1` `Sniff` | unary | Live | `u8` |
| `ImportProfile { importer, file_name, content, limits }` | `debrief.profile.v1` `Import` | stream of `Piece` | Bulk | `ProfileDone` |
| `SniffDocument`, `ImportDocument`, `ResymboliseDocument` | `debrief.document.v1` `Sniff`, `Import`, `Resymbolise` | unary; streams | Live; Bulk | `u8`; `DocumentDone` |

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
    type Request = DescribeRequest; type Reply = DebriefContributions; }
pub enum DescribeRequest { Contributions }
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
    pub times: TimeSettings,            // the parser's assume_timezone etc., `Inherit` by default (ADR 0066 §2)
}
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
    pub ts_kind: WireTsKind,                        // Parsed | Inherited | Untimed (TsState), plus Wall (ADR 0066 §2)
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
pub enum WireTsKind { Parsed, Inherited, Untimed, Wall }
pub const MAX_EVENTS_PER_ITEM: usize = 8192;
pub const ITEM_BYTES: usize = 4 << 20;
pub const EVENT_FRAME_MAX: usize = 12 << 20;        // over it the SDK drops `raw`, then the event, with EventTooLarge
```

`WireEvent` loses `id` (§2.1) and gains `ts_kind`, `ordinal`, `raw` and `byte_offset`. `ts_kind = Wall` replaces the
need for a later field when ADR 0066's `Stamp::Wall` reaches the store; until Z1 lands, plugins send `Parsed` [J].

### 5.4 `debrief.import.v1`

```rust
pub struct Import;
impl Contract for Import { const NS: &'static str = "debrief.import.v1"; const MINOR: u16 = 0;
    type Request = ImportRequest; type Reply = ImportReply;
    fn role(r: &ImportRequest) -> WorkRole { match r { ImportRequest::Import { .. } => WorkRole::Bulk, _ => WorkRole::Live } } }
impl StreamContract for Import { type Item = EventItem; }

pub enum ImportRequest {
    /// One call per plugin per file: a score per named importer, in order. `head` ≤ 64 KiB raw bytes.
    Sniff { importers: Vec<Box<str>>, file: FileInfo, head: Vec<u8>, encoding: Option<Box<str>> },
    /// Items: EventItem. Reply: ImportReply::Done. Bulk.
    Import { importer: Box<str>, file: FileInfo, input: WireInput, plan: TextPlan },
    /// Items: EventItem. Reply: ImportReply::TailDone. Live. `head` ≤ HEAD_BYTES, inline.
    Tail { importer: Box<str>, file: FileInfo, head: Vec<u8>, input: WireInput, plan: TextPlan },
}
pub enum ImportReply { Scores(Vec<u8>), Done(ImportDone), TailDone(TailDone) }
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
- **Sniff batching** (SI-6): the host memoises `format_score` per file per importer and sends one `Sniff` per plugin per
  file with every importer of that plugin whose sniff is `Dynamic` and whose parser rules passed.

### 5.5 `debrief.config.v1`

```rust
pub enum ConfigRequest {
    Sniff { parser: Box<str>, file_name: Box<str>, head: String },            // Live; → Score(u8)
    Parse { parser: Box<str>, content: HostFileRef, limits: ConfigLimits },  // Bulk; → Parsed(Result<ConfigTree, ConfigError>)
}
pub enum ConfigReply { Score(u8), Parsed(Result<ConfigTree, ConfigError>) }
pub struct ConfigLimits { pub max_bytes: u64, pub max_depth: u16, pub max_nodes: u32 }    // 64 MiB today [S]
```

Configuration parsers stay whole-reading under their 64 MiB cap (ADR 0064 D1, "leave them out") [A].

### 5.6 `debrief.annotate.v1` and `debrief.transform.v1`

```rust
pub enum AnnotateRequest { Annotate { annotator: Box<str>, ticket: u64, since: Option<RecordCursor> } }   // Bulk
pub struct AnnotateReply { pub annotations: Vec<Annotation>, pub dropped: u64 }   // at most MAX_ANNOTATIONS (existing)
pub struct RecordCursor { pub after: EventId, pub ts: Option<i64> }

pub enum TransformRequest { Transform { transform: Box<str>, source_name: Box<str>, rows: Vec<WireEvent> } } // Bulk
pub struct TransformOut { pub rows: Vec<WireEvent>, pub keep: Vec<bool>, pub marks: Vec<(u32, Box<str>)>, pub notes: Vec<Box<str>> }
```

`RecordCursor` is `{ id: u32, ts }` in protocol 1 (`debrief-plugin-api/src/annotate.rs:21`) [S]; v1 widens the id to
`EventId` (§2.1). An id is a fair cursor here: the host makes it, within one store, where ids are never reused and are
allocated in increasing blocks [S: `ids.rs`], so "after" means "imported since". It is not stable across a re-import
[A: ADR 0062], and an annotation run never spans one.

### 5.7 `debrief.profile.v1` and `debrief.document.v1`

```rust
pub enum ProfileRequest {
    Sniff { importer: Box<str>, file_name: Box<str>, head: Vec<u8> },                       // Live → Score(u8)
    Import { importer: Box<str>, file: FileInfo, input: WireInput, limits: ProfileLimits }, // Bulk, items: Piece → Done(ProfileDone)
}
pub enum DocumentRequest {
    Sniff { importer: Box<str>, file_name: Box<str>, head: Vec<u8> },
    Import { importer: Box<str>, file: FileInfo, input: WireInput, options: DocumentOptions },
    Resymbolise { importer: Box<str>, document: HostFileRef, options: DocumentOptions },
}
pub struct Piece { pub bytes: Vec<u8> }            // ≤ OUTPUT_CHUNK (256 KiB), the library's own encoding
pub struct ProfileDone { pub bytes: u64, pub heap: bool }
pub struct DocumentDone { pub bytes: u64 }
```

`ProfileLimits` and `DocumentOptions` become typed structs (they are `Json` in protocol 1 [S]); their fields are copied
from the JSON shapes the plugins read today, in PH2a [J].

### 5.8 `debrief.view.v1` (host side)

```rust
pub enum ViewRequest {
    Select { ticket: u64, filter: ViewFilter, limit: u32 },     // → Events(Bounded rows as EventItem, cut count)
    Instances { ticket: u64 },                                  // → Instances(Vec<InstanceRow>)
    TimeRange { ticket: u64 },                                  // → Range(Option<(i64, i64)>)
}
pub enum ViewReply { Events { item: EventItem, more: bool }, Instances(Vec<InstanceRow>), Range(Option<(i64, i64)>) }
```

Served by Debrief as a `HostExtension` (§4.2) during an annotation, under its ticket; a ticket not open is `denied`.
`ViewFilter` replaces protocol 1's `filter: Json` with the subset of `Query` that annotators use [U: the subset is listed
by PH2a from the first-party annotators].

### 5.9 Room left for features

| Namespace (reserved, not designed) | For | Rule |
|---|---|---|
| `debrief.dumpquery.v1` | ADR 0068 §7's `DumpQuery` range reads | Its own namespace so `debrief.document.v1` does not change; lends ranges through `HostFiles::lend_ranges` (§4.2) |
| `debrief.analysis.v1` | plugin analysis contributions (roadmap layer C, "later") | Its reply is §11's `Finding`; registered as a `kind = "extension"` contribution |
| a variant appended to any request enum above | a new operation of an existing namespace | Minor; refused before sending to an older peer (`introduced_in`, §4.1) |

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
pub fn judge<'a>(env: &JudgeEnv<'a>, file: &FileMeta, head: &Head, whole: bool) -> Result<Judged<'a>, Refusal>;
pub struct Judged<'a> { pub choice: Choice<'a>, pub plan: FilePlan, pub limits: Vec<JudgedLimit> }
pub enum JudgedLimit { NoTime, ZoneUnknown, DateFromFileTime, DayOrderUndecided,
                       OverMemory { side: MemorySide, need: u64, can_give: u64, measured: bool }, OverLimit(LimitExceeded) }
pub enum MemorySide { Plugin, StoreWorker }
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    Empty, Binary, NoParser, NoImporter, NoFormat,
    PluginStopped { plugin: Box<str> },
    Locked { detail: Box<str> }, Unreadable { detail: Box<str> },
    // archive members (§6.9)
    Encrypted, Damaged, TooManyEntries, TooLarge, Nested,
}
pub enum Status { Pending, Supported, Limited, NotSupported, CantRead }
pub fn status_of(r: &Result<Judged, Refusal>) -> Status;          // pure, the table of ADR 0067 §2
```

`Refusal`'s `Display` gives today's strings [A]; the UI words come from a key per variant (`refusal.<kebab>`), F9b.
`choose` and `plan_with` become `pub(crate)`; F1b's conformance check fails on another whole-file caller.

### 6.3 The table and view, pure (F3)

```rust
// debrief-core/src/folder_scan/table.rs   (no I/O)
pub struct ScanTable { /* flat arrays: parent, name arena offset, kind, status, reason, size, files, selected, check, children */ }
pub struct NodeId(u32);
pub struct Version(u64);
impl ScanTable {
    pub fn from_listing(listing: impl Iterator<Item = ListedEntry>, limit: &CutRow<First>) -> Bounded<ScanTable>;
    pub fn set_verdict(&mut self, node: NodeId, v: Verdict) -> Version;
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

`Verdict` is `{ status, class, reason: Option<ReasonKey>, limits }`; `ReasonKey` is a typed enum (Refusal kinds, limit
kinds, `LikelyDuplicate { of: NodeId }`, `SameContent { of: NodeId }`, `NoRule`, `ProjectData`).

### 6.4 The scan job (F4)

```rust
// debrief-core/src/folder_scan/run.rs
pub struct ScanSpec { pub roots: Vec<GrantedRoot>, pub threads: ThreadCount, pub head_bytes: u32 }
pub struct ScanHandle { pub scan: ScanId, pub job: JobId }
pub trait ScanPorts: Send + Sync {                  // adapters: fs, plugins, the zone probe; fakes in tests
    fn list(&self, root: &GrantedRoot, sink: &mut dyn FnMut(ListedEntry) -> ControlFlow<()>) -> io::Result<()>;
    fn head(&self, file: &FileMeta) -> Result<Head, Refusal>;
    fn sniff(&self, batch: SniffBatch) -> Result<SniffScores, PluginFault>;
    fn zone_probe(&self, table: &ScanTable, budget: ProbeBudget) -> Option<ZoneGuess>;
}
pub fn run_scan(spec: ScanSpec, ports: &dyn ScanPorts, job: &JobContext, table: &RwLock<ScanState>) -> ScanOutcome;
pub enum ScanOutcome { Finished, Cancelled, Failed(ScanError) }
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
`RelPath` is `archive!/member` (`archive::ENTRY_SEPARATOR`) [A]. Archive errors map to `Refusal::{Encrypted, Damaged,
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

- Ids: the block allocator checks `EventId::new` (§2.1); ordinals checked per source (§2.2); the source is listed with
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
| Documents (dumps, captures) | 512 MiB encoded per document; reader 3 GiB | ADR 0023 limits, ADR 0068 §12 | §14.6 |
| Open documents | `MAX_DOCS` 32 by count today | MEM-2's byte budget; MEM-6 widened (FD-G) | §14.6 |
| UI process | no import parsing in the app process (MEM-1) | conformance C<whole-file-read> over `app/src-tauri/src/{profiling,dumps}.rs` | §3.9 |

Footprint is physical memory per rule 26 (macOS footprint, Linux PSS, Windows private bytes, recorded by 2.2).

## 9. Jobs

### 9.1 Kinds and queues

Debrief's `JobKind` enum (`app/src-tauri/src/jobs/model.rs`) [S] stays the queue key; M2a maps each to
`paddock_jobs::JobKind` slugs. Added: **`Scan`** (`"scan"`, ADR 0067), one per window, does not wait behind an import.

| Kind | Slug | Concurrency | Role (rule 52) |
|---|---|---|---|
| `Import` | `import` | one per window (ADR 0012) | Bulk |
| `Scan` | `scan` | one per window | Live |
| `Analysis` | `analysis` | cores − 1 shared | Live (a person waits) |
| `Export`, `Collection`, `Download`, `Ai`, `Research` | as today | as today | as today |

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
    pub fn submit_keyed(&self, kind: JobKind, key: IdempotencyKey, start: impl FnOnce(JobContext) + Send + 'static) -> Submitted;
}
pub enum Submitted { Started(JobId), Joined(JobId) }    // two identical submits give one id
```

### 9.4 Parallelism and cost of the foundation's long operations (ER-20)

| Operation | Bound | Setting | Cheaper alternative checked |
|---|---|---|---|
| Import parse | cores − 1, at most 8 | `import.parse_threads` (Auto) | §3.8 |
| Scan judge | `import.parse_threads` local, 4 remote | same | raw-byte binary check (F1) cuts about 95 % of judge time [A, calc] |
| Plugin calls | files in flight | none (derived) | sniff memo and batch |
| Spool | one per file in flight | `import.spool_bytes` | sequential members never spool |
| Evidence resolve | one query per 1,000 refs | none | id hint first, locator second, hash last |

## 10. Time model (ADR 0066)

ADR 0066 [A] decides it. Placement and the signatures a plugin or package needs:

### 10.1 Placement

- `debrief-plugin-ext::time`: `TimestampParser` (with `parse_stamp`), `TimeReader`, `TimeTally`, `Stamp`, `Written`,
  `NativeClock`, `EventStamp`, `TimeReport`, `AssumedZone::Inherit`. No zone database (plugins do not link `jiff`).
- `debrief-core::zone` (`model`, `report`, `table` (only file calling `jiff`), `resolve`, `plan`, `detect`, `check`).
- `debrief-store-duck`: `zone_settings`, `import_roots`, `source_time` tables; `Skew` port rows (§7.1).

### 10.2 What importers see

`ctx.times()` (§3.2) returns the one reader of the call; the SDK calls `finish()` after `import` returns and puts the
`TimeReport` into `ImportDone.time` (§5.4). A splittable in-process importer gets one reader per region; tallies merge
in line order [A].

```rust
impl TimeReader {
    pub fn read(&mut self, text: &str, line: u32) -> Option<EventStamp>;
    pub fn absolute(&mut self, ms: i64, line: u32, kind: NativeClock) -> EventStamp;
}
pub struct EventStamp { pub ms: i64, pub kind: WireTsKind }     // Wall when digits only (ADR 0066 §2)
pub enum NativeClock { FileTime, UnixMs, UnixNs, Header }
```

### 10.3 Times on documents and references (roadmap layer J)

**One rule [J, from the roadmap]:** every time a foundation type carries outside the event table (a dump's create time,
a UMDH snapshot, a capture start, an evidence reference, a process start) is a `StampedTime`:

```rust
// debrief-core::zone::model
pub struct StampedTime { pub ms: i64, pub basis: TimeBasis, pub confidence: Confidence }
pub enum TimeBasis { Instant(Written), Wall { zone: ZoneReadingShort } }
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
pub struct AnyFinding { pub producer: Producer, pub kind: Box<str>, pub verdict: serde_json::Value /* validated */, pub core: Finding<()> }
pub enum Producer { DumpCheck(CheckKind), Health, Profile, Heap, Analysis(Box<str>) }

// debrief-dump::api (ADR 0068 §10), built on it:
pub struct CheckReport {
    pub kind: CheckKind,
    pub docs: Vec<CheckedDoc>,                 // CheckedDoc.process: ProcessInstance (§2.5)
    pub pairing: Option<Pairing>,              // §2.5
    pub finding: Finding<Verdict>,
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

`app/src/app/core/generated/` gains: `limits.ts` (registry ids, values, `Bounded`, `Cut`), `evidence.ts`, `process.ts`,
`finding.ts`, `folder-scan.ts`, `time.ts`; `store.ts` gets `EventId`. Shared ones (`Bounded`, `Cut`) are generated once
into `ui/projects/paddock-ui/limits/generated/` and re-exported, not copied (rule 55). CI regenerates and fails on a
diff.

### 13.3 Rules for UI code

- A service calls only its `<Area>Backend`; no component imports `ipc.ts` (existing lint).
- A view of a `Bounded` reply reads `.result` and renders `.cuts` through `<pd-limit-note>` or names its surface in
  `BOUNDED_SURFACES` (ADR 0065 part 5) [A].
- Every state is designed: loading (skeleton), empty, error with Retry, cut.
- Ids are `EventId` (`number`), never array indexes, as row keys (`rowKey`, ADR 0017).

### 13.4 ADR 0063

Nothing in this file depends on ADR 0063's outcome: `Bounded` travels in the MessagePack head under it [A: ADR 0065
§7], ids stay below 2^53 either way (§2.1). The roadmap's advice stands: decide 0063 before 3.3 and 4.5 build their reply
shapes (§17, Q1).

## 14. Extension points for feature packages

Not designed here; each entry names what the feature must build on.

### 14.1 Timeline: lane canvas and Lanes picker (2.4, 3.3, 4.1), with the measurement plan

**Extension point:** 3.3's lane endpoint is `Bounded` (top N plus a paged catalogue: `Bounded::ranked` then
`Bounded::page`); per-lane summaries take `Buckets` (§4.6); lane identity uses `ProcessInstance` where the lane is a
process (§2.5).

**Canvas vs DOM, the measurement plan (rule 48; run by 2.4 before the choice is made):**

| Step | What | Pass |
|---|---|---|
| 1 | Two prototypes in `workbench-demo`: (a) one `<canvas>` per visible lane band, drawn from bucket arrays; (b) DOM rows with one absolutely positioned `div` per bucket run, virtualized vertically | both render the same 5,000-lane mock profile |
| 2 | Scripted fling over All (5,000 lanes) and zoom in 10 steps, Chromium and WebKit, 1440 × 900 at scale 1 and 2, on the Mac mini and one 4-core laptop class | recorded: long tasks > 50 ms, p95 frame time, DOM node count, JS heap |
| 3 | Same at 60 lanes and 10 lanes (the default) | as above |
| 4 | Accessibility: keyboard to each lane and bucket, screen-reader names; axe both themes | canvas needs a parallel accessible list; its cost is counted in (a) |
| 5 | Choice rule | the DOM form wins unless it misses the frame budget (no long task at a warm start, p95 ≤ 16.7 ms) in WebKit at 5,000 lanes; ties go to DOM (less code, accessibility for free) [J] |

Results go in `docs/benchmarks.md` (shared) before any number is quoted.

### 14.2 Lists and the virtual primitive (1.3, 2.1, 2.7, 4.5, 3.7, F10b)

Rows keyed by `EventId` or `NodeId`; windowed rows (`PdRowWindow`, ADR 0067 T4) built as the first user of 2.1, not a
second implementation (roadmap stop-gap table) [A].

### 14.3 Citations, report blocks, Pitwall drafting

Use `EvidenceRef` (§2.4) and `AnyFinding` (§11); never `u32` ids.

### 14.4 `LimitNotice` and `pd-limit-note` (2.3, B2)

Render `LimitExceeded` and `Cut` (§4.6); Raise it deep-links `SettingPath`.

### 14.5 Thread/sequence view

Order by `(source, ordinal)` (§2.2); thread ids only within a `ProcessInstance` (§2.5).

### 14.6 Dump checks (ADR 0068), UMDH series (ADR 0069), MEM-6

`CheckReport` on `Finding` (§11); range reads through `debrief.dumpquery.v1` and `HostFiles::lend_ranges` (§5.9,
§4.2); times as `StampedTime` (§10.3); document memory by byte budget (MEM-6, FD-G).

### 14.7 Analyses at scale (FD-F)

Output is `Finding<_>` or `Bounded<_>` with an `analysis.events` cut; long runs are `Analysis` jobs with phases;
`Analysis run` log line (§12.2).

### 14.8 Plugin analysis contributions (later)

`debrief.analysis.v1` (§5.9), reply `AnyFinding`.

## 15. Package index: which section each package cites

| Package | Section | | Package | Section |
|---|---|---|---|---|
| 0.4 | §1.1 | | SI-0, SI-1, SI-2 | §3.1 to §3.3, §3.9 |
| 1.4 + B1 | §4.6 | | SI-3, SI-11 | §3.2, §3.3, §5.7 |
| 2.2, P1, BP1 | §4.5 | | SI-4, SI-5 | §4.8, §3.4, §6.9 |
| IP1 | §4.3, §4.1 (item 7) | | SI-6 | §5.4 (batching), §3.8 |
| 2.6a | §4.2 (grants, lifecycle, failure, sniff) | | SI-7 | §4.1 (`HostFile`), §4.2 (`HostFiles`) |
| 2.6b | §4.1, §4.2 (stream, host extensions) | | SI-8 | §5.3, §5.4, §5.7 |
| 2.6c | §4.4 | | SI-9, PH4 | §5.4, §3.3 |
| 2.6d, LV1, LV2 | §4.3 (frame v3), §4.4 | | SI-10, 3.4 | §3.4, §3.5, §4.2 |
| PH1 | §4.7, §4.9 | | SI-12, MEM-3, MEM-4 | §3.3 (`ByPath`), §5.4 |
| PH2a | §3.1, §5 | | SI-13, 3.6 | §3.7, §4.6 |
| PH2b | §4.9, §5.8, §3.4 | | SI-14 | §3.9 |
| PH3 | §4.9 (installed) | | SI-15, SI-16 | §3.9, §3.3 (`MemoryInput`) |
| J1 | §9.3 | | SI-17, SI-18 | §14.2, §3.8 |
| 2.5 (rest) | §4.6 (request types) | | F1, F1b, F2 | §6.2 |
| 4.2 | §7.1, §4.6 | | F3 | §6.3 |
| 3.5 | §3.3, §4.8 | | F4 | §6.4, §9 |
| B3 to B8 | §4.6, §13.3 | | F5, F5b | §6.3 (`ReasonKey`), §3.2 (`class`) |
| Z0 to Z13 | §10 | | F6 | §6.6 |
| FD-U | §2.1 | | F7 | §6.5, §6.7 |
| FD-B | §2.2, §7.2 | | F8 | §6.10 |
| FD-P | §2.5 | | F9, F9b | §6.5 (UI), §14.2 |
| FD-E1, FD-E2 | §2.3, §2.4 | | F10, F10b, F11 | §14.2, §6.9 |
| FD-I, ADR 0068 WP-1 | §11 | | F12, FM | §6.11, §6.8 |
| FD-K, WP-SL1 | §12 | | D-a, D-b | §7.3 |
| Feature packages (§0) | §14 | | 1.1, 1.2, M2b | dialogs part |

## 16. Settings introduced or touched

All under Settings ▸ Performance unless named; each validated in the engine; Auto where a count.

| Path | Default | Bounds | Owner |
|---|---|---|---|
| `performance.import.parse_threads` | Auto (cores − 1, ≤ 8) | 1 to cores | exists |
| `performance.import.record_bytes` | 8 MiB | 1 to 256 MiB | SI-13 |
| `performance.import.dump_whole_bytes` | 1 GiB | exists | exists |
| `performance.import.spool_bytes` | 16 GiB | 1 GiB to disk | SI-13 |
| `performance.import.archive_ratio` | 200 (floor 64 MiB) | 10 to 2,000 | SI-13 |
| `performance.import_whole_file_mb` | store cap − import budget (1,280) | 0 to store cap | FM |
| `performance.import_headroom_mb` | 256 | 64 to 1,024 | FM |
| `performance.plugin_memory_mb` | 1,024, or No cap | 64 to 4,096 | 2.6a, 3.2 |
| `performance.plugins.start` | On first use | On first use, At startup | 2.6a, 4.4 |
| `performance.plugins.idle_stop_min` | 10 [J] | 1 to 120, Never | 2.6a, 4.4 |
| `performance.plugins.stream_credit` | Auto (4) | 1 to 64 | 2.6b |
| `performance.stall.<key>.blocked_ticks`, `.spin_cpu_s` | Auto (per key, LV1) | per key | 2.6c, LV1 |
| `folder.scan_entries` | 500,000 | 10,000 to 5,000,000 | F4 |
| `folder.scan_threads` | Auto (`parse_threads` local, 4 remote) | 1 to cores | F4 |
| `time.display` | UTC | UTC, Local, Logs' time zone | Z-packages |
| `time.zone_probe_budget_ms` | ADR 0066 D3 | as there | Z8 |

## 17. Open questions, corrections, recommendations

| # | Question | Recommendation |
|---|---|---|
| Q1 | ADR 0063 is still Proposed; FD-U, 3.3 and 4.5 shape replies | **Decide 0063 before 3.3 and 4.5 start** (roadmap R8). FD-U does not wait: the declared bound (§2.1) holds under JSON and under MessagePack alike |
| Q2 | Ids above 2^53 | **Declared bound** (§2.1), the cheapest of string, bigint or bound; a registry row `store.event_ids` (`hard`) and a test at `2^53 − 1` |
| Q3 | ADR 0068 §12 drops `WholeReason::DumpWithoutMemoryList` (WP-8), which ADR 0064 lists | Keep it in the enum until WP-8 lands, then remove it in WP-8's change with ADR 0064's amendment; SI-1 should not design around its absence |
| Q4 | ADR 0068 §10 names `left_out: Vec<Omission>`; ADR 0065 has no `Omission` | **Use `Vec<Cut>`** (§11); a one-word amendment to ADR 0068 in WP-1's change |
| Q5 | `NextStep` lives in family ADR 0052's `paddock-jobs` enum, not on main | Use it there; if ADR 0052's crate change slips past WP-1, define `NextStep` in `debrief-core::finding` registered in `docs/known-copies.toml` with a delete-by (rule 55), not silently |
| Q6 | ADR 0031's `RunStateProbe::sample(pid: u32)` takes a bare pid (rule 53) | **Take `&ProcessId`** in 2.6c; the probe checks the start time before sampling, so a recycled pid reads `Exited`. A one-line amendment to ADR 0031 |
| Q7 | `WireEvent.ts_kind = Wall` lands before ADR 0066's store column | Ship the variant in v1 (postcard: adding it later would be a v2); plugins send `Parsed` until Z1 |
| Q8 | §6.5's `ChooseToken` instead of paths in `folder_scan_start` deviates from ADR 0067 | Accept the token (no path crosses twice); a one-line note in ADR 0067 by F7 |
| Q9 | `Supervisor` without containment as a type error (§4.2) instead of ADR 0001's run-time refusal | Type error plus the existing test; the run-time refusal stays for the `developer` feature path |
| Q10 | `ViewFilter`'s subset (§5.8) and `ProfileLimits` / `DocumentOptions` fields (§5.7) were not listed from source here | PH2a lists them from the first-party plugins before freezing the v1 bodies; the reviewer of PH2a checks every field is present (postcard: no later additions) |
| Q11 | `FileInfo.rel_tail` caps the path at 3 segments [J] | Accept; importers that read a folder name from the path (a machine folder) get it; the full path stays host-side |
| Q12 | Plan D5 (`Limit`/`Page` home) | **`paddock-limits`** (§4.6), with ADR 0065's types, not `paddock-view` |
| Q13 | `performance.plugins.idle_stop_min` default | 10 minutes [J]; 4.4 measures plugin start cost and may change it with a recorded result |

## 18. Completeness

**Complete** (signatures, entry points, names, Settings, test homes): §2 ids and references, §3 streaming import, §4
the shared platform, §5 the `debrief.*` namespace (with Q10's field lists left to PH2a), §6 folder open, §16 Settings,
§15 the package index.

**Complete at the level of placement and contract, deferring to the accepted ADR for detail:** §7 store, §8 memory
bounds, §9 jobs, §10 time model, §11 findings, §12 logs, §13 engine-to-UI contract.

**Not designed, by the scope decision:** every package §0 lists as feature; §14 gives their extension points and the
lane measurement plan.

**Not verified:** nothing was compiled; signatures were checked against the source files named in the header, not
built. `ViewFilter`, `ProfileLimits` and `DocumentOptions` field lists (Q10). The claim that ids grow with import order
inside a store (§5.6) rests on the block allocator as read, not on a test.
