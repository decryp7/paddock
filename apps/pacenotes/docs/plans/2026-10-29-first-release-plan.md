# Plan: Pacenotes first release

- Status: Plan, 2026-10-29. Nothing here is built. Product code starts once Dyno 1.0 and Spotter 1.0 have shipped (the
  brief of 2026-10-29); section 7 asks the user to confirm that against the order of 2026-10-28, which puts Pitwall 1.1 and
  Debrief 1.2 first.
- App: Pacenotes (`apps/pacenotes`). Rule 1 holds: while another app is releasing, Pacenotes work runs only within load,
  CI capacity and the shared-code limits of section 4.1.
- Scope: the first public release, which [architecture-direction.md](../architecture-direction.md) ("Phasing") calls
  **v1: core, Understand, Review, Debrief bridge and the core language packs**. Diagrams, Requirements and Tests & coverage
  follow as later phases. Every feature below is taken from Pacenotes' own docs; where they disagree, the disagreement is
  an open question (section 7), not a choice made here.
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
  `/Volumes/d7sst7/dev-targets/review/2026-10-28-pacenotes-sighter-backend-plan.md` (outside the repo; its SAFE NOW
  packages PN1-PN8 are folded in below).
- Companion plan: [the shared workbench move](../../../../docs/plans/2026-10-29-shared-workbench-move.md). Pacenotes' UI
  is built on the shared workbench. Its first screens need only what the workbench already has, so Pacenotes does not
  wait for Debrief's move (section 4.1).

## 0. Facts this plan starts from (checked 2026-10-29 on `origin/main` `912ad7c12`)

| Fact | Evidence |
|---|---|
| Pacenotes has no code, no Cargo workspace and no CI leg | `apps/pacenotes/` holds docs, `deny.toml`, `rustfmt.toml`, `cliff.toml` and `scripts/release`; `paddock.ci.json` is a `$comment` stub |
| The v1 cut is core, Understand, Review, the Debrief bridge and the eight core language packs; then Diagrams, then Requirements, then Tests & coverage | `architecture-direction.md` "Phasing" (decided 2026-10-23) |
| The proposal's own feature tables mark more as "v1" than that cut (diagrams, requirements, coverage import) | `proposal.md` §6.4, §6.6, §6.7, and §19's milestones M3-M5 |
| Search default: Model2Vec potion-code-16M-v2 + BM25 on the CPU; Qwen through Pitwall re-ranks a query's top ~50 and runs the opt-in "Deep index with Qwen" job; no per-chunk LLM summaries | ADR 0001 amendment of 2026-10-28; measurements in [performance.md](../performance.md) "2026-10-25 (amended 2026-10-28), embedding engines" |
| The shared workbench exists and has a reference app; the shell exists and Dyno and Spotter are moving onto it | `ui/projects/paddock-ui/workbench/` (frame, dock, explorer, palette, panels, assistant, testing); `ui/projects/workbench-demo/`; crates `paddock-workbench`, `paddock-panels`, `paddock-assistant`; branches `dyno/shell-1.0`, `spotter/shell-1.0` |
| A starter exists, but it writes a new repository on the registries, not an app in the monorepo | `scripts/new-app.mjs` header: "into a new repository ... with the shell crates and the UI package taken from the registries" |
| Shared crates that Pacenotes was named as a user of are built: `paddock-repo-files`, `paddock-repo-graph`, `paddock-names`, `paddock-mcp`, `paddock-pitwall`, `paddock-assistant`, `paddock-guard`, `paddock-ipc`, `paddock-plugin-*`, `paddock-view`, `paddock-packs`, `paddock-watch` | `crates/README.md` table |
| `paddock-embed` (X55) and `paddock-limits` do not exist yet; Debrief 1.2 changes `paddock-jobs`, `paddock-proc`, `paddock-ipc` and the `paddock-plugin-*` crates | `crates/` listing; `docs/plans/debrief-1-2.md` packages 1.4, 2.2, 2.6, PH1, PH2a |
| The Pacenotes and Sighter accents are in the tokens | `feat/accents-pacenotes-sighter` is merged; `_tokens.scss` names `pacenotes` |

## 1. What the first release contains

Taken from the docs. "Source" is where the feature is decided; "v1" in the proposal tables is only included when it falls
inside the architecture-direction cut.

| Area | In the first release | Source |
|---|---|---|
| Workspaces and safety | Open a folder; workspace allow-list; canonical paths, symlink escape refused; secret denylist and redaction of every reply; never execute repository code; read-only towards repositories; local only, no telemetry | AGENTS.md "Pacenotes-specific rules"; proposal §6.10 |
| Index, tier 1 (text) | Incremental index of every file: full-text and regex search, file tree, git history, hot spots, docs, build-system detection from manifests only | proposal §20.1, §20.2 step 1 |
| Index, tier 2 (syntax) | tree-sitter symbols, outline, imports, approximate (confidence-labelled) references and call graph, syntax-aware chunks; parsing in worker processes with limits | proposal §6.1, §14, §20.1 |
| Index, tier 3 (semantic) | LSP client with eight built-in language packs (C, C++, C#, Java, TypeScript/JavaScript, Rust, Python, Go); servers never bundled; started only after "Trust this repository"; supervised, cached by index revision | proposal §20.4 (decision 2026-10-23); AGENTS.md trust rule |
| Search | Text, structural and semantic search fused with RRF; Model2Vec + BM25 by default, per repository opt-in; Qwen re-rank of the top ~50 when Pitwall runs; "Deep index with Qwen" as a confirmed job | ADR 0001 and its amendment; proposal §21 |
| Understand | Repo map with token budget, outline, read symbol, find symbol, references, call graph and call-path trace, stack-trace and log-site resolution, ownership (blame, CODEOWNERS), churn and hot spots, complexity metrics | architecture-direction "The plugins"; proposal §6.1, §6.2 |
| Orientation | One page after indexing: entry points, modules and dependencies, external libraries, hot spots, where the tests are, config keys and env vars, a suggested reading order | proposal §20.2 steps 2 and 4 (the draw.io diagram on that page is a question, section 7) |
| Views | Module and package dependency graph with cycles; call graph and call-path trace; symbol outline and API surface; entry-point inventory (framework recognisers as packs); change impact; hot spots; owners; complexity and size; TODO/FIXME inventory; dependency inventory (versions from manifests); config and env-var inventory; generated reading tour | proposal §22 rows marked v1 that need no diagram, coverage or requirement data |
| Dependency graph | Zoom levels, filters, focus N hops, shortest path, cycles, layer rules from `.pacenotes/architecture.toml` fed to Review, diff mode; layout and aggregation in the engine | proposal §6.1, §23 (advisories and Spotter versions are v1.1) |
| Review | Diff, branch, commit range or whole repository; structural pattern rules, SARIF/rdjson import, Markdown rules (model-checked, needs Pitwall); rules in `.pacenotes/rules/` and per user, with tests; findings with evidence and a suggested diff; suppressions with a reason; baselines; Markdown and SARIF output; CLI exit codes 0/1/2/3 | proposal §6.3, §24 |
| Debrief bridge | `check_log_site`, `check_exception`, `find_config_usage`, `find_throw_sites` with verdicts and confidence; deep links both ways | architecture-direction; proposal §6.5, §10, §13 |
| MCP | Read-only server over stdio and loopback HTTP with per-client keys, grants, budgets, call log; tools of §13 and §22 for the functions above; Pitwall toolsets `orient`, `code`, `review` | proposal §6.8, §13, §22.1 |
| Assistant | Ask box over Pitwall with visible tool calls and `path:line` citations; "needs local AI" state without Pitwall | proposal §6.2, §20.2 step 3; ADR 0013 §1 |
| Headless | `pacenotes` CLI and service (index, search, review in CI, `trust`/`untrust`, `serve --stdio`) | architecture-direction "Core and plugins"; proposal §20.1 |
| Admin | Index health (unparsed files with reasons, tier per language, rebuild), settings export/import, pack manager, ignore editor, "Low memory" preset, autosave and versioned state | proposal §6.10, §21.4 |

**Not in the first release** (later phases or Later in the docs): Diagrams (draw.io round trip, C4, sequence diagrams,
diagram diff); Requirements and traceability; Tests & coverage (coverage import, patch coverage, test map); review checks
that need those (diagram stale, requirement touched, test gap on changed lines); Gitea posting; MCP host; SCIP import;
advisory mirror and Spotter versions; data flow, duplicates, dead code, glossary (v1.1 or Later in §22); the further
proposals of §25.

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
 └── function plugins        Understand, Review, Debrief bridge           (paddock-plugin-host, signed, quarantined)
 pacenotes serve --stdio     the same engine headless, sharing the SQLite file (WAL, single-writer lock)
 Pitwall (optional)          chat, re-rank, deep-index embeddings, over paddock-pitwall
```

The function plugins follow AGENTS.md ("Plugin architecture": out of process, plugin API crate only, one IPC, guarded,
quarantined, signed, declarative views, no web code). Their logic lives in library crates that the plugin binaries link,
so the functions are tested without a process and the packaging is a thin last step (package F1). Whether the first
party's own functions must already be separate processes in the first release is a question for the user (section 7, Q4).

### 2.2 Crates, ports and adapters

The core depends on ports it owns; adapters are thin and replaceable (AGENTS.md "Design principles"; proposal §14). Names
follow proposal §14 under `apps/pacenotes/crates/`.

| Crate | Pure core | Ports it defines (adapters elsewhere) | Uses (shared) |
|---|---|---|---|
| `pacenotes-core` | ids (`SymbolId`, `ChunkId`, stable by rule 14), `Location`, `Rev`, `Confidence`, `Tier`, `Language`, finding state enum, language-pack and rule schemas (types and validation only) | `History`, `Store`, `Embedder`, `VectorIndex`, `LanguageServer`, `Llm`, `Forge` (later) | `paddock-names` types |
| `pacenotes-parse` | tree-sitter extraction (symbols, outline, imports, call candidates, chunks), query packs as files, limits (bytes, nodes, time) | none; the worker binary is its adapter | `paddock-ipc`, `paddock-guard`, `paddock-proc` (worker) |
| `pacenotes-index` | schema v1 and migrations, change set from a snapshot, fingerprints, rename matching (path, then fingerprint) | `Store` (SQLite adapter here) | `paddock-repo-files` (walk, hashing), `paddock-watch` (changes), `paddock-env` (`VersionedFile`), `paddock-files` (allow-list grants) |
| `pacenotes-git` | churn, hot-spot ranking, blame aggregation | `History` adapter on gix (in process; never the `git` binary on the analysed repository) | `paddock-repo-graph`'s `History` port where the shapes agree (ADR 0022 §6) |
| `pacenotes-search` | chunker, BM25/FTS5 query building, identifier matching, RRF, deterministic re-rank score | `Embedder`, `VectorIndex` adapters (Model2Vec worker, Pitwall, the spike's winner) | `paddock-embed` (X55a, new), `paddock-names` |
| `pacenotes-lsp` | LSP client state machine, request/response cache by index revision, trust record | `LanguageServer` adapter: child processes | `paddock-proc` (supervised, memory cap, kill-tree), `paddock-packs` (language packs), `paddock-download`/`paddock-runtime` (pinned server downloads on click) |
| `pacenotes-understand` | references with confidence, call graph and call-path trace, repo map budgeting, recogniser matching, ownership, complexity, change impact, orientation and reading order | none | `paddock-packs` (recogniser packs) |
| `pacenotes-graph` | graph model, aggregation per zoom level, SCC cycles, shortest path, diff, layer-rule evaluation, layout of a bounded subgraph | none | `paddock-repo-graph` (Cargo and npm manifests; its `cargo-metadata` feature stays off, ADR 0022 §6) |
| `pacenotes-rules` | structural patterns over tree-sitter, SARIF 2.1.0 and rdjson reader/writer, suppressions, fingerprint baselines, rule tests | none | the joint rules model with `paddock-repo-rules` (ADR 0022 §6, section 3.3) |
| `pacenotes-review` | collect, ground, run deterministic rules, budgeted Markdown-rule pass, outputs, exit codes | `Llm` (Pitwall adapter in the app, fake in tests) | `paddock-pitwall` |
| `pacenotes-check` | log-site template match, exception reachability, config-key use, verdict and confidence | none | the shared stack-trace parser (section 3.3) |
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
| BM25, RRF with weights, cosine top-k, index record, benchmark harness and query sets (X55a) | new crate `paddock-embed` (pure half first, worker and Pitwall adapter as X55b) | package B1, from Pacenotes' needs | Debrief's N7 uses it (debrief-1-2.md); ADR 0001 amendment |
| Stack-trace parsers (.NET, Java, Python, JS) | a shared crate extracted from `apps/debrief/crates/debrief-core/src/exceptions.rs`, Debrief adopting in the same change | after Debrief 1.2 merges (backend note: mid-1.2 it collides with Debrief branches) | Debrief and Pacenotes parse the same traces |
| One rules model (severities, findings with fix diff, scoped suppressions, fingerprint baselines, SARIF, exit codes) | `paddock-repo-rules` per ADR 0022 §6; the joint rules-model note is written with package D-rules | package D-rules before C1 | conformance and Pacenotes review would otherwise grow two models |
| Typed limit errors | `paddock-limits` (Debrief 1.2 package 1.4) | consume when merged | every bounded input in the engine |
| A monorepo mode for the starter | `scripts/new-app.mjs --in-repo` (path dependencies, root npm workspace entry, `paddock.ci.json`, CODEOWNERS line) | package P0.2 | Sighter starts the same way |
| JUnit, TRX, LCOV, Cobertura readers | next to `paddock-testplan`'s JUnit export | Tests & coverage phase, not this release | Sighter writes TRX |
| `paddock-drawio` (canonical draw.io writer and shapes) | ADR 0022 §6 | Diagrams phase, not this release | the family's diagram tooling |
| A read-only code viewer | stays in Pacenotes | | no second app needs one (rule of three) |

## 4. Work packages

### 4.1 Conventions

- Four agents in parallel (streams A to D), each in its own sparse worktree of `paddock`
  (`git sparse-checkout set --cone crates ui scripts docs conformance apps/pacenotes`), its own `CARGO_TARGET_DIR`, its own
  e2e port, `CARGO_BUILD_JOBS=2`, heavy steps one at a time (rule 41). The coordinator watches load and holds packages when
  another app's release needs the machine.
- Each package owns the files listed; nothing outside them. Shared integration points (`apps/pacenotes/Cargo.toml`,
  `crates/Cargo.toml`, `lib.rs` files, the DTO and command tables, `public-api.ts`, the root `package.json`, `.gitea/`,
  `CODEOWNERS`) belong to the integrating agent, or take one declared line.
- Every package starts with its failing test (rule 5) and ends with its gates: `cargo fmt --check`, `cargo clippy
  --all-targets -- -D warnings`, `cargo test` and `cargo deny check` in the Pacenotes workspace for the touched crates; core
  crates at 99 % lines or more; for UI work `npx ng lint`, `ng test`, `ng build` and the touched Playwright specs in Chromium
  (rule 3); `node scripts/affected.mjs origin/main --explain` lists what else to run. A shared crate or package changed by a
  Pacenotes package keeps its own tests in `crates/` or `ui/` (rule 43) and carries its CHANGELOG entry.
- **Shared-code limit while another app releases (rule 1).** A package that changes shared code may land only if that code
  is not in the releasing app's dependency set. During Dyno 1.0 and Spotter 1.0 the frozen set is listed in
  [the workbench move plan](../../../../docs/plans/2026-10-29-shared-workbench-move.md) §4; while Debrief 1.2 runs, also
  `paddock-jobs`, `-proc`, `-ipc`, `-guard`, `-plugin-*`, `-packs` and `-pitwall` (its packages 1.4, 2.2, 2.6, PH1-PH2b, N3,
  M2a, M2c). Packages that need those are marked "waits for 1.2".
- Sizes are agent-days of focused work with tests, ±50 %. Roles: implementer (Sonnet) unless marked designer or researcher
  (Opus); every package is reviewed by an agent that did not write it (ER-25).

### 4.2 Wave 0: workspace, starter, spikes and designs (about 2 days)

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| P0.1 | Cargo workspace and CI leg (integrating agent) | `apps/pacenotes/Cargo.toml`, `Cargo.lock`, `paddock.ci.json` (a Linux leg of `ci.yml`: fmt, clippy, test, deny), `CODEOWNERS` line | `scripts/affected.test.mjs` sees the leg and marks Pacenotes on a change under `apps/pacenotes/crates/` | none | 0.5 |
| P0.2 | Starter in the monorepo, then the app skeleton | `scripts/new-app.mjs` (`--in-repo`) and its test; `apps/pacenotes/app/**` from `workbench-demo` with the `pacenotes` accent; root `package.json` workspace line (integrating agent) | `new-app.test.mjs`: an in-repo app builds with path dependencies and appears in `affected.mjs`; the skeleton's e2e opens the empty workbench with axe clean in both themes | P0.1 | 1.5 |
| R1 | Vector-index spike (researcher): hannoy, usearch, sqlite-vec, libSQL on generated repositories; RAM, build, query latency, recall@10, update and delete cost, Windows build (proposal §21.3) | notes and a results block in `apps/pacenotes/docs/performance.md` (rule 48); ADR draft | numbers decide; no product code | none; a quiet machine | 2 |
| D-index | Index schema, core types and port signatures (designer) | ADR draft `apps/pacenotes/docs/adr/` (number at merge) | the ADR lists one test per port contract | none | 1 |
| D-rules | Joint rules model note with the conformance owner (designer; ADR 0022 §6 phase 4) | `docs/research/` note | the note maps every conformance and Pacenotes field | none | 1 |
| D-view | Code viewer and graph canvas (designer): CSP-safe drawing of engine tokens on the shared virtual list; graph on ECharts or SVG; Material where it fits | ADR draft | a spike page renders 100,000 generated lines without inline style attributes | none | 1.5 |

### 4.3 Wave 1: pure crates (about 4 days)

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| A1 | `pacenotes-core` (backend note PN1, PN8): ids, `Location`, `Rev`, `Confidence`, `Tier`, finding state; port traits; language-pack and rule schema types with validation | `apps/pacenotes/crates/pacenotes-core/` | ids stay stable under rule 14 after a rename matched by fingerprint; a pack with an unpinned download is refused with a message | P0.1, D-index | 3 |
| B1 | `paddock-embed` pure half (X55a): BM25 (k1, b, split identifiers), RRF with weights, brute-force cosine top-k, index record (engine, model id and hash, dimension, prefixes), benchmark harness over generated repositories and query sets | `crates/paddock-embed/` (new crate per the `new-crate` checklist; `crates/Cargo.toml` line by the integrating agent) | the harness reproduces ADR 0001's BM25 ordering on a generated corpus; two records with different models refuse to mix | P0.1 | 4 |
| C1 | `pacenotes-rules` (PN7): structural patterns over tree-sitter, SARIF 2.1.0 and rdjson, suppressions with reason and expiry, fingerprint baselines, `rules test` | `apps/pacenotes/crates/pacenotes-rules/` | a rule's own good and bad examples are its test; a clippy SARIF fixture imports to findings; a suppression without a reason is refused | A1 (types), D-rules | 4 |
| D1 | App wiring on the workbench against the mock backend: contributions (activity items, view stubs with layout, perspectives Understand, Review, Graph), DI backend token, splash, health entries, settings sections with at least one setting each (rule 44) | `apps/pacenotes/app/src/**` except the views of later packages | workbench lint passes; e2e: each perspective opens with every pane at or above its minimum at 1440x900 and 1920x1080 (scenario runner), `expectNothingCovers`, axe | P0.2 | 3 |

### 4.4 Wave 2: parsing, search, history, the code view (about 5 days)

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| A2 | `pacenotes-parse` (PN2): tree-sitter for the eight languages (grammar licences recorded in an ADR), symbols, outline, imports, call candidates, chunks; limits; the worker binary | `apps/pacenotes/crates/pacenotes-parse/` | golden outline per language on generated files; arbitrary bytes never panic and respect the node cap (C36); a worker killed mid-file is an error attributed to the file | A1; worker binary waits for 1.2 (`paddock-ipc`, `-guard`, `-proc`) | 5 |
| B2 | `pacenotes-search` (PN3): chunker (path, doc, signature, body, 80-line cap), FTS5 queries, identifier search through `paddock-names`, RRF through `paddock-embed` | `apps/pacenotes/crates/pacenotes-search/` | `HttpRequestParser` is found by "http request parser" and "parse request"; chunk boundaries golden | A1, B1 | 3 |
| C2 | `pacenotes-git` (PN4): gix behind `History`; log, blame, churn, hot spots, read at a revision, diff for reviews | `apps/pacenotes/crates/pacenotes-git/` | hot-spot ranking over a repository built in `paddock-testkit::Scratch`; never runs the `git` binary on the analysed repository | A1 | 3 |
| D2 | Code view and Files/Symbols Explorer providers against the mock: engine tokens drawn on the virtual list, `path:line` navigation, open in editor, middle-truncated names | `apps/pacenotes/app/src/app/code/`, `.../explorer/` | 100,000-line file scrolls with a stable viewport; no inline style attribute (CSP e2e); a path longer than the pane truncates in the middle with a tooltip (rule 18) | D1, D-view | 4 |

### 4.5 Wave 3: index, semantic search, review core, MCP (about 5 days)

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| A3 | `pacenotes-index`: SQLite store (WAL, schema versions, corrupt file rebuilt), incremental change sets, walk and watch, secret denylist pack, allow-list, the index job pipeline with phases and Cancel | `apps/pacenotes/crates/pacenotes-index/` | renaming a file keeps its symbol ids; a damaged database is rebuilt, not trusted; Cancel in each phase leaves the previous generation readable; progress never goes backwards | A2, C2; job wiring waits for 1.2 (ADR 0024 keys) | 5 |
| B3 | Embedders and the vector index: Model2Vec worker under the guard, the `VectorIndex` adapter chosen by R1, hybrid retrieval, the Qwen re-rank port (bounded top 50), the "Deep index with Qwen" job with its estimate and confirmation | `apps/pacenotes/crates/pacenotes-search/src/{embed,vector}/`, the worker binary | a tokenizer panic in the worker is an error, not a crash; vectors of two embedders never mix; the deep index cannot start without the confirmation's token | B2, R1; worker waits for 1.2 | 5 |
| C3 | `pacenotes-review`: diff, branch, range and whole-repo modes; deterministic rules; Markdown rules through the `Llm` port on a diff only, citations required; Markdown and SARIF output; CLI exit codes | `apps/pacenotes/crates/pacenotes-review/`, `pacenotes-cli/src/review.rs` | a finding without a resolvable citation is "unverified"; without Pitwall the report lists the skipped rules and exits 3, never 0 | C1, C2 | 4 |
| D3 | `pacenotes-mcp` on `paddock-mcp`: read-only tools `list_workspaces`, `index_status`, `repo_map`, `file_outline`, `read_symbol`, `read_range`, `find_symbol`, `search`, `blame`, `history`; keys, grants, budgets, call log; the MCP clients dialog (tool kind) | `apps/pacenotes/crates/pacenotes-mcp/`, `app/src/app/mcp/` | a tool reply never contains a denylisted secret; an over-budget call returns `truncated` with a cursor; no tool that writes is listed | A1; data from A3 in tests via fakes | 4 |

### 4.6 Wave 4: understand, tier 3, the bridge, the main views (about 5 days)

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| A4 | Understand engine: references with confidence, approximate call graph and call-path trace, repo map with budget, entry-point and config recognisers as packs, ownership, complexity, TODO inventory, change impact, the orientation page and reading order | `apps/pacenotes/crates/pacenotes-understand/` | a dynamic-dispatch call edge is "likely", never certain; the repo map stays within its token budget; recogniser packs reload without a restart | A3 | 5 |
| B4 | `pacenotes-lsp` and the eight packs: client, supervision (timeouts, memory cap, back-off, quarantine, idle stop), answers cached by revision, trust record and dialog, pinned downloads on click | `apps/pacenotes/crates/pacenotes-lsp/`, `packs/lsp/*.toml`, `app/src/app/trust/` | an untrusted repository never starts a server (process table check with a fake server); a server that crashes three times is quarantined and tier 2 stays visible; revoke kills the tree | A3; `paddock-proc` and `-packs` wait for 1.2 | 5 |
| C4 | `pacenotes-check`, the Debrief bridge: log-site template match, exception reachability, config usage, throw sites, verdicts with confidence; MCP tools; deep links through `paddock-siblings` | `apps/pacenotes/crates/pacenotes-check/` | `exception_not_throwable_here` is never certain under dynamic dispatch (proposal §17); a template whose format string changed gives `template_changed` with both versions | A3; stack-trace crate after Debrief 1.2 | 4 |
| D4 | Views on the real engine: Orientation, Search results, References and Call path, Jobs, status items (index progress, tier per language, MCP clients, Pitwall), Index health, Settings sections | `apps/pacenotes/app/src/app/{orientation,search,references,status}/`, `app/src-tauri/src/commands/` | each view's empty, loading, error-with-Retry and loaded states; a running index shows phase and count from the first event; "Still working" after silence, never a timeout | D2, A3, D3 | 5 |

### 4.7 Wave 5: graph, review UI, assistant, packaging (about 5 days)

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| A5 | `pacenotes-graph`: model, zoom aggregation, cycles, shortest path, diff, layer rules from `.pacenotes/architecture.toml` as review findings, bounded subgraph layout | `apps/pacenotes/crates/pacenotes-graph/` | a 5,000-node generated graph returns at most the viewport's cap plus hidden counts; a forbidden edge becomes a finding with the path as evidence | A4 | 5 |
| B5 | Assistant: `AssistantTool`s (`repo_map`, `search`, `read_symbol`, `references`, `call_graph`), context chips, citations as links, the re-rank path in the ask box | `apps/pacenotes/app/src-tauri/src/assistant/`, `app/src/app/assistant/` | a write tool is never offered to the model; Stop cancels within the shared pane's contract; answers without a citation are marked unverified | B3, A4 | 3 |
| C5 | Review UI: setup (form, modeless; the run is a job), findings in `pd-table`, detail with evidence and suggested diff, suppress with reason, baseline, rule view with "why this rule applies" | `apps/pacenotes/app/src/app/review/` | a suppression without a reason cannot be saved; the findings table keeps its header on top (C43, `expectHeaderOnTop`) | C3, D4 | 4 |
| D5 | Graph view, palette providers (go to file, `@` symbol, `#` text), perspectives tuned, pop-out of the graph and code views | `apps/pacenotes/app/src/app/graph/`, `.../palette/` | a slow palette provider is cancelled on the next keystroke; a popped-out graph follows the main window's selection | A5, D4 | 4 |

### 4.8 Wave 6: plugins, budgets, release (about 5 days)

| # | Package | Owned files | Failing test first | Depends | Days |
|---|---|---|---|---|---|
| F1 | Function plugins: Understand, Review and Debrief bridge as signed `.paddockplugin` binaries over `paddock-plugin-api`, contributing MCP tools and `ViewDoc`s; host wiring, quarantine | `apps/pacenotes/plugins/**`, `app/src-tauri/src/plugins.rs` | a plugin that panics is attributed and quarantined after three faults while the index stays usable (`paddock-plugin-test` conformance) | A4, C3, C4; `paddock-plugin-*` after 1.2 | 4 |
| F2 | Budgets (rule 16, rule 48): startup, time to first result, index throughput, query latency, memory as physical footprint (rule 26), on generated repositories; nightly | `apps/pacenotes/benches/`, `docs/performance.md` | a budget breach fails the nightly job | A3, B3 | 2 |
| F3 | Own workflow `pacenotes.yml` (gates on three OS, bundles, `--self-test` for every sidecar, ad-hoc sealing and `codesign --verify`; rule 23), release notes, user guide, screenshots with independent review (rule 22) | `.gitea/workflows/pacenotes.yml` (integrating agent), `apps/pacenotes/docs/**`, `CHANGELOG.md` | C04, C08, C24, C29, C34 green on a dry run | all | 4 |
| F4 | Safety net on the combined result: full suite Chromium and WebKit, real-app smoke on three OS, robustness triggers (hostile grammar input, symlink escape, corrupt database, crashing server, prompt injection fixtures) | `apps/pacenotes/app/e2e/`, `crates/*/tests/` | each robustness behaviour has a test that triggers it | all | 3 |

### 4.9 Totals

About 120 agent-days (±50 %): roughly six to seven calendar weeks at four agents if no package waits, longer while
Debrief 1.2's shared changes are open (the "waits for 1.2" marks above). For comparison, the proposal's own estimate for
its larger v1 (with diagrams, requirements and coverage) was 22 to 26 weeks elapsed (proposal §19); this plan's figure is
for the narrower cut and is optimistic.

## 5. ADRs to write (Pacenotes, numbered at merge)

1. Index schema, ids and ports (D-index).
2. Vector index engine (R1).
3. Code viewer and graph canvas: what Material does not cover and why (D-view).
4. Process model of the functions: plugins from the first release or later (Q4).
5. Language-server trust and supervision (proposal §20.4, decided; the ADR records the mechanism and its tests).
6. Grammar, server and model licences (AGENTS.md: record each, verified against the official repository).

## 6. Risks

| Risk | Mitigation |
|---|---|
| Scope: even the narrower cut is the size of Debrief | waves with internal milestones; no feature from later phases slips in; the user confirms the cut (Q1) |
| Shared crates change under Pacenotes during Debrief 1.2 (`paddock-jobs`, `-proc`, `-ipc`, `-plugin-*`, `-packs`, `-pitwall`) | pure crates first (waves 1-2); every process, job and plugin concern behind a Pacenotes port until 1.2 merges; the "waits for 1.2" marks |
| The vector engine is not chosen | `VectorIndex` port; brute-force cosine from `paddock-embed` works until R1's ADR |
| Language servers use gigabytes and run build logic | trust gate, one server at a time, memory cap and idle stop, answers cached; the "Low memory" preset |
| Approximate references mislead | confidence labels everywhere; tier shown per view; verdicts never claim certainty under dynamic dispatch |
| Rule 46: Markdown rules or summaries drift into per-item LLM work | section 2.5 table; a review test that a whole-repo run makes no `Llm` call |
| Code viewer vs CSP (no inline style attributes) | D-view spike before D2; engine tokens with class names only |
| Grammar licences (some tree-sitter grammars are not MIT) | ADR 6 before A2 merges; `cargo deny` with the grammar crates |
| UI drift from Debrief while Debrief moves onto the workbench | Pacenotes uses only the shared entry points; any gap is a shared change through the integrating agent, never an app copy |
| Machine load with Debrief 1.2 and Pacenotes in parallel | the coordinator holds Pacenotes packages first (rule 1); `CARGO_BUILD_JOBS=2`; heavy steps one at a time |

## 7. Open questions for the user

1. **The first release's cut.** architecture-direction.md says v1 is core, Understand, Review, Debrief bridge and packs,
   and "no cut-down public release inside these phases". Is the first public release phase 1 alone (this plan), or does it
   wait for Diagrams, Requirements and Tests & coverage, as the proposal's §19 milestones suggest?
2. **The orientation page's architecture diagram.** It is a draw.io diagram (§20.2), which belongs to Diagrams. Ship
   orientation without it (a module list and the graph view instead), or pull a minimal diagram into the first release?
3. **Start date.** The brief says Pacenotes code starts after Dyno 1.0 and Spotter 1.0. The order of 2026-10-28 puts
   Pitwall 1.1 and Debrief 1.2 first and says Pacenotes waits until Debrief 1.2 is stable. Which holds? If the brief, may
   waves 0-2 (pure crates) run beside Debrief 1.2 at one or two agents?
4. **Function plugins in the first release.** AGENTS.md makes Understand, Review and the bridge out-of-process plugins and
   forbids in-process plugins. Must the first release ship them as separate processes (F1), or may the first party's
   functions run in the engine, with only parsers, embedder and language servers out of process, and the plugin packaging
   following later?
5. **First languages for tier 2.** Rust, C# and TypeScript first (dogfooding on Paddock), or the proposal's C#, Java and
   TypeScript (§19 M1)? All eight are in the release either way.
6. **Semantic search default.** Off by default per repository (proposal §21.1) or on, now that Model2Vec indexing takes
   seconds (ADR 0001)? And the per-repository caps (500k chunks, 2 GB)?
7. **Review in CI without Pitwall.** May Markdown rules run in CI when a Pitwall is reachable, or does CI run only
   deterministic rules (proposal question 30)?
8. **Markdown rule sources.** `AGENTS.md` read as is; add `REVIEW.md` and `.pacenotes/rules/*.md`; honour `CLAUDE.md`,
   Copilot and Cursor files as read-only sources behind toggles (proposal questions 20, 21)?
9. **Framework recognisers in the first release.** Which of ASP.NET routes and DI, Spring routes and beans, Express,
   Django/FastAPI/Flask and Go `net/http`/gin routes (proposal §20.4)?
10. **C# default server.** csharp-ls until the Roslyn binary's terms are confirmed, and pyright or basedpyright for Python
    (proposal question 31)?
11. **Dependency graph kinds.** Internal modules, packages, files, symbols, imports and calls first; services, tables,
    topics and endpoints as recognisers grow (proposal question 26)?
12. **Name and trademark.** The trademark search for "Pacenotes" is still open (proposal question 1); it is needed before a
    public release.
13. **Model file.** Bundle potion-code-16M-v2 (32 MB) in the installer or download it on the first click (ADR 0001 leaves
    this to packaging time)?
