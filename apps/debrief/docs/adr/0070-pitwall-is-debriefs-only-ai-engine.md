# 70. Pitwall is Debrief's only AI engine: the built-in engine is removed, every AI feature stays

- Status: **Accepted** (decisions by the user, 2026-11-08: Q1 engine only, Q2 keep Model2Vec, Q5 a one-time notice and no
  migration; Q4, Q6, Q7, Q8 and Q9 as recommended). The text is under independent review.
- Date: 2026-11-08
- Supersedes: ADR 0009 (prebuilt llama-server runtimes), ADR 0015 (AI update checks), and in ADR 0059 the engine choice,
  the built-in fallback and "Built-in downloads while Pitwall is used".
- Amends: ADR 0059 (Pitwall is the only engine; the key moves to a file, per family ADR 0057 WP-5), ADR 0025 (decisions
  1 to 3: images go to Pitwall, which owns the projector), ADR 0029 (the knowledge budget uses Pitwall's context), ADR 0017
  (the `debrief-ai` IPC endpoint and the worker protocol go).
- Related: ADR 0011, 0043, 0057 and 0060 (the AI browser, unchanged, now over Pitwall), ADR 0032 (parser generation,
  unchanged), ADR 0055 (one notification service and service health), ADR 0067 (folder import); family ADR 0004
  (`paddock-pitwall`, "Pitwall is optional" as a standard state), family ADR 0013 (workbench), family ADR 0015 (assistant
  memory), family ADR 0034 (one implementation per mechanism), family ADR 0057 (no blocking Keychain; Proposed on
  `origin/docs/adr-keychain-family` `d37b7f452`); Pitwall ADR 0009 (app integration contract) and ADR 0023 rev 3
  (Playground chat, MCP client); Paddock rules 3, 7, 10, 19, 42, 44, 46, 54, 55; ER-11, ER-14, ER-17 to ER-19, ER-27.
- Number: 0070. The highest Debrief ADR on any remote branch is 0069 (`origin/docs/debrief-analysis-usefulness`); 0063
  to 0068 are on open branches (`git ls-tree` over every `origin/*` branch, 2026-11-08). Renumber at landing if another
  branch takes 0070 first.
- Inputs: the researcher's inventory `/Volumes/d7sst7/dev-targets/scratch/debrief-ai-removal/NOTE.md` (2026-11-08, cited
  `note §n`); the Debrief 1.2 plan `docs/plans/debrief-1-2.md` on `origin/docs/debrief-1-2-plan-refresh` `ac85e26c7`
  (cited `plan:<line>`); the Pitwall 1.1 plan on `origin/docs/pitwall-1-1-plan-refresh` `da3c699cc` (cited
  `pw-plan:<line>`); code on `origin/main` `9e18230c5` and `origin/pitwall/1.1-final` `b7151a57f`.
- Evidence marks: **[V]** read in code by the author of this ADR at the commits above; **[N]** taken from the note (which
  marks its own sources); **[J]** judgement; **[U]** not verified. Sizes are raw agent-days, ±50 %. Nothing was built or
  run for this ADR.

## Context

Debrief 1.1 has two engines behind one port (ADR 0059): a built-in engine (a supervised `debrief-ai-worker` sidecar
running a downloaded llama.cpp `llama-server`, with Debrief's own model catalogue, downloads, updates, hardware fit and a
set-up wizard) and Pitwall, the family's local LLM server. The built-in engine exists only because Pitwall was optional
and might not be installed. It is about 16,600 lines across the `debrief-ai` crate, the worker crate, the shell and the UI
[N §1.1], and it puts a second copy of a model on disk and possibly a second model in memory.

On 2026-11-08 the user decided that Pitwall 1.1 ships before Debrief 1.2 and that Debrief 1.2 drops its own engine. Every
AI feature stays in Debrief and runs on Pitwall. Facts that make this a deletion, not a port:

- **Every feature already reaches the model through one port.** `AiCore::provider()` and `debrief_ai::llm::LlmProvider`
  (ADR 0059 Decision) [N §1.2]. The Pitwall adapter (`debrief-ai/src/engine/pitwall/`, 2,842 lines with tests [V]) is
  already in Debrief 1.1.
- **Pitwall already serves what the features ask** [N §0, §3.1]: streamed chat with Cancel, JSON-schema answers
  (`pitwall-core/src/api/openai.rs:106-111`), inline images, embeddings, pairing, `select`, `models`, health and jobs.
  Pitwall 1.1 adds `contextPerSlot` and the "installed but not running" discovery signal [N, V on `1.1-final`]. It also
  sends `X-Pitwall-Job-Id` on a queued stream, so a queued request can be cancelled by id
  (`pitwall-server/src/http.rs:129-133`, `tests/jobs.rs` on `1.1-final`) [V; the note listed this as not verified].
- **"Needs Pitwall" is a state the family already has** [V]. `paddock_pitwall::AiState` has these variants: `Disabled`,
  `NotInstalled`, `NotRunning`, `Incompatible`, `NeedsPairing`, `Paused`, `NoSuitableModel { needs }`, `Unavailable` and
  `Ready { vision, context_tokens }` (`crates/paddock-pitwall/src/status.rs:14-58`). `NeedsAiView` turns a state into one
  sentence plus `NextStep` buttons (`status.rs:151-175`; steps in `error.rs:25-45`). `<pd-needs-ai>` renders it
  (`ui/projects/paddock-ui/shell/pitwall/src/needs-ai.ts`), and so does the shell's Pitwall set-up item
  (`paddock-shell/src/setup/items.rs:27`). `paddock-siblings` starts an installed Pitwall (`AppSpec` "pitwall",
  `app.rs:60`).
- **Debrief does not use these shared pieces yet** [V]. No Debrief manifest depends on `paddock-pitwall`,
  `paddock-assistant` or `paddock-siblings`, and no Debrief UI file uses `<pd-needs-ai>`. Moving Debrief's client onto
  `paddock-pitwall` is already plan package M2c (`plan:124`), and the assistant pane on `paddock-assistant` is M3b
  (`plan:156`). The built-in parts of the shared code are `engine.rs` (290 lines), the `built_in_available` and
  `built_in_for_session` fields, the `pitwall_use_builtin` command and the UI's `useBuiltIn` (`status.rs:182-209`,
  `needs-ai.ts:54-68`). Their only other user is `workbench-demo`, which sets `built_in_available: false`
  (`apps/workbench-demo/src-tauri/src/commands.rs:28-29`) [V].
- **Today an entry point is hidden when AI is off.** The e2e test "without AI in this run, its entry points are absent"
  (`app/e2e/ai-entry-points.e2e.ts:119`) checks it [V]. Debrief has no user switch that turns AI features off. The AI
  settings have no such key, so "off" is safe mode only (`settings.rs:60-97`) [V].
- **Old files are already known to the storage manager** [V]. `storage/plan.rs` has three AI categories:
  `ai_runtimes_outdated`, `ai_runtimes` and `ai_models` (`:185-200`, `:238-280`). They reach the engine through `AiPort`
  (`:15-26`). Models live in `<data>/models` unless the user chose `models_dir`; runtimes live in `<data>/ai/runtimes`
  (`app/src-tauri/src/ai.rs:93-95`, `ai/core.rs:79`, `:461-466`).
- **Pitwall's starter models mostly read images.** Its built-in pack lists `vision` for 8 of 11 entries, including every
  Qwen3.5 size and Gemma 4 (`pitwall-core/src/packs/starter.models.toml`) [V]. Pitwall reports vision when a projector
  file sits next to the model (`pitwall-server/src/library.rs:68-97`, `serving.rs:934`) [V]. Whether installing a starter
  model also downloads its projector is [U].

## Decision

### D1. One engine: Pitwall

- Debrief has exactly one production adapter behind `LlmProvider`: the Pitwall client. M2c moves it onto
  `paddock_pitwall::Client`. The `AiEngine` trait, `engine::select`, `EngineChoice` and the `engine` setting are deleted;
  a trait with one implementation and a selector with one answer is noise (YAGNI). `LlmProvider` stays as the port, so
  features stay testable against `llm::fake` and the shared fake Pitwall (`paddock_pitwall::testing`).
- Model choice stays as ADR 0059 wrote it: a Pitwall preset named `debrief` when Pitwall lists one, otherwise
  `auto:chat,<n>k`, plus `,vision` for a request with images. Requests and typed errors are unchanged. Pitwall's model,
  queue, memory and runtime are Pitwall's job, and Debrief adds no mechanism of its own.
- The context budget (ADR 0029's quarter for knowledge, chat history, tool output and samples) uses `contextPerSlot` from
  `POST /x/v1/select` when Pitwall reports it (1.1). Otherwise it uses the `<n>k` that Debrief asked for (1.0.3 and 1.0.4).
- "Never silently mix" (ADR 0059) is now trivial: there is nothing to mix. When Pitwall stops mid-request, the request
  fails with "Pitwall stopped answering". The partial answer is kept and marked incomplete, the state card appears (D3),
  and *Try again* re-sends once Pitwall is ready.
- **Compatibility (Q7).** Debrief speaks Pitwall's client contract `apiVersion` 1 and works with Pitwall 1.0.3, 1.0.4 and
  1.1. The user guide names Pitwall 1.1 as the supported version, because of the re-pair flag (family ADR 0057 WP-4,
  Pitwall's B5, `pw-plan:75`) and `contextPerSlot`. A server with another `apiVersion` is `Incompatible`, with *Update
  Pitwall* or *Update Debrief*. There is no engine to fall back to.
- **The key.** It moves from `keyring` to `paddock-pitwall`'s keys over `paddock-secrets` (`FileSecrets`), as family ADR
  0057 WP-5 asks. That replaces ADR 0059's "never in a file". The work is M2c's (the client move) and family ADR 0057's.
  This ADR adds none. `keyring` leaves Debrief's manifest with it (`app/src-tauri/Cargo.toml:42`) [V].
- **Safe mode** does not probe for Pitwall (as ADR 0059). AI is `Disabled`, and the AI entry points are absent, as
  today.

### D2. Every AI feature stays in Debrief (Q1, Q6)

Kept, on Pitwall: the assistant chat and its read-only engine tools, Explain, Root causes, Summarize, Ask AI in the
filter bar, the proposals (regex, profile, pipeline steps, markers, extractors), parser generation (ADR 0032), folder
set-up, the incident narrative, vision and *Find related logs*, video *describe frames*, the AI browser (research,
read-along, agent mode; ADRs 0011, 0043, 0057, 0060), the drafted search text, knowledge files (ADR 0029), and AI memory
when it is built (family ADR 0015, Y1 after 1.2).

Knowledge files and AI memory stay Debrief's data and are sent as context. Pitwall has no per-app knowledge, and its RAG
waits for Recce (`pw-plan:45`) [N].

**Later, not in this ADR's build:** Debrief offers its read-only tools (`find_events`, `find_values`, `run_query`, `sql`,
`exception_groups`, `get_events`) to Pitwall's chat over `paddock-mcp`, through a loopback listener like Dyno's. Pitwall's
side is generic: it is an MCP client of family apps (Pitwall ADR 0023 rev 3 §MCP) [N]. That will be its own ADR, about
3 to 5 days [N, J].

### D3. "Needs Pitwall" on every entry point, with a next step

**One rule.** An AI entry point stays visible whenever AI is not `Disabled`. On use, it opens its normal place (dialog,
pane or panel). If Pitwall is not ready for that feature, it shows `<pd-needs-ai>` there instead of the AI part. That is
the engine's one sentence and the buttons that fix it, best first. The user's typed question, selection or image is kept.
There is never a dead button and never an error toast (family ADR 0004, `status.rs:1-3`). Everything in the feature that
does not need a model keeps working.

**Per feature, in Rust.** A new `debrief_ai::AiFeature` enum names every entry point. `AiFeature::caps()` gives its needs:
`Caps::chat(8192)` with `json` for the proposal kinds, and with `vision` for the image features. A new query command,
`ai_feature_state(feature)`, returns `Option<NeedsAiView>`. It is
`NeedsAiView::for_state(feature.label(), &AiState::from_view(&link, feature.caps(), safe_mode), "Debrief")`, so a vision
feature on a text-only model gets `NoSuitableModel { needs.vision }`. The sentence is "Pitwall has no model with vision
that suits Debrief. Download one in Pitwall." (`status.rs:137-140`). The UI never decides readiness itself, because the
engine is the source of truth (AGENTS.md "Where logic lives").

**The shared component gets one optional input.** `<pd-needs-ai [view]="...">` shows a given `NeedsAiView` instead of the
store's app-wide one. This is an additive change in `ui/projects/paddock-ui/shell/pitwall`: it waits for T and lands
after Pitwall's L1 (rule 42; the alternative, Debrief drawing `<pd-needs-ai-card>` itself for the vision case, would
copy the component's button logic).

**What each step does in Debrief.** The shared component runs pairing itself (`NextStep::Pair`, the code inline). The app
runs the steps that name a place (`needs-ai.ts:6-25`, the `(step)` output):

| Step | Button (shared wording) | Debrief does |
|---|---|---|
| `InstallPitwall` | Get Pitwall | Opens Pitwall's release page in the default browser (`files_open_url`; the exact URL is AI-3's, [U]) |
| `StartPitwall` | Open Pitwall | `paddock-siblings` `Siblings::open("pitwall")`. A `NotInstalled` outcome turns the card into the `InstallPitwall` state |
| `Pair` | Connect to Pitwall | Run by the component (pairing code, Cancel) |
| `InstallModel` | Download a model in Pitwall | Opens Pitwall. Pitwall accepts no hand-off kinds (`app.rs:60-66`) [V], so there is no deep link to its Models page today |
| `UpdatePitwall` | Update Pitwall | Opens Pitwall (its updater) |
| `UpdateApp` | Update this app | Debrief's *Check for updates* |
| `ChangePermissions` | Change permissions in Pitwall | Opens Pitwall |
| `Wait` | Try again | Probes again at once |

**The entry points.** "Card" means `<pd-needs-ai>` with that feature's view.

| Entry point (where) | Needs | When Pitwall is not ready | Keeps working without Pitwall |
|---|---|---|---|
| Assistant chat (View ▸ AI, *AI: Open the assistant chat*, activity bar) | chat | The pane opens; the card replaces the composer; a typed draft is kept | Saved chats open, read, copy, export, delete |
| *Explain this event (AI)* (row menu, *AI: Explain the selected event*) | chat | The chat pane opens with the question as an unsent draft and the card | Everything else in the row menu |
| *AI: Find likely root causes* (Home), *AI: Summarize this range* (Moment view) | chat | As Explain | Home, Moment view, the deterministic analyses |
| *AI: Ask AI to write a query…* (filter bar `ai-ask-query`, palette) | chat, json | The query dialog opens; the card replaces the answer area; the question is kept | The query field and QL |
| *Suggest a regex for these lines (AI)* (row menu, infer-regex dialog) | chat, json | The dialog's AI button area shows the card | *Infer pattern* (deterministic) and manual regex |
| *AI: Propose markers / pipeline steps / extractors…* (markers manager, profile editor, palette) | chat, json | The proposal dialog shows the card | The manual editors |
| *Generate a parser with AI* (format wizard, parser-suggest notice) | chat, json | The AI step shows the card | The parser library and the wizard's manual path |
| Folder set-up with AI (ADR 0067 folder import) | chat, json | The AI proposal step shows the card next to *Continue without AI* | Folder import with built-in parsers |
| Incident narrative (narrative dialog, report) | chat | The narrative block shows the card | The report without a narrative (as the CLI's `debrief report`) |
| *AI: Find logs related to an image…*, *AI: Open an image…*, an image in chat | chat, vision | The card; on a text-only model, the vision sentence with *Download a model in Pitwall* | The image view (sanitised copy, its EXIF time); images in notes |
| Video *describe frames* (behind its confirmation, N6) | chat, vision | The card in the confirmation | Video import, frames, timeline |
| Research, read-along, agent mode, the model's `web_research` (AI browser) | chat | The research dialog or chat shows the card; the browser is not started | *Search the web for this error* |
| *Search the web for this error* | none | Never shows the card: it uses the deterministic search text (features.md) | Everything; with Pitwall the model drafts the text |
| Settings ▸ AI ▸ Knowledge | none | A line says the files are used when Pitwall answers | Edit, import, export, enable |
| Settings ▸ AI | none | The Pitwall connection (state sentence, *Connect to Pitwall…*, *Forget Pitwall*) and the request settings | All of it |
| Status bar AI button, health entry `ai` | none | Health `ai` is optional and degraded, with the state's sentence and its step as the repair (rule 19: no repair depends on Pitwall running) | n/a |
| First-run set-up (shell item `pitwall`) | none | Offered when Pitwall is found; skipping it is fine | n/a |
| Name aliases in the background | (removed) | N1 removes the job (folded into AI-1) | Names without aliases; old aliases still load |
| CLI `debrief` | none | No AI commands [V: no `debrief_ai` use outside the app and `debrief-video`] | All of it |
| Safe mode | none | AI `Disabled`: the entry points are absent, as today | All non-AI features |

Settings ▸ AI keeps registered settings (the request settings `context_length`, `max_tokens`, `temperature`, and the
connection), so rule 44 holds.

### D4. What is deleted

Line counts from the note [N §1.1], files [V] at `origin/main`. Anything on this list that the kept code still needs is
moved, not deleted (D5). The compiler decides, and the implementer records each such case in the commit.

**Crates and binaries**

- `crates/debrief-ai-worker` (4,049 lines, with `fake_llama_server` and `debrief-ai-fake-worker`). Also its entry in
  `Cargo.toml` and `Cargo.lock`, `tauri.sidecars.conf.json:7`, `app/scripts/build-sidecars.mjs:27` and its test,
  `paddock.ci.json` `notices.manifests`, `.config/nextest.toml:36`, `.vscode/tasks.json:46-47`,
  `scripts/ci/macos-smoke.sh`, `macos-smoke.test.mjs` and `macos-verify-bundle.sh` (one sidecar fewer), `vm-smoke.json`,
  and `debrief-ipc`'s `endpoint::AI` (`endpoint.rs:66`).
- Fuzz targets `fuzz_worker_http` and `fuzz_ai_request`, from `fuzz/Cargo.toml`, `fuzz/Cargo.lock`, `docs/fuzzing.md:68`
  and the target list in `.gitea/workflows/debrief-fuzz.yml:47`. That last one is a shared path, one declared line by the
  integrator, in the same landing.

**`crates/debrief-ai/src`**

- `worker/` (1,600), `llm/lazy.rs` (227), `runtime.rs` (155), `runtimes/` (2,769 with `llama-runtimes.json`), `catalog.rs`
  and `data/local-models.json`, `gguf.rs` (713), `manual.rs`, `hardware.rs`, `fit/`, `recommend.rs`, `dxgi.rs`, `setup.rs`,
  `updates/`, `engine/mod.rs` (the trait and `select`), `names_model.rs` (with N1).
- In `models/`: everything model-specific. The generic download pieces that the FFmpeg runtime and the Chromium download
  still use stay until M2c ports them onto `paddock-download` (D5). The FFmpeg runtime uses `debrief_ai::models` and
  `runtimes::catalog` (`debrief-video/src/runtime/{catalog,store}.rs`), and the Chromium download uses `debrief_ai::net`
  (`browser/runtime.rs`) [V].
- `llm/prompt_format.rs`, if only the worker path renders prompts with it (`worker/supervisor.rs` and `llm/prompted.rs`
  use it [V]; Pitwall applies the chat template itself).
- The settings keys `active_model`, `autoload`, `idle_unload_minutes`, `gpu_offload`, `threads`, `hf_endpoint`,
  `download_proxy`, `max_download_gb`, `download_connections`, `models_dir`, `inference_build`, `llama_server_path`,
  `runtime_updates`, `model_updates`, `update_check_hours`, `name_aliases` and `engine` (`settings.rs:60-97`).
  `SETTINGS_VERSION` goes up by one. A 1.1 `ai.json` still loads, because serde ignores unknown keys: the removed keys are
  dropped and named in one log line, with no conversion. This is not a migration; the kept keys mean the same.
- `examples/fetch_model.rs`. `examples/eval.rs` is kept and re-pointed (D7).

**Shell (`app/src-tauri/src`)**

- `ai/runtimes.rs`, `ai/runtime_commands.rs`, `ai/updates.rs`, `ai/update_commands.rs`, `ai/manual.rs` and
  `ai/assistant_setup.rs`, with their tests (`ai/tests/update_checks.rs` and the worker parts of `ai/tests.rs`).
- The engine parts of `ai/core.rs` (worker spawn, models, runtimes; `:1231` finds the worker child), `ai/commands.rs`
  (model, fit and recommend commands) and `ai/settings_section.rs` (engine and runtime fields).
- The `pitwall_use_builtin` repair and every "Use the built-in engine" text.
- In `storage/plan.rs`: the `AiPort` model and runtime methods and the three AI categories. One `old_ai_files` category
  replaces them (D6).

**UI (`app/src/app`)**

- `ai/runtimes`, `ai/models`, `ai/setup`, `ai/updates` and `ai/manual`, and the palette item *AI: Manage models…*.
- The engine choice and the worker status in `ai-status.store.ts`, `ai.types.ts`, `ai-lazy.ts` and `ai/settings`.
- The `ai-setup` entry in the dialog registry, which removes one of M2b's five fixed-height forms (`plan:133`).

**End-to-end tests**

- `ai-runtime`, `ai-models`, `ai-setup`, `ai-updates` and `ai-manual-download` go.
- `ai-pitwall` loses "Download a model for Debrief instead" (`:40`).
- `ai-vision` "a text-only model offers its image support" (`:95`) becomes the vision state.
- `ai-entry-points` "…entry points are absent" (`:119`) stays for safe mode only. A new test covers the visible states (D7).
- The worker references in `activity-report.e2e.ts` and `export_diagnostics.rs` change.

**Registers**

- `docs/known-copies.toml`:
  - the Debrief members of `loopback-http-client` for `debrief-ai-worker/src/http.rs` go in AI-1;
  - the member for `engine/pitwall/http.rs` goes in M2c;
  - `model-download` goes in M2c, when the last caller of `models/download.rs` moves onto `paddock-download`;
  - `prompted-tools` goes in M3b (`:525-605`).
- Every deletion removes its member in the same commit (rule 55).
- `paddock.waivers.toml`: rows `:44`, `:50` and `:118` go in AI-1 (worker `child.rs` and `http.rs`). Row `:62` goes when
  `tests/it/net_tls.rs` goes with the model download, in AI-1 if no kept downloader needs it, otherwise in M2c. Row `:56`
  goes in M2c (`engine/pitwall/http.rs`). That is 5 of Debrief's 15 rows [V]. The two `chromium-driver` rows stay with
  the browser (X6).

**Not deleted:** the AI browser and `chromium-driver` (D2; X6 still moves it onto `paddock-chromium`), `paddock-pitwall`
itself, and the planned Model2Vec search (D8).

### D5. Where the kept code goes

| Kept code (note §1.3) | New home | Why | When |
|---|---|---|---|
| Image sanitising and EXIF (`vision/image.rs` 460, `exif.rs` 226), fuzz target `fuzz_image` | New app crate `debrief-image` (pure; `image` with the five formats, as ADR 0025 decision 4) | Notes (`notes/images.rs:7`, `notes/commands.rs:221`) and video (`debrief-video/src/describe.rs`, `runtime/selftest.rs`) use it without AI; `debrief-core` does not depend on `image` today [V] | AI-2 |
| Leak guard and untrusted-web marking (`web/`, 1,003) | Stays in `debrief-ai` | Only AI-bound text uses it (ADR 0059 Privacy) | n/a |
| Knowledge files (`knowledge/`, 777) | Stays in `debrief-ai` | Prompt context (D2) | n/a |
| `StoreAccess`, errors, settings (`access.rs`, `error.rs`, `settings.rs`) | Stay in `debrief-ai`, shrunk; `AiError::{NoModel, NoRuntime}` give way to the Pitwall states (`debrief-video/src/error.rs` maps them [V]) | Used by `investigate/`, the tools and video | AI-1 |
| Disk-space probe (`models/disk.rs`, 90) | Shell, `performance/disk.rs` | Its one user is `performance/suggest.rs:147` [N]; a shared home in `paddock-metrics` would wait for T for no gain (rule of three) | AI-2 |
| Generic downloads (GitHub releases, segmented download, verify, `net.rs` TLS and proxy errors) | Stay in `debrief-ai` until M2c ports FFmpeg and Chromium onto `paddock-download`, then deleted | Moving them twice is waste; M2c already owns the port (`plan:124`, workbench.md §16) | M2c |
| Prompt evals (`evals/cases.toml`, `make_cases.py`, `examples/eval.rs`) | Stay; `eval.rs` drops its `gguf` and `hardware` use and runs against Pitwall | Q9: local quality checks (D7) | AI-1 |

### D6. The one-time notice for old AI files (Q5)

There is no migration and no import into Pitwall. Pitwall's own *Add local GGUF* covers anyone who wants to keep a model.

- **One storage category.** `old_ai_files` ("Old AI files") replaces `ai_runtimes_outdated`, `ai_runtimes` and
  `ai_models` in `storage/plan.rs`. It covers:
  - `<data>/models` (models, `.mmproj.gguf` projectors, partial downloads);
  - `<data>/ai/runtimes`;
  - the leftover catalogue and update-state files in `<config>/ai`, except `ai.json` and `knowledge/`.

  It uses the existing walk, budget and confinement (`storage/walk.rs`), so sizes, Settings ▸ Storage and the notice show
  the same numbers from one mechanism.
- **A folder the user chose.** If the 1.1 `ai.json` names a `models_dir` outside the data folder, the category lists it
  and its size but never deletes in it: "In a folder you chose: delete these yourself", with *Show in folder*. Debrief
  reads that one key raw, for this notice only. Links are removed, never followed.
- **The notice.** After the first window is painted, a background check (rule 10) runs. If the category is not empty
  and the notice has not been answered, one notice goes through the notification service (ADR 0055): "Debrief no longer
  runs AI models itself. AI now runs in Pitwall. <N> GB of old model and runtime files can be deleted." (`<N>` from the category's walk) The buttons are
  **Delete old AI files** and **Keep**.
  - Delete runs as a job with live progress and Cancel (ER-17, ER-18). A cancelled run leaves what it has not reached.
  - Either answer is recorded in a versioned UI-state flag, so the notice never comes back. *Keep* leaves the category
    in Settings ▸ Storage for later.
  - An install with no old files never shows it.
- **Logs (ER-27).** The check logs the bytes and counts found and the answer. The job logs its phases, durations, counts
  and bytes removed, and each refusal with its reason. No file names.

### D7. Tests

Failing tests first (ER-11), with invented data and temporary folders only (ER-14, rule 7: never the real data folder,
port 11435, the credential store or an installed Pitwall).

1. **Feature table (Rust, pure).** For every `AiFeature` variant (an exhaustive `match`, so a new feature cannot skip
   it) and every `AiState`, `ai_feature_state` gives the expected `NeedsAiView`. A vision feature on
   `Ready { vision: false }` gives `NoSuitableModel` with vision. `Ready` with vision gives `None`. Safe mode gives
   `Disabled`.
2. **No engine left.** A 1.1 `ai.json` with `engine: "builtIn"`, `active_model`, `inference_build` and `models_dir` loads.
   The kept keys keep their values, the removed keys are named in the log, and the saved file has none of them.
3. **Contract against the shared fake Pitwall** (`paddock_pitwall::testing`), for one feature of each kind (chat, json,
   vision, research). The request carries the expected model string (`debrief` preset, `auto:chat,8k`, `,vision`) and
   the knowledge budget from `contextPerSlot`. If the fake has no `contextPerSlot` (1.0.x), the asked context is used.
   The fake dying mid-stream gives "Pitwall stopped answering", keeps the partial answer and shows the state.
4. **Old files (D6).** A temporary data folder with invented files (a few-byte `.gguf`, a projector, a runtime folder,
   a link to a file outside) gives the category's size. Delete removes only inside the data folder: the link goes, its
   target stays. A `models_dir` outside is listed and never touched. Cancel mid-way leaves a consistent folder. The
   answered flag stops the notice, and an empty category never shows it.
5. **End-to-end `ai-entry-points`.** The mock Pitwall backend is set to `NotInstalled`, `NotRunning`, `NeedsPairing`,
   `NoSuitableModel` and Ready without vision. For each row of D3's table, the card shows the engine's sentence and
   buttons, the user's input is kept, and the "keeps working" column works. Safe mode still hides the entry points.
6. **End-to-end `ai-pitwall`.** Pairing works from Settings ▸ AI and from a card. No text anywhere offers a model
   download in Debrief (a text search over the built UI strings).
7. **Nothing left behind.** `build-sidecars.test.mjs` and `macos-smoke.test.mjs` expect two sidecars.
   `cargo tree -p debrief-app` has no `debrief-ai-worker` and no `keyring` (after M2c). The known-copies register check
   (C58) is green.
8. **Health.** Health `ai` is optional. When degraded, its repair is the state's step and never "Use the built-in
   engine".
9. **Shared input.** `<pd-needs-ai [view]>` gets a spec in `paddock-ui` (components 100 %) and a gallery page in both
   themes, with the screenshot independently reviewed (rule 22).
10. **Evals.** `cargo build --example eval` passes in CI. Running the evals against a real Pitwall stays a local check,
    never in CI (Q9).
11. **Release path (rule 54).** The sidecar list and bundle checks change Debrief's release path. AI-1 dispatches
    `debrief-release.yml` with gates on and the bundle built before it lands on `main`.

### D8. Model2Vec search stays planned (Q2)

The small embedding search (Model2Vec over message templates, fused with BM25; X55 `paddock-embed` and Y2) is not an
LLM. It is an algorithm: about 3 MB of code, a 32 MB model, and milliseconds per query. It works without Pitwall and is
allowed by rule 46 [N §1.4]. It stays built in, in its release after 1.2. When Pitwall runs, Pitwall's embedder
(Qwen3-Embedding, `/v1/embeddings`) is used as well. No code for it exists today [N, V: no `model2vec` in `apps/debrief`],
so nothing in this ADR's build touches it.

### D9. The shared `paddock-pitwall` cleanup waits for Pitwall's base

The built-in hooks in shared code lose their last real user once AI-1 lands. Those hooks are `engine.rs`, the
`built_in_available` and `built_in_for_session` fields of `AiSnapshot`, `pitwall_use_builtin` in `COMMANDS`, and
`useBuiltIn`, `canUseBuiltIn`, `onBuiltIn` and their texts in `ui/.../shell/pitwall`.

Package AI-5 removes them as one breaking shared change (AGENTS.md "Shared code": a breaking change updates every app in the same change): `workbench-demo` and the generated TypeScript
change in the same change, and the CHANGELOG gets an entry. It lands only after Pitwall's L1 (Pitwall's base landing on
`main`) and after M2c. L1 changes `paddock-pitwall` first (`pw-plan:232`), and removing the hooks before it would make
L1 merge the removal. Until then, Debrief passes `built_in_available: false`, as `workbench-demo` does. It is optional
for 1.2: dead fields cost nothing at run time.

### Logs (ER-27)

- Pitwall link state changes (from, to, reason).
- Each request's start and outcome with a correlation id, plus Pitwall's `X-Pitwall-Job-Id` and `X-Pitwall-Model` when
  present.
- Pairing outcomes.
- The old-files check and job (D6).
- No prompt, answer, knowledge text or key, as today.

## Consequences

- One engine and one model on disk and in memory for the family. Debrief loses about 16,600 lines, one sidecar (2.41 MB
  uncompressed on macOS arm64 [N, built]), one IPC endpoint, two fuzz targets and five e2e specs. Users no longer
  download llama.cpp builds of 12 to 767 MB or models of 1.3 to 22 GB through Debrief [N §4.1].
- **AI is a second install.** Without Pitwall, every AI feature shows *Get Pitwall* until the user installs Pitwall,
  gets a model there and connects. Debrief's non-AI features are unaffected.
- Debrief's AI quality follows Pitwall's model choice. The evals are the local guard.
- No third-party crate leaves with `debrief-ai` and the worker, and no licence class changes [N §4.2]. `keyring` leaves
  with M2c.
- One copy register member and three waiver rows go in AI-1. Two more members and two more rows go in M2c and M3b
  without a port.
- ADR 0009 and 0015 are superseded. ADR 0059, 0025, 0029 and 0017 carry an amendment line pointing here.

## Work packages

**T** is the first full-speed day after the Dyno tag (plan §9.2). "Waits" means it changes shared code and lands only
after T.

| # | Package | Role | Depends | Waits | Days | Failing test first |
|---|---|---|---|---|---|---|
| AI-0 | This ADR, then its independent review | designer, reviewer | - | no | 1-1.5 | (design) |
| AI-1 | Delete the engine (D4) with N1 folded in: the crate, the modules, the shell and UI parts, the settings keys, the sidecar and bundle checks, fuzz targets, e2e specs, the register members and waiver rows; `eval.rs` re-pointed; rule 54 dispatch. The `debrief-fuzz.yml` line is one declared line by the integrator | implementer | AI-0 accepted | no (app-only; the one workflow line through the integrator) | 3-4 | D7 tests 2, 7 and 10 |
| AI-2 | Move the kept code (D5): `debrief-image` with `fuzz_image`, the disk probe to the shell, `AiError` cleanup in `debrief-video` | implementer | AI-1 | no | 1.5-2 | `fuzz_image` and the notes image tests pass from the new crate; a damaged image is still refused |
| AI-3 | "Needs Pitwall" on every entry point (D3): `AiFeature`, `ai_feature_state`, the step handlers, the card on each row of the table, health `ai`, the shared `[view]` input | implementer | M2c, Pitwall's L1 | **yes** (the `[view]` input) | 2-2.5 | D7 tests 1, 3, 5, 6, 8 and 9 |
| AI-4 | The old-files category and notice (D6); docs: features.md, the user guide (Pitwall 1.1 supported), `docs/fuzzing.md`, the CHANGELOG, third-party notices | implementer | AI-1 | no | 1-1.5 | D7 test 4 |
| AI-5 | Shared cleanup (D9), optional in 1.2 | implementer | AI-1, M2c, Pitwall's L1 | **yes** | 0.5-1 | `COMMANDS` has three entries; the snapshot has no built-in fields; `workbench-demo` builds |
| | **Total** | | | | **8.5-11.5** (AI-5 adds 0.5-1) | |

**Order.** AI-1, AI-2 and AI-4 are app-only and start now, before T, on the integration branch. That way M2c and M3b are
built Pitwall-only from their first line (Q8). AI-3 follows M2c, because it renders the shared `<pd-needs-ai>` from
`paddock-pitwall`'s snapshot. Between AI-1 and AI-3, the integration branch shows Debrief 1.1's existing Pitwall messages
(no release is cut from it).

## Plan delta (Debrief 1.2 plan, `ac85e26c7`)

| Package | Change | Days |
|---|---|---|
| AI-0 to AI-4 | New (above) | +8.5 to +11.5 |
| AI-5 | New, optional, shared | (+0.5 to +1) |
| N1 (`plan:253`, 0.5) | Folded into AI-1 | -0.5 |
| PH1 (`plan:125`, 3.5) | No AI worker to move onto `paddock-ipc` and `paddock-proc` or to self-test; `crates/debrief-ai/src/worker/` leaves its file list | -1 |
| LV1 (`plan:161`, 3) | `CONNECT_WINDOW` and the runtimes' `REPORT_TIMEOUT` go | -0.75 |
| BP1 (`plan:226`, 2) | llama-server's class and `--threads` go; `debrief-ai-worker` and `models/priority.rs` leave its file list | -0.25 |
| X7 (`plan:531`, 5) | 5 of 15 Debrief waiver rows are deleted with their files, not fixed | -1.5 to -2 |
| M2c (`plan:124`, 3) | The Pitwall client moves onto `paddock-pitwall` with no engine choice or fallback; the downloader port covers FFmpeg and Chromium only | -0.5 to -1 |
| M3b (`plan:156`, 2.5) | The assistant pane uses `paddock-assistant` with Pitwall only; "the built-in llama.cpp adapter kept (workbench.md D5)" is struck | -0.5 |
| M2b (`plan:133`) | `ai-setup` leaves the five fixed-height forms | small, not counted |
| X6, Y2 and X55, 2.7, FD-E2, M5 | Unchanged (the browser stays; Model2Vec stays; citations unchanged). M5 and the release checklist start one sidecar fewer | 0 |
| **Saved** | | **-5 to -6** |
| **Net** | | **+2.5 to +6.5 raw** (+3 to +7.5 with AI-5) |

Effect on the plan's totals (§9.5, after the release split: 510.5 raw, 612.6 with 20 %, about 2027-04-21 at 5.5 agent-days
a day):

- The total becomes **about 513 to 517 raw**, or 615.6 to 620.4 with 20 %.
- That is **+0.5 to +1.4 working days**, so the end is still about 2027-04-21 to 2027-04-23 [calc].
- The note's +2 to +5 used a saved range of 5 to 6.5 [N §4.3]. Summing its own rows gives 5 to 6, and AI-4 here also
  carries the notice's job and category, so this ADR's net is about 0.5 to 1.5 days higher.

**Plan text to change when this ADR is accepted:**

- M3b's "built-in llama.cpp adapter kept";
- PH1, LV1 and BP1's file lists;
- N1 marked as folded into AI-1;
- §9.2 Layer 0 gains AI-1, AI-2 and AI-4 (app-only);
- Layer 5 gains AI-3 and AI-5 (wait);
- §9.5 gains one row.

The plan's owner makes the edit on the plan branch. This ADR does not touch the plan file.

## Risks

1. **Pitwall must be released, on all three OS, before Debrief 1.2.** Pitwall's tag is estimated for 2026-12-18 to
   2027-01-13 and is likely in January; Debrief 1.2 is about 2027-04-21, which leaves about three months [N §4.5].
   Pitwall 1.1's Windows build has never run on a real machine (`pw-plan:38`). If Pitwall 1.1 slips, Debrief still works
   with 1.0.3 and 1.0.4 (`apiVersion` 1); only the re-pair flag is 1.1-only. Debrief 1.2's release checklist gains a row:
   a released Pitwall exists for each OS.
2. **Vision depends on a model in Pitwall.** Eight starter entries list vision [V], but whether installing one also
   fetches its projector is [U]. If it does not, *Find related logs* and *describe frames* show the vision state until
   the user adds the projector in Pitwall. AI-3 checks this first with a scout lookup, and if needed asks Pitwall for a
   roadmap item (Debrief adds no workaround).
3. **The shared `[view]` input waits for T and L1.** If L1 slips, AI-3 slips with it. Fallback: AI-3 ships the non-vision
   rows first (the app-wide store state is enough for them), and the vision rows follow.
4. **Behaviour drift in M3b's move to `paddock-assistant`** (budgets, the SQL tool, citations). The `prompted-tools`
   canonical was taken from Debrief's code [N]. M3b's `ai-chat` and `ai-entry-points` e2e are the check. This ADR does
   not add to that risk, but it removes the second adapter that once kept the behaviour honest.
5. **Kept downloader code lingers until M2c.** If M2c slips past 1.2, the `model-download` member still expires on
   `debrief-v1.2.0`, and the downloader port must not be dropped from M2c.
6. **Old data.** GBs in 1.1 users' data folders. The notice and the storage category make them visible and deletable.
   Nothing deletes them silently, and nothing in a folder the user chose is deleted at all.
7. **Ordering of shared code.** AI-5 after L1 and M2c (D9); the `debrief-fuzz.yml` line through the integrator; the
   re-pair flag (Pitwall's B5) before Debrief's family ADR 0057 WP-5 (`pw-plan:246`).
8. **The safe-mode test changes meaning.** "Absent" now means only safe mode. A missed entry point would show as a
   visible but unhandled control. D7 test 1's exhaustive `match` and test 5's table guard it.

## Not verified

From the note (§6), still open:

- The share of `debrief-ai` in the `debrief-app` binary. Measuring it needs a full release build with libduckdb and
  `cargo bloat`.
- The worker's size on Windows and Linux.
- Whether `paddock-assistant` covers every budget and tool behaviour of Debrief's loop. That is M3b's job.
- Any Debrief 1.1.x installer sizes: no `debrief-v1.1.*` release assets were found.

From the note, now settled or narrowed:

- `X-Pitwall-Job-Id` on streamed answers: **verified present** on `pitwall/1.1-final` (`http.rs:129-133`,
  `tests/jobs.rs`).
- A vision model in Pitwall's starter set: **8 of 11 entries list vision** [V]. Whether installing one also fetches its
  projector remains [U] (risk 2).

Found while writing this ADR:

- Which exact files in `models/` and `runtimes/` FFmpeg and the Chromium download still reach. D5 lets the compiler
  decide.
- The URL behind *Get Pitwall*.
- Family ADR 0057's state. It is Proposed on its branch, and the memory records D1 to D4 as accepted with Pitwall 1.1.
  Whether its WP-5 is counted inside the plan's "Decided 2026-11-05, +41" row is [U]. If it is not, M2c carries it.
- Whether Pitwall will add a hand-off to its Models page. *Download a model in Pitwall* only opens Pitwall today.
