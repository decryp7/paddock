# 14. Model presets and aliases

Status: accepted. The number is provisional; it is allocated again when this branch merges (see `docs/suite-lessons.md`).

## Context

Apps hard-code model ids, so changing which model serves an app means changing the app, and parameters such as the
temperature or a house system prompt live in every caller. The roadmap approved aliases: an app asks for a name such as
`debrief-explain` and the user decides which model and parameters serve it. Capability selection (`auto:chat,fast`, ADR 0003)
already lets an app say what it needs; a preset lets the user decide.

## Decision

- **A preset is pure data in `pitwall_core::presets`**: an `AliasName` (parsed once: lowercase letters, digits and `-_.`, up to
  48, starting with a letter or digit; `auto` and `default` are reserved because they already mean Pitwall's own choice), a
  `Target` (one `ModelId`, or capability `Requirements` written `auto:chat,fast`; the text form round-trips) and optional
  context, temperature, top-p, max tokens, system prompt and engine. Every range and length is checked in Rust
  (`ModelPreset::checked`), at most 64 presets, system prompt at most 8 KB.
- **Resolution sits next to selection.** `Serving::resolve` looks up an alias only when the name is not an installed model
  (a real model's name always wins; a preset that a later download shadows is listed as *shadowed*). A fixed target must be
  installed, else the typed `PresetError::StaleTarget`; a capability target runs the same `select::choose` as `auto:` names,
  with the request's vision and tool needs added. Both failures are `ErrorKind::PresetTarget` (404, code
  `preset_target_unavailable`) with a message that names the preset and says where to fix it.
- **Request values win.** `presets::apply` fills temperature, top-p and max tokens only where the request set none, and puts
  the system prompt in front only when the request has no system message at all (OpenAI, Anthropic `system` and Ollama
  system messages all become a system message first). Context and engine are load-time: they apply when the model is loaded for the
  request; a model that is already loaded keeps what it was loaded with until it is released, which the editor says. A
  crash-restart loads with the model's own preferences.
- **Creation cannot clash.** An alias that equals an installed model id is refused, a fixed target must be installed, the
  engine must be llama.cpp or a loaded pack, duplicates and the cap give conflicts. Management calls: `presets`,
  `create_preset`, `update_preset` (also renames), `delete_preset` and `resolve_preset`, the editor's live check, which takes
  plain text, never changes anything and answers with the problem in words plus what the target would serve.
- **Visible to clients and the user.** An alias that resolves is listed in `GET /v1/models`, `/v1/models/{id}` and `/api/tags`
  with its model's details (not in `/api/ps`, which lists running models); a stale one is not offered. Answers carry
  `X-Pitwall-Alias` next to `X-Pitwall-Model` (the real model) and `X-Pitwall-Chosen-By`; the request record has an `alias`
  field that the inspector shows as "Preset asked for" and "Resolved to". Response bodies name the real model.
- **State is `presets.json`**: versioned, written atomically with a `.bak`, loaded primary then backup then empty, and one damaged
  or duplicate entry is skipped instead of losing the file (the app logs an event with the count). A failed save changes
  nothing in memory.

## Consequences

- No new outbound connection; presets hold names, numbers and prompts the user typed, kept on this machine only.
- An app that asks for a removed model's alias gets a clear 404 instead of a silently different model.
- Preset parameters are defaults, not enforcement; an app that sends its own sampling values keeps them.
- Per-preset context on an already loaded model needs a reload; a future "reload with this preset" action can add that.
