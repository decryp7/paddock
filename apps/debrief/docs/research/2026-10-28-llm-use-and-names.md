# How Debrief uses the LLM, and learning human names without it (2026-10-28)

Question (user): "check how Debrief uses the LLM; perhaps learning human names should not use the LLM?"
Design constraint added by the user the same day: **"avoid LLM for large jobs"**: work over many items (imports,
catalogues, name tables, indexing, bulk classification, tagging, summaries) uses deterministic rules, shipped TOML
knowledge packs and static embeddings, never an LLM per item. An LLM is only for bounded, user-started requests.

Read on `origin/main` at `9b14d621e`. Performance of the model itself is measured separately (branch `docs/debrief-perf`);
this note does not repeat it. Everything marked *measured* was run here on invented names only; *read* means taken from
code or a document; *estimate* means not verified.

## Answer in short

- Of all LLM uses, **one runs without the user asking: "Learning human names"** (the alias job). It starts after every
  import and after a name-table rebuild during search, is on by default, and with the default settings it loads the model
  if it is not loaded (`autoload` is on by default). Up to 2,000 names in 125 requests of up to 1,500 tokens each, per
  model version. That is the "large job" the new constraint forbids, and the likely reason a model sits in memory and the
  Windows PC is busy right after opening logs.
- A second, user-started bulk use also breaks the constraint: **"Describe frames without a description"** sends one
  vision request per video frame, up to 600 per project.
- **Recommended for names:** a deterministic hybrid. (1) An abbreviation rule in layer 1 (measured 23/24 on ADR 0053's
  own abbreviation set with a four-entry table, 22/24 without it, which is what shipped; no false matches); (2) a shipped, hot-reloadable **names pack**
  (`paddock-packs`) for public Windows, macOS and Linux programs, libraries, services and exceptions; (3) **names the logs
  themselves carry** (Sysmon, SCM, Procmon, msinfo32, journald); (4) Model2Vec synonyms only as a *question* ("did you
  mean…"), never a silent answer (measured top-3 17/24, top-1 10/24 on the ADR set); (5) the LLM only as an explicit
  **"Suggest names with AI"** for at most 16 names the user picks, reviewed before anything is stored.
- What the user loses: confident top-1 answers for plain synonyms of in-house names ("image editor" for
  `gfxbldr.exe`): today the AI layer (with *perfect* fixture answers) gives them; afterwards Debrief asks a question or
  the user teaches it once (user facts already outrank everything). For code-named in-house programs nothing loses,
  because the model sees only the spelling and has to guess.

## 1. Inventory of LLM use

Engine: every use goes through `AiCore::provider()` / `assistant()` / `background_provider()`
(`app/src-tauri/src/ai/core.rs:643-700`), which is Pitwall when it is usable and the setting is Automatic or Pitwall,
otherwise the built-in `debrief-ai-worker` (pinned llama.cpp `llama-server`, ADR 0009/0059). No remote engine exists:
both talk to 127.0.0.1 only (ADR 0059 "Privacy"). Models: the built-in catalogue
(`crates/debrief-ai/data/local-models.json`) recommends Qwen3.5 9B/4B/2B Q4_K_M, Gemma 4 12B, Phi-4 mini, Qwen3.5 27B
and Qwen3.6 35B-A3B; the 9B file is 5.68 GB plus a vision projector. Defaults (`crates/debrief-ai/src/settings.rs:103-128`):
context 8,192 tokens, `max_tokens` 1,024, temperature 0.2, `autoload` **on**, idle unload after 15 min,
`name_aliases` **on**. On Pitwall the model is `debrief` preset or `auto:chat,8k` (ADR 0059).

| Feature | Code | Trigger | Runs without user action? | Typical cost (estimate) | Essential? | Breaks "no LLM for large jobs"? |
|---|---|---|---|---|---|---|
| Chat (assistant with tools: SQL, find values/events, notes) | `ai/requests.rs:422 start_turn` | user | no | 1 turn = 1 to `MAX_TOOL_ROUNDS` requests | core AI feature | no (bounded, user-started) |
| Explain event | `requests.rs:329` `explain_event` | user | no | 1-few requests | replaceable by docs, but is the feature | no |
| Summarize a range | `requests.rs:330` `summarize` | user | no | 1 request over a digest | yes (AI feature) | no: one chosen range |
| Find likely root causes | `requests.rs:331` `root_causes` | user | no | 1 request + tools | yes | no |
| Proposals: regex, pipeline, markers, extractors, query from a question | `requests.rs:332-358` | user | no | 1 request each | convenience; all have manual editors | no |
| Generate a parser (ADR 0032) | `requests.rs:333`, `ai/parser_job.rs` | user | no | 1-few requests on a sample | convenience | no |
| Improve the folder set-up | `requests.rs:359` `refine_folder_setup` | user | no | 1 request | convenience | no |
| Known issues: suggest matching notes | `investigate/issues.rs:21` | user (button) | no | 1 request over a digest | convenience | no |
| Incident report narrative | `investigate/incident.rs:29` | user | no | 1 request | optional block | no |
| Web research (agent in the sandboxed browser) and its query draft | `browser/research.rs:132, 493` | user | no | up to 120 tool rounds (`assistant_with_rounds`, clamp 1-120) | optional | no (user-started, bounded by rounds; long but one request chain) |
| Explain an image / find related logs (ADR 0025) | `requests.rs:434 start_turn_with_images` | user | no | 1 vision request | optional | no |
| Video: describe a frame | `video/commands.rs:457 video_describe` with indices | user | no | 1 vision request per frame | optional | no, for a selection |
| **Video: "Describe frames without a description"** | same command, empty `indices` (`frame-strip.ts:26`) | user | no | **1 vision request per frame, up to 600** (`MAX_PROJECT_FRAMES`, `debrief-video/src/record.rs:28`) | optional | **yes** |
| Video: read the clock on a frame | `video/commands.rs:515`, `debrief-video/src/describe.rs:93` | user | no | 1 vision request | replaceable (OCR + regex) | no |
| Runtime self-test | `debrief-ai/src/runtimes/selftest.rs` | automatic after a runtime download or update | yes, rarely | 1 short completion + 1 tiny image | yes (safety gate) | no (one request) |
| **Learning human names (alias job)** | `app/src-tauri/src/names.rs:233 schedule_alias_job`, `debrief-core/src/names/alias_job.rs`, `debrief-ai/src/names_model.rs` | **automatic**: after every import (`window_manager.rs:222` → `after_import`) and when a search rebuilds the name table (`search_everything.rs:155`) | **yes** | up to `max_groups` 2,000 names, `BATCH` 16 → **125 requests**, each ≤ 1,500 output tokens, temperature 0, JSON schema; repeated per model version | no (ADR 0053 calls it optional) | **yes** |
| Model preload (`autoload`) | `ai/core.rs:396`, `ai/pitwall.rs:96-134` | automatic at start-up (built-in engine) | yes | no generation, but the whole model in memory (≈ 6-8 GB for the 9B, *estimate*: 5.68 GB file + projector + 8k KV cache) | no | not an LLM job, but it is the memory the user saw |

Not found on `main`: LLM query rewriting (rule 20's "AI resolves the user's words" is done by the assistant's
`find_values` tool inside a chat turn, not by a separate rewrite step), Model2Vec and BM25. Model2Vec + BM25 is approved for
Debrief (semantic search over message templates, user decision 2026-10-25) and measured in
`docs/research/2026-10-embeddings-for-logs.md` (branch `docs/embeddings-for-logs`), and the shared embedder is plan X55
(`paddock-embed`, `docs/research/2026-10-ai-memory.md`), but none of it is merged. The full-text search is the store's
index plus `smart_search.rs`'s substring scan; no model is involved.

### Why the alias job costs so much (read, plus estimate)

- **Trigger.** `schedule_alias_job` runs when `name_aliases && background_ready()`. `background_ready()`
  (`core.rs:408`) is true when the model is loaded *or* when it is idle and `autoload` is on, which is the default, so
  the first request loads the model (the `LazyProvider` loads on `acquire`). With Pitwall, the status is always `Ready`
  (`core.rs:740`), so Pitwall is asked and loads its model if needed (`X-Pitwall-Priority: background`, but still a full
  model in memory). ADR 0053 says the job "never loads a model by itself"; with default settings it does.
- **Scope.** `NameBook::wants_ai` (`book.rs:405`) accepts every `Scope::Entity` group with ≥ 5 events: executables,
  libraries, files, exceptions, but also **hosts, IPs, URLs, GUIDs, error codes and paths**, which have no "human name".
- **Volume.** 2,000 names / 16 = 125 requests; prompt ≈ 350 tokens; answer ≈ 16 × 40-55 ≈ 650-900 tokens (*estimate*),
  so ≈ 80,000-110,000 generated tokens per full run. At llama.cpp's typical token rates for a 7-9B Q4 model (tens of
  tokens/s on Apple silicon GPUs, single digits on a CPU-only PC; *unverified*, see llama.cpp's Apple-silicon results
  thread https://github.com/ggml-org/llama.cpp/discussions/4167 and the `docs/debrief-perf` branch for real numbers) that
  is tens of minutes on the Mac and hours on a CPU-only Windows PC. The worker runs at below-normal priority
  (`debrief-ai-worker/src/child.rs:194`), which protects CPU scheduling but not memory, memory bandwidth or the GPU.
- **Repeats.** Answers are cached per group *and model version* (`local:<provider id>`, `names_model.rs:56`): changing
  the model or switching between Pitwall and the built-in engine re-asks everything. The book is one global file
  (`<config>/names/book.json`), so a new workspace re-uses earlier answers.
- **Politeness.** Before each request the job waits for queued AI jobs (`Polite`, `names.rs:160`), up to 10 minutes;
  `background_ready()` calls `status()`, which scans the system's processes for the worker's memory (`core.rs:702`,
  "takes up to seconds on a busy Mac").

## 2. Learning human names without the LLM

### What the AI layer actually contributes (read + measured)

ADR 0053's evaluation (`crates/debrief-core/tests/it/names_eval.rs`) uses 28 invented entities with five query kinds.
Spaced, joined and typo queries are already solved by layer 1 (asserted ≥ 95 % top-1). The AI layer is credited only for
**abbreviations** (`gfxbldr`) and **synonyms** (`image editor`), and it is credited with *perfect* fixture answers
(`book(with_ai)` writes the expected abbreviation and synonym as aliases), so the eval's "+15 points" is an upper bound for
a real model. A real 2B-9B model asked "name this: `gfxbldr.exe`" has to guess an in-house program's purpose from its
spelling; the same family of small model made plain-language log search *worse* when unguided
(`2026-10-embeddings-for-logs.md`, "LLM rewrite (2B, unguided)": hit@5 0.60 → 0.50).

### Options compared

Measured with `docs/research/2026-10-28-llm-use-and-names/adr_eval.py` (ADR 0053's ENTS and NEGATIVES, copied) and
`synonyms.py` (30 invented in-house names plus public Windows binary names), Python `model2vec` 0.10.0, weights
`minishlab/potion-*` (MIT). The Rust `model2vec-rs` reads the same weights, so ranks should carry over (*assumption*).

| Option | Quality (measured or estimated) | CPU | Memory | Latency | What the user loses |
|---|---|---|---|---|---|
| **b. Abbreviation rule** (query splits in order into one piece per entity word; each piece starts with the word's first letter and is a subsequence of it; a small table of `gfx`→graphic, `pkg`, `xfer` was measured at 23/24 and then dropped from code: no curated lists, see ADR 0053) | **23/24 top-1** on ADR 0053's abbreviations, 0 matches on its 18 negatives, still 23/24 unique with 2,000 random distractor names built from `words.txt` (*measured*). Miss: `lickchk` (`k` inside `lic|kchk`). | microseconds per entity; linear in groups | none | < 1 ms per query for thousands of names (*estimate*) | nothing the AI layer did for abbreviations |
| **a. Names pack** (TOML via `paddock-packs`, rule 21): `match` spellings → human name, aliases, kind, platform | Exact for what it lists. *Estimate* for Event Viewer: on a typical Windows System + Application log most distinct exe/dll/service names are inbox Windows components (svchost, lsass, spoolsv, wuauserv/wuauclt, TiWorker, MsMpEng, WerFault, dwm, explorer, RuntimeBroker, SearchIndexer, ntdll, KERNELBASE, ucrtbase, clr/coreclr, …) and widely used third-party software (browsers, Office, OneDrive, Teams, GPU drivers, updaters). A pack of ≈ 600-1,000 Windows entries plus ≈ 300 each for macOS and Linux would cover roughly 60-85 % of distinct names and more by event volume on a home or office PC; for logs of the user's *own* applications (Debrief's main use, e.g. the samples' `orderservice.exe`) the names the user asks about are mostly not coverable. Unverified: WP-N0 measures it on the user's own machine without exporting names. | parse once at start, hot reload | ≈ 0.3-1 MB | lookup in the existing index | nothing; gains names that the model only guessed |
| **c. Model2Vec similarity** (query vs. group words + pack/metadata descriptions) | ADR set synonyms: potion-base-8M **top-1 10/24, top-3 17/24**; potion-base-32M 12/24, 16/24; retrieval-32M 8/24, 14/24. Margins are thin: right answers' median cosine 0.29, the negatives' best median 0.20, but "grape builder" and "tree agent" reach 0.54-0.56, above most right answers. With pack descriptions, well-known Windows names go from 1/12 to **11/12 top-1** ("printer queue"→spoolsv, "antivirus"→MsMpEng, "sign in"→winlogon). "image editor"→graphic builder is rank 2 (0.23, behind "crash reporter" 0.29). (*measured*) | 2,000 names encoded in 9 ms, one query 0.02 ms (*measured*, Python) | ≈ 30 MB model (8M); in-process, no GPU | sub-millisecond | Confident synonyms: usable only as top-3 with a question, never as top-1 |
| **d. Names in the logs** | Exact where present (they are the vendor's own names): Sysmon event 1 (`Image` with `Description`, `Product`, `Company`, `OriginalFileName`), SCM 7045 (`ServiceName`, `ImagePath`) and 7036 (`param1` is the display name), Procmon CSV (`Description`, `Company`; `crates/debrief-plugin-windows/src/procmon.rs:7`), msinfo32 `.nfo` services (Display Name, Name, Path; `nfo.rs`), journald (`_SYSTEMD_UNIT`, `SYSLOG_IDENTIFIER`, systemd's "Started <Description>."), macOS unified log `subsystem`. Application Error 1000 carries only names and versions. Coverage depends entirely on what was collected (*read*; field names from Microsoft's Sysmon and SCM documentation, not re-checked today). | one pass over a few event ids during the catalogue build | a few KB per name | done in the background with the catalogue | nothing |
| **e. LLM as explicit "Suggest names with AI"** | Same as today's model, but over ≤ 16 names the user picks, shown for review | one request | model must be loaded (the user chose to) | seconds to a minute | the automatic answers for names never looked at |

### Recommended design (hybrid, no automatic LLM)

Matching order, all in the engine (`debrief_core::names`, moving to `paddock-names` when Debrief adopts X18):

1. **User facts** (unchanged, outrank everything).
2. **Layer 1** (unchanged) **plus the abbreviation rule** (`How::Abbreviation`, reason "letters of graphic builder").
3. **Known names**: aliases from the logs' own metadata (`How::LogName`, evidence event ids) and from the names pack
   (`How::Pack`, "Windows Print Spooler"), labelled with their source; a log name outranks the pack.
4. **Similar words** (when X55 `paddock-embed` lands): Model2Vec over the group words plus their pack and log
   descriptions. Never a top-1 answer on its own: at most three candidates as a clarifying question, above a floor tuned
   on the eval so that every negative yields nothing.
5. **"Suggest names with AI"**: a user command on a selection (≤ 16 names, one request, interactive priority) or on
   "names nothing else explains", shown for review; only accepted aliases are stored (as user facts, so they never need
   the model again). No background job, no Settings switch: `name_aliases` and the alias job are removed.

Trade-off: the system answers less often with certainty for in-house synonyms and asks more often; in exchange no model
is loaded or run because logs were opened, answers come from sources that can be shown ("from the Sysmon event", "from
the Windows names pack"), and they do not change when the model changes.

## 3. Work packages for Debrief 1.2

Each starts with its failing test (rule 7 / ER-11). Estimates in agent-days.

| WP | What | Failing test first | Estimate | Depends on |
|---|---|---|---|---|
| N1 | Remove the automatic alias job: drop `schedule_alias_job` calls from import and search, the `name_aliases` setting, `Polite`, `LocalAliasModel`'s background use and `background_provider` if unused; keep `AiEntry` reading so existing books load (old AI aliases kept, labelled, removable) | `app/src-tauri/src/names.rs` tests: after `after_import` with a ready `ScriptedProvider`, `provider.requests().len() == 0` and no `Maintenance` job is queued | 0.5 | none |
| N2 | Abbreviation rule in layer 1 (`How::Abbreviation`), in `paddock-names` and Debrief's `names/` (or in `paddock-names` only, if X18 adoption lands first) | `names_eval.rs`: "abbreviation" kind top-1 ≥ 95 % **without** layer 2 (today the test asserts the opposite, `without.top1 <= n/4`); negatives 0 false suggestions; property test: never panics, bounded | 1.5 | none |
| N3 | Names pack: a `names` `PackKind` on `paddock-packs` (`[[name]] match`, `kind`, `human`, `aliases`, `platform`, `source`), built-in `windows.toml`, `macos.toml`, `linux.toml`, `exceptions.toml`, user `packs/`, hot reload, health line | A test pack maps `spoolsv.exe` → "Print Spooler" with alias "printer queue"; `find_values("printer queue")` returns it with `how = pack`; a broken user pack leaves the built-ins and reports the line | 2 code + 3 curation (authored entries from public vendor documentation, no copied third-party lists; ≈ 1,500 entries) | none |
| N4 | Log-name harvest during the catalogue build (Sysmon 1, SCM 7045/7036, Procmon, msinfo32 services, journald units, macOS subsystems) as `How::LogName` aliases with evidence | Import an invented Sysmon XML sample with `Image=C:\Tools\gfxbldr.exe` and `Description=Graphic Builder`: "graphic builder" resolves to the `gfxbldr` group with `how = logName` and the event id as evidence | 2 | N3's alias-source enum |
| N5 | Explicit "Suggest names with AI" (command + Names UI): ≤ 16 selected names, one request at interactive priority, review list, accepted aliases stored as user facts | Command with 40 names is refused with a message; with 3 names exactly one provider request; nothing is in the book until `names_accept` | 1.5 | N1 |
| N6 | Video "Describe frames without a description": limit to a selection of at most 8 frames per job (the user picks), remove "describe all" | `video_describe` with empty `indices` and 50 undescribed frames returns `Invalid` and sends no request; with 8 indices, 8 requests | 0.5 | none |
| N7 | Synonym suggestions with Model2Vec (questions only) | `names_eval.rs`: synonym top-3 ≥ 65 % with every hit asked as a question, negatives 0 suggestions | 1.5 | X55 `paddock-embed` (may slip to 1.3) |
| N0 | Local coverage report in Diagnostics: counts of names explained per layer (pack, log, rule, none), no names leave the machine | Unit test over an invented catalogue: counts per layer | 0.5 | N3, N4 |
| N8 | Docs: ADR amendments, rule 46, lessons, user guide (Names section, AI section) | n/a (docs) | 0.5 | decisions below |

Order: N1 and N6 first (they remove the cost now), then N2, N3, N4, N5, N0; N7 after X55.

### ADR changes

- **Debrief ADR 0053 (amend):** layer 2 becomes "known names" (log metadata, names pack, abbreviation rule); the AI is a
  user-started suggestion with review, not a background job; the evaluation asserts abbreviations without AI; embeddings
  are allowed as a question-only layer (the 2026-10-23 "no embeddings" paragraph is superseded by the 2026-10-25 decision).
- **Debrief ADR 0059 (amend):** the `background` priority for "name aliases" goes; no Debrief feature sends background
  requests.
- **Debrief ADR 0033 §6 (amend):** describing frames is per selection, at most 8 per job; no "describe all".
- **Debrief ADR 0025:** no change (single requests). **ADR 0009 / settings:** see decision 3 below.
- **Paddock `docs/rules.md` (new rule 46)**, draft:

  > 46. **No LLM for large jobs.** Work over many items (imports, catalogues, name tables, indexing, classification,
  > tagging or summaries per item) uses deterministic rules, shipped TOML knowledge packs (rule 21) and static
  > embeddings; an LLM is used only for bounded requests the user starts (a chat turn, a chosen range, an explicit
  > "Suggest with AI" over a small selection), never automatically and never once per item.
  > *Why:* a local model per item turns a cheap background task into hours of GPU and gigabytes of memory on the user's
  > machine (Debrief 1.1 loaded an 8 GB model to name 2,000 log names after every import), its answers change with the
  > model and cannot be explained, and rules, packs and the logs' own metadata were measured as good or better for the
  > same job.

  Plus a lessons entry in `docs/lessons.md`, and the Pitwall/Pacenotes/Sighter AGENTS sections that describe background
  AI should be checked against it (none found on `main` besides Debrief's alias job).

## Decisions for the user

1. Approve the hybrid and removal of the automatic alias job and the `name_aliases` setting (N1), and rule 46's wording.
2. Video: cap "describe" to a selection of ≤ 8 frames (N6), or keep "describe all" as an explicitly confirmed exception?
3. Separate from names but the same symptom: `autoload` is on by default, so the built-in model (≈ 6-8 GB for the 9B)
   loads at start-up even when no AI feature is used. Turn the default off (load on first AI use) in 1.2?
4. Names pack curation effort (≈ 3 days, ≈ 1,500 authored entries) now, or start with ≈ 300 Windows entries?

## Reproduction

```sh
uv venv .venv && uv pip install --python .venv/bin/python model2vec==0.10.0
HF_HOME=$PWD/hf .venv/bin/python -I docs/research/2026-10-28-llm-use-and-names/adr_eval.py
HF_HOME=$PWD/hf .venv/bin/python -I docs/research/2026-10-28-llm-use-and-names/synonyms.py -v
```

All names in the scripts are invented or public Windows binary names; no user logs were used.

## Sources

- Code: `app/src-tauri/src/names.rs`, `app/src-tauri/src/ai/core.rs`, `app/src-tauri/src/ai/requests.rs`,
  `app/src-tauri/src/search_everything.rs`, `app/src-tauri/src/window_manager.rs`, `app/src-tauri/src/video/commands.rs`,
  `app/src-tauri/src/investigate/{incident,issues}.rs`, `app/src-tauri/src/browser/research.rs`,
  `crates/debrief-core/src/names/{alias_job,book}.rs`, `crates/debrief-ai/src/{names_model,settings}.rs`,
  `crates/debrief-ai/data/local-models.json`, `crates/debrief-core/tests/it/names_eval.rs`,
  `crates/paddock-names`, `crates/paddock-packs` (repo root).
- ADRs: Debrief 0009, 0025, 0033, 0053, 0059; Paddock rules 20, 21; `apps/pacenotes/docs/adr/0001-embeddings.md`
  (Model2Vec sizes and speed); `docs/research/2026-10-ai-memory.md` (X55).
- Model2Vec: https://github.com/MinishLab/model2vec, https://github.com/MinishLab/model2vec-rs (MIT).
- Sysmon event 1 fields: https://learn.microsoft.com/sysinternals/downloads/sysmon (read from memory, not re-checked today).
- llama.cpp token rates (secondary, unverified): https://github.com/ggml-org/llama.cpp/discussions/4167
