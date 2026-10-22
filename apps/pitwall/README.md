# Pitwall

**The private, local AI server your apps talk to.** Pitwall runs language models on your own computer and gives
every app one place to use them — through the standard OpenAI-, Anthropic- and Ollama-compatible APIs, plus a few
extras built for the apps in the suite (Debrief, Pitcrew, Spotter, Dyno). Like the pit wall in a race, it's where
every car radios in.

> **Local only.** Nothing you ask and nothing the model answers ever leaves your computer. No cloud models, no
> telemetry. The only network traffic is downloading engines and models you asked for — and offline mode turns even
> that off. See [ADR 0001](docs/adr/0001-local-only.md).

**Status: in development.** The pure core (API formats, queue, memory policy, model selection, keys, metrics) is
written and tested; the server and the app are next. Nothing here is ready to install yet.

## What it will do

- **Engines:** llama.cpp first (CPU, Vulkan, CUDA, Metal builds chosen for your hardware), with pinned, checksummed
  downloads, a self-test after every update and automatic rollback. MLX, vLLM, whisper.cpp and embedding/reranker
  engines later.
- **Model library:** search and download GGUF models from Hugging Face with a "will this fit?" check before you
  download, resumable verified downloads, quantisation variants, and cleanup of models you don't use.
- **Standard APIs** on `127.0.0.1:11435`: OpenAI (`/v1/chat/completions`, `/v1/responses`, `/v1/embeddings`,
  `/v1/audio/transcriptions`, `/v1/models`), Anthropic Messages (`/v1/messages`) and Ollama (`/api/chat`,
  `/api/generate`, `/api/tags`, `/api/embed`), with streaming, tool calls, vision and JSON-schema output.
- **Extras for apps (`/x/v1`):** pick a model by capability (`auto:vision,8k,fast`), a priority queue (interactive
  before background before batch) with cancel and progress, knowledge collections (local RAG), task endpoints
  (summarise, classify, extract, translate), token counting, a response cache, and per-app usage and limits.
- **Memory that looks after itself:** live system RAM/VRAM monitoring, models released when idle and reloaded on the
  next request, release under memory pressure (never a model that is answering), and presets: Low memory, Balanced,
  Performance.
- **Security:** per-app keys approved with a pairing code and kept in the OS credential store, per-app permissions,
  `Host`/`Origin` checks against DNS rebinding; sharing on your own network only if you turn it on (TLS, pinned
  certificate).
- **Plugins:** Rust only, each in its own sandboxed process — engines, endpoints, tools and request filters. A
  crashing plugin is isolated and never takes Pitwall down; plugins get no network access by default.
- **The app:** a dashboard (memory, loaded models with their idle countdown, tokens per second, queue, per-app
  usage, history), model and engine managers, apps and keys, a request inspector, a chat playground with side-by-side
  comparison, benchmarks on your hardware, and a tray / menu-bar view.

## For app developers

Apps find Pitwall through a `discovery.json` file (or port 11435), pair once with a six-digit code the user approves,
then use the OpenAI-compatible API with a few headers for priority and provenance. Debrief keeps its own built-in AI as
the default and offline fallback. See [ADR 0009](docs/adr/0009-app-integration-contract.md).

## Repository

| Path | What it is |
|---|---|
| `crates/pitwall-core` | Pure logic: API formats, scheduler, memory policy, selection, keys, metrics, settings |
| `crates/pitwall-loopback` | HTTP client that can only talk to `127.0.0.1` (engines) |
| `crates/pitwall-server` | The server (in progress) |
| `crates/pitwall-ipc`, `pitwall-plugin-api`, `pitwall-plugin-host` | Plugin protocol, SDK and guarded host |
| `plugins/`, `templates/plugin` | Example plugin and the plugin template |
| `app/` | Angular UI (Tauri shell to come) |
| `docs/adr` | Architecture decisions |

Engineering rules for contributors and coding agents are in [AGENTS.md](AGENTS.md).

## Roadmap

1. Server: engines (llama.cpp), model library and downloads, serving with load-on-first-use and idle release, the
   standard APIs, `/x/v1` and the management API, the no-network test.
2. Tauri app: tray / menu bar, dashboard and screens, updater, start at login; Windows installer and macOS `.dmg`.
3. ADRs for the architecture, API compatibility, engines, queue, memory release, security, plugins and metrics;
   `docs/development.md`, `docs/releasing.md`; CI on Linux, Windows and macOS.
4. Knowledge collections, task endpoints, MCP and tool hosting, benchmarks.
5. More engines: MLX, whisper.cpp, embeddings and rerankers, vLLM.
6. Debrief integration (its side), then Pitcrew, Spotter and Dyno.

## Licence

MIT OR Apache-2.0.
