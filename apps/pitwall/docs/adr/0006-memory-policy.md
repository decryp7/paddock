# 6. Memory policy: load on use, release when idle, never touch a busy model

- Status: accepted (2026-10-22)
- Related: ADR 0004, ADR 0005; code: [`pitwall_core::memory`](../../crates/pitwall-core/src/memory.rs),
  [`pitwall_core::fit`](../../crates/pitwall-core/src/fit.rs), [`pitwall_core::config`](../../crates/pitwall-core/src/config.rs),
  [`serving.rs`](../../crates/pitwall-server/src/serving.rs) (`ensure`, `tick`), [`system.rs`](../../crates/pitwall-server/src/system.rs)

## Context

Models are large. A user's machine also runs a browser, an IDE and sometimes a game. Pitwall should keep a model hot
while it is useful, give the memory back when it is not, never push the machine into swap, and never kill a request
that is being answered.

## Decision

All decisions are pure functions over values the server measures (`System`: total and available RAM, free VRAM, OS
pressure flags, foreground-busy flag; `Hot`: per loaded model its RAM and VRAM, pinned, active requests, loading, last used).

**Load on first use.** `Serving::ensure` loads a model when a request needs it and reports the cold-start time
(`X-Pitwall-Cold-Start-Ms`). A model's context is the request's/preferences' value, else `models.defaultContext` (8192),
capped at the model's trained context and at least 512.

**Fit and load plan.** `fit::need` estimates memory from weights, projector, KV cache per token and compute overhead
(512 MiB) plus host overhead (384 MiB); with a discrete GPU of at least 2 GiB free it splits the need between VRAM and RAM
(all, partial or no offload); on Apple Silicon video memory is RAM. `memory::plan_load` returns one of:
`Proceed` (load now, maybe with partial offload), `Evict` (unload these idle, unpinned models, least recently used first,
then load), `Wait` (busy models hold the memory; the server retries every 500 ms, up to 40 times) or `Refuse` (cannot fit
even with everything idle released: typed `insufficient_memory`, 503). A load never leaves less than `minFree` available,
never exceeds `maxHot` models or the optional `hotCap`. `fit::assess` is the same estimate shown before downloading
("will this fit?"): `fits` when the need is at most 80% of available RAM, `tight` when idle release would make room and
it is within 85% of total RAM, else `wontFit`; `unknown` when the size is not known.

**Release rules, in precedence order** (`memory::plan_release`, evaluated every 5 seconds by `Serving::tick`):
1. A model serving a request, or still loading, is never released.
2. Pressure: available RAM below `pressureAvailablePercent` of total, or the OS signals pressure: unload idle models,
   unpinned before pinned, least recently used first, until the threshold is met. Pinned models go only on critical OS pressure.
3. Idle: an unpinned model idle for its idle time (per-model override, else `idleUnloadSecs`) is unloaded.
4. Deep idle: nothing used for `deepIdleSecs` unloads every unpinned model.
5. Yield: while a foreground full-screen app is detected and `yieldToForeground` is on, idle times become a quarter (at least 30 s).

**Presets** (`Preset`, applied with `apply_preset`; editing a value makes it `custom`):

| Preset | Idle unload | Deep idle | Pressure below | Min free | Max hot | Yield |
|---|---|---|---|---|---|---|
| Low memory | 2 min | 10 min | 25% available | 2 GiB | 1 | yes |
| Balanced (default) | 10 min | 1 h | 15% | 1.5 GiB | 3 | yes |
| Performance | 1 h | 8 h | 8% | 1 GiB | 6 | no |

Per model: "keep loaded" (pinned) and its own idle time and context (`models.prefs`). The dashboard shows each model's idle
countdown (`idle_release_at`) and every release is an event with its reason (`idle`, `deepIdle`, `pressure`, `yield`,
`manual`, `shutdown`, `failed`, `evicted`). "Release memory now" unloads every idle model.

## Consequences

- Memory is given back without user effort and a busy model is never touched (tests cover this).
- A cold request pays the load time once; presets trade that against memory.
- Estimates are estimates: the fit check leaves margin and the real resident size is read from the engine process
  (`rss`) after loading.

## OS signals and context release (built)

`pitwall_server::os_signals` reads the OS every tick: Linux PSI (`/proc/pressure/memory`), macOS `sysctl
kern.memorystatus_vm_pressure_level` and `pmset -g assertions` (a display-sleep assertion means a full-screen app), Windows
`QueryMemoryResourceNotification` and `SHQueryUserNotificationState`. `pitwall_core::pressure` holds the pure parsing and
thresholds (PSI `some` avg10 >= 10 is warn, `full` avg10 >= 5 is critical) and feeds `memory::System`. A source that is missing
reads as normal. `ReleaseContext` erases the engine's slots (`POST /slots/{n}?action=erase`) once per idle period and the next
request resets it.

## Not built yet

- Clearing the context frees the engine's cached prompts and KV contents; the KV buffer itself is allocated for the engine's
  life and is returned only when the weights are unloaded.
- Foreground detection is not available on Linux. The OS-pressure and foreground readings (`os_signals.rs`) have run for real
  on macOS only; the Windows and Linux readers are exercised by their CI runners.
- Notifications for each release (`notifyOnRelease`) are a stored setting; the shell does not act on it yet.
