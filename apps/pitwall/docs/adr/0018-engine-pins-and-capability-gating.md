# 18. Engine version pins, rollback and capability gating

Status: accepted. The number is provisional; it is allocated again when this branch merges (see `docs/suite-lessons.md`).

## Context

Pitwall installs official llama.cpp builds from a pinned catalog and self-tests them, but a user could not say "this
model stays on that build", could not go back to a build that worked, and a request for a feature the build in use does
not have (vision, tools, embeddings, rerank, JSON-schema output) failed obscurely inside the engine. Other local servers
gate features by llama.cpp release number and show an upgrade hint (Jan gates Multi-Token Prediction on `b9193` and newer);
we want the same, data-driven and explained, in all three API families.

An installed build here is one folder per build id (`cpu`, `vulkan`, `cuda-13`, `metal`, ...), each with the release tag it
was installed from and its self-test result in `build.json`. Updating a build replaces its folder, so "versions" of one id
are not kept side by side.

## Decision

- **A typed `BuildId`** (`pitwall_core::runtimes::BuildId`) is the id of an installed build, parsed once at every edge
  (IPC, the pin file). A pin, a history entry and a rollback target are `BuildId`s, never strings.
- **Selection order, first match wins** (`runtimes::pins::select`, pure): the model's own pin, then the global pin, then
  automatic (the build `inference_build` prefers, else Metal, CUDA, Vulkan, CPU, among builds that passed their self-test).
  Pins are **strict**: a pin to a build that is not installed, or did not pass its self-test, is the typed error
  `PinError` ("The engine build X is pinned for Y but is not installed. Install it in Engines, or remove the pin."); it never
  silently falls back, because someone chose it on purpose. Setting such a pin is refused the same way.
- **Rollback** (`pins::rollback_target`, pure) goes to the most recent earlier build that is installed **and passed its
  self-test**; a build that failed is never a target, and the refusal names it ("failed their self-test, so Pitwall will not
  go back to them"). The earlier builds are remembered (at most 8, newest first) whenever the build serving models in
  general changes: a pin, the build choice, an install. Rollback pins the target for all models, so it stays put until the
  user unpins; the page says so before asking for confirmation. The old `previous_build` setting is removed (pre-v1, no
  compatibility).
- **State** is `engine-pins.json` (global pin, per-model pins, history): versioned, atomic with a `.bak`, bounded (256 model
  pins), and tolerant of damage like `presets.json`. A damaged file falls back to the backup, then to no pins; one bad entry
  is skipped; a file from a newer Pitwall is not guessed at. A pin whose build is gone is reported on the Engines page and
  in the event log, not fatal.
- **A busy engine is never swapped.** The binary is chosen when a model is loaded (`LlamaCpp::start`); a loaded model keeps
  running on the build it started with. Every pin change and rollback returns `appliesOnNextLoad`, the loaded models that a new
  selection would move, and the toast says "Loaded models keep their current engine until they are loaded again".
- **A capability matrix as data** (`runtimes/capabilities.json`, embedded next to `llama-runtimes.json`): per capability a
  label, a minimum llama.cpp release number (`minBuild`) and optional `unsupportedBackends` (build id to reason). The floors
  are the first release containing the change that brought the feature to `llama-server` (verified, see "Verified floors" below)
  and are reviewed when the catalog pin moves. **`Matrix::gate(needed, build)`** is pure: `Allowed`, `NeedsBuild { capability, min_build, have, hint }` or
  `NotSupported { capability, reason }`. A backend that can never serve a capability wins over "too old"; among too-old
  answers the newest requirement is reported so one upgrade fixes everything; a build of unknown number (a custom
  `llama-server`) is trusted.
- **Where the gate is applied** (`CapabilityGate`, implemented by the llama.cpp manager; pack engines declare their own
  capabilities and are not gated): after the model is resolved and before the queue, a chat request is checked for vision
  (images), tools and JSON-schema output, an embeddings request for embeddings. Refusals are `ApiError`s with new kinds, so
  every family renders them in its own shape: `engine_build_too_old` (503, OpenAI `server_error`, Anthropic
  `overloaded_error`, Ollama `{"error": ...}`), `capability_unsupported` (400, `invalid_request_error`) and
  `engine_pin_unavailable` (503). The message names the model, the feature, the build in use and the build needed. With no
  engine installed the gate passes and loading the model explains that.
- **Listings and selection follow the gate.** `/x/v1/models` lists only the capabilities the serving build can provide in
  `capabilities` and the rest in `unavailable` (capability, reason, `minBuild`), and `auto:` / preset selection treats a
  model as lacking such a capability, so a client never picks a model for something it cannot do here.
- **Management API** (all Tauri commands async): `engine_pins` (state, `rollbackTo` or why not), `set_global_pin`,
  `set_model_pin`, `rollback_build` (now returns the change), `build_capabilities` (per installed build, in words).
  Queries have no side effects.
- **UI.** Engines: a Version pin toggle per build (accent plus the word "Pinned", `aria-pressed`), a Features list per build
  in words (Supported / Needs bNNNN / Not supported, with the hint), and Roll back naming its target with a confirmation.
  Models: a small build menu per model and "pinned to cuda-13 b11669". Failures of the pin calls leave the rest of both pages
  working.

## Consequences

- Rollback goes back to a different build id (for example from CUDA to Vulkan), not to an older release of the same id, because
  only one release per id is kept. Keeping superseded releases side by side is a possible follow-up; the selection and
  rollback logic already work on `BuildId`s and would not change.
- The matrix is only as accurate as its floors; a wrong floor is a data edit, not a code change, and the catalog's own pin
  (`b11669`) satisfies every entry (a test checks it).
- Gating is by release number, not by probing the binary; probing (`--help` flags) was rejected as slower, platform specific
  and still needing a table for what the flags mean.

## Verified floors

Checked on 2026-10-23 against the ggml-org/llama.cpp git history: each pull request's merge commit was mapped to the first
`bNNNN` release tag that contains it (`git describe --contains --tags --match 'b[0-9]*' <commit>`). The earlier numbers were
estimates; these replace them. The documentation (`tools/server/README.md`, `docs/multimodal.md`) names the pull requests but
no build numbers, so the numbers come from the tags.

| Capability | Change | Merged | First release (floor) |
|---|---|---|---|
| JSON-schema output | #5978 json-schema-to-grammar improvements, added to the server | 2024-03-21 | b2487 |
| Tool calling | #9639 tool call support (generic and native templates) with lazy grammars | 2025-01-30 | b4599 |
| Vision (mtmd) | #12898 server: vision support via libmtmd | 2025-05-09 | b5331 |
| Embeddings | #5190 server: OpenAI-compatible embeddings | 2024-01-29 | b2004 |
| Reranking | #9510 llama: add reranking support (`/rerank` in the server) | 2024-09-28 | b3841 |

These are the first releases that contain the feature, not releases known to be free of its early bugs (for example the
embedding normalisation fix #5956 and the TEI rerank format #11942, b4738, came later); raise a floor in the data file if a
real build proves unreliable. Hardware backends are not verified here; `unsupportedBackends` stays empty.
