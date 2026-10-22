# 4. Engines: one trait, one process per model

- Status: accepted (2026-10-22); llama.cpp verified on macOS only
- Related: ADR 0001, ADR 0002, ADR 0010, ADR 0011; code: [`engines.rs`](../../crates/pitwall-server/src/engines.rs),
  [`pack_engine.rs`](../../crates/pitwall-server/src/pack_engine.rs), [`process.rs`](../../crates/pitwall-server/src/process.rs),
  [`pitwall_core::{llama, runtimes, backoff}`](../../crates/pitwall-core/src/runtimes/mod.rs)

## Context

Pitwall runs models through third-party inference engines. They crash, hang, need GPU-specific builds, and must not
see the user's environment or the network. The install flow (pinned, checksummed, self-tested, reversible) was proven in
Debrief (`imt-ai`, IMT ADR 0009) and is adapted here. There is no plugin system (ADR 0010).

## Decision

**The `Engine` trait** (server) is the only engine port: `kind()`, `id()`, `availability()` and
`start(&Launch) -> Running`. An implementation honours one contract: the returned engine is healthy, bound to
`127.0.0.1` on the caller's port with a private key, and is killed when its handle is dropped. `Running` holds the child,
a `pitwall_loopback::Client` for its port and what `/props` reported (context, vision, build).

**Adapters.**
- `LlamaCpp` is the first-class built-in: install management plus `Engine`.
- `PackEngine` is one generic adapter that runs every engine pack (ADR 0011) over an OpenAI-compatible server.
- `Planned` adapters (MLX, vLLM, whisper.cpp) report `NotAvailable` with a reason the UI shows; they start nothing.

**Process model.** Each loaded model is one child process of the server (`llama-server`), started with
`--host 127.0.0.1`, a free loopback port, `--offline --no-webui --jinja`, `--n-gpu-layers 999` (all layers to the GPU unless
told otherwise), `--parallel` from `server.maxConcurrent` (1 to 4), `--cache-ram` (1024 MiB with 16 GiB RAM or more, else
256), `--mmproj` for vision models, `--embeddings` for embedding models. The engine is protected by a random 16-byte key
passed in `LLAMA_API_KEY`. All children are started only in `process::command`:
- an empty environment plus the allow-list `KEPT_ENV` (`PATH`, `SystemRoot`, `SYSTEMROOT`, `windir`, `TEMP`, `TMP`, `TMPDIR`,
  `HOME`, `USERPROFILE`, `LANG`, `LC_ALL`, `LD_LIBRARY_PATH`, `DYLD_LIBRARY_PATH`, `XDG_RUNTIME_DIR`), so tokens, proxies
  and user variables never reach an engine;
- stdin closed, `kill_on_drop` (engines die with Pitwall), no console window on Windows, below-normal priority on Windows;
- stderr is kept (last 4000 bytes) to explain a failed start.

**Health.** After spawning, the server polls `GET /health` every 250 ms, fails at once if the child exits (with the last
stderr lines), and gives up after 300 s ("the model did not become ready within 5 minutes"). Then it reads `/props`.
Every engine call has a timeout (request timeout setting); a hung engine fails that request, not the server. The release
loop (every 5 s) forgets a model whose process exited and reports `EngineFailed`; the next request reloads it.

**Builds per OS and GPU.** `runtimes/llama-runtimes.json` pins one llama.cpp release (currently tag `b11669`) with, per
platform, the official prebuilt assets, their sizes and SHA-256: Windows x64 (cpu, vulkan, cuda-12, cuda-13, rocm, sycl,
openvino), Windows arm64 (cpu, vulkan, cuda-13), Linux x64 (cpu, vulkan, cuda-12, cuda-13, rocm, sycl, sycl-fp32,
openvino), Linux arm64 (cpu, vulkan, cuda-13), macOS arm64 (`metal`) and macOS x64 (`cpu`). `runtimes::suggest` proposes a build from the detected hardware; the automatic order is Metal,
then CUDA (newest first), then Vulkan, then CPU. ROCm, SYCL and OpenVINO are chosen explicitly only. Updating the pin is a
reviewed change that must pass the end-to-end test.

**Install flow** (`LlamaCpp::install`): for each asset, download into `partial/` through `egress` (allow-listed HTTPS host,
resumable, SHA-256 verified before use), unpack defensively into `engines/<id>.new` (zip or tar.gz; only expected flat
names, size caps, symlinks only to plain sibling names, no path escapes), mark executable, then run the **self-test**:
`llama-server --version` (30 s timeout) and `--list-devices`, whose output becomes the build's device list. If the
self-test fails, the new directory is discarded and the previous build of that id stays active. If it passes, the new
directory replaces the old one by rename. The previous *choice* is remembered (`engines.previousBuild`) so the user can
roll back to it from Engines (`rollback_build`).

**Verified.** On macOS (Apple Silicon), the Metal build b11669 was installed through the app flow with its checksum and
self-test, and served Qwen2.5-0.5B-Instruct Q4_K_M (downloaded from Hugging Face, SHA-256 matched Hugging Face's LFS id)
for OpenAI chat (streaming and not), Anthropic messages, Ollama chat and JSON-schema constrained output, at about 213
generated tokens per second on an M4 Pro. Windows, Linux, CUDA and Vulkan builds have not been run for real yet; the CI and
unit tests cover archive handling, the self-test and rollback metadata with fakes.

## Consequences

- A crashing or hanging engine costs one request, not the server or another model.
- Engines cannot see the network or the user's environment, and cannot be reached by anything but Pitwall.
- Adding an engine that speaks the OpenAI protocol needs only a pack; one with a different protocol needs an `Engine`
  implementation.
- One process per model costs memory per model (accounted for in ADR 0006) but isolates failures.

## Not built yet

- MLX, vLLM, whisper.cpp, reranker adapters: `Planned` stubs only. Pack engines have no grammar-constrained output unless
  the engine supports it and no `/props`.
- Automatic restart with backoff: `pitwall_core::backoff` (1 s to 60 s, reset after 5 healthy minutes, give up after 5 crashes
  in 10 minutes) is written and tested but the server does not use it. A dead engine is dropped and reloaded on the next request.
- Below-normal process priority is applied on Windows only; Unix children run at normal priority.
- Memory caps on engine processes beyond the fit check and release policy.
- Real runs on Windows, Linux, CUDA and Vulkan.
