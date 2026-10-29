# Pitwall roadmap

An honest account of what works, what is partly there and what is only planned. Status words: **works** (implemented
and covered by tests), **verified** (also run for real on a machine), **partial**, **planned**. Pitwall 1.0.x is
released; 1.1 is the current line. Nothing is promised for a date beyond the release in progress.

## Verified on a real machine

One end-to-end run on macOS (Apple Silicon, an M4 Pro), through the app's own flows:

- Metal llama.cpp `b11669` installed with its SHA-256 checked and the self-test passed.
- Qwen2.5-0.5B-Instruct Q4_K_M downloaded from Hugging Face, its SHA-256 verified against Hugging Face's LFS object id.
- OpenAI chat (streaming and not), Anthropic messages, Ollama chat and JSON-schema constrained output all answered,
  at about 213 generated tokens per second. The test model was deleted afterwards.

**Not verified yet:** real runs on Windows and Linux, CUDA and Vulkan builds, the installers, the service on Windows
and Linux, restart after an app update on a real install. Linux behaviour is exercised in container tests by the shell
task; CI runs the Rust and UI test suites on Linux, Windows and macOS, which is not a real-model run.

## 1.0.2 and 1.0.3

- **1.0.2**: the macOS Login Items repair (shown progress, retries, a disabled item is reported), llama.cpp can be installed from
  its page on a Mac with no build, and the unreachable-server test no longer depends on the default port.
- **1.0.3** (this tree): llama.cpp installs itself (ADR 0020); llama.cpp is the only engine and the Engines page is the
  llama.cpp page; "When you quit Pitwall" (ADR 0021); the model catalog lists current-generation models only
  (checked against Hugging Face on 2026-10-23). The pinned llama.cpp `b11669` (2026-10-15) already runs the `qwen35`,
  `qwen35moe`, `qwen3next` and `gemma4` architectures of those models, so the pin did not change.

## Phase 1: the server *(works)*

- Engine management for llama.cpp: pinned and checksummed builds for Windows, macOS and Linux (Metal, CUDA 12 and 13,
  Vulkan, ROCm, SYCL, OpenVINO and CPU where the project publishes them), defensive unpacking, self-test, discard on a
  failed self-test, select an earlier build.
- Model library: Hugging Face search and file lists, "will this fit?", resumable downloads verified against the
  published SHA-256, GGUF check, delete, disk usage.
- Serving: load on first use, several hot models while they fit, idle and deep-idle release, pressure release,
  pinning, per-model idle time and context, a priority queue with caps and aging, pause and resume, per-request
  timeouts and size limits.
- Standard APIs on `127.0.0.1:11435`: OpenAI (`/v1/chat/completions`, `/v1/responses`, `/v1/embeddings`,
  `/v1/models`), Anthropic (`/v1/messages`, `/v1/messages/count_tokens`), Ollama (`/api/chat`, `/api/generate`,
  `/api/tags`, `/api/ps`, `/api/show`, `/api/embed`, `/api/version`), with streaming, tools, vision through a
  projector and JSON-schema output on llama.cpp.
- `/x/v1`: health, pairing, model selection by capability, models, jobs and cancel; provenance headers on every answer.
- Security: hashed per-app keys, pairing with a six-digit code, per-app permissions and rate and token budgets,
  admin key, Host and Origin checks, a single audited outbound client, offline mode.
- Model catalog packs (declarative, `*.models.toml`). Engine packs and the llamafile pack existed in 1.0.1 and were removed in 1.0.3: llama.cpp is the only engine (ADR 0020).
- Management API and live feed for the UI; versioned settings with backup; metrics with bounded retention; discovery file.

## Phase 2: the app *(implemented, not released)*

- Angular UI: dashboard, models, engines, apps, requests, playground, benchmarks, settings and tray view. **Works**
  against the mock backend and the management API contract.
- Tauri shell: tray and window lifecycle, admin key in the OS credential store, service install per OS (Windows
  service, macOS launch agent via `SMAppService`, Linux systemd user service), start at login, restart after update.
  **Implemented and unit-tested**; not yet exercised on real Windows and Linux machines.
- Installers: Windows NSIS, macOS `.dmg`, Linux `.deb` and AppImage, built by `pitwall-release.yml` from a `pitwall-v*`
  tag; the macOS app is ad-hoc sealed and not notarised, the Windows installer is not code-signed
  ([releasing.md](releasing.md)).
- Updater with signed `latest.json` (the family key, the `pitwall-latest` feed): **works** in 1.1; the VM test plan is in
  the [testing manual](testing-manual.md#update). 1.0.x copies do not look at that feed and are reinstalled once by hand.
- CI on Linux, the Windows runner and the macOS host: Pitwall's legs of the monorepo `ci.yml` and `pitwall-release.yml`.

## Phase 3: the rest of the product brief *(planned)*

| Item | State |
| --- | --- |
| Task endpoints (`/x/v1/tasks/...`: summarise, classify, extract, translate) with versioned prompts | Implemented and tested over a fake engine; **not verified** against a real model |
| Token counting and truncation helpers (`/x/v1/tokens/...`) | Implemented as estimates (no tokenizer call yet); **partial** |
| Caching (ADR 0019): slot affinity and `--cache-ram` sizing, an opt-in response cache, an embeddings cache, `cache.sqlite`, sweep, Settings > Caching | **Specified; engine in progress.** Replaces the old unwired policy in `cache.rs`. Verify against a real model before calling it built |
| Stored benchmarks feeding "fast" selection | A benchmark run works and results are kept in memory; not written to disk, not used by selection; **partial** |
| OS memory-pressure signals and foreground-app yielding | **Built.** Linux PSI, macOS `sysctl` and `pmset`, Windows memory notification and `SHQueryUserNotificationState`; foreground detection is not available on Linux. Verified on macOS; Windows and Linux are checked by their CI runners |
| Releasing a model's context before its weights | **Built.** At half the idle time the engine's slots are erased (KV and prompt cache); the weights go at the full idle time. The KV buffer itself is only returned on unload |
| Engine restart with backoff | **Built.** Restart with exponential backoff, a crash-loop guard, and engine problems with Retry on the dashboard |
| Request inspector with prompts on demand | Metadata only; content not kept yet |
| LAN sharing with TLS and certificate pinning | Settings and status fields exist; **not built** |
| Knowledge collections (local RAG: store, chunk, embed, rerank, query) | **Not built** |
| MCP and tool hosting | **Not built** |
| Batch jobs that resume | **Not built** (the batch priority exists) |
| `/v1/audio/transcriptions` | Answers 501 with an explanation; **planned** (no engine for it) |
| Other engines (MLX, vLLM) | **Dropped** (ADR 0020): a new engine would be a new adapter behind the `Engine` trait and a new ADR |
| Dedicated embedding and reranker adapters | Embeddings work through llama.cpp with an embedding model; rerank **planned** |
| Real-engine automated test (`PITWALL_LLAMA_E2E=1`) | **Planned**; the smoke test is manual |
| Usage per connected app | **Built.** Requests, prompt and completion tokens, errors and peak memory per app in hourly buckets (7 days) and daily buckets (90 days), stored locally in `usage.json` (versioned, atomic, backup, tolerant of damage). The Apps page has an app detail with a 24 h / 7 d / 30 d chart, limits with progress and the latest requests; the Dashboard has a usage-by-app chart. Optional limits per app: requests per minute and per day, tokens per day and per month, concurrent requests and a memory share, each refused with a typed, explained error, with a warning event at 80 % |
| Model update notices | **Built.** Off until the user turns it on (weekly or daily, or Check now). Asks `huggingface.co`, through the one egress client, only for the repository ids of models you downloaded, never in offline mode (zero requests, a typed `offline` outcome), at most 40 repositories per check with timeouts, a pause, a rate limit and a stop on HTTP 429. Rules in the core: a newer revision of the same file (by recorded SHA-256, or by size for old entries), a better quantisation that fits, an imatrix build. Notices show as a Models banner, a note on the library row, a dashboard line and a tray entry; Dismiss or Remind me in a week; "Get it" confirms with the host and then uses the normal download, never automatically. A sidecar `<file>.origin.json` records where a model came from. Stored in `model-updates.json` (versioned, atomic, backup, tolerant of damage). Tests use a fake Hugging Face API. ADR 0017 |
| Model presets and aliases | **Built.** An app asks for a name such as `debrief-explain`; the user decides which model (or capability selection like `auto:chat,fast`) and parameters (context, temperature, top-p, max tokens, system prompt) serve it. Resolved in the core next to capability selection, in the OpenAI, Anthropic and Ollama families; listed in `/v1/models` and `/api/tags`; `X-Pitwall-Alias` and the request record say which alias was used; a removed target model gives a typed `preset_target_unavailable` error. Models has a Presets tab with a live preview from the engine; stored in `presets.json` (versioned, atomic, backup, tolerant of damage). ADR 0014 |
| Parallel slots per model, with their memory cost | **Built.** Choose 1 to 16 parallel slots per model (default 2); the context is per slot and `--ctx-size` is the total, so each slot adds its own KV cache. Models > Slots shows "adds ~X GB" per step and a fit table from the engine (longest context at 1, 2, 4, 8 slots, the most slots at a context, in words with status colours); Discover shows how many requests a variant allows; the scheduler queues requests beyond a model's slots; a change applies on the next load and never touches a busy model. ADR 0016 |
| `pitwall` command-line tool | **Built.** A small Rust CLI (`crates/pitwall-cli`) over the local management and standard APIs: `status`, `ps`, `models`, `load`, `unload`, `pull` (progress, resumable, honours offline mode), `keys`, `presets`, `chat` (streaming, per-app key from `PITWALL_API_KEY`) and `bench`; `--json`, timeouts, stable exit codes 0 to 4; no new server surface. Not yet shipped inside the installers: the bundle needs a second `externalBin` and a `PATH` entry (follow-up) |
| Engine version pin, rollback and capability gating | **Built.** Pin a build for all models or per model (strict: a pin to an uninstalled or failed build is a typed error saying to install it on the llama.cpp page); selection is model pin, then global pin, then automatic. One-click rollback goes to the previous installed build that passed its self-test, never a failed one, and says which loaded models switch on their next load (an engine is never swapped under a busy model). A data-driven capability matrix (`capabilities.json`, minimum llama.cpp release and unsupported backends per feature) gates vision, tools, JSON-schema output, embeddings and rerank: a request the build cannot serve gets a typed, explained error naming the build needed (503 `engine_build_too_old`, 400 `capability_unsupported`, 503 `engine_pin_unavailable`) in the OpenAI, Anthropic and Ollama shapes, and `/x/v1/models` and `auto:` selection leave those capabilities out. The llama.cpp page shows a Version pin toggle, a Features list per build in words and Roll back naming its target; Models shows `pinned to` with a menu. Stored in `engine-pins.json` (versioned, atomic, backup, tolerant of damage). ADR 0018 |

## Phase 4: the suite *(planned)*

Debrief uses Pitwall when it is running and keeps its built-in AI as the default and the offline fallback
([ADR 0009](adr/0009-app-integration-contract.md); the Debrief side is not built). Then Pitcrew, Spotter and Dyno.

Pacenotes: toolsets `orient`, `code`, `review`, `diagrams`, `requirements` and `tests` over Pacenotes' read-only MCP tools, after "MCP and tool hosting" above and once Pacenotes' MCP server exists (about 2 agent-days; Pacenotes plan `apps/pacenotes/docs/plans/2026-10-29-first-release-plan.md`, package MC1).

## Engineering follow-ups

- Windows and Linux real-model runs; CUDA and Vulkan runs; record the results in the testing manual.
- Property tests ("never panics") and fuzz targets for every parser; mutation testing to measure test strength.
- Below-normal process priority for engines is applied on Windows only; decide the macOS and Linux equivalent.
- Pin updates for llama.cpp as reviewed changes that pass the end-to-end test.

## Next (hand-off, 2026-10-22)

1. **Linux CI** (`rust tests + core coverage (linux)`): the two end-to-end tests that start a real child process
   (`a_crashing_engine_is_restarted_with_backoff_then_given_up_on_until_retry` and `stopping_serving_unloads_every_model_ends_the_engine_processes_and_resuming_serves_again`, which use `pitwall-server --fake-engine` as a real child process)
   fail in the container with `Permission denied (os error 13)` when launching `target/debug/pitwall-server`; the first one
   predates the crash-handling work. Probably the workspace/target mount is `noexec` or the binary is not executable for the
   container user: run the tests with `CARGO_TARGET_DIR` on an executable path (or copy the binary to a temp dir inside the
   test), then re-run the whole job. Everything after those tests (core coverage guard >= 99 % lines, which is 99.4 % locally)
   has not been seen green on Linux yet. Windows, macOS, format and clippy, licences and privacy, frontend and Playwright, and
   the three bundles were green on run 6056.
2. **Design review P2/P3** from `design-review-pitwall.md` (sidebar header pill, quick-action row, history legend and event
   lane, playground chips, disk bar labels, tray sizes, page subtitles) and fresh light and dark screenshots of every screen.
3. Model update notices; the `pitwall` CLI; engine pin and capability gating (see the table above).
4. Verify on real Windows and Linux machines: the OS pressure readers, the service flows, single instance, the first-run question.
