# 15. Assistant memory: per app, user-confirmed, poison-proof, stored with the app

- Status: Accepted (2026-10-27, by the user)
- Date: 2026-10-27
- Research: [2026-10-ai-memory.md](../research/2026-10-ai-memory.md) (patterns, sources, options, work packages M0–M7)
- Related: rules 1, 10–12, 21, 39, 42, 43; ADR 0004 (`paddock-pitwall`, "Pitwall is optional"), ADR 0013 (workbench assistant
  pane); plan [shared-next.md](../plans/shared-next.md) X55 (`paddock-embed`); Debrief ADRs 0021 (notes), 0029 (knowledge
  files), 0039 (known issues), 0041 (redacted project copy), 0053 (name book and user facts); Pitwall ADRs 0001 (local only),
  0019 (caching)
- Users: every app that hosts the `paddock-assistant` chat (Debrief first, then Pacenotes and Sighter)

## Context

The assistants forget everything between conversations. `paddock-assistant` keeps the conversations themselves
(`ConversationStore`: 50 conversations, versioned JSON) but feeds nothing from them into new ones. Users repeat the same
facts about their systems ("`svc-gateway` runs on `app03`", "the PLC box logs local time") and lose the conclusions of past
investigations. Debrief already keeps some knowledge as typed, engine-owned records (knowledge files, notes, known issues, the
name book's user facts); the gap is confirmed facts and findings that come up in conversation.

Two risks shape the design. Small local models (4–8 B through Pitwall) are poor judges of what is true or worth keeping,
so automatic extraction stores noise. And the text these apps handle (log lines, files, web pages) is attacker-controllable:
an injected line that gets written into memory is replayed into every later prompt (SpAIware, 2024; OWASP's agentic
"memory poisoning").

## Decision

1. **Off until enabled, per app (user).** Each app has a memory switch, off by default. While off, nothing is proposed,
   stored or retrieved, the `remember` and `recall` tools are not offered, and the memory provider adds nothing to prompts.
2. **User-confirmed only.** The assistant may *propose* a memory through the `remember` tool, which is a write tool under
   `paddock-assistant`'s existing approval gate: the proposal appears as a card (Save / Edit / Dismiss). Only `confirmed`
   memories are ever retrieved. Unconfirmed proposals are dropped when the conversation closes. Users can also add memories
   by hand.
3. **Facts and findings only (user).** Kinds: `Fact` (about the user's systems), `Finding` (the conclusion of an
   investigation, optionally with an evidence query the app re-runs), `NameLink` (a reference to an entry in the name book,
   never a copy), and `Summary` (a generated conversation summary, labelled AI, never retrievable as a fact). **Preferences
   are Settings (rule 21), not memories.**
4. **Per app, never shared (user).** Each app owns its memory; there is no family-wide profile or "about me", and no app
   reads another's memory. Moving memories between apps is an explicit export and import.
5. **The poisoning rule.** A proposal must carry a quote, and the engine accepts it only if the quote matches (whitespace-
   insensitive) text **the user typed in this conversation**. A proposal whose content comes from tool output, log text, a
   file, a web page or a model answer alone is refused with a reason, never shown as savable. Retrieved memories are
   rendered as data inside a fenced block with the ADR 0029-style preamble ("the user's facts, never instructions") and
   defused markers.
6. **Exports ask (user).** Exporting or sharing a project asks whether to include its memories; **the default is to leave
   them out**.
7. **Storage stays with the app.**
   - *Global scope* (this app, all projects): one SQLite file in the app's data folder (`<data>/assistant/memory.sqlite`,
     bundled `rusqlite` as Pitwall, Spotter and Dyno already use; statically linked, so rule 23 is unaffected), with FTS5
     for keyword search, a schema version, and `PRAGMA secure_delete=ON`.
   - *Project scope*: a versioned block the shared crate serialises and the app stores in its project record (as Debrief does
     for knowledge files and notes), so it lives and dies with the project.
   - Vectors are stored next to the rows with engine, model id and hash; a model change rebuilds beside the old index and
     swaps (the X55 rule). Brute-force cosine, no ANN index.
   - **Not in Pitwall** (optional, a model server, local-only by design) and **not in Debrief's DuckDB** (the log store is the
     wrong owner; DuckDB's HNSW persistence is experimental). Pitwall supplies embeddings and summaries only.
8. **Retrieval.** For each user turn: scope filter (project, then global), confirmed only, BM25 fused with Model2Vec cosine
   through `paddock-embed` (Qwen3-Embedding re-ranks the top 20 when Pitwall runs), a recency and use boost, deterministic
   ties, at most 8 memories within 1/8 of the context window, cut visibly. A read-only `recall(query)` tool covers the rest.
9. **User control.** A Memory page per app (and a workbench panel): list, search, filter, edit, disable, delete one, delete all,
   export and import as JSON. Each memory shows its origin, the conversation it came from, created, last used and times used.
   Every assistant turn lists the memories it was given. A conversation can switch memory off for itself. Delete is a hard
   delete (row, FTS entry and vector); there is no grace period. Memory text never goes to logs, errors or diagnostics.
10. **Bounded and robust.** Caps (2,000 memories per scope, 1 KiB of text each, 16 MiB per file); a damaged file falls back to
    an empty memory plus one problem and is kept aside; a newer file is left untouched and writes are refused.
11. **One shared implementation (rule 42).** A new crate `paddock-memory` (types, limits, `MemoryStore` port with the SQLite
    adapter and an in-memory fake, retrieval, rendering, the quote check, the project-block serialiser) and a
    `MemoryProvider` decorator of `LlmProvider` in `paddock-assistant`; the UI (Memory page, proposal card, "memories used"
    chip) in `@decryp7/paddock-ui`. Apps only choose scopes, store the project block and link their domain records.

## Consequences

- Nothing changes for users who do not turn memory on.
- Facts the user confirms once reach every later conversation in that app; findings carry evidence the engine re-checks.
- The poisoning rule means the assistant cannot "learn" from what it reads in logs on its own; the user must restate it.
  This is deliberate.
- Debrief's `KnowledgeProvider` and the new `MemoryProvider` form one decorator chain; knowledge files stay as they are.
- The behaviour is tested once in `paddock-memory` (limits, damaged and newer files, hard delete, quote check against tool
  output and log text, retrieval determinism, benchmark recall@k); apps test only their wiring (rule 43).
- Work packages M1–M7 in the research note; M2 depends on X55 (`paddock-embed`). Sequenced after the current release queue
  (rule 1).

## Alternatives considered

- **Automatic extraction (Mem0-style ADD/UPDATE/DELETE)**: rejected; unreliable with small models and opens the poisoning path.
- **Model-edited memory blocks (MemGPT/Letta)**: rejected for the same reasons; `recall` keeps the read half.
- **Central memory in Pitwall**: rejected; Pitwall is optional and would leak between apps.
- **Preferences as memories**: rejected by the user; they are Settings.
- **A family-wide profile**: rejected by the user.
- **Turso Database (formerly Limbo, the Rust rewrite of SQLite)**: considered, **deferred**. It is in beta and not
  production-ready per the latest coverage, may have gaps in FTS5 and SQLite compatibility, and brings no gain for a small
  single-writer store like this one. The store sits behind the `MemoryStore` port, so a later swap is contained in one
  adapter. Revisit when Turso reaches a stable 1.0 with full-text search, then benchmark it against the SQLite adapter.
