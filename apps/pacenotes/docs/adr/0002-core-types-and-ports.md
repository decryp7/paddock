# 2. Core types, ports and the index schema contract

Status: proposed (2026-10-29), package D-index of the first-release plan. Implemented by `pacenotes-core` (package A1).
Numbered at merge.

## Context

Stage 1 builds pure crates (parse, git, rules, index, diagram, requirements, coverage) that must fit together before any
app exists. They share a small vocabulary (ids, locations, revisions, confidence, tiers, finding states) and talk to the
outside only through ports the core owns (AGENTS.md "Design principles"; proposal section 14). This ADR fixes that
vocabulary and the port contracts once, so the crates written later and in parallel do not each invent theirs. It lists
one test per contract; each test is written first (ER-11).

## Decision

### Types (`pacenotes-core`, no I/O, no process, no clock)

| Type | Shape | Rule |
|---|---|---|
| `RepoPath` | a workspace-relative path, `/` separated | parsed once at the edge: not empty, not absolute, no `..`, no `.` segment, no NUL, no backslash, at most 4096 bytes and 255 per segment; Windows drive prefixes refused |
| `Span`, `Location` | 1-based line, 1-based column in UTF-8 bytes; `Location = RepoPath + Span` | `start <= end`; a span never wraps |
| `Fingerprint` | SHA-256 of normalised content, 32 bytes | a file's content with line endings normalised to `\n`; used for rename matching and baselines |
| `FileId` | opaque 128-bit id issued by the store from a persistent counter (`FileIdAllocator`); never recomputed | **survives renames**: the index re-binds the same `FileId` to the new path when the fingerprint matches (A3a). It is issued, not hashed from path and content, because `git mv a b` followed by a new file at `a` with the same content would otherwise give two files one id |
| `SymbolId`, `ChunkId` | 128-bit hash, displayed `sym:<32 hex>` and `chk:<32 hex>` | `SymbolId = H(FileId, language, kind, qualified name, disambiguator)`; **no path in it**, so a rename that keeps the `FileId` keeps every symbol id (rule 14: ids derive from identity, not order or place); a changed qualified name is a new id, by design |
| `Rev` | `Commit(CommitHash)` or `WorkingTree(Generation)` | `CommitHash` is 40 or 64 lowercase hex; `Generation` is the index generation counter, so caches keyed by `Rev` never serve an older index |
| `Tier` | `Text < Syntax < Semantic` | ordered; "tier 3 failed" is a missing `Semantic` fact, never an error of the whole answer |
| `Confidence` | `Guess < Heuristic < Resolved < Exact` | every reference, call edge and link carries one; the UI shows it (proposal 20.1) |
| `Language` | `Known(KnownLanguage)` or `Other(LanguageId)` | `LanguageId` is a validated slug (`[a-z][a-z0-9+#-]{0,31}`); parsing `"c++"` and `"cpp"` gives the same value |
| `Severity` | `Info < Low < Medium < High < Blocker` | proposal section 8 |
| `FindingState` | `Open`, `Confirmed`, `Dismissed`, `Fixed`, `Suppressed` | a transition table (`FindingState::transition`) refuses illegal moves with a typed error; a state is an enum, never flags |

**Qualified name.** The chain of containers *inside the file*, joined by the language's separator (`Outer::Inner::method`,
`Class.method`, `Type#member`), never a module path derived from where the file lives (a Rust `crate::a::b` from
`src/a/b.rs`, a Python dotted module from the folder, a Java package from the directory). Otherwise moving a file would
change every id in it. The module path is a separate, mutable column of the `symbols` table, shown to users and used to
resolve imports, and it is not part of the id.

**Wire rules.** All types derive `serde`. Wire enums are externally tagged only (postcard cannot read internally or
adjacently tagged ones). A validated type (`Position`, `Span`, `RepoPath`, `ServerPack`, `Download`, `RuleSpec`, `Budget`,
`Contributions`, `HostRequest`, ...) has private fields and is read through `#[serde(try_from = ...)]` into the same
validation as its parser, so no format builds an invalid value; one test lists every such type and checks JSON and postcard
(`wire_tests`), and another round-trips every type through both. `ServerPack` carries `init_options` as JSON text (an
object, at most 16 KiB) so it crosses postcard; the core does not judge its content: the pack loader in `pacenotes-lsp`
owns the check of known execution switches (rust-analyzer's `cargo.buildScripts`, `procMacro`) against the repository's
trust state. A pack may not set loader variables (`LD_*`, `DYLD_*`), any `*_OPTIONS`/`*_OPTS`/`*OPT` variable (`NODE_OPTIONS`,
`JAVA_TOOL_OPTIONS`, `PERL5OPT`, `RUBYOPT`) or the path, startup and rc variables of common runtimes (`PYTHONPATH`,
`PYTHONSTARTUP`, `BASH_ENV`, `NODE_PATH`, `RUSTC_WRAPPER`, ...). A language-server reply names places outside the workspace
(the standard library, dependencies); they are dropped and counted (`outside_workspace`), not an error, and Windows drive
letters and their percent encodings are one place. `LspReply` and `SymbolPage` are bounded on the wire too.

### Schemas with validation (types only; loaders in the crates that use them)

- **`ServerPack`** (tier 3 language-server pack, proposal 20.4): `[pack]`, `[server]`, `[workspace]`, `[features]`,
  `[scip]`. Parsed from TOML with unknown keys refused. Refused with a message that says what to fix: a `command` that is
  neither a bare program name nor an absolute path (no shell metacharacters, no relative path with separators); a
  `download` without a 64-hex SHA-256 or without an `https` URL (**an unpinned download is refused**); an empty glob
  list; an unknown LSP feature; a duplicate pack id. Sizes are bounded (file 64 KiB, 64 globs, 32 detect entries).
- **`RuleSpec`** (structural and Markdown rules, proposal 24.2): id, title, severity, include/exclude globs, languages,
  the check (`Pattern` or `Markdown`), optional fix hint, `good` and `bad` examples. A rule with a structural check and
  no examples is refused (the examples are its tests). The pattern itself compiles in `pacenotes-rules`; the core
  validates shape and bounds only.

### Ports (traits owned by the core; adapters elsewhere)

Every port method takes a `&CancelToken` (from `paddock-jobs`) when it can block or run long (every `Store` read, since
each may touch disk), returns a typed error that never panics on bad input, bounds what it returns (`symbols_in_file`
takes a limit and reports truncation; `embed` takes at most 256 texts of 64 KiB; a language-server reply holds at most
10,000 locations) and is object safe. `Llm::complete` returns the model that answered with the text. A port documents its error semantics; every adapter honours them
(Liskov).

| Port | Methods (A1) | Contract | Contract test (written first) |
|---|---|---|---|
| `History` | `head`, `read_at(rev, path, max_bytes)`, `changed_between(from, to, limit)` | reads never touch the working tree; a missing path is `NotFound`, not an empty file; `max_bytes` caps the read and an over-long file is `TooLarge`; `changed_between` is sorted and capped, and says when it truncated | `history_contract`: not found vs empty, size cap, sorted and truncation flag, cancel before work |
| `Store` | `generation`, `file_id(path)`, `symbols_in_file(file, limit)` (read side), each with a cancel token | reads are consistent within one generation; the writer is a single actor behind a separate trait in `pacenotes-index` | `store_contract`: a read never mixes two generations; unknown file is `None` |
| `Embedder` | `model`, `dimension`, `embed(texts, cancel)` | the returned vectors have `dimension` entries each, one per text, in order; the model id is recorded with every vector (**vectors of different models never mix**) | `embedder_contract`: count, order, dimension, cancel |
| `VectorIndex` | `model`, `insert`, `remove`, `nearest(query, k)` | refuses a vector of another model or dimension; `nearest` is deterministic for equal scores (by id) | `vector_index_contract`: mix refused, deterministic ties, remove then absent |
| `LanguageServer` | `request(feature, params, cancel)` | a malformed or oversized reply is `BadReply`, a stopped or quarantined server is `Unavailable` (callers fall back to tier 2) | `language_server_contract`: both errors, never a panic |
| `Llm` | `complete(prompt, budget, cancel)` | the token budget is a hard cap; no Pitwall is `Unavailable` with a next step, never a hang | `llm_contract`: budget cap, unavailable |
| `Forge` | not in stage 1 | later | none |

Each contract test is a generic function in `pacenotes_core::contract` (behind the `testing` feature) that any adapter's
own test calls with its instance; the core ships an in-memory fake of each port that passes it.

### The function port's budget (package FN0)

A call across the function port carries a `Budget` of **host ticks**, items and bytes. It is a *cap* in the sense of ADR 0031:
the tick count is the host's own beats (a pause spends none; there is no progress channel across the port, so the cap is a
plain count and does not stretch for a call that is still working), the default lives in the settings the host owns
(`Budget::setting_specs`, keys `host_budget_*`, the `host_` prefix reserved so a function cannot clash: 30 ticks, 1000
items, 1 MiB) and not in code, and
the error that a limit raises says which limit, its value and the setting to change. The byte bound is the codec's message
bound (4 MiB): one source. Items and bytes replace the per-call `limit` and `max_bytes` fields. A model call
(`HostCall::Complete`) names its origin as the function's claim; the host decides from the job context it runs the function
in and cross-checks the claim: a background context refuses every model call whatever the field says, and a user context
refuses a claim of `Background` (family rule 46). The contract test `host_contract` checks cancel, the budget's effect and
both refusals for any host.

### Deviation from ADR 0022 §3.7

§3.7 says to extend the WP-C6 check so that no manifest under `apps/pacenotes/` enables `paddock-repo-graph`'s
`cargo-metadata`. The guard is two things instead, because WP-C6 checks copies of CI scripts and shares no mechanism with
this: (1) the real guard in Pacenotes' own CI leg, `cargo tree -e features --all-features -i paddock-repo-graph`, which sees
the resolved graph ("not in the graph" passes); (2) conformance C52 as a backstop that reads the manifests with a small
TOML reader (multi-line arrays, quoted headers, `package` renames, `alias/cargo-metadata` forwards, `#` in strings) and runs
from that leg's checks.

### Index schema v1 (the contract `pacenotes-index` implements in A3a)

One SQLite file per repository in the app's data folder (never in the repository), WAL, a `schema_version` row, one write
connection (the writer actor) fed by a bounded queue, any number of readers. Tables: `files(file_id, path, fingerprint,
language, size, generation)`, `symbols(symbol_id, file_id, kind, qualified_name, span, confidence)`, `chunks`, `imports`,
`calls`, `meta`. An unknown or newer `schema_version`, a failed integrity check or a file that is not SQLite is **rebuilt,
never trusted** (the previous file is kept aside for one run). Persistent user decisions (links, suppressions) are also
written to committable TOML, so a rebuild loses no decision (proposal section 14).

### Not in A1

The grammar-pack manifest of tier 2 (`pack.toml` with grammar source, queries and capabilities, proposal 7.2) is not
specified yet: tier 2 ships compiled-in grammars first (A2) and the manifest schema follows with the first non-core
pack. `Store` is the read side only; `Forge` waits for the Gitea integration.

### Parallelism and cost (rule 51)

Stage 1 has no bulk operation of its own; the contracts above fix what the bulk operations of A3a and A-embed must obey so
that they can be both parallel and cheap. Parallel: reads (`Store`, `History`) are independent and bounded by the kind of
work (SSD reads a few at a time, CPU work cores minus one), the one writer is a single actor behind a bounded queue, and
embedding takes batches of at most 256 texts, which is the unit a worker pool maps over, with deterministic ordering (output
in input order; ties by id). Cheaper: `Fingerprint` lets the index skip unchanged files and match renames without
re-parsing; `symbols_in_file` and every reply are bounded so a large repository never builds one huge answer; no per-item
model call (an `Llm` call needs a user origin). A3a's design records the thread counts as settings with an "Auto" default
and the measurements (rule 48) in `docs/performance.md`.

### Shared later

Four things here are copies that belong in shared crates, and move only when shared code unfreezes after the Dyno and
Spotter tags (the plan's shared-code limit, rule 1): (1) `RepoPath` (other crates parse workspace-relative paths);
(2) the postcard wire helpers (`pacenotes-function-api::codec`, which `paddock_ipc::wire` already covers, with its `Json`
helper); (3) one canonical content `Fingerprint` for the family; (4) the `Budget`/cap vocabulary, with ADR 0031's policy
types. Each copy carries a `TODO(shared-later, ADR 0002)` comment.

## Consequences

- Ids are stable under renames because the path is not in them; the cost is that the index must persist `FileId` and
  re-bind it on a fingerprint match. A3a owns that and its test.
- A crate that needs a type from another stage-1 crate depends on `pacenotes-core` alone; nothing depends on an adapter.
- Adding a port method is additive and updates the contract test and the fake in the same change.
- Everything here is replaceable behind the function port later (FN0) without touching the types.
