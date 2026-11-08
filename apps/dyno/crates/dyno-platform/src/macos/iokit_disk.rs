//! IOKit's block-storage driver statistics, read in process: no `ioreg` child is started, so the monitor's own cost stays a few
//! system calls per reading (Dyno ADR for the macOS disk busy time; ER-20). The values are the same `Statistics` dictionary
//! `ioreg -c IOBlockStorageDriver` prints; the pure parsing and the busy computation live in [`crate::parse::ioreg_disk`].
#![allow(unsafe_code)]

use crate::parse::ioreg_disk::DiskCounters;
use std::ffi::c_void;

type CfRef = *const c_void;
type IoObject = u32;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFAllocatorDefault: CfRef;
    fn CFStringCreateWithCString(alloc: CfRef, c_str: *const std::ffi::c_char, encoding: u32) -> CfRef;
    fn CFDictionaryGetValue(dict: CfRef, key: CfRef) -> CfRef;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFNumberGetValue(number: CfRef, kind: isize, out: *mut i64) -> u8;
    fn CFGetTypeID(cf: CfRef) -> usize;
    fn CFRelease(cf: CfRef);
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const std::ffi::c_char) -> CfRef;
    fn IOServiceGetMatchingServices(main_port: u32, matching: CfRef, existing: *mut IoObject) -> i32;
    fn IOIteratorNext(iterator: IoObject) -> IoObject;
    fn IOObjectRelease(object: IoObject) -> i32;
    fn IORegistryEntryGetRegistryEntryID(entry: IoObject, id: *mut u64) -> i32;
    fn IORegistryEntryCreateCFProperty(entry: IoObject, key: CfRef, allocator: CfRef, options: u32) -> CfRef;
}

/// kCFStringEncodingUTF8.
const UTF8: u32 = 0x0800_0100;
/// kCFNumberSInt64Type.
const SINT64: isize = 4;
/// kIOMainPortDefault (a null port asks for the default).
const MAIN_PORT_DEFAULT: u32 = 0;

/// An owned CoreFoundation object, released when dropped (every path, including an early return).
struct Cf(CfRef);

impl Drop for Cf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer came from a Create/Copy call and is released exactly once, here.
            unsafe { CFRelease(self.0) };
        }
    }
}

/// An owned IOKit object (a service or an iterator), released when dropped.
struct Io(IoObject);

impl Drop for Io {
    fn drop(&mut self) {
        if self.0 != 0 {
            // SAFETY: the handle came from IOKit and is released exactly once, here.
            unsafe { IOObjectRelease(self.0) };
        }
    }
}

fn cf_string(text: &std::ffi::CStr) -> Result<Cf, String> {
    // SAFETY: `text` is NUL-terminated; the default allocator is a valid constant.
    let s = unsafe { CFStringCreateWithCString(kCFAllocatorDefault, text.as_ptr(), UTF8) };
    if s.is_null() {
        Err("CoreFoundation could not make a string".into())
    } else {
        Ok(Cf(s))
    }
}

/// One counter of a Statistics dictionary; `None` when absent or not a number.
fn counter(stats: CfRef, key: &std::ffi::CStr) -> Option<u64> {
    let k = cf_string(key).ok()?;
    // SAFETY: `stats` is a live CFDictionary and `k.0` a live CFString; the returned value is borrowed (not owned) and only read
    // while `stats` is alive, as a CFNumber after its type is checked.
    unsafe {
        let v = CFDictionaryGetValue(stats, k.0);
        if v.is_null() || CFGetTypeID(v) != CFNumberGetTypeID() {
            return None;
        }
        let mut out = 0i64;
        (CFNumberGetValue(v, SINT64, &mut out) != 0)
            .then(|| u64::try_from(out).ok())
            .flatten()
    }
}

/// Reads every block-storage driver's cumulative counters. `Err` when IOKit itself fails (the caller may fall back to `ioreg`);
/// `Ok(vec![])` when it works and the Mac lists no driver.
pub fn read_disk_counters() -> Result<Vec<DiskCounters>, String> {
    // SAFETY: the name is NUL-terminated. The returned dictionary is consumed by IOServiceGetMatchingServices (even on failure).
    let matching = unsafe { IOServiceMatching(c"IOBlockStorageDriver".as_ptr()) };
    if matching.is_null() {
        return Err("IOServiceMatching returned nothing".into());
    }
    let mut iter: IoObject = 0;
    // SAFETY: `matching` is an owned dictionary handed over to the call; `iter` is a valid out pointer.
    let kr = unsafe { IOServiceGetMatchingServices(MAIN_PORT_DEFAULT, matching, &mut iter) };
    if kr != 0 {
        return Err(format!("IOServiceGetMatchingServices failed (kern_return {kr})"));
    }
    let iter = Io(iter);
    let key = cf_string(c"Statistics")?;
    let mut out = Vec::new();
    loop {
        // SAFETY: `iter.0` is a live iterator.
        let entry = Io(unsafe { IOIteratorNext(iter.0) });
        if entry.0 == 0 {
            break;
        }
        let mut id = 0u64;
        // SAFETY: `entry.0` is a live registry entry; `id` is a valid out pointer.
        if unsafe { IORegistryEntryGetRegistryEntryID(entry.0, &mut id) } != 0 {
            continue;
        }
        // SAFETY: a live entry and key; the returned property is owned and released by `Cf`.
        let stats = Cf(unsafe { IORegistryEntryCreateCFProperty(entry.0, key.0, kCFAllocatorDefault, 0) });
        // SAFETY: only compares type ids of a non-null object.
        if stats.0.is_null() || unsafe { CFGetTypeID(stats.0) != CFDictionaryGetTypeID() } {
            continue;
        }
        let (Some(read_ns), Some(write_ns), Some(read_bytes), Some(write_bytes)) = (
            counter(stats.0, c"Total Time (Read)"),
            counter(stats.0, c"Total Time (Write)"),
            counter(stats.0, c"Bytes (Read)"),
            counter(stats.0, c"Bytes (Write)"),
        ) else {
            continue;
        };
        out.push(DiskCounters {
            id,
            read_ns,
            write_ns,
            read_bytes,
            write_bytes,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::ioreg_disk::parse_disk_counters;
    use std::time::{Duration, Instant};

    /// CPU time (user + system) of this process and of its waited-for children, in microseconds.
    fn cpu_micros() -> u64 {
        let us = |t: libc::timeval| t.tv_sec as u64 * 1_000_000 + t.tv_usec as u64;
        let mut total = 0;
        for who in [libc::RUSAGE_SELF, libc::RUSAGE_CHILDREN] {
            // SAFETY: `ru` is a valid out pointer for a plain-data struct.
            let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
            // SAFETY: as above.
            assert_eq!(unsafe { libc::getrusage(who, &mut ru) }, 0);
            total += us(ru.ru_utime) + us(ru.ru_stime);
        }
        total
    }

    #[test]
    fn iokit_lists_the_same_drivers_as_ioreg_with_sane_counters() {
        let io = read_disk_counters().expect("IOKit reads the statistics");
        assert!(!io.is_empty(), "a Mac has a block-storage driver");
        let mut cmd = crate::process::command("/usr/sbin/ioreg");
        cmd.args(["-r", "-d", "1", "-w0", "-c", "IOBlockStorageDriver"]);
        let text = crate::process::output_text(cmd, Duration::from_secs(5)).expect("ioreg runs");
        let reference = parse_disk_counters(&text);
        for r in &reference {
            let i = io
                .iter()
                .find(|i| i.id == r.id)
                .expect("every ioreg driver is read through IOKit too");
            // ioreg ran after the IOKit reading: the cumulative counters never go backwards.
            assert!(
                r.read_ns >= i.read_ns && r.write_ns >= i.write_ns && r.read_bytes >= i.read_bytes && r.write_bytes >= i.write_bytes,
                "{i:?} then {r:?}"
            );
        }
        let again = read_disk_counters().expect("a second reading works (nothing leaked or left open)");
        assert_eq!(again.len(), io.len());
    }

    /// The CPU cost of the reader at its 2 s cadence over 60 s: `cargo nextest run -p dyno-platform --run-ignored only
    /// measure_disk_reader_cpu --no-capture`. Prints both paths so the numbers can be recorded (docs/performance.md).
    #[test]
    #[ignore = "a 2 minute measurement, not a gate"]
    fn measure_disk_reader_cpu() {
        let run = |name: &str, read: &dyn Fn()| {
            let before = cpu_micros();
            let start = Instant::now();
            let mut n = 0;
            while start.elapsed() < Duration::from_secs(60) {
                read();
                n += 1;
                // conformance: hang-guard — 60 s observation window: the measurement is the reader's cost at its real 2 s cadence (an ignored benchmark, not a gate)
                std::thread::sleep(Duration::from_secs(2));
            }
            let used = cpu_micros() - before;
            println!("{name}: {n} readings in 60 s, {used} us CPU ({} us per reading)", used / n.max(1));
        };
        run("ioreg", &|| {
            let mut cmd = crate::process::command("/usr/sbin/ioreg");
            cmd.args(["-r", "-d", "1", "-w0", "-c", "IOBlockStorageDriver"]);
            let t = crate::process::output_text(cmd, Duration::from_secs(5)).expect("ioreg");
            assert!(!parse_disk_counters(&t).is_empty());
        });
        run("iokit", &|| {
            assert!(!read_disk_counters().expect("iokit").is_empty());
        });
    }
}
