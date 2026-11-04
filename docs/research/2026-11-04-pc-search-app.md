# A PC-wide search, launcher and answers app: competitive scan and architecture (2026-11-04)

Research note for a new Paddock app (working name **Recce**, section 9). Plan:
[plans/2026-11-04-pc-search-app.md](../plans/2026-11-04-pc-search-app.md). Status: research only. Nothing was built or
measured for this note (Dyno's release has the machine), so every performance figure below is either **cited** (with its
source and whether the source is the vendor) or **measured earlier in this repository** (with the file that records it).
Proposed budgets are targets, not results. Claims I could not check against a primary source are marked **(verify)**.
Sources were read on 2026-11-04 unless a date is given.

## The idea (user, 2026-11-04)

A PC-wide app to find things: "like X1 Search but powerful", "like Spotlight but powerful", "able to launch apps etc like
Alfred, support plugins". It has one shortcut and a search box, results as you type, a preview and actions. It indexes the whole
PC (all drives and network shares) and keeps the index current in the background. It reads documents, email (Outlook through
Graph or local PST/OST, IMAP, mbox, attachments) and SharePoint/OneDrive. It searches by meaning with local AI. It gives
answers with sources from a local model, and later serves MCP. It also launches apps, runs actions, works as a calculator,
keeps clipboard history and snippets, runs system commands, and takes plugins and workflows.

Family constraints that shape the answer:
- a separate app, not part of Pacenotes (per product), sharing engine pieces through crates (rule 42);
- AI local;
- network features on by default, each with its own switch;
- personal use;
- Rust core with Tauri and Angular on the shared shell (ADR 0012);
- Rust-only, out-of-process plugins over the lean IPC (ADR 0001; the user's plugin decision of 2026-10-20);
- long work per rules 10 to 14 and 51 (ER-17 to ER-20);
- logs per ER-27 with no user content;
- this app comes after Dyno, Spotter, Pitwall, Pitcrew, Debrief 1.2, Pacenotes and Sighter.

## Answer in short

- **No existing product covers the brief, and none is reusable as a base.**
  - X1 is the closest on sources: Outlook, PST, SharePoint, OneDrive and Teams, plus an MCP connector for Claude. It is
    closed, Windows-only and paid.
  - Spotlight (macOS 26) and Raycast are the closest on launcher feel. Raycast's AI is a paid cloud feature.
  - Everything is the reference for name search speed. It is closed freeware, and its fast path needs the NTFS journal
    with admin rights.
  - Recoll, DocFetcher and similar tools are content indexers without a launcher, meaning search or answers. Recoll is GPL.
  - What we can reuse are permissively licensed *libraries*, not products: Tantivy, pdfium-render, calamine, mail-parser,
    Microsoft's `outlook-pst`, Model2Vec, USearch, rmcp, global-hotkey, and possibly kreuzberg as an extraction toolkit.
- **Recommended architecture.** It mirrors Debrief, Pacenotes and Dyno, so most of it already exists in Paddock:
  - a per-user **agent** (Dyno's pattern) owns the index and the change feeds;
  - the **app** is a hidden, pre-created launcher window plus a main window on the shared shell;
  - **extraction runs in guarded worker processes**, because parsers face untrusted files;
  - storage:
    - **an in-memory name table** (Everything-like) for names, typed as you go;
    - **Tantivy** for content;
    - **a SQLite catalog** for the manifest, sources and state, as in Pacenotes ADR 0012 D4;
    - **Model2Vec through the planned `paddock-embed` (X55)** for meaning, with a flat or HNSW vector index;
  - **Pitwall** for answers and re-ranking, so the app never loads an LLM itself;
  - **`paddock-mcp`** for MCP;
  - three extension tiers, as in Dyno ADR 0007: built-in, TOML packs (quicklinks, snippets, workflows), and Rust
    out-of-process plugins (new sources and actions).
- **Hardest parts:**
  1. Change feeds without admin rights. Windows' journal needs admin, so a helper is needed, as with Dyno 1.1.
  2. macOS Full Disk Access on an ad-hoc signed app. The grant is tied to the signature, so it may need re-granting after
     each update (lessons.md, the Sighter note).
  3. Microsoft 365 sign-in for work tenants that block user consent.
  4. Keeping the index and cached email text private at rest.
- **Name: "Recce"**, the rally reconnaissance run that drives the route ahead and writes the notes. It pairs with Pacenotes.
  Alternatives: Marshal, Sweep (section 9).

## 1. Competitive scan

| Product | What it does well | Gaps against the brief | Licence and price | Reuse for us |
|---|---|---|---|---|
| **X1 Search** (Windows) | Type-ahead search over Outlook mail and attachments, local and archived PST, SharePoint, OneDrive, Teams, Slack, M365, Google Drive, Gmail, Box, Dropbox and file shares. Native-format preview. Actions from the results. "Customer-enabled AI models" for classifying. An **X1 Search Connector for Claude over MCP** ([x1.com product page](https://www.x1.com/products/x1-search/)). | Windows 10/11 only; "macOS and Windows on ARM are not currently supported". Needs .NET 4.8 and 8 GB of RAM (16 GB and a GPU for AI). No launcher, calculator, clipboard or plugins. | Proprietary. 14-day trial; single-user licence online, enterprise on request (same page). Price listings disagree: $79 a year ([TrustRadius](https://www.trustradius.com/products/x1-search/pricing)) versus $79 a month (G2) **(verify)**. | None (closed). It confirms the source list and that MCP access is a selling point. |
| **Spotlight** (macOS 26 Tahoe) | System-wide index; app actions through App Intents; Quick Keys; Shortcuts from the bar; **built-in clipboard history** (retention 30 min, 8 h or 7 days, or off, in 26.1) ([MacRumors, 2025-06-09](https://www.macrumors.com/2025/06/09/apple-supercharges-spotlight-in-macos-tahoe-with-quick-keys-and-more/), [MacRumors how-to](https://www.macrumors.com/how-to/do-more-with-spotlight-in-macos-tahoe/); secondary sources). | macOS only. No network-share content. Weak control over what is indexed and how it ranks. No Graph or IMAP connectors. No answers with sources from a local model of our choice. | Part of the OS. | Possibly as a *source* on macOS: query its index for files we do not index ourselves (`NSMetadataQuery`) **(verify that results come back fast enough to merge)**. |
| **Windows Search** | OS index with IFilters. On Copilot+ PCs, "semantic indexing models along with traditional lexical indexing", run on the NPU, offline, data kept local. Semantic indexing is on by default there ([The Register, 2025-01-20](https://www.theregister.com/2025/01/20/microsoft_unveils_windows_search_improvements/), [Windows Central](https://www.windowscentral.com/hardware/laptops/copilot-pcs-will-soon-get-their-first-must-have-exclusive-feature); secondary). | Semantic search needs a Copilot+ PC (Snapdragon first). The index is opaque. Poor support for network shares and other mail. No launcher actions beyond Start. | Part of the OS. | Possibly as a Windows *source* through its SQL query interface **(verify)**. Not a base. |
| **Everything** (voidtools) | Names only, near-instant. About 5 s to index 250k files and about 1 minute for 1M. About 35 MB RAM and under 14 MB disk for 250k files; about 100 MB RAM and 45 MB disk for 1M. Kept current through the **USN journal**, also across restarts. Network shares can be added as folders. `content:` exists but is unindexed and slow ([voidtools FAQ](https://www.voidtools.com/faq/); vendor figures). | Windows only. "NTFS indexing requires the Everything service or running Everything as administrator" (same FAQ). No content index, meaning search, mail or launcher. | Freeware, closed source. | The design reference for our name table and its budgets (section 5). |
| **Alfred** (macOS) | Launcher; workflows; clipboard history; snippets; file actions; Remote on iPad ([Powerpack page](https://www.alfredapp.com/powerpack/)). | macOS only. File content search relies on Spotlight **(verify)**. No meaning search or answers. | Proprietary. The Powerpack is £34 (v5) or £59 (Mega Supporter, lifetime upgrades) (same page). | Ideas only: keyword workflows and snippets with expansion. |
| **Raycast** (macOS; Windows; iOS) | Launcher with an extension store (thousands); clipboard history (3 months free); AI and cloud sync in Pro ([pricing](https://www.raycast.com/pricing)). Extensions in React and TypeScript, repo **MIT** ([raycast/extensions](https://github.com/raycast/extensions), GitHub API licence MIT). | AI is a paid credit system; the pricing page does not say it runs locally. Windows extensions are a subset (secondary). No whole-PC content index. | App proprietary; Free, or Pro at $10 a month / $96 a year. | Ideas only. Running Raycast extensions would need a Node runtime and their API, against the Rust-only plugin decision. |
| **PowerToys Command Palette** (Windows) | Microsoft's successor to PowerToys Run; extensions in any WinRT language ([Microsoft Learn](https://learn.microsoft.com/en-us/windows/powertoys/command-palette/overview)). | Windows only. No content index. The extension API is still before 1.0 (secondary). | **MIT** (PowerToys LICENSE, checked). | Code reference for Windows app enumeration and the launcher's look (MIT, so we may read and borrow with attribution). |
| **Flow Launcher** (Windows) | Open-source launcher with plugins; uses Everything for files. | Windows only; .NET; no content or meaning search. | **MIT** (LICENSE checked). | Code reference only. |
| **Recoll** (Linux, Windows, macOS) | Full-text desktop search on Xapian; many formats through helpers; mail. Index size is "often close to the data set size", sometimes much bigger (compressed mbox) ([Recoll manual](https://www.recoll.org/usermanual/webhelp/docs/RCL.INDEXING.STORAGE.html)). | Dated UI; no launcher, meaning search, Graph or answers. | **GPL** (v2+ per distro packages, [Mageia](https://madb.mageia.org/package/show/source/1/release/cauldron/name/recoll)); Xapian is GPL too **(verify)**. | Not reusable (GPL). The index-size data point is useful. |
| **DocFetcher** (Win/Linux/macOS) | Portable full-text search; Office, PDF, EPUB, HTML, RTF and **PST**; a daemon notes changes while the app is closed ([site](https://docfetcher.sourceforge.io/)). | Java; no launcher, meaning search or connectors. Pro is commercial. | Eclipse Public License (site). The GitHub mirror says Apache-2.0 and was last pushed 2018-07-22 (GitHub API). | Not reused (weak copyleft, Java, stale mirror). |
| **Copernic Desktop Search** (Windows) | Files, Outlook, network drives; plug-ins for Outlook and Explorer. Outlook/PST needs a local Outlook or PST files ([pricing page](https://copernic.com/desktop/pricing/)). | Windows only. Reviewers call indexing heavy ([Capterra](https://www.capterra.com/p/9020/Copernic-Desktop-Search/); user reviews). | Proprietary, subscription (prices vary by listing) **(verify)**. | None. |
| **Khoj** | Local or cloud "AI second brain" with RAG over your notes. | Not a PC-wide indexer or launcher. | **AGPL-3.0** (GitHub API). | Not reusable (AGPL). |
| **AnythingLLM** | Desktop RAG workspace over documents with local models. | Manual "workspaces", not a live PC index; no launcher. | **MIT** (GitHub API). | Ideas for the answer UI (citations per answer). |
| **Others checked** | Wox (now **GPL-3.0**), FSearch (**GPL-2.0**), Ulauncher and Albert (licence not stated in the API; GPL per their sites **(verify)**), ripgrep-all (licence not stated in the API; AGPL per its README **(verify)**). | | | Not reusable (copyleft). |

**What nobody does, which is our case:**
- one app across macOS, Windows and Linux;
- names, content, mail and M365 in one index;
- meaning search and cited answers both from **local** models;
- MCP for an agent;
- a launcher with Rust plugins;
- privacy defaults a person can check.

## 2. Architecture options and recommendation

### 2.1 Process model (recommended)

```
Recce app (Tauri, shared shell)                  recce-agent (per user, login item, paddock-service)
├─ launcher window: pre-created, hidden,         ├─ name table (in memory, mmap snapshot)
│  shown on the hotkey                           ├─ catalog.sqlite (sources, manifest, cursors, state)
├─ main window: sources, settings, jobs,         ├─ content index (Tantivy) + vectors (flat / HNSW)
│  answers                                       ├─ change feeds (FSEvents / USN via helper / fanotify / watch)
└─ talks to the agent over paddock-ipc ───────▶  ├─ connectors (Graph, IMAP) through paddock-net Egress
                                                 ├─ extract workers (guarded, memory cap, per-file budget)
                                                 ├─ embed worker (paddock-embed, Model2Vec)
                                                 ├─ MCP server (paddock-mcp, off until enabled)
                                                 └─ plugin host (paddock-plugin-host)
optional: recce-helper (Windows: MFT/USN read-only; Dyno ADR 0022/0027 pattern)
optional: Pitwall (answers, re-rank, deep embeddings) over paddock-pitwall
```

Why an agent and not the app process:
- the index must stay current while no window is open;
- a launcher must answer within a frame, so the warm index lives in a process that is always running;
- Dyno already proves the pattern: agent, login item, supervisor (Dyno ADRs 0010 and 0025).

Why workers for extraction: PDF, Office and PST parsers run on untrusted bytes. The family pattern is crash containment, a
memory cap and quarantine (`paddock-guard`, `paddock-proc` supervised children, the plugin memory research of
2026-10-27). The launcher window is created hidden and shown on the hotkey, which fits ADR 0026 (windows created hidden,
shown at a ready handshake) and ADR 0029 (the shell owns the start).

### 2.2 Change feeds

| OS | Feed | Rights | Across restarts | Notes and sources |
|---|---|---|---|---|
| Windows (NTFS/ReFS) | **USN change journal** (`FSCTL_QUERY_USN_JOURNAL`, `FSCTL_READ_USN_JOURNAL`, `FSCTL_ENUM_USN_DATA` for a fast first listing) | "To perform this and all other change journal operations, you must have system administrator privileges" ([Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/fileio/using-the-change-journal-identifier)). | Yes: store journal id + USN; a new journal id means a full rescan (same page). | Not over SMB ("SMB 3.0: No", [FSCTL_READ_USN_JOURNAL](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_read_usn_journal)). An "unprivileged" read mode exists in one .NET library and "file names may not be available" in it ([Meziantou, 2024-12-23](https://www.meziantou.net/reading-usn-journal-with-dotnet.htm)). The control code and Windows version are not documented there **(verify before relying on it)**. Rust: `usn-journal-rs` 0.5.0 (MIT, young, 19k downloads), `mft` 0.9.2 (MIT/Apache), `ntfs` 0.4.0 (MIT/Apache, last release 2023) (crates.io). Dyno ADR 0029 already plans the journal through its helper (WP-8). |
| Windows (no helper) | `ReadDirectoryChangesW` (through `notify`/`paddock-watch`) plus the manifest walk of Pacenotes ADR 0012 D1 (`FileIdExtdDirectoryInfo` per directory) | user | No (the manifest catches up by stat compare) | RDCW drops events on overflow (Dyno ADR 0029 table). |
| macOS | **FSEvents** per device with `sinceWhen` | user, but TCC: protected folders need Full Disk Access **(verify per folder)** | Yes: "persistence across reboots"; check `FSEventsCopyUUIDForDevice`; events are "advisory"; `MustScanSubDirs` and dropped events mean a rescan ([Apple, FSEvents guide](https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/FSEvents_ProgGuide/UsingtheFSEventsFramework/UsingtheFSEventsFramework.html)) | Directory granularity by default, so stat-compare that directory's entries against the manifest. |
| Linux | **fanotify** with `FAN_REPORT_DFID_NAME` (5.9+) or inotify | Unprivileged fanotify since 5.13 can only mark inodes; `FAN_MARK_FILESYSTEM`/`MOUNT` need `CAP_SYS_ADMIN` ([fanotify_init(2)](https://man7.org/linux/man-pages/man2/fanotify_init.2.html)) | No persistent journal (Dyno ADR 0029: "full scan only") | inotify needs one watch per folder. A whole-filesystem mark needs a privileged helper. |
| Network shares (all) | No journal; manifest rescans on a schedule, plus watches where the server supports them | user | Manifest | Pacenotes ADR 0012 D1: remote and FAT volumes are "always check content"; never read cloud placeholders (`RECALL_ON_DATA_ACCESS`). |

**Recommendation.**
- **Core:** one `ChangeFeed` port with a persisted cursor, and the manifest walk as the always-correct fallback.
- **macOS:** FSEvents history first.
- **Windows:** the journal through an optional read-only helper. Without the helper, RDCW plus the manifest.
- **Linux:** inotify for chosen roots plus scheduled manifest walks.
- **Shared:** this belongs in `paddock-watch`, which Dyno (saved disk maps) and Pacenotes (watcher plus manifest) both need
  (rule 55: one implementation).

### 2.3 Text extraction (licences checked on crates.io and GitHub, 2026-11-04)

| Format | Candidate | Licence | Notes |
|---|---|---|---|
| PDF | `pdfium-render` 0.9.5 + PDFium binaries (bblanchon/pdfium-binaries) | MIT OR Apache-2.0. PDFium itself is BSD-3/Apache **(verify)**. | Best text fidelity. PDFium "should be assumed _not_ to be thread safe"; the crate serialises calls with a mutex, so run parallelism as **one PDFium per worker process** ([README](https://github.com/ajrcarey/pdfium-render)). Native library per OS: ship it pinned through `paddock-runtime`, or bundle it. |
| PDF (pure Rust) | `pdf_oxide` 0.3.78; `lopdf` 0.47.0; `pdf-extract` 0.12.1 | MIT OR Apache-2.0; MIT; MIT | No native library. Quality unmeasured: a spike on a synthetic corpus must compare them (rule 48). |
| OOXML (docx, pptx, xlsx) | `zip` + `quick-xml` through Pacenotes' safe XML (`pacenotes-xml`, ADR 0010) and `paddock-archive` limits; `calamine` 0.38.0 for spreadsheets | MIT (calamine) | Pacenotes ADR 0009 already reads docx/xlsx safely (entry, size and ratio limits). Share, don't copy (rule 55). |
| Legacy .doc/.xls/.ppt | `calamine` (xls); `cfb` for the OLE container | MIT | Word 97 binary text needs a parser. kreuzberg has one **(verify quality)**. |
| Email: eml, mbox | `mail-parser` 1.0.1 | Apache-2.0 OR MIT | RFC 5322 and MIME, 41 charsets, an mbox reader, nested messages, "every function … fuzzed" ([README](https://github.com/stalwartlabs/mail-parser)). |
| Email: PST | Microsoft `outlook-pst` 1.2.0 ([outlook-pst-rs](https://github.com/microsoft/outlook-pst-rs)) | MIT | A "clean room" implementation of MS-PST, compatible with `mspst32.dll`. The README does not state **OST** support **(verify; OST is the cached-mode file and may differ)**. |
| Email: msg | `msg_parser` 0.3.6 | MIT | Small; quality unknown. |
| Everything above in one | **kreuzberg** 4.10.4 (Rust core; PDF via its pdfium fork, Office, `outlook-pst`, `mail-parser`, `calamine`, `lopdf`, `cfb`, optional OCR with Tesseract, Paddle or ONNX) | MIT (crate and repo). Dependencies listed via the crates.io API. | Attractive as a library *inside our worker*, with features trimmed. Risks: a large tree (tokio, ort, axum behind features), a fast release cadence, and our limits and redaction must wrap it. Decide by a spike: binary size, licences (`cargo deny`), quality and memory per file. |
| Everything (Tika) | `extractous` 0.3.0 (Tika compiled with GraalVM) | Apache-2.0 | **Last push 2024-12-21** (GitHub API): stale. Not recommended. |
| OCR | Platform OCR (Apple Vision; `Windows.Media.Ocr`) **(verify APIs)**; `ocrs` 0.13.1 (MIT/Apache) as the portable fallback | | Expensive: off by default, a confirmed bulk job (rules 46 and 51). |

Excluded: **MuPDF** (AGPL **(verify current licence)**), **Xapian** and **Recoll** filters (GPL), and **evalexpr** for the
calculator (now **AGPL-3.0-only**, crates.io); `fend-core` (MIT) or `numbat` (MIT/Apache) are the calculator candidates.

### 2.4 Index: names, content and meaning

| Need | Option | For | Against | Choice |
|---|---|---|---|---|
| **File names, typed as you go** | Own in-memory table: interned name parts, parent ids, sizes, times, mmap snapshot; matching through `paddock-names` (rule 20) | Everything shows the shape works: about 100 MB RAM for 1M entries (vendor). No query parser per keystroke. | We build it. | **Own table**, plus a spike (section 5). |
| | Tantivy for names | Reuses the content engine. | A heavier per-keystroke path; prefix and substring need n-gram fields. | Fallback. |
| **Content** | **Tantivy** 0.27 | MIT; BM25 "the same as Lucene"; phrase queries; incremental indexing; mmap; "<10ms" startup; claims about 2x Lucene's speed in its own benchmark ([repo](https://github.com/quickwit-oss/tantivy)). Debrief uses it already (`debrief-store-duck`, 0.27). | No encryption; segment merges cost I/O. | **Tantivy.** |
| | SQLite FTS5 | Public domain; trigram tokenizer for substring/LIKE; external-content tables; `bm25()` ([sqlite.org/fts5](https://www.sqlite.org/fts5.html)). Pacenotes uses SQLite. | Slower ranking at millions of documents **(verify; no measurement in-repo)**. | Use for small tables (clipboard, snippets), not the main index. |
| | DuckDB FTS | BM25; stemmers. "By default, the FTS index will not update automatically", but `incremental = 1` exists ([DuckDB 2.0 docs](https://duckdb.org/docs/2.0/core_extensions/full_text_search.html)). | Built for analytics; Pacenotes ADR 0012 D8 keeps DuckDB "for export only". | No. |
| **Catalog** (sources, manifest, cursors, jobs) | SQLite (`rusqlite` 0.41, bundled) | As Pacenotes ADR 0012 D4 and Dyno's `cache.db`. | | **SQLite.** |
| **Embeddings** | **`paddock-embed` (planned X55)** with Model2Vec | Measured here: potion-base-8M 9,581 chunks/s on 1 thread, potion-code-16M-v2 6,365, potion-retrieval-32M 8,001, peak RSS 131 to 357 MB; fastembed's BGE peaked at 2.6 to 37 GB ([Pacenotes ADR 0001](../../apps/pacenotes/docs/adr/0001-embeddings.md)). Weights MIT. `potion-multilingual-128M` (MIT, 512 MB safetensors, HF API) covers mail in other languages. | Static vectors lose word order and negation (shared-next X55). | **Model2Vec** by default; **Qwen3-Embedding-0.6B** (Apache-2.0, 639 MB Q8_0 GGUF, HF API) through Pitwall re-ranks a query's top 50. A deep re-embed only as a confirmed job (rule 46). |
| **Vector store** | Flat (f16 or int8, mmap) brute-force cosine | Simple and exact. 256 dims in f16 is 512 B a chunk: **51 MB per 100k chunks** (arithmetic). | Linear cost: about 1M chunks before latency hurts **(estimate; measure)**. | Up to about 1M chunks. |
| | **USearch** 2.26.6 (HNSW) | Apache-2.0; mmap; active (pushed 2026-10-31). | C++ core (build per OS). | Beyond 1M chunks, after a spike. |
| | `sqlite-vec` 0.1.9 | MIT/Apache; inside the catalog. | Brute force; last release 2026-05. | No. |
| | LanceDB 0.41 | Apache-2.0. | Heavy (Arrow/DataFusion) for one app. | No. |
| | `hnsw_rs` 0.3.5 | Pure Rust, MIT/Apache. | Less used. | Alternative to USearch. |
| **Fusion** | RRF (k = 60) of BM25 and cosine top lists, as in Pacenotes ADR 0001 and X55 | | | **RRF**, weights tuned on a stored query set. |

Rule 46 holds throughout. Indexing uses deterministic extraction, BM25 and static embeddings. An LLM is used only when a
person asks (an answer, or a re-rank of one query's top 50), never once per item.

### 2.5 Connectors

**Microsoft 365 (Graph).**
- **Auth.**
  - A desktop app is a *public client* and "can't have client secrets" ([MSAL client types](https://learn.microsoft.com/en-us/entra/identity-platform/msal-client-applications)).
  - Use the authorization code flow with PKCE and a loopback redirect. For loopback, "the port component … is ignored",
    and Microsoft says "prefer 127.0.0.1 over localhost" ([redirect URIs](https://learn.microsoft.com/en-us/entra/identity-platform/reply-url)).
  - Rust: `oauth2` 5.0.0 (MIT/Apache). There is no official Rust MSAL **(verify)**.
  - Tokens go in the OS keychain through `paddock-secrets` (scope `graph:<account>`). HTTP goes through `paddock-net` Egress,
    with `graph.microsoft.com` and `login.microsoftonline.com` on the allow-list.
- **Consent risk.** By default users can consent to "permissions that don't require administrator consent", but admins
  can disable user consent or limit it to verified publishers
  ([Entra user consent](https://learn.microsoft.com/en-us/entra/identity/enterprise-apps/configure-user-consent)).
  - A personal, unverified app registration may be blocked in a work tenant.
  - The fallback is "bring your own client id": the user registers the app in their tenant, or asks IT to.
  - Personal Microsoft accounts work with delegated `Files.Read` and `Mail.Read`.
- **Incremental sync.** Delta query covers `driveItem`, `listItem`, `message`, `mailFolder` and `event`, with an
  `@odata.deltaLink`. `410 Gone` means a full resync. Outlook delta tokens expire with an internal cache ("upper limit
  isn't fixed"); expiry surfaces as `syncStateNotFound` ([delta overview](https://learn.microsoft.com/en-us/graph/delta-query-overview)).
  - For drives: `token=latest` exists, the same item can appear more than once ("use the last occurrence"), and items are
    tracked "by id" because the path is not returned ([driveItem: delta](https://learn.microsoft.com/en-us/graph/api/driveitem-delta?view=graph-rest-1.0)).
  - Least privilege for drives is delegated `Files.Read`, or `Sites.Read.All` for SharePoint sites (same page).
- **Throttling.** On `429`, honour `Retry-After`; otherwise use exponential back-off. Prefer delta to polling
  ([throttling](https://learn.microsoft.com/en-us/graph/throttling)). The family back-off is in `paddock-jobs::time`.
- **Permission trimming.** We index with the user's *delegated* rights, so the index never holds what the user cannot read
  at sync time. Lost access shows up in the next delta, or as a 403/404 when the user opens a result; we then drop the item.
  - Tracking sharing changes precisely through `Prefer: deltashowsharingchanges` needs **`Sites.FullControl.All`** (same
    page). That is far beyond least privilege, so we do **not** use it.
  - Instead the result list re-checks remote items on open, and a "Recheck access" job runs weekly.
- **Content.** Download a file's content only for formats we extract, within a size cap, and hash while writing (rule 15).
  Never download whole drives' binaries.

**IMAP.**
- `async-imap` 0.12 (MIT/Apache; needs an async runtime), or a small blocking client.
- Incremental sync with UIDVALIDITY plus UID, and CONDSTORE/QRESYNC where the server offers them (RFC 7162 **(verify server
  support per provider)**). OAuth (XOAUTH2) for Gmail and Outlook.com **(verify)**.
- Passwords and app passwords go in `paddock-secrets`.

**Local mail.**
- **mbox and eml:** `mail-parser`.
- **PST:** `outlook-pst`.
- **OST:** unknown **(verify)**.
- **Apple Mail:** `~/Library/Mail` `.emlx` files, behind Full Disk Access **(verify the format details)**.
- **Thunderbird:** mbox or maildir.
- The "new Outlook" for Windows keeps no PST cache **(verify)**, so Graph is the default for M365 mail.

### 2.6 Local model for answers (RAG)

- **Engine:** Pitwall (llama.cpp only, Pitwall ADR 0020), through `paddock-pitwall` (`chat`, `embeddings` with bounded
  answers, `client.rs`). The app never loads a model itself. Debrief ADR 0059 makes the same choice.
- **Model sizes** (HF API, 2026-11-04):
  - Qwen3-4B-Instruct-2507: Apache-2.0, **2.5 GB** Q4_K_M, 4.3 GB Q8_0;
  - Qwen3-1.7B: 1.8 GB Q8_0;
  - IBM Granite 4.0 micro: Apache-2.0, size unmeasured.
  - Plan for about **file size + 0.5 to 1.5 GB** of KV cache at 8k to 16k context **(estimate; Pitwall's memory policy,
    ADR 0006, owns the real numbers)**.
  - Default preset `recce-answer` (Pitwall ADR 0014 aliases), so the user picks the model.
- **Grounding:**
  1. Hybrid retrieval fetches the top 8 to 20 chunks, within a token budget.
  2. The prompt numbers the sources.
  3. The answer must cite `[n]`.
  4. A **citation check in Rust** drops any `[n]` not in the context and flags sentences without one.
  5. The UI shows each source with "Open" and the matched passage.
- **Rules:** rule 46 (a user-started, bounded request); rule 12 ("Still working", never a UI timeout); Cancel works.
- **Without Pitwall:** search and meaning search still work. Answers show "Pitwall is not running" with a working Start
  (rule 19).

### 2.7 MCP server

- `paddock-mcp` (ADR 0007): local only, read-only by default, off until enabled, schema-checked arguments, result caps,
  paging and an audit log.
- **Tools:** `search` (query, kinds, sources, limit ≤ 50), `get_passage` (result id, a bounded window of text), `list_sources`
  (names and kinds, no paths unless the user allows them), and `answer` (only if the user also enables answers over MCP).
- The spec has moved on from `paddock-mcp`'s 2025-06-18 shapes:
  - 2025-11-25 added icons, URL-mode elicitation and experimental tasks ([changelog](https://modelcontextprotocol.io/specification/2025-11-25/changelog));
  - a 2026-07-28 revision is reported, deprecating HTTP+SSE (secondary: [releases.sh](https://releases.sh/modelcontextprotocol/mcp-specification)) **(verify)**.
- That spec update is `paddock-mcp`'s job, not this app's. The official Rust SDK `rmcp` 3.8.0 (Apache-2.0) is the
  reference, should Paddock adopt it.

### 2.8 Launcher and global hotkey

- **Hotkey:**
  - `global-hotkey` 0.9.0 (Apache-2.0 OR MIT; the crate behind Tauri's global-shortcut plugin) on macOS, Windows and X11.
  - **Wayland** has no global key grab. The xdg-desktop-portal *GlobalShortcuts* interface is implemented by GNOME 48+,
    KDE Plasma 6+ and Hyprland (secondary: [OBS draft PR](https://obs-versions.com/community/pulls/13680),
    [global-shortcut-portal](https://pypi.org/project/global-shortcut-portal/)) **(verify against the portal spec)**.
  - Fallback: the user binds a desktop shortcut to `recce --toggle`.
- **App discovery:**
  - macOS: application bundles in the standard folders, or Spotlight's application query **(verify)**;
  - Windows: Start Menu `.lnk` files plus the AppsFolder shell namespace for packaged apps **(verify)**;
  - Linux: freedesktop `.desktop` files.
  - Each OS is an adapter behind an `AppCatalog` port; launching goes through `paddock-proc` (no console window) or the
    OS's open call.
- **Calculator:** `fend-core` (MIT; units and number bases). Currency rates are a network feature with its own switch.
- **Clipboard history.** `arboard` 3.7 (MIT/Apache) reads and writes. Clipboard history is the riskiest feature for privacy:
  - Off by default: macOS 26 already has it natively, and secrets pass through the clipboard.
  - Honour the "concealed" and "transient" markers that password managers set **(verify the per-OS markers)**.
  - Exclude chosen apps; keep it in the encrypted store (section 3).
  - macOS has a pasteboard privacy alert for programmatic reads, previewed in 15.4; whether it shipped on by default in 26 is
    unclear (secondary: [mjtsai](https://mjtsai.com/blog/2025/05/12/pasteboard-privacy-preview-in-macos-15-4)) **(verify)**.
  - Watching the clipboard on macOS means polling `changeCount` **(verify)**, against ADR 0024 "wake-ups instead of polls".
    It needs a declared, cheap poll (a rule 52 live-work exception) or stays off.
- **Snippets and expansion:** snippets inserted from the launcher are simple. *Typed* text expansion needs a system-wide
  keyboard hook (Accessibility on macOS) and is out of scope for v1.
- **System commands** (lock, sleep, empty trash, restart): each is an `Action` with a confirmation where it is destructive.

### 2.9 Plugins and workflows

The family decision is Rust-only, out of process, over the lean IPC. ADR 0001 (`paddock-plugin-api`, `-host`, `-package`,
`-test`, signed `.paddockplugin`) and the user's decision of 2026-10-20 (backlog memory) both record it. Apply Dyno's three
tiers (Dyno ADR 0007):

1. **Built-in.** Files, apps, content, mail, M365, calculator, clipboard, snippets and system commands.
2. **TOML packs (no code)**, parsed once with limits through `paddock-packs` (rule 21):
   - quicklinks and web searches (URL templates, opened in the default browser);
   - snippets;
   - **workflows** as declared steps: a keyword, then argument prompts, then steps from a closed set (open URL, open
     file, launch app with arguments, copy text, notify);
   - a "run command" step is allowed only for packs in the user's own folder, after a one-time confirmation that names the
     command.
3. **Rust plugins** for new *sources* (for example Jira, Confluence or Obsidian vaults), *result providers* and *actions*.
   - Two new contribution kinds in `paddock-plugin-api`: `SearchSource` (a stream of documents with a cursor) and
     `ResultProvider` (answer a query within a time budget, for example 50 ms, or be skipped).
   - Guarded, quarantined, with a memory cap (ADR 0001, research 2026-10-27).

Not offered: Raycast or Alfred plugin compatibility (Node or AppleScript runtimes), and in-process dynamic libraries.

### 2.10 Paddock crates: reuse, extend, new

| Crate | Use |
|---|---|
| **Reuse as is** | `paddock-shell` and the shell crates (ADR 0012), `paddock-jobs` (jobs, phases, executor ADR 0020, `time`), `paddock-ipc`, `paddock-service` (agent login item), `paddock-proc`, `paddock-guard`, `paddock-secrets`, `paddock-net`, `paddock-log`, `paddock-diag`, `paddock-env`, `paddock-settings`, `paddock-packs`, `paddock-names` (identifier matching for names), `paddock-pitwall`, `paddock-mcp`, `paddock-plugin-*`, `paddock-files`, `paddock-siblings` (hand-off to Debrief and Pacenotes), `paddock-archive` (zip limits), `paddock-bench` (budgets), `paddock-storage` (clear caches), `paddock-window` (tray, single instance) |
| **Extend (shared change)** | `paddock-repo-files` → `paddock-walk` with stat fields and Windows enumeration (Pacenotes ADR 0012 IG14; we are its second user); `paddock-watch` → a `ChangeFeed` port with persistent cursors (FSEvents history, USN via a helper, fanotify) shared with Dyno ADR 0029 WP-8 and Pacenotes; `paddock-plugin-api` → `SearchSource` and `ResultProvider` contributions |
| **Planned, consume** | `paddock-embed` (X55: Model2Vec worker, Pitwall adapter, cosine top-k, RRF, index record). We would be its third consumer after Debrief and Pacenotes. |
| **New shared** | `paddock-extract`: text extraction ports and the guarded worker protocol, with per-format adapters (OOXML via the safe-XML and zip limits now in Pacenotes ADR 0009/0010, PDF, mail). A second user (Pacenotes requirement importers, Debrief attachments) makes it shared, not app code. `paddock-mail`: eml, mbox, PST and IMAP readers to one `Message` type, only if a second app needs mail (Debrief importing mail logs is a candidate); otherwise app crate `recce-mail` first. `paddock-graph`: Graph auth (PKCE, loopback), delta cursor, throttling over `paddock-net`. App crate first, shared on a second user (rule of three, PRINCIPLES). `paddock-helper` (later): extract Dyno's privileged helper pattern (install, wire, caller check) when a second app (this one) needs it, per ADR 0034 copies. |
| **App crates** | `recce-core` (ids, kinds, query model, ranking policy, pure), `recce-names` (name table), `recce-index` (Tantivy schema, vector store), `recce-agent`, `recce-launcher` (apps, calculator, clipboard, snippets, system actions), `recce-graph`, `recce-mail`, `recce-answer` (retrieval, prompt, citation check) |

## 3. Privacy and security

- **What is sensitive:** names, paths, document text, mail, and M365 content. All of it is the user's data under ER-27 and
  `paddock-log`. Logs carry ids, counts, kinds, durations and outcomes, never names, paths, queries or text. Dyno ADR 0029
  §6 has the test pattern: invented distinctive names, then assert they never appear in the log.
- **Encryption at rest:**
  - **(a)** Check full-disk encryption (FileVault, BitLocker, LUKS) at first run and say plainly when it is off. Tantivy's
    term dictionary *is* content.
  - **(b)** Store **no full text of local files**. The index holds terms and positions, and passages are re-read from the
    file when shown.
  - **(c)** Remote content (mail bodies, M365 text, clipboard) that cannot be re-read offline is stored **encrypted**, with
    a key in the keychain (`paddock-secrets`): SQLCipher (`rusqlite` `bundled-sqlcipher`, BSD-style **(verify licence)**)
    or AES-GCM blobs (RustCrypto, MIT/Apache).
  - **(d)** An encrypted Tantivy `Directory` is possible in principle (the trait exists) but unmeasured **(spike only if the
    user wants (d))**.
  - Index files are `0600`, live in the cache folder (out of backups, as Dyno's `cache.db`), and "Forget a source" deletes
    them outright with `secure_delete` (Dyno ADR 0029 §6).
- **Exclusions:**
  - **Built-in exclusion pack:** system and package folders; caches; `~/.ssh`, `~/.gnupg`; keychains and password-manager
    vaults; browser profiles; `.env` and key-file patterns.
  - **User settings:** per-folder and per-pattern exclusions.
  - **OS markers:** Windows' `FILE_ATTRIBUTE_NOT_CONTENT_INDEXED` and macOS `.metadata_never_index` **(verify both)**.
  - **Names only:** content indexing can be off per source while names stay on.
- **Source permissions:** we index only what the user's account can read, and never escalate:
  - local: the user's rights;
  - Graph: delegated rights;
  - helper: names and change records only, never file contents.
  - The Windows helper (if built) exposes **read-only** requests for the journal and the MFT listing. Callers are checked as
    in Dyno ADR 0022.
- **What MCP may expose:**
  - off by default;
  - per-source "visible to MCP" switches (mail and M365 off by default);
  - no file contents beyond a bounded passage per call;
  - paths shown only if the user allows them;
  - the audit log lists every call (tool, counts, never the text) and is visible in the app;
  - no write or launch tools.
  - Note: an answer the LLM makes from MCP results leaves the machine if the *client* is a cloud model such as Claude. The
    MCP switch must say so (the user's "AI local" rule covers our models, not a client's).
- **Network:**
  - all through `paddock-net` Egress with a host allow-list;
  - each connector has its own switch, on by default once the user adds the account (family rule: user-added remote sources
    start only when the user adds them);
  - the first-run note lists hosts.
- **Plugins:** default-deny grants and the `sandbox_for` mapping (ADR 0001). A plugin source's documents carry its id, so
  "Forget plugin" removes them.

## 4. Parallelism and cost (rule 51, ER-20)

| Work | Kind | Bound (Auto default, a Setting) | Cheaper operation |
|---|---|---|---|
| First name listing | metadata I/O | SSD 4 to 8 directory readers, HDD 1 to 2, network 2 per host | Windows with the helper: `FSCTL_ENUM_USN_DATA` lists a volume without a tree walk (how Everything is fast, **(verify)**); otherwise one directory read per folder with stat fields (Pacenotes D1) |
| Change catch-up | metadata I/O | 1 per volume | Journal or FSEvents since the cursor, so only changed folders are stat-compared |
| Extraction | CPU plus I/O; untrusted | cores − 1 worker processes, each with a memory cap; PDFium one per process | Skip unchanged by the manifest (size, mtime, ctime, file id, then content hash, Pacenotes D1/D3); skip by type and size caps; extract the first N MB of huge files |
| Embedding | CPU | cores − 1 threads in the embed worker (55k chunks/s on 13 threads measured for potion-code, Pacenotes ADR 0001) | Static model; chunks deduplicated by hash; re-embed only changed chunks |
| Tantivy writes | CPU plus I/O | one writer, memory budget 64 MB (Debrief's figure) | Batch commits; merge policy tuned for many small updates |
| Graph and IMAP sync | network | 2 to 4 requests per host | Delta and CONDSTORE; content download only for extractable types under a cap |
| Answers | GPU/CPU in Pitwall | Pitwall's queue | Retrieval narrows the context to the top 8 to 20 chunks |

- All bulk work runs at `QOS_CLASS_UTILITY`, nice 10 or `THREAD_PRIORITY_BELOW_NORMAL`. Launcher queries are live work at
  normal level (rule 52).
- Jobs have phases, growing counts and Cancel (rules 10 to 12), and build new data before swapping it in.

## 5. Performance budgets (targets, to be confirmed by spikes and recorded per rule 48)

| Measure | Target | Basis |
|---|---|---|
| Hotkey to launcher painted | ≤ 50 ms p95 | Window pre-created and hidden (ADR 0026 pattern); **unmeasured** |
| Name results per keystroke, 2M entries | ≤ 20 ms p95 | Everything's in-memory model (vendor); **spike S1 measures our table** |
| Content query (BM25) | ≤ 100 ms p95 at 1M documents | Tantivy design; **unmeasured here** |
| Hybrid (BM25 + vectors + RRF) | ≤ 200 ms p95 | Model2Vec query embedding is one chunk (about 0.15 ms at 6,365 chunks/s, Pacenotes ADR 0001); flat cosine over 1M × 256 is **unmeasured** |
| Answer: first token | ≤ 3 s with Pitwall warm | Pitwall's figures; **unmeasured** |
| First name index, 1M files | ≤ 60 s on SSD | Everything: about 1 minute for 1M (vendor); ours by directory walk is likely slower without the helper **(spike)** |
| First content index | Background, resumable, no budget on total time; throughput target set after spike S2 | Depends on the corpus |
| Idle CPU (agent) | ≈ 0 % (event-driven, ADR 0024) | Rule 16 budget; measure with `paddock-metrics` |
| Idle RAM | agent ≤ 150 MB with 1M names and content index closed; launcher webview measured separately | Everything about 100 MB per 1M (vendor); Model2Vec worker 131 to 357 MB **while embedding**, so the worker exits when idle |
| Index size per 100k files | names ≈ 5 to 10 MB; content: often comparable to the extracted text (Recoll's experience, vendor docs); vectors 51 MB per 100k chunks at 256-dim f16 | Everything 45 MB per 1M (vendor); arithmetic |

## 6. Risks

1. **Windows change feed needs admin.** Without the helper, freshness depends on RDCW plus rescans. Mitigation: the helper
   (Dyno pattern), with a clear "names may lag" status without it.
2. **macOS Full Disk Access with ad-hoc signing.** Grants are tied to the signature, and an ad-hoc re-sign on update changes
   it (lessons.md, "macOS permission grants…"). Without the grant, Mail, Messages and parts of `~/Library` are invisible.
   Mitigation: a Repair button that opens the right pane (rule 19), plus a status that lists what is unreadable. **Verify
   in the first spike whether FDA survives an update.**
3. **M365 consent in work tenants.** Mitigation: BYO client id and a clear message.
4. **Parser crashes and memory on hostile files.** Mitigation: workers, caps, quarantine per file hash.
5. **PST/OST coverage** of `outlook-pst` **(verify OST)**.
6. **Scope.** The brief is a multi-product suite (X1 + Spotlight + Alfred + an answer engine). Staging keeps each stage
   useful on its own.
7. **Wayland hotkey** depends on the desktop's portal.
8. **Clipboard privacy.** It stays off by default.

## 7. Decisions for the user (defaults recommended)

See the plan's decision table. In short:
- the name "Recce";
- an agent plus a Windows helper (helper in stage 2);
- Tantivy plus the own name table;
- Model2Vec default with Pitwall re-rank;
- store no local file text, and encrypt remote content;
- clipboard history off by default;
- Graph with a family app registration plus a BYO client id;
- MCP off, with mail and M365 hidden from it by default;
- plugins in three tiers;
- start after Pacenotes and Sighter, with spikes allowed earlier only when load allows.

## 8. What was not done

No builds, no measurements, no VM tests (Dyno's release has the machine). Spikes S1 to S5 in the plan are the first work
when this app starts.

## 9. Name

The family names come from motorsport roles and tools: Debrief, Dyno, Pacenotes, Pitcrew, Pitwall, Sighter and Spotter.

- **Recce** (recommended). In rallying, the recce is the reconnaissance run in which the crew drives the route ahead and
  writes the pacenotes. That is exactly "go and find what's out there", and it pairs with Pacenotes. It is short and easy to
  type in a launcher. Other products use the word (a film-recommendation app, a cycling-route app, Airbus RECCE Touch, a
  Rust crate `recce` for web reconnaissance; [search](https://lib.rs/crates/recce)), but no desktop search tool does as
  far as I found **(verify trademark before any public release)**.
- **Marshal**: the trackside marshals see the whole circuit and flag what matters. Generic word.
- **Sweep**: the sweep car runs the whole stage after the field, checking that nothing is left behind (a thorough index).
  Could read as "cleanup", which overlaps Dyno.
