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
- spikes S1 to S5, which touch no shared code;
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
  - Tantivy (content);
  - the vector file (flat f16; USearch above about 1M chunks);
  - encrypted blobs for remote text.
- **Extension tiers:** built-in; TOML packs (quicklinks, web searches, snippets, workflows); Rust out-of-process plugins
  (new `SearchSource` and `ResultProvider` contributions).
- **Every long operation** is a `paddock-jobs` job with phases, a growing count, Cancel, and build-then-swap (rules 10
  to 14). Bulk work runs at utility QoS; queries are live work (rule 52).
- **Logs** follow ER-27: phases, durations, ids, counts and outcomes; never names, paths, queries or text. A test with
  invented, distinctive names asserts they never reach a log or a diagnostics bundle (the Dyno ADR 0029 §6 pattern).

## 1. Spikes first (before any ADR is accepted)

Each spike runs on synthetic data only (ER-14). Each records its numbers in `apps/recce/docs/performance.md` (rule 48),
uses a private `CARGO_TARGET_DIR`, and runs as a heavy job through the semaphore.

| Spike | Question | Output | Size |
|---|---|---|---|
| S1 | Name table: memory and per-keystroke latency for 0.5M, 2M and 5M generated paths; own table against a Tantivy n-gram field; `paddock-names` matching cost | A results block, and the choice for ADR R2 | 2 to 3 |
| S2 | Extraction: `pdfium-render`, `pdf_oxide` and `lopdf` on a generated PDF corpus; kreuzberg (features trimmed) against our own thin adapters on OOXML, PDF, eml and PST; binary size, `cargo deny`, peak RSS per file, throughput per worker | Library choice for `paddock-extract` | 3 to 4 |
| S3 | Change feeds: macOS FSEvents history across a reboot, with a UUID check; Windows journal through an elevated test process in the VM (local only, never CI); whether the "unprivileged" journal read exists and what it returns; Linux inotify cost at 100k folders | Feed design for ADR R3 | 3 |
| S4 | macOS: does Full Disk Access survive an ad-hoc re-signed update? (local VM, lessons.md) | Yes or no, and the Repair flow | 1 |
| S5 | Graph: PKCE loopback sign-in with a personal account and a test tenant; delta for drive and mail; `410` and `syncStateNotFound` paths with a fake server | Connector design for ADR R5 | 2 to 3 |

## 2. ADRs to write (app numbering, `apps/recce/docs/adr/`)

| ADR | Decision | After |
|---|---|---|
| R1 | Architecture: agent, app, workers, helper; ports and adapters; the IPC contract | user decisions D1 to D3 |
| R2 | Name table and query model (kinds, filters, ranking, frecency) | S1 |
| R3 | Change feeds and the manifest (shared `paddock-watch` `ChangeFeed`; shared `paddock-walk`) | S3, Pacenotes ADR 0012 |
| R4 | Content and meaning index (Tantivy schema, chunking, `paddock-embed`, vector store, RRF) | S2, X55 |
| R5 | Connectors: Graph, IMAP, local mail; secrets; permission handling | S5 |
| R6 | Privacy: storage, encryption, exclusions, MCP exposure, logs | D5 to D8 |
| R7 | Launcher: hotkey per OS, app catalogue, calculator, clipboard, snippets, system actions | D9 |
| R8 | Extensions: packs and workflows, plugin contributions | D10 |
| R9 | Answers: retrieval, prompt, citation check, Pitwall preset | stage 4 |

Family ADRs needed (shared code):
- an amendment to ADR 0001 for the new contribution kinds;
- one ADR for `paddock-extract`;
- an amendment to ADR 0007 if MCP needs new shared limits.

## 3. Stages and work packages

Sizes are agent-days and rough; ranges are wide on purpose. The "Shared" column names the shared work each package depends
on or creates (rules 42, 43 and 55: a shared change lands with its adoption).

### Stage 1: launcher and file name search (about 38 to 57 agent-days)

The first useful release: hotkey, apps, files by name, calculator, actions, plus the agent and the shell.

| WP | Scope | Shared | Acceptance | Size |
|---|---|---|---|---|
| 1.1 | App skeleton from the template (`scripts/new-app.mjs`): shell, settings, jobs pane, `paddock.ci.json`, workflow, CODEOWNERS | shell (ADR 0012), composition contract (ADR 0037) | Conformance green; the app starts and quits on three OS | 3 to 4 |
| 1.2 | `recce-agent` as a login item; IPC contract with generated TypeScript; supervisor; health | `paddock-service`, `paddock-ipc`, `paddock-health` | Agent restarts after a crash; app shows Repair (rule 19) | 4 to 6 |
| 1.3 | Name table (from S1): build, query, mmap snapshot, frecency | `paddock-names` | S1 budgets met in `paddock-bench` | 5 to 7 |
| 1.4 | First listing and the manifest walk with stat fields; roots and exclusions (built-in pack plus settings); network shares by scheduled rescan; cloud placeholders never read | `paddock-walk` (IG14, with Pacenotes), `paddock-packs` | 1M generated files indexed within the budget on SSD; progress and Cancel per phase | 5 to 7 |
| 1.5 | Change feeds: FSEvents history (macOS); RDCW plus manifest (Windows, no helper); inotify roots (Linux); cursors in the catalog | `paddock-watch` `ChangeFeed` (new, with Dyno ADR 0029 WP-8) | Create, rename and delete show within 2 s; a dropped-events path triggers a rescan; tests use fakes (ADR 0041) | 5 to 7 |
| 1.6 | Launcher window: pre-created and hidden, shown on the hotkey; results as you type; keyboard only; preview pane (file metadata, Quick Look-style preview where the OS offers one **(verify)**); actions (open, reveal, copy path, open with) | `global-hotkey`, `paddock-files`, `paddock-window` | ≤ 50 ms to paint (budget); accessible (CDK listbox, rule 47) | 5 to 7 |
| 1.7 | App catalogue per OS, including Store apps and Settings pages (L2); calculator (`fend-core`); system commands with confirmation | none | The listed apps of a VM image are found; destructive actions confirm | 3 to 5 |
| 1.8 | Query and action model (catalogue Q2 to Q7, Q12, A1 to A4, L5 to L10, L16, E1 in part): filter syntax and chips, saved searches and scopes as a TOML pack, recent searches; action panel on every result, file buffer, Alt+1..9, run as admin; quicklinks and keyword web searches with `{query}` placeholders and keyword-only suggestions (moved here from 4.3); fallbacks, aliases, Home with pinned and recent items, compact mode, `recce://` command links, hotkey conflict check | `paddock-packs`, `paddock-siblings` (deep links) | Syntax and chips give the same query (property test); a plain query never reaches the network (test with a fake egress); every action reachable by keyboard with its shortcut shown | 5 to 8 |
| 1.9 | Coverage and source health for local sources (C16, F4, V6): counts, last change, unreadable folders and why (no Full Disk Access, excluded, locked) with Repair; rebuild one source; pause indexing from the tray | `paddock-health`, `paddock-window` | A denied folder in the VM shows its reason and a working Repair; the invented-names test covers the view's logs | 3 to 4 |

### Stage 2: content and meaning search (about 41 to 65 agent-days)

| WP | Scope | Shared | Acceptance | Size |
|---|---|---|---|---|
| 2.1 | `paddock-extract`: ports, guarded worker protocol, limits (size, time, memory, zip ratio), quarantine by content hash; OOXML (moved from Pacenotes' reader), PDF (S2 choice), text, HTML, RTF | **new shared crate**; `paddock-guard`, `paddock-proc`, `paddock-archive`; safe XML from Pacenotes ADR 0010 | Fuzz targets; a hostile corpus never takes the agent down; Pacenotes adopts it in the same change (rule 55) | 8 to 12 |
| 2.2 | Tantivy content index: schema, chunking, passage re-read from the source, deletion on change; content operators (phrase, same line or section, fields) (Q8); preview with hit highlighting and open at the match (P2, P3); "why this result" (Q11) | none | BM25 query ≤ 100 ms p95 at 1M generated documents; a stored synthetic corpus finds every planted hit (Q15) | 6 to 9 |
| 2.3 | Embeddings through `paddock-embed` (Model2Vec default; multilingual model a Setting); flat vector store; RRF; Qwen re-rank of the top 50 through Pitwall when it runs | **X55 `paddock-embed`** (Debrief and Pacenotes build it first) | hit@5 against a stored synthetic query set (the bar set in R4); the index record lets a model change rebuild beside and swap | 5 to 8 |
| 2.4 | Content job: pipeline walk → extract → chunk → index → embed with stage threads as Settings (Auto); utility QoS; resumable | `paddock-jobs` | Phases, counts and Cancel; idle CPU ≈ 0 after catch-up | 4 to 6 |
| 2.5 | Windows helper (journal and MFT listing, read-only) | **`paddock-helper`** extracted from Dyno's helper (ADR 0034 copies), or an app helper with a registered copy | Callers checked; installer and uninstall paths in the VM; rule 54 bundle build | 8 to 12 |
| 2.6 | Privacy storage: no stored local text; encrypted store for remote text and clipboard; FDE check at first run; Forget a source; the privacy ledger (V3: per source what is indexed, where, its size; index size and memory cap, F3) | `paddock-secrets`, `paddock-storage` | The invented-names test passes on index, logs and diagnostics | 5 to 8 |
| 2.7 | Deterministic natural-language filters shown as chips (Q10); browser bookmarks (C10); EPUB (C2); pause bulk work on battery and while the user is busy (F2); "Open in Debrief" for logs and bundles (G3) | `paddock-siblings` | The filter parser has a table test of phrases to chips and never calls a model; idle CPU ≈ 0 after catch-up | 4 to 6 |

### Stage 3: email and SharePoint/OneDrive (about 31 to 47 agent-days)

| WP | Scope | Shared | Acceptance | Size |
|---|---|---|---|---|
| 3.1 | Local mail: eml, mbox (`mail-parser`), PST (`outlook-pst`), msg; attachments through `paddock-extract`; Apple Mail `.emlx` and Thunderbird folders | `paddock-mail` if a second app needs it, else `recce-mail` | Generated mailboxes (never real mail, ER-14); threads and attachments searchable | 6 to 9 |
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

**Total, rough:** about 146 to 223 agent-days after the spikes (10 to 13), before review rounds and releases. Firm figures
come with the ADRs' work packages.

## 4. Dependencies on other apps' shared work

| Needs | From | Status (2026-11-04) |
|---|---|---|
| `paddock-embed` (X55) | Debrief 1.2 and Pacenotes stage 2 | Planned ([shared-next X55](shared-next.md)) |
| `paddock-walk` stat fields and Windows enumeration (IG14) | Pacenotes ADR 0012 | Accepted 2026-11-04 on `docs/pacenotes-fast-ingest` |
| `ChangeFeed` with cursors in `paddock-watch` | Dyno ADR 0029 WP-8 (journal through the helper) and Pacenotes | Not started; propose as shared from the start |
| Privileged helper pattern | Dyno 1.1 (ADR 0027) | In the Dyno 1.1 release |
| Safe XML and OOXML reading | Pacenotes ADRs 0009 and 0010 | Accepted; code in Pacenotes stage work |
| Shell, dialogs, jobs pane, settings | ADRs 0012, 0026, 0037 | Built |
| `paddock-mcp` spec update (2025-11-25 and later) | family | Not planned; raise separately |

## 5. Open decisions for the user (recommended default first)

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
| D14 | Timing | **After Pacenotes and Sighter; spikes S1 to S5 earlier only while load is low** | Start after Debrief 1.2 |
| D15 | Web search suggestions | **On by default, only after an engine keyword (`g term`)**, never for plain queries; a switch per engine | Off by default; suggestions for every query (rejected: sends keystrokes off the machine) |
| D16 | Family command registry | **A shared declaration in `paddock-siblings` that every app adopts** (catalogue G1) | Recce hard-codes each sibling's commands |

## 6. Risks and their owners

The top risks are listed in research section 6. Each ADR records its own risks. Two items need a check before stage 1 is
committed:
- whether Full Disk Access survives an update (S4);
- whether `outlook-pst` reads OST files (S2).
