# Follow-ups

Work that is known, decided and waiting for something else. Each entry has a date, the reason it waits and what ends the wait.

## Shared-code follow-ups (2026-10-29)

Shared code (`crates/`, `ui/`) is frozen until the Dyno and Spotter tags. After the tags:

1. **Move the benchmark results-block renderer and the interleaved A/B timer to `paddock-bench`.** Today `RunInfo`, `ResultRow`,
   `render_results_block` and `time_ab` live in `sighter_core::bench` (they are generic; nothing in them is Sighter's). The
   clock, the sample statistics and the results files are already `paddock-bench`'s. Sighter then deletes its copies and keeps
   only the budget constants. Ends the wait: the tags. Owner: whoever integrates `paddock-bench`.
2. **Wrap `paddock-secrets`' `Secret` instead of keeping a second `Secret` type** in `sighter_core::hud::mask`. The masking
   function and the "a secret shows only as `•••• secret NAME`" rule stay Sighter's; the value storage and its redacting
   `Debug` should be the shared type's. Ends the wait: the tags.
3. **Check whether the trace bus belongs on `paddock-live`.** `sighter_core::hud::trace` has a bounded ring and a
   drop-oldest subscriber fan-out; `paddock-live` may already offer the same. If it does, use it and keep only the typed
   events. Needs a read of `paddock-live` first (ER-22). Ends the wait: the tags.
4. **Fix the "rust + ui" label in the plan summary** (`scripts/affected.mjs` marks Sighter as `rust + ui` although it has no
   UI yet). Planner and its tests are shared tooling. Ends the wait: the tags, or earlier if the planner owner agrees.

## ADR-dependent (2026-10-29; the wait on the ADR ended 2026-10-30)

ADR 0001 (the object detector) was accepted by the user on 2026-10-30 and is on this branch. What remains is ordered by the
[stage 2 plan](plans/stage-2.md): model-, GPU- or UI-related work (ONNX Runtime, model download, capture adapters, the HUD
overlay and live view) waits only for its package's dependencies there, and for the shared UI.
