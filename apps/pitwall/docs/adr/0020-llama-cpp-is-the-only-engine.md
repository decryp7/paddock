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
  (downloads stay queued), the Dashboard and the llama.cpp page show the progress, and a network error
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

## Amendment 2026-11-12: whisper.cpp for speech to text

Decided by the user: the Playground's speech input uses Whisper small Q8_0 run by whisper.cpp (2026-11-11, after the speech
spike, [research note](../research/speech-spike-2026-11-11.md)), and whisper.cpp is delivered by downloading a pinned
release (W1, 2026-11-12). This amends "llama.cpp is the only engine" for that one purpose; it is the new decision the
Consequences above ask for.

- **whisper.cpp is a second engine, for speech to text only.** It serves no chat, embedding or rerank request, is not
  listed among chat engines, cannot be chosen per model or preset, and its model (`ggml-small-q8_0.bin`, not GGUF) lives
  apart from the model library. llama.cpp stays the only engine for everything else.
- **Its own pin.** A catalog in the shape of `llama-runtimes.json` (release, tag, commit, per platform the asset's URL, size
  and SHA-256), checked as llama.cpp's builds are: SHA-256 before unpacking, the unpack keep-list, a self-test, roll back.
  It is downloaded on first use as a job with progress and Cancel (ER-17, ER-18), from hosts already in
  `privacy::DOWNLOAD_HOSTS`. Where a platform has no official release binary (whisper.cpp publishes none for macOS: only
  an xcframework), the user is asked at that point; nothing is built or fetched for it without that answer.
- **Its own port.** whisper-server runs as a child of `pitwall-server`, like llama-server: through
  `pitwall_server::process`, bound to 127.0.0.1 on a port Pitwall picks, never with `--convert` (it runs ffmpeg), reached
  only through `pitwall_loopback`. The server talks to it behind the `Transcriber` port (`pitwall_server::speech`); no C++
  runs inside a Pitwall process.
- **Windows runtime.** The official Windows builds import the VC++ runtime (`vcruntime140`, `msvcp140`, and `vcomp140` on
  x64), as llama.cpp's do. The x64 installer already installs it (`ff40d2a9f`, on `train/2026-11-11-5`, not in a released
  tag yet); Windows on Arm needs `vc_redist.arm64`, which the installer does not bring (a WP-7 engine-slice item).

Evidence read for this amendment (ADR 0091): `origin/train/2026-11-11-5` `607bb7928`, `origin/feat/pitwall-speech`
`cbf37f568`; the note's release facts are at whisper.cpp `v1.9.5` / `b5454`, commit `d1be6fde1`.
