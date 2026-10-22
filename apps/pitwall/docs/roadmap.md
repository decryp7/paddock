# Pitwall roadmap

An honest account of what works, what is partly there and what is only planned. Status words: **works** (implemented
and covered by tests), **verified** (also run for real on a machine), **partial**, **planned**. Pitwall is
unreleased; there are no installers yet and nothing is promised for a date.

## Verified on a real machine

One end-to-end run on macOS (Apple Silicon, an M4 Pro), through the app's own flows:

- Metal llama.cpp `b11669` installed with its SHA-256 checked and the self-test passed.
- Qwen2.5-0.5B-Instruct Q4_K_M downloaded from Hugging Face, its SHA-256 verified against Hugging Face's LFS object id.
- OpenAI chat (streaming and not), Anthropic messages, Ollama chat and JSON-schema constrained output all answered,
  at about 213 generated tokens per second. The test model was deleted afterwards.

**Not verified yet:** real runs on Windows and Linux, CUDA and Vulkan builds, the installers, the service on Windows
and Linux, restart after an app update on a real install. Linux behaviour is exercised in container tests by the shell
task; CI runs the Rust and UI test suites on Linux, Windows and macOS, which is not a real-model run.

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
- Declarative packs: engine packs (the generic adapter; a llamafile pack ships, experimental) and model catalog packs.
- Management API and live feed for the UI; versioned settings with backup; metrics with bounded retention; discovery file.

## Phase 2: the app *(implemented, not released)*

- Angular UI: dashboard, models, engines, apps, requests, playground, benchmarks, settings and tray view. **Works**
  against the mock backend and the management API contract.
- Tauri shell: tray and window lifecycle, admin key in the OS credential store, service install per OS (Windows
  service, macOS launch agent via `SMAppService`, Linux systemd user service), start at login, restart after update.
  **Implemented and unit-tested**; not yet exercised on real Windows and Linux machines.
- Installers: Windows NSIS, macOS `.dmg`, Linux `.deb` and AppImage; unsigned until the signing secrets are set
  ([releasing.md](releasing.md)).
- Updater with signed `latest.json`: **planned**, needs the secrets.
- CI on Linux, the Windows runner and the macOS host: `build.yml` and `release.yml` exist.

## Phase 3: the rest of the product brief *(planned)*

| Item | State |
| --- | --- |
| Task endpoints (`/x/v1/tasks/...`: summarise, classify, extract, translate) with versioned prompts | Implemented and tested over a fake engine; **not verified** against a real model |
| Token counting and truncation helpers (`/x/v1/tokens/...`) | Implemented as estimates (no tokenizer call yet); **partial** |
| Response cache and prompt-prefix policy | Policy in the core (`cache.rs`); **not wired** into the server |
| Stored benchmarks feeding "fast" selection | A benchmark run works and results are kept in memory; not written to disk, not used by selection; **partial** |
| OS memory-pressure signals and foreground-app yielding | **Built.** Linux PSI, macOS `sysctl` and `pmset`, Windows memory notification and `SHQueryUserNotificationState`; foreground detection is not available on Linux. Verified on macOS; Windows and Linux are checked by their CI runners |
| Releasing a model's context before its weights | **Built.** At half the idle time the engine's slots are erased (KV and prompt cache); the weights go at the full idle time. The KV buffer itself is only returned on unload |
| Engine restart with backoff | **Built.** Restart with exponential backoff, a crash-loop guard, and engine problems with Retry on the dashboard |
| Request inspector with prompts on demand | Metadata only; content not kept yet |
| LAN sharing with TLS and certificate pinning | Settings and status fields exist; **not built** |
| Knowledge collections (local RAG: store, chunk, embed, rerank, query) | **Not built** |
| MCP and tool hosting | **Not built** |
| Batch jobs that resume | **Not built** (the batch priority exists) |
| `/v1/audio/transcriptions` with whisper.cpp | Answers 501 with an explanation; engine **planned** |
| MLX (Apple Silicon) and vLLM (Linux, NVIDIA) engines | Show as "not available" with the reason; **planned** |
| Dedicated embedding and reranker adapters | Embeddings work through llama.cpp with an embedding model; rerank **planned** |
| Real-engine automated test (`PITWALL_LLAMA_E2E=1`) | **Planned**; the smoke test is manual |
| Usage per connected app | **Built.** Requests, prompt and completion tokens, errors and peak memory per app in hourly buckets (7 days) and daily buckets (90 days), stored locally in `usage.json` (versioned, atomic, backup, tolerant of damage). The Apps page has an app detail with a 24 h / 7 d / 30 d chart, limits with progress and the latest requests; the Dashboard has a usage-by-app chart. Optional limits per app: requests per minute and per day, tokens per day and per month, concurrent requests and a memory share, each refused with a typed, explained error, with a warning event at 80 % |
| Model update notices | **Approved, not built.** On a schedule, and only when offline mode allows, ask Hugging Face for newer revisions or better quantisations of installed models; notify and offer them, never download by themselves. Tests use a fake Hugging Face API |
| Model presets and aliases | **Built.** An app asks for a name such as `debrief-explain`; the user decides which model (or capability selection like `auto:chat,fast`) and parameters (context, temperature, top-p, max tokens, system prompt, engine) serve it. Resolved in the core next to capability selection, in the OpenAI, Anthropic and Ollama families; listed in `/v1/models` and `/api/tags`; `X-Pitwall-Alias` and the request record say which alias was used; a removed target model gives a typed `preset_target_unavailable` error. Models has a Presets tab with a live preview from the engine; stored in `presets.json` (versioned, atomic, backup, tolerant of damage). ADR 0014 |
| Parallel slots per model, with their memory cost | **Approved for v1, not built.** Choose the number of parallel slots per model; the UI shows the memory each adds, and the fit check answers "how much context or concurrency fits" instead of only yes or no |
| `pitwall` command-line tool | **Built.** A small Rust CLI (`crates/pitwall-cli`) over the local management and standard APIs: `status`, `ps`, `models`, `load`, `unload`, `pull` (progress, resumable, honours offline mode), `keys`, `presets`, `chat` (streaming, per-app key from `PITWALL_API_KEY`) and `bench`; `--json`, timeouts, stable exit codes 0 to 4; no new server surface. Not yet shipped inside the installers: the bundle needs a second `externalBin` and a `PATH` entry (follow-up) |
| Engine version pin, rollback and capability gating | **Approved for v1, partly built.** Pinned catalog, rollback and self-test exist; missing: pin a build per model or globally from the UI, and gate features (vision, tools, embeddings, rerank) by what the active build supports |

## Phase 4: the suite *(planned)*

Debrief uses Pitwall when it is running and keeps its built-in AI as the default and the offline fallback
([ADR 0009](adr/0009-app-integration-contract.md); the Debrief side is not built). Then Pitcrew, Spotter and Dyno.

## Engineering follow-ups

- Windows and Linux real-model runs; CUDA and Vulkan runs; record the results in the testing manual.
- Property tests ("never panics") and fuzz targets for every parser; mutation testing to measure test strength.
- Below-normal process priority for engines is applied on Windows only; decide the macOS and Linux equivalent.
- Pin updates for llama.cpp as reviewed changes that pass the end-to-end test.

## Next (hand-off, 2026-10-22)

1. **Linux CI** (`rust tests + core coverage (linux)`): the two end-to-end tests that start a real child process
   (`an_engine_pack_serves_a_model_through_a_real_child_process` and `a_crashing_engine_is_restarted_with_backoff_then_given_up_on_until_retry`)
   fail in the container with `Permission denied (os error 13)` when launching `target/debug/pitwall-server`; the first one
   predates the crash-handling work. Probably the workspace/target mount is `noexec` or the binary is not executable for the
   container user: run the tests with `CARGO_TARGET_DIR` on an executable path (or copy the binary to a temp dir inside the
   test), then re-run the whole job. Everything after those tests (core coverage guard >= 99 % lines, which is 99.4 % locally)
   has not been seen green on Linux yet. Windows, macOS, format and clippy, licences and privacy, frontend and Playwright, and
   the three bundles were green on run 6056.
2. **Design review P2/P3** from `design-review-pitwall.md` (sidebar header pill, quick-action row, history legend and event
   lane, playground chips, disk bar labels, tray sizes, page subtitles) and fresh light and dark screenshots of every screen.
3. Model update notices; parallel slots with their
   memory cost; the `pitwall` CLI; engine pin and capability gating (see the table above).
4. Verify on real Windows and Linux machines: the OS pressure readers, the service flows, single instance, the first-run question.
