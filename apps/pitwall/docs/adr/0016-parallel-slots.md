# 16. Parallel slots per model and what they cost

Status: accepted. The number is provisional; it is allocated again when this branch merges (see `docs/suite-lessons.md`).

## Context

llama-server serves several requests at once on one loaded model through *slots* (`--parallel N`). Pitwall started every model
with the same global count, so the user could neither trade memory for concurrency nor ask "how much context or concurrency
fits?". The fit check answered only yes or no for one context length. The real semantics of the engine's options matter here:

- `--ctx-size C` is the **total** number of KV positions, shared by all slots; each slot gets `C / N` tokens.
- The KV cache is allocated up front, so the memory is `C x bytes per token` whether the slots are busy or not.

## Decision

- **The configured context is the context per slot.** Choosing more slots must not silently shrink what one request may send
  (least astonishment), so Pitwall launches with `--ctx-size = per-slot context x slots` and `--parallel = slots`
  (`Slots::launch_context`). The alternative, a fixed total split into smaller and smaller slots, remains available to the user by
  lowering the context; the fit table shows both sides (longest context each slot can have at every slot count).
- **`Slots` is a newtype, 1 to 16** (`pitwall_core::slots`), parsed once at the edge. It is stored per model in `ModelPrefs.slots`
  (`settings.toml`, optional) with a default from `models.default_slots` (2). A value that is out of range or the wrong type in
  the file reads as unset instead of failing the whole file; an invalid default falls back to the backup like any other invalid
  setting. No version bump: the key is optional.
- **Memory model.** Total = weights + projector + compute buffers + slots x per-slot KV, where the per-slot KV is
  `context x kv bytes per token`; the bytes per token are `2 x layers x kv heads x head size x 2 bytes` from the GGUF header
  (`GgufInfo::kv_bytes_per_token`, f16) or, when the header lacks the layout, 128 KiB (a typical 7-9B model; before a download,
  scaled from the file size). The answer says when it is an estimate (`kvExact`). The load plan, the loaded footprint and the
  fit verdicts all use the total number of KV positions (`fit::need_tokens`).
- **A pure solver** (`slots::solve`, `max_context`, `max_slots`) answers from the model's size and the machine's `Room`: the
  longest context per slot at each of 1, 2, 4, 8 slots (and the chosen count), the most slots that fit at a context, what each
  slot adds, and the usual verdict (fits, tight, won't fit), each also "with room to spare". Zero free memory, an unknown size,
  a huge context and a trained-context cap are covered by tests; arithmetic saturates and nothing panics.
- **Scheduling uses the slots.** The scheduler has a per-model concurrency (`Scheduler::set_model_slots`); requests beyond a model's
  slots wait in the queue. A loaded model keeps the count it was loaded with; a model that is not loaded uses its preference.
- **A change applies on the next load.** `set_model_slots` never unloads or restarts a model, least of all a busy one; the reply
  says `appliesOnNextLoad` and the UI says so. (Regression test: a busy model stays loaded and unchanged when its slots change.)
- **Management API:** `set_model_slots` (null returns to the default), `model_fit` (the solver's table; optional context and
  slots), `slots`, `slotsCustom` and `loadedSlots` on `local_models`, `slots` on loaded models and `maxSlots` on a Discover
  variant (how many requests fit at the default context). All validated in Rust. Tauri commands are async.
- **Pack engines** receive `{context}` as the context per slot and may use the new `{slots}` placeholder.

## Consequences

- Eight slots at 8,192 tokens hold eight times the KV of one slot; the UI shows "adds ~X GB" for each step before anything is
  saved, so the cost is visible.
- The default of two slots doubles the KV memory of a model compared with a single slot, in exchange for a second app being
  served at the same time; users with little memory pick one slot (or the Low memory preset's single hot model).
- A KV cache quantisation option would change the bytes per token only; the model is one function.
- Which slot serves a request matters for prompt-prefix reuse: ADR 0019 adds slot affinity (`id_slot`) on top of these slots.
