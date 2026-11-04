//! Windows-only sensor readings: performance counters (PDH) for disk queue and busy time, GPU engines and memory and
//! the effective CPU frequency; commit charge (GetPerformanceInfo); the GPU's name and memory size (DXGI); and the
//! reasons temperatures and fans are not available.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use crate::parse::gpu::busiest_engine;
use crate::sensors::{clamp_pct, SensorProbe, NO_TEMPERATURES};
use dyno_core::metrics::{Commit, GpuSample, Reading, SystemSample};
use windows::core::HSTRING;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhGetFormattedCounterValue, PDH_FMT,
    PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_FMT_LARGE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};
use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};

/// Shown when Windows hides temperatures from normal users.
pub const WINDOWS_NO_TEMPERATURES: &str = "Windows reports temperatures only to vendor drivers or administrators";
/// Shown for fans.
pub const WINDOWS_NO_FANS: &str = "Windows reports fan speeds only to vendor drivers";
/// Most counter instances read from one wildcard counter.
const MAX_INSTANCES: u32 = 20_000;

/// The status PDH gives a value that is fine.
const PDH_CSTATUS_VALID_DATA: u32 = 0;
const PDH_CSTATUS_NEW_DATA: u32 = 1;

/// A PDH query closed on drop.
struct Query(PDH_HQUERY);

impl Drop for Query {
    fn drop(&mut self) {
        // SAFETY: the query was opened by PdhOpenQueryW and is closed once.
        unsafe {
            paddock_proc::cleanup::logged("closing the performance query", super::counters::result_of(PdhCloseQuery(self.0)));
        }
    }
}

// SAFETY: PDH query handles may be used from any thread (one at a time, which `&mut self` guarantees).
unsafe impl Send for Query {}

struct Counters {
    query: Query,
    disk_queue: Option<PDH_HCOUNTER>,
    disk_idle: Option<PDH_HCOUNTER>,
    cpu_frequency: Option<PDH_HCOUNTER>,
    cpu_performance: Option<PDH_HCOUNTER>,
    gpu_engines: Option<PDH_HCOUNTER>,
    gpu_memory: Option<PDH_HCOUNTER>,
}

// SAFETY: counter handles belong to the query and are only used with it, behind `&mut self`.
unsafe impl Send for Counters {}

fn add(query: &Query, path: &str) -> Option<PDH_HCOUNTER> {
    let mut c = PDH_HCOUNTER::default();
    // SAFETY: the query is open; the path is a valid wide string for the call's duration; `c` is an out-pointer.
    let status = unsafe { PdhAddEnglishCounterW(query.0, &HSTRING::from(path), 0, &mut c) };
    (status == 0).then_some(c)
}

fn value(c: Option<PDH_HCOUNTER>, format: PDH_FMT) -> Option<PDH_FMT_COUNTERVALUE> {
    let mut v = PDH_FMT_COUNTERVALUE::default();
    // SAFETY: `c` belongs to an open, collected query; `v` is a valid out-pointer.
    let status = unsafe { PdhGetFormattedCounterValue(c?, format, None, &mut v) };
    (status == 0 && matches!(v.CStatus, PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA)).then_some(v)
}

fn double(c: Option<PDH_HCOUNTER>) -> Option<f64> {
    // SAFETY: PDH_FMT_DOUBLE was requested, so the union holds a double.
    value(c, PDH_FMT_DOUBLE)
        .map(|v| unsafe { v.Anonymous.doubleValue })
        .filter(|d| d.is_finite())
}

/// Every instance of a wildcard counter as (name, value).
fn array(c: Option<PDH_HCOUNTER>, format: PDH_FMT) -> Option<Vec<(String, f64)>> {
    let c = c?;
    let (mut size, mut count) = (0u32, 0u32);
    // SAFETY: a size query: no buffer, the two out-pointers are valid.
    let status = unsafe { PdhGetFormattedCounterArrayW(c, format, &mut size, &mut count, None) };
    if status != PDH_MORE_DATA || count > MAX_INSTANCES || size == 0 {
        return (status == 0).then(Vec::new);
    }
    // Item structs followed by their names; allocate as items for alignment.
    let item = size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
    let mut buf: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> = vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); (size as usize).div_ceil(item)];
    let mut size = u32::try_from(buf.len() * item).ok()?;
    // SAFETY: `buf` holds at least `size` bytes, suitably aligned for the items.
    let status = unsafe { PdhGetFormattedCounterArrayW(c, format, &mut size, &mut count, Some(buf.as_mut_ptr())) };
    if status != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(count as usize);
    for it in buf.iter().take(count as usize) {
        if !matches!(it.FmtValue.CStatus, PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA) {
            continue;
        }
        // SAFETY: PDH filled szName with a NUL-terminated string inside `buf`.
        let name = unsafe { it.szName.to_string() }.unwrap_or_default();
        // SAFETY: the union member matches the requested format.
        let v = unsafe {
            if format == PDH_FMT_LARGE {
                it.FmtValue.Anonymous.largeValue as f64
            } else {
                it.FmtValue.Anonymous.doubleValue
            }
        };
        out.push((name, v));
    }
    Some(out)
}

impl Counters {
    fn open() -> Option<Counters> {
        let mut q = PDH_HQUERY::default();
        // SAFETY: a null data source means live data; `q` is an out-pointer.
        let status = unsafe { windows::Win32::System::Performance::PdhOpenQueryW(None, 0, &mut q) };
        if status != 0 {
            return None;
        }
        let query = Query(q);
        let c = Counters {
            disk_queue: add(&query, r"\PhysicalDisk(_Total)\Avg. Disk Queue Length"),
            disk_idle: add(&query, r"\PhysicalDisk(_Total)\% Idle Time"),
            cpu_frequency: add(&query, r"\Processor Information(_Total)\Processor Frequency"),
            cpu_performance: add(&query, r"\Processor Information(_Total)\% Processor Performance"),
            gpu_engines: add(&query, r"\GPU Engine(*)\Utilization Percentage"),
            gpu_memory: add(&query, r"\GPU Adapter Memory(*)\Dedicated Usage"),
            query,
        };
        // Rate counters need a first collection to compare against.
        // SAFETY: the query is open.
        let status = unsafe { PdhCollectQueryData(c.query.0) };
        if status != 0 {
            log::debug!(status = status; "the first performance collection failed; rate counters start one sample late");
        }
        Some(c)
    }
}

/// The main GPU (most dedicated memory, not the software renderer): name and dedicated memory in bytes.
fn main_adapter() -> Option<(String, u64)> {
    // SAFETY: plain COM factory creation; the interface is released on drop.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
    let mut best: Option<(String, u64)> = None;
    for i in 0..16u32 {
        // SAFETY: enumerating adapters by index; an error ends the list.
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else {
            break;
        };
        // SAFETY: GetDesc1 fills a plain struct.
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else { continue };
        if desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
            continue;
        }
        let end = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
        let name = String::from_utf16_lossy(&desc.Description[..end]).trim().to_string();
        let mem = desc.DedicatedVideoMemory as u64;
        if best.as_ref().is_none_or(|(_, m)| mem > *m) {
            best = Some((name, mem));
        }
    }
    best
}

/// Commit charge and limit in bytes.
pub fn commit() -> Option<Commit> {
    let mut info = PERFORMANCE_INFORMATION {
        cb: size_of::<PERFORMANCE_INFORMATION>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a valid struct of the size passed.
    unsafe { GetPerformanceInfo(&mut info, info.cb) }.ok()?;
    let page = info.PageSize as u64;
    Some(Commit {
        used: (info.CommitTotal as u64).saturating_mul(page),
        limit: (info.CommitLimit as u64).saturating_mul(page),
    })
}

/// The Windows [`SensorProbe`].
pub struct WinProbe {
    counters: Option<Counters>,
    adapter: Option<(String, u64)>,
    kinds: Option<Vec<dyno_core::metrics::CoreKind>>,
}

impl WinProbe {
    /// Opens the performance counters and looks up the GPU.
    pub fn new() -> Self {
        WinProbe {
            counters: Counters::open(),
            adapter: main_adapter(),
            kinds: None,
        }
    }
}

impl Default for WinProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl SensorProbe for WinProbe {
    fn enrich(&mut self, s: &mut SystemSample) {
        if s.cpu.core_kinds.is_empty() {
            let n = s.cpu.per_core.len();
            let kinds = self.kinds.get_or_insert_with(|| super::topology::core_kinds(n));
            s.cpu.core_kinds = kinds.clone();
        }
        if let Some(c) = commit() {
            s.memory.commit = Reading::Available(c);
            s.memory.pressure = Reading::Available(dyno_core::metrics::derive_pressure(&s.memory));
        }
        if matches!(&s.temperatures, Reading::Unavailable(r) if r == NO_TEMPERATURES) {
            s.temperatures = Reading::unavailable(WINDOWS_NO_TEMPERATURES);
        }
        s.fans = Reading::unavailable(WINDOWS_NO_FANS);
        let Some(c) = &self.counters else {
            s.gpu = Reading::unavailable("Windows performance counters are not available");
            return;
        };
        // SAFETY: the query is open.
        let collected = unsafe { PdhCollectQueryData(c.query.0) } == 0;
        if !collected {
            return;
        }
        if let Some(q) = double(c.disk_queue) {
            s.disk_io.queue = Reading::Available(q.max(0.0) as f32);
        }
        if let Some(idle) = double(c.disk_idle) {
            s.disk_io.busy = Reading::Available(clamp_pct((100.0 - idle) as f32));
        }
        if let (Some(base), Some(perf)) = (double(c.cpu_frequency), double(c.cpu_performance)) {
            if base > 0.0 {
                s.cpu.base_frequency_mhz = base as u32;
                s.cpu.frequency_mhz = (base * perf / 100.0).max(0.0) as u32;
            }
        }
        let engines = array(c.gpu_engines, PDH_FMT_DOUBLE);
        let memory = array(c.gpu_memory, PDH_FMT_LARGE);
        s.gpu = match (engines, &self.adapter) {
            (Some(e), adapter) => {
                let util = busiest_engine(e.iter().map(|(n, v)| (n.as_str(), *v)))
                    .map(|(u, _)| u)
                    .unwrap_or(0.0);
                let used = memory.unwrap_or_default().iter().map(|(_, v)| v.max(0.0) as u64).max().unwrap_or(0);
                Reading::Available(GpuSample {
                    name: adapter.as_ref().map(|a| a.0.clone()).unwrap_or_default(),
                    utilization: util,
                    memory_used: used,
                    memory_total: adapter.as_ref().map(|a| a.1).unwrap_or(0),
                })
            }
            (None, _) => Reading::unavailable("Windows reports no GPU counters on this computer"),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::SysinfoSensors;
    use dyno_core::ports::SystemSensors;
    use dyno_core::time::Timestamp;

    #[test]
    fn reads_counters_on_this_machine() {
        let mut s = SysinfoSensors::new(Box::new(WinProbe::new()));
        let _ = s.sample(Timestamp(0));
        std::thread::sleep(std::time::Duration::from_millis(300));
        let x = s.sample(Timestamp(1));
        let c = x.memory.commit.value().expect("commit");
        assert!(c.limit >= c.used && c.used > 0);
        assert!(
            x.disk_io.busy.value().is_some_and(|b| (0.0..=100.0).contains(b)),
            "{:?}",
            x.disk_io.busy
        );
        assert!(x.disk_io.queue.value().is_some(), "{:?}", x.disk_io.queue);
        assert!(x.cpu.base_frequency_mhz > 0, "{:?}", x.cpu);
        assert_eq!(x.fans, Reading::unavailable(WINDOWS_NO_FANS));
        if let Reading::Available(g) = &x.gpu {
            assert!((0.0..=100.0).contains(&g.utilization));
        }
        assert!(commit().is_some());
    }
}
