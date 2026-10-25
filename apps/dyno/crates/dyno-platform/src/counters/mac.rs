//! macOS counters from the host's own read-only tools (`vm_stat`, `netstat -inb`, `iostat -Id`, `sysctl`). The text
//! parsers are pure and tested with real output; the reader takes a function that runs a tool, so tests inject
//! output (and failures) and only the macOS build runs the real tools, each under a timeout.

use super::{catalog_of, read_paths, Entry};
use dyno_core::counters::{
    CounterCatalog,
    CounterKind::{Cumulative, Gauge},
    CounterPath, CounterUnit as U, RawCounter,
};
use dyno_core::ports::PlatformError;
use std::time::Duration;

/// How long one tool may take.
const TOOL_TIMEOUT: Duration = Duration::from_secs(5);
/// Most interfaces and disks listed.
const MAX_DEVICES: usize = 64;

/// A tool Dyno reads from, with the objects it feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// `vm_stat`.
    VmStat,
    /// `netstat -inb`.
    Netstat,
    /// `iostat -Id`.
    Iostat,
    /// `sysctl -n vm.loadavg hw.memsize`.
    Sysctl,
}

impl Tool {
    const ALL: [Tool; 4] = [Tool::VmStat, Tool::Netstat, Tool::Iostat, Tool::Sysctl];

    fn command(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Tool::VmStat => ("/usr/bin/vm_stat", &[]),
            Tool::Netstat => ("/usr/sbin/netstat", &["-inb"]),
            Tool::Iostat => ("/usr/sbin/iostat", &["-Id"]),
            Tool::Sysctl => ("/usr/sbin/sysctl", &["-n", "vm.loadavg", "hw.memsize"]),
        }
    }

    fn objects(self) -> &'static [&'static str] {
        match self {
            Tool::VmStat => &["Memory", "Virtual Memory"],
            Tool::Netstat => &["Network Interface"],
            Tool::Iostat => &["Physical Disk"],
            Tool::Sysctl => &["System", "Memory"],
        }
    }

    fn parse(self, text: &str) -> Vec<Entry> {
        match self {
            Tool::VmStat => parse_vm_stat(text),
            Tool::Netstat => parse_netstat_ib(text),
            Tool::Iostat => parse_iostat(text),
            Tool::Sysctl => parse_sysctl(text),
        }
    }
}

/// Runs a real tool with a timeout.
pub fn run_tool(tool: Tool) -> Result<String, PlatformError> {
    let (program, args) = tool.command();
    let mut cmd = crate::process::command(program);
    cmd.args(args);
    crate::process::output_text(cmd, TOOL_TIMEOUT)
}

/// `vm_stat`: memory in bytes, and the paging counters.
pub fn parse_vm_stat(text: &str) -> Vec<Entry> {
    let page: f64 = text
        .lines()
        .next()
        .and_then(|l| l.split("page size of ").nth(1))
        .and_then(|r| r.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(16384.0);
    const LEVELS: [(&str, &str); 8] = [
        ("Pages free", "Free"),
        ("Pages active", "Active"),
        ("Pages inactive", "Inactive"),
        ("Pages speculative", "Speculative"),
        ("Pages wired down", "Wired"),
        ("Pages purgeable", "Purgeable"),
        ("Pages occupied by compressor", "Used By Compressor"),
        ("File-backed pages", "File Cache"),
    ];
    const EVENTS: [(&str, &str); 7] = [
        ("Translation faults", "Page Faults"),
        ("Pageins", "Pages Read From Disk"),
        ("Pageouts", "Pages Written To Disk"),
        ("Swapins", "Pages Swapped In"),
        ("Swapouts", "Pages Swapped Out"),
        ("Decompressions", "Pages Decompressed"),
        ("Compressions", "Pages Compressed"),
    ];
    let mut out = Vec::new();
    for l in text.lines().skip(1) {
        let Some((key, v)) = l.rsplit_once(':') else { continue };
        let key = key.trim().trim_matches('"');
        let Ok(v) = v.trim().trim_end_matches('.').parse::<f64>() else {
            continue;
        };
        if let Some((_, name)) = LEVELS.iter().find(|(k, _)| *k == key) {
            out.push(Entry::new("Memory", None, name, U::Bytes, Gauge, v * page));
        } else if let Some((_, name)) = EVENTS.iter().find(|(k, _)| *k == key) {
            out.push(Entry::new("Virtual Memory", None, name, U::Count, Cumulative, v));
        }
    }
    out
}

/// `netstat -inb`: the link-level row of each interface (the address rows repeat the same totals).
pub fn parse_netstat_ib(text: &str) -> Vec<Entry> {
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for l in text.lines().skip(1) {
        let t: Vec<&str> = l.split_whitespace().collect();
        // Name Mtu Network [Address] Ipkts Ierrs Ibytes Opkts Oerrs Obytes Coll: the numbers are the last seven.
        if t.len() < 10 || !t[2].starts_with("<Link#") {
            continue;
        }
        let name = t[0].trim_end_matches('*').to_string();
        if seen.contains(&name) || seen.len() >= MAX_DEVICES {
            continue;
        }
        let n: Vec<Option<f64>> = t[t.len() - 7..].iter().map(|x| x.parse().ok()).collect();
        let (Some(ip), Some(ie), Some(ib), Some(op), Some(oe), Some(ob)) = (n[0], n[1], n[2], n[3], n[4], n[5]) else {
            continue;
        };
        let d = Some(name.as_str());
        out.extend([
            Entry::new("Network Interface", d, "Bytes Received", U::Bytes, Cumulative, ib),
            Entry::new("Network Interface", d, "Packets Received", U::Count, Cumulative, ip),
            Entry::new("Network Interface", d, "Receive Errors", U::Count, Cumulative, ie),
            Entry::new("Network Interface", d, "Bytes Sent", U::Bytes, Cumulative, ob),
            Entry::new("Network Interface", d, "Packets Sent", U::Count, Cumulative, op),
            Entry::new("Network Interface", d, "Send Errors", U::Count, Cumulative, oe),
        ]);
        seen.push(name);
    }
    out
}

/// `iostat -Id`: transfers and megabytes per disk since boot.
pub fn parse_iostat(text: &str) -> Vec<Entry> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let (Some(names), Some(_header), Some(values)) = (lines.next(), lines.next(), lines.next()) else {
        return Vec::new();
    };
    let disks: Vec<&str> = names.split_whitespace().take(MAX_DEVICES).collect();
    let v: Vec<f64> = values.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    let mut out = Vec::new();
    for (i, d) in disks.iter().enumerate() {
        let (Some(xfrs), Some(mb)) = (v.get(i * 3 + 1), v.get(i * 3 + 2)) else {
            break;
        };
        out.push(Entry::new("Physical Disk", Some(d), "Transfers", U::Count, Cumulative, *xfrs));
        out.push(Entry::new(
            "Physical Disk",
            Some(d),
            "Bytes Transferred",
            U::Bytes,
            Cumulative,
            mb * 1_048_576.0,
        ));
    }
    out
}

/// `sysctl -n vm.loadavg hw.memsize`: `{ 1.0 2.0 3.0 }` then the installed memory in bytes.
pub fn parse_sysctl(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    if let Some(l) = lines.next() {
        let f: Vec<f64> = l
            .split(|c: char| c.is_whitespace() || c == '{' || c == '}')
            .filter_map(|x| x.parse().ok())
            .collect();
        if f.len() == 3 {
            for (n, v) in ["Load Average (1 minute)", "Load Average (5 minutes)", "Load Average (15 minutes)"]
                .iter()
                .zip(f)
            {
                out.push(Entry::new("System", None, n, U::Number, Gauge, v));
            }
        }
    }
    if let Some(Ok(bytes)) = lines.next().map(|l| l.trim().parse::<f64>()) {
        out.push(Entry::new("Memory", None, "Installed", U::Bytes, Gauge, bytes));
    }
    out
}

/// The counters of a Mac, through a function that runs the tools.
pub struct MacCounters<R: Fn(Tool) -> Result<String, PlatformError>> {
    run: R,
}

impl<R: Fn(Tool) -> Result<String, PlatformError>> MacCounters<R> {
    /// A reader running tools through `run`.
    pub fn new(run: R) -> Self {
        MacCounters { run }
    }

    /// Entries of the tools feeding `objects` (all tools when `None`). A tool that fails contributes nothing; the
    /// error of the last failure is returned when nothing worked at all.
    fn entries(&self, objects: Option<&[String]>) -> Result<Vec<Entry>, PlatformError> {
        let mut out = Vec::new();
        let mut last_error = None;
        let mut asked = 0;
        for tool in Tool::ALL {
            if objects.is_some_and(|l| !tool.objects().iter().any(|o| l.iter().any(|x| x == o))) {
                continue;
            }
            asked += 1;
            match (self.run)(tool) {
                Ok(text) => out.extend(tool.parse(&text)),
                Err(e) => last_error = Some(e),
            }
        }
        match last_error {
            Some(e) if out.is_empty() && asked > 0 => Err(e),
            _ => Ok(out),
        }
    }

    /// Everything the tools offer.
    pub fn catalog(&self) -> Result<CounterCatalog, PlatformError> {
        Ok(catalog_of(&self.entries(None)?))
    }

    /// One reading per path; only the tools the paths need are run.
    pub fn read(&self, paths: &[CounterPath]) -> Vec<RawCounter> {
        let objects: Vec<String> = paths.iter().map(|p| p.object().to_string()).collect();
        match self.entries(Some(&objects)) {
            Ok(entries) => read_paths(&entries, paths),
            Err(e) => paths
                .iter()
                .map(|p| RawCounter {
                    path: p.to_string(),
                    unit: U::Number,
                    value: Err(e.to_string()),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VM: &str = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free:                                   473558.\nPages active:                                1471092.\nPages wired down:                             230647.\n\"Translation faults\":                     7037337950.\nPageins:                                    76492454.\nPages occupied by compressor:                 374868.\nAnonymous pages:   5.\n";
    const NET: &str = "Name       Mtu   Network       Address            Ipkts Ierrs     Ibytes    Opkts Oerrs     Obytes  Coll\nlo0        16384 <Link#1>                      43067861     0 341821759708 43067861     0 341821759708     0\nlo0        16384 127           localhost       43067861     - 341821759708 43067861     - 341821759708     -\ngif0*      1280  <Link#2>                             0     0          0        0     0          0     0\nen0        1500  <Link#4>    56:d2:8e:1e:82:a0   10     1       2000       20     2       4000     0\nen0        1500  192.168.1     192.168.1.5       10     -       2000       20     -       4000     -\n";
    const IO: &str = "              disk0               disk4 \n    KB/t xfrs   MB     KB/t xfrs   MB \n   25.88 118115441 2985570.45    50.63 1160867 57393.98 \n";

    fn val(e: &[Entry], inst: Option<&str>, counter: &str) -> f64 {
        e.iter()
            .find(|x| x.instance.as_deref() == inst && x.counter == counter)
            .unwrap_or_else(|| panic!("{counter}"))
            .value
    }

    #[test]
    fn vm_stat_uses_the_reported_page_size() {
        let e = parse_vm_stat(VM);
        assert_eq!(val(&e, None, "Free"), 473558.0 * 16384.0);
        assert_eq!(val(&e, None, "Wired"), 230647.0 * 16384.0);
        assert_eq!(val(&e, None, "Used By Compressor"), 374868.0 * 16384.0);
        assert_eq!(val(&e, None, "Page Faults"), 7037337950.0);
        assert_eq!(e.iter().find(|x| x.counter == "Page Faults").unwrap().kind, Cumulative);
        assert!(e.iter().all(|x| x.counter != "Anonymous pages"));
        let small = parse_vm_stat("Mach Virtual Memory Statistics: (page size of 4096 bytes)\nPages free: 2.\n");
        assert_eq!(val(&small, None, "Free"), 8192.0);
        assert_eq!(val(&parse_vm_stat("no header\nPages free: 1.\n"), None, "Free"), 16384.0);
        assert!(parse_vm_stat("").is_empty());
    }

    #[test]
    fn netstat_takes_one_link_row_per_interface_with_and_without_an_address() {
        let e = parse_netstat_ib(NET);
        let ifaces: std::collections::BTreeSet<_> = e.iter().filter_map(|x| x.instance.clone()).collect();
        assert_eq!(ifaces.into_iter().collect::<Vec<_>>(), ["en0", "gif0", "lo0"]);
        assert_eq!(val(&e, Some("lo0"), "Bytes Received"), 341821759708.0);
        assert_eq!(val(&e, Some("en0"), "Bytes Sent"), 4000.0);
        assert_eq!(val(&e, Some("en0"), "Send Errors"), 2.0);
        assert!(parse_netstat_ib("Name\nlo0 1 <Link#1> a b c d e f g\n").is_empty());
    }

    #[test]
    fn iostat_pairs_each_disk_with_its_three_columns() {
        let e = parse_iostat(IO);
        assert_eq!(val(&e, Some("disk0"), "Transfers"), 118115441.0);
        assert_eq!(val(&e, Some("disk4"), "Bytes Transferred"), 57393.98 * 1_048_576.0);
        assert!(parse_iostat("disk0\n").is_empty());
        assert_eq!(parse_iostat("a b\nh\n1 2 3\n").len(), 2, "a disk with no values is skipped");
    }

    #[test]
    fn sysctl_gives_load_and_installed_memory() {
        let e = parse_sysctl("{ 35.21 34.60 40.63 }\n68719476736\n");
        assert_eq!(val(&e, None, "Load Average (5 minutes)"), 34.60);
        assert_eq!(val(&e, None, "Installed"), 68719476736.0);
        assert!(parse_sysctl("garbage\n").is_empty());
    }

    fn fake(fail: &'static [Tool]) -> impl Fn(Tool) -> Result<String, PlatformError> {
        move |t| {
            if fail.contains(&t) {
                return Err(PlatformError::Timeout(format!("{t:?} took too long")));
            }
            Ok(match t {
                Tool::VmStat => VM,
                Tool::Netstat => NET,
                Tool::Iostat => IO,
                Tool::Sysctl => "{ 1.0 2.0 3.0 }\n1024\n",
            }
            .to_string())
        }
    }

    #[test]
    fn the_catalog_covers_every_object_and_a_failing_tool_only_loses_its_own() {
        let cat = MacCounters::new(fake(&[])).catalog().unwrap();
        let names: Vec<_> = cat.objects.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["Memory", "Network Interface", "Physical Disk", "System", "Virtual Memory"]);
        let cat = MacCounters::new(fake(&[Tool::Iostat])).catalog().unwrap();
        assert!(cat.object("Physical Disk").is_none() && cat.object("Network Interface").is_some());
    }

    #[test]
    fn when_every_tool_fails_the_error_is_reported() {
        let all: &'static [Tool] = &Tool::ALL;
        assert!(matches!(MacCounters::new(fake(all)).catalog(), Err(PlatformError::Timeout(_))));
        let p: CounterPath = "\\Memory\\Free".parse().unwrap();
        let r = MacCounters::new(fake(all)).read(&[p]);
        assert!(r[0].value.as_ref().unwrap_err().contains("took too long"));
    }

    #[test]
    fn reading_runs_only_the_tools_the_paths_need() {
        let ran = std::cell::RefCell::new(Vec::new());
        let inner = fake(&[]);
        let c = MacCounters::new(|t| {
            ran.borrow_mut().push(t);
            inner(t)
        });
        let p: CounterPath = "\\Network Interface(en0)\\Bytes Sent".parse().unwrap();
        let r = c.read(&[p]);
        assert_eq!(r[0].value.as_ref().unwrap().value, 4000.0);
        assert_eq!(*ran.borrow(), [Tool::Netstat]);
    }

    /// The real tools on this Mac (read-only), if this is one.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_real_tools_parse() {
        for tool in Tool::ALL {
            let text = run_tool(tool).unwrap();
            assert!(!tool.parse(&text).is_empty(), "{tool:?}");
        }
    }
}
