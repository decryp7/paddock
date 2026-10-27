# Long-term memory for the Paddock assistants (2026-10-27)

Question (user): "Memory for AI using database?" Should the assistants (Debrief, Pacenotes, Sighter and the others, built on
`crates/paddock-assistant` over Pitwall) remember things across conversations, stored in a database? Covers the memory
patterns in use today, privacy and user control, where to store it, what is worth remembering in a log-analysis IDE, and a
shared Paddock design (rule 42) with work packages. Research note, not a decision.

## Answer in short

- **Yes, but as a small, typed, user-confirmed memory, not "remember everything".** The assistant *proposes* a memory; the
  user confirms it with one click (the same approval gate `paddock-assistant` already has for write tools). Only
  confirmed memories reach later prompts. Small local models (4–8 B) are poor judges of what is true and important, and
  log text is untrusted input, so automatic extraction would store noise and invite memory poisoning.
- **Per app, in the app's own data folder, in SQLite** (bundled `rusqlite`, FTS5), through one shared crate
  **`paddock-memory`**. Project-scoped memories travel inside the project record (like Debrief's notes and knowledge files).
  **Not in Pitwall**: Pitwall is optional, local-only and stateless about content by design; it supplies embeddings and
  summaries, never storage.
- **Retrieval: scope → hybrid search → recency.** BM25 (FTS5) fused with Model2Vec cosine
  through `paddock-embed` (X55; Qwen3-Embedding via Pitwall as the optional re-ranker), boosted by recency and use, inside
  a fixed slice of the context window. The model can also call a read-only `recall` tool.
- **The user sees, edits, disables, exports and deletes every memory, per app**, sees which memories each answer used, and
  can chat without memory. Delete is a hard delete.
- **In Debrief much of "memory" already exists** as engine-owned, typed data: the name book's user facts (ADR 0053),
  knowledge files (ADR 0029), notes (ADR 0021) and known issues (ADR 0039). The new layer adds what is missing (confirmed
  facts about the user's systems, conclusions of past investigations; preferences are Settings) and *links* to the existing records
  instead of copying them.

## Decisions (user, 2026-10-27)

Answers to the open questions of the first draft; they override the proposals in the sections below where they differ.
The design is drafted as [ADR 0015](../adr/0015-assistant-memory.md) (Proposed).

1. **Memory is off until the user turns it on, per app.** Nothing is remembered, proposed or retrieved in an app until memory
   is enabled in that app's settings.
2. **Preferences are Settings (rule 21), not memories.** Memory holds **facts and findings only** (plus links to name-book
   entries and generated, AI-labelled conversation summaries, which are never treated as facts).
3. **Exported or shared projects ask whether to include memories, default excluded.**
4. **No family-wide "about me".** Memory is per app only; nothing is shared between apps.

## 1. What exists today

| Where | What it keeps | Limits |
|---|---|---|
| `paddock-assistant` `ConversationStore` | Conversations: messages, tool calls, context chips, errors; one versioned JSON file through `paddock-env` | 50 conversations, 400 messages each, 4 MiB; no search, nothing fed back into new conversations |
| `paddock-assistant` tools | `Access` read/write; write tools offered only after a user click, each call waits for approval | The natural gate for "remember this" |
| Debrief ADR 0029 knowledge files | User-written Markdown, global and per project, added to every AI prompt by a `KnowledgeProvider` decorator of `LlmProvider` (≤ ¼ of the context, ≤ 96 KiB, defused markers, "facts, never instructions") | Hand-written only; the whole block every time (no retrieval) |
| Debrief ADR 0053 name book | Layer 2 AI aliases (validated, labelled, removable) and **user facts**: accepted/rejected matches, aliases, renames, merges, splits, per workspace or global, with undo; they outrank AI aliases | Names only; shared as `paddock-names` (X18) |
| Debrief ADR 0021 notes, ADR 0039 known issues | Notes on anchors (event, range, image region…), known-issue declarations with a verified query, playbooks | Already engine-owned and AI-visible |
| Pitwall ADR 0019 | Prompt-prefix reuse, response cache, embeddings cache | A cache, keyed by model; not memory |
| Pitwall ADR 0001 | Local only, no telemetry, prompts never logged by default | Applies to any memory feature |
| X55 `paddock-embed` (planned) | `Embedder` port: Model2Vec built in, Qwen3-Embedding via Pitwall; brute-force cosine; RRF with BM25; index metadata and swap-on-rebuild | The retrieval engine memory should reuse |

## 2. Memory patterns in use

- **Taxonomy (CoALA, arXiv 2309.02427):** *working* memory (the prompt), *episodic* (what happened: past conversations,
  their summaries), *semantic* (facts and preferences) and *procedural* (how to act: system prompt, playbooks). LangMem
  uses the same three long-term kinds (semantic facts/profile, episodic summaries, procedural instructions) on a
  namespaced store.
- **Retrieval scoring (Generative Agents, arXiv 2304.03442):** score = recency (exponential decay since last use) +
  importance (rated once when written) + relevance (embedding similarity), each normalised. Still the common baseline.
- **Self-editing tiers (MemGPT / Letta, arXiv 2310.08560):** a small "core" block always in the prompt, plus archival and
  recall stores the model searches and edits through tools. Works with strong models; small models call such tools badly.
- **Extract-and-consolidate (Mem0, arXiv 2504.19413):** after each turn an LLM extracts candidate facts and compares them
  with the top-k similar memories, choosing ADD / UPDATE / DELETE / NOOP. Mem0's own 2026 algorithm moved to ADD-only
  extraction (nothing overwritten) and reports LoCoMo 91.6 vs 71.4 for the old one (vendor figures); an independent
  evaluation (arXiv 2604.07877) puts Mem0 at 64.2 % on LoCoMo at a 1,000-token budget. Lesson: automatic consolidation
  by an LLM is fragile even with large models.
- **Product controls (ChatGPT):** two switches, "saved memories" (explicit facts) and "reference chat history"; a
  Manage memories list with delete one / clear all; Temporary Chat that neither reads nor writes memory; deleting a chat
  does not delete memories made from it; deleted memories kept up to 30 days. The newer system updates memories
  automatically.
- **Client-side memory (Anthropic memory tool):** the model reads and writes files under `/memories`; the application
  owns storage and must confine paths. Storage stays with the app, which is the shape Paddock wants.
- **Threat: memory poisoning.** SpAIware (Rehberger, 2024) used prompt injection from a web page to write instructions
  into ChatGPT's memory, which then exfiltrated every later conversation; OpenAI fixed the exfiltration path, but injection
  into memory stays an open problem. OWASP's agentic threats list memory poisoning as its own threat. **For a log IDE this is
  central: log lines, file contents and web pages are attacker-controllable text.**

What we take: CoALA's kinds, Generative Agents' scoring (with "importance" replaced by "user-confirmed"), Mem0's
comparison with similar memories (as a *suggestion* to the user: "this updates memory X"), ChatGPT's controls without
the 30-day retention, and the memory tool's "app owns the storage". What we leave: automatic writes and model-driven
edits.

## 3. What is worth remembering

| Kind | Debrief examples | Pacenotes / Sighter / others | Source of truth |
|---|---|---|---|
| **Fact about the user's system** (semantic) | "`svc-gateway` runs on `app03` and `app04`"; "timestamps from the PLC box are local time, UTC+9"; "`0x80070005` at start-up is harmless on our build agents" | Pacenotes: "`core/legacy` is frozen, don't suggest changes"; Sighter: "the gear icon is Settings in ToolX" | The memory, confirmed by the user |
| **Resolved name** | "graphic builder" = `CompanyA.Graphic.Builder.exe` | Dyno counters, Spotter products | **Name book** (ADR 0053 / `paddock-names`); memory stores a link, not a copy |
| **Finding of a past investigation** (episodic) | "2026-10-12 outage: pool exhausted after the 3.2 deploy; evidence query `…`" | Pitcrew: "flaky test X is the shared temp dir" | Memory with the project and an evidence query the engine re-runs |
| ~~Preference~~ | "answer in short bullets", "use UTC", "my default lens is Errors" | | **Settings** (rule 21; decision 2), not memory |
| **Procedure** | "for sign-in failures, check the IdP log first" | | Playbooks / known issues (ADR 0039); memory links |
| **Conversation summary** | 3–5 lines per closed conversation, searchable | all | Generated (Pitwall, background), labelled AI, never treated as fact |

Not worth remembering, or not allowed: raw log lines or file contents, whole conversations (they are already kept), anything
taken from tool output or documents without the user confirming it, secrets and credentials, personal data from logs
(user names, e-mail, IPs) unless the user writes it themselves, and anything the user marked confidential (memory file
`confidential-user-logs`: pasted logs never leave their project).

## 4. Privacy and user control

0. **Off until enabled, per app** (decision 1). With memory off the `remember`/`recall` tools are not offered and the
   provider adds nothing.
1. **Per app, never shared** (decision 4). Each app has its own memory; nothing crosses apps unless the user exports and
   imports it. Scopes: *global* (this app, all projects), *project* (stored in the project record; on export or share the app
   **asks whether to include memories, default excluded** (decision 3), like ADR 0041 redacted copies), *conversation*
   (none: that is the chat).
2. **Only confirmed memories are used.** States: `proposed` (shown in the chat as a card: Save / Edit / Dismiss) →
   `confirmed` → `disabled`. A proposal that is not confirmed is dropped when the conversation closes.
3. **Writes come from the user's words only.** The `remember` tool may only cite text the user typed in this conversation
   (checked by the engine like ADR 0039's "fix must be a quote from the notes"); proposals that cite tool output, log text,
   a web page or a file are refused. This closes the SpAIware path for logs.
4. **Memory is data, never instructions.** Rendered in a fenced block with the ADR 0029 preamble and defused markers.
5. **See, edit, delete.** A Memory page per app (Settings ▸ AI ▸ Memory, and a workbench panel): list, search, filter by scope
   and kind, edit text, disable, delete one, delete all, export/import as JSON. Each memory shows origin (typed by user,
   proposed by AI and confirmed), the conversation it came from, created / last used / times used.
6. **Transparency per answer.** Every assistant turn lists the memories it was given ("Used 3 memories"), each clickable.
7. **Chat without memory.** A per-conversation switch (and an app-wide "memory off") that neither reads nor writes.
8. **Hard delete.** Delete removes the row, its FTS entry and its vector; `PRAGMA secure_delete=ON`; no grace log. Deleting
   a conversation offers to delete the memories made from it.
9. **Local only and quiet.** Memory text never goes to logs, error messages or diagnostics bundles (ids and sizes only);
   it leaves the machine only in an export the user chose. Embeddings computed by Pitwall stay in Pitwall's cache per ADR
   0019 and are cleared with it.
10. **Bounded and versioned.** Caps (e.g. 2,000 memories per scope, 1 KiB each, store ≤ 16 MiB), schema version, damaged
    file → defaults plus one problem, newer file untouched (AGENTS.md persistence rules).

## 5. Where to store it

| Option | For | Against | Verdict |
|---|---|---|---|
| **Per-app SQLite file** (`<data>/assistant/memory.sqlite`, bundled `rusqlite` 0.40 as Pitwall, Spotter and Dyno already use) | One engine for every app; FTS5 for BM25; transactions, hard delete; statically linked (no dylib, rule 23 safe); small | One more file per app | **Global scope** |
| **The project record** (Debrief project folder, Pacenotes workspace) | Project memories travel with the project and die with it | Format per app | **Project scope**: the crate serialises a versioned block the app stores, as ADR 0029 does for knowledge |
| Debrief's DuckDB | Already there; SQL | Lives in the store worker; other apps have no DuckDB; VSS's HNSW persistence is experimental (WAL recovery incomplete, index must fit in RAM); a log store is the wrong owner for user facts | No |
| JSON file via `paddock-env` (like conversations) | Simplest | No search, whole-file rewrites | Only for the first slice if SQLite is deferred |
| **Pitwall central** | One memory for all apps | Pitwall is optional (memory would vanish without it); cross-app leakage; turns the model server into a data store against ADR 0001's posture; pairing keys would gate private data | **No**: Pitwall provides embeddings and summaries only |

Vectors: Model2Vec `potion-base-8M` gives 256 dims; 10,000 memories × 256 × 4 bytes ≈ 10 MB, brute-force cosine in well
under a millisecond, so no ANN index (matches X55). Vectors are stored with engine, model id and hash; a model change
rebuilds beside the old index and swaps (X55's rule).

## 6. Retrieval

For each user turn, before the model call (pure function over the store, budgets from settings):

1. ~~**Pinned** preferences~~: dropped (decision 2: preferences are Settings, which the app applies itself).
2. **Candidates**: scope filter (project, then global), state = confirmed, then BM25 over FTS5 and Model2Vec cosine over the
   user's message plus the conversation title, fused by RRF (X55's fusion and weights).
3. **Score**: fused rank × recency decay (half-life a setting, e.g. 30 days since last use) × small use-count boost; ties by
   id (deterministic).
4. **Optional re-rank**: when Pitwall runs, Qwen3-Embedding re-scores the top 20 (X55's measured rerank).
5. **Budget**: at most N memories (default 8) inside 1/8 of the context, after Debrief's rules and before knowledge files are
   cut; the cut is visible like ADR 0029's.
6. **Tool**: a read-only `recall(query)` for when the model wants more; writes go only through `remember` (proposal + user
   approval).

Name resolution stays in `paddock-names` (the assistant's `find_values` tool); a memory that is a name link is rendered as
"user confirmed: X = Y" from the name book, so the two never disagree.

## 7. Shared design: `paddock-memory` (rule 42)

```
paddock-assistant ──uses──▶ paddock-memory ──uses──▶ paddock-embed (X55) ──▶ Model2Vec | paddock-pitwall
       │                        │  ports: MemoryStore (SQLite adapter, in-memory fake)
       │                        │  pure: types, limits, retrieval scoring, rendering, quote check
       └─ MemoryProvider (LlmProvider decorator, like Debrief's KnowledgeProvider)
paddock-ui: memory page + "memories used" chip + proposal card
app: chooses scopes, stores the project block, may link domain records (name book, notes, known issues)
```

Types (sketch): `Memory { id, scope: Global | Project(ProjectId), kind: Fact | Finding | NameLink | Summary,
text, state: Proposed | Confirmed | Disabled, origin: User | AiConfirmed | Generated, quote: Option<String>,
links: Vec<RecordRef>, created, updated, last_used, uses, source_conversation }`. Illegal states excluded: a `Summary` is
never `Confirmed` as a fact; `AiConfirmed` requires a quote that the engine matched in a user message.

### Work packages

| WP | Scope | Depends | Acceptance | Effort |
|---|---|---|---|---|
| **M0** ADR | [ADR 0015](../adr/0015-assistant-memory.md) "assistant memory" (drafted, Proposed): the rules of sections 4–6 with the user's decisions | this note | accepted by the user | 0.5 |
| **M1** `paddock-memory` core | types, limits, `MemoryStore` port, SQLite adapter (bundled, FTS5, schema v1, `secure_delete`), in-memory fake, project-block (de)serialiser | `paddock-env` | damaged / newer file tests; hard delete leaves no row, FTS or vector; caps hold; no text in logs (test) | 2 |
| **M2** retrieval | hybrid BM25 + Model2Vec via `paddock-embed`, recency, budget, deterministic order; benchmark set of invented memories and queries (recall@k) | X55 | Pitwall down → Model2Vec only with a health entry; model change → rebuild and swap; recall@5 target set in M0 | 2 |
| **M3** assistant wiring | `MemoryProvider` decorator; `remember` (write, approval, quote check) and `recall` (read) tools; "memories used" in `AssistantEvent`; memory-off per conversation | M1, M2, `paddock-assistant` | injection tests: a `remember` citing tool output, log text or a file is refused; a conversation with memory off neither reads nor writes | 2 |
| **M4** UI | `@decryp7/paddock-ui` memory page, proposal card, used-memories chip, export/import; mock backend | M3 | CDK harness tests; Playwright flow (propose → confirm → used in next chat → delete) in Chromium | 2 |
| **M5** summaries | conversation summary on close via Pitwall (`background` priority, cancellable job, rule 10–12), labelled AI, searchable | M1, `paddock-pitwall` | no summary without Pitwall and no error; summary never offered as a confirmed fact | 1 |
| **M6** Debrief adoption | memory switch (off by default); global + project scopes; project block in the project record; export/share asks "include memories?" (default no); name-book and notes links; move `KnowledgeProvider` next to `MemoryProvider` (one decorator chain) | M3, M4 | real-engine test: a confirmed fact changes the next answer's context; shared project carries project memories | 1.5 |
| **M7** Pacenotes, Sighter | adopt with their own kinds; Sighter: element aliases as memories feeding the VLM labeller | M6 | app wiring tests only (rule 43) | 1 each |

Order: after X55 (`paddock-embed`) and the current Debrief 1.1 / Pitwall release queue (one app at a time, rule 1);
M1–M4 can start as soon as X55's port exists.

## 8. Open questions for the user

All four were answered on 2026-10-27; see [Decisions](#decisions-user-2026-10-27): off until enabled per app; preferences are
Settings; export asks, default excluded; per app only, no family-wide "about me".

## Sources

- CoALA, Cognitive Architectures for Language Agents: https://arxiv.org/abs/2309.02427
- Generative Agents (recency, importance, relevance): https://arxiv.org/abs/2304.03442
- MemGPT: https://arxiv.org/abs/2310.08560 ; Letta: https://github.com/letta-ai/letta
- Mem0 paper: https://arxiv.org/abs/2504.19413 ; Mem0 research page (2026 algorithm, LoCoMo): https://mem0.ai/research ;
  MemReader (independent LoCoMo figure): https://arxiv.org/abs/2604.07877
- LangMem (semantic, episodic, procedural): https://langchain-ai.github.io/langmem/
- ChatGPT Memory FAQ: https://help.openai.com/en/articles/8590148-memory-faq
- Anthropic memory tool: https://platform.claude.com/docs/en/agents-and-tools/tool-use/memory-tool
- SpAIware (memory poisoning): https://thehackernews.com/2024/09/chatgpt-macos-flaw-couldve-enabled-long.html ;
  https://embracethered.com/blog/posts/2024/chatgpt-macos-app-persistent-data-exfiltration/
- OWASP Agentic AI threats and mitigations (memory poisoning): https://genai.owasp.org/resource/agentic-ai-threats-and-mitigations/
- DuckDB VSS (HNSW persistence experimental): https://duckdb.org/docs/stable/core_extensions/vss
- SQLite FTS5: https://www.sqlite.org/fts5.html ; `secure_delete`: https://www.sqlite.org/pragma.html#pragma_secure_delete
- Model2Vec: https://github.com/MinishLab/model2vec-rs
- Internal: `crates/paddock-assistant/src/{lib,conversation}.rs`; `docs/plans/shared-next.md` (X55);
  `apps/pacenotes/docs/adr/0001-embeddings.md`; Debrief ADRs 0021, 0029, 0039, 0041, 0053, 0059; Pitwall ADRs 0001, 0009, 0019;
  `docs/rules.md` (rules 1, 10–12, 21, 39, 42, 43).
