# 20. llama.cpp is the only engine

- Status: accepted (2026-10-23)
- Supersedes: the engine-pack parts of [ADR 0011](0011-declarative-packs.md) and the pack and planned adapters of
  [ADR 0004](0004-engine-abstraction-and-process-model.md)
- Related: ADR 0018 (pins), ADR 0019 (caching); code: [`engines.rs`](../../crates/pitwall-server/src/engines.rs),
  [`engine_setup.rs`](../../crates/pitwall-server/src/engine_setup.rs), [`packs.rs`](../../crates/pitwall-core/src/packs.rs)

## Context

Version 1.0.1 shipped a generic engine-pack adapter, a built-in llamafile pack and three "planned" engines (MLX, vLLM,
whisper.cpp) that started nothing. In use this was confusing: llamafile could be picked per model and was not what
people wanted, the Engines page listed engines that cannot run, and a Mac with no llama.cpp build could not install one
because the page treated it like a planned engine. Every model Pitwall serves is a GGUF file and llama.cpp serves GGUF
on every platform with CPU, Vulkan, CUDA and Metal builds. Several engines were complexity nobody used.

## Decision

- **llama.cpp is the only engine.** The engine-pack schema and loader, the generic pack adapter (`PackEngine`), the
  built-in llamafile pack, the per-model and per-preset engine choice (API, UI, CLI) and the `EngineKind` variants
  without an implementation (MLX, vLLM, whisper.cpp, pack) are removed.
- **Model catalog packs stay** (`*.models.toml`, ADR 0011): curated models for the "will this fit?" check and the
  recommendations. Engine packs (`*.engine.toml`) are now reported as an unknown pack file.
- **The internal `Engine` trait stays.** It is the seam the tests use (an in-process fake and a real child process that
  crashes on purpose); `LlamaCpp` is its one production implementation. `Engine::ready` lets an engine make callers wait
  while it sets itself up.
- **llama.cpp installs itself.** On start, and whenever a model needs a build and none is installed, Pitwall downloads
  the build suggested for the machine (checksum and self-test as in ADR 0004). Model loads and model downloads wait for it
  (downloads stay queued), the Dashboard and the llama.cpp page show the progress, and offline mode or a network error
  ends in a message with Retry. The user can still install, pick or pin another build (ADR 0018).
- **The Engines page is the llama.cpp page**: the builds catalog (the suggested build first), installed builds, pins,
  self-test results and processes.
- **Existing installs.** An engine folder left by 1.0.1 (`engines/pack-llamafile`) is listed on the llama.cpp page with
  "Remove llamafile (size)". It is never deleted without that click. A per-model or per-preset engine written by an older
  version is ignored (llama.cpp serves it). There is no other compatibility code (pre-v1).

## Consequences

- Less code, one documented behaviour, a first run that works without a decision.
- No other local engine can be added by a user file any more. If another engine is ever needed it is a new adapter behind
  the `Engine` trait, decided in a new ADR.
- The wire contract loses `setModelEngine`, `PackEngine` and the engine fields of presets and models, and gains
  `engineSetup` (overview and live update), `retryEngineSetup`, `removeLeftover` and `Engine.leftovers`.
