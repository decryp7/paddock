# Plan: Pacenotes first release

- Status: Plan, 2026-10-29; the user's decisions of the same day are in section 7.1. Nothing here is built.
- Start (decided 2026-10-29): once Dyno 1.0 and Spotter 1.0 have shipped, the pure Rust app crates (stage 1, section
  4.3) run beside Pitwall 1.1 and Debrief 1.2 with one or two agents, within load and CI limits. Stage 1 changes no shared
  crate; it consumes some that Debrief 1.2 changes and rebases when those changes merge (section 4.1). Everything that
  touches the UI or shared code (stage 2) waits until Debrief 1.2 has shipped.
- App: Pacenotes (`apps/pacenotes`). Rule 1 holds: while another app is releasing, Pacenotes work runs only within load,
  CI capacity and the shared-code limits of section 4.1.
- Scope (decided 2026-10-29): the **full v1**, no cut-down release. That is the four phases of
  [architecture-direction.md](../architecture-direction.md) ("Phasing") together: core, Understand, Review, Debrief bridge
  and the core language packs; Diagrams; Requirements; Tests & coverage. Their order stays as the internal order of the
  waves. Every feature below is taken from Pacenotes' own docs; where they disagree, the disagreement is an open question
  (section 7.2), not a choice made here.
- Sources read: [AGENTS.md](../../AGENTS.md), [README.md](../../README.md), [proposal.md](../proposal.md) (sections 6,
  13, 14, 17-24), [architecture-direction.md](../architecture-direction.md), [ADR 0001](../adr/0001-embeddings.md)
  (accepted, amended 2026-10-28), [suite-lessons.md](../suite-lessons.md), [performance.md](../performance.md); family
  [rules](../../../../docs/rules.md), [AGENTS.md](../../../../AGENTS.md), ADRs [0013](../../../../docs/adr/0013-paddock-workbench.md),
  [0014](../../../../docs/adr/0014-monorepo-under-paddock.md), [0020](../../../../docs/adr/0020-shared-executor.md),
  [0022](../../../../docs/adr/0022-rust-developer-and-ci-tool.md) §6, [0024](../../../../docs/adr/0024-background-work-contract.md),
  [0026](../../../../docs/adr/0026-shared-dialog-api.md), [0027](../../../../docs/adr/0027-table-layers-and-sticky-headers.md);
  plans [workbench.md](../../../../docs/plans/workbench.md) §4.4, [debrief-1-2.md](../../../../docs/plans/debrief-1-2.md),
  [shared-next.md](../../../../docs/plans/shared-next.md) X55; [crates/README.md](../../../../crates/README.md),
  [ui/README.md](../../../../ui/README.md); the read-only backend note
  `/Volumes/d7sst7/dev-targets/review/2026-10-28-pacenotes-sighter-backend-plan.md` (outside the repo; its content is
  summarised in appendix A; its packages are cited here as BN1-BN8, not PN, because `docs/audits/pacenotes.md` uses PN1-PN10
  for its findings).
- Companion plan: [the shared workbench move](../../../../docs/plans/2026-10-29-shared-workbench-move.md). Pacenotes' UI
  is built on the shared workbench. The UI starts in stage 2, after Debrief 1.2 has shipped, and the move's switch
  (its Phases 4-6) is part of Debrief 1.2, so Pacenotes builds on a workbench that Debrief already uses.

## 0. Facts this plan starts from (checked 2026-10-29 on `origin/main` `912ad7c12`)

| Fact | Evidence |
|---|---|
| Pacenotes has no code, no Cargo workspace and no CI leg | `apps/pacenotes/` holds docs, `deny.toml`, `rustfmt.toml`, `cliff.toml` and `scripts/release`; `paddock.ci.json` is a `$comment` stub |
| The phases are core, Understand, Review, the Debrief bridge and the eight core language packs; then Diagrams, then Requirements, then Tests & coverage, with "no cut-down public release inside these phases" | `architecture-direction.md` "Phasing" (decided 2026-10-23); the user confirmed on 2026-10-29 that v1 is all four |
| The proposal's feature tables mark the v1 rows of all four; its estimate for that v1 is 100-125k lines of Rust and 22-26 weeks elapsed | `proposal.md` §6.4, §6.6, §6.7, §19 |
| Search default: Model2Vec potion-code-16M-v2 + BM25 on the CPU; Qwen through Pitwall re-ranks a query's top ~50 and runs the opt-in "Deep index with Qwen" job; no per-chunk LLM summaries | ADR 0001 amendment of 2026-10-28; measurements in [performance.md](../performance.md) "2026-10-25 (amended 2026-10-28), embedding engines" |
| The shared workbench exists and has a reference app; the shell exists and Dyno and Spotter are moving onto it | `ui/projects/paddock-ui/workbench/` (frame, dock, explorer, palette, panels, assistant, testing); `ui/projects/workbench-demo/`; crates `paddock-workbench`, `paddock-panels`, `paddock-assistant`; branches `dyno/shell-1.0`, `spotter/shell-1.0` |
| A starter exists, but it writes a new repository on the registries, not an app in the monorepo | `scripts/new-app.mjs` header: "into a new repository ... with the shell crates and the UI package taken from the registries" |
| Shared crates that Pacenotes was named as a user of are built: `paddock-repo-files`, `paddock-repo-graph`, `paddock-names`, `paddock-mcp`, `paddock-pitwall`, `paddock-assistant`, `paddock-guard`, `paddock-ipc`, `paddock-plugin-*`, `paddock-view`, `paddock-packs`, `paddock-watch` | `crates/README.md` table |
| `paddock-embed` (X55) and `paddock-limits` do not exist yet | `crates/` listing |
| Debrief 1.2 changes many shared crates and `@decryp7/paddock-ui` (listed below the table) | `grep -o 'paddock-[a-z-]*' docs/plans/debrief-1-2.md`; the packages named in the list |
| The Pacenotes and Sighter accents are in the tokens | `feat/accents-pacenotes-sighter` is merged; `_tokens.scss` names `pacenotes` |

Shared code Debrief 1.2 changes: `paddock-jobs` (J1 idempotency keys, ADR 0024 keys); `-proc` (2.2); `-ipc`, `-guard`,
`-plugin-api`, `-plugin-host`, `-plugin-test`, `-plugin-package` (1.4, 2.6, PH1-PH2b); `-testkit` (`limit_case!`, the
large-fixture generator); `-view` or a new `-query` (2.5); `-dialogs` (1.1); `-settings`, `-health`, `-archive` (limits
variants); `-metrics`; `-packs` (N3); `-bench`; `-shell`, `-startup`, `-updater`, `-download`, `-workbench`; `-pitwall`
(its first heavy consumer, M2c); new `-limits`, possibly `-embed` (N7) and `-repo-rules`; and `@decryp7/paddock-ui`
(the dialog frame, lists, the virtual primitive, the workbench move).

## 1. What the first release contains

The full v1, taken from the docs. "Source" is where the feature is decided; a row of the proposal's tables marked v1.1 or
Later stays out.

| Area | In the first release | Source |
|---|---|---|
| Workspaces and safety | Open a folder; workspace allow-list; canonical paths, symlink escape refused; secret denylist and redaction of every reply; never execute repository code; read-only towards repositories; local only, no telemetry | AGENTS.md "Pacenotes-specific rules"; proposal §6.10 |
| Index, tier 1 (text) | Incremental index of every file: full-text and regex search, file tree, git history, hot spots, docs, build-system detection from manifests only | proposal §20.1, §20.2 step 1 |
| Index, tier 2 (syntax) | tree-sitter grammars for the eight v1 languages plus Kotlin (decided order: Go, Java and Kotlin, C and C++, then the rest); symbols, outline, imports, approximate (confidence-labelled) references and call graph, syntax-aware chunks; parsing in worker processes with limits | proposal §6.1, §14, §20.1 |
| Index, tier 3 (semantic) | LSP client with eight built-in language packs (C, C++, C#, Java, TypeScript/JavaScript, Rust, Python, Go); servers never bundled; started only after "Trust this repository"; supervised, cached by index revision | proposal §20.4 (decision 2026-10-23); AGENTS.md trust rule |
| Search | Text, structural and semantic search fused with RRF; Model2Vec + BM25 by default; Qwen re-rank of the top ~50 when Pitwall runs; "Deep index with Qwen" as a confirmed job | ADR 0001 and its amendment; proposal §21 |
| Understand | Repo map with token budget, outline, read symbol, find symbol, references, call graph and call-path trace, stack-trace and log-site resolution, ownership (blame, CODEOWNERS), churn and hot spots, complexity metrics, entry points and public API, manifest package graph, architecture summary | architecture-direction "The plugins"; proposal §6.1, §6.2 |
| Orientation | One page after indexing: entry points, modules and dependencies, external libraries, hot spots, where the tests are, config keys and env vars, the generated editable draw.io architecture diagram, a suggested reading order | proposal §20.2 steps 2 and 4 |
| Views | Architecture map; module and package dependency graph with cycles; call graph and call-path trace; symbol outline and API surface; entry-point inventory (framework recognisers as packs); change impact; hot spots; owners; test map and coverage overlay; complexity and size; TODO/FIXME inventory; dependency inventory; config and env-var inventory; generated reading tour | proposal §22, rows marked v1 |
| Dependency graph | Zoom levels, filters, focus N hops, shortest path, cycles, layer rules from `.pacenotes/architecture.toml` fed to Review, diff mode, export to draw.io and SVG; layout and aggregation in the engine | proposal §6.1, §23 (advisories and Spotter versions are v1.1) |
| Review: sources | Diff, branch, commit range or whole repository; structural pattern rules; SARIF and rdjson import; Markdown rules (model-checked, needs Pitwall) with conversion to structural rules and a hygiene report; rules in `.pacenotes/rules/` and per user, with tests; grounding from the index | proposal §6.3, §24 |
| Review: findings | Evidence and a suggested diff; suppressions with a reason; baselines; layer-rule violations; diagram-stale, requirement-touched and test-gap checks; Markdown and SARIF output; CLI exit codes 0/1/2/3 | proposal §6.3, §24 |
| Diagrams: kinds | draw.io output in the Paddock style: C4 context, containers and components; module and dependency graphs; class; call graph for a symbol; sequence diagram from a call path or stack trace | proposal §6.4, §9.4 |
| Diagrams: round trip and outputs | Regeneration that keeps hand edits and flags stale elements; size limits with splitting; diagram diff between two commits; SVG/PNG export, in-app preview, "Open in draw.io"; `docs/diagrams` kept current as a review check | proposal §6.4, §9 |
| Requirements | Importers for .xlsx, .docx, .csv and .md with declarative mapping packs; links from explicit tags (`REQ-123` in comments, test names, commits); traceability matrix requirement to code, tests and commits; coverage reports, orphans, suspect-link drift; impact view; matrix export to CSV; review integration | proposal §6.6, §11 |
| Tests & coverage | Coverage import (LCOV, Cobertura, JaCoCo, llvm-cov JSON, istanbul); coverage per file, symbol, requirement and diff (patch coverage) with trend; risk hot spots (churn x complexity x low coverage); untested error paths (throw and log sites); test inventory from language-pack conventions; requirement-to-tests matrix | proposal §6.7, §12 |
| Debrief bridge | `check_log_site`, `check_exception`, `find_config_usage`, `find_throw_sites` with verdicts and confidence; bulk throw-site enrichment (no LLM); "seen in production logs, never covered by tests"; deep links both ways | architecture-direction; proposal §6.5, §10, §13 |
| MCP | Read-only server over stdio and loopback HTTP with per-client keys, grants, budgets, call log; the tools of §13 and §22 for the functions above; Pitwall toolsets `orient`, `code`, `review`, `diagrams`, `requirements`, `tests` | proposal §6.8, §13, §22.1 |
| Assistant | Ask box over Pitwall with visible tool calls and `path:line` citations; "needs local AI" state without Pitwall | proposal §6.2, §20.2 step 3; ADR 0013 §1 |
| Headless | `pacenotes` CLI and service (index, search, review in CI, `trust`/`untrust`, `serve --stdio`) | architecture-direction "Core and plugins"; proposal §20.1 |
| Admin | Index health (unparsed files with reasons, tier per language, rebuild), settings export/import, pack manager, ignore editor, "Low memory" preset, autosave and versioned state | proposal §6.10, §21.4 |

**Not in the first release** (v1.1 or Later in the docs): Gitea posting; MCP host; SCIP import; advisory mirror and
Spotter versions; .pptx, .pdf and ReqIF importers; Pitwall-assisted requirement extraction and semantic links; xlsx
write-back; Markdown/HTML traceability reports; test-results import and flaky-test detection; drafted tests and
`.sighterplan` hand-off; state and deployment diagrams; Pitwall diagram captions; Mermaid export; data flow, duplicates,
dead code, glossary; the further proposals of §25. Out-of-process packaging of the first party's functions also follows
after v1 (section 2.1).

## 2. Architecture sketch

### 2.1 Processes

```
 Pacenotes app (Tauri 2 + Angular on the shared shell and workbench)
 ├── engine (Rust, in the app process; same crates as the CLI)
 │     index store (SQLite, WAL; one writer) · search · graph · review · check · MCP server (loopback, per-client keys)
 │     jobs on the shared executor (ADR 0020/0024): index, re-embed, deep index, review, pack download
 ├── parser workers          tree-sitter, limits, killed on overrun      (paddock-ipc + paddock-guard + paddock-proc)
 ├── embed worker            Model2Vec (it panics on tokenizer errors)    (same)
 ├── language servers        only after "Trust this repository"; one at a time by default, memory cap, idle stop
 │     functions Understand, Review, Debrief bridge, Diagrams, Requirements, Tests & coverage: in the engine (v1),
 │     each behind the function port, so it can move out of process later without a code change of its own
 └── third-party plugins     none in v1; the same port over paddock-plugin-host later
 pacenotes serve --stdio     the same engine headless, sharing the SQLite file (WAL, single-writer lock)
 Pitwall (optional)          chat, re-rank, deep-index embeddings, over paddock-pitwall
```

**Functions in the engine first, behind the plugin port (decided 2026-10-29).** Understand, Review and the Debrief bridge
(and the later functions, Diagrams, Requirements and Tests & coverage) run in the engine in v1. They are packaged as
out-of-process plugins on `paddock-plugin-host` later. To keep that later move mechanical, the boundary is strict:

- Each function is its own crate and depends only on `pacenotes-function-api` and the small allow-list that F1 checks
  (`serde`, `thiserror` and named pure helpers, no I/O). Everything else is a host call: index and history queries,
  parsing (the parser worker), knowledge packs, the `Llm` port (Pitwall through `paddock-pitwall` in the host), archive
  reading, the draw.io writer and the shared coverage readers. The port's contributions mirror `paddock-plugin-api`'s
  (tools with checked schemas, settings, `ViewDoc`s, MCP tools). In stage 1, before Debrief 1.2 settles
  `paddock-plugin-api`, `pacenotes-function-api` defines its own types; in stage 2 (package FN1) it is rebased onto
  `paddock-plugin-api`'s and `paddock-view`'s types, re-exporting them, with a mapping test, so no second copy drifts.
- Every request, reply and event across the port is a serialisable type (serde, postcard-compatible: no references,
  closures, trait objects or handles), and every call is bounded, cancellable and returns a typed error, as it will be over
  the IPC.
- **What v1 claims (decided 2026-10-29):** in-process panic attribution, quarantine after repeated faults, a time budget
  after which the caller stops waiting (`paddock-guard`), and cooperative cancel through the token every host call
  carries. Nothing more: `paddock-guard` cannot stop a running call, survive a stack overflow or an abort, or free memory
  (its module doc, `crates/paddock-guard/src/lib.rs`), and `paddock-plugin-test`'s `FakeHost` runs a plugin in the same
  process, so it cannot test hang or memory containment either. The hang and memory containment tests belong to the
  packaging work after v1, with a plugin wrapper binary per function on `paddock-plugin-host`.
- **The remaining risk, plainly:** a function that hangs without checking its token keeps its thread. If it held the
  SQLite writer, indexing and every write would stop behind it; if it held a read snapshot, the WAL could not be
  checkpointed and would grow. The mitigation is structural:
  - **One writer actor** in `pacenotes-index` owns the only SQLite write connection. Every other component (the index
    job, review, links, suppressions, the CLI's engine) sends it write batches over a bounded queue and gets a typed
    result back; no other code can open or hold the writer, because no other code has the connection.
  - **Each host call opens and closes its read transaction inside the call.** Results are copied out as data before the
    call returns, so no snapshot or connection ever crosses into function code.
  - A function returns its results as data; the host turns them into a write batch for the actor after the call.
- Checked by package F1 (section 4): a `cargo tree` check of each function crate against the explicit allow-list, a
  round-trip test of every port message through postcard, and the writer and checkpoint tests ("F1 in detail", section 4).

This is a user decision that amends AGENTS.md's "Plugin architecture" ("no in-process plugins") for the first party's
functions in v1; P0.1 records the amendment there.

### 2.2 Crates, ports and adapters

The core depends on ports it owns; adapters are thin and replaceable (AGENTS.md "Design principles"; proposal §14). Names
follow proposal §14 under `apps/pacenotes/crates/`.

| Crate | Pure core | Ports it defines (adapters elsewhere) | Uses (shared) |
|---|---|---|---|
| `pacenotes-core` | ids (`SymbolId`, `ChunkId`, stable by rule 14), `Location`, `Rev`, `Confidence`, `Tier`, `Language`, finding state enum, language-pack and rule schemas (types and validation only) | `History`, `Store`, `Embedder`, `VectorIndex`, `LanguageServer`, `Llm`, `Forge` (later) | `paddock-names` types |
| `pacenotes-parse` | tree-sitter extraction (symbols, outline, imports, call candidates, chunks), query packs as files, limits (bytes, nodes, time) | none; the worker binary is its adapter | `paddock-ipc`, `paddock-guard`, `paddock-proc` (worker) |
| `pacenotes-index` | the writer actor (the only write connection, write batches over a bounded queue), schema v1 and migrations, change set from a snapshot, fingerprints, rename matching (path, then fingerprint) | `Store` (SQLite adapter here) | `paddock-repo-files` (walk, hashing), `paddock-watch` (changes), `paddock-env` (`VersionedFile`), `paddock-files` (allow-list grants) |
| `pacenotes-git` | churn, hot-spot ranking, blame aggregation | `History` adapter on gix (in process; never the `git` binary on the analysed repository) | `paddock-repo-graph`'s `History` port where the shapes agree (ADR 0022 §6) |
| `pacenotes-search` | chunker, BM25/FTS5 query building, identifier matching, RRF, deterministic re-rank score | `Embedder`, `VectorIndex` adapters (Model2Vec worker, Pitwall, the spike's winner) | `paddock-embed` (X55a, new), `paddock-names` |
| `pacenotes-lsp` | LSP client state machine, request/response cache by index revision, trust record | `LanguageServer` adapter: child processes | `paddock-proc` (supervised, memory cap, kill-tree), `paddock-packs` (language packs), `paddock-download`/`paddock-runtime` (pinned server downloads on click) |
| `pacenotes-function-api` | the function port: contributions, host calls, serialisable messages, typed errors (section 2.1) | the port itself | mirrors `paddock-plugin-api` |
| `pacenotes-understand` | references with confidence, call graph and call-path trace, repo map budgeting, recogniser matching, ownership, complexity, change impact, orientation and reading order | none (a function: only the port) | recogniser packs through a host call (the host uses `paddock-packs`) |
| `pacenotes-graph` | graph model, aggregation per zoom level, SCC cycles, shortest path, diff, layer-rule evaluation, layout of a bounded subgraph | none | `paddock-repo-graph` (Cargo and npm manifests; its `cargo-metadata` feature stays off, ADR 0022 §6) |
| `pacenotes-rules` | structural patterns over tree-sitter, SARIF 2.1.0 and rdjson reader/writer, suppressions, fingerprint baselines, rule tests | none | the joint rules model with `paddock-repo-rules` (ADR 0022 §6, section 3.3) |
| `pacenotes-review` | collect, ground, run deterministic rules, budgeted Markdown-rule pass, outputs, exit codes | none (a function: the `Llm` port is a host call) | the host's Pitwall adapter on `paddock-pitwall` |
| `pacenotes-check` | log-site template match, exception reachability, config-key use, verdict and confidence | none (a function) | the shared stack-trace parser through a host call (section 3.3) |
| `pacenotes-diagram` | model to layout (templates, layered, router) to canonical draw.io XML; reader for compressed and plain files; round-trip merge; diagram diff; SVG preview geometry | none (a function) | the draw.io writer through a host call; the host uses `paddock-drawio` (section 3.3) |
| `pacenotes-req` | requirement model, mapping packs, tag links, suspect model (requirement and code hashes), traceability reports, CSV export | none (a function); the importers are host-side adapters run in the parser worker | mapping packs and archive reading through host calls (the host uses `paddock-packs` and `paddock-archive`) |
| `pacenotes-cover` | coverage model, path mapping, patch coverage, hot spots with coverage, test inventory | none (a function); the readers are host-side adapters | the shared coverage and results readers through a host call (section 3.3) |
| `pacenotes-mcp` | tool schemas and handlers over the ports | none | `paddock-mcp` (protocol, limits, paging, audit, transports) |
| `pacenotes-cli` | argument parsing, exit codes | none | `paddock-cli` |
| `app/src-tauri` | commands only: parse at the edge, call the engine, return | all adapters wired here | `paddock-shell`, `paddock-workbench` (with `paddock-panels`, `paddock-assistant`), `paddock-commands` |

Every untrusted input is parsed once at the edge: source files in the parser worker, pack and rule files by their loaders
(unknown keys and bad patterns refused with a message), IPC arguments in the command layer, MCP requests by
`paddock-mcp`'s schema validation, LSP replies by the client (a malformed or oversized reply is an error, never a panic).

### 2.3 UI: presentation only, on the workbench, Material first

- The window is `<pd-workbench>` with `<pd-dock>` (ADR 0013). Pacenotes contributes views, activity items, Explorer
  providers, palette providers, perspectives, commands, status items, health services, dialogs, settings sections and
  assistant context, as [workbench.md](../../../../docs/plans/workbench.md) §4.4 lists them. Every view declares its layout
  (area, min, pref, priority; ADR 0011), which the workbench lint enforces.
- Components never call `invoke`; they use the backend behind a DI token (Tauri or mock), and state lives in SignalStores
  (rules 27-38; AGENTS.md "Where logic lives"). Every count, match, rank, verdict, graph layout and highlight comes from
  the engine; the UI only renders and formats.
- Material first (rule 47): `pd-table` (Material table) for findings, references, inventories and search results;
  `mat-tree` or the shared Explorer tree for file and symbol trees; Material tabs, menus, chips, toggles, progress and snack
  bars; dialogs are OS windows through the shared dialog API (rule 17, ADR 0026). The code viewer and the graph canvas have
  no Material component: each needs a short ADR (section 5) with the reason in the component's doc comment.
- The code viewer is read-only and draws engine-provided tokens (tier 2 highlighting from tree-sitter) in the shared
  virtual list (Debrief 1.2 package 2.1, ADR 0018), so no JavaScript syntax engine re-implements parsing. Monaco stays an
  option (ADR 0013, "Alternatives") only if the ADR shows a need.
- The graph view renders a bounded, pre-laid-out subgraph that the engine returns (proposal §23 "Scale"); the shared
  ECharts entry (`@decryp7/paddock-ui/chart`) is the first candidate for drawing it.
- Plugin views are `ViewDoc`s hosted by `pd-view` (ADR 0002); no plugin web code.
- No IPC before first paint; a plain splash in `index.html` (suite lessons).

### 2.4 Long work: off the UI thread, live progress, Cancel (ER-17 to ER-20)

Every long operation is a job on `paddock-jobs` and the shared executor (ADR 0020), keyed and at background priority (ADR
0024); commands return a job id at once, and the shared Jobs view and status item show it. No UI timeout (rule 12).

| Job | Phases (each on its own scale) | Parallelism (rule 13) | Consistency |
|---|---|---|---|
| Index a repository | walk (growing count), text index, parse (tier 2, worker pool), chunk, embed (Model2Vec), graph, language servers (tier 3, only when trusted) | walk: SSD I/O 4-8; parse and embed: cores - 1 or the Performance setting; one language server at a time by default | pipeline with bounded queues and per-stage progress (rule 14); a new index generation is built beside the old one and swapped; IDs never renumbered |
| Incremental update | changed files, parse, re-embed changed chunks | as above | per-file transactions; a watcher burst is debounced (`paddock-watch`) |
| Deep index with Qwen | estimate (shown before start, with chunk count, time and size), embed through Pitwall in batches, swap | Pitwall's own queue | the confirmed exception of rule 46; new vectors beside the old, then swap; vectors of two embedders never mixed (ADR 0001) |
| Review | collect, deterministic rules, Markdown rules (when asked and Pitwall runs), merge and baseline | CPU pool | a partial result says what was skipped and why (exit code 3) |
| Generate or regenerate a diagram | model, layout, merge with the file on disk, diff preview (the write waits for the user's click) | CPU pool | the file is written only after the preview is accepted; a failed write leaves the old file |
| Import requirements | parse (worker, limits), map, link, report | one document at a time per worker | a new import beside the last, then swap; links keep their state |
| Import coverage | parse, map paths, attach to the revision | CPU pool | runs of different revisions never blended; stale marks |
| Pack or server download | download (`paddock-download`, hash while writing), verify, install | per host | staged, then swapped (`paddock-runtime`) |

Cancel works in every phase and leaves the last complete generation in place. Progress never goes backwards; an unknown
total is a growing count, never a static 0 (rule 11).

### 2.5 Bulk work with rules and packs, not an LLM (rule 46)

| Job | How | LLM |
|---|---|---|
| Language detection, build systems, entry points, config keys, framework routes | TOML recogniser packs on `paddock-packs` (built in plus the user's `packs/`, hot reloaded, never executing code; rule 21) and tree-sitter queries | none |
| Indexing and semantic search | BM25 + Model2Vec on the CPU | Qwen re-rank of one query's top ~50 (bounded, user-started); deep index only after the confirmation |
| Chunk context | doc comment, signature, path, split identifiers | none; summaries only on click for the code shown (ADR 0001 amendment) |
| Review over a whole repository | structural and SARIF rules only | Markdown rules run on a diff or a chosen selection, capped; a larger run needs the count-and-time confirmation (backend note §3) |
| Identifier search | `paddock-names` (rule 20) | none |
| Debrief bridge verdicts | template matching and the index | none; "suspected cause" via Pitwall is per request |

### 2.6 Safety boundaries

- Trust: per repository, off by default, keyed by canonical path, granted only by the trust dialog (decision, modal), a
  setting or `pacenotes trust <path>`; revoke stops the server (kill-tree) and removes the record (proposal §20.1).
- Writes: only inside the data folder and to destinations the user picks; no tool that writes is exposed over MCP.
- Secrets: denylist before indexing, redaction before any reply, log or view; logs carry no source text.
- MCP: loopback or stdio only, per-client keys in `paddock-secrets`, grants per workspace and tool, request and result caps.

## 3. Shared parts: reuse and gaps (rule 42)

### 3.1 Rust

| Need | Shared crate | Note |
|---|---|---|
| App shell, dialogs, health, jobs, settings, startup, updater, window, menus, downloads | `paddock-shell` and the crates it wires | as Dyno and Spotter adopt it |
| Workbench, pop-outs, assistant | `paddock-workbench`, `paddock-panels`, `paddock-assistant` | the assistant's tools are Rust `AssistantTool`s (ADR 0013 §2) |
| Long work | `paddock-jobs`, the ADR 0020 executor | wait for Debrief 1.2's changes (packages M2a, J1, ADR 0024 keys) |
| Workers and plugins | `paddock-ipc`, `paddock-guard`, `paddock-proc`, `paddock-plugin-api`, `-host`, `-package`, `-test`, `cargo-paddock-plugin`, `paddock-view` | wait for Debrief 1.2 packages 1.4, 2.2, 2.6, PH2b |
| Repository walk and project graph | `paddock-repo-files`, `paddock-repo-graph` | `cargo-metadata` feature stays off; a check fails if Pacenotes enables it (ADR 0022 §6) |
| Identifier search | `paddock-names` | no app splitter |
| Knowledge packs | `paddock-packs` | language and recogniser packs; waits for Debrief 1.2's N3 `PackKind` work |
| MCP | `paddock-mcp` | |
| Pitwall | `paddock-pitwall` | Debrief moves onto it in 1.2 (M2c) |
| Credentials, files, folders, logs, diagnostics, storage | `paddock-secrets`, `paddock-files`, `paddock-env`, `paddock-log`, `paddock-diag`, `paddock-storage` | |
| Folder watching | `paddock-watch` | |
| Downloads of servers and models | `paddock-download`, `paddock-runtime`, `paddock-archive`, `paddock-net` | pinned, SHA-256-checked, on the user's click only |
| Deep links to and from Debrief | `paddock-siblings` | |
| CLI conventions | `paddock-cli` | |
| Benchmarks and test kit | `paddock-bench`, `paddock-testkit` | rule 16 budgets; `Scratch` repositories for every git and index test |

### 3.2 UI

| Need | Shared part |
|---|---|
| Frame, dock, Explorer, palette, pop-outs, assistant pane | `@decryp7/paddock-ui/workbench/*` |
| Shell services | `@decryp7/paddock-ui/shell/*` (dialogs, notify, jobs, settings, about, startup, menu, i18n, testing) |
| Tables | `pd-table` on Material's table (ADR 0027) |
| Virtual lists for the code viewer and long results | the shared virtual primitive of Debrief 1.2 (package 2.1) |
| Charts and the graph canvas candidate | `@decryp7/paddock-ui/chart` |
| Plugin views | `@decryp7/paddock-ui/view` (`pd-view`) |
| Sash for hand-sized panes | `@decryp7/paddock-ui/sash` (on `fix/debrief-explorer-resize`) |
| Name search in the palette | `namesPaletteProvider` (`workbench/palette`) |
| IPC timeouts declared | `@decryp7/paddock-ui/ipc` (`withCallTimeout`, C35) |
| e2e kit | `@decryp7/paddock-e2e` (page objects, axe, `expectNothingCovers` from `feat/e2e-hit-test`, the layout scenario runner) |

### 3.3 Missing from the shared code

| Gap | Proposed home | When | Why shared |
|---|---|---|---|
| BM25, RRF with weights, cosine top-k, index record, benchmark harness and query sets (X55a) | new crate `paddock-embed` (pure half first, worker and Pitwall adapter as X55b) | stage 2 (shared code waits for Debrief 1.2) | Debrief's N7 uses it (debrief-1-2.md); ADR 0001 amendment |
| Stack-trace parsers (.NET, Java, Python, JS) | a shared crate extracted from `apps/debrief/crates/debrief-core/src/exceptions.rs`, Debrief adopting in the same change | stage 2, after Debrief 1.2 | Debrief and Pacenotes parse the same traces |
| One rules model (severities, findings with fix diff, scoped suppressions, fingerprint baselines, SARIF, exit codes) | `paddock-repo-rules` per ADR 0022 §6; the joint rules-model note is written in stage 1 (a document), the crate in stage 2 | note in stage 1, crate in stage 2 | conformance and Pacenotes review would otherwise grow two models |
| Canonical draw.io writer, theme tokens and UML/C4 shapes | `paddock-drawio` (ADR 0022 §6), golden-tested against today's `.mjs` output | stage 2; stage 1's round-trip prototype stays in `pacenotes-diagram` and moves its writer into the crate then | the family's diagram tooling and Pacenotes share one writer |
| Coverage readers (LCOV, Cobertura, JaCoCo, llvm-cov, istanbul) and test-result types | next to `paddock-testplan`'s JUnit export | stage 2; stage 1 writes them inside `pacenotes-cover` behind `CoverageSource` and they move with their tests (rule 42) | Sighter writes TRX; Pitcrew collects coverage artefacts |
| Typed limit errors | `paddock-limits` (Debrief 1.2 package 1.4) | consume when merged | every bounded input in the engine |
| A monorepo mode for the starter | `scripts/new-app.mjs --in-repo` (path dependencies, root npm workspace entry, `paddock.ci.json`, CODEOWNERS line) | stage 2 (shared script) | Sighter starts the same way |
| A read-only code viewer | stays in Pacenotes | | no second app needs one (rule of three) |

## 4. Work packages

### 4.1 Conventions

- Two stages (decided 2026-10-29). **Stage 1** starts once Dyno 1.0 and Spotter 1.0 have shipped and runs beside Debrief
  1.2 with one or two agents, within load and CI limits: pure Rust app crates under `apps/pacenotes/crates/` only. Stage 1
  **changes no shared crate**. It consumes `paddock-names`, `paddock-cli`, `paddock-repo-files`, `paddock-testkit` (as a
  dev-dependency), `paddock-jobs` (cancel token and clock), `paddock-env` (`VersionedFile`) and `paddock-guard`; of these
  Debrief 1.2 changes `paddock-testkit`, `paddock-jobs` and `paddock-guard`, so stage 1 rebases when those changes merge.
  Every process, plugin, Pitwall and UI concern stays behind a Pacenotes port and unimplemented. Stage 1 also overlaps
  Pitwall 1.1: that release comes first (rule 1), so stage 1 holds while Pitwall's release jobs run, and its own freeze
  (package 0a of the workbench move plan) does not touch stage 1, which changes no shared path. **Stage 2** starts when Debrief 1.2 has shipped: the UI, every shared-code change and the adapters, with
  four agents (streams A to D).
- Each agent works in its own sparse worktree of `paddock`
  (`git sparse-checkout set --cone crates ui scripts docs conformance apps/pacenotes`), its own `CARGO_TARGET_DIR`, its own
  e2e port, `CARGO_BUILD_JOBS=2`, heavy steps one at a time (rule 41). The coordinator holds Pacenotes packages first when
  a release needs the machine (rule 1).
- **Registration point.** The app's contributions are registered in one file, `apps/pacenotes/app/src/app/contributions.ts`
  (each feature's `provide…Contributions()`), and its commands and plugins in `app/src-tauri/src/commands.rs` (the
  command table) and `app/src-tauri/src/lib.rs`. These belong to the integrating agent (D0 creates them); every package
  that adds a feature adds **one declared line** to each of them that it needs, named in its brief.
- Each package owns the files listed; nothing outside them. Shared integration points (`apps/pacenotes/Cargo.toml`,
  `crates/Cargo.toml`, `lib.rs` files, the DTO and command tables, `public-api.ts`, the root `package.json`, `.gitea/`,
  `CODEOWNERS`) belong to the integrating agent, or take one declared line.
- Every package starts with its failing test (rule 5) and ends with its gates: `cargo fmt --check`, `cargo clippy
  --all-targets -- -D warnings`, `cargo test` and `cargo deny check` in the Pacenotes workspace for the touched crates; core
  crates at 99 % lines or more; from stage 2, F1's port checks; for UI work `npx ng lint`, `ng test`, `ng build`, Angular
  coverage at 80 % or more overall and near 100 % for stores, services and pure utilities, `npm audit --omit=dev
  --audit-level=high` and the touched Playwright specs in Chromium (rule 3); for release work
  `node --test scripts/release/publish.test.mjs` (the gates of `apps/pacenotes/AGENTS.md`); `node scripts/affected.mjs origin/main --explain` lists what else to run.
  A shared crate or package changed by a Pacenotes package keeps its own tests in `crates/` or `ui/` (rule 43), is adopted
  by its other users in the same change (rule 42) and carries its CHANGELOG entry.
- **Shared-code limit while another app releases (rule 1).** The frozen set of a release is declared for the
  release-freeze check ([workbench move plan](../../../../docs/plans/2026-10-29-shared-workbench-move.md) §4 and package
  0a); a Pacenotes package never changes a frozen path.
- Sizes are agent-days of focused work with tests, ±50 %. Roles: implementer (Sonnet) unless marked designer or researcher
  (Opus); every package is reviewed by an agent that did not write it (ER-25).

### 4.2 Streams in stage 2

| Stream | Owns |
|---|---|
| A | index, understand, graph, coverage engine, the port checks and budgets |
| B | search, semantic index, language servers, diagrams |
| C | rules, review, the Debrief bridge, requirements |
| D | the app (UI and `src-tauri`), MCP, the views of every function |

### 4.3 Stage 1: pure Rust crates beside Pitwall 1.1 and Debrief 1.2 (one or two agents; about 41 agent-days, five to seven weeks)

Order: top to bottom; two packages run at once when load allows. Designs and the spike are documents and numbers only.

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| P0.1 | App files only: the Cargo workspace; AGENTS.md "Plugin architecture" amended for the in-engine first rule; the **trap repository** of `docs/audits/pacenotes.md` PN1 as a generated fixture (a build script, a `.env`, a symlink escaping the workspace) and its test, ignored with a reason until A3a | `apps/pacenotes/Cargo.toml`, `Cargo.lock`, `apps/pacenotes/AGENTS.md`, `apps/pacenotes/tests/trap/` | the trap test exists and fails (ignored with the reason "A3a") | none | 1 |
| P0.1b | CI leg and the `cargo-metadata` check (integrating agent; details below) | `apps/pacenotes/paddock.ci.json`, `scripts/affected.test.mjs`, `CODEOWNERS`, `conformance/src/checks/Cnn-cargo-metadata-off.mjs` and its test | `affected.test.mjs` sees the leg and marks Pacenotes on a change under `apps/pacenotes/crates/`; a fixture manifest enabling `cargo-metadata` fails | P0.1 | 1 |
| D-index | Index schema, core types and port signatures (designer) | ADR draft in `apps/pacenotes/docs/adr/` | the ADR lists one test per port contract | none | 1 |
| D-rules | Joint rules model note with the conformance owner (designer; ADR 0022 §6 phase 4) | `docs/research/` note | the note maps every conformance and Pacenotes field | none | 1 |
| D-diagram | Canonical draw.io form and the round-trip merge rules (designer; proposal §9.2) | ADR draft | the ADR names each hand edit that must survive and how it is detected | none | 1 |
| R1 | Vector-index spike (researcher): hannoy, usearch, sqlite-vec, libSQL on generated repositories; RAM, build, latency, recall@10, update and delete cost, Windows build (proposal §21.3) | results block in `apps/pacenotes/docs/performance.md` (rule 48); ADR draft | numbers decide; no product code | a quiet machine | 2 |
| A1 | `pacenotes-core` (backend note BN1, BN8): ids, `Location`, `Rev`, `Confidence`, `Tier`, finding state; port traits; language-pack and rule schema types with validation | `apps/pacenotes/crates/pacenotes-core/` | ids stay stable under rule 14 after a rename matched by fingerprint; a pack with an unpinned download is refused with a message | P0.1, D-index | 3 |
| FN0 | `pacenotes-function-api`: contributions, host calls, serialisable messages, typed errors (section 2.1) | `apps/pacenotes/crates/pacenotes-function-api/` | every message type round-trips through postcard; a host call has a budget and a cancel token | A1 | 1.5 |
| A2 | `pacenotes-parse` library (BN2): tree-sitter grammars in the decided order (Go, Java, Kotlin, C, C++, then C#, TypeScript/JavaScript, Rust, Python; grammar licences in an ADR), symbols, outline, imports, call candidates, chunks; limits. No worker binary yet | `apps/pacenotes/crates/pacenotes-parse/` | golden outline per language on generated files; arbitrary bytes never panic and respect the node cap (C36) | A1 | 5.5 |
| C2 | `pacenotes-git` (BN4): gix behind `History`; log, blame, churn, hot spots, read at a revision, diffs | `apps/pacenotes/crates/pacenotes-git/` | hot-spot ranking over a repository built in `paddock-testkit::Scratch`; the `git` binary is never run on the analysed repository | A1 | 3 |
| C1 | `pacenotes-rules` (BN7): structural patterns over tree-sitter, SARIF 2.1.0 and rdjson, suppressions with reason and expiry, fingerprint baselines, `rules test`, on the model of D-rules | `apps/pacenotes/crates/pacenotes-rules/` | a rule's own good and bad examples are its test; a clippy SARIF fixture imports to findings; a suppression without a reason is refused | A2, D-rules | 4 |
| A3a | `pacenotes-index`, pure half (BN5): schema v1 (versioned, a corrupt file is rebuilt), fingerprints, the change set from a folder snapshot, secret denylist, canonical paths and symlink refusal; no watcher, no jobs; the trap test un-ignored | `apps/pacenotes/crates/pacenotes-index/` | renaming a file keeps its symbol ids; a damaged database is rebuilt, not trusted; indexing the trap repository runs nothing (a process-spawn seam records no child), leaks nothing (the `.env` content is in no table and no reply) and escapes nothing (no file outside the workspace is read) | A1, A2, P0.1 | 4.5 |
| DG1 | Diagram round-trip prototype (BN6): canonical uncompressed writer, reader for compressed and plain files, merge that keeps hand-moved positions, styles and added shapes, stale elements flagged | `apps/pacenotes/crates/pacenotes-diagram/` | regenerating after a hand edit keeps the edit byte for byte in canonical form; a stale node is flagged, not deleted; the tests read round trips recorded by the user (below) | A1, D-diagram; the user's recordings | 5 |
| RQ1 | Requirement importers, pure: xlsx, docx, csv, md behind `RequirementSource`, with limits (zip-bomb ratio, entry count, no DTD or external entities), and the mapping-pack schema | `apps/pacenotes/crates/pacenotes-req/src/{import,mapping}/` | a zip bomb and a docx with an external entity are refused with a limit error; a generated sheet with merged cells maps by its pack | A1 | 4 |
| CV1 | Coverage readers, pure: LCOV, Cobertura, JaCoCo, llvm-cov JSON, istanbul behind `CoverageSource`; path mapping; revision and tool recorded | `apps/pacenotes/crates/pacenotes-cover/src/import/` | a malformed report is an error, never a panic (C36); two runs of different revisions refuse to blend | A1 | 3 |

- **P0.1b** runs after the Dyno and Spotter tags, at a time the coordinator picks when no release CI is running: the
  `paddock.ci.json` leg (Linux: fmt, clippy, test, deny), `scripts/affected.test.mjs`, the `CODEOWNERS` line, and the
  extended WP-C6 check of ADR 0022 §6, as `conformance/src/checks/Cnn-cargo-metadata-off.mjs` (number at merge, C31),
  that fails when a Pacenotes manifest enables `paddock-repo-graph`'s `cargo-metadata` feature, directly or through
  `cargo tree -e features` (the integrating agent owns it). Stage 1 work merges to `main` only after P0.1b; until then
  its gates run locally.
- **DG1's real round trips are a user task** (ER-14: tests never touch the user's installed apps). The agent writes the
  generated files and a short list of edits; the user opens, edits and saves them in draw.io desktop, draw.io on the web
  and the VS Code extension; the saved files are committed as fixtures, and the tests read only those.

Stage 1 delivers no app and nothing the user can run: it is the parsing, history, rules, index, diagram, requirement and
coverage core with its tests, so stage 2 starts with the hardest pure parts proven.

### 4.4 Stage 2, wave 2: app skeleton, index job, embeddings, shared rules (about 5 days)

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| D0 | Starter in the monorepo, then the app skeleton (stream D) | `scripts/new-app.mjs` (`--in-repo`) and its test; `apps/pacenotes/app/**` from `workbench-demo` with the `pacenotes` accent; root `package.json` line (integrating agent) | an in-repo app builds with path dependencies and appears in `affected.mjs`; the skeleton opens the empty workbench with axe clean in both themes | stage 1 | 1.5 |
| D1 | App wiring on the workbench against the mock backend: contributions (activity items, view stubs with layout, perspectives Understand, Review, Graph, Diagrams, Requirements, Tests), DI backend token, splash, health entries, settings sections with a setting each (rule 44) (D) | `apps/pacenotes/app/src/**` except later packages' views | the workbench lint passes; every perspective opens with each pane at or above its minimum at 1440x900 and 1920x1080 (scenario runner), `expectNothingCovers`, axe | D0 | 3 |
| A3 | Index wiring (A): the parser worker binary on `paddock-ipc`, `-guard`, `-proc`; walk on `paddock-repo-files`, watch on `paddock-watch`, allow-list on `paddock-files`, secret denylist pack; the index job pipeline with phases and Cancel (ADR 0020, 0024) | `apps/pacenotes/crates/pacenotes-parse/src/worker/`, `pacenotes-index/src/{job,watch,store}/` (A3 extends A3a's `pacenotes-index` and keeps its tests) | a worker killed mid-file is an error attributed to the file; Cancel in each phase leaves the previous generation readable; progress never goes backwards; the trap test, re-run through the real walk on `paddock-repo-files` and the worker, still finds nothing run, leaked or escaped | stage 1 | 5 |
| B1 | `paddock-embed` pure half (X55a) (B): BM25, RRF with weights, brute-force cosine top-k, index record, benchmark harness over generated repositories and query sets | `crates/paddock-embed/` (new crate, the `new-crate` checklist; `crates/Cargo.toml` line by the integrating agent) | the harness reproduces ADR 0001's BM25 ordering on a generated corpus; two records with different models refuse to mix | stage 1 | 4 |
| FN1 | `pacenotes-function-api` rebased onto `paddock-plugin-api` and `paddock-view` types (C): contributions, tool schemas and `ViewDoc`s re-exported, Pacenotes' host calls kept as its own extension; a mapping test | `apps/pacenotes/crates/pacenotes-function-api/` | a contribution type that differs from `paddock-plugin-api`'s fails the mapping test | FN0, Debrief 1.2 shipped | 1.5 |
| C3a | `paddock-repo-rules` from the D-rules note (C), with conformance adopting it in the same change (rule 42) and `pacenotes-rules` moved onto it | `crates/paddock-repo-rules/`, `pacenotes-rules`, the conformance reporters it replaces | conformance's report and a Pacenotes SARIF output come from one `Finding` type; every existing conformance test stays green | C1 | 3 |

### 4.5 Wave 3: search, understand, review core, the code view (about 5 days)

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| B2 | `pacenotes-search` (BN3) and the semantic index (B): chunker, FTS5 queries, `paddock-names` matching, RRF through `paddock-embed`; Model2Vec worker under the guard; the model downloaded the first time semantic search is used (not bundled), through `paddock-download` with its SHA-256 checked while writing, as a job with progress and Cancel; the `VectorIndex` adapter chosen by R1; Qwen re-rank port (top 50); the "Deep index with Qwen" job with estimate and confirmation | `apps/pacenotes/crates/pacenotes-search/` | `HttpRequestParser` is found by "http request parser"; a tokenizer panic is an error, not a crash; a download with a wrong hash is discarded and semantic search stays off with a Retry; the deep index cannot start without the confirmation's token | A3, B1, R1 | 8 |
| A4 | `pacenotes-understand` as a function (A): references with confidence, call graph and call-path trace, repo map with budget, entry-point and config recognisers as packs, ownership, complexity, TODO inventory, change impact, orientation and reading order | `apps/pacenotes/crates/pacenotes-understand/` | a dynamic-dispatch edge is "likely", never certain; the repo map stays within its budget; recogniser packs reload without a restart; the crate reaches only `pacenotes-function-api` | A3, FN0 | 5 |
| C3 | `pacenotes-review` as a function (C): diff, branch, range and whole-repo modes; deterministic rules; Markdown rules through the `Llm` port on a diff only, citations required; conversion and hygiene; outputs and CLI exit codes | `apps/pacenotes/crates/pacenotes-review/`, `pacenotes-cli/src/review.rs` | a finding without a resolvable citation is "unverified"; without Pitwall the report lists skipped rules and exits 3; a whole-repo run makes no `Llm` call | C3a, C2 | 5 |
| D-view, D2 | Code viewer and graph canvas design (designer), then the code view and Files/Symbols Explorer providers (D): engine tokens on the shared virtual list, `path:line` navigation, open in editor | ADR draft; `apps/pacenotes/app/src/app/{code,explorer}/` | a 100,000-line generated file scrolls with a stable viewport; no inline style attribute (CSP e2e); long paths truncate in the middle (rule 18) | D1 | 5.5 |

### 4.6 Wave 4: tier 3, graph, the bridge, MCP (about 5 days)

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| B4 | `pacenotes-lsp` and the eight packs (B): client, supervision (timeouts, memory cap, back-off, quarantine, idle stop), answers cached by revision, trust record and dialog, pinned downloads on click | `apps/pacenotes/crates/pacenotes-lsp/`, `packs/lsp/*.toml`, `app/src/app/trust/` | an untrusted repository never starts a server (fake server, process table), and the trap test, extended with a language-server pack for it, starts none; three crashes quarantine it and tier 2 stays visible; revoke kills the tree | A3 | 5 |
| A5 | `pacenotes-graph` (A): model, zoom aggregation, cycles, shortest path, diff, layer rules as review findings, bounded subgraph layout, draw.io and SVG export through the diagram writer | `apps/pacenotes/crates/pacenotes-graph/` | a 5,000-node generated graph returns at most the viewport's cap plus hidden counts; a forbidden edge becomes a finding with the path as evidence | A4 | 5 |
| C4 | Debrief bridge as a function (C): the shared stack-trace crate extracted from `debrief-core/src/exceptions.rs` with Debrief adopting it in the same change; log-site template match, exception reachability, config usage, throw sites, bulk enrichment; deep links through `paddock-siblings` | new shared crate under `crates/`, Debrief's `exceptions.rs` users, `apps/pacenotes/crates/pacenotes-check/` | `exception_not_throwable_here` is never certain under dynamic dispatch; a changed format string gives `template_changed`; Debrief's exception tests stay green on the shared crate | A3, FN0 | 5 |
| D3 | `pacenotes-mcp` on `paddock-mcp` (D): read-only tools of §13 and §22 for search, understand, review, bridge; keys, grants, budgets, call log; the MCP clients dialog | `apps/pacenotes/crates/pacenotes-mcp/`, `app/src/app/mcp/` | a reply never contains a denylisted secret; an over-budget call returns `truncated` with a cursor; no tool that writes is listed | A4, C3 | 4 |

### 4.7 Wave 5: diagrams, requirements and coverage engines, the main views (about 5 days)

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| DG2 | `paddock-drawio` (B): canonical writer, theme tokens, UML and C4 shapes, golden tests against the family's `.mjs` output (ADR 0022 §6), adopted by `pacenotes-diagram` in the same change; then the diagram kinds: C4 from manifests, module, dependency, class, call graph for a symbol, sequence from a trace; template and layered layouts with the router; size limits and splitting | `crates/paddock-drawio/`, `apps/pacenotes/crates/pacenotes-diagram/` | the writer's output equals the `.mjs` oracle on the family's diagram data; a 40-node module graph splits into diagrams within 1600x1200 with a shared legend | DG1, A5 | 6 |
| RQ2 | Requirements engine (C): mapping packs on `paddock-packs`, tag links, the suspect model, traceability matrix, coverage reports, orphans, impact view data, CSV export | `apps/pacenotes/crates/pacenotes-req/` | editing a requirement's text marks its links suspect; a tag in a test name links requirement and test; the matrix export is deterministic | RQ1, A4 | 5 |
| CV2 | Coverage engine (A): the readers moved next to `paddock-testplan` with their tests (rule 42), patch coverage, coverage per symbol and requirement, risk hot spots, untested throw and log sites, test inventory from pack conventions | the shared readers' crate, `apps/pacenotes/crates/pacenotes-cover/` | an added line without coverage is listed in patch coverage; a report from another revision is marked stale, never blended | CV1, A4, C4 | 5 |
| D4 | Views on the real engine (D): Orientation, Search results, References and Call path, Jobs, status items (index progress, tier per language, MCP clients, Pitwall), Index health, Settings sections | `apps/pacenotes/app/src/app/{orientation,search,references,status}/`, `app/src-tauri/src/commands/` | empty, loading, error-with-Retry and loaded states; a running index shows phase and count from the first event; "Still working" after silence, never a timeout | D2, A4, D3 | 5 |

### 4.8 Wave 6: review integration, diagram diff, graph and assistant (about 5 days)

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| DG3 | Diagram diff between commits, SVG preview geometry, `diagram_status`, the "diagram stale" review check (B) | `pacenotes-diagram/src/{diff,preview,status}/` | a removed module shows as removed in the diff overlay; a stale `docs/diagrams` file is a review finding | DG2 | 5 |
| C5 | Review integration (C): requirement-touched, diagram-stale and test-gap checks; review setup (form, modeless; the run is a job); findings in `pd-table` with evidence, suggested diff, suppress with reason, baseline; the rule view | `pacenotes-review/src/checks/`, `apps/pacenotes/app/src/app/review/` | a PR touching linked code flags its requirement; a suppression without a reason cannot be saved; the findings header stays on top (C43, `expectHeaderOnTop`) | C3, RQ2, CV2, DG3 | 6 |
| A6 | Views that need coverage and requirements (A): test map and coverage overlay data, untested hot spots, "seen in production logs, never covered" for the bridge | `pacenotes-understand/src/views/`, `pacenotes-check/src/coverage.rs` | an uncovered throw site that Debrief reports is listed; the overlay is empty with a next step when no coverage is imported | CV2, C4 | 4 |
| D5 | Graph view, palette providers (go to file, `@` symbol, `#` text), the assistant (`AssistantTool`s, context chips, citations), pop-out of the graph and code views (D) | `apps/pacenotes/app/src/app/{graph,palette,assistant}/`, `app/src-tauri/src/assistant/` | a slow palette provider is cancelled on the next keystroke; a write tool is never offered to the model; answers without a citation are marked unverified | A5, B2, D4 | 5 |

### 4.9 Wave 7: the function views and the port checks (about 5 days)

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| DG4 | Diagram UI (B): preview, diff overlay, "Open in draw.io" (validated path, with a timeout), write with a diff preview and confirmation before removal | `apps/pacenotes/app/src/app/diagrams/` | nothing is written before the preview is accepted; a file changed on disk since the preview is not overwritten | DG3, D4 | 4 |
| RQ3 | Requirements UI (C): import with the mapping editor and live preview, matrix, impact view, link confirm and reject | `apps/pacenotes/app/src/app/requirements/` | nothing is auto-accepted; a document over the limit shows the limit and its setting (ADR 0019) | RQ2, D4 | 5 |
| D6 | Coverage and tests UI, and the MCP tools for diagrams, requirements and tests (D) | `apps/pacenotes/app/src/app/coverage/`, `pacenotes-mcp/src/{diagrams,requirements,tests}.rs` | `generate_diagram` returns XML as text and writes nothing; `patch_coverage` matches the engine's | CV2, RQ2, DG3, D3 | 4 |
| F1 | The function port, checked (A); details below the table | `apps/pacenotes/function-port.toml`, `scripts/check-function-port.*`, `pacenotes-function-api/tests/`, `apps/pacenotes/tests/writer/` | a planted dependency of `pacenotes-review` on `paddock-pitwall` fails the check; a function blocked inside or after a host call stops neither commits nor a WAL checkpoint | A4, C3, C4, RQ2, CV2, DG2, FN1 | 3 |
| F2 | Budgets (rule 16, rule 48): startup, time to first result, index throughput, query latency, memory as physical footprint (rule 26), on generated repositories; nightly (A) | `apps/pacenotes/benches/`, `docs/performance.md` | a budget breach fails the nightly job | A3, B2 | 2 |
| F2b | ADR 0001's measurement plan (B): BM25 + Model2Vec against + Qwen re-rank of the top 50 against the deep index (recall@5, MRR, latency); indexing time, peak memory and index size per 1M lines; re-rank latency on CPU-only machines; results in `performance.md` and ADR 0001 | `apps/pacenotes/benches/search/`, `docs/performance.md`, `docs/adr/0001-embeddings.md` | the harness fails when the default no longer meets ADR 0001's bar | B2, F2 | 2 |

F1 in detail:

- The allow-list in `apps/pacenotes/function-port.toml`: `pacenotes-function-api`, `serde`, `thiserror` and pure helpers
  named one by one with a reason. A `cargo tree` check in Pacenotes' gate fails any other dependency of a function crate;
  packs, `paddock-pitwall`, `paddock-archive`, `paddock-drawio`, the shared readers and every Pacenotes adapter crate are
  host-side.
- A postcard round trip of every port message.
- In the engine: panic attribution and quarantine through `paddock-guard` (a panicking function is attributed and
  quarantined while the index stays usable).
- The writer rules of section 2.1, tested: a function that blocks forever inside a host call, and one that blocks after
  its last host call, stop neither the writer actor's next commits nor a WAL checkpoint (`PRAGMA wal_checkpoint(TRUNCATE)`
  succeeds and the WAL stays under a bounded size while the function is still blocked); opening a second write
  connection outside the actor fails (a test over the crate's public API).
- A second `cargo tree` check: only `pacenotes-index` depends on `rusqlite` or `libsqlite3-sys`, directly or
  transitively, in the Pacenotes workspace; any other crate that reaches either fails the gate, so no other code can
  open a database connection at all.
- Not in v1: hang and memory containment tests (they need a separate process; the packaging work after v1).

### 4.10 Wave 7b: headless, admin, the other views, Debrief's side of the bridge (about 21 agent-days; about seven days at four agents: stream A 7, B 5, C 4, D 5)

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| H1 | Headless CLI and service (A): `pacenotes index`, `search`, `review`, `trust` and `untrust` (explicit flag), `serve --stdio`; the single-writer lock shared with the app (WAL): only the process that holds it runs a writer actor; `paddock-cli` exit codes and `--json` | `apps/pacenotes/crates/pacenotes-cli/` | a second writer waits or refuses with a message, never corrupts; `trust` without its flag refuses | A3, C3, D3 | 4 |
| VW1a | Structure views' UI (B): architecture map, API surface, entry points, change impact; tables in `pd-table`, CSV and Markdown export from the engine | `apps/pacenotes/app/src/app/views/structure/` | each view's four states; every row opens at `path:line`; an export equals the engine's | D4, A4, A5 | 3 |
| VW1b | Inventory views' UI (A): hot spots, owners, complexity, TODO, dependency and config inventories, reading tour | `apps/pacenotes/app/src/app/views/inventories/` | as VW1a | D4, A4 | 3 |
| MC1 | MCP loopback HTTP transport (B): Streamable HTTP on loopback with bearer keys from `paddock-secrets`, request and result caps, the call log view; the Pitwall toolsets (`orient`, `code`, `review`, `diagrams`, `requirements`, `tests`) are Pitwall work, listed in `apps/pitwall/docs/roadmap.md` Phase 4 (about 2 days there, not counted here) | `pacenotes-mcp/src/http/`, `app/src/app/mcp/log/` | a request without a key or from a non-loopback address is refused; an oversized request is refused before parsing | D3 | 2 |
| AD1 | Admin (D): index health (unparsed files with reasons, tier per language, rebuild), settings export and import, pack manager, ignore editor, the "Low memory" preset, autosave and versioned state of review and links | `apps/pacenotes/app/src/app/admin/`, `app/src-tauri/src/admin/` | a corrupt settings file loads defaults with one notification; the preset changes each of its knobs | D4, B4 | 5 |
| BR1 | Debrief's side of the bridge (C; Debrief work, in **Debrief 1.3**, the release after 1.2): Debrief asks Pacenotes over MCP for log sites, exceptions and config keys and shows the verdict with evidence; the `pacenotes://` deep link both ways through `paddock-siblings`; "not installed" with a next step | new `apps/debrief/app/src-tauri/src/investigate/code_check.rs` (the MCP client call and the sibling hand-off) with one line in `investigate/mod.rs`; `apps/debrief/app/src/app/investigate/investigate-backend.ts` and `investigate/mock/` (the backend call and its mock); new `investigate/code-check/` (the verdict panel and its spec); one declared line each in Debrief's command table and contributions; `apps/debrief/app/e2e/code-check.e2e.ts`; the line in `apps/debrief/docs/roadmap.md` | without Pacenotes, Debrief shows "not installed" with a next step; a verdict is shown with its confidence | C4, D3 | 4 |

### 4.11 Wave 8: release (about 5 days)

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| F3 | Own workflow `pacenotes.yml` (gates on three OS, bundles, `--self-test` for every sidecar, ad-hoc sealing and `codesign --verify`; rule 23), release notes, user guide, screenshots and diagrams with independent review (rule 22) | `.gitea/workflows/pacenotes.yml` (integrating agent), `apps/pacenotes/docs/**`, `CHANGELOG.md` | C04, C08, C24, C29, C34 green on a dry run | all | 4 |
| F4 | Safety net on the combined result: full suite in Chromium and WebKit, real-app smoke on three OS, robustness triggers (hostile grammar input, symlink escape, corrupt database, crashing server, zip bomb, hostile draw.io file, malformed coverage, prompt-injection fixtures in code and requirement text; proposal §16) | `apps/pacenotes/app/e2e/`, `crates/*/tests/` | each robustness behaviour has a test that triggers it | all | 4 |
| F5 | Fixes found by F3 and F4 | as found | each fix starts with its failing test | F3, F4 | 5 |

### 4.12 Totals

| Stage | Agent-days (±50 %) | Calendar |
|---|---|---|
| Stage 1, beside Pitwall 1.1 and Debrief 1.2 | about 41 | five to seven weeks at one or two agents |
| Stage 2, waves 2-8 | about 156 | nine to eleven weeks at four agents if nothing waits |
| Package sum | about 197 | |
| Review and CI overhead (about 20 %) | about 40 | |
| **Planning figure for the full v1** | **230 to 270** | re-estimated after stage 1 with the measured pace |

Not counted: the Pitwall toolsets (about 2 days in Pitwall's release) and the out-of-process packaging of the functions
(after v1). The proposal's own estimate for this scope is 100-125k lines of Rust and 22-26 weeks elapsed (proposal §19);
the planning figure at four agents is in the same range once waits on Debrief 1.2 and review rounds are included.

## 5. ADRs to write (Pacenotes, numbered at merge)

1. Index schema, ids and ports (D-index).
2. The function port and the in-engine first rule (section 2.1; amends AGENTS.md "Plugin architecture").
3. Vector index engine (R1).
4. Canonical draw.io form and round-trip merge (D-diagram; proposal §9.2, the largest uncertainty after reference quality).
5. Requirement links and the suspect model; mapping packs (proposal §11).
6. Coverage import and path mapping (proposal §12).
7. Code viewer and graph canvas: what Material does not cover and why (D-view).
8. Language-server trust and supervision (proposal §20.4, decided; the ADR records the mechanism and its tests).
9. Grammar, server, model and document-parser licences (AGENTS.md: record each, verified against the official repository).

## 6. Risks

| Risk | Mitigation |
|---|---|
| Scope: the full v1 is larger than Debrief (proposal §19: 100-125k lines of Rust) | internal milestones in the phase order; no v1.1 row slips in; the estimate is re-checked after stage 1 with real numbers |
| Diagram round trip: draw.io behaviours not yet observed (ids changed on paste, compressed saves, label edits) | the round-trip prototype first, in stage 1 (DG1), with real round trips on desktop, web and VS Code before the emitter is built (proposal §17) |
| Messy requirement documents (merged cells, tracked changes, numbering by style) | mapping packs with a live preview, nothing auto-accepted, v1 limited to xlsx/docx/csv/md; hardened parsers in the worker (zip bombs, no DTDs) |
| Coverage semantics differ between tools; reports from other commits mislead | the revision and tool recorded per run, staleness marks, a path-mapping view, runs never blended silently |
| Function code reaches past the port, so the later move is not mechanical | F1's dependency and serialisation checks run in every Pacenotes gate from stage 2 on |
| Shared crates change under Pacenotes during Debrief 1.2 (`paddock-jobs`, `-proc`, `-ipc`, `-plugin-*`, `-packs`, `-pitwall`) | stage 1 is pure app crates on shared crates 1.2 does not touch; stage 2, with every shared change, starts after 1.2 has shipped; until then every process, job and plugin concern stays behind a Pacenotes port until 1.2 merges; the "waits for 1.2" marks |
| The vector engine is not chosen | `VectorIndex` port; brute-force cosine from `paddock-embed` works until R1's ADR |
| Language servers use gigabytes and run build logic | trust gate, one server at a time, memory cap and idle stop, answers cached; the "Low memory" preset |
| Approximate references mislead | confidence labels everywhere; tier shown per view; verdicts never claim certainty under dynamic dispatch |
| Rule 46: Markdown rules, summaries or requirement links drift into per-item LLM work | section 2.5 table; a review test that a whole-repo run makes no `Llm` call |
| Code viewer vs CSP (no inline style attributes) | D-view spike before D2; engine tokens with class names only |
| Grammar licences (some tree-sitter grammars are not MIT) | ADR 9 before A2 merges; `cargo deny` with the grammar crates |
| UI drift from Debrief while Debrief moves onto the workbench | Pacenotes uses only the shared entry points; any gap is a shared change through the integrating agent, never an app copy |
| Machine load with Debrief 1.2 and Pacenotes in parallel | the coordinator holds Pacenotes packages first (rule 1); `CARGO_BUILD_JOBS=2`; heavy steps one at a time |

## 7. Decisions and open questions

### 7.1 Decided by the user (2026-10-29)

1. **Timing.** Once Dyno 1.0 and Spotter 1.0 have shipped, the pure Rust crates (stage 1) may run beside Debrief 1.2 with
   one or two agents, within load and CI limits. The UI and every shared-code change wait for Debrief 1.2 (stage 2).
2. **First release.** The full v1, including Diagrams, Requirements and Tests & coverage. No cut-down release.
3. **Functions.** Understand, Review and the Debrief bridge (and the other functions) run in the engine first, behind the
   function port, and are packaged as out-of-process plugins later. The port boundary is strict so that the move is
   mechanical (section 2.1, package F1). AGENTS.md's "Plugin architecture" is amended to match (package P0.1). v1
   claims in-process panic attribution, quarantine, a time budget and cooperative cancel only; hang and memory
   containment tests come with the packaging after v1.
4. **Tier-2 languages, in this order:** Go, Java and Kotlin, C and C++; then the other v1 languages (C#,
   TypeScript/JavaScript, Rust, Python). Kotlin gets a tier-2 grammar in v1; its tier-3 language pack stays Later
   (proposal §20.4), so this adds one grammar to the eight packs.
5. **The embedding model** (potion-code-16M-v2) is downloaded the first time someone uses semantic search, not bundled in
   the installer. Semantic search is opt-in per repository. The download is verified (SHA-256 while writing) and runs as
   a job with progress and Cancel (package B2). Recorded in ADR 0001 as an amendment of 2026-10-29 (it answers the
   ADR's "decide at packaging time").

### 7.2 Open questions for the user

1. **Semantic search caps.** Semantic search is opt-in per repository (ADR 0001, AGENTS.md); what are the default
   per-repository caps (proposal §21.1 suggests 500k chunks and 2 GB)?
2. **Review in CI without Pitwall.** May Markdown rules run in CI when a Pitwall is reachable, or does CI run only
   deterministic rules (proposal question 30)?
3. **Markdown rule sources.** `AGENTS.md` read as is; add `REVIEW.md` and `.pacenotes/rules/*.md`; honour `CLAUDE.md`,
   Copilot and Cursor files as read-only sources behind toggles (proposal questions 20, 21)?
4. **Framework recognisers.** Which of ASP.NET routes and DI, Spring routes and beans, Express, Django/FastAPI/Flask and
   Go `net/http`/gin routes (proposal §20.4)?
5. **Default language servers.** csharp-ls for C# until the Roslyn binary's terms are confirmed; pyright or basedpyright
   for Python (proposal question 31)?
6. **Dependency graph kinds.** Internal modules, packages, files, symbols, imports and calls first; services, tables,
   topics and endpoints as recognisers grow (proposal question 26)?
7. **Canonical draw.io files.** May Pacenotes rewrite `.drawio` files into the canonical uncompressed one-cell-per-line
   form (proposal question 13)?
8. **Requirement samples.** Which requirement documents do you have (Word headings, Excel columns), and may the first
    mapping packs be designed on anonymised same-shape samples (proposal question 11)?
9. **Name and trademark.** The trademark search for "Pacenotes" is still open (proposal question 1); it is needed before
    a public release.

## Appendix A. The backend note of 2026-10-28, folded in

`/Volumes/d7sst7/dev-targets/review/2026-10-28-pacenotes-sighter-backend-plan.md` (outside the repo) planned the backend
work that was safe to start before Debrief 1.2. Its Pacenotes content, as it maps to this plan:

| Backend note | Here |
|---|---|
| W0 app workspace and CI leg | P0.1 and P0.1b |
| BN1 `pacenotes-core`, BN8 language-pack schema | A1 |
| BN2 syntax tier, in-process library | A2 (the worker binary in A3) |
| BN3 chunker and lexical search through `paddock-names` | B2 |
| BN4 `pacenotes-git` behind `History` | C2 |
| BN5 index schema and incremental diff, pure half | A3a |
| BN6 diagram round-trip prototype | DG1 |
| BN7 structural rules and SARIF | C1 (moved onto `paddock-repo-rules` in C3a) |
| R1 vector-index spike | R1 |
| X55a `paddock-embed` pure half, "built now" (its D2) | B1, in stage 2: shared code waits for Debrief 1.2 (decided 2026-10-29) |

Its "wait" list, still true: worker processes (parse, embed) wait for `paddock-ipc`, `-proc` and `-guard` in 1.2; jobs for
`paddock-jobs` keys and the ADR 0020 executor; plugins for `paddock-plugin-*`; Pitwall uses for Debrief's move onto
`paddock-pitwall`; pack loading for the N3 `PackKind`; stack-trace parsing for the shared crate after 1.2; LSP
supervision, MCP servers and the Tauri app for the shell and 1.2.

Its rule 46 conflicts, all resolved here (section 2.5): no per-chunk summaries; Model2Vec + BM25 as the indexing
default with Qwen only re-ranking or behind the deep-index confirmation (its D1, now ADR 0001's amendment); Markdown
rules only on a diff or a selection; diagram captions and requirement links on click.

Its shared-first notes: `paddock-names` for identifier search (with a drift note: Debrief's own `names` gains
abbreviations in 1.2 and the rule 42 survey reconciles them after); stack traces shared after 1.2; JUnit and TRX next to
`paddock-testplan`; packs on `paddock-packs` after N3; the draw.io round trip stays in Pacenotes (only its writer moves to
`paddock-drawio`). Its open D7 (first tier-2 languages) is decided: section 7.1, item 4.
