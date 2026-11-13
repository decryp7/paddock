//! Linux counters from `/proc` (text parsers are pure; the reader takes the `/proc` folder so tests use a temporary
//! tree). Memory, virtual memory, network interfaces, disks, load, context switches and pressure stall information.

use super::{catalog_of, read_paths, Entry};
use dyno_core::counters::{
    CounterCatalog,
    CounterKind::{Cumulative, Gauge},
    CounterPath, CounterUnit as U, RawCounter,
};
use dyno_core::ports::PlatformError;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Most bytes read from one file (`/proc/diskstats` and `/proc/net/dev` stay far below).
const MAX_FILE: u64 = 1024 * 1024;
/// Most interfaces and disks listed.
const MAX_DEVICES: usize = 64;

/// `/proc/meminfo`: every `Key: value kB` line as bytes.
pub fn parse_meminfo(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|l| {
            let (key, rest) = l.split_once(':')?;
            let mut it = rest.split_whitespace();
            let n: f64 = it.next()?.parse().ok()?;
            (it.next()? == "kB").then(|| Entry::new("Memory", None, key.trim(), U::Bytes, Gauge, n * 1024.0))
        })
        .collect()
}

/// `/proc/vmstat`: the paging counters worth charting, under friendly names.
pub fn parse_vmstat(text: &str) -> Vec<Entry> {
    const NAMES: [(&str, &str); 7] = [
        ("pgfault", "Page Faults"),
        ("pgmajfault", "Major Page Faults"),
        ("pgpgin", "Pages Read From Disk"),
        ("pgpgout", "Pages Written To Disk"),
        ("pswpin", "Pages Swapped In"),
        ("pswpout", "Pages Swapped Out"),
        ("oom_kill", "Out Of Memory Kills"),
    ];
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let (key, v) = (it.next()?, it.next()?.parse::<f64>().ok()?);
            let name = NAMES.iter().find(|(k, _)| *k == key)?.1;
            Some(Entry::new("Virtual Memory", None, name, U::Count, Cumulative, v))
        })
        .collect()
}

/// `/proc/net/dev`: bytes, packets and errors per interface.
pub fn parse_net_dev(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    for l in text.lines().skip(2).take(MAX_DEVICES) {
        let Some((name, rest)) = l.split_once(':') else { continue };
        let f: Vec<f64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        if f.len() < 11 {
            continue;
        }
        let name = name.trim();
        let rows = [
            ("Bytes Received", U::Bytes, f[0]),
            ("Packets Received", U::Count, f[1]),
            ("Receive Errors", U::Count, f[2]),
            ("Bytes Sent", U::Bytes, f[8]),
            ("Packets Sent", U::Count, f[9]),
            ("Send Errors", U::Count, f[10]),
        ];
        out.extend(
            rows.into_iter()
                .map(|(c, u, v)| Entry::new("Network Interface", Some(name), c, u, Cumulative, v)),
        );
    }
    out
}

/// `/proc/diskstats`: operations and bytes per disk (partitions and virtual devices are included; loop and ram
/// devices are not).
pub fn parse_diskstats(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut disks = 0;
    for l in text.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() < 14 || f[2].starts_with("loop") || f[2].starts_with("ram") {
            continue;
        }
        let n = |i: usize| f[i].parse::<f64>().ok();
        let (Some(reads), Some(sectors_read), Some(writes), Some(sectors_written), Some(in_flight)) = (n(3), n(5), n(7), n(9), n(11))
        else {
            continue;
        };
        disks += 1;
        if disks > MAX_DEVICES {
            break;
        }
        let d = Some(f[2]);
        out.extend([
            Entry::new("Physical Disk", d, "Reads", U::Count, Cumulative, reads),
            Entry::new("Physical Disk", d, "Writes", U::Count, Cumulative, writes),
            Entry::new("Physical Disk", d, "Bytes Read", U::Bytes, Cumulative, sectors_read * 512.0),
            Entry::new("Physical Disk", d, "Bytes Written", U::Bytes, Cumulative, sectors_written * 512.0),
            Entry::new("Physical Disk", d, "Operations In Progress", U::Count, Gauge, in_flight),
        ]);
    }
    out
}

/// `/proc/loadavg`: the three load averages and the runnable and total task counts.
pub fn parse_loadavg(text: &str) -> Vec<Entry> {
    let f: Vec<&str> = text.split_whitespace().collect();
    if f.len() < 4 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (name, i) in [
        ("Load Average (1 minute)", 0),
        ("Load Average (5 minutes)", 1),
        ("Load Average (15 minutes)", 2),
    ] {
        if let Ok(v) = f[i].parse() {
            out.push(Entry::new("System", None, name, U::Number, Gauge, v));
        }
    }
    if let Some((running, total)) = f[3].split_once('/') {
        if let (Ok(r), Ok(t)) = (running.parse(), total.parse()) {
            out.push(Entry::new("System", None, "Runnable Tasks", U::Count, Gauge, r));
            out.push(Entry::new("System", None, "Tasks", U::Count, Gauge, t));
        }
    }
    out
}

/// `/proc/stat`: context switches, tasks created and blocked.
pub fn parse_stat(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let (key, v) = (it.next()?, it.next()?.parse::<f64>().ok()?);
            match key {
                "ctxt" => Some(Entry::new("System", None, "Context Switches", U::Count, Cumulative, v)),
                "processes" => Some(Entry::new("System", None, "Tasks Created", U::Count, Cumulative, v)),
                "procs_blocked" => Some(Entry::new("System", None, "Blocked Tasks", U::Count, Gauge, v)),
                _ => None,
            }
        })
        .collect()
}

/// `/proc/pressure/<resource>`: the share of the last 10 seconds tasks were stalled (`some`) or all stalled (`full`).
pub fn parse_pressure(resource: &str, text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let kind = it.next()?;
            let avg10: f64 = it.find_map(|w| w.strip_prefix("avg10=")?.parse().ok())?;
            let name = match kind {
                "some" => "Some Tasks Stalled (10 s average)",
                "full" => "All Tasks Stalled (10 s average)",
                _ => return None,
            };
            Some(Entry::new("Pressure", Some(resource), name, U::Percent, Gauge, avg10))
        })
        .collect()
}

/// The counters of a Linux machine, read from a `/proc` folder.
pub struct ProcCounters {
    proc: PathBuf,
}

impl ProcCounters {
    /// This machine's `/proc`.
    pub fn host() -> ProcCounters {
        ProcCounters::at("/proc")
    }

    /// A `/proc` folder chosen by the caller (tests).
    pub fn at(proc: impl Into<PathBuf>) -> ProcCounters {
        ProcCounters { proc: proc.into() }
    }

    fn text(&self, rel: &str) -> Option<String> {
        read_bounded(&self.proc.join(rel))
    }

    fn entries(&self, objects: Option<&[String]>) -> Vec<Entry> {
        let wanted = |o: &str| objects.is_none_or(|l| l.iter().any(|x| x == o));
        let mut out = Vec::new();
        let mut add = |object: &str, f: &dyn Fn() -> Vec<Entry>| {
            if wanted(object) {
                out.extend(f());
            }
        };
        add("Memory", &|| self.text("meminfo").map(|t| parse_meminfo(&t)).unwrap_or_default());
        add("Virtual Memory", &|| {
            self.text("vmstat").map(|t| parse_vmstat(&t)).unwrap_or_default()
        });
        add("Network Interface", &|| {
            self.text("net/dev").map(|t| parse_net_dev(&t)).unwrap_or_default()
        });
        add("Physical Disk", &|| {
            self.text("diskstats").map(|t| parse_diskstats(&t)).unwrap_or_default()
        });
        add("System", &|| {
            let mut v = self.text("loadavg").map(|t| parse_loadavg(&t)).unwrap_or_default();
            v.extend(self.text("stat").map(|t| parse_stat(&t)).unwrap_or_default());
            v
        });
        add("Pressure", &|| {
            ["cpu", "memory", "io"]
                .iter()
                .flat_map(|r| {
                    self.text(&format!("pressure/{r}"))
                        .map(|t| parse_pressure(r, &t))
                        .unwrap_or_default()
                })
                .collect()
        });
        out
    }

    /// Everything this kernel offers.
    pub fn catalog(&self) -> Result<CounterCatalog, PlatformError> {
        let entries = self.entries(None);
        if entries.is_empty() {
            return Err(PlatformError::Unsupported(format!(
                "{} has no readable statistics.",
                self.proc.display()
            )));
        }
        Ok(catalog_of(&entries))
    }

    /// One reading per path; only the files the paths need are read.
    pub fn read(&self, paths: &[CounterPath]) -> Vec<RawCounter> {
        let objects: Vec<String> = paths.iter().map(|p| p.object().to_string()).collect();
        read_paths(&self.entries(Some(&objects)), paths)
    }
}

fn read_bounded(path: &Path) -> Option<String> {
    let mut buf = Vec::new();
    std::fs::File::open(path).ok()?.take(MAX_FILE).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NET: &str = "Inter-|   Receive                                                |  Transmit\n face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n    lo: 1000 10 0 0 0 0 0 0 1000 10 0 0 0 0 0 0\n  eth0: 5000 50 1 0 0 0 0 0 2500 25 2 0 0 0 0 0\n";
    const DISK: &str = "   7       0 loop0 1 0 2 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n 259       0 nvme0n1 100 0 2000 50 200 0 4000 80 3 120 130 0 0 0 0 0 0\n   8       0 sda 5 0 10\n";

    fn find<'a>(e: &'a [Entry], inst: Option<&str>, counter: &str) -> &'a Entry {
        e.iter()
            .find(|x| x.instance.as_deref() == inst && x.counter == counter)
            .unwrap_or_else(|| panic!("{counter}"))
    }

    #[test]
    fn meminfo_gives_bytes_and_skips_counts() {
        let e = parse_meminfo("MemTotal:       16384 kB\nHugePages_Total:       0\nbroken\nMemFree:  2 kB\n");
        assert_eq!(e.len(), 2);
        assert_eq!(find(&e, None, "MemTotal").value, 16384.0 * 1024.0);
        assert_eq!(e[0].unit, U::Bytes);
    }

    #[test]
    fn vmstat_keeps_only_the_named_counters() {
        let e = parse_vmstat("nr_free_pages 5\npgfault 123\npswpout 4\noom_kill 0\nx");
        let names: Vec<_> = e.iter().map(|x| x.counter.as_str()).collect();
        assert_eq!(names, ["Page Faults", "Pages Swapped Out", "Out Of Memory Kills"]);
        assert!(e.iter().all(|x| x.kind == Cumulative));
    }

    #[test]
    fn net_dev_has_six_counters_per_interface() {
        let e = parse_net_dev(NET);
        assert_eq!(e.len(), 12);
        assert_eq!(find(&e, Some("eth0"), "Bytes Received").value, 5000.0);
        assert_eq!(find(&e, Some("eth0"), "Send Errors").value, 2.0);
        assert!(parse_net_dev("a\nb\nshort: 1 2 3\nnocolon\n").is_empty());
    }

    #[test]
    fn diskstats_skips_loop_devices_and_short_lines_and_converts_sectors() {
        let e = parse_diskstats(DISK);
        assert!(e.iter().all(|x| x.instance.as_deref() == Some("nvme0n1")));
        assert_eq!(find(&e, Some("nvme0n1"), "Bytes Read").value, 2000.0 * 512.0);
        assert_eq!(find(&e, Some("nvme0n1"), "Bytes Written").value, 4000.0 * 512.0);
        assert_eq!(find(&e, Some("nvme0n1"), "Operations In Progress").kind, Gauge);
        assert!(parse_diskstats("8 0 sda x y z").is_empty());
    }

    #[test]
    fn disks_and_interfaces_are_capped() {
        let many: String = (0..200).map(|i| format!("8 {i} sd{i} 1 0 2 0 1 0 2 0 0 0 0 0 0 0\n")).collect();
        assert_eq!(parse_diskstats(&many).len(), MAX_DEVICES * 5);
    }

    #[test]
    fn loadavg_and_stat_and_pressure() {
        let e = parse_loadavg("0.52 0.40 0.30 2/310 4567\n");
        assert_eq!(find(&e, None, "Load Average (5 minutes)").value, 0.40);
        assert_eq!(find(&e, None, "Runnable Tasks").value, 2.0);
        assert_eq!(find(&e, None, "Tasks").value, 310.0);
        assert!(parse_loadavg("1").is_empty());
        assert_eq!(parse_loadavg("1 2 3 x").len(), 3);

        let s = parse_stat("cpu 1 2 3\nctxt 900\nprocesses 77\nprocs_blocked 1\nintr 5\n");
        assert_eq!(find(&s, None, "Context Switches").kind, Cumulative);
        assert_eq!(find(&s, None, "Blocked Tasks").value, 1.0);

        let p = parse_pressure(
            "memory",
            "some avg10=1.50 avg60=0.80 avg300=0.10 total=123\nfull avg10=0.25 avg60=0 avg300=0 total=9\nweird avg10=1\n",
        );
        assert_eq!(p.len(), 2);
        assert_eq!(find(&p, Some("memory"), "Some Tasks Stalled (10 s average)").value, 1.5);
        assert_eq!(p[0].unit, U::Percent);
    }

    fn tree() -> paddock_testkit::PrivateTemp {
        let d = paddock_testkit::private_tempdir().unwrap();
        let w = |rel: &str, text: &str| {
            let p = d.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        w("meminfo", "MemTotal: 100 kB\nMemFree: 40 kB\n");
        w("net/dev", NET);
        w("diskstats", DISK);
        w("loadavg", "0.1 0.2 0.3 1/2 3\n");
        w("pressure/cpu", "some avg10=0.00 avg60=0 avg300=0 total=0\n");
        d
    }

    #[test]
    fn the_reader_lists_and_reads_a_proc_tree_and_reads_only_what_the_paths_need() {
        let d = tree();
        let c = ProcCounters::at(d.path());
        let cat = c.catalog().unwrap();
        let names: Vec<_> = cat.objects.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["Memory", "Network Interface", "Physical Disk", "Pressure", "System"]);
        let paths: Vec<CounterPath> = ["\\Memory\\MemFree", "\\Network Interface(eth0)\\Bytes Sent", "\\Memory\\Gone"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        let r = c.read(&paths);
        assert_eq!(r[0].value.as_ref().unwrap().value, 40.0 * 1024.0);
        assert_eq!(r[1].value.as_ref().unwrap().value, 2500.0);
        assert!(r[2].value.is_err());
        std::fs::remove_file(d.path().join("net/dev")).unwrap();
        assert!(
            c.read(&paths)[1].value.is_err(),
            "a vanished source is an error for its counters only"
        );
        assert!(c.read(&paths)[0].value.is_ok());
    }

    #[test]
    fn an_empty_proc_says_unsupported() {
        let d = paddock_testkit::private_tempdir().unwrap();
        assert!(matches!(ProcCounters::at(d.path()).catalog(), Err(PlatformError::Unsupported(_))));
    }

    #[test]
    fn big_files_are_read_only_up_to_the_limit() {
        let d = paddock_testkit::private_tempdir().unwrap();
        let p = d.path().join("big");
        std::fs::write(&p, vec![b'a'; MAX_FILE as usize + 100]).unwrap();
        assert_eq!(read_bounded(&p).unwrap().len(), MAX_FILE as usize);
        assert!(read_bounded(&d.path().join("missing")).is_none());
    }
}
