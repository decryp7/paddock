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
| Usage per connected app | **Approved, partly built.** Per-app limits (requests per minute, tokens per day) are enforced with a typed, explained error today. Missing: tokens, requests and peak memory per app over time (a dashboard chart and an app detail view), stored locally with bounded retention |
| Model update notices | **Approved, not built.** On a schedule, and only when offline mode allows, ask Hugging Face for newer revisions or better quantisations of installed models; notify and offer them, never download by themselves. Tests use a fake Hugging Face API |
| Model presets and aliases | **Approved for v1, not built.** An app asks for a name such as `debrief-explain`; the user decides which model and parameters (context, temperature, system prompt, engine) serve it. Resolved in the core next to capability selection; shown in Models and in the request inspector |
| Parallel slots per model, with their memory cost | **Approved for v1, not built.** Choose the number of parallel slots per model; the UI shows the memory each adds, and the fit check answers "how much context or concurrency fits" instead of only yes or no |
| `pitwall` command-line tool | **Approved for v1, not built.** A small Rust CLI over the local management and standard APIs (status, models, load, unload, pull, keys, chat) for scripts and terminals; no new server surface |
| Engine version pin, rollback and capability gating | **Approved for v1, partly built.** Pinned catalog, rollback and self-test exist; missing: pin a build per model or globally from the UI, and gate features (vision, tools, embeddings, rerank) by what the active build supports |

## Phase 4: the suite *(planned)*

Debrief uses Pitwall when it is running and keeps its built-in AI as the default and the offline fallback
([ADR 0009](adr/0009-app-integration-contract.md); the Debrief side is not built). Then Pitcrew, Spotter and Dyno.

## Engineering follow-ups

- Windows and Linux real-model runs; CUDA and Vulkan runs; record the results in the testing manual.
- Property tests ("never panics") and fuzz targets for every parser; mutation testing to measure test strength.
- Below-normal process priority for engines is applied on Windows only; decide the macOS and Linux equivalent.
- Pin updates for llama.cpp as reviewed changes that pass the end-to-end test.
