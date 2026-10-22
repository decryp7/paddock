//! When Pitwall gives memory back, and whether a model may load (ADR 0006). Pure decisions over values the server
//! measures: the hot models, the system's memory and the policy.
//!
//! Rules, in order of precedence:
//! 1. A model that is serving a request or loading is **never** released.
//! 2. **Pressure**: below the available-memory threshold (or when the OS signals pressure) idle, unpinned models are
//!    unloaded least recently used first until the threshold is met; pinned models only when the OS reports critical
//!    pressure.
//! 3. **Idle**: an unpinned model idle for its idle time (per-model override, else the policy's) is unloaded; with
//!    "release context first" its KV cache / context is released at half that time.
//! 4. **Deep idle**: when nothing has been used for the deep-idle time, every unpinned model is unloaded.
//! 5. **Yield**: while a foreground full-screen app is detected and yielding is on, idle times are a quarter (at
//!    least 30 s).
//!
//! Loads are planned with [`plan_load`]: proceed, evict idle models first, wait for busy ones, or refuse with a
//! typed error, never pushing the machine into swap.

use crate::ids::ModelId;
use crate::time::{Millis, HOUR, MINUTE, SECOND};
use crate::GIB;
use serde::{Deserialize, Serialize};

/// Why memory was released (shown in the events feed and the dashboard's timeline).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReleaseReason {
    /// The model was idle for its idle time.
    Idle,
    /// Nothing was used for the deep-idle time.
    DeepIdle,
    /// The system ran low on memory.
    Pressure,
    /// A foreground app needed the machine.
    Yield,
    /// The user asked.
    Manual,
    /// The server is stopping.
    Shutdown,
    /// The engine failed.
    Failed,
    /// Room for another model.
    Evicted,
}

/// A named set of memory settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Preset {
    /// Give memory back quickly; one model at a time.
    LowMemory,
    /// The default.
    #[default]
    Balanced,
    /// Keep models hot.
    Performance,
    /// The user's own values.
    Custom,
}

/// The memory policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    /// Unload a model idle this long.
    pub idle_unload: Millis,
    /// Unload everything after this long without any request.
    pub deep_idle: Millis,
    /// Release the context at half the idle time, the weights at the idle time.
    pub release_context_first: bool,
    /// Pressure when available RAM is below this percentage of total.
    pub pressure_available_percent: u8,
    /// A load never leaves less than this available.
    pub min_free: u64,
    /// Most hot models.
    pub max_hot: usize,
    /// Most memory hot models may hold together.
    pub hot_cap: Option<u64>,
    /// Shorter idle times while a foreground app is busy.
    pub yield_to_foreground: bool,
}

impl Policy {
    /// The values of a preset (`Custom` gives the balanced values as a starting point).
    pub fn preset(p: Preset) -> Policy {
        match p {
            Preset::LowMemory => Policy {
                idle_unload: 2 * MINUTE,
                deep_idle: 10 * MINUTE,
                release_context_first: true,
                pressure_available_percent: 25,
                min_free: 2 * GIB,
                max_hot: 1,
                hot_cap: None,
                yield_to_foreground: true,
            },
            Preset::Balanced | Preset::Custom => Policy {
                idle_unload: 10 * MINUTE,
                deep_idle: HOUR,
                release_context_first: false,
                pressure_available_percent: 15,
                min_free: 3 * GIB / 2,
                max_hot: 3,
                hot_cap: None,
                yield_to_foreground: true,
            },
            Preset::Performance => Policy {
                idle_unload: HOUR,
                deep_idle: 8 * HOUR,
                release_context_first: false,
                pressure_available_percent: 8,
                min_free: GIB,
                max_hot: 6,
                hot_cap: None,
                yield_to_foreground: false,
            },
        }
    }
}

impl Default for Policy {
    fn default() -> Self {
        Policy::preset(Preset::Balanced)
    }
}

/// A loaded model, as the policy sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hot {
    /// Which model.
    pub id: ModelId,
    /// Resident memory of its engine process.
    pub ram: u64,
    /// Video memory it holds.
    pub vram: u64,
    /// "Keep loaded".
    pub pinned: bool,
    /// Requests running on it.
    pub active: usize,
    /// Still loading.
    pub loading: bool,
    /// Last request ended (or the load finished).
    pub last_used: Millis,
    /// Its own idle time.
    pub idle_override: Option<Millis>,
    /// Its context was released already.
    pub context_released: bool,
}

impl Hot {
    fn idle(&self) -> bool {
        self.active == 0 && !self.loading
    }
}

/// The system's memory right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct System {
    /// Physical memory.
    pub total: u64,
    /// Available physical memory.
    pub available: u64,
    /// Free video memory, when known.
    pub vram_free: Option<u64>,
    /// The OS signals memory pressure (macOS warn, Windows low-memory notification).
    pub os_pressure: bool,
    /// The OS signals critical pressure.
    pub os_critical: bool,
    /// A foreground full-screen app or game is running.
    pub foreground_busy: bool,
}

impl System {
    fn threshold(&self, policy: &Policy) -> u64 {
        self.total / 100 * u64::from(policy.pressure_available_percent.min(90))
    }

    /// Whether the system is under memory pressure by the policy's threshold or the OS's signal.
    pub fn under_pressure(&self, policy: &Policy) -> bool {
        self.os_pressure || self.os_critical || self.available < self.threshold(policy)
    }
}

/// What to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Release the KV cache / context but keep the weights.
    ReleaseContext(ModelId, ReleaseReason),
    /// Unload the model.
    Unload(ModelId, ReleaseReason),
}

impl Action {
    /// The model the action concerns.
    pub fn model(&self) -> &ModelId {
        match self {
            Action::ReleaseContext(m, _) | Action::Unload(m, _) => m,
        }
    }
}

const MIN_YIELD_IDLE: Millis = 30 * SECOND;

fn idle_limit(m: &Hot, sys: &System, policy: &Policy) -> (Millis, ReleaseReason) {
    let base = m.idle_override.unwrap_or(policy.idle_unload);
    if policy.yield_to_foreground && sys.foreground_busy {
        ((base / 4).max(MIN_YIELD_IDLE).min(base), ReleaseReason::Yield)
    } else {
        (base, ReleaseReason::Idle)
    }
}

/// The releases due now. `last_activity` is when any request last ended or started.
pub fn plan_release(now: Millis, models: &[Hot], sys: &System, policy: &Policy, last_activity: Millis) -> Vec<Action> {
    let mut out: Vec<Action> = vec![];
    let unloaded = |out: &Vec<Action>, id: &ModelId| out.iter().any(|a| matches!(a, Action::Unload(m, _) if m == id));

    // Pressure first: LRU idle models until the threshold is met.
    if sys.under_pressure(policy) {
        let mut available = sys.available;
        let threshold = sys.threshold(policy);
        let mut candidates: Vec<&Hot> = models.iter().filter(|m| m.idle()).collect();
        // Unpinned before pinned, then least recently used first.
        candidates.sort_by_key(|m| (m.pinned, m.last_used));
        for m in candidates {
            let still = available < threshold || sys.os_pressure || sys.os_critical;
            if !still || (m.pinned && !sys.os_critical) {
                break;
            }
            out.push(Action::Unload(m.id.clone(), ReleaseReason::Pressure));
            available = available.saturating_add(m.ram);
            if available >= threshold && !sys.os_critical {
                break;
            }
        }
    }

    let deep = now.saturating_sub(last_activity) >= policy.deep_idle && models.iter().all(Hot::idle);
    for m in models.iter().filter(|m| m.idle() && !m.pinned) {
        if unloaded(&out, &m.id) {
            continue;
        }
        let idle_for = now.saturating_sub(m.last_used);
        let (limit, reason) = idle_limit(m, sys, policy);
        if deep {
            out.push(Action::Unload(m.id.clone(), ReleaseReason::DeepIdle));
        } else if idle_for >= limit {
            out.push(Action::Unload(m.id.clone(), reason));
        } else if policy.release_context_first && !m.context_released && idle_for >= limit / 2 {
            out.push(Action::ReleaseContext(m.id.clone(), reason));
        }
    }
    out
}

/// When the idle rule will release a model (for the UI's countdown); `None` for pinned, busy or loading models.
pub fn idle_release_at(m: &Hot, sys: &System, policy: &Policy) -> Option<Millis> {
    (m.idle() && !m.pinned).then(|| m.last_used.saturating_add(idle_limit(m, sys, policy).0))
}

/// What loading a model needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Need {
    /// System memory.
    pub ram: u64,
    /// Video memory for a full offload (0 on CPU-only machines).
    pub vram: u64,
}

/// The outcome of planning a load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadPlan {
    /// Load now; `partial_offload` when not all of it fits the free video memory.
    Proceed {
        /// Run part of it on the CPU.
        partial_offload: bool,
    },
    /// Unload these idle models first (LRU order), then load.
    Evict {
        /// The models to unload.
        models: Vec<ModelId>,
        /// Run part of it on the CPU.
        partial_offload: bool,
    },
    /// Not now: busy models hold the memory; queue the request until one is released.
    Wait,
    /// It cannot fit even with everything idle released.
    Refuse(Refusal),
}

/// Why a load is refused (becomes the typed `insufficientMemory` error).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{model} needs about {need_gib:.1} GiB of memory but only {have_gib:.1} GiB can be made free. Close other apps, choose a smaller quantisation or a shorter context.", need_gib = *need as f64 / GIB as f64, have_gib = *have as f64 / GIB as f64)]
pub struct Refusal {
    /// The model.
    pub model: ModelId,
    /// Bytes it needs.
    pub need: u64,
    /// Bytes that could be made available.
    pub have: u64,
}

/// Plans loading `model` (not yet hot) needing `need`.
pub fn plan_load(model: &ModelId, need: Need, models: &[Hot], sys: &System, policy: &Policy) -> LoadPlan {
    let usable = |available: u64| available.saturating_sub(policy.min_free);
    let hot_total = |ms: &[&Hot]| ms.iter().map(|m| m.ram).sum::<u64>();
    let mut remaining: Vec<&Hot> = models.iter().filter(|m| &m.id != model).collect();
    let mut available = sys.available;
    let mut vram_free = sys.vram_free;
    let fits = |available: u64, remaining: &[&Hot]| {
        need.ram <= usable(available)
            && remaining.len() < policy.max_hot.max(1)
            && policy.hot_cap.is_none_or(|cap| hot_total(remaining) + need.ram <= cap)
    };
    let vram_ok = |vf: Option<u64>| need.vram == 0 || vf.is_none_or(|f| f >= need.vram);
    let mut evict = vec![];
    let mut idle: Vec<&Hot> = remaining.iter().copied().filter(|m| m.idle() && !m.pinned).collect();
    idle.sort_by_key(|m| m.last_used);
    for m in idle {
        if fits(available, &remaining) && vram_ok(vram_free) {
            break;
        }
        evict.push(m.id.clone());
        available = available.saturating_add(m.ram);
        vram_free = vram_free.map(|f| f.saturating_add(m.vram));
        remaining.retain(|r| r.id != m.id);
    }
    if fits(available, &remaining) {
        let partial_offload = !vram_ok(vram_free);
        // Don't evict for video memory alone when it doesn't get us a full offload anyway.
        let needed_for_ram: Vec<ModelId> = if partial_offload { trim_to_ram(&evict, models, need, sys, policy) } else { evict };
        return if needed_for_ram.is_empty() {
            LoadPlan::Proceed { partial_offload }
        } else {
            LoadPlan::Evict { models: needed_for_ram, partial_offload }
        };
    }
    let busy_ram: u64 = remaining.iter().filter(|m| !m.idle() || m.pinned).map(|m| m.ram).sum();
    let busy_or_pinned_exist = remaining.iter().any(|m| !m.idle());
    if busy_or_pinned_exist && need.ram <= usable(available.saturating_add(busy_ram)) {
        LoadPlan::Wait
    } else {
        LoadPlan::Refuse(Refusal { model: model.clone(), need: need.ram + policy.min_free, have: available })
    }
}

/// The prefix of `evict` that is enough for system memory and the hot-model limits alone.
fn trim_to_ram(evict: &[ModelId], models: &[Hot], need: Need, sys: &System, policy: &Policy) -> Vec<ModelId> {
    let mut available = sys.available;
    let mut remaining: Vec<&Hot> = models.iter().collect();
    let mut out = vec![];
    for id in evict {
        let ok = need.ram <= available.saturating_sub(policy.min_free)
            && remaining.len() < policy.max_hot.max(1)
            && policy.hot_cap.is_none_or(|cap| remaining.iter().map(|m| m.ram).sum::<u64>() + need.ram <= cap);
        if ok {
            break;
        }
        out.push(id.clone());
        if let Some(m) = models.iter().find(|m| &m.id == id) {
            available = available.saturating_add(m.ram);
        }
        remaining.retain(|r| &r.id != id);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> ModelId {
        ModelId::parse(s).unwrap()
    }

    fn hot(name: &str, ram_gib: u64, last_used: Millis) -> Hot {
        Hot {
            id: id(name),
            ram: ram_gib * GIB,
            vram: 0,
            pinned: false,
            active: 0,
            loading: false,
            last_used,
            idle_override: None,
            context_released: false,
        }
    }

    fn sys(total_gib: u64, available_gib: u64) -> System {
        System { total: total_gib * GIB, available: available_gib * GIB, ..System::default() }
    }

    #[test]
    fn presets_differ_sensibly() {
        let low = Policy::preset(Preset::LowMemory);
        let bal = Policy::default();
        let perf = Policy::preset(Preset::Performance);
        assert!(low.idle_unload < bal.idle_unload && bal.idle_unload < perf.idle_unload);
        assert!(low.max_hot < bal.max_hot && bal.max_hot < perf.max_hot);
        assert_eq!(Policy::preset(Preset::Custom), bal);
        assert_eq!(Preset::default(), Preset::Balanced);
    }

    #[test]
    fn a_busy_model_is_never_unloaded() {
        let policy = Policy::preset(Preset::LowMemory);
        let mut busy = hot("busy", 8, 0);
        busy.active = 1;
        let mut loading = hot("loading", 8, 0);
        loading.loading = true;
        // Idle for days, deep idle, critical OS pressure, yield mode: still never touched.
        let s = System { os_pressure: true, os_critical: true, foreground_busy: true, ..sys(16, 0) };
        let actions = plan_release(10 * 24 * HOUR, &[busy.clone(), loading.clone()], &s, &policy, 0);
        assert!(actions.is_empty(), "{actions:?}");
        assert_eq!(idle_release_at(&busy, &s, &policy), None);
        // Load planning never evicts it either.
        let plan = plan_load(&id("new"), Need { ram: 8 * GIB, vram: 0 }, &[busy], &sys(32, 4), &policy);
        assert_eq!(plan, LoadPlan::Wait);
    }

    #[test]
    fn idle_models_are_unloaded_after_their_idle_time() {
        let p = Policy::default();
        let s = sys(32, 20);
        let fresh = hot("fresh", 4, 9 * MINUTE);
        let stale = hot("stale", 4, 0);
        let mut pinned = hot("pinned", 4, 0);
        pinned.pinned = true;
        let mut short = hot("short", 4, 9 * MINUTE);
        short.idle_override = Some(30 * SECOND);
        let a = plan_release(10 * MINUTE, &[fresh.clone(), stale, pinned.clone(), short], &s, &p, 10 * MINUTE);
        assert_eq!(a, vec![Action::Unload(id("stale"), ReleaseReason::Idle), Action::Unload(id("short"), ReleaseReason::Idle)]);
        assert_eq!(a[0].model().as_str(), "stale");
        assert_eq!(idle_release_at(&fresh, &s, &p), Some(19 * MINUTE));
        assert_eq!(idle_release_at(&pinned, &s, &p), None);
    }

    #[test]
    fn context_goes_first_when_asked() {
        let p = Policy { release_context_first: true, ..Policy::default() };
        let s = sys(32, 20);
        let m = hot("m", 4, 0);
        let a = plan_release(6 * MINUTE, std::slice::from_ref(&m), &s, &p, 6 * MINUTE);
        assert_eq!(a, vec![Action::ReleaseContext(id("m"), ReleaseReason::Idle)]);
        assert_eq!(a[0].model().as_str(), "m");
        let released = Hot { context_released: true, ..m.clone() };
        assert!(plan_release(6 * MINUTE, &[released], &s, &p, 6 * MINUTE).is_empty());
        assert_eq!(plan_release(10 * MINUTE, &[m], &s, &p, 10 * MINUTE), vec![Action::Unload(id("m"), ReleaseReason::Idle)]);
    }

    #[test]
    fn deep_idle_unloads_everything_unpinned() {
        let p = Policy::default();
        let s = sys(32, 20);
        let a = hot("a", 4, HOUR - MINUTE);
        let mut pinned = hot("p", 4, 0);
        pinned.pinned = true;
        let out = plan_release(2 * HOUR, &[a.clone(), pinned.clone()], &s, &p, HOUR - MINUTE);
        assert_eq!(out, vec![Action::Unload(id("a"), ReleaseReason::DeepIdle)]);
        // Not deep while something is busy.
        let mut busy = hot("b", 1, 0);
        busy.active = 1;
        let out = plan_release(HOUR + 5 * MINUTE, &[Hot { last_used: HOUR, ..a }, busy], &s, &p, 0);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn yield_mode_shortens_idle_times() {
        let p = Policy::default();
        let s = System { foreground_busy: true, ..sys(32, 20) };
        let m = hot("m", 4, 0);
        assert_eq!(plan_release(3 * MINUTE, std::slice::from_ref(&m), &s, &p, 3 * MINUTE), vec![Action::Unload(id("m"), ReleaseReason::Yield)]);
        let short = Hot { idle_override: Some(10 * SECOND), ..m.clone() };
        assert_eq!(idle_release_at(&short, &s, &p), Some(10 * SECOND), "never longer than its own idle time");
        let off = Policy { yield_to_foreground: false, ..p };
        assert!(plan_release(3 * MINUTE, &[m], &s, &off, 3 * MINUTE).is_empty());
    }

    #[test]
    fn pressure_releases_least_recently_used_idle_models_until_the_threshold() {
        let p = Policy::default(); // 15 % of 32 GiB = 4.8 GiB
        let s = sys(32, 2);
        let old = hot("old", 2, 100);
        let older = hot("older", 2, 50);
        let newest = hot("newest", 8, 200);
        let mut pinned = hot("pinned", 8, 0);
        pinned.pinned = true;
        let mut busy = hot("busy", 8, 0);
        busy.active = 2;
        let out = plan_release(300, &[old, older, newest, pinned.clone(), busy.clone()], &s, &p, 300);
        assert_eq!(
            out,
            vec![
                Action::Unload(id("older"), ReleaseReason::Pressure),
                Action::Unload(id("old"), ReleaseReason::Pressure),
            ],
            "stops once the threshold (4.8 GiB) is met"
        );
        // Pinned models go only under critical OS pressure, busy ones never.
        let crit = System { os_critical: true, ..sys(32, 30) };
        let out = plan_release(300, &[pinned, busy], &crit, &p, 300);
        assert_eq!(out, vec![Action::Unload(id("pinned"), ReleaseReason::Pressure)]);
        assert!(crit.under_pressure(&p));
        assert!(!sys(32, 30).under_pressure(&p));
        // OS pressure without critical: unpinned only, one by one until the signal clears.
        let warn = System { os_pressure: true, ..sys(32, 30) };
        let mut p2 = hot("pin2", 1, 0);
        p2.pinned = true;
        let out = plan_release(300, &[hot("x", 1, 0), p2], &warn, &p, 300);
        assert_eq!(out, vec![Action::Unload(id("x"), ReleaseReason::Pressure)]);
    }

    #[test]
    fn loads_proceed_evict_wait_or_refuse() {
        let p = Policy::default(); // min_free 1.5 GiB, max_hot 3
        let need = Need { ram: 6 * GIB, vram: 0 };
        let m = id("new");
        assert_eq!(plan_load(&m, need, &[], &sys(32, 20), &p), LoadPlan::Proceed { partial_offload: false });
        // Evict LRU idle first.
        let a = hot("a", 4, 10);
        let b = hot("b", 4, 5);
        let plan = plan_load(&m, need, &[a.clone(), b.clone()], &sys(32, 6), &p);
        assert_eq!(plan, LoadPlan::Evict { models: vec![id("b")], partial_offload: false });
        // The hot-model limit.
        let c = hot("c", 1, 20);
        let plan = plan_load(&m, need, &[a.clone(), b.clone(), c.clone()], &sys(32, 20), &p);
        assert_eq!(plan, LoadPlan::Evict { models: vec![id("b")], partial_offload: false });
        // A hot cap.
        let capped = Policy { hot_cap: Some(10 * GIB), ..p };
        let plan = plan_load(&m, need, &[a.clone(), b.clone()], &sys(32, 20), &capped);
        assert_eq!(plan, LoadPlan::Evict { models: vec![id("b")], partial_offload: false });
        // Busy models hold the memory: wait.
        let mut busy = hot("busy", 10, 0);
        busy.active = 1;
        assert_eq!(plan_load(&m, need, &[busy], &sys(16, 2), &p), LoadPlan::Wait);
        // Too big for this machine: refuse with numbers.
        let huge = Need { ram: 40 * GIB, vram: 0 };
        let LoadPlan::Refuse(r) = plan_load(&m, huge, &[a], &sys(32, 20), &p) else { panic!("refused") };
        assert_eq!(r.model, m);
        let msg = r.to_string();
        assert!(msg.contains("needs about 41.5 GiB") && msg.contains("24.0 GiB"), "{msg}");
        // Reloading a model that is already in the list ignores its own entry.
        assert_eq!(plan_load(&id("a"), need, &[hot("a", 4, 0)], &sys(32, 20), &p), LoadPlan::Proceed { partial_offload: false });
    }

    #[test]
    fn video_memory_is_freed_when_it_buys_a_full_offload() {
        let p = Policy::default();
        let need = Need { ram: 2 * GIB, vram: 6 * GIB };
        let m = id("new");
        let mut g = hot("gpu", 1, 0);
        g.vram = 4 * GIB;
        let s = System { vram_free: Some(3 * GIB), ..sys(32, 20) };
        assert_eq!(plan_load(&m, need, &[g.clone()], &s, &p), LoadPlan::Evict { models: vec![id("gpu")], partial_offload: false });
        // When even evicting can't give a full offload, nothing is evicted for video memory: partial offload.
        let s = System { vram_free: Some(GIB), ..sys(32, 20) };
        assert_eq!(plan_load(&m, need, &[g.clone()], &s, &p), LoadPlan::Proceed { partial_offload: true });
        // ...but evictions needed for system memory still happen.
        let s = System { vram_free: Some(GIB), ..sys(32, 3) };
        assert_eq!(plan_load(&m, need, &[g], &s, &p), LoadPlan::Evict { models: vec![id("gpu")], partial_offload: true });
        // Unknown video memory: assume it fits.
        assert_eq!(plan_load(&m, need, &[], &sys(32, 20), &p), LoadPlan::Proceed { partial_offload: false });
    }

    #[test]
    fn trim_handles_hot_limits_and_unknown_ids() {
        let p = Policy { max_hot: 1, hot_cap: Some(100 * GIB), ..Policy::default() };
        let need = Need { ram: GIB, vram: 0 };
        let out = trim_to_ram(&[id("a"), id("ghost")], &[hot("a", 1, 0)], need, &sys(32, 20), &p);
        assert_eq!(out, vec![id("a")]);
        let out = trim_to_ram(&[id("ghost"), id("x")], &[], need, &sys(32, 0), &p);
        assert_eq!(out, vec![id("ghost"), id("x")]);
    }
}
