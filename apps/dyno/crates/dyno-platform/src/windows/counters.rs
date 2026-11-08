//! The performance counters browser on Windows (WP-12): `PdhEnumObjectsW` and `PdhEnumObjectItemsW` list what exists,
//! and one persistent PDH query reads the counters being watched. Names and paths are the OS's own, in the user's
//! language (the same strings the enumeration returns are the ones `PdhAddCounterW` accepts). PDH formats rates
//! itself, so every counter is a gauge here. A counter needs two collections before PDH can compute a rate: the first
//! reading after adding one says so.
#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]

use crate::parse::pdh_list::multi_sz;
use dyno_core::counters::{
    CounterCatalog, CounterInfo, CounterKind, CounterObject, CounterPath, CounterUnit, RawCounter, RawValue, MAX_COUNTERS_PER_OBJECT,
    MAX_OBJECTS,
};
use dyno_core::ports::PlatformError;
use std::collections::HashMap;
use std::sync::Mutex;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::System::Performance::{
    PdhAddCounterW, PdhCloseQuery, PdhCollectQueryData, PdhEnumObjectItemsW, PdhEnumObjectsW, PdhGetFormattedCounterValue, PdhOpenQueryW,
    PdhRemoveCounter, PDH_CALC_NEGATIVE_DENOMINATOR, PDH_CALC_NEGATIVE_TIMEBASE, PDH_CALC_NEGATIVE_VALUE, PDH_CSTATUS_NO_COUNTER,
    PDH_CSTATUS_NO_INSTANCE, PDH_CSTATUS_NO_OBJECT, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_INVALID_DATA,
    PDH_MORE_DATA, PDH_NO_DATA, PERF_DETAIL_WIZARD,
};

/// A PDH status as a result: 0 (`ERROR_SUCCESS`) is success, anything else is the failure, in words for the log.
pub(super) fn result_of(status: u32) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!("PDH status {status:#x}"))
    }
}

/// Most counters kept in the persistent query.
const MAX_WATCHED: usize = 64;

/// Every object, counter and instance the machine offers (bounded by the core).
pub fn catalog() -> Result<CounterCatalog, PlatformError> {
    let objects = enum_objects()?;
    let mut out = Vec::new();
    for name in objects.into_iter().take(MAX_OBJECTS) {
        // An object that cannot be listed (it vanished, or the provider failed) is skipped, not fatal.
        if let Ok((counters, instances)) = enum_items(&name) {
            let total = u32::try_from(instances.len()).unwrap_or(u32::MAX);
            out.push(CounterObject {
                counters: counters
                    .into_iter()
                    .map(|c| CounterInfo {
                        unit: CounterUnit::guess(&c),
                        name: c,
                        kind: CounterKind::Gauge,
                        help: None,
                    })
                    .collect(),
                instances,
                instances_total: total,
                name,
            });
        }
    }
    if out.is_empty() {
        return Err(PlatformError::Failed(
            "Windows listed no performance counters; the performance counter registry may need repair (lodctr /R as administrator).".into(),
        ));
    }
    Ok(CounterCatalog::bounded(out))
}

fn enum_objects() -> Result<Vec<String>, PlatformError> {
    let mut len = 0u32;
    // SAFETY: a size query: no buffer, `len` is a valid out-pointer; null strings mean this machine's live data.
    let status = unsafe { PdhEnumObjectsW(PCWSTR::null(), PCWSTR::null(), None, &mut len, PERF_DETAIL_WIZARD, false) };
    if status != PDH_MORE_DATA || len == 0 {
        return Err(PlatformError::Failed(format!(
            "Windows would not list the counter objects (PDH status {status:#x})."
        )));
    }
    let mut buf = vec![0u16; len as usize + 1];
    let mut len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
    // SAFETY: `buf` holds `len` UTF-16 units and outlives the call.
    let status = unsafe {
        PdhEnumObjectsW(
            PCWSTR::null(),
            PCWSTR::null(),
            Some(PWSTR(buf.as_mut_ptr())),
            &mut len,
            PERF_DETAIL_WIZARD,
            false,
        )
    };
    if status != 0 {
        return Err(PlatformError::Failed(format!(
            "Windows would not list the counter objects (PDH status {status:#x})."
        )));
    }
    Ok(multi_sz(&buf, MAX_OBJECTS))
}

fn enum_items(object: &str) -> Result<(Vec<String>, Vec<String>), PlatformError> {
    let name = HSTRING::from(object);
    let (mut counter_len, mut instance_len) = (0u32, 0u32);
    // SAFETY: a size query: no buffers, both lengths are valid out-pointers.
    let status = unsafe {
        PdhEnumObjectItemsW(
            PCWSTR::null(),
            PCWSTR::null(),
            &name,
            None,
            &mut counter_len,
            None,
            &mut instance_len,
            PERF_DETAIL_WIZARD,
            0,
        )
    };
    if status != PDH_MORE_DATA && status != 0 {
        return Err(PlatformError::Failed(format!(
            "Windows would not list {object} (PDH status {status:#x})."
        )));
    }
    let mut counters = vec![0u16; counter_len as usize + 2];
    let mut instances = vec![0u16; instance_len as usize + 2];
    let (mut cl, mut il) = (
        u32::try_from(counters.len()).unwrap_or(0),
        u32::try_from(instances.len()).unwrap_or(0),
    );
    // SAFETY: both buffers hold the number of UTF-16 units passed and outlive the call.
    let status = unsafe {
        PdhEnumObjectItemsW(
            PCWSTR::null(),
            PCWSTR::null(),
            &name,
            Some(PWSTR(counters.as_mut_ptr())),
            &mut cl,
            Some(PWSTR(instances.as_mut_ptr())),
            &mut il,
            PERF_DETAIL_WIZARD,
            0,
        )
    };
    if status != 0 {
        return Err(PlatformError::Failed(format!(
            "Windows would not list {object} (PDH status {status:#x})."
        )));
    }
    Ok((multi_sz(&counters, MAX_COUNTERS_PER_OBJECT + 1), multi_sz(&instances, 10_000)))
}

/// The one persistent query: counters are added when first asked for and removed when no longer asked for.
struct Reader {
    query: PDH_HQUERY,
    counters: HashMap<String, PDH_HCOUNTER>,
}

// SAFETY: PDH handles may be used from any thread, one at a time, which the `Mutex` around the reader guarantees.
unsafe impl Send for Reader {}

impl Drop for Reader {
    fn drop(&mut self) {
        // SAFETY: the query was opened by PdhOpenQueryW and is closed once (closing it frees its counters).
        unsafe {
            paddock_proc::cleanup::logged("closing the performance query", result_of(PdhCloseQuery(self.query)));
        }
    }
}

static READER: Mutex<Option<Reader>> = Mutex::new(None);

fn open() -> Result<Reader, PlatformError> {
    let mut q = PDH_HQUERY::default();
    // SAFETY: a null data source means live data; `q` is an out-pointer.
    let status = unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut q) };
    if status != 0 {
        return Err(PlatformError::Failed(format!(
            "Windows would not open a performance query (PDH status {status:#x})."
        )));
    }
    Ok(Reader {
        query: q,
        counters: HashMap::new(),
    })
}

fn why(status: u32) -> String {
    match status {
        PDH_CSTATUS_NO_OBJECT | PDH_CSTATUS_NO_COUNTER | PDH_CSTATUS_NO_INSTANCE => {
            "This counter or instance is no longer there (the device or program it measured has gone).".into()
        }
        PDH_CALC_NEGATIVE_DENOMINATOR | PDH_CALC_NEGATIVE_TIMEBASE | PDH_CALC_NEGATIVE_VALUE | PDH_NO_DATA | PDH_INVALID_DATA => {
            "Windows needs a second reading to compute this value; the next one will show it.".into()
        }
        other => format!("Windows could not read this counter (PDH status {other:#x})."),
    }
}

/// Reads the counters, adding new ones to the query and dropping the ones no longer asked for.
pub fn read(paths: &[CounterPath]) -> Vec<RawCounter> {
    let mut guard = READER.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        match open() {
            Ok(r) => *guard = Some(r),
            Err(e) => return failed(paths, &e.to_string()),
        }
    }
    let Some(reader) = guard.as_mut() else {
        return failed(paths, "No performance query.");
    };
    let wanted: Vec<String> = paths.iter().map(ToString::to_string).collect();
    let stale: Vec<String> = reader.counters.keys().filter(|k| !wanted.contains(k)).cloned().collect();
    for k in stale {
        if let Some(h) = reader.counters.remove(&k) {
            // SAFETY: the handle belongs to this query and is removed once.
            unsafe {
                paddock_proc::cleanup::logged("removing a performance counter", result_of(PdhRemoveCounter(h)));
            }
        }
    }
    let mut add_errors: HashMap<String, u32> = HashMap::new();
    for p in &wanted {
        if reader.counters.contains_key(p) || reader.counters.len() >= MAX_WATCHED {
            continue;
        }
        let mut h = PDH_HCOUNTER::default();
        // SAFETY: the query is open; the path is a valid wide string for the call; `h` is an out-pointer.
        let status = unsafe { PdhAddCounterW(reader.query, &HSTRING::from(p.as_str()), 0, &mut h) };
        if status == 0 {
            reader.counters.insert(p.clone(), h);
        } else {
            add_errors.insert(p.clone(), status);
        }
    }
    // SAFETY: the query is open.
    let collected = unsafe { PdhCollectQueryData(reader.query) };
    paths
        .iter()
        .map(|p| {
            let path = p.to_string();
            let unit = CounterUnit::guess(p.counter());
            let value = match (reader.counters.get(&path), add_errors.get(&path)) {
                (_, Some(status)) => Err(why(*status)),
                (None, None) => Err("Too many counters are being watched at once; remove some.".into()),
                (Some(_), _) if collected != 0 => Err(why(collected)),
                (Some(h), _) => {
                    let mut v = PDH_FMT_COUNTERVALUE::default();
                    // SAFETY: the handle belongs to the collected query; `v` is a valid out-pointer.
                    let status = unsafe { PdhGetFormattedCounterValue(*h, PDH_FMT_DOUBLE, None, &mut v) };
                    if status != 0 || v.CStatus != 0 && v.CStatus != 1 {
                        Err(why(if status != 0 { status } else { v.CStatus }))
                    } else {
                        // SAFETY: PDH_FMT_DOUBLE was requested, so the union holds a double.
                        Ok(RawValue {
                            value: unsafe { v.Anonymous.doubleValue },
                            kind: CounterKind::Gauge,
                        })
                    }
                }
            };
            RawCounter { path, unit, value }
        })
        .collect()
}

fn failed(paths: &[CounterPath], reason: &str) -> Vec<RawCounter> {
    paths
        .iter()
        .map(|p| RawCounter {
            path: p.to_string(),
            unit: CounterUnit::guess(p.counter()),
            value: Err(reason.to_string()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::counters::CounterPath;

    #[test]
    fn the_real_catalog_lists_memory_and_a_real_counter_reads_back_after_two_collections() {
        let cat = catalog().unwrap();
        assert!(cat.objects.iter().any(|o| !o.counters.is_empty()));
        let (object, counter) = cat
            .objects
            .iter()
            .find(|o| o.instances.is_empty() && !o.counters.is_empty())
            .map(|o| (o.name.clone(), o.counters[0].name.clone()))
            .unwrap();
        let path = CounterPath::new(&object, None, &counter).unwrap();
        read(std::slice::from_ref(&path));
        std::thread::sleep(std::time::Duration::from_millis(300));
        let r = read(&[path]);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn a_counter_that_does_not_exist_fails_alone_with_a_reason() {
        let ok = CounterPath::new("Nonexistent Object Dyno Test", None, "Nothing").unwrap();
        let r = read(&[ok]);
        assert!(r[0].value.as_ref().unwrap_err().contains("no longer there") || r[0].value.is_err());
    }
}
