# Recce: feature catalogue from what users love and miss in search tools and launchers (2026-11-04)

Research note for the planned PC-wide search, launcher and answers app (working name **Recce**). It adds to the competitive
scan and architecture in [2026-11-04-pc-search-app.md](2026-11-04-pc-search-app.md); the plan is
[plans/2026-11-04-pc-search-app.md](../plans/2026-11-04-pc-search-app.md), whose stages this note uses and changes
(section 5). Status: research only. Nothing was built or measured.

**How to read the evidence.** Sources were read on 2026-11-04. Each claim names its source and, where the page gives one,
its date. Vendor pages and docs are marked *vendor*; reviews, forums and news are *secondary*; aggregator pages of unclear
independence are *weak* and only support a claim another source also makes. Claims I could not confirm are marked
**(verify)**.

**The brief (user, 2026-11-04):** "like Spotlight / X1 Search but powerful", "able to launch apps etc like Alfred, support
plugins"; email, documents and SharePoint sites "etc."; answers with sources from a **local** model (RAG); MCP later;
personal use.

## Answer in short

- **What users love, across all products:** results as you type that forgive typos; one action panel on every result;
  saved searches and filter syntax (Everything); quicklinks with placeholders (Raycast, Command Palette); clipboard history
  that ignores password managers (Alfred); source breadth with native previews and hit highlighting (X1); answers that
  show their sources (Glean); local models (Raycast added Ollama in 1.99, 2025-05-21).
- **What users miss or hate:** missing wildcards (X1), heavy or crashing indexers (Copernic), slow or incomplete search
  (Logseq, Spotlight in early Tahoe), features moved or removed (Launchpad in macOS 26), hidden features nobody discovers
  (Listary), opaque answers without citations (Glean's own docs admit this happens), and screen recording as a privacy
  hazard (Recall's 2024 backlash; Signal blocking it in 2025; Rewind's capture switched off on 2025-12-19).
- **The catalogue (section 3)** lists 112 features in 13 groups, each with a stage, an effort and a privacy note. The
  catalogue changes the plan in four ways (section 5): the action panel, filter syntax, saved searches, quicklinks and
  fallback searches move into stage 1; a coverage and privacy status lands in stage 1 and 2; "why this result" and hit
  highlighting join stage 2; family hand-offs become a stage 4 work package.
- **Differentiators (section 4):** nine features that none of the products checked offers as a whole, led by one
  cross-platform index over files, content, mail and Microsoft 365 with cited answers from a local model, checked in code.
- **What not to build (section 6):** screen or audio recording, window management, a dock, cloud AI inside the app,
  foreign extension runtimes, a system-wide typing hook, web answers, and team features.

## 1. Effort and priority scales

- **Effort:** **S** up to 3 agent-days; **M** 4 to 10; **L** more than 10. Rough, like the plan's sizes; firm figures come
  with the ADRs' work packages.
- **Priority:** **1** to **4** are the plan's stages (1 launcher and names; 2 content and meaning; 3 mail and M365; 4 answers,
  MCP, plugins and workflows). **Later** means after stage 4, on demand. **Never** means section 6.
- **Differentiator:** marked **D** (bold, at the end of the feature) when no product in section 2 offers it (section 4 gives the reasons).

## 2. What users love and miss, per product

| Product | Loved (evidence) | Missed or hated (evidence) | Lesson for Recce |
|---|---|---|---|
| **X1 Search** (Windows) | Type-ahead search, "hit highlighting", "50+ metadata filters", preview "in native format", actions "from within a single unified interface"; sources from M365 (Teams, SharePoint, OneDrive, Exchange, Outlook), Slack, Gmail, PST and file shares; an "X1 Search Connector for Claude" that is permission-aware and sends only "the most relevant information" ([x1.com product page](https://www.x1.com/products/x1-search/), vendor). Reviewers praise support and speed ([G2 comparison](https://www.g2.com/compare/listary-vs-x1-search), secondary). | "doesn't have wildcard function", so mistyped names are not found (G2 reviewer); imported saved searches land in the wrong place, "occasionally freezes", "not the prettiest interface" ([Capterra reviews](https://www.capterra.com/p/119912/X1-Professional-Client/reviews/), 2023, secondary; Capterra notes some reviews are incentivised). Windows only, 8 GB RAM minimum, 16 GB plus a GPU for AI (vendor). | Source breadth, native preview, hit highlighting and an MCP connector are table stakes for the "X1 but powerful" half. Add typo tolerance and wildcards; keep the UI fast and plain. |
| **Spotlight** (macOS 26) | One place for files, apps, events and messages; filters by type ("PDFs or Mail messages"); third-party cloud drives; browse views for apps, files and clipboard history; "hundreds of actions" through App Intents; quick keys; menu-bar commands of the front app; personalised suggestions ([Apple Newsroom, 2025-06-09](https://www.apple.com/newsroom/2025/06/macos-tahoe-26-makes-the-mac-more-capable-productive-and-intelligent-than-ever/), vendor). Reviewers call Actions "an absolute game-changer" ([Tom's Guide](https://www.tomsguide.com/computing/macos/ive-been-using-the-new-spotlight-in-macos-tahoe-26-and-its-an-absolute-game-changer), secondary). Clipboard retention 30 min, 8 h or 7 days (earlier note, MacRumors). | Launchpad removed; users lost folders and a visual app grid, and hot corners went dead ([Apple Community thread](https://discussions.apple.com/thread/256138531); [PiunikaWeb, 2025-09-17](https://piunikaweb.com/2025/09/17/macos-26-launchpad-removed-backlash/), secondary). Results showing "only apps" and not files in early Tahoe; better in 26.1 ([MacRumors forum](https://forums.macrumors.com/threads/spotlight-doesnt-show-results.2467459), secondary). Little control over what is indexed (earlier note). | Actions on results and quick keys are now the platform baseline on macOS; Recce must match them and add what Spotlight lacks: control over scope, ranking that explains itself, mail beyond Apple Mail, M365, and Windows and Linux. Never remove a way of working without a replacement. |
| **Windows Search / Copilot search** | On Copilot+ PCs, semantic indexing next to lexical; search by describing a file; works in Settings too; "Enhanced" indexes the whole PC; Copilot app 1.25082.132.0 adds natural-language file search with "File search" and "File read" permissions ([Neowin](https://www.neowin.net/amp/copilot-app-on-windows-gets-new-home-page-and-file-search-with-natural-language/); [Windows Central](https://windowscentral.com/hardware/laptops/copilot-pcs-will-soon-get-their-first-must-have-exclusive-feature); secondary). | Semantic search only on Copilot+ hardware and only for local files in indexed locations; six languages in the first test (same sources). | Meaning search on any hardware (Model2Vec runs on a CPU) is an easy win against Windows. Copilot's separate "search" and "read" permissions are a good model for MCP and answers. |
| **Everything** (voidtools) | Near-instant names; rich syntax: `\|`, `!`, wildcards, `regex:`, `size:`, `dm:`, `ext:`, `parent:`, `content:`, macros such as `audio:` and `doc:`, filters, bookmarks that save search, filter, columns and sort ([search syntax](https://www.voidtools.com/support/everything/searching/), vendor). 1.5 (beta in 2026) adds property and optional content indexing, a preview pane with preview handlers, and size duplicates ([Softpedia changelog](https://www.softpedia.com/progChangelog/Portable-Everything-Changelog-221520.html); [voidtools forum](https://www.voidtools.com/forum/viewtopic.php?p=37706), secondary). A community "really useful" bookmark set for 1.5 (2026-06) shows how much users invest in saved searches ([forum](https://www.voidtools.com/forum/viewtopic.php?p=47431)). | `content:` is "extremely slow" because it is not indexed (vendor page); Windows only; NTFS fast path needs the service or admin (earlier note). Filter search in the sidebar is still a to-do ([forum](https://www.voidtools.com/forum/viewtopic.php?p=23592)). | Copy the syntax shape and saved searches; make content indexed rather than scanned. |
| **Alfred** (macOS) | Universal Actions on text, URLs or files from anywhere, chained without leaving Alfred (since 4.5) ([Universal Actions](https://www.alfredapp.com/universal-actions), vendor); File Buffer gathers several files for one action ([help](https://alfredapp.com/help/features/universal-actions), vendor); workflows from no-code blocks, launched by hotkey, keyword or Alfred Remote; clipboard history of text, images and file paths; snippets with keyword expansion; Quick Look before opening; Large Type; contacts viewer ([Powerpack](https://www.alfredapp.com/powerpack/), vendor). Clipboard ignores Keychain Access and 1Password by default, with an "Ignore Apps" list; "clear" wipes the last 5 or 15 minutes ([Clipboard help](https://www.alfredapp.com/help/features/clipboard/), vendor). | macOS only; clipboard is off by default and needs Accessibility (same page). Whether it honours the "transient" marker in general is unconfirmed ([pass mailing list, 2017](https://lists.zx2c4.com/pipermail/password-store/2017-February/002773.html)) **(verify)**. | Universal Actions and the File Buffer are the best action model in the field. The clipboard defaults (ignore password apps, clear recent) are the right privacy baseline. |
| **Raycast** (macOS; Windows beta) | Quicklinks with dynamic placeholders, snippets, clipboard history, window management (custom commands and layouts in Pro), extensions store (1,300 to 1,500+, secondary) ([changelog](https://raycast.com/changelog/page/2), vendor; [G2](https://www.g2.com/sellers/raycast), secondary: "a true all-in-one productivity hub"). Local models through Ollama since 1.99 (2025-05-21), with tool calling from 1.104 (2025-12-16) ([changelog 1.99](https://www.raycast.com/changelog/macos/1-99-0), vendor). MCP client since 1.98 (2025-05-08): @-mention a server in Quick AI or AI Chat; read-only tools approved by default, others prompt ([MCP manual](https://manual.raycast.com/ai/model-context-protocol), vendor). Windows public beta 2025-11 ([AlternativeTo](https://alternativeto.net/news/2025/11/raycast-for-windows-launches-public-beta-with-native-search-and-extension-support/), secondary). | AI is a paid cloud feature by default; local-model extensions "can be a bit unreliable" (1.99 notes); Windows is a subset (secondary). No whole-PC content index (earlier note). | The @-mention scope and the read-only-by-default tool approval are the right shape for Recce's answers and MCP. Quicklinks with placeholders are cheap and loved. |
| **PowerToys Command Palette** (Windows) | Prefixes: `>` run, `??` web, `$` Settings pages, `=` calculator, `file` search; WinGet install; window switcher; clipboard history; Services; Registry; Remote Desktop; system commands including "Update and restart"; Home with pinned and recent items; compact mode and quick-access shelf; Alt+1..9 to run the n-th result; Ctrl+Shift+Enter run as administrator; bookmarks with `{query}` placeholders prompted inline; command links to run a command from a script; the Dock (0.98, 2026-03-17); an in-app Extension Gallery (0.100, 2026-06-09) ([Microsoft Learn, updated 2026-10-21](https://learn.microsoft.com/en-us/windows/powertoys/command-palette/overview), vendor; [0.100 post](https://devblogs.microsoft.com/commandline/powertoys-0-100-is-here-new-shortcut-guide-command-palette-improvements-and-much-more/), vendor). MIT licence (earlier note). | Windows only; no content index (earlier note). | Prefixes, pinned and recent items, Alt+number, run as admin, inline placeholder prompts and command links are all S-sized and expected by Windows users. |
| **Listary** (Windows) | Fuzzy matching that finds "Project" from "pjroect" ([review](https://filelocator-net.pages.dev/reviews/listary/), *weak*: run by a competitor); Tab or → opens file actions (copy path, open folder, run as admin); web keywords with live suggestions (`g`, `wiki`) ([Listary 6 beta](https://discussion.listary.com/t/listary-6-beta/4615), vendor forum); **Quick Switch**: Ctrl+G moves an Open/Save dialog to the recommended or last folder ([help](https://help.listary.com/quick-switch), vendor). | Quick Switch fails in custom file dialogs ([Quick Save & Open](https://www.listary.com/feature/quick-save-and-open), vendor) and sometimes in the beta (forum); features take "deliberate learning to discover" (review, *weak*). | Quick Switch is loved but Windows-specific and fragile; later at most. Discoverability must be designed in (hints, a cheat sheet). |
| **Flow Launcher** (Windows) | Files and contents through Everything or Windows Search; plugin store; batch and PowerShell as admin or another user; browser bookmarks across browsers; Windows and Control Panel settings; system commands; calculator with copy ([flowlauncher.com](https://www.flowlauncher.com/), vendor; v2.1.3 of 2026-06-08). | Depends on another indexer for files; Windows only. | A launcher on top of someone else's index is limited by it; Recce owns its index. |
| **Recoll** | Xesam-based query language in GUI, CLI and Web UI; field filters (`author:`, `dir:`, `mime:`); `issub:` in 1.31; preview window; readable snippets with case and punctuation ([query language](https://recoll.org/usermanual/webhelp/docs/RCL.SEARCH.LANG.html); [release history](https://recoll.org/pages/release-history.html), vendor). Front ends like consult-recoll add live preview ([jao.io](https://jao.io/blog/consult-recoll-with-live-preview.html)). | Stores document text in the index for snippets (same history); a reported sluggish regression with a new Xapian ([openSUSE bug 1064133](https://lists.opensuse.org/archives/list/bugs@lists.opensuse.org/thread/OZICUAXCFWEFXM4T3DETLXXR5XZPAEBP)); dated UI (earlier note). GPL. | Fielded queries and good snippets matter; we re-read passages from the file instead of storing text (plan D5). |
| **DocFetcher** (and Pro) | Portable; Office, PDF, EPUB, HTML, RTF, PST; daemon notes changes while closed (earlier note). Pro 1.23 (2025-11-26) shows the memory limit in Preferences and fixes Office, Outlook and EPUB extraction crashes; 1.21 (2025-09-12) fixed an antivirus-caused indexing crash and signs all Windows binaries ([Pro news](https://docfetcherpro.com/news/2025/11/26/docfetcher-pro-1-23-release/); [changelog](https://docfetcherpro.com/changelog-docfetcher-pro/), vendor). | New features only in Pro; opening Outlook items "may be added" in v2.0 or later (vendor). | Extraction crashes are the recurring bug class: guarded workers and quarantine (plan 2.1) are right. Showing resource limits to the user is cheap trust. |
| **Copernic Desktop Search** | "finds results Windows often misses", more search options ([G2 review](https://www.g2.com/survey_responses/copernic-desktop-search-review-1195412), secondary). | 3.4 to 3.5 of 5; features removed between versions; a 2025-08 review reports blue screens with the indexing view on and says it suits only small data volumes ([Capterra](https://www.capterra.com/p/9020/Copernic-Desktop-Search/), secondary). | An indexer that hogs or crashes the machine loses users; idle cost and crash containment are features. |
| **Glean** (work search) | Connectors keep the source's permissions so results "strictly adhere" to them ([Glean docs](https://docs.glean.com/connectors/connectors-power-glean), vendor); Assistant answers with inline citation markers and "View sources", and people records can be cited ([citations](https://docs.glean.com/user-guide/assistant/glean-chat/glean-chat-citations/glean-citations), vendor); go links in search and the address bar ([GitLab handbook](https://handbook.gitlab.com/handbook/eta/ai/tools/glean/tips/), secondary). | Citations sometimes do not appear, and the docs suggest asking explicitly (same vendor page); rollout complexity; a 2025-09-17 Gartner review headline on "unexpected data disclosure" (headline only seen) ([Gartner](https://www.gartner.com/reviews/market/enterprise-search-engines/vendor/glean)) **(verify)**; go-link adoption hard ([G2](https://g2.com/products/glean-2022-05-27/reviews/glean-review-6891957)). | Permission-aware sources and visible citations are expected. Make uncited answers impossible to miss, not merely possible. |
| **Lookup** | Not identified. Searches found several products of that name (a video-AI tool launched 2025-09-23, a data-analysis tool, an inactive lookup.ai) and none that is a work-search product ([Product Hunt](https://www.producthunt.com/products/lookup-4/launches)). | | **The user should say which Lookup they mean**; nothing in this note depends on it. GoSearch is a comparable work-search product ([gosearch.ai](https://gosearch.ai/mobile-app), vendor). |
| **Rewind / Limitless** | Recorded and transcribed what the user saw and heard, searchable later (secondary). | Meta acquired Limitless; screen and audio capture in the Rewind Mac app and Limitless apps was switched off on 2025-12-19, and EU and UK users lost access at once ([9to5Mac, 2025-12-05](https://9to5mac.com/2025/12/05/rewind-limitless-meta-acquisition/), secondary). | A memory product that depends on a vendor's cloud and one company's fate is fragile. |
| **Microsoft Recall** | Opt-in snapshots, Windows Hello on every open, encrypted local snapshots and index, app and website filters, a sensitive-information filter on by default, delete per app or site, pause from the tray, pauses below 25 GB free ([Microsoft Support](https://support.microsoft.com/en-us/windows/retrace-your-steps-with-recall-aa03f8a0-a78b-4b3e-b0a1-2eb8ac48701c), vendor). | Made opt-in only after a 2024 backlash ([Axios, 2024-06-07](https://www.axios.com/2024/06/07/microsoft-recall-security-privacy-criticism)); Signal turned on a DRM screen-capture flag by default because Microsoft gave developers no way to opt out, and Brave and AdGuard followed in 2025-07 ([TechRepublic](https://www.techrepublic.com/article/news-signal-blocks-windows-recall-privacy/), secondary). | The privacy controls are worth copying for Recce's sensitive stores (mail, clipboard); the capture is not (section 6). |
| **Khoj** | Semantic search and chat over pdf, markdown, org, word and notion files; "any local or online LLM"; clients for browser, Obsidian, Emacs, desktop, phone and WhatsApp; automations ([README](https://github.com/khoj-ai/khoj), vendor). | Not a PC-wide indexer or launcher; AGPL-3.0 (same page). | Meet people where they work (an Obsidian or editor client is a later MCP or plugin job). |
| **Other local RAG** | AnythingLLM: desktop app with an embedded vector store, per-project document sets, citations ([review](https://pasqualepillitteri.it/en/news/3844/anythingllm-what-it-is-review-2026), secondary); MIT. Reor: local notes with auto-linking and RAG Q&A; GPL-3.0, no releases on the fork seen ([GitHub](https://github.com/chensheng0619/reor)). | Manual workspaces, not a live index (earlier note). | Scoped answers ("these results", "this folder") give the workspace benefit without manual uploads. |
| **Obsidian** search | `path:`, `tag:`, `line:(foo bar)` for same-line matches, `section:` per heading, and embedded `query` blocks that render live results in a note ([Obsidian help](https://obsidian.md/help/plugins/search), vendor); Omnisearch adds ranked full-text search usable from other plugins ([embedded-omnisearch](https://github.com/fnsign/embedded-omnisearch)). | Core search is not ranked (hence Omnisearch) **(verify)**. | Structural operators (same line, same section) and saved live queries are worth having for content search. |
| **Logseq** search | | Search missed a block in a 1,200-line page and the block shortcut was slow; the developer blamed lazy loading and pointed to the DB version ([Logseq forum, late 2024](https://discuss.logseq.com/t/weak-search-functionality/29746)). | Users notice a single missed hit; recall tests on a synthetic corpus belong in CI. |

## 3. Feature catalogue

Columns: **Stage** (1 to 4, Later, Never), **Effort** (S/M/L), **Why** (evidence and reasoning), **Privacy** (what to guard).
"Plan" means the feature is already in the plan; "new" means this catalogue adds it. Ids: Q search, P previews, A actions,
L launcher, C sources, K answers, M MCP, E extensions, V privacy, F performance, N cross-device, Y accessibility, G family
(chosen so they do not clash with the plan's spikes S1 to S6, ADRs R1 to R10 and decisions D1 to D19 (S6, R10 and D17 to D19 added with the scale design, 2026-11-04)).

### 3.1 Search and filters

| # | Feature | Stage | Effort | Why | Privacy |
|---|---|---|---|---|---|
| Q1 | One list as you type across apps, files, actions, content and mail, grouped by kind (plan) | 1 | M | Spotlight, Raycast, Alfred, X1 all do it; the core promise. | Queries never logged (ER-27). |
| Q2 | Typo-tolerant, initialism and fuzzy matching ("fnc rpt") (new detail) | 1 | S | Listary's best-loved trait; X1's missing wildcard is its top complaint. `paddock-names` does the matching. | none |
| Q3 | Learned ranking: frecency per query prefix (plan 1.3) | 1 | S | Spotlight "learns from routines"; Alfred and Raycast learn picks. | Usage history local, clearable, off-switch. |
| Q4 | Inline filter syntax: `kind:`, `ext:`, `size:`, `dm:`, `in:`/`parent:`, `\|`, `!`, wildcards, `regex:` (new) | 1 | M | Everything's syntax is what power users invest in; ADR R2 defines the query model anyway. | none |
| Q5 | Filter chips for the same filters (type, source, date, cloud) (new) | 1 | S | Spotlight 26 filters; X1's "50+ metadata filters". Chips and syntax are one model, two views. | none |
| Q6 | Saved searches and macros with a keyword (Everything bookmarks) (new) | 1 | S | Everything users share bookmark sets; X1 users save searches. Stored in a TOML pack so they can be edited and shared. | A saved search may name private paths; stays local. |
| Q7 | Scopes: this folder, this project, this source (new) | 1 | S | Listary project scopes, Everything `parent:`. | none |
| Q8 | Content operators: phrases, proximity, same line or section, fields (`from:`, `subject:`, `author:`) | 2 (files), 3 (mail) | M | Recoll fields, Obsidian `line:`/`section:`; Tantivy supports phrases. | none |
| Q9 | Meaning search (describe a file) through `paddock-embed` (plan 2.3) | 2 | M | Windows does it only on Copilot+ hardware; Khoj does it for notes. | Embeddings are derived content: same protection as the index. |
| Q10 | Natural-language filters parsed **deterministically** into visible chips ("pdfs from Anna last week") (new) | 2 | M | Copilot search shows the demand; a rule-based parser keeps rule 46 (no LLM per query) and shows what it understood. | none |
| Q11 | "Why this result": matched field, passage, name vs content vs meaning score (new) **D** | 2 | S | No product checked explains its ranking; Glean explains answers, not hits. Builds trust and helps tune RRF. | none |
| Q12 | Search history and recent searches (new) | 1 | S | Command Palette recents. | Off-switch and Clear; never in logs or diagnostics. |
| Q13 | Duplicates by size then hash (Everything 1.5) | Later | M | Useful but a Dyno-like cleanup job; the manifest has sizes and hashes already. | none |
| Q14 | Multilingual meaning search (model as a Setting, plan 2.3) | 2 | S | Windows' first test covered six languages; mail is multilingual. | none |
| Q15 | Recall tests: a stored synthetic corpus and query set must find every planted hit (new) | 2 | S | Logseq's single missed block became a forum thread; X1's misses are its top complaint. | Synthetic data only (ER-14). |

### 3.2 Previews

| # | Feature | Stage | Effort | Why | Privacy |
|---|---|---|---|---|---|
| P1 | Metadata preview: path, size, dates, kind, source (plan 1.6) | 1 | S | Every product. | Paths shown only in the app. |
| P2 | Text preview with hit highlighting and next-hit keys (new) | 2 | M | X1's "hit highlighting", Recoll snippets. Passages are re-read from the file, not stored (D5). | Re-read on demand; nothing cached on disk. |
| P3 | Open at the match: page in a PDF, line in a text file (new) | 2 | S | Obsidian `line:`; users want to land on the hit. Per-format: page for PDF, line for text; others open plainly. | none |
| P4 | Mail preview: headers, thread, attachment list (new) | 3 | M | X1's core view. | Mail text from the encrypted store; shown, never logged. |
| P5 | Image thumbnails (new) | 2 | S | Spotlight and Alfred show them. | Thumbnails are content: in memory or the encrypted store only. |
| P6 | Native preview (Quick Look on macOS; Windows preview handlers) (plan 1.6 "verify") | Later | L | Alfred, X1 and Everything 1.5 offer it, but Windows preview handlers are third-party shell code run on untrusted files; host them out of process or skip. Our own text and image preview covers most needs first. | Run out of process if ever built. |

### 3.3 Actions

| # | Feature | Stage | Effort | Why | Privacy |
|---|---|---|---|---|---|
| A1 | Action panel on every result (Tab or Ctrl/⌘+K): open, open with, reveal, copy path, copy name, copy as link (plan 1.6, widened) | 1 | M | Alfred Universal Actions, Raycast action panel, Listary Tab, Command Palette context menu. The one feature every launcher user expects. | none |
| A2 | File buffer: gather several results, run one action on all (new) | 1 | S | Alfred's File Buffer. Cheap once A1 exists. | none |
| A3 | Run as administrator (Windows, Ctrl+Shift+Enter) with confirmation (new) | 1 | S | Command Palette, Listary, Flow. | Elevation prompt is the OS's; never cached. |
| A4 | Alt+1..9 to run the n-th result (new) | 1 | S | Command Palette. | none |
| A5 | Chained actions (move to, compress, rename, then open) through workflows (plan 4.3) | 4 | M | Alfred 4.5 chaining. | Writes confirm (destructive). |
| A6 | Actions on the selection in any app (select text or files, press a hotkey) (new) | Later | M | Alfred Universal Actions "from anywhere". Needs Accessibility on macOS and UI Automation on Windows. | Reads the selection only on the hotkey; never watches. |
| A7 | Paste a result or snippet into the front app (new) | 4 | M | Raycast "instant paste" is its most praised trait (secondary). Needs Accessibility on macOS. | Synthetic key events only on the user's command. |
| A8 | Quick Switch: move an Open/Save dialog to a result's folder (new) | Later | L | Listary's loved feature, but Windows-only in practice and fragile in custom dialogs (vendor). | none |
| A9 | Share a file (mail it, AirDrop, OS share sheet) (new) | Later | S | Spotlight actions. | Hands the file to the OS only. |

### 3.4 Launcher

| # | Feature | Stage | Effort | Why | Privacy |
|---|---|---|---|---|---|
| L1 | Global hotkey, pre-created window, keyboard only, ≤ 50 ms paint (plan 1.6) | 1 | M | All products. | none |
| L2 | App catalogue including Store apps and Settings pages (`$ display`) (plan 1.7, widened) | 1 | M | Command Palette `$`, Flow settings search. Settings pages need a per-OS list (data, not code). | none |
| L3 | Calculator with units and bases (`fend-core`); currency behind its own network switch (plan 1.7) | 1 | S | Command Palette, Raycast, Flow. | Currency rates fetch is a network feature. |
| L4 | System commands with confirmation (lock, sleep, empty trash, restart, "Update and restart") (plan 1.7) | 1 | S | Command Palette, Flow. | Destructive ones confirm. |
| L5 | Quicklinks and web searches with `{query}` placeholders prompted inline (plan 4.3 → **moved to 1**) | 1 | S | Raycast quicklinks with dynamic placeholders, Command Palette bookmarks, Listary `g`. Cheap, loved, and the pack format is needed in stage 1 anyway for exclusions. | Opening a URL is the user's action. |
| L6 | Live suggestions from a web engine, only after its keyword (`g term`) (new) | 1 | S | Listary 6. Only after an explicit keyword, so plain queries never leave the machine. On by default (family network rule) with a switch per engine. | Never for plain queries. |
| L7 | Fallback commands when nothing matches: search the web, ask (stage 4), create a note (new) | 1 | S | Raycast and Command Palette fallbacks. Shown, never run automatically. | Never sends a query by itself. |
| L8 | Aliases and per-command hotkeys (new) | 1 | S | Raycast; Spotlight quick keys. | none |
| L9 | Home with pinned and recent commands; compact mode (new) | 1 | S | Command Palette Home, shelf and compact mode. | Recents: Clear and off-switch. |
| L10 | Command links: `recce://run/<command>` to start a command from a script or another app (new) | 1 | S | Command Palette "Copy command link"; `paddock-siblings` already has a deep-link scheme (`crates/paddock-siblings/src/app.rs`). | Links run only declared commands; destructive ones still confirm. |
| L11 | Clipboard history (plan 4.5, D6 off by default) | 4 | M | Spotlight 26, Raycast, Alfred, Command Palette all have it; on macOS 26 it is native, so ours is mainly for Windows and Linux. | Off by default; ignore password apps by default (Alfred); transient markers; retention choices (Spotlight's 30 min, 8 h, 7 days); encrypted; "clear last 5 or 15 minutes". |
| L12 | Snippets inserted from the launcher, with placeholders (plan 4.5) | 4 | S | Alfred, Raycast. | Snippets may hold secrets: encrypted store. |
| L13 | Window switcher (new) | Later | M | Command Palette, Raycast. macOS needs Accessibility. | Window titles are user content: not logged. |
| L14 | Emoji and symbol picker (new) | Later | S | Raycast; the OS pickers exist. | none |
| L15 | Large type for a result (new) | Later | S | Alfred Large Type. | none |
| L16 | Hotkey conflict check and rebinding; optionally take the Copilot key on Windows (new) | 1 | S | Command Palette offers the Copilot key. | none |
| L17 | Typed text expansion everywhere | Never | L | Needs a system-wide keyboard hook (section 6). | Keylogger-shaped. |

### 3.5 Sources and connectors

| # | Feature | Stage | Effort | Why | Privacy |
|---|---|---|---|---|---|
| C1 | All local drives and network shares by name, current within 2 s (plan 1.4, 1.5) | 1 | L | Everything, X1. | Built-in exclusion pack. |
| C2 | Content: Office, PDF, text, HTML, RTF, **EPUB** (plan 2.1; EPUB new) | 2 | L | DocFetcher, Recoll, X1; DocFetcher Pro's 2025 fixes show EPUB matters. | Guarded workers; no stored text (D5). |
| C3 | Inside archives (zip) with ratio and size limits (new) | Later | M | Recoll and DocFetcher read archives **(verify DocFetcher)**; `paddock-archive` has the limits. | Zip bombs: limits and quarantine. |
| C4 | Local mail: PST, mbox, eml, msg, Apple Mail, Thunderbird (plan 3.1) | 3 | L | X1, DocFetcher PST, Copernic. | Generated mailboxes in tests (ER-14). |
| C5 | Outlook mail, OneDrive and SharePoint through Graph (plan 3.2 to 3.4) | 3 | L | X1's core; the user's brief names SharePoint. | Delegated rights only; access re-check. |
| C6 | IMAP and Gmail (plan 3.5) | 3 | M | X1 Gmail. | App passwords in the keychain. |
| C7 | Teams chats and channels | Later | L | X1 has Teams. Needs extra Graph permissions that tenants often block. | Chat is sensitive: off by default. |
| C8 | Google Drive, Dropbox, Box as plugins (new) | Later | M each | X1 has them; plugins (plan 4.4) keep the core small. | Per-plugin grants. |
| C9 | Calendar events and contacts (new) | Later | M | Spotlight events, Alfred contacts viewer. | Contacts are personal data: off by default. |
| C10 | Browser bookmarks (new) | 2 | S | Flow searches bookmarks across browsers. Small files per browser. | Bookmarks only; history is separate (C11). |
| C11 | Browser history (new) | Later | M | Asked for in launchers; very sensitive. | Off by default; per browser; never to MCP by default. |
| C12 | Note vaults (Obsidian, Logseq markdown): tags, links, headings (plan 4.4 sample plugin) | 4 | M | Khoj and Obsidian users; Logseq users report missed hits. | Same as files. |
| C13 | Slack and other work apps as plugins | Later | M each | X1, Glean. | Per-plugin grants. |
| C14 | OCR for images and scanned PDFs (plan D13, off by default) | 2 | M | Windows and X1 classify images; costly (rule 51). | Confirmed bulk job. |
| C15 | Cloud placeholders: names only, never download (plan 1.4) | 1 | S | Pacenotes ADR 0012 D1. | Never hydrates files. |
| C16 | **Coverage and source health**: per source, last sync, counts, errors, unreadable folders and why (no Full Disk Access, excluded, locked), with Repair (new) **D** | 1 (files), 3 (remote) | M | Spotlight users cannot tell why files are missing; Glean gives admins a validation view, nobody gives a person one. Risk 2 of the research note needs it. | Shows paths only in the app; counts only in logs. |

### 3.6 Answers and RAG

| # | Feature | Stage | Effort | Why | Privacy |
|---|---|---|---|---|---|
| K1 | Answers with numbered sources from a local model through Pitwall (plan 4.1) | 4 | M | Glean, Khoj, AnythingLLM. | Local only. |
| K2 | Citation check in Rust: drop unknown `[n]`, flag uncited sentences, and **say "no supported answer" instead of an uncited one** (plan 4.1, last part new) **D** | 4 | S | Glean's docs admit citations sometimes do not appear; none checks them in code. | none |
| K3 | Source cards with the passage and "Open at passage" (new) | 4 | S | Glean "View sources". Reuses P2 and P3. | Passages re-read. |
| K4 | Scoped answers: "these results", "this folder", "this mailbox", `@source` (new) | 4 | S | Raycast @-mentions, AnythingLLM workspaces. | The scope limits what the model sees. |
| K5 | Ask from the launcher (Tab on a query) with streaming, Cancel and "Still working" (plan 4.1) | 4 | S | Raycast Quick AI; rules 10 to 12. | none |
| K6 | Follow-up chat on an answer (new) | 4 | M | Raycast AI Chat, Khoj. | Chat history local, encrypted, deletable, retention setting. |
| K7 | Per-result AI actions: summarise this document or thread (new) | 4 | S | Raycast AI commands. One document, user-started (rule 46). | none |
| K8 | Re-rank of the top 50 by Qwen through Pitwall (plan 2.3) | 2 | S | Plan. | Local. |
| K9 | Cloud LLMs or bring-your-own cloud keys inside Recce | Never | | "AI local" (family rule). A user who wants a cloud model connects it through MCP (M1), with a warning. | |
| K10 | Answers from the web; scheduled research newsletters (Khoj) | Never | | Out of scope: Recce is local search. | |

### 3.7 MCP

| # | Feature | Stage | Effort | Why | Privacy |
|---|---|---|---|---|---|
| M1 | MCP server: `search`, `get_passage`, `list_sources`; bounded passages, not whole files (plan 4.2) | 4 | M | X1's Claude connector sells exactly this, including "fewer tokens". | Off by default; no write or launch tools. |
| M2 | Per-source MCP visibility, mail and M365 hidden by default, an audit view of every call (plan 4.2, D8) **D** | 4 | S | Copilot's separate "search" and "read" permissions; no desktop tool checked shows an audit of what an agent read. | The audit holds tool, counts and ids, never text. |
| M3 | Warn that a cloud client sends results off the machine (plan, research §3) | 4 | S | User's AI-local rule covers our models, not a client's. | Shown when MCP is switched on. |
| M4 | Recce as an MCP **client** in answers (`@server`), read-only tools approved, others prompt (new) | Later | M | Raycast's model since 1.98. | Write tools prompt every time. |
| M5 | Expose `answer` over MCP (plan, only if enabled) | 4 | S | Plan. | Separate switch. |

### 3.8 Plugins and workflows

| # | Feature | Stage | Effort | Why | Privacy |
|---|---|---|---|---|---|
| E1 | TOML packs: quicklinks, web searches, saved searches (stage 1); snippets and workflows (stage 4) (plan 4.3, split) | 1 and 4 | S + M | Alfred workflows, Raycast quicklinks, Everything bookmarks. Packs are data (rule 21). | Run-command steps only from the user's packs, after confirmation. |
| E2 | Rust plugins: `SearchSource`, `ResultProvider`, actions (plan 4.4) | 4 | L | Every launcher has extensions; ours are out of process. | Default-deny grants; "Forget plugin". |
| E3 | Visible plugin budgets: a provider over its time budget is skipped and shown as slow; repeat offenders quarantined with the reason (plan 4.4, visibility new) **D** | 4 | S | Launcher extensions slow every keystroke; none of the products shows which one. | none |
| E4 | In-app gallery to browse, install, update and remove signed plugins (new) | Later | M | Command Palette 0.100 (2026-06-09), Raycast store, Flow store. Needs a family index of signed `.paddockplugin` files. | Signed only (ADR 0001). |
| E5 | Run Raycast or Alfred extensions | Never | | Node and AppleScript runtimes against the Rust-only plugin decision (research §2.9). | |

### 3.9 Privacy and security

| # | Feature | Stage | Effort | Why | Privacy |
|---|---|---|---|---|---|
| V1 | Exclusion pack plus user exclusions; OS markers; names-only per source (plan) | 1 | S | Plan §3. | |
| V2 | No stored local text; remote text encrypted; FDE check (plan 2.6, D5) | 2 | M | Recoll stores text for snippets; Recall had to add encryption after the backlash. | |
| V3 | **Privacy ledger**: what is indexed per source (counts, kinds), where it is stored, its size, what MCP read, with "Forget this source" (new) **D** | 2 | M | Recall offers delete per app and site; none of the search tools shows the whole picture in one place. Pairs with C16. | Shows counts and kinds, paths only on request. |
| V4 | Lock sensitive sources (mail, clipboard, answers history) behind Touch ID or Windows Hello (new) | 3 | M | Recall requires Windows Hello on every open (vendor). | Platform auth; Linux falls back to the session (verify a portal). |
| V5 | Deterministic secret filter: card numbers, keys and tokens kept out of clipboard history and MCP passages (new) | 4 | S | Recall's sensitive-information filter, on by default. Pattern list in a pack. | Patterns, not a model (rule 46). |
| V6 | Pause indexing and pause clipboard from the tray, with a timer (new) | 1 (indexing), 4 (clipboard) | S | Recall pauses from the tray; Alfred clears recent minutes. | none |
| V7 | Retention per sensitive store (clipboard, answer history, recent searches) (new) | 4 | S | Spotlight's 30 min, 8 h, 7 days. | Defaults short. |
| V8 | Delegated, permission-aware remote sources with re-check on open (plan 3.3) | 3 | S | Glean, X1. | Never escalates. |
| V9 | An invented-names test over index, logs, diagnostics and the MCP audit (plan §0, widened to MCP) | 1 | S | ER-27; Dyno ADR 0029 §6 pattern. | |

### 3.10 Performance

| # | Feature | Stage | Effort | Why | Privacy |
|---|---|---|---|---|---|
| F1 | Budgets: ≤ 50 ms paint, ≤ 20 ms names, ≤ 100 ms BM25, ≤ 200 ms hybrid (plan, research §5) | 1 and 2 | (in WPs) | Everything sets the bar; Spotlight's slow-results complaints show the cost of missing it. | |
| F2 | Idle CPU ≈ 0, utility QoS for bulk work, pause on battery and while the user is busy (new: battery and busy) | 2 | S | Copernic's "heavy" indexing and blue screens; DocFetcher showing its memory limit. | |
| F3 | Index size per source and a memory cap shown in Settings (new) | 2 | S | DocFetcher Pro 1.23 shows its memory limit; Everything shows per-property database sizes. | Counts and sizes only. |
| F4 | Rebuild one source (not everything) from the coverage view (new) | 1 | S | Spotlight's "only apps" complaints end in whole-index rebuilds. | |
| F5 | Resumable first index with phases, growing counts and Cancel (plan) | 1 | (in WPs) | Rules 10 to 14. | |

### 3.11 Cross-device

| # | Feature | Stage | Effort | Why | Privacy |
|---|---|---|---|---|---|
| N1 | One app, the same features and pack formats on macOS, Windows and Linux (plan) **D** | 1 | (in WPs) | X1, Everything, Listary, Flow and Command Palette are Windows-only; Alfred and Spotlight macOS-only; Raycast's Windows build is a subset. | |
| N2 | Settings, packs and snippets sync through a folder the user picks (no Recce cloud) (new) | Later | S | Raycast's cloud sync is a Pro feature; a folder works with any sync tool the user trusts. | Snippets encrypted with a passphrase if synced. |
| N3 | Search my other machines on the LAN (agent to agent, mutual auth, off by default) (new) | Later | L | Nobody checked does it locally; useful with a Mac mini plus a Windows PC. Needs a design for trust (Pitwall link trust is already deferred). | LAN only, paired devices, results not text unless opened. |
| N4 | Phone or iPad remote (Alfred Remote) | Never | | Personal desktop scope. | |

### 3.12 Accessibility

| # | Feature | Stage | Effort | Why | Privacy |
|---|---|---|---|---|---|
| Y1 | Listbox semantics, result counts announced, live region for "Still working" (plan 1.6, CDK) | 1 | S | Rule 47; launchers are often silent to screen readers **(verify per product)**. | |
| Y2 | Every action reachable by keyboard, with its shortcut shown in the action panel and a cheat sheet (new) | 1 | S | Listary's hidden features are its main complaint. | |
| Y3 | Mouse and touch work too (click, scroll, right-click for actions) (new) | 1 | S | Listary 6 added mouse use for Quick Switch. | |
| Y4 | Tokens, contrast, reduced motion, density from the shared shell (shared) | 1 | (shared) | Family rule. | |
| Y5 | Large type (L15) | Later | S | Alfred. | |

### 3.13 Paddock-family integrations

All hand-offs go through `paddock-siblings` (deep links `<scheme>://show/<kind>/<id>`, `crates/paddock-siblings/src/handoff.rs`).
Recce never acts on another app's domain itself: it hands off.

| # | Feature | Stage | Effort | Why | Privacy |
|---|---|---|---|---|---|
| G1 | **Family command registry**: each Paddock app declares its commands (open a Debrief job, a Dyno view, a Pitcrew machine), and Recce lists them in the launcher (new) **D** | 4 | M | Spotlight's App Intents and Command Palette extensions do this per platform; a shared declaration keeps one contract (rule 55). Needs a family ADR (shared `paddock-siblings` change). | Commands, not data. |
| G2 | Dyno: "what is using this file", "disk use of this folder" → Dyno's views; `cpu`, `memory` show Dyno's summary as a result (new) | 4 | S | Command Palette shows system metrics; Dyno is the family's monitor. Kills stay in Dyno (rule 53). | Read-only. |
| G3 | Debrief: log files and diagnostic bundles get "Open in Debrief"; Debrief projects appear as results (new) | 2 | S | Recce finds the files; Debrief analyses them. | none |
| G4 | Pacenotes: code repositories get "Open in Pacenotes"; symbol search across Pacenotes projects through its MCP or a `ResultProvider`; Recce indexes only names inside repositories Pacenotes owns, so code is not indexed twice (new) | 4 (open), Later (symbols) | S, M | Pacenotes already indexes code (its README); one index per kind of data. | none |
| G5 | Pitwall: answers, re-rank, model presets, "Pitwall is not running → Start" (plan 2.3, 4.1) | 2 and 4 | (in WPs) | Plan. | Local. |
| G6 | Spotter: watched releases as results ("release of X") (new) | Later | S | Cheap through G1. | none |

## 4. Differentiators

Each was checked against the products in section 2; "none" means none of those products offers it as described.

1. **One cross-platform index over names, content, mail and Microsoft 365** (C1 to C6, N1). X1 has the sources but only on
   Windows; Spotlight and Windows Search are single-OS; launchers rely on other indexers.
2. **Cited answers from a local model, checked in code, that refuse rather than invent** (K1, K2). Glean cites but its docs
   admit missing citations; Raycast and Khoj run local models but do not check citations against an index.
3. **"Why this result"** (Q11): none explains ranking.
4. **Coverage and source health with Repair** (C16, F4): none tells a person what it cannot read and why.
5. **Privacy ledger** (V3) together with **MCP per-source visibility and an audit view** (M2): Recall has per-app delete,
   X1 has a permission-aware connector, none shows one place for what is indexed and what an agent read.
6. **Natural-language filters without an LLM**, shown as editable chips (Q10): Copilot search uses AI and hides the
   interpretation.
7. **Visible plugin budgets and quarantine** (E3).
8. **Family command registry and hand-offs** (G1 to G4): an ecosystem of local apps that a single launcher drives.
9. **Logs and diagnostics proven free of user content** by test (V9, ER-27): no product checked documents this.

## 5. Changes to the plan

Applied to [plans/2026-11-04-pc-search-app.md](../plans/2026-11-04-pc-search-app.md) in the same commit:

- **Stage 1 gains WP 1.8** (query syntax and filter chips, saved searches, scopes, action panel, file buffer, Alt+number,
  run as admin, quicklinks and keyword web searches with suggestions, fallbacks, aliases, Home and recents, command links,
  hotkey conflicts) and **WP 1.9** (coverage and source health for local sources, pause indexing, rebuild one source).
  Quicklinks and web searches move from WP 4.3 to WP 1.8. Stage 1 grows to about 38 to 57 agent-days.
- **Stage 2:** WP 2.2 adds hit highlighting, open-at-match, "why this result", content operators and recall tests; WP 2.6
  adds the privacy ledger; a new **WP 2.7** adds the deterministic natural-language filter parser, browser bookmarks, EPUB,
  battery and busy pausing, and the Debrief hand-off.
- **Stage 3:** WP 3.4 adds the mail preview and OS-auth lock for mail.
- **Stage 4:** WP 4.1 adds refusal of uncited answers, source cards, scoped answers, follow-up chat and per-result AI
  actions; WP 4.3 keeps snippets and workflows; WP 4.5 adds the secret filter and retention; a new **WP 4.6** adds the
  family command registry and the Dyno, Pacenotes and Pitwall hand-offs.
- **New decisions:** D15 (keyword web suggestions on by default, never for plain queries) and D16 (family command
  registry as a shared `paddock-siblings` contract).

## 6. What not to build

| Feature | Seen in | Why not |
|---|---|---|
| Screen recording or periodic snapshots | Recall, Rewind | It stores everything on screen, including other apps' secrets: Recall needed a 2024 redesign to become opt-in and encrypted, and Signal still had to block capture with a DRM flag in 2025. Recce reads the sources themselves, so the text is already there without pictures of it. |
| Audio or meeting recording | Rewind, Limitless pendant | Consent of other people, legal exposure, large stores; Rewind's capture ended on 2025-12-19 after the acquisition. Out of scope. |
| Window management and layouts | Raycast | Not search; the OS offers snapping; a separate product. |
| A dock or menu-bar widgets | Command Palette Dock | Dyno owns tray metrics; one tray icon per purpose. |
| Cloud LLMs or bring-your-own cloud keys inside Recce | Raycast, Khoj | Family rule: AI local. A user may connect a cloud client through MCP, with a warning (M3). |
| Running Raycast, Alfred or Node extensions | Raycast, Alfred | Rust-only out-of-process plugins (ADR 0001, user decision 2026-10-20). |
| System-wide typed text expansion | Alfred, Raycast | Needs a global keyboard hook, which is keylogger-shaped and needs Accessibility; snippets inserted from the launcher cover the need. |
| Web answers and scheduled research | Khoj | Recce is local search; Pitwall and a browser cover the web. |
| Team features: shared go links, people graph, admin consoles | Glean | Personal use; enterprise-only value. |
| Phone or iPad remote | Alfred Remote | Desktop scope; another app to maintain. |
| Downloading cloud placeholders to index them | none (a risk) | Fills the disk and the network; names only (C15). |
| Ranking from telemetry sent to us | Glean (server-side) **(verify)** | No telemetry; learning stays on the machine (Q3). |

## 7. What was not done

- No builds, measurements or VMs (Dyno's release has the machine).
- "Lookup" was not identified (section 2); the user should name it if it matters.
- Many review sites are aggregators of unclear independence; they are marked *weak* and never the only source of a claim
  that drives a priority.

