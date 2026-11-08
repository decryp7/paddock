# 70. Pitwall is Debrief's only AI engine: the built-in engine is removed, every AI feature stays

- Status: **Accepted** (decisions by the user, 2026-11-08: Q1 engine only, every AI feature stays, which keeps the AI
  browser over Pitwall (the note's Q3); Q2 keep Model2Vec; Q5 a one-time notice and no migration; Q4, Q6, Q7, Q8 and Q9 as
  recommended). Revision 2 answers the independent review of `b1872a1c5` (4 P1s, P2s).
- Date: 2026-11-08
- Supersedes: ADR 0009 (prebuilt llama-server runtimes), ADR 0015 (AI update checks), and in ADR 0059 the engine choice,
  the built-in fallback and "Built-in downloads while Pitwall is used".
- Amends: ADR 0059 (Pitwall is the only engine; the key moves to a file, per family ADR 0057 WP-5), ADR 0025 (decisions
  1 to 3: images go to Pitwall, which owns the projector), ADR 0029 (the knowledge budget uses Pitwall's context), ADR 0017
  (the `debrief-ai` IPC endpoint and the worker protocol go), ADR 0031 (the AI worker's memory cap goes), ADR 0033 (the
  FFmpeg runtime keeps the downloader until M2c), ADR 0053 (layer 2, the AI alias job, goes with N1), ADR 0055 (the AI
  worker's process entry and health wording), ADR 0002 and ADR 0012 (the AI worker as the first supervised worker, its
  quarantine pause and model and runtime downloads as jobs).
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
- **Readiness is app-wide today, and it ignores vision** [V]. The Link probes once with the app's one `needs`
  (`LinkConfig::new` sets `Caps::chat(8192)`, `crates/paddock-pitwall/src/service.rs:152`), selects
  `ModelSel::Auto(needs)` and records whether the picked model can read images (`service.rs:376-406`).
  `AiState::from_view` (`status.rs:62-96`) never compares `needs` with `view.vision`: a usable link is `Ready` whatever
  the feature asks, and `NoSuitableModel { needs }` only echoes the app-wide needs. So a vision feature on a text model
  is `Ready` and sends images, and a card can say "no model with vision" when Pitwall has one that the chat pick did not
  choose. Debrief 1.1's own client asks `auto:chat,<n>k` for every request (`engine/pitwall/provider.rs:36`), even for
  the JSON-schema features. Readiness and requests must use the same capabilities (D1, D3).
- **Pitwall's starter models mostly read images.** Its built-in pack has 10 models, 8 of them with `vision`, including
  every Qwen3.5 size and Gemma 4 (`pitwall-core/src/packs/starter.models.toml`) [V]. Pitwall reports vision when a
  projector file sits next to the model (`pitwall-server/src/library.rs:68-97`, `serving.rs:934`) [V]. The projector comes
  along when the user downloads from Discover, whose "Include the vision projector" toggle is on by default
  (`app/src/app/features/models/discover-tab.ts:144-148`, `:388`, `:447`) [V]; one-click recommendations download it too,
  and `pitwall pull` sends no projector [review, not re-checked].

## Decision

### D1. One engine: Pitwall

- Debrief has exactly one production adapter behind `LlmProvider`: the Pitwall client. M2c moves it onto
  `paddock_pitwall::Client`. The `AiEngine` trait, `engine::select`, `EngineChoice` and the `engine` setting are deleted;
  a trait with one implementation and a selector with one answer is noise (YAGNI). `LlmProvider` stays as the port, so
  features stay testable against `llm::fake` and the shared fake Pitwall (`paddock_pitwall::testing`).
- **One set of capabilities per request.** Each request names its `AiFeature` (D3), and the model it asks for comes from
  that feature's `caps()`:
  - the Pitwall preset `debrief` when Pitwall lists one (Pitwall ADR 0014: the user decides what serves Debrief);
  - otherwise `ModelSel::Auto(feature.caps())`, which spells `auto:chat,json,8k` for a JSON-schema feature and
    `auto:chat,vision,8k` for an image feature.

  Readiness (D3) asks Pitwall with the same `ModelSel`, so a feature is never called ready for one model and sent to
  another. That replaces 1.1's single `auto:chat,<n>k`. Requests, typed errors, timeouts and Cancel are otherwise ADR
  0059's. Pitwall's model, queue, memory and runtime are Pitwall's job, and Debrief adds no mechanism of its own.
- **Context.** The budget (ADR 0029's quarter for knowledge, then chat history, tool output and samples) uses the
  context the readiness check reports for that feature: Pitwall's `contextPerSlot` (1.1) or, when Pitwall does not report
  it (1.0.3 and 1.0.4), its `contextMax`, in both cases capped at the context Debrief asked for and at least 1,024
  (`ModelInfo::usable_context`, `wire.rs:121-123`; `service.rs:399-405`) [V].
- "Never silently mix" (ADR 0059) is now trivial: there is nothing to mix. When Pitwall stops mid-request, the request
  fails with "Pitwall stopped answering". The partial answer is kept and marked incomplete, the state card appears (D3),
  and *Try again* re-sends once Pitwall is ready.
- **Compatibility (Q7).** Debrief speaks Pitwall's client contract `apiVersion` 1 and works with Pitwall 1.0.3, 1.0.4 and
  1.1. The user guide names Pitwall 1.1 as the supported version, because of the re-pair flag (family ADR 0057 WP-4,
  Pitwall's B5, `pw-plan:75`) and `contextPerSlot`. A server with another `apiVersion` is `Incompatible`, with *Update
  Pitwall* or *Update Debrief*. There is no engine to fall back to.
- **The key.** It moves from `keyring` to `paddock-pitwall`'s keys over `paddock-secrets` (`FileSecrets`), as family ADR
  0057 WP-5 asks. That replaces ADR 0059's "never in a file". The work is M2c's (the client move) and family ADR 0057's.
  This ADR adds none. `keyring` leaves Debrief's manifest with it (`app/src-tauri/Cargo.toml:42`) [V]. Family ADR 0057
  assumed Debrief 1.2 ships before Pitwall 1.1 (its `:322-323`: until then, Debrief's notes tell the user to revoke the
  old key by hand). With Pitwall now first, that assumption no longer holds: Debrief's re-pair sends the flag and Pitwall
  revokes the old key, so the hand-revoke note is not needed. Family ADR 0057 should be amended to say so; that is its
  owner's change, not this ADR's.
- **Safe mode** does not probe for Pitwall (as ADR 0059). AI is `Disabled`, and the AI entry points are absent, as
  today.

### D2. Every AI feature stays in Debrief (Q1, Q6)

Kept, on Pitwall: the assistant chat and its read-only engine tools, Explain, Root causes, Summarize, Ask AI in the
filter bar, the proposals (regex, profile, pipeline steps, markers, extractors), parser generation (ADR 0032), folder
set-up, the incident narrative, vision and *Find related logs*, video frame use, the AI browser (research, read-along,
agent mode; ADRs 0011, 0043, 0057, 0060), the drafted search text, known-issue suggestions, the dump, UMDH and profiling
*Explain with AI*, knowledge files (ADR 0029), and AI memory when it is built (family ADR 0015, Y1 after 1.2).

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

**Readiness per capability set, in `paddock-pitwall` (rule 42; shared, waits for T).** The Link gains
`Link::state_for(caps: Caps) -> AiState`:

- It answers from a per-`Caps` cache filled by `select` with the same `ModelSel` the request will use (D1). The cache
  holds at most 8 entries (one per distinct capability set; Debrief has three: chat, chat+json, chat+vision). An entry
  is dropped when the probe sees a change (models, pairing, serving state) and refreshed on the Link's existing
  background probe, never on the UI or an IPC thread (rule 10). A cold call returns the app-wide state until its first
  answer lands and then publishes the `paddock://pitwall` event again.
- `NoModelFits` from that `select` (`error.rs:71`, `:259`) becomes `NoSuitableModel { needs: caps }`, so the sentence
  names what is missing ("…no model with vision that suits Debrief…", `status.rs:137-140`).
- `Ready` carries the model and context for those capabilities. A chat-ready link with a text model no longer makes a
  vision feature ready, and a vision-capable model that the chat pick passed over no longer produces a false "no vision"
  card.
- `AiState::from_view` keeps its app-wide meaning for the store's status and health; per-feature cards use
  `state_for`. Pitwall and `workbench-demo` keep working unchanged (the method is additive).

**Per feature, in Debrief.** `debrief_ai::AiFeature` is an enum naming every entry point in the table below, exported to
TypeScript with `ts-rs` like the other wire types. `AiFeature::caps()` gives `Caps::chat(8192)`, plus `json` for the
proposal, query and parser kinds and `vision` for the image and video kinds. A query command,
`ai_feature_state(feature)`, returns `Option<NeedsAiView>` from `NeedsAiView::for_state(feature.label(),
&link.state_for(feature.caps()), "Debrief")`, or `Disabled` in safe mode. The UI never decides readiness itself, because
the engine is the source of truth (AGENTS.md "Where logic lives").

**UI wiring that this replaces.** The UI decides readiness on its own today, in several places [review; files V]:

- `hasModel` and `available()` in `ai/ai-activity.ts`, `ai/ai-button.ts`, `ai/ai-launcher.ts`,
  `ai/chat/chat-composer.ts`, `ai/folder-setup/folder-setup-wizard.ts` and `ai/vision/image-panel.ts`;
- `visionMissing` in `ai/vision/ai-image.store.ts`;
- `ai/vision/no-vision-card.ts` (it offers to install a projector in Debrief).

AI-3 deletes them. The UI gets a registry `Record<AiFeature, AiEntry>` (label, the place the card shows, the
deterministic fallback), so a new `AiFeature` variant fails to compile until it has an entry. `debrief-ai-button` and
`AiLauncher` take a required `feature` input. A spec walks the registry, and an ESLint rule in Debrief's config (or a
spec over the templates) fails any `debrief-ai-button` or `AiLauncher` call without a `feature`.

**The shared component gets one optional input.** `<pd-needs-ai [view]="...">` shows a given `NeedsAiView` instead of the
store's app-wide one. This is an additive change in `ui/projects/paddock-ui/shell/pitwall`: it waits for T and lands
after Pitwall's L1 (rule 42; the alternative, Debrief drawing `<pd-needs-ai-card>` itself, would copy the component's
button logic).

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

**The entry points.** "Card" means `<pd-needs-ai>` with that feature's view. Paths are under `app/src/app/` [V].

| Entry point (where) | Needs | When Pitwall is not ready | Keeps working without Pitwall |
|---|---|---|---|
| Assistant chat (View ▸ AI, *AI: Open the assistant chat*, activity bar) | chat | The pane opens; the card replaces the composer; a typed draft is kept | Saved chats open, read, copy, export, delete |
| *Explain this event (AI)* (row menu, *AI: Explain the selected event*) | chat | The chat pane opens with the question as an unsent draft and the card | Everything else in the row menu |
| *AI: Find likely root causes* (Home), *AI: Summarize this range* (Moment view) | chat | As Explain | Home, Moment view, the deterministic analyses |
| *AI: Ask AI to write a query…* (filter bar `ai-ask-query`, palette) | chat, json | The query dialog opens; the card replaces the answer area; the question is kept | The query field and QL |
| *Suggest a regex for these lines (AI)* (row menu, `markers/regex/infer-dialog.ts`) | chat, json | The dialog's AI area shows the card | *Infer pattern* (deterministic) and manual regex |
| *Suggest regex with AI* (`format/format-wizard.ts:105`) | chat, json | The wizard's AI area shows the card | The wizard's manual path |
| *AI: Propose markers / pipeline steps / extractors…* (markers manager, profile editor, palette) | chat, json | The proposal dialog shows the card | The manual editors |
| *Generate a parser with AI…* (parser-suggest notice, `ai/parser-suggest-notice.ts:54`) | chat, json | The notice's action opens the card | The parser library |
| *Generate with AI* (Settings ▸ Parsers, `settings/parsers/parsers-settings.ts:28`) | chat, json | The menu opens the card | Every other parser setting |
| *Ask the AI to fix* (Problems panel, `problems/problems-panel.ts:163`) | chat, json | The card in the panel | The format wizard hint and the problem list |
| Folder set-up with AI, and its *Improve* step (`ai/folder-setup/folder-setup-wizard.ts:90`, ADR 0067 import) | chat, json | The AI step shows the card next to *Continue without AI* | Folder import with built-in parsers |
| *Suggest with AI* (known issues, `investigate/known-issues/known-issues.ts:161`) | chat, json | The card in the known-issues view | The built-in and user rules |
| Incident narrative (narrative dialog, incident summary) | chat | The narrative block shows the card | The report without a narrative (as the CLI's `debrief report`) |
| *Explain with AI* on dumps and UMDH (`dumps/dumps-shared.ts:60-83`) | chat | The chat pane opens with the engine's redacted question as a draft and the card | The findings and their evidence |
| *Explain with AI* on profiling findings (`profiling/findings/findings-view.ts:56`) | chat | As dumps | The findings |
| *AI: Find logs related to an image…*, *AI: Open an image…*, *Explain with AI* in the image view, an image in chat | chat, vision | The card; on a text-only model, the vision sentence with *Download a model in Pitwall* | The image view (sanitised copy, its EXIF time); images in notes |
| Video frame *explain* and *use with AI* (`video/frame-detail.ts:44`), *describe frames* (behind its confirmation, N6) | chat, vision | The card in the frame detail or the confirmation | Video import, frames, timeline |
| Research, read-along, agent mode, the model's `web_research` (AI browser) | chat | The research dialog or chat shows the card; the browser is not started | *Search the web for this error* |
| *Search the web for this error* | none | Never shows the card: it uses the deterministic search text (features.md) | Everything; with Pitwall the model drafts the text |
| Settings ▸ AI ▸ Knowledge | none | A line says the files are used when Pitwall answers | Edit, import, export, enable |
| Settings ▸ AI | none | The Pitwall connection (state sentence, *Connect to Pitwall…*, *Forget Pitwall*) and the request settings | All of it |
| Status bar AI button, health entry `ai` | none | Health `ai` is optional and degraded, with the app-wide state's sentence and its step as the repair (rule 19: no repair depends on Pitwall running) | n/a |
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
  `scripts/ci/macos-smoke.sh`, `macos-smoke.test.mjs` and `macos-verify-bundle.sh` (one sidecar fewer), `vm-smoke.json`.
- `debrief-ipc`: `endpoint::AI` (`endpoint.rs:66`) and `ChildKind::AiWorker` with its names (`child_log.rs:49`, `:60`,
  `:71`).
- Fuzz targets `fuzz_worker_http` and `fuzz_ai_request`, from `fuzz/Cargo.toml`, `fuzz/Cargo.lock`, `docs/fuzzing.md:68`
  and the target list in `.gitea/workflows/debrief-fuzz.yml:47`. That last one is a shared path, one declared line by the
  integrator, in the same landing.

**`crates/debrief-ai/src`**

- `worker/` (1,600), `llm/lazy.rs` (227), `runtime.rs` (155), `gguf.rs` (713), `manual.rs`, `hardware.rs`, `fit/`,
  `recommend.rs`, `dxgi.rs`, `setup.rs`, `updates/`, `engine/mod.rs` (the trait and `select`), `names_model.rs` (with
  N1), `bin/fake_worker.rs`, the worker and engine parts of `testing.rs`.
- `catalog.rs` and `data/local-models.json`, except `is_safe_file_name`, which moves into `models/download.rs` (its one
  kept user).
- In `runtimes/`: `llama-runtimes.json`, `builds.rs` beyond `is_build_id`, `macos.rs`, `overview.rs`, `report.rs`,
  `selftest.rs`, `store.rs`, `unpack.rs` and `mod.rs`'s llama parts, unless the compiler shows the FFmpeg path needs one.
- In `models/`: `hf.rs`, `header.rs`, `verify.rs`, `store.rs`, `priority.rs` and `mod.rs`'s model parts, under the same
  rule.
- `llm/prompt_format.rs`, if only the worker path renders prompts with it (`worker/supervisor.rs` and `llm/prompted.rs`
  use it [V]; Pitwall applies the chat template itself).
- `tests/it/`: `worker.rs`, `real_worker.rs`, `worker_memory.rs`, `frames.rs`, `runtimes.rs`, `setup.rs`, `hf_download.rs`,
  `hf_live.rs`, `names_local.rs`, and `net_tls.rs` and `segmented_download.rs` only if no kept downloader uses what they
  test (otherwise they go in M2c).
- The settings keys `active_model`, `autoload`, `idle_unload_minutes`, `gpu_offload`, `threads`, `hf_endpoint`,
  `download_proxy`, `max_download_gb`, `download_connections`, `models_dir`, `inference_build`, `llama_server_path`,
  `runtime_updates`, `model_updates`, `update_check_hours`, `name_aliases` and `engine` (`settings.rs:60-97`).
  `SETTINGS_VERSION` goes up by one. A 1.1 `ai.json` still loads, because serde ignores unknown keys: the removed keys are
  dropped and named in one log line, with no conversion. This is not a migration; the kept keys mean the same. D6 reads
  `models_dir` and `llama_server_path` raw from the old file, for the notice only.
- `AiError::{NoModel, NoRuntime}` and the other engine errors, with their mapping in `debrief-video/src/error.rs` (all in
  AI-1).
- `examples/fetch_model.rs`. `examples/eval.rs` is kept and re-pointed (D7).

**Shell (`app/src-tauri/src`)**

- `ai/runtimes.rs`, `ai/runtime_commands.rs`, `ai/updates.rs`, `ai/update_commands.rs`, `ai/manual.rs` and
  `ai/assistant_setup.rs`, with their tests (`ai/tests/update_checks.rs` and the worker parts of `ai/tests.rs`).
- The engine parts of `ai/core.rs` (worker spawn, models, runtimes; `:1231` finds the worker child), `ai/commands.rs`
  (model, fit and recommend commands), `ai/settings_section.rs` (engine and runtime fields), and in `ai.rs` the worker
  paths and memory cap (`:100-103`).
- `ai/vision.rs:72`'s use of the worker protocol's `MAX_IMAGES`: the limit of 4 images moves next to the Pitwall
  request (ADR 0025's limit is unchanged).
- The AI worker's memory cap in Performance: `ai_worker_memory_mb` (`performance/mod.rs:46`, `:81`), its setting field
  (`performance/section.rs:64`), `tuning.rs:92`, its rows in `performance/commands.rs:84` and `:134`,
  `ProcessRef::Ai` in `performance/watch.rs:20`, `:43`, `:52`, `ai_model_mb` in `performance/suggest.rs:32`, `:54`, and
  the UI's `settings/performance/memory-actions.ts` entry for it.
- `storage.rs:102`'s `runtime_tag` and the `AiPort` model, runtime and custom-server methods in `storage/plan.rs`, with
  the three AI categories. One `old_ai_files` category replaces them (D6).
- The health registry's AI-worker wording and its `ai_reset` repair (`health/commands.rs:86-140`, `:190`); the `ai`
  entry's repair is the Pitwall step (D3).
- The `pitwall_use_builtin` repair and every "Use the built-in engine" text.

**UI (`app/src/app`)**

- `ai/runtimes`, `ai/models`, `ai/setup`, `ai/updates`, `ai/manual` and `ai/vision/no-vision-card.ts`, and the palette
  item *AI: Manage models…*.
- The engine choice, the worker status and the UI-side readiness (D3) in `ai-status.store.ts`, `ai.types.ts`,
  `ai-lazy.ts`, `ai-activity.ts`, `ai-button.ts`, `ai-launcher.ts`, `chat/chat-composer.ts`, the folder-set-up wizard,
  the image panel and store, and `ai/settings`.
- The mock backend's runtime, update and engine modes, and the `mockAi=` modes that exist only for them.
- The `ai-setup` entry in the dialog registry, which removes one of M2b's five fixed-height forms (`plan:133`).

**End-to-end tests**

- `ai-runtime`, `ai-models`, `ai-setup`, `ai-updates` and `ai-manual-download` go.
- `ai-pitwall` loses "Download a model for Debrief instead" (`:40`).
- `ai-vision` "a text-only model offers its image support" (`:95`) becomes the vision state.
- `ai-entry-points` "…entry points are absent" (`:119`) stays for safe mode only. A new test covers the visible states (D7).
- The specs that pick a `mockAi=` mode change with the mock: `activity-bar`, `ai-dark`, `ai-entry-points`, `ai-pitwall`,
  `incident-report`, `known-issues` and `ux-welcome` [V: 10 specs use `mockAi=`, 3 of them deleted above]; also
  `ai-chat`, `ai-vision`, `browser`, `browser-scripts`, `dock-full-height`, `jobs`, `main-layout` and `activity-report`,
  which name the worker or its states. Their screenshots change, so the changed screenshots get an independent review
  (rule 22) before AI-1 and AI-3 land.
- `app/src-tauri/tests/it/export_diagnostics.rs` loses the worker's bundle section.

**Registers**

- `docs/known-copies.toml`:
  - the Debrief member of `loopback-http-client` for `debrief-ai-worker/src/http.rs` goes in AI-1;
  - the member for `engine/pitwall/http.rs` goes in M2c;
  - `model-download` goes in M2c, when the last caller of `models/download.rs` moves onto `paddock-download`;
  - `prompted-tools` goes in M3b (`:525-605`).
- Every deletion removes its member in the same commit (rule 55).
- `paddock.waivers.toml`: rows `:44`, `:50` and `:118` go in AI-1 (worker `child.rs` and `http.rs`). Row `:62` goes with
  `tests/it/net_tls.rs`, in AI-1 or M2c as above. Row `:56` goes in M2c (`engine/pitwall/http.rs`). That is 5 of
  Debrief's 15 rows [V]. The two `chromium-driver` rows stay with the browser (X6).

**Not deleted:** the AI browser and `chromium-driver` (D2; X6 still moves it onto `paddock-chromium`), `paddock-pitwall`
itself, the generic downloader that FFmpeg and Chromium use (D5), and the planned Model2Vec search (D8).

### D5. Where the kept code goes

| Kept code (note §1.3) | New home | Why | When |
|---|---|---|---|
| Image sanitising and EXIF (`vision/image.rs` 460, `exif.rs` 226), fuzz target `fuzz_image` | New app crate `debrief-image` (pure; `image` with the five formats, as ADR 0025 decision 4). `debrief-ai` depends on it, never the reverse. `fuzz_image` moves to it; `fuzz_vision_facts` stays on `debrief-ai` (the facts parser stays there) | Notes (`notes/images.rs:7`, `notes/commands.rs:221`) and video (`debrief-video/src/describe.rs`, `runtime/selftest.rs`) use it without AI; `debrief-core` does not depend on `image` today [V] | AI-2 |
| Leak guard and untrusted-web marking (`web/`, 1,003) | Stays in `debrief-ai` | Only AI-bound text uses it (ADR 0059 Privacy) | n/a |
| Knowledge files (`knowledge/`, 777) | Stays in `debrief-ai` | Prompt context (D2) | n/a |
| `StoreAccess`, errors, settings (`access.rs`, `error.rs`, `settings.rs`) | Stay in `debrief-ai`, shrunk (D4) | Used by `investigate/`, the tools and video | AI-1 |
| The generic downloader that the FFmpeg runtime and the Chromium download use [V]: `models/{download,segmented,source,disk}.rs` (`Downloader`, `DownloadOptions`, `ModelSource`, `Progress`, `Phase`, `RemoteFile`, `DiskSpace`), `runtimes/catalog.rs` (`RuntimeCatalog`, `RuntimeBuild`, `RuntimeAsset`, `platform_key`; its `hardware` use goes), `runtimes/github.rs` (`GithubReleases`), `runtimes/builds.rs::is_build_id`, `net.rs` | Stays in `debrief-ai` until M2c ports FFmpeg and Chromium onto `paddock-download`, then deleted | `debrief-video/src/runtime/{catalog,store}.rs` and `browser/runtime.rs` import exactly these [V]; moving them twice is waste; M2c already owns the port (`plan:124`, workbench.md §16) | M2c |
| Disk-space probe for `performance/suggest.rs:147` | Uses `DiskSpace` from the kept downloader until M2c, then `paddock-download`'s check or a shell copy of the 90 lines if it has none | `DiskSpace` is also the FFmpeg store's (`debrief-video/src/runtime/store.rs:19`) [V], so it cannot move to the shell alone | M2c |
| Prompt evals (`evals/cases.toml`, `make_cases.py`, `examples/eval.rs`) | Stay; `eval.rs` drops its `gguf` and `hardware` use and runs against Pitwall | Q9: local quality checks (D7) | AI-1 |

### D6. The one-time notice for old AI files (Q5)

There is no migration and no import into Pitwall. A user who wants to keep a model file can keep it outside Debrief's
folders and point Pitwall at it [U: Pitwall 1.1-final has no *Add local GGUF* action; how Pitwall takes an existing file
is its own question].

- **One storage category.** `old_ai_files` ("Old AI files") replaces `ai_runtimes_outdated`, `ai_runtimes` and
  `ai_models` in `storage/plan.rs`. It covers:
  - `<data>/models` (models, `.mmproj.gguf` projectors, partial downloads);
  - `<data>/ai/runtimes`;
  - in `<config>/ai`, an explicit list only: today `local-models.json` and `updates.json`. Nothing else in that folder is
    touched; `ai.json`, `knowledge/` and any unknown file survive.

  The later Model2Vec model (D8) is never stored in these folders, so it can never be counted or removed here. The
  category uses the existing walk, budget and confinement (`storage/walk.rs`), so sizes, Settings ▸ Storage and the
  notice show the same numbers from one mechanism.
- **Links and junctions (deletion safety).** Before acting, each path component from `<data>` (or `<config>/ai`) down
  to the target is checked with `symlink_metadata`, which also sees Windows junctions as reparse points. If a component
  is a link or junction, the link itself is removed and nothing behind it is followed. If the link is the category's root
  (`<data>/models` itself a link), the target is treated as a folder the user chose: listed, never deleted inside.
- **Folders the user chose.** If the 1.1 `ai.json` names a `models_dir` or a `llama_server_path` outside the data folder,
  the category lists it and its size but never deletes in it: "In a folder you chose: delete these yourself", with *Show
  in folder*. Debrief reads those two keys raw, for this notice only. Their size is not part of the notice's number.
- **The notice.** After the first window is painted, a background check (rule 10) runs, in the main instance only (a
  second window or instance never shows it). If the category is not empty and the notice has not been answered, one
  notice goes through the notification service (ADR 0055): "Debrief no longer runs AI models itself. AI now runs in
  Pitwall. <N> GB of old model and runtime files can be deleted." `<N>` is the walk's total without the folders the user
  chose; when the walk stops at its budget (`MAX_ITEMS` or the byte budget), it says "at least <N> GB". The buttons are
  **Delete old AI files** and **Keep**.
  - Delete runs as a job with live, phase-labelled progress (ER-17, ER-18): *Counting* with a growing count, then
    *Deleting* with bytes removed of bytes found. Cancel works in both phases; a cancelled run leaves what it has not
    reached.
  - Either answer is recorded by the engine in its versioned UI state (Rust owns the flag, not the webview), so the
    notice never comes back. *Keep* leaves the category in Settings ▸ Storage for later.
  - An install with no old files never shows it.
- **Logs (ER-27).** The check logs the bytes and counts found and the answer. The job logs its phases, durations, counts
  and bytes removed, and each refusal (link, user folder, unlisted file) with its reason. No file names.

### D7. Tests

Failing tests first (ER-11), with invented data and temporary folders only (ER-14, rule 7: never the real data folder,
port 11435, the credential store or an installed Pitwall).

1. **Readiness per capability set (`paddock-pitwall`).** Against the shared fake Pitwall, both directions:
   - Pitwall has a vision model, but the chat pick is a text model: `state_for(chat)` is `Ready` with the text model,
     `state_for(chat+vision)` is `Ready` with the vision model, and the card says nothing about missing vision.
   - Pitwall has only text models: `state_for(chat+vision)` is `NoSuitableModel { needs.vision }` while
     `state_for(chat)` is `Ready`.
   - The request for a feature asks the same `ModelSel` that `state_for` checked (a JSON feature asks
     `auto:chat,json,8k`).
   - The cache holds at most 8 entries, is dropped on a probe change, and no call runs on the caller's thread.
2. **Feature table (Debrief, Rust).** For every `AiFeature` variant (an exhaustive `match`) and every `AiState`,
   `ai_feature_state` gives the expected `NeedsAiView`. Safe mode gives `Disabled`. The `ts-rs` export lists every
   variant.
3. **No engine left.** A 1.1 `ai.json` with `engine: "builtIn"`, `active_model`, `inference_build` and `models_dir`
   loads. The kept keys keep their values, the removed keys are named in the log, and the saved file has none of them.
4. **Contract against the shared fake Pitwall**, for one feature of each kind (chat, json, vision, research). The
   knowledge budget comes from `contextPerSlot` capped at the asked context; without it (1.0.x), from `contextMax`
   capped the same way. The fake dying mid-stream gives "Pitwall stopped answering", keeps the partial answer and shows
   the state.
5. **Old files (D6).** A temporary data folder with invented files (a few-byte `.gguf`, a projector, a runtime folder)
   gives the category's size.
   - Delete removes only inside the data folder.
   - A symlink to a file outside: the link goes, the target stays. A symlinked `models` root: listed, nothing deleted
     inside. On Windows, a junction in the path: the junction goes, its target stays (on the Windows runner).
   - An unknown file in `<config>/ai` survives; `ai.json` and `knowledge/` survive.
   - A `models_dir` and a `llama_server_path` outside are listed, never touched, and not in `<N>`.
   - A capped walk says "at least". Cancel mid-way leaves a consistent folder.
   - The answered flag stops the notice; an empty category never shows it; a second instance never shows it.
6. **End-to-end `ai-entry-points`.** The mock Pitwall backend is set to `NotInstalled`, `NotRunning`, `NeedsPairing`,
   `NoSuitableModel` and Ready without vision. For each row of D3's table, the card shows the engine's sentence and
   buttons, the user's input is kept, and the "keeps working" column works. Safe mode still hides the entry points.
7. **Registry guard.** A spec fails if an `AiFeature` lacks an `AiEntry`, and the lint (or template spec) fails a
   `debrief-ai-button` or `AiLauncher` without a `feature`.
8. **End-to-end `ai-pitwall`.** Pairing works from Settings ▸ AI and from a card. No text anywhere offers a model
   download in Debrief (a text search over the built UI strings).
9. **Nothing left behind.** `build-sidecars.test.mjs` and `macos-smoke.test.mjs` expect two sidecars.
   `cargo tree -p debrief-app` has no `debrief-ai-worker` and no `keyring` (after M2c). No `ProcessRef::Ai` or
   `ChildKind::AiWorker` remains. The known-copies register check (C58) is green.
10. **Health.** Health `ai` is optional. When degraded, its repair is the state's step and never "Use the built-in
    engine".
11. **Shared input.** `<pd-needs-ai [view]>` gets a spec in `paddock-ui` (components 100 %) and a gallery page in both
    themes, with the screenshot independently reviewed (rule 22).
12. **Evals.** `cargo build --example eval` passes in CI. Running the evals against a real Pitwall stays a local check,
    never in CI (Q9).
13. **Release path (rule 54).** The sidecar list and bundle checks change Debrief's release path. AI-1 dispatches
    `debrief-release.yml` with gates on and the bundle built before it lands on `main`.

### D8. Model2Vec search stays planned (Q2)

The small embedding search (Model2Vec over message templates, fused with BM25; X55 `paddock-embed` and Y2) is not an
LLM. It is an algorithm: about 3 MB of code, a 32 MB model, and milliseconds per query. It works without Pitwall and is
allowed by rule 46 [N §1.4]. It stays built in, in its release after 1.2, with its model file outside `<data>/models` and
`<data>/ai` (D6). When Pitwall runs, Pitwall's embedder (Qwen3-Embedding, `/v1/embeddings`) is used as well. No code for
it exists today [N, V: no `model2vec` in `apps/debrief`], so nothing in this ADR's build touches it.

### D9. The shared `paddock-pitwall` cleanup waits for Pitwall's base

The built-in hooks in shared code lose their last real user once AI-1 lands. Those hooks are `engine.rs`, the
`built_in_available` and `built_in_for_session` fields of `AiSnapshot`, `pitwall_use_builtin` in `COMMANDS`, and
`useBuiltIn`, `canUseBuiltIn`, `onBuiltIn` and their texts in `ui/.../shell/pitwall`.

Package AI-5 removes them as one breaking shared change (AGENTS.md "Shared code": a breaking change updates every app in
the same change): `workbench-demo` and the generated TypeScript change in the same change, and the CHANGELOG gets an
entry. It lands only after Pitwall's L1 (Pitwall's base landing on `main`) and after M2c. L1 changes `paddock-pitwall`
first (`pw-plan:232`), and removing the hooks before it would make L1 merge the removal. Until then, Debrief passes
`built_in_available: false`, as `workbench-demo` does. It is optional for 1.2: dead fields cost nothing at run time.
`Link::state_for` (D3) is a separate, additive shared change inside AI-3.

### Logs (ER-27)

- Pitwall link state changes (from, to, reason), and per-capability readiness changes.
- Each request's start and outcome with a correlation id and its `AiFeature`, plus Pitwall's `X-Pitwall-Job-Id` and
  `X-Pitwall-Model` when present.
- Pairing outcomes.
- The old-files check and job (D6).
- No prompt, answer, knowledge text or key, as today.

## Consequences

- One engine and one model on disk and in memory for the family. Debrief loses about 16,600 lines, one sidecar (2.41 MB
  uncompressed on macOS arm64 [N, built]), one IPC endpoint, two fuzz targets and five e2e specs. Users no longer
  download llama.cpp builds of 12 to 767 MB or models of 1.3 to 22 GB through Debrief [N §4.1].
- **AI is a second install.** Without Pitwall, every AI feature shows *Get Pitwall* until the user installs Pitwall,
  gets a model there and connects. Debrief's non-AI features are unaffected.
- Readiness becomes honest for every app: `Link::state_for` lets any family app ask "ready for these capabilities?".
- Debrief's AI quality follows Pitwall's model choice. The evals are the local guard.
- No third-party crate leaves with `debrief-ai` and the worker, and no licence class changes [N §4.2]. `keyring` leaves
  with M2c.
- One copy register member and three waiver rows go in AI-1. Two more members and two more rows go in M2c and M3b
  without a port.
- ADR 0009 and 0015 are superseded. ADRs 0002, 0012, 0017, 0025, 0029, 0031, 0033, 0053, 0055 and 0059 carry an
  amendment line pointing here.

## Work packages

**T** is the first full-speed day after the Dyno tag (plan §9.2). "Waits" means it changes shared code and lands only
after T.

| # | Package | Role | Depends | Waits | Days | Failing test first |
|---|---|---|---|---|---|---|
| AI-0 | This ADR, then its independent review | designer, reviewer | - | no | 1-1.5 | (design) |
| AI-1 | Delete the engine (D4) with N1 folded in: the crate, the modules, the shell, Performance, storage, health and UI parts, the `debrief-ipc` kinds, the mock modes and the e2e specs (with the rule 22 screenshot review), the settings keys, the sidecar and bundle checks, fuzz targets, the register members and waiver rows; `AiError` cleanup in `debrief-video`; `eval.rs` re-pointed; rule 54 dispatch. The `debrief-fuzz.yml` line is one declared line by the integrator | implementer | AI-0 accepted | no (app-only; the one workflow line through the integrator) | 3.5-5 | D7 tests 3, 9 and 12 |
| AI-2 | Move the kept code (D5): `debrief-image` with `fuzz_image` | implementer | AI-1 | no | 1-1.5 | `fuzz_image` and the notes image tests pass from the new crate; a damaged image is still refused |
| AI-3 | Readiness and "needs Pitwall" (D3): `Link::state_for` in `paddock-pitwall`; `AiFeature` with `caps()`, its `ts-rs` export and registry; requests on the feature's `ModelSel`; `ai_feature_state`; the UI-side readiness deleted; the step handlers; the card on each row; health `ai`; the shared `[view]` input; the registry lint | implementer (the shared half reviewed by the `paddock-pitwall` owner) | M2c, Pitwall's L1 | **yes** (`state_for`, the `[view]` input) | 3.5-5 | D7 tests 1, 2, 4, 6, 7, 8, 10 and 11 |
| AI-4 | The old-files category and notice (D6), with the link, junction and explicit-list safety; docs: features.md, the user guide (Pitwall 1.1 supported), `docs/fuzzing.md`, the CHANGELOG, third-party notices | implementer | AI-1 | no | 1.5-2 | D7 test 5 |
| AI-5 | Shared cleanup (D9), optional in 1.2 | implementer | AI-1, M2c, Pitwall's L1 | **yes** | 0.5-1 | `COMMANDS` has three entries; the snapshot has no built-in fields; `workbench-demo` builds |
| | **Total** | | | | **10.5-15** (AI-5 adds 0.5-1) | |

**Order.** AI-1, AI-2 and AI-4 are app-only and start now, before T, on the integration branch. That way M2c and M3b are
built Pitwall-only from their first line (Q8). AI-3 follows M2c, because it renders the shared `<pd-needs-ai>` from
`paddock-pitwall`'s snapshot and adds `state_for` there. Between AI-1 and AI-3, the integration branch shows Debrief 1.1's
existing Pitwall messages (no release is cut from it).

## Plan delta (Debrief 1.2 plan, `ac85e26c7`)

| Package | Change | Days |
|---|---|---|
| AI-0 to AI-4 | New (above) | +10.5 to +15 |
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
| **Net** | | **+4.5 to +10 raw** (+5 to +11 with AI-5) |

Effect on the plan's totals (§9.5, after the release split: 510.5 raw, 612.6 with 20 %, about 2027-04-21 at 5.5 agent-days
a day):

- The total becomes **about 515 to 520.5 raw**, or 618 to 624.6 with 20 %.
- That is **+1 to +2.2 working days**, so the end moves from about 2027-04-21 to about 2027-04-22 to 2027-04-26 [calc].
- Revision 1 said +2.5 to +6.5. The review found the per-capability readiness (AI-3 from 2-2.5 to 3.5-5) and the missed
  deletions (AI-1 +0.5 to +1); AI-4 grew by 0.5 for the deletion safety, and AI-2 shrank by 0.5 (the disk probe stays
  with the downloader).

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
2. **Vision depends on a model in Pitwall.** Eight of Pitwall's ten starter models read images [V]. The projector comes
   with a Discover download by default [V] and with one-click recommendations [review], but not with `pitwall pull`
   [review]. A user who pulled from the CLI sees the vision state on *Find related logs* and the video frame features
   until they add the projector in Pitwall; `state_for` makes that state truthful. Debrief adds no workaround.
3. **The shared changes wait for T and L1.** `state_for` and the `[view]` input are both in AI-3. If L1 slips, AI-3
   slips with it. There is no safe partial: without `state_for`, a vision feature can be called ready on a text model.
   So AI-3 lands whole.
4. **`state_for` costs Pitwall calls.** Up to three `select` calls per probe change instead of one; they run on the
   background probe and are cached (D3) [J: negligible against a chat request].
5. **Behaviour drift in M3b's move to `paddock-assistant`** (budgets, the SQL tool, citations). The `prompted-tools`
   canonical was taken from Debrief's code [N]. M3b's `ai-chat` and `ai-entry-points` e2e are the check.
6. **Kept downloader code lingers until M2c.** If M2c slips past 1.2, the `model-download` member still expires on
   `debrief-v1.2.0`, and the downloader port must not be dropped from M2c.
7. **Old data.** GBs in 1.1 users' data folders. The notice and the storage category make them visible and deletable.
   Nothing deletes them silently, nothing behind a link or in a folder the user chose is deleted at all, and only listed
   files in `<config>/ai` are touched.
8. **Many screenshots change.** About fifteen e2e specs change with the mock modes and the cards; their screenshots
   need an independent review (rule 22) before landing, which the AI-1 and AI-3 estimates include.
9. **Ordering of shared code.** AI-5 after L1 and M2c (D9); the `debrief-fuzz.yml` line through the integrator; the
   re-pair flag (Pitwall's B5) before Debrief's family ADR 0057 WP-5 (`pw-plan:246`).
10. **The safe-mode test changes meaning.** "Absent" now means only safe mode. A missed entry point would show as a
    visible but unhandled control. The `Record<AiFeature, …>` registry, the required `feature` input and its lint (D7
    tests 2 and 7) guard it.

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
- A vision model in Pitwall's starter set: **10 models, 8 with vision** [V]. The Discover projector toggle is verified;
  the one-click recommendation and `pitwall pull` behaviour come from the review and were not re-checked here.

Found while writing or revising this ADR:

- Which helper files in `models/` and `runtimes/` beyond the named types the FFmpeg path reaches. D4 and D5 name the
  types, and the compiler decides the rest.
- The URL behind *Get Pitwall*.
- How Pitwall 1.1 takes an existing GGUF file: it has no *Add local GGUF* action on `pitwall/1.1-final`.
- Family ADR 0057's state. It is Proposed on its branch, and the memory records D1 to D4 as accepted with Pitwall 1.1.
  Whether its WP-5 is counted inside the plan's "Decided 2026-11-05, +41" row is [U]. If it is not, M2c carries it.
- Whether Pitwall will add a hand-off to its Models page. *Download a model in Pitwall* only opens Pitwall today.
