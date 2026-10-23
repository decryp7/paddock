Status: proposal, open questions pending. Direction decided 2026-10-23: Pacenotes is its own app (not inside Debrief) for everything code-related; its functions (Understand, Review, Diagrams, Requirements, Tests & coverage, Debrief bridge, language packs) are plugins on a core of code index, headless service, MCP server and UI shell. Sighter owns UI test execution; Pacenotes owns requirements and traceability.

# Proposal: a local codebase tool, rev 2 (working name "Pacenotes")

Status: revision 2 of the proposal for the user, 2026-10-23. Nothing is built or queued. Revision 1 is kept as `proposal-codebase-tool-rev1.md`.
Research by web search, GitHub/crates.io/PyPI/npm metadata and raw LICENSE files on 2026-10-23. Items marked (unverified) were not confirmed
from a primary source. Licence statements are engineering reading, not legal advice.

What changed since rev 1: the user's objectives are now (1) understand a codebase, (2) code review, (3) generate draw.io diagrams, (4) MCP for
Pitwall, (5) let Debrief query and check code; plus requirements traceability (Word/Excel/PPT/ReqIF), tests and coverage (with Sighter for UI
tests), language packs, and round-trip-safe hand-editable diagrams. The product is no longer "a code search MCP server"; it is a **read-only
code intelligence workbench**. Sections 1-3 reframe; 4-5 are the survey and the licence findings; 6 is the full catalogue; 7-13 the designs; 14-15
architecture and integrations; 16-19 safety, risks, questions and size.

## 1. Product pitch and the one hard rule

Pacenotes indexes code on your machine into a symbol, reference, call, import, history, test, coverage and requirement graph, and then:
**explains** it (repo map, tours, architecture summary, hotspots), **answers** exact questions (who calls this, what reads this key), **reviews**
changes (deterministic rule packs plus a Pitwall LLM pass, grounded in the index), **draws** it (draw.io diagrams in the Paddock style that you can
edit by hand and that survive regeneration), **checks** Debrief's logs and traces against the code at the right revision, **traces** requirements
to code and tests, **measures** test coverage by importing reports, and **drafts** tests. It is a Tauri 2 + Angular desktop app with a Rust
engine, and at the same time a **read-only MCP server** for Pitwall.

**Hard rule (kept from rev 1, now covers every new feature): Pacenotes never executes repository code.** No build, no tests, no coverage runs, no
language servers by default, no rule that runs a script from the repo, no proc macros. Anything that needs execution (running a generated test,
producing a coverage report, building a SCIP index) is done by the user or by **Pitcrew on the user's runner**, and the results come back as
**imported files** (coverage, JUnit/TRX, SCIP). Pacenotes writes only: its own index/app data, files the user explicitly exports, diagram files in
`docs/diagrams` after a previewed diff, and (optionally, with consent) Gitea review comments through the user's token.

## 2. Name (re-evaluated for the broader scope)

Collision checks 2026-10-23: GitHub repository-name search, crates.io, npm registry, plus web search. Not a legal/trademark search.

| Candidate | Meaning | Fit for understand + review + diagrams + MCP + check + requirements | Collision evidence | Verdict |
|---|---|---|---|---|
| **Pacenotes** | Co-driver reads the road ahead from notes recorded on the recce | Good: notes about the terrain (understand, diagrams, tours), read aloud to the driver/model (MCP), "caution" calls (review), "does the road match the notes?" (check). Weakest for requirements | crates.io and npm free. 30 GitHub repos by name, best 3 stars (a TypeScript one, rally tools). Consumer rally apps ("Pacenote", "Pacenotes Pro", RallyEasy). `Ashon/pacenote` is a macOS app ("Pace") that sandboxes Claude Code/Codex sign-in: a different product, same developer-tool space, low risk | **Recommended** |
| **Roadbook** | The rally navigation document handed to crews: route, notes, distances, diagrams | Very good for the document side (diagrams, requirements, coverage reports, tours, architecture docs), less for "review" | crates.io free, npm free. 454 repos by name, top ones are travel-itinerary scripts (30 stars), no developer tool | **Runner-up** (replaces Gridwalk) |
| Gridwalk | Walk the starting grid, inspect every car | Fine for "walk the repo", weak for review/diagrams/MCP | crates/npm free; 95 repos, a creative-coding `gridwalk` org (26 stars). Meaning is thin | Dropped to third |
| Parc Ferme | Cars frozen and inspected after the race: a read-only snapshot at a revision | Good for "check at version X" and review, poor for understand/diagrams | 18 repos, nothing notable; crates/npm free (as `parcferme`). Accent, two words, awkward in CLI/crate names | Not recommended |
| Recce | Reconnaissance drive that produces the pacenotes | Excellent meaning ("understand before you race") | **Collision**: DataRecce/recce, 480 stars, TypeScript/Python, a dbt data-validation tool for PR review (same "review" space); a `recce` crate and npm package exist; also a Rust web-recon crate | Avoid |
| Scrutineer | Inspector who checks cars against the rules | Perfect for review/requirements | **Collision**: alpha-omega-security/scrutineer (256 stars, Go, local security-scan tool with agent backends), npm package exists | Avoid |
| Stewards | Officials who judge incidents | Review only | Generic; 781 repos by name, many unrelated; nothing that owns it | Weak identity |
| Codriver | Co-driver in your tool chain | Good for MCP/assistant framing | 96 repos; `ViktorTrn/codriver-mcp` (16 stars) and `RickyDane/CoDriver` (152 stars); sounds like Copilot | Avoid |
| Homologate | Certify a car against regulations | Excellent for requirements, narrow otherwise | Free on crates/npm, 3 repos; clumsy word | Only as the name of the requirements module |

Also considered and rejected: Telltale (a `telltale` crate and npm package exist), Marshal (crate and npm exist, generic), Chicane, Slipstream
(generic, rev 1). **Recommendation: Pacenotes** (crates `pacenotes-*`, CLI `pacenotes`, MCP server `pacenotes`); sub-modules keep plain names
(Review, Diagrams, Check, Requirements, Coverage). **Runner-up: Roadbook.** Do a trademark search before the name is public. Avoid Paddock, Pitwall,
Pitcrew, Spotter, Debrief, Dyno, Sighter.

## 3. What works for LLM grounding (research summary, from rev 1)

- Agentic lexical search beats a one-shot vector index for coding agents (secondhand reports on Claude Code; Cursor still embeds). Consistent with the
  user's no-vector-search decision: structure + exact search + an LLM that iterates. This also governs traceability (section 11): no vector DB.
- Repo map = tree-sitter definitions + reference graph + ranking cut to a token budget (Aider, default 1k tokens): ideal for small local models.
- Symbol-level tools (find definition, references) save most tokens (Serena). Serena's licence, checked in this run: **SolidLSP MIT, the Serena
  application GPL-3.0-or-later** (rev 1 said MIT; corrected). Reference design only; it is Python.
- Precise graphs exist as formats: SCIP (Apache-2.0), Kythe (heavy). GitHub stack-graphs was archived 2025-09-09: do not depend on it.
- Git history is cheap, high-value context. Greptile (cloud) sells a code graph for review; Sourcegraph MCP is Enterprise only; Bloop archived Jan 2025.
- Community MCP code servers exist (filesystem, git, tree-sitter, language-server wrappers); none is a native, desktop-managed, budget-aware,
  history-aware, review/diagram/requirements-capable server.
- LLM traceability research: recall of roughly 60-80% with lower precision, authors say not sufficient without human oversight; small LLMs work well
  as a **filter over candidate pairs** (keeps recall, shrinks the space). This directly shapes section 11: candidates from lexical/structural
  evidence, LLM as judge, human confirms.

MCP state (verified 2026-10-23, from rev 1): spec revision 2026-07-28 is **stateless** (no initialize handshake; `server/discover`; per-request
`_meta`), Streamable HTTP + stdio, Roots/Sampling/Logging deprecated. Rust SDK `rmcp` (Apache-2.0; open: does 3.x implement the stateless
revision). Consequence: every tool takes explicit `workspace` and `rev`, no connection state.

## 4. Survey

### 4.1 Code review tools (what to take, what to avoid)

| Tool | Licence / model (checked) | Takeaway for Pacenotes |
|---|---|---|
| CodeRabbit | Commercial SaaS; self-hosted only Enterprise (docs: 500+ seats; an AWS Marketplace listing shows $15,000/month) | Walkthrough summary, per-file comments, chat on a PR, learnings from feedback. Not usable locally |
| Qodo PR-Agent | Was AGPL-3.0 after the Apache-2.0 launch; Qodo announced the return to Apache-2.0 and community stewardship at `The-PR-Agent/pr-agent` (GitHub API now reports MIT for that repo; secondary listings disagree). **AGPL flag: do not copy code from the AGPL-era history.** Qodo Cover is AGPL-3.0 | Commands (`/review`, `/describe`, `/improve`), token-budgeted PR compression, JSON-schema'd findings. Ideas only; we write our own |
| Sourcery | `sourcery-ai/sourcery` repo MIT (the product is a commercial service) | Refactoring-suggestion style, rules as code |
| Graphite reviewer, GitHub Copilot review | Commercial SaaS (unverified detail) | Stack/PR summaries, suggestion blocks one click to apply. Cloud: out |
| SonarQube / SonarLint (SonarQube for IDE) | Community Build and IDE: LGPL-3.0, but since Dec 2024 the **bundled analyzers are under the Sonar Source Available License v1.0** (bars building similar products, bars using AI to interact beyond what is included) | **Do not embed or copy Sonar rules/analyzers.** Take the taxonomy (bug, vulnerability, smell, hotspot), quality gate on new code, "clean as you code" |
| Semgrep | Engine **LGPL-2.1**; **rules (`semgrep-rules`) under Semgrep Rules License v1.0: internal business use only, no redistribution or competing service**; third-party registry rules inherit their own licences (Trail of Bits: AGPL-3.0) | Do not ship or copy Semgrep rules. Pattern-with-metavariables rule syntax is a good model. The engine is OCaml: not bundled |
| **ast-grep** | MIT; Rust, tree-sitter based | Closest to our design: structural patterns with metavariables and YAML rules. Use as **design and possibly as a library** (ast-grep-core is a Rust crate; check its tree-sitter version pinning) |
| reviewdog | MIT (Go) | Converts any linter output to review comments via a common diff-position format (rdjson); informs our import of external linters |
| Danger (danger-js) | MIT | "Rules over a PR" as code; informs the cross-check rules (changelog updated? tests touched?) but we do it as TOML |
| clippy / eslint | MIT/Apache-2.0 / MIT | Run by the user's CI (Pitcrew), **not by us** (they compile/execute plugins and build scripts). We import their SARIF/JSON output |
| Qodana | Community edition free but limited; Ultimate/Ultimate Plus paid per active contributor (90 days), annual licence | JetBrains inspections as SARIF; import only |
| CodeScene | Commercial | Behavioural analysis: hotspots (change frequency x low Code Health), change coupling, knowledge silos / bus factor, Code Health trend on PRs. Worth copying as features (section 6) |

### 4.2 Diagram and layout tools

| Tool | Licence (checked) | Note / verdict |
|---|---|---|
| **draw.io** (`jgraph/drawio`) | Apache-2.0 (the desktop wrapper `drawio-desktop` is GPL-3.0: we only **launch** it, never link it) | Our target format. Plain `<mxGraphModel>` accepted; `<mxfile>` wrapper; diagram body may be raw-deflate+Base64 compressed (draw.io's default) or uncompressed; draw.io docs recommend generating uncompressed |
| Structurizr (CLI/DSL) | `structurizr/cli` Apache-2.0 (Java); layout stored in a separate JSON and merged into the re-parsed model by matching element ids; ids come from creation order, so reordering loses layout | The closest published precedent for "regenerate without losing hand layout". We use content-derived stable ids instead of order-derived |
| PlantUML | GitHub reports **LGPL-3.0** (multi-licence offered; the user's note said GPL). Java, needs Graphviz for most layouts; draw.io is removing PlantUML support (end 2025 online, 2028 Confluence/Jira) | Unusable for us regardless: JVM + Graphviz runtime, no hand-editable round trip |
| Mermaid | MIT (JS) | Not our format (no stable layout; imports into draw.io lose it, unverified); useful only as a text export for READMEs |
| Graphviz | EPL-2.0 (repo LICENSE on GitLab; older releases CPL) | Native binary; permissive enough to spawn, but a runtime dependency we do not need and its edge routing does not follow the Paddock rules. Not bundled |
| **ELK / elkjs** | **EPL-2.0** (elkjs LICENSE.md; ELK Maven POMs also offer EPL-2.0 OR GPL-3.0-or-later) | Weak, file-level copyleft: using it unmodified is allowed with notices and source availability for the EPL files, modifications to those files stay EPL. Usable in principle, but it is a JS/Java artefact: running it needs a JS engine inside the Rust engine. Not recommended for the core; acceptable only as an optional UI-side helper |
| dagre | MIT (`dagrejs/dagre`, JS) | The algorithm (Sugiyama layered) is well known and simple. Rust ports exist: `dagre` 0.1.1 (Apache-2.0, 7.8k downloads, immature), `dugong`, `dagre-rs` (licences not checked) |
| Rust layered layouts | `rust-sugiyama` 0.4.0 **MIT** (52k downloads); `layout-rs` 0.1.3 **MIT** (827k downloads, parses DOT and renders SVG); `bra0-layout` 0.2.2 MIT OR Apache-2.0 (163 downloads) | Candidates for call/dependency graphs |
| pyreverse (pylint), code2flow, dependency-cruiser, Doxygen graphs | pylint GPL-2.0 (unverified), code2flow MIT (unverified), dependency-cruiser MIT (unverified); **Doxygen GPL-2.0** | Feature models only: UML class/package diagrams from code, call graphs by name, dependency rules/validation, include/call/caller graphs. Doxygen's use of Graphviz/dot is the precedent for large-graph pruning (node limits, depth) |

**Layout recommendation.** Most Paddock diagrams are small and structured, so do **not** adopt a general engine first. (1) C4, deployment, class, state:
**template layouts** in Rust: role-based rows/columns on the 8 px grid, deterministic, the way the Paddock templates are built. (2) Call graphs, module
and dependency graphs: a small **Sugiyama layered layout** (rank by longest path, crossing reduction by barycentre, coordinate assignment, then
snap to the grid) implemented in `pacenotes-diagram` using `rust-sugiyama` (MIT) as a starting point or a clean-room port of the dagre algorithm
(MIT) if its output does not meet the style (verify when prototyping). (3) **Edge routing is ours**: orthogonal with pinned ports per STYLE.md, which
no off-the-shelf engine gives. ELK stays the escape hatch if a benchmark shows our layered layout inadequate; its EPL-2.0 would then need an ADR.
The layout engine places only new cells or everything on request (section 9.3).

### 4.3 Understand-tools and wiki generators (features worth taking)

| Product | Features to take | Notes |
|---|---|---|
| Sourcegraph | Code search (regex/structural), code navigation, batch changes, code insights | Cody Free/Pro ended July 2025; MCP is Enterprise |
| Greptile | Code graph for review, learns from comments | Cloud |
| SciTools Understand | Butterfly graph (callers/callees), call/inheritance/include trees, metrics, "butterfly compare" (added/removed/modified between versions), coverage-coloured graphs | Commercial; our call graph + diagram diff mirror these |
| Doxygen | Source browsing, caller/callee, include and inheritance graphs | GPL-2.0, feature model only |
| Structurizr | C4 model as code, views, themes | Section 4.2 |
| Swimm | Docs coupled to code: live snippets, auto-sync, CI check fails when code change may stale docs | Direct model for "stale diagram/doc" checks and tours |
| DeepWiki / Devin Wiki; DeepWiki-Open (AsyncFuncAI), OpenDeepWiki | Generated wiki per repo with architecture diagrams (Mermaid) and chat | Self-hosted clones use embeddings/RAG; ours is index-first, local, MIT-compatible |
| Mintlify / Docusaurus autogen | Docs site generation | Out of scope (we export Markdown, not host sites) |
| JetBrains Qodana, Code With Me insights | Inspections as quality gate; baseline; ownership/insights | Qodana is import-only; no live collaboration |
| Aider repo map | Ranked outline within token budget | v1 |
| Serena | Symbol tools over LSP | Our index replaces LSP by default |
| CodeScene | Hotspots, change coupling, knowledge loss, Code Health | v1 hotspots, Later coupling/knowledge |

### 4.4 Requirements and traceability tools

| Tool | Licence | Takeaway |
|---|---|---|
| Jama Connect, Polarion, codebeamer, IBM DOORS | Commercial | Items, links, suspect links (flagged when an upstream item changes), coverage/impact analysis, baselines, ReqIF exchange |
| **Doorstop** | **LGPL-3.0** (PyPI classifiers) | Requirements as YAML in git; links with a content hash so changes show as "suspect". Idea reused: store a hash of the linked requirement text in each confirmed link to detect drift |
| **StrictDoc** | **Apache-2.0** (repo LICENSE) | Requirements + source-code traceability (forward and reverse), exports HTML, RST, ReqIF, PDF, Excel. Closest open model; pattern for in-code references |
| OpenFastTrack | licence not confirmed in this run (GitHub API gave none; I recall GPL-3.0, unverified) | Tagging requirements in code and docs, "covered/uncovered" reports. Idea only |
| sphinx-needs | MIT (GitHub API) | Typed needs with links, filters, traceability tables in docs |
| ReqIF | OMG standard (ReqIF 1.2, 2016), XML; Python `reqif` library exists; a `.reqifz` zip variant; the standard leaves open how to distinguish requirements from headings, so mapping packs are needed | Parse with quick-xml; export later |
| LLM traceability research | TraceLLM (recall ~60-70%), KIT RAG work, small-LLM candidate filtering (arXiv 2602.01253 and others) | Human in the loop, candidates then judge |

### 4.5 Test and coverage tools

| Tool | Licence | Takeaway |
|---|---|---|
| Codecov, Coveralls | SaaS (the Codecov uploader is Apache-2.0) | Patch coverage on PRs, trends, flags per component; we offer local equivalents |
| SonarQube coverage | Import of JaCoCo/Cobertura/LCOV/OpenCover into "coverage on new code" | Quality gate on new code concept |
| JaCoCo | EPL-2.0 (from memory; LICENSE not retrievable) | Produces XML we import; never run by us |
| coverlet / OpenCover / ReportGenerator | coverlet MIT (checked); OpenCover MIT-compatible text (checked); ReportGenerator Apache-2.0 (unverified) | Cobertura/OpenCover XML imports |
| cargo-llvm-cov | Apache-2.0 OR MIT (crates.io) | Emits LCOV / llvm-cov JSON that we import |
| istanbul/nyc, c8 | istanbul BSD-3 (unverified); c8 **ISC** (npm) | istanbul JSON, LCOV |
| gcovr | BSD-3 (unverified) | Cobertura/LCOV/JSON for C/C++ |
| Diffblue Cover | Commercial (unverified) | Java unit test generation by RL; reference only |
| Qodo Cover / Cover-Agent | **AGPL-3.0** (repo LICENSE) | Loop: generate, run, keep only tests that build, pass and raise coverage. **Do not copy**; we reimplement the loop with the execution delegated to the user's CI |
| EvoSuite, Pynguin | LGPL-3.0 each (unverified; I recall) | Search-based generation; not usable (JVM / Python), ideas only |
| Meta TestGen-LLM (FSE 2024) | Paper | Filters: builds, passes repeatedly (flake check), raises coverage; 75% built, 57% passed reliably, 25% raised coverage; 73% of recommendations accepted by engineers |

### 4.6 Language pack ecosystems

- **Zed**: language extensions are Git repos with `extension.toml`; grammars declared as `[grammars.<name>]` with repository and `rev` (a commit SHA);
  each language has a folder with `config.toml` plus query files (`highlights.scm`, ...); grammars are compiled to **WebAssembly** (wasm32-wasip2,
  wasi-sdk); extension logic is also Wasm via WIT. Closest precedent to our packs.
- **Helix**: `languages.toml` with `[[language]]` and `[[grammar]]` entries; queries in `runtime/queries/<lang>/`; the first matching query wins
  (Neovim: last wins). Helix is MPL-2.0.
- **nvim-treesitter** (Apache-2.0): a parser registry plus per-language `highlights/locals/injections/folds` queries; no wasm by default. Neovim can
  load wasm parsers when built with the tree-sitter wasm feature (from a Neovim PR; secondary).
- **GitHub code navigation**: per-language `tags.scm` queries (`@definition.*`, `@reference.*`) in each grammar repo: the standard we reuse (also what
  Aider's repo map uses).
- **difftastic** (MIT): compiles a fixed set of tree-sitter grammars in; shows the trade-off of built-in only.
- **tree-sitter Rust crate** (MIT): a `wasm` feature using wasmtime (`WasmStore`, `load_language(name, bytes)`, `Parser::set_wasm_store`).
  `wasmtime` is Apache-2.0 WITH LLVM-exception (crates.io), a large dependency; compile-time cost is real.

## 5. Licence findings in one place

Unusable, or usable only as ideas:

| Item | Finding | Consequence |
|---|---|---|
| Semgrep rules | Semgrep Rules License v1.0: internal business use, no redistribution or competing service (engine is LGPL-2.1 and OCaml) | Never copy or bundle Semgrep rules; we write our own packs |
| SonarQube analyzers | Sonar Source Available License v1.0 (restricts similar products and AI interaction); the Community Build/IDE shell is LGPL-3.0 | Do not embed or copy |
| Qodo PR-Agent | AGPL-3.0 era; now announced back to Apache-2.0 under `The-PR-Agent` (GitHub reports MIT; unclear) | Ideas only; do not take AGPL-era code |
| Qodo Cover / Cover-Agent | AGPL-3.0 | Ideas only |
| Serena application | GPL-3.0-or-later (SolidLSP MIT) | Reference design only (it is Python anyway) |
| universal-ctags | GPL-2.0 (rev 1) | Unusable; tree-sitter tags replace it |
| Doxygen | GPL-2.0 | Do not bundle; feature model |
| PlantUML | LGPL-3.0 per GitHub (the user's note said GPL); Java + Graphviz | Unusable for other reasons (runtime, no round trip) |
| Doorstop | LGPL-3.0, Python | Idea only |
| drawio-desktop | GPL-3.0 | Launch as an external program only; never link or bundle |
| ELK / elkjs | EPL-2.0 (weak copyleft) | Usable with obligations, but a JS/Java runtime; not recommended for core |
| Graphviz | EPL-2.0 | Possible to spawn; not needed |
| CodeRabbit, Graphite, Copilot review, CodeScene, Swimm, Understand | Commercial | Features only |

Usable: tree-sitter and most grammars (MIT; checked: C#, Java, Rust, Python, TypeScript, Go, C, C++, JSON, Kotlin (fwcd), TOML, YAML, XML, SQL
(DerekStride), Markdown), ast-grep (MIT), reviewdog (MIT), Danger (MIT), calamine (MIT), quick-xml (MIT), docx-rs (MIT), lopdf (MIT), pdf-extract
(MIT), pdf_oxide (MIT OR Apache-2.0), zip (MIT), csv (Unlicense/MIT), roxmltree (MIT OR Apache-2.0), cargo-llvm-cov (Apache-2.0 OR MIT, an
external tool we do not run), the `lcov` crate (MIT/Apache-2.0), wasmtime (Apache-2.0 WITH LLVM-exception), rmcp (Apache-2.0), gix, SCIP, StrictDoc
(Apache-2.0, reference), sphinx-needs (MIT, reference). `cargo deny` must still check each transitive grammar crate; per-grammar licence is
checked at pack-add time (packs declare it, section 7).

## 6. Feature catalogue

"Seen in" lists the closest products the feature comes from. v1 means the planned first release; Later means designed but after v1.
The v1 set is large and is staged in milestones (section 19).

### 6.1 Understand

| Feature | v1 / Later | Seen in |
|---|---|---|
| Workspaces (multi-root projects), ignore rules, secret denylist | v1 | Sourcegraph, Cursor |
| Tree-sitter symbol table; confidence-labelled reference table; import graph; call graph | v1 | Understand, Aider, GitHub tags |
| Manifest-driven package graph (Cargo, npm, pyproject, pom/gradle, csproj, go.mod) | v1 | Structurizr (inputs), Understand |
| Repo map with token budget and seed (file/symbol/stack trace) | v1 | Aider |
| Architecture summary (generated from manifests + dependency graph + README/ADRs; LLM caption optional) | v1 | DeepWiki, Swimm |
| Glossary (domain terms from identifiers, docs and log text, with where used) | Later | DeepWiki |
| Onboarding tours: ordered, code-anchored steps ("start here: entry points, then ..."), generated from the repo map and edited by hand, stored as Markdown in the repo, anchors re-resolved after changes | Later (v1.1) | Swimm |
| Entry points and public API surface list (main, handlers, routes, exported items) | v1 | Understand |
| Ownership: blame-based top authors per file/symbol, CODEOWNERS, bus factor | v1 (blame, CODEOWNERS), Later (bus factor) | CodeScene, GitHub |
| Churn and hotspots: change frequency x complexity (cyclomatic from tree-sitter) | v1 | CodeScene |
| Complexity and size metrics per function/file (cyclomatic, nesting, length, params) | v1 | Understand, Sonar |
| Change coupling (files that change together), knowledge-loss map | Later | CodeScene |
| Dead-code candidates, API diff between tags, layer-violation rules | Later | dependency-cruiser, Understand |
| Cross-repo links (OpenAPI/proto), DB schema to usage | Later | Sourcegraph |
| Optional SCIP import for exact references | Later (v1.1) | Sourcegraph/SCIP |

### 6.2 Ask & navigate

| Feature | v1 / Later | Seen in |
|---|---|---|
| Live regex/text search (ripgrep libs) and identifier/symbol search (FTS5, fuzzy) | v1 | Sourcegraph, ripgrep |
| Go to definition, references, call hierarchy, outline, dependency view, all with confidence badges | v1 | IDEs, Understand |
| Structural search (ast-grep style patterns with metavariables) | v1 (as rule-pack engine), UI box Later | ast-grep, Sourcegraph |
| Stack-trace explainer (Java, .NET, Python, Rust, Node, C++) | v1 | Debrief, Sentry |
| Ask box via Pitwall with visible tool calls and citations | v1 (needs Pitwall) | Cursor, DeepWiki chat |
| Open in editor by file:line; code viewer read-only | v1 | all |
| Embeddings / concept search | Later, not queued (user decision) | Cursor |

### 6.3 Review

| Feature | v1 / Later | Seen in |
|---|---|---|
| Review a working-tree diff, branch, commit range or Gitea PR (read-only) | v1 | CodeRabbit, PR-Agent |
| Deterministic rule packs (TOML, ast-grep-style) with severities | v1 | ast-grep, Semgrep (syntax only), Danger |
| **Markdown review rules**: `AGENTS.md` (root + nested, nearest applies), optional `REVIEW.md` / `review/*.md`, user-level rules; per-path `applies_to`, severity hints, addressable rule ids; findings quote the rule | v1 | AGENTS.md, Copilot `*.instructions.md`, Cursor `.mdc`, CodeRabbit `path_instructions` |
| Offer to convert a Markdown rule into a deterministic check (proposed ast-grep/regex pattern, tested on the repo, user accepts); converted rules run without Pitwall | v1 | none (unique) |
| Rule hygiene report (conflicting, vague, never-triggered rules; suggestions mined from dismissed findings) | v1 | CodeRabbit learnings (partly) |
| Starter `REVIEW.md` template and language/framework rule packs (Rust, Angular, C#, Java...) as editable Markdown | v1 | Cursor rule libraries, Copilot instruction collections |
| Grounding from the index: callers of changed functions, tests touching them, owners, related history, linked requirements | v1 | Greptile |
| LLM pass via Pitwall with token budget, severity taxonomy, per-hunk context | v1 | CodeRabbit, PR-Agent |
| Review panel, finding detail with evidence, status workflow | v1 | CodeRabbit |
| "Not an issue" feedback that writes suppressions (per rule, path, symbol, with expiry) | v1 | CodeRabbit learnings, Sonar |
| Markdown and SARIF export; CI mode via Pitcrew with exit codes | v1 | Qodana, reviewdog |
| Import external linter results (clippy, eslint, Sonar/Qodana SARIF) and merge them into one list, deduplicated | v1 (SARIF, rdjson), Later (more) | reviewdog |
| Post comments/summary to a Gitea PR with the user's token and explicit consent per post | Later (v1.1) | reviewdog, CodeRabbit |
| Diagram-stale and requirement-touched checks in a review | v1 | Swimm, Jama suspect links |
| Test-gap check on changed lines (patch coverage, from imported coverage) | v1 | Codecov |
| Quality gate on new code (rules: no new High findings, patch coverage >= N) | Later (v1.1) | SonarQube |
| Reviewer assignment from ownership | Later | GitHub |

### 6.4 Diagrams

| Feature | v1 / Later | Seen in |
|---|---|---|
| Paddock-style draw.io output (C4 context/containers/components, module and dependency graphs, class, call graph for a symbol) | v1 | Structurizr, Doxygen graphs |
| Sequence diagram from a call path or stack trace | v1 | Understand, IDE tools |
| State diagrams from detectable enums + transitions | Later (v1.1) | pyreverse (class only) |
| Deployment diagrams from manifests (Dockerfile, compose, k8s, tauri conf) | Later (v1.1) | Structurizr |
| Round-trip-safe regeneration (stable ids, pinned layout, manual additions kept, "removed in code" marks, diff preview) | v1 | Structurizr layout merge |
| Size limits (1600x1200; sequence 8 participants, 25 messages), automatic splitting with shared legend, "updated <date>" footer | v1 | Paddock STYLE.md |
| Diagram diff between two commits (added/removed/changed elements, shown as overlay in the preview) | v1 | Understand butterfly compare |
| SVG/PNG export, in-app preview from our own geometry, "Open in draw.io" | v1 | draw.io |
| Pitwall caption (alt text and one-line summary) per diagram | Later (v1.1) | DeepWiki |
| `docs/diagrams` kept current as a review check ("diagram stale") | v1 | Swimm |
| Text export (Mermaid) for READMEs | Later | Mermaid |

### 6.5 Check (Debrief)

| Feature | v1 / Later | Seen in |
|---|---|---|
| Stack trace to code at the right revision (version mapping rules) | v1 | Sentry source context |
| Log message to emitting code (template match, values) | v1 | Debrief templates |
| Config key to usages, defaults, aliases | v1 | grep tools |
| Bulk enrichment of exception groups with throw sites (no LLM) | v1 | Sentry |
| **Check**: does this log line / behaviour match the code at that version (template changed, path gone, exception cannot be thrown there) | v1 | none (unique) |
| Suspected-cause suggestions grounded in code + logs + recent commits (via Pitwall) | v1 | Sentry Autofix (cloud) |
| "Seen in production logs, never covered by tests" (log/throw sites hit in Debrief but uncovered) | v1 (needs coverage import) | none (unique) |
| Deep link both ways: Debrief to Pacenotes (`pacenotes://`) and back | v1 | Pitcrew/Dyno "Open in Debrief" |

### 6.6 Requirements and traceability

| Feature | v1 / Later | Seen in |
|---|---|---|
| Source importers: .xlsx, .docx, .csv, .md | v1 | StrictDoc, Jama import |
| .pptx, .pdf (text layer), ReqIF/.reqifz | Later (v1.1) | DOORS/Polarion, StrictDoc ReqIF |
| Declarative mapping packs (Excel columns, Word headings/numbering/styles, PPT slide/shape rules) | v1 | Jama, sphinx-needs |
| Pitwall-assisted extraction for unstructured documents, every item shown with source location, user-reviewed, low confidence flagged | v1.1 | LLM RE research |
| Traceability matrix requirement -> symbols/files -> tests -> commits/PRs | v1 | Polarion, StrictDoc |
| Links from explicit tags (`REQ-123` in comments, test names, commits, PR text; configurable) | v1 | StrictDoc, OpenFastTrack |
| Pitwall-assisted semantic candidates (lexical + structural candidates, LLM judge, user confirms) | v1.1 | TraceLLM |
| Coverage reports (no implementation, no tests), orphans, suspect-link drift (hash of requirement text stored in each link) | v1 | Doorstop, Polarion |
| Conflicts / duplicates between requirements (LLM-assisted, flagged not decided) | Later | research |
| Impact view: edit a requirement, see affected code and tests | v1 | Jama |
| Matrix export to .xlsx/.csv, round-trip into the user's own sheet columns | v1 (csv), v1.1 (xlsx write-back) | Jama, Polarion |
| Markdown/HTML report (reuse of the Debrief report engine) | v1.1 | StrictDoc |
| Review integration: a PR touching linked code flags its requirements | v1 | Polarion |

### 6.7 Tests and coverage

| Feature | v1 / Later | Seen in |
|---|---|---|
| Coverage import: LCOV, Cobertura XML, JaCoCo XML, OpenCover/coverlet, llvm-cov JSON, istanbul JSON, gcov | v1 (LCOV, Cobertura, JaCoCo, llvm-cov JSON, istanbul), Later (OpenCover, gcov) | Codecov, Sonar |
| Coverage per file, symbol, requirement, PR diff (patch coverage); trend over commits | v1 | Codecov |
| Risk hotspots = churn x complexity x low coverage | v1 | CodeScene |
| Untested error paths (throw sites and log sites uncovered) | v1 | none (unique with Debrief) |
| Test inventory via language-pack conventions (attributes/decorators/names) | v1 | IDE test explorers |
| Test results import (JUnit XML, TRX) and flaky-test detection from history/Pitcrew | v1.1 | Gradle Enterprise, Buildkite |
| Requirement -> tests matrix; requirements without tests; acceptance criteria not covered; tests without requirement | v1 | Polarion |
| Trigger a coverage run through Pitcrew (never inside Pacenotes) | v1.1 | Codecov (CI) |
| Draft unit tests via Pitwall as a diff, validated by the user's CI and reported back by import | v1.1 | TestGen-LLM, Cover-Agent |
| Draft UI test plans for Sighter (`.sighterplan`) and Sighter results back as evidence | Later (after Sighter exists) | Sighter |
| Draft regression test from a production exception | v1.1 | Debrief-driven |
| Mutation-testing report import | Later | cargo-mutants, Stryker |

### 6.8 MCP and Pitwall

| Feature | v1 / Later | Seen in |
|---|---|---|
| Stdio MCP server (`pacenotes serve --stdio`) and loopback Streamable HTTP with bearer token | v1 | MCP spec |
| Tools listed in section 13; resources and prompts | v1 | Serena, Sourcegraph MCP |
| Per-client grants (workspaces, tools), budgets, call log | v1 | Pitwall grants |
| Managed loop in Pitwall, toolsets, presets | v1 (Pitwall side) | Pitwall roadmap |
| Subscriptions and tasks extension for long jobs | Later | MCP 2026-07-28 |

### 6.9 Integrations

| Integration | v1 / Later |
|---|---|
| Pitwall (MCP client, LLM, tool router) | v1 |
| Debrief (flows in section 10; deep links) | v1 |
| Gitea: read PRs/diffs via API with a token in the OS credential store; post comments with consent | v1 read, v1.1 post |
| Pitcrew: CI mode runner (`pacenotes review --ci`), trigger coverage runs, collect artifacts (coverage, JUnit, SARIF) | v1.1 |
| Sighter: `.sighterplan` hand-off | Later |
| Spotter: "watch this dependency for new releases" from the manifest graph (one-way deep link) | Later |
| Dyno: none (no useful link found) | n/a |
| Editors: VS Code/JetBrains/`$EDITOR` open at file:line; draw.io desktop for diagrams | v1 |

### 6.10 Admin and safety

| Feature | v1 / Later |
|---|---|
| Path allow-list, canonicalisation, symlink-escape refusal | v1 |
| Secret denylist + redaction of returned snippets; requirement documents treated as confidential (strictly local) | v1 |
| Per-client grants; budgets on every call; call log without contents | v1 |
| Parser/wasm sandbox with time and memory limits, crash containment | v1 |
| Prompt-injection hygiene (tool output labelled untrusted) | v1 |
| Index health: unparsed files with reasons, language coverage, rebuild | v1 |
| Settings export/import, pack manager, ignore editor | v1 |
| Autosave of review state/suppressions/links; versioned persistent state, corrupt files moved aside | v1 |

## 7. Language packs

Decision: **core languages compiled in; every other language is a declarative language pack (data, not code).** Plugins are not used.

### 7.1 Core (compiled in, v1)

C#, Java, TypeScript/JavaScript (incl. TSX/JSX), Python, Rust, Go, C/C++, Kotlin; config/data: JSON, YAML, TOML, XML (incl. csproj/pom/appsettings),
INI, SQL; Markdown. All the checked grammars are MIT. **Order of work (quality first, rev 1 advice): C#, Java, then TypeScript, Python, Rust, C/C++, Go,
Kotlin** (Debrief's users' logs come from C#/Java/C++). If time is tight, ship C#, Java, TS, Python, Rust and Go as "complete", C/C++ and Kotlin as
"beta" (outline and search, resolver limited), and say so in the UI. First packs (v1.1): PHP, Ruby, Swift, Dart, Scala, Lua, Shell, PowerShell,
Dockerfile/HCL, Protobuf/OpenAPI, Gradle/MSBuild props.

### 7.2 What a pack is

A folder (or zip) with:

- `pack.toml`: id, name, version, pack-format version, licence of grammar and queries (required, shown at install), grammar source (repository + commit,
  like Zed's `rev`), file extensions/filenames/shebangs, comment syntax, string/escape rules, test conventions (file globs, attribute/decorator/name
  patterns), requirement-tag pattern, **log-call patterns for Debrief** (e.g. `logger.Error(` with the format-string argument position, or a
  query capture), **config-key access patterns** (`getenv`, `config.get("a.b")`, serde field names), manifest hints, and the declared capabilities
  (`symbols`, `refs`, `calls`, `throws`, `logs`, `config`, `tests`).
- `grammar.wasm`: the tree-sitter grammar compiled to WebAssembly, loaded via the tree-sitter crate's `wasm` feature and wasmtime. Wasm gives a
  sandbox without native code: no host access, memory/instruction limits and a wall-clock timeout (epoch interruption) contained per file. (Core
  grammars are compiled in natively for speed; the same queries run for both.)
- `queries/`: `tags.scm` (definitions and references, GitHub-style `@definition.*` / `@reference.*`), `locals.scm` (scopes), plus ours: `calls.scm`,
  `imports.scm`, `throws.scm`, `logs.scm`, `config.scm`, `tests.scm`. Core languages use the same files embedded.
- `tests/`: sample sources with expected output (`expected.json`: symbols, refs, calls, log sites, test cases). `pacenotes pack test <dir>` runs them,
  so authors validate without the app. This is the **pack test kit**.

### 7.3 Distribution, trust and signing

- Installed from a folder or zip; a registry is not needed in v1. A pack has a semver, a pack-format version, and a content hash listing.
- **Signing:** reuse Debrief's `.debriefplugin` scheme if it already provides detached signatures with a trust store (the user decides; I did not
  inspect that code). Because packs contain no native code and run in the Wasm sandbox, **unsigned packs are allowed with a visible "unsigned"
  warning**; signatures matter more for later third-party sharing. Built-in packs are signed with the app's key.
- Packs cannot read files, spawn processes or see the network: queries are pure data, Wasm has no imports. A hostile grammar can only waste time or
  memory, which the limits handle (robustness test: infinite-loop grammar, huge-output scanner).
- Grammar quality varies. The pack manifest records the grammar commit and the test-kit result; the index shows "pack quality: tested N samples".

### 7.4 Precision upgrades per language (optional)

SCIP import (static files from the user's own indexing run) upgrades references to "exact" for covered files; an opt-in language server (off by
default, never started on untrusted repos without a clear warning; rust-analyzer and others run build scripts) can resolve single references. Neither
is part of a pack; the pack only says which SCIP indexer name applies.

### 7.5 Risks specific to packs

Grammar maintenance (grammars lag language versions), query drift between grammar versions (pin the commit; the test kit catches it), Wasm toolchain for
authors (wasi-sdk / tree-sitter CLI `build --wasm`), compile time of wasmtime in CI, and per-language resolver effort that queries alone cannot give
(name resolution across imports stays Rust code per language family; packs get only the generic scope+import resolver).

## 8. Code review design

Modes: working-tree diff, branch vs base, commit range, Gitea PR (fetch diff and metadata through the API; the repository is cloned/read by the
user's git, we never execute hooks or fetch automatically). Read-only by default.

1. **Collect**: parse the diff into hunks; map hunks to changed/added/removed symbols using the index at both revisions; classify each change
   (signature change, body only, new file, test only, config).
2. **Ground**: for each changed symbol: callers (with confidence), tests touching it, owners (blame), related history (commits touching the symbol),
   coverage of the changed lines, linked requirements, diagrams that include it. Cap by budget; keep the evidence with each finding.
3. **Deterministic checks** (rule packs): ast-grep-style rules in TOML, with metavariables and `not`/`inside` relations, a message, severity, fix
   hint, and optional `applies_to` scopes. Packs are the family pattern (profile-like, mergeable, test kit with good/bad examples). Examples from
   Debrief's AGENTS.md as a shipped pack `agents-md-rust-ts`:
   - `no-unwrap-on-user-data`: `$X.unwrap()` / `.expect(..)` outside `#[cfg(test)]`, with an allow-list of paths (High);
   - `tauri-command-must-be-async`: `#[tauri::command] fn` without `async` (High);
   - `no-innerhtml-with-log-text`: `[innerHTML]` binding in Angular templates (High);
   - `no-github-server-url-for-public-urls`: `github.server_url` in workflow files used to build public links (Medium);
   - `no-invoke-in-components`: `invoke(` imported in Angular component files (Medium);
   - `panic-free-parsers`: indexing/`unwrap` inside parser modules (Medium);
   - `no-conventional-commit`: commit subject check (Low, commit-range mode).
   Rules can also be written as Markdown and converted (section 8.1).
   The rule engine is Pacenotes' own Rust code over tree-sitter (we may use `ast-grep-core` as a library if its licence and tree-sitter version fit
   after a spike); it never runs scripts from the repo. Semgrep/Sonar rules are not imported (licences, section 5).
4. **LLM pass via Pitwall**: per changed hunk group, prompt with diff + grounding excerpts + the deterministic findings so far (to avoid repeating
   them) and the rule "cite evidence or say nothing"; JSON-schema output (finding, severity, category, file:line, evidence refs, suggested change,
   confidence). Token budget per review (default 12k tool/context tokens, configurable) and per hunk group; hunks are prioritised by risk (changed
   public API, low coverage, hotspots, no tests). Overflow is reported ("12 files not reviewed by the LLM; budget"), never silent. Findings without
   a resolvable citation are dropped or shown as "unverified". Small local models: first pass summarises, second pass judges candidates, a
   Fuchß-style filter approach (small models filter, deterministic evidence decides).
5. **Severity taxonomy**: `blocker` (will break or is a security/data-loss defect), `high`, `medium`, `low`, `info`; categories: correctness,
   security, robustness, performance, maintainability, testing, docs/diagram stale, requirement impact, style. Rule packs set the severity; the LLM
   suggests, the user's config caps what the LLM may emit (default: never `blocker`).
6. **Feedback**: "Not an issue" asks for a scope (this line / this rule in this file / this rule in this path / this rule everywhere) and optional
   expiry, and writes a suppression to `.pacenotes/suppressions.toml` (committable) plus a local note; LLM findings similar to a suppressed one
   (same rule or same fingerprint: file path + normalised snippet hash) are dropped; "Useful" raises the weight of a category in the prompt. No
   training of models.
7. **Outputs**: review panel (grouped by file/severity, diff-inline, evidence popovers, open in editor); Markdown report; **SARIF 2.1.0** (for
   Gitea/other viewers and Qodana-style tooling); optional Gitea comment payloads that are only sent after a preview with explicit consent per
   review; CI mode `pacenotes review --base X --head Y --fail-on high --format sarif|md` with exit codes (0 clean, 1 findings at/above threshold, 2
   error, 3 budget exhausted/partial), run by Pitcrew on the user's runner against a checkout the runner already has.
8. **Review of the review**: every finding stores rule id/model/prompt version, evidence and index revision, so results are reproducible and
   comparable between runs; the LLM pass is cached per hunk fingerprint.
9. **Hard rule**: no repository code is executed; external linters' output is imported, not run.

### 8.1 Markdown review rules (first-class)

Teams already write review expectations as prose in `AGENTS.md`-style files. Pacenotes makes that Markdown the primary way to say "what to look for".

**Conventions surveyed** (what to align with):

| Convention | How it scopes rules | Takeaway |
|---|---|---|
| AGENTS.md (agents.md, cross-vendor) | Plain Markdown, no required fields; the AGENTS.md closest to the edited file takes effect, nested files for monorepos; tools differ: Codex concatenates root-to-leaf with later winning, Claude Code concatenates `CLAUDE.md` files, Copilot CLI defines no precedence | We read AGENTS.md as-is; nearest wins by default, with a setting "layered" (concatenate, nearest last) for people who write them that way |
| CLAUDE.md | Same idea, concatenated | Optional read-only source |
| `.github/copilot-instructions.md` and `.github/instructions/*.instructions.md` | Repo-wide file plus path-scoped files with `applyTo` globs; `excludeAgent: "code-review"` hides a file from review; a third-party guide says review reads only the first 4,000 characters (unverified, not in GitHub docs) | Frontmatter-with-globs is the de facto pattern; the per-file budget is a reminder to keep rules short and to budget tokens |
| Cursor `.cursor/rules/*.mdc` | Frontmatter `description`, `globs`, `alwaysApply`; activation modes always / auto-attached by glob / agent-requested / manual; malformed frontmatter fails silently (community reports) | Our frontmatter validator must fail loudly with the line |
| CodeRabbit `reviews.path_instructions` in `.coderabbit.yaml` | List of `path` glob (minimatch) + `instructions` text; only shapes guidance, does not disable other checks; org-level overrides win | Path glob + instruction text is the unit; org/user-level layering needs a clear precedence |
| Qodo / PR-Agent `extra_instructions` (`[pr_reviewer]`; newer docs `[review_agent]` guidelines) | Free text in TOML; advice: be specific, concise, bullet points | Keep rules short; per-tool sections are not needed |
| Continue rules (`.continue/rules`) | Markdown with frontmatter (name, globs, alwaysApply) (from memory; not verified in this run) | Same shape as Cursor |

**Sources, in precedence order (highest first):** (1) user-level rules in the app (personal, never committed, can disable repo rules per rule id);
(2) review-only files: `REVIEW.md` at the repo root and `review/*.md` (or `.pacenotes/review/*.md`); (3) `AGENTS.md`, root plus nested (the nearest to
the changed file applies; "layered" mode available); (4) optional read-only vendor files (`CLAUDE.md`, `.github/copilot-instructions.md`,
`.github/instructions/*.instructions.md`, `.cursor/rules/*.mdc`), off by default, one toggle per source. A rule on a more specific path or from a
higher source overrides a conflicting rule with the same id; unresolved contradictions go to the hygiene report instead of being guessed.

**Syntax (all optional, plain Markdown stays valid):**

- Frontmatter per file: `applies_to: ["app/src/**/*.ts"]`, `exclude: [...]`, `severity: should` (default), `languages: [...]`, `enabled: true`.
  Without frontmatter, a file applies to its directory subtree (the AGENTS.md rule). A heading may carry its own scope: `## Angular templates {applies_to="app/src/**/*.html"}`.
- Each **bullet or heading line under a "review" heading** becomes a rule. The whole of AGENTS.md is parsed, but only sections matching a
  configurable heading filter (default: headings containing review, security, robustness, style, testing, "never", "must") are treated as review
  rules; the user can include/exclude sections in the Rules view (a rule list is shown with source lines; nothing is silently included).
- Severity hints at the start of a bullet: `[must]` (maps to high), `[should]` (medium), `[nit]` (low), `[info]`; imperative words ("Never", "Always",
  "must") imply `[must]` when no tag is present (shown as inferred, editable).
- Rule ids: explicit `{#no-unwrap-user-data}` suffix or `(R-012)` tag, otherwise derived: `<file-slug>/<heading-slug>/<n>` plus a short content hash;
  when the text of a bullet changes slightly the id stays (the previous id is matched by similarity), so suppressions and statistics survive edits.
  Every finding cites the rule id, quotes the rule text, and links to `file:line`.

**Two kinds of checks, with no pretending:**

1. **Mechanical rules** are those where a pattern can say yes or no (no `unwrap()` on user data, `[innerHTML]`, every `#[tauri::command]` is
   `async`, no `github.server_url` for public URLs). For each rule Pacenotes **offers a conversion**: the LLM (Pitwall) or a built-in recipe library
   proposes an ast-grep/regex pattern with good and bad examples; Pacenotes runs it on the repo at the current revision (count of matches, a sample,
   false-positive guess), the user edits and accepts; the result is written as a deterministic rule in the rule pack and linked back to the Markdown
   rule id (`converted`). Converted rules run **without Pitwall**, in CI mode too, and are the same engine as section 8 item 3. The Markdown stays the
   source of intent; if the Markdown bullet changes, the converted rule is flagged "source changed, re-check".
2. **Judgement rules** ("functions should do one thing", "prefer composition") are checked by the LLM pass through Pitwall: each applicable rule is
   injected for the hunks whose paths match, the model must answer per finding with the rule id, quoted text and code evidence. **Without Pitwall only
   converted rules run, and the UI and the report say so** ("14 of 31 rules were not checked: need Pitwall").

**Rule hygiene report** (a view, and a CI-optional output): rules that conflict (same scope, opposite advice; flagged by pattern checks and an LLM
pass), vague rules (no verb/object, no scope, unobservable terms), duplicates, rules never triggered over the last N reviews/commits (candidate for
removal or evidence the code complies), noisy rules (high dismissal rate), rules with no mechanical form that could have one, rules whose referenced
paths or names no longer exist, and **suggestions mined from feedback**: "you dismissed `unwrap` findings in `tests/` five times: add `exclude` to
this rule or a suppression?" and "three accepted review comments mention X with no rule: add one?" (suggestions are drafts; the user edits the Markdown;
Pacenotes never edits the rule files on its own, it shows a diff and writes only after the user accepts).

**Templates:** a starter `REVIEW.md` (sections: Correctness, Security, Robustness, Testing, Docs and diagrams, Style, Out of scope; each with example
bullets, severity tags and a comment explaining the syntax), plus language/framework packs as editable Markdown that the user copies into the repo
(Rust, Angular, C#, Java, Python, TypeScript, SQL, Tauri; the Debrief AGENTS.md rules are the Rust/Angular/Tauri seed). Packs are plain Markdown with
frontmatter, so they work in other tools too.

**Parsing and safety:** the Markdown is parsed in the parser worker (pulldown-cmark or tree-sitter-markdown, line numbers kept), size-capped, front matter
validated with errors that name the line; rule text is **untrusted data** when injected into prompts (a rule file in a PR could try to
instruct the model; review rules are read from the **base** revision by default, and a changed rules file in the PR is itself flagged
"rules changed in this PR" and applied only after the user opts in); rule text never executes anything. MCP: `review_rules` and `explain_rule`
(section 13).

## 9. Diagram generation (draw.io, hand-editable, regenerable)

### 9.1 Format

Plain, **uncompressed** `.drawio` (`<mxfile><diagram><mxGraphModel>...`), written with the Paddock styles (`light-dark()` colours, kit styles from
`paddock.xml`) so they open and edit in draw.io desktop, the web app and the VS Code extension. draw.io's default save compresses the diagram body
(raw deflate + Base64), so the **reader must accept both**, and the writer always produces uncompressed. If the user saves in draw.io and the file
becomes compressed, Pacenotes still reads it and rewrites it uncompressed at the next regeneration (the "Compressed" file option can be turned off in
draw.io; the exact menu location is unverified). SVG and PNG exports sit alongside (`svg/`, `png/`), produced by our own renderer for the preview
and by `scripts/export-diagrams.sh` (draw.io CLI, external, with timeout) when the user wants the official export.

### 9.2 Round-trip safety (the key requirement)

Every generated cell carries:

- a stable id `pn:<kind>:<hash>` where `<hash>` derives from the code element's identity, not its order: symbol qualified name + kind + language (or
  manifest package id, or crate/package path), so renames produce a new id only when the qualified name changes, and reordering never matters
  (contrast Structurizr's order-derived ids that lose layout);
- a marker `pnGen=1` in its `style` (draw.io keeps unknown style keys) and custom attributes through an `<object>` wrapper: `pn_key` (element
  identity string), `pn_rev` (revision generated from), `pn_hash` (hash of generated label/content), `pn_geo` (generated geometry `x,y,w,h`).

Rules on regeneration (three-way, using the file's own attributes as the base, so no sidecar file is needed):

| Situation | Behaviour |
|---|---|
| Cell without the marker or without the `pn:` id (manual additions; also **duplicates** the user made of a generated cell, which keep the style but get a new id) | Never touched, never removed |
| Generated cell, geometry equals `pn_geo` | Eligible for re-layout; moved only if the user chose "re-layout" |
| Generated cell moved/resized by the user (geometry differs from `pn_geo`) | **Pinned**: position and size kept; style tweaks (colours, fonts) kept; text edits to the label kept and flagged "label edited" |
| Code element still exists, content changed | Label/members updated unless the label was edited (then shown as a conflict in the preview) |
| Code element gone | Cell marked "removed in code" (greyed via style, small note), **kept until the user confirms removal** in the preview |
| New code element | Added; placed near related cells (neighbours by edge, else by group) in free grid slots without moving existing cells; edges routed by the orthogonal router around pinned cells |
| Edge between generated cells | Regenerated routing unless the user edited waypoints (pinned like geometry) |

**Diff preview before applying**: a list and an overlay of added / changed / removed / pinned / conflicting items, with accept per item; nothing is
written until the user accepts; the previous file is kept in the app data folder for undo. The **stale-diagram check** compares the code against the
generated part only (cells with the marker): a diagram is stale when `pn_hash`/identity sets differ from the current model; manual cells are ignored.

Git-friendliness (merge conflicts in XML are the main risk): the writer emits a **canonical form**: uncompressed, one `mxCell`/`object` per line,
attributes in fixed order, cells sorted by id (generated first by kind then id, manual cells in their original order after them), numbers rounded to
the 8 px grid, no volatile fields (no timestamps inside cells; the "updated <date>" footer is one text cell). draw.io rewrites the file on save in its
own order and formatting (unverified in detail), so regeneration normalises the whole file once; a recommended `.gitattributes` entry
(`*.drawio merge=union` is NOT recommended; a normal text merge plus the canonical form is) and a `pacenotes diagram merge-driver` (optional, Later)
that merges cell-wise by id is the clean fix.

No existing tool was found that does incremental regeneration of a hand-edited **draw.io** diagram; the nearest published precedent is Structurizr's
layout-merge (match elements between old and new workspace and keep positions). Ours extends that to manual cells, pinning, removal confirmation
and a preview. This is a design bet; prototype early with real draw.io round trips (open, move, add, save in desktop and web) as acceptance tests.

### 9.3 Layout

The layout engine places only new cells, or everything when the user asks for "re-layout" (which resets pins after a confirmation). Engine choice
in section 4.2: templates for C4/deployment/class/state, Sugiyama layered (rust-sugiyama MIT or our port) for call/dependency graphs, our
orthogonal router for edges. ELK only as an ADR-gated fallback (EPL-2.0).

### 9.4 Diagram kinds (from the index)

- **C4 context/containers/components**: from workspace + crate/package structure + manifests (Cargo workspace members, package.json workspaces,
  csproj references, compose services, tauri config). Context level needs a small hand-written `pacenotes.diagram.toml` (actors, external systems);
  otherwise the generator only claims what it can see and says so in the legend ("inferred").
- **Module and dependency graphs**: import graph at package/module level; cycles highlighted; capped (section 9.5).
- **Class diagrams**: types, members, inheritance/implements, composition from field types (confidence-labelled); one diagram per module.
- **Call graph for a symbol**: callers/callees to depth N with confidence styling (dashed = name-only).
- **Sequence from a call path**: from a stack trace (Debrief) or a chosen entry symbol; participants = types/modules; messages in call order;
  async boundaries flagged; at most 8 participants and 25 messages, else split.
- **State diagrams**: enums used in `match`/`switch` with assignments to a field (state machines detectable by pattern); Later because detection is
  heuristic.
- **Deployment**: manifests (Dockerfile, compose, k8s, tauri `bundle`) per OS.

### 9.5 Keeping diagrams honest

Canvas at most 1600 x 1200 (sequence 1600 x 1800), at most 12 edges (STYLE.md), so large graphs are **split** (by package boundary/community) into
linked diagrams with a shared legend and an index page; aggregation nodes ("+37 more") with the count; confidence shown (inferred vs declared);
"updated YYYY-MM-DD" footer plus `pn_rev` visible in the footer as short hash; a **diagram diff between two commits** (generate both models, diff by
stable id, render added/removed/changed as an overlay like Understand's butterfly compare); a Pitwall-assisted **caption** (alt text and one line)
stored in the file as a note cell marked generated, never replacing human text; "diagram stale" as a review check for diagrams under
`docs/diagrams` with generated cells.

### 9.6 Workflow

"Open in draw.io" launches the installed desktop app (else the system default for `.drawio`); the user edits and saves to the same file under
`docs/diagrams`; Pacenotes watches the file and shows "edited outside, N manual cells"; optional "commit" button runs the user's own git with a
prepared message (never automatic, never push).

### 9.7 Emitter: reuse the Paddock tools or port to Rust?

Paddock's `tools/*.mjs` (lib, uml, themes, overview, apply-accent, build-kit) and `scripts/export-diagrams.sh` exist and define the look. They are
Node scripts; the app may not ship other runtimes (AGENTS.md "Languages"). Options:

1. Call the Node tools from Rust: needs Node on the user's machine; breaks the "nothing else to install" rule; headless MCP use would depend on it. No.
2. **Port to Rust (recommended)**: `pacenotes-diagram` implements the model -> mxGraph XML emitter, the theme tokens (read from a shared
   `themes.json` generated from `tools/themes.mjs` so the colours have one source) and the UML/C4 shape helpers; **the Paddock `.mjs` tools become the
   conformance oracle**: golden tests generate a fixture model with both and compare the XML after canonicalisation (this presumes the `.mjs` APIs
   are scriptable; I have read STYLE.md and the directory listing only, not the tool sources: confirm before committing).
3. Keep `export-diagrams.sh` as the external exporter (draw.io CLI) and `apply-accent.mjs` as an author-time tool.

Per-app accent: Pacenotes needs its own entry in `themes.mjs` (three role colours); pick during the design pass. A Paddock change request: expose
`themes.json` as a build artefact of the kit.

## 10. Check: Debrief queries and verifies code

Debrief stores per-profile `codeWorkspace` and a **rev rule** (log-reported app version/build id -> git tag/commit; open question 9). Debrief ->
Pitwall (managed loop, preset `debrief-code`) -> Pacenotes; Pure Rust parts need no LLM.

1. Stack trace -> code at the right revision (`explain_stack_trace(rev)`): frames to symbols, throw site, callers, recent commits (rev 1).
2. Log message -> emitting code (`find_log_site`): Debrief passes its template; result has logger, function, value sources, history ("added in v2.3").
3. Config key -> usage (`find_config_usage`).
4. Bulk enrichment of top exception groups and patterns (no LLM, cached per rev).
5. **Check** (`check_log_site`, `check_behavior`): given a log line (or template) and a revision, returns a verdict with evidence:
   - `match`: emitted at `path:line`, template identical;
   - `template_changed`: the nearest site's template differs at this rev (shows both texts and the commit that changed it);
   - `site_missing`: no site at this rev matches; the closest candidates and the commit that removed it;
   - `path_unreachable`: the site exists but no call path from any entry point (name-level; labelled with confidence);
   - `exception_not_throwable_here`: the exception type has no throw site in the stack's functions or their callees at this rev, with the throw sites
     that do exist and the handler/catch chain; uncertainty labelled for dynamic dispatch/reflection;
   - `ambiguous`: several candidates, with the discriminator needed (levels, values).
   The verdict is computed in Rust; the LLM only narrates.
6. **Suspected-cause suggestions**: Pitwall agent loop with grounded inputs (trace, surrounding log lines redacted by Debrief rules, the code, `blame`,
   changes since the previous good version, coverage of the site); the answer must cite `path:line` and say what evidence is missing; offered as
   hypotheses, never as facts.
7. "Seen in production, never covered": for log and throw sites with Debrief occurrence counts, join with imported coverage: list uncovered ones
   ordered by frequency/severity; offers a regression-test draft (section 12).
8. Deep links: Debrief opens `pacenotes://open?workspace=..&rev=..&path=..&line=..` or `pacenotes://check?...`; Pacenotes opens Debrief with
   a template query ("Open matching lines in Debrief"). Degrades with a message naming what to start when Pitwall/Pacenotes are absent.

## 11. Requirements and traceability

**Plugins here are built-in requirement-source importers behind a trait (`RequirementSource`) plus declarative TOML mapping packs, not code
plugins.** A proprietary-format code plugin only if the user needs one later (YAGNI; consistent with the suite decisions).

### 11.1 Ingest (parsers)

| Format | Parser | Licence | Notes |
|---|---|---|---|
| .xlsx | `calamine` 0.37 (read-only; sheets, ranges, merged cells) | MIT | Writing the matrix back (round-trip into the user's columns) needs a writer: patch the OOXML with `quick-xml` + `zip` for cell values (keeps formatting) rather than a full writer; Later |
| .docx | `zip` + `quick-xml` over OOXML (`word/document.xml`, `styles.xml`, `numbering.xml`, comments); `docx-rs` 0.4.22 (MIT) is mainly a writer/reader of its own model | MIT | Recommend quick-xml directly: styles, numbering levels and tables are what mapping rules need, and we control limits |
| .pptx | `zip` + `quick-xml` (`ppt/slides/slideN.xml`, notes, tables) | MIT | Later |
| .pdf | `pdf-extract` 0.12 (MIT, built on `lopdf` 0.46 MIT) or `pdf_oxide` 0.3 (MIT OR Apache-2.0) | MIT | Text layer only, layout loss; no OCR; Later; extraction is always reviewed |
| .md | existing Markdown tree-sitter or pulldown-cmark | MIT | headings/lists/tables |
| .csv | `csv` crate | Unlicense/MIT | |
| ReqIF / .reqifz | `quick-xml` + `zip`; SPEC-OBJECTS, SPEC-TYPES, SPEC-RELATIONS, hierarchy | MIT | OMG ReqIF 1.2; mapping needed to tell requirements from headings; Later (v1.1) |

All parsers run in the parser worker process (untrusted documents), with limits on archive expansion (zip bomb), XML entity expansion (no DTD),
file size and sheet size; failures mark the document "unparsed" with a reason.

### 11.2 Extract (mapping packs)

TOML mapping pack per document type: Excel: sheet, header row, columns for ID, title, text, priority, parent, status, acceptance criteria,
custom columns, ID pattern; Word: heading levels/styles/numbering patterns that start a requirement, paragraphs belonging to it, "shall" statements
(optional), tables with ID columns; PPT: slide title/shape-name/table rules; Markdown: heading/list/table rules; ReqIF: attribute-definition names.
A mapping has a live preview (parse, show items and unmatched rows) and a validator. Output item: `{id, title, text, priority?, parent?,
acceptance[], source:{doc, location (sheet!cell / paragraph path / slide+shape / page), hash}, confidence}`.

**Pitwall-assisted extraction** for unstructured documents: the LLM proposes item boundaries and fields; every item is shown **with its source
location and highlighted text**, the user accepts/edits/rejects (no silent guessing), low-confidence items (LLM unsure, no ID, ambiguous
boundaries) are flagged and sorted first; accepted items are stored with `origin=llm-reviewed`. Strictly local: Pitwall is local-only; documents
never leave the machine.

### 11.3 Cross-check

- **Link candidates** from (a) explicit tags: configurable patterns (`REQ-\d+`, `[A-Z]+-\d+`, `@req(..)`) in code comments, test names/attributes,
  commit messages and PR text (Gitea API); (b) structural/lexical candidates from the index: identifiers and strings matching requirement terms,
  files touched by commits that mention the ID, tests in the same module; (c) **Pitwall judge** (no vector DB): for each requirement with weak or no
  links, pass the requirement plus a small set of candidate symbols (retrieved by FTS5 identifier search and call-graph neighbourhood) and ask for
  a verdict with a quoted reason. Research reports modest recall for LLM-only traceability, so candidates are shown as **suggested, with evidence,
  never auto-confirmed**; confirmed links are stored (and may live in a committable `requirements/links.toml`).
- Each confirmed link stores the **hash of the requirement text and of the linked code range** at confirmation. Changes later produce **suspect
  links**: requirement changed after the code (drift), or code changed and the requirement may no longer hold (the LLM can assess "still holds?"
  with a diff; the human decides).
- **Reports**: coverage (requirements without implementation / without tests, acceptance criteria without a test), orphans (code or tests with no
  requirement, filtered to public API and non-trivial tests to avoid noise), drift (suspect links), conflicts/duplicates between requirements
  (LLM-assisted pairs within a topic, flagged), and an **impact view**: choose a requirement (or a pending edit) and see linked symbols, callers
  of those, tests, owners and open reviews.
- **Review integration**: a PR review lists requirements whose linked code changed and whether their tests changed or still pass/cover.

### 11.4 Outputs

Matrix view in the UI (requirement rows x columns: implementing symbols, tests, commits/PRs, coverage %, status); CSV export (v1) and Excel export
with round trip into the user's sheet columns where possible (v1.1: write only the columns the user designates, preserving formatting via OOXML
patching, never overwriting the source without a backup copy); Markdown/HTML report through Debrief's report engine if it can be reused (check:
if it is tied to log data, build a thin `report` layer shared via Paddock); MCP tools in section 13.

### 11.5 Confidentiality

Requirement documents are confidential: strictly local; stored in the workspace's Pacenotes data folder (encrypted at rest only if the OS provides
it; not our concern), excluded from logs and crash reports; Pitwall requests carrying requirement text are never cached by the response cache (ADR
0019 rule for tool-bearing requests; confirm for plain completions) and the call log has names and sizes only. Export is explicit.

## 12. Tests and coverage

### 12.1 Coverage by import (never run)

Importers behind a `CoverageSource` trait: LCOV (the `lcov` crate, MIT/Apache-2.0, or a small parser), Cobertura XML, JaCoCo XML, llvm-cov JSON
(`cargo llvm-cov --json`), istanbul JSON; v1.1: OpenCover/coverlet, gcov. Each import records the **commit and tool** (reports without a rev are
matched by the user choosing the rev or from CI metadata) and maps file paths to the index (path normalisation rules for CI checkouts).
Views: coverage per file, per symbol (lines of the symbol range), per requirement (union of linked symbols), per PR diff (**patch coverage**:
covered/uncovered added lines), trend over commits (import history), **risk hotspots** = churn x complexity x (1 - coverage), and
**untested error paths**: throw sites and log sites with zero coverage, joined with Debrief occurrence counts (section 10). Staleness: a report older
than the files' hashes is marked stale per file, never trusted silently.
Optionally trigger a coverage run through **Pitcrew** (a workflow dispatch on the user's runner, user-configured command in the repo's CI, results
fetched as artifacts), never locally by Pacenotes.

### 12.2 Test-case coverage

Test inventory from language packs (`tests.scm`: `[Test]`/`[Fact]`, `#[test]`, `@Test`, `def test_*`, `describe/it`), including unit, integration and
UI/e2e (Playwright/WebdriverIO file conventions, Sighter plans later). Requirement -> tests matrix (from links in 11.3), requirements with no test,
acceptance criteria not mapped to a test (each criterion can be linked separately), tests with no requirement, and flaky tests from imported JUnit/TRX
results over time and Pitcrew job history (a test with mixed outcomes at the same commit).

### 12.3 Test generation (proposals, never auto-committed)

- **Unit tests via Pitwall**: for a symbol (or an uncovered hotspot): context = signature and body, callers (usage shape), existing tests in the same
  module (style, fixtures, naming), error paths from throw sites, language-pack test conventions. Output: a **draft test file shown as a diff**; the
  user accepts to write it into the working tree (or exports a patch). **Validation requires running it, which Pacenotes never does.** The loop:
  1. user accepts the draft and commits it on a branch, or Pitcrew runs it on the branch;
  2. the user's CI runs build + tests + coverage;
  3. Pacenotes imports the results and, following TestGen-LLM's filters (builds, passes repeatedly, raises coverage), marks each draft
     `kept / failed to build / flaky / no coverage gain`, shows failures with the compiler/test output and offers "repair draft" (Pitwall) for another
     round. Meta reports 75% built, 57% passed reliably, 25% raised coverage; expect a similar attrition with local models, so the UI frames drafts as
     "suggestions", shows the expected gain, and limits batch size.
- **UI tests via Sighter**: from requirements and acceptance criteria plus the index's UI knowledge (strings, routes, dialogs, component selectors,
  labels), draft **Sighter script skeletons** (Python/TS) using `locate`, `wait_for`, `assert_screen` with element names. Unknown elements become
  "record this screen in Sighter" tasks. Reverse: Sighter runs link back to requirements via ids in the plan (traceability evidence).
  **Hand-off contract (`.sighterplan`, JSON, versioned, proposed; Sighter's side not yet designed in detail)**:
  `{ "schema": 1, "id": "...", "title": "...", "requirements": ["REQ-12"], "app": {"name": "...", "revision": "<git rev>"},
  "screens": [{"name": "Login", "status": "known|to_record", "hint": "strings/route from code"}],
  "steps": [{"id": "s1", "action": "locate|click|type|wait_for|assert_screen", "target": {"name": "Sign in button", "hint": {"text": "Sign in"}},
  "text": null, "expect": {"screen": "Dashboard"}}], "provenance": {"generator": "pacenotes", "sources": ["src/login.ts:41"]} }`.
  Sighter imports it (opens the studio at the first `to_record` screen via `sighter://record?plan=..&screen=..`), runs it, and writes a result JSON
  (`planId`, per-step status, screenshots refs, requirement ids, revision) that Pacenotes imports as test evidence next to JUnit results.
  This depends on Sighter existing; **Later**.
- **Debrief-driven**: "this exception from production has no test; here is a draft regression test" (trace + throw site + minimal reproduction
  from log values, always flagged as a guess).
- Risks: LLM test quality (assertions that restate the code, brittle mocks, tests that pass vacuously); mitigations: rubric checks in Rust (has
  assertions, calls the target, not a copy of the body), coverage-gain gate, mutation-report import (Later) as the real quality signal, human review
  of every draft. Report/tool licences: all import formats are open; no AGPL generator code is reused (Cover-Agent is AGPL; we reimplement the idea).

## 13. MCP surface

Server `pacenotes`; stdio and loopback Streamable HTTP with bearer token. Every tool takes `workspace` and optional `rev` (git rev or
`WORKTREE`) and `budget_tokens`; results have `structuredContent` plus short text, deterministic order, `truncated`, `next_cursor`, and
`path:line` citations. All tools are `readOnlyHint`; the few that write to disk are **not exposed to MCP** (see "Not exposed").

Rev 1 tools (unchanged): `list_workspaces`, `repo_map`, `file_outline`, `read_symbol`, `read_range`, `find_symbol`, `references`, `call_graph`,
`dependencies`, `search`, `explain_stack_trace`, `find_log_site`, `find_config_usage`, `find_throw_sites`, `blame`, `history`, `diff_since`,
`index_status`.

New:

| Tool | Args | Returns |
|---|---|---|
| `review_diff` | `(workspace, base, head or range or pr_url, rules?: pack ids, llm?: bool, budget_tokens?)` | review id, findings (rule/LLM, severity, category, location, evidence, suggestion), skipped parts with reasons. Runs deterministic rules always; LLM pass only when `llm=true` and Pitwall available. Does not post anywhere |
| `review_status` | `(review_id)` | progress, counts by severity, budget used, partial flag |
| `review_findings` | `(review_id, min_severity?, path?, cursor?)` | paged findings |
| `review_rules` | `(workspace, path?: file to resolve rules for, include?: mechanical\|llm\|all, rev?)` | the rules that apply to a path (source file + line, id, severity hint, kind: converted/mechanical/llm, last triggered, hygiene flags), in precedence order |
| `explain_rule` | `(workspace, rule_id)` | rule text and source location, why it applies to which paths, its converted pattern if any, findings and dismissals history, conflicts with other rules |
| `generate_diagram` | `(workspace, kind: c4_context\|c4_containers\|c4_components\|modules\|dependencies\|class\|deployment\|state, scope?, rev?, format: summary\|drawio_xml)` | model summary + (optionally) canonical draw.io XML as text; does **not** write files |
| `diagram_for_symbol` | `(workspace, symbol, kind: call_graph\|sequence\|class, depth?, rev?)` | same, for one symbol or call path |
| `diagram_status` | `(workspace, path?)` | for each `docs/diagrams` file: stale or current, added/removed elements vs the code |
| `check_log_site` | `(workspace, message or template, rev?, level?)` | verdict (`match`, `template_changed`, `site_missing`, `path_unreachable`, `ambiguous`) with evidence |
| `check_exception` | `(workspace, exception_type, stack?: string, rev?)` | whether it can be thrown from those frames, throw/handler chain, confidence |
| `list_requirements` | `(workspace, query?, status?, link_state?: linked\|unlinked\|suspect)` | items with id, title, source location, link counts |
| `trace_requirement` | `(workspace, requirement_id)` | linked symbols, tests, commits/PRs, coverage, suspect flags with evidence |
| `requirements_for_symbol` | `(workspace, symbol or path:line)` | requirements linked (confirmed and suggested, marked) |
| `coverage_report` | `(workspace, scope: requirements\|files\|symbols, rev?)` | uncovered requirements (no code, no tests), orphans, drift |
| `check_requirement` | `(workspace, requirement_id, rev?)` | LLM-free evidence bundle (linked code excerpts, tests, last changes) for the caller to judge; with `judge=true` Pitwall assesses "implemented? / still holds?" |
| `coverage_for` | `(workspace, target: path\|symbol\|requirement, rev?)` | line/branch coverage from the latest import, staleness |
| `patch_coverage` | `(workspace, base, head)` | uncovered added lines, % |
| `untested_hotspots` | `(workspace, limit?, include_log_sites?: bool)` | ranked churn x complexity x low coverage, throw/log sites uncovered |
| `tests_for_requirement` | `(workspace, requirement_id)` | tests linked, acceptance criteria without tests |
| `draft_unit_test` | `(workspace, symbol, style_from?: path)` | context bundle (signature, callers, test style, error paths) and, with Pitwall, a draft as unified diff text; never written |
| `draft_ui_test_plan` | `(workspace, requirement_ids[])` | a `.sighterplan` JSON (not written) |
| `hotspots` | `(workspace, since?, limit?)` | churn x complexity ranking |
| `owners` | `(workspace, path or symbol)` | blame-based owners, CODEOWNERS |
| `architecture_summary` | `(workspace, level?)` | text summary from manifests + dependency graph (+ ADR index) |

**Not exposed over MCP** (UI/CLI only, behind user clicks): writing diagram files, posting review comments, accepting drafted tests, importing
documents, writing suppressions. Resources add `pacenotes://{workspace}/requirements`, `.../coverage`, `.../diagrams/{name}`, `.../review/{id}`;
prompts add `review_change`, `check_log_line`, `trace_requirement`, `draft_test`. Naming stays short; ~40 tools is a lot for small models, so
Pitwall toolsets expose subsets (`code`, `review`, `diagrams`, `requirements`, `tests`), and one-shot tools (`repo_map`, `explain_stack_trace`,
`check_log_site`, `review_diff`) do the heavy lifting.

## 14. Architecture

Crates (pure core, thin edges, ports and adapters; names are placeholders for the final product name):

- `pacenotes-core`: pure types (SymbolId, Confidence, Location, Rev), query semantics, ranking, repo-map budgeting, stack-trace parsers, template
  matchers, resolver heuristics, budget accounting, **severity/finding types**, **requirement/link/coverage domain types** (no I/O).
- `pacenotes-parse`: tree-sitter (native core grammars; wasm packs through the `wasm` feature + wasmtime), query packs, **parser worker binary**
  (process isolation, time/memory limits, postcard IPC); document parsers for xlsx/docx/pptx/pdf/ReqIF also run here.
- `pacenotes-packs`: pack format, loader, validator, test kit runner, signature check.
- `pacenotes-index`: SQLite store (WAL, schema versions), incremental indexer, watcher, ignore, secret denylist.
- `pacenotes-git`: gix wrapper behind `History` (blame, log, diff, read-at-rev).
- `pacenotes-search`: live text search + FTS5 identifiers.
- `pacenotes-rules`: rule-pack engine (ast-grep-style matching over tree-sitter), suppressions, SARIF writer.
- `pacenotes-review`: review orchestration (collect, ground, deterministic, LLM through a `Llm` port, budgets, outputs); `Llm` port implemented by a
  Pitwall HTTP client in the app, by a fake in tests.
- `pacenotes-diagram`: model -> layout (templates, layered, router) -> canonical draw.io XML; reader for compressed/uncompressed; round-trip merge;
  diagram diff; SVG preview renderer; tests with golden files and the Paddock oracle.
- `pacenotes-req`: `RequirementSource` importers (behind ports), mapping packs, link store, traceability reports, matrix export (csv/xlsx patch).
- `pacenotes-cover`: `CoverageSource` importers, coverage model, patch coverage, hotspots, test inventory, results import (JUnit/TRX).
- `pacenotes-check`: log-site/exception/config checks and verdicts (pure, over core + index ports).
- `pacenotes-mcp`: MCP server, tool schemas, grants, budgets, call log (depends on ports only).
- `pacenotes-scip` (optional), `pacenotes-lsp` (optional, Later), `pacenotes-gitea` (REST client for PRs/comments; a port `Forge` so other forges can be
  added), `pacenotes-cli`, `app/src-tauri`, `app/src`.
- Shared Paddock crates and `@decryp7/paddock-ui`. Debrief report engine: check reuse; else a small shared `paddock-report` crate.

Store: SQLite only (as rev 1) with new tables: `findings`, `suppressions`, `reviews`, `requirements`, `req_docs`, `req_links`
(requirement, target kind, target id, state: suggested/confirmed/rejected/suspect, evidence json, req_hash, code_hash), `coverage_runs`,
`coverage_lines` (run, file, line ranges compressed), `tests` (inventory), `test_results`, `diagram_files`. Persistent user decisions
(links, suppressions, packs config) are also written to committable TOML where sensible, so they survive an index rebuild and live in git.

Process model: the app hosts the engine; the MCP server runs in the app process group or as `pacenotes serve --stdio` sharing the SQLite file (WAL,
single writer lock); parser workers and document parsers are child processes; draw.io and the editor are external programs launched on click (with
validated paths). No repository code is executed anywhere; the only subprocesses are our own workers, git (via gix, in-process), the optional draw.io
CLI export (with timeout) and the user's editor.

## 15. Pitwall integration (unchanged principles, new toolsets)

Roles unchanged: Pitwall is the MCP client and router; models never talk to MCP; Debrief talks only to Pitwall. Toolsets: `code`, `review`,
`diagrams`, `requirements`, `tests`; managed loop for Debrief and for the review's LLM pass (which may be driven by Pacenotes itself calling
Pitwall's chat endpoint with a fixed prompt rather than an open loop: fewer failure modes for small models). Grants per app, workspace and toolset;
budgets (iterations, tool calls, tool-result tokens); only read-only tools auto-run; tool results labelled untrusted; tool-bearing requests not
cached. Local-only: requirement documents and code never leave the machine. This tool is the first real consumer of Pitwall's MCP-client work
(suggest the ADR "MCP client and tool router"); Pitwall also needs a structured-output (JSON schema) path for reviews and extraction.

## 16. Safety and privacy (additions to rev 1)

- Never execute repo code (hard rule); generated tests, coverage and SCIP indexes are produced elsewhere and imported.
- Writes are explicit and previewed: diagram files (diff preview), accepted test drafts, suppressions, link stores, exported matrices; Gitea posting
  needs the user's token (OS credential store), a preview and consent per post; tokens never logged; rate limits and timeouts on the Forge client.
- Document parsers hardened (zip-bomb limits, no XML DTD/external entities, size caps), run in the worker.
- Wasm packs sandboxed with epoch timeouts and memory caps; pack licences shown at install.
- Prompt injection: code, comments, requirement text and PR descriptions are untrusted data in every prompt; LLM findings need citations; the
  review cannot call write tools.
- Requirement documents and coverage data stay local; logs contain no contents.
- Every robustness behaviour has a trigger test: corrupt docx/xlsx/zip bomb, hostile grammar, infinite-loop query, huge diff, malformed SARIF/LCOV,
  draw.io file edited and compressed by the user, duplicate cell ids, clipboard-duplicated generated cells, Gitea 401/429/timeout, symlink escape,
  poisoned locks, worker crash, prompt-injection fixtures in code and requirement text.

## 17. Risks

- **Approximate references** (rev 1): confidence labels everywhere; SCIP upgrade; honest "unresolved". Checks (section 10) inherit this: verdicts
  carry confidence, and `exception_not_throwable_here` must never claim certainty under dynamic dispatch/reflection.
- **Scope**: the catalogue is now several products. Mitigation: milestones (section 19), shared infrastructure (index, evidence, previewed writes),
  and a strict v1 cut. Scope creep towards an IDE/CI system is refused: no editing, no running.
- **Round-trip diagram edits**: draw.io rewrites files; ids may change on paste; users edit labels; merge conflicts in XML. Mitigations in 9.2
  (stable content-derived ids, marker + id-prefix pair, `pn_geo` base, canonical one-cell-per-line writer, preview, confirmation before removal); the
  largest remaining risk is draw.io behaviours we have not observed (needs an early prototype with real round trips on desktop, web and VS Code).
- **Layout quality** vs the strict Paddock rules (orthogonal pinned ports, at most one crossing): template layouts cover most kinds; call graphs may
  need manual fixing; the 12-edge split rule makes big graphs several diagrams.
- **LLM quality locally**: review noise, wrong traceability judgements, weak tests, small models mis-calling tools. Mitigations: deterministic first,
  evidence-required findings, candidate-then-judge, human confirmation of links and drafts, budgets, caps, feedback suppressions.
- **Document formats**: real-world Word/Excel requirement documents are messy (merged cells, tracked changes, numbering by style, tables inside
  tables, images), PDF text extraction loses structure, PowerPoint has no semantics; ReqIF dialects vary by tool. Mitigation: mapping packs with live
  preview, nothing auto-accepted, v1 limited to xlsx/docx/csv/md.
- **Confidentiality of requirement documents**: strictly local, no cloud, no telemetry, not cached, not logged; Pitwall local-only rule holds.
- **Tests and coverage**: coverage tools differ in semantics (line vs statement vs branch; path normalisation; reports from other commits); stale or
  mismatched reports mislead. Mitigation: record rev/tool, staleness marks, path-mapping UI, never blend runs silently. Drafted tests may be vacuous
  (rubric + coverage-gain gate + user review).
- **Grammar quality and query maintenance** for packs and 8 core languages; wasm toolchain friction for authors; wasmtime build size/time.
- **Licence drift**: Semgrep/Sonar/Qodo terms changed within the last two years; keep a licence table in the repo and run `cargo deny` for every
  grammar/crate; re-check on adding dependencies.
- **MCP spec churn** (rev 1): rmcp 3.x stateless support unverified; thin protocol layer behind a trait.
- **Name**: only a web-search sanity check; trademark search pending.
- Index size/time, symbol history across renames approximate (rev 1).

## 18. Open questions for the user

1. **Name**: Pacenotes (recommended) or Roadbook (runner-up)? Trademark search before public.
2. Is the broadened scope right for one app, or should requirements/traceability and tests/coverage ship as a second step (v1.1) behind the same
   engine? (I recommend staged milestones, one product.)
3. Languages and order: C#, Java, TS, Python, Rust, C/C++, Go, Kotlin, with C/C++ and Kotlin allowed to ship as "beta"?
4. SCIP import in v1 or v1.1 (suggest v1.1)?
5. Standalone app (recommended) or panes inside Pitwall/Debrief?
6. Hard rule "never execute anything" confirmed, including no language servers by default and no coverage/test runs (everything via Pitcrew and imports)?
7. Managed agent loop in Pitwall (recommended) vs Debrief orchestrating?
8. Embeddings stay out of roadmap (also for traceability), or keep an opt-in hook?
9. Priority relative to the queue: needs Pitwall's MCP client and structured output; start after Pitwall? And relation to Sighter timing (UI test drafts wait for it).
10. Version mapping for `rev` in Debrief: does the log carry app version/build id so profile rules can pick the revision?
11. Which requirement documents do you actually have (Word headings? Excel columns? PPT slides? DOORS/Polarion ReqIF exports?) and may I get **anonymised
    same-shape samples** (never real ones, per the confidentiality rule) to design the first mapping packs?
12. Gitea comment posting: acceptable in v1.1 with per-review consent, or never?
13. Diagrams: is it acceptable that Pacenotes rewrites `.drawio` files into a canonical one-cell-per-line uncompressed form (so git diffs work), and should
    Pacenotes get its own accent entry in the Paddock `themes.mjs`? May I request `themes.json` as a Paddock build artefact?
14. Rust port of the diagram emitter with the Paddock `.mjs` tools as conformance tests (recommended) vs requiring Node?
15. Pack signing: reuse Debrief's `.debriefplugin` signing scheme, and allow unsigned wasm packs with a warning?
16. Report engine: can Debrief's report engine be shared via Paddock, or should Pacenotes have its own small one?
17. Code review rules: should the AGENTS.md rule pack (no unwrap on user data, async Tauri commands, no innerHTML, no `github.server_url`) ship as a
    built-in example pack, and for which repos?
18. Test generation: acceptable as suggestions only, with validation exclusively through the user's CI/Pitcrew?
19. `.sighterplan` contract: does it match what Sighter will offer (`locate`, `wait_for`, `assert_screen`) and who owns the schema (Sighter)?
20. Markdown review rules: file names and locations. `AGENTS.md` (root + nested) is read as-is; do we add `REVIEW.md` (repo root) and `review/*.md` for review-only rules, and **standardise `REVIEW.md` across the Paddock family repos** (with a shared starter in `paddock/docs`)? Should user-level rules live in the app only, or also in a `~/.config` Markdown file?
21. Should the review also honour other vendors' files (`CLAUDE.md`, `.github/copilot-instructions.md`, `.cursor/rules/*.mdc`) as read-only rule sources when present (default off, one toggle per source)?

## 19. Size and phasing

Rev 1 estimate was about 35-45k LOC Rust (about 70k with tests) and about 12-18k LOC Angular for the code intelligence core alone. Additions:

| Area | Rust (code) | Notes |
|---|---|---|
| Review (rules engine, orchestration, SARIF, Gitea client, Markdown rules parser/conversion/hygiene) | 7-10k | rule packs + test kit; Markdown rules add about 1.5-2k |
| Diagrams (emitter, layout, router, round trip, diff, SVG preview) | 8-10k | the hardest to get right visually |
| Requirements (importers, mapping, links, reports, matrix export) | 6-8k | xlsx/docx/csv/md in v1; pptx/pdf/ReqIF v1.1 |
| Coverage and tests (importers, model, hotspots, inventory, results) | 4-5k | |
| Language packs (wasm loader, validator, test kit) | 3-4k | |
| Check (verdicts) | 2k | |
| MCP additions | +2k | |

Total v1 as catalogued: roughly **70-85k LOC Rust code, about 130-150k with tests**, Angular about 25-30k LOC with tests (review panel, diagram
preview/diff, requirements matrix and mapping editor, coverage views, pack manager), about 12 ADRs (add: rule packs, diagram round-trip, canonical
draw.io form, requirement links/suspect model, coverage import, wasm packs, no-execution contract, Forge port).

**Keep v1 realistic: milestones inside one product** (no cut-down public release, as per your standing preference; internal milestones only):

- M1 (about 3 weeks): core + index + 3 languages (C#, Java, TS) + search/outline/repo map over MCP stdio with Pitwall, git layer.
- M2 (about 3 weeks): references/call graph/stack-trace/log-site/config/throw sites; Debrief flows and the check verdicts; ownership, hotspots.
- M3 (about 3 weeks): diagrams (C4 from manifests, module, class, call graph, sequence) with the round-trip prototype first; remaining core languages.
- M4 (about 3 weeks): review (rule engine, agents-md pack, LLM pass, suppressions, SARIF, CI mode); coverage import and patch coverage; test inventory.
- M5 (about 3 weeks): requirements (xlsx/docx/csv/md, mappings, tags, matrix, reports, impact, review integration); language packs; UI polish;
  hardening, screenshots, docs.
- v1.1 items (Pitwall extraction/semantic links, pptx/pdf/ReqIF, xlsx write-back, Gitea posting, test drafts, JUnit results, Pitcrew triggers,
  state/deployment diagrams, SCIP) follow without being on the critical path.

That is roughly **15-18 weeks elapsed** with parallel agents at the current pace (rev 1 said 8-10 weeks for the smaller scope), plus about 2 weeks of
Pitwall-side work (MCP client, toolsets, managed loop, grants, structured output). The two biggest uncertainty drivers are diagram round-trip
fidelity and per-language reference quality.

## 20. Sources

Code review and quality tools
- Qodo PR-Agent relicensing and community move: https://github.com/The-PR-Agent/pr-agent , https://futurumgroup.com/insights/qodo-hands-pr-agent-to-the-community-will-open-governance-accelerate-ai-code-review/
- Semgrep licensing (engine LGPL-2.1, Rules License v1.0): https://docs.semgrep.dev/licensing , https://semgrep.dev/blog/2024/important-updates-to-semgrep-oss , rules LICENSE: https://raw.githubusercontent.com/semgrep/semgrep-rules/develop/LICENSE
- SonarQube/SonarLint licences and Sonar Source Available License: https://community.sonarsource.com/t/a-new-sonar-license-for-sonarqube-analyzers/130731 , https://sonarsource.com/legal/archive/sonarqube-terms-and-conditions-2026-06/
- CodeRabbit self-hosted: https://docs.coderabbit.ai/self-hosted/overview.md , https://www.augmentcode.com/guides/coderabbit-pricing-costs-limits
- CodeScene: https://rubinlake.com/en/technology-radar/developer-ai-and-delivery/codescene , https://aicodereview.cc/tool/codescene
- Qodana editions: https://www.jetbrains.com/help/qodana/2026.2/pricing.html
- ast-grep (MIT), reviewdog (MIT), danger-js (MIT), sourcery (MIT): GitHub licence API, checked 2026-10-23.

Markdown review rules
- AGENTS.md convention and nesting (secondary sources; check agents.md itself): https://www.mintlify.com/agentsmd/agents.md/guides/monorepos , https://getunblocked.com/blog/claude-md-vs-agents-md-vs-cursor-rules/ , https://hidekazu-konishi.com/entry/agents_md_and_instruction_files_across_coding_agents.html , https://codex.danielvaughan.com/2026/03/26/agents-md-advanced-patterns/
- Copilot instructions: https://docs.github.com/en/copilot/how-tos/configure-custom-instructions-in-your-ide/add-repository-instructions-in-your-ide , https://github.blog/changelog/2025-11-12-copilot-code-review-and-coding-agent-now-support-agent-specific-instructions/ , 4,000-character claim (unverified): https://www.morphllm.com/copilot-code-review
- Cursor rules (community/third-party): https://forum.cursor.com/t/a-deep-dive-into-cursor-rules-0-45/60721 , https://www.morphllm.com/cursor-rules-best-practices
- CodeRabbit path instructions: https://docs.coderabbit.ai/configuration/path-instructions , https://docs.coderabbit.ai/knowledge-base/code-guidelines
- Qodo/PR-Agent extra instructions: https://docs.qodo.ai/code-review/get-started/use-qodo-in-prs/code-review/extra-instructions , https://qodo-merge-docs.qodo.ai/tools/review
- Continue rules: not verified in this run.

Diagrams
- draw.io diagram generation / format: https://www.drawio.com/docs/reference/diagram-generation/ , https://www2.drawio.com/doc/faq/ai-drawio-generation , https://www.drawio.com/doc/faq/export-to-xml
- draw.io removing PlantUML: https://www.drawio.com/blog/plantuml-to-mermaid
- Structurizr manual layout merge: https://docs.structurizr.com/ui/diagrams/manual-layout ; CLI Apache-2.0: https://github.com/structurizr/cli
- ELK/elkjs EPL-2.0: https://raw.githubusercontent.com/kieler/elkjs/master/LICENSE.md , https://projects.eclipse.org/node/8803 , https://projects.eclipse.org/projects/modeling.elk/releases/0.12.0/review
- Graphviz LICENSE (EPL-2.0): https://gitlab.com/graphviz/graphviz/-/raw/main/LICENSE
- PlantUML (LGPL-3.0): https://github.com/plantuml/plantuml ; Mermaid and dagre MIT: GitHub licence API
- Rust layout crates: https://crates.io/crates/rust-sugiyama , https://crates.io/crates/layout-rs , https://docs.rs/crate/bra0-layout/0.2.2 , https://docs.rs/crate/dagre/0.1.1
- Paddock diagram kit: /Volumes/d8sst7/dev.decryptology.net/paddock/diagrams (STYLE.md, paddock.xml, templates, tools)

Understand and docs
- DeepWiki and clones: https://medium.com/@jasonyang.algo/unlocking-codebases-with-ai-introducing-deepwiki-open-a61de3402cc4 , https://codersera.com/blog/deepwiki-complete-developer-guide-2026/
- Swimm: https://docs.swimm.io/new-to-swimm/benefits-of-code-coupled-documentation
- SciTools Understand graphs: https://scitools.com/graphs , https://blog.scitools.com/graphlist/
- Aider repo map: https://aider.chat/docs/repomap.html ; Serena licensing: https://raw.githubusercontent.com/oraios/serena/main/LICENSE

Requirements
- ReqIF: https://www.omg.org/spec/ReqIF , https://pypi.org/project/reqif , https://en.wikipedia.org/wiki/Requirements_Interchange_Format
- StrictDoc (Apache-2.0): https://github.com/strictdoc-project/strictdoc , https://strictdoc.readthedocs.io/en/latest/strictdoc_02_faq.html ; Doorstop (LGPL-3.0, PyPI): https://pypi.org/project/doorstop ; sphinx-needs (MIT): https://github.com/useblocks/sphinx-needs ; RTEMS tool evaluation: https://docs.rtems.org/docs/6.2/eng/req/tooling.html
- LLM traceability: https://arxiv.org/pdf/2602.01253 , https://fuchss.org/conferences/aire25/ , https://arxiv.org/pdf/2509.20149
- Parser crates licences: crates.io API (calamine 0.37 MIT, docx-rs 0.4.22 MIT, quick-xml 0.42 MIT, lopdf 0.46 MIT, pdf-extract 0.12.1 MIT, pdf_oxide 0.3 MIT OR Apache-2.0, zip 9 MIT, csv Unlicense/MIT)

Tests and coverage
- Meta TestGen-LLM: https://arxiv.org/abs/2402.09171v1 , https://2024.esec-fse.org/details/fse-2024-industry/18/Automated-Unit-Test-Improvement-using-Large-Language-Models-at-Meta
- Qodo Cover (AGPL-3.0): https://github.com/qodo-ai/qodo-cover ; coverlet MIT: https://github.com/coverlet-coverage/coverlet ; OpenCover: https://github.com/OpenCover/opencover ; codecov uploader Apache-2.0: https://github.com/codecov/uploader ; cargo-llvm-cov (crates.io, Apache-2.0 OR MIT); c8 (npm, ISC)

Language packs
- Zed extensions and languages: https://zed.dev/docs/extensions/developing-extensions.md , https://zed.dev/docs/extensions/languages , https://zed.dev/blog/zed-decoded-extensions
- Helix adding languages: https://docs.helix-editor.com/guides/adding_languages.html
- tree-sitter Rust crate and wasm feature: https://docs.rs/tree-sitter ; wasmtime (Apache-2.0 WITH LLVM-exception): crates.io
- Grammar licences: raw LICENSE files of tree-sitter-{c-sharp,java,rust,python,typescript,go,cpp,c,json}, fwcd/tree-sitter-kotlin, tree-sitter-grammars/{toml,yaml,xml,markdown}, DerekStride/tree-sitter-sql (all MIT, checked 2026-10-23)

Names
- GitHub repository-name search, crates.io and npm registry lookups for pacenotes, roadbook, gridwalk, recce, scrutineer, stewards, parcferme, codriver, homologate (2026-10-23); https://github.com/DataRecce/recce ; https://github.com/alpha-omega-security/scrutineer ; https://github.com/Ashon/pacenote

Carried over from rev 1
- MCP spec 2026-07-28: https://modelcontextprotocol.io/specification/latest/changelog ; rmcp: https://github.com/modelcontextprotocol/rust-sdk ; SCIP: https://github.com/sourcegraph/scip ; stack-graphs (archived): https://github.com/github/stack-graphs ; Cursor indexing: https://cursor.com/blog/secure-codebase-indexing ; Greptile: https://www.greptile.com/docs/how-greptile-works/graph-based-codebase-context ; Sourcegraph changes: https://sourcegraph.com/blog/changes-to-cody-free-pro-and-enterprise-starter-plans ; Pitwall docs: /Volumes/d8sst7/dev.decryptology.net/pitwall/docs/
