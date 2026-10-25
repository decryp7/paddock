//! Boot analysis: how long start-up took, what slowed it, and whether it is slower than usual. The stored boots (the
//! `boots` table) give the history; per-OS sources add the breakdown: `systemd-analyze time`/`blame` (Linux) and the
//! Diagnostics-Performance events 100-110 (Windows, `wevtutil ... /f:xml`). macOS keeps no breakdown, only the boot time
//! (`sysctl kern.boottime`), and says so.

use super::{clean, clip};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

/// Most steps listed.
pub const MAX_STEPS: usize = 15;

/// One thing that took time during start-up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootStep {
    /// The service, driver or program.
    pub name: String,
    /// Seconds it took (or delayed start-up by).
    pub secs: f32,
}

/// One boot in the history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootRow {
    /// When the computer started.
    pub at: Timestamp,
    /// Seconds until the system was up, when known.
    pub boot_secs: Option<f32>,
    /// Seconds until it was ready for use (settled), when known.
    pub ready_secs: Option<f32>,
}

/// The Boot view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BootReport {
    /// Boots, newest first.
    pub boots: Vec<BootRow>,
    /// What slowed the last start-up, slowest first.
    pub slowest: Vec<BootStep>,
    /// The last boot's total in seconds, when the OS tells.
    pub last_total_secs: Option<f32>,
    /// One sentence: how the last boot compares with the usual one, or `None` without enough history.
    pub verdict: Option<String>,
    /// Why the breakdown is unavailable (the history may still be there).
    pub steps_unavailable: Option<String>,
}

/// Finds when the desktop settled after login: the first moment the machine stayed under a CPU threshold for a whole
/// hold time. Fed one sample at a time by the agent.
#[derive(Debug, Clone, Copy)]
pub struct SettleTracker {
    started: Timestamp,
    threshold: f32,
    hold_ms: i64,
    quiet_since: Option<Timestamp>,
}

impl SettleTracker {
    /// Starts watching at `started` (the moment the session began).
    pub fn new(started: Timestamp, threshold_percent: f32, hold_ms: i64) -> SettleTracker {
        SettleTracker {
            started,
            threshold: threshold_percent,
            hold_ms,
            quiet_since: None,
        }
    }

    /// A sample of the machine's CPU (percent) at `at`. Returns the seconds from the start to the beginning of the quiet
    /// stretch once that stretch has lasted the hold time.
    pub fn observe(&mut self, at: Timestamp, cpu_percent: f32) -> Option<f32> {
        if cpu_percent > self.threshold {
            self.quiet_since = None;
            return None;
        }
        let since = *self.quiet_since.get_or_insert(at);
        (at.since(since) >= self.hold_ms).then(|| since.since(self.started).max(0) as f32 / 1000.0)
    }
}

/// `Startup finished in 3.2s (firmware) + 5.1s (loader) + 2.3s (kernel) + 1min 12.5s (userspace) = 1min 23.1s`:
/// the total in seconds.
pub fn parse_systemd_time(text: &str) -> Option<f32> {
    let line = text.lines().find(|l| l.starts_with("Startup finished in"))?;
    let total = line
        .rsplit_once('=')
        .map_or(line.trim_start_matches("Startup finished in"), |(_, t)| t);
    parse_systemd_duration(total.trim().trim_end_matches('.'))
}

/// `1min 12.5s`, `3.2s`, `450ms`, `2h 3min 4s`.
pub fn parse_systemd_duration(text: &str) -> Option<f32> {
    let mut secs = 0.0f32;
    let mut seen = false;
    for tok in text.split_whitespace() {
        let split = tok.find(|c: char| c.is_ascii_alphabetic())?;
        let (num, unit) = tok.split_at(split);
        let n: f32 = num.parse().ok()?;
        secs += match unit {
            "h" => n * 3600.0,
            "min" => n * 60.0,
            "s" => n,
            "ms" => n / 1000.0,
            "us" => n / 1_000_000.0,
            _ => return None,
        };
        seen = true;
    }
    seen.then_some(secs)
}

/// `systemd-analyze blame`: `1min 2.1s NetworkManager-wait-online.service` per line, slowest first.
pub fn parse_systemd_blame(text: &str) -> Vec<BootStep> {
    let mut out: Vec<BootStep> = text
        .lines()
        .map(clean)
        .filter_map(|l| {
            let (dur, name) = l.rsplit_once(' ')?;
            Some(BootStep {
                name: clip(name, 100),
                secs: parse_systemd_duration(dur)?,
            })
        })
        .collect();
    out.sort_by(|a, b| b.secs.total_cmp(&a.secs));
    out.truncate(MAX_STEPS);
    out
}

/// One `<Event>` of the Diagnostics-Performance log as `wevtutil qe ... /f:xml` prints it: its id and `Name`d data.
struct XmlEvent<'a> {
    id: u32,
    data: Vec<(&'a str, &'a str)>,
}

fn xml_events(xml: &str) -> Vec<XmlEvent<'_>> {
    let mut out = Vec::new();
    for block in xml.split("<Event ").skip(1).take(64) {
        let block = block.split("</Event>").next().unwrap_or(block);
        let id = block
            .split("<EventID")
            .nth(1)
            .and_then(|r| r.split_once('>'))
            .and_then(|(_, r)| r.split('<').next())
            .and_then(|n| n.trim().parse().ok());
        let Some(id) = id else { continue };
        let mut data = Vec::new();
        for part in block.split("<Data Name='").skip(1) {
            let Some((name, rest)) = part.split_once("'>") else { continue };
            let value = rest.split("</Data>").next().unwrap_or("");
            data.push((name, value));
        }
        out.push(XmlEvent { id, data });
    }
    out
}

fn data<'a>(e: &XmlEvent<'a>, name: &str) -> Option<&'a str> {
    e.data.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

/// Windows event 100 (boot performance): `BootTime` in milliseconds.
pub fn parse_windows_boot_total(xml: &str) -> Option<f32> {
    xml_events(xml)
        .iter()
        .find(|e| e.id == 100)
        .and_then(|e| data(e, "BootTime"))
        .and_then(|v| v.parse::<f32>().ok())
        .map(|ms| ms / 1000.0)
}

/// Windows events 101-110 (an application, driver or service slowed the boot): `Name`/`FriendlyName` and `TotalTime`
/// (or `DegradationTime`) in milliseconds, slowest first.
pub fn parse_windows_boot_degradations(xml: &str) -> Vec<BootStep> {
    let mut out: Vec<BootStep> = xml_events(xml)
        .iter()
        .filter(|e| (101..=110).contains(&e.id))
        .filter_map(|e| {
            let name = data(e, "FriendlyName").or_else(|| data(e, "Name"))?;
            let ms: f32 = data(e, "TotalTime").or_else(|| data(e, "DegradationTime"))?.parse().ok()?;
            Some(BootStep {
                name: clip(name, 100),
                secs: ms / 1000.0,
            })
        })
        .collect();
    out.sort_by(|a, b| b.secs.total_cmp(&a.secs));
    out.truncate(MAX_STEPS);
    out
}

/// `kern.boottime: { sec = 1792628306, usec = 91888 } Thu Oct 22 08:18:26 2026`: when the computer started.
pub fn parse_macos_boottime(text: &str) -> Option<Timestamp> {
    let sec = text.split("sec =").nth(1)?.split(',').next()?.trim().parse::<i64>().ok()?;
    (sec > 0).then_some(Timestamp(sec.checked_mul(1000)?))
}

/// How the last boot compares with the median of the earlier ones. Needs at least three earlier boots with a time.
pub fn verdict(boots: &[BootRow]) -> Option<String> {
    let secs = |b: &BootRow| b.ready_secs.or(b.boot_secs);
    let mut sorted: Vec<&BootRow> = boots.iter().collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.at));
    let (last, earlier) = sorted.split_first()?;
    let last_secs = secs(last)?;
    let mut prev: Vec<f32> = earlier.iter().filter_map(|b| secs(b)).collect();
    if prev.len() < 3 {
        return None;
    }
    prev.sort_by(f32::total_cmp);
    let median = prev[prev.len() / 2];
    if median <= 0.0 {
        return None;
    }
    let ratio = last_secs / median;
    Some(if ratio >= 1.5 {
        format!(
            "The last start-up took {last_secs:.0} s, {:.0} % longer than the usual {median:.0} s.",
            (ratio - 1.0) * 100.0
        )
    } else if ratio <= 0.75 {
        format!("The last start-up took {last_secs:.0} s, faster than the usual {median:.0} s.")
    } else {
        format!("The last start-up took {last_secs:.0} s, about the usual {median:.0} s.")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemd_durations() {
        assert_eq!(parse_systemd_duration("3.2s"), Some(3.2));
        assert_eq!(parse_systemd_duration("1min 12.5s"), Some(72.5));
        assert_eq!(parse_systemd_duration("450ms"), Some(0.45));
        assert_eq!(parse_systemd_duration("2h 3min 4s"), Some(7384.0));
        assert_eq!(parse_systemd_duration("soon"), None);
        assert_eq!(parse_systemd_duration(""), None);
        assert_eq!(parse_systemd_duration("5 parsecs"), None);
    }

    #[test]
    fn systemd_time_and_blame() {
        let t = "Startup finished in 3.2s (firmware) + 5.1s (loader) + 2.3s (kernel) + 1min 12.5s (userspace) = 1min 23.1s\ngraphical.target reached after 1min 12s in userspace.";
        assert!((parse_systemd_time(t).unwrap() - 83.1).abs() < 0.01);
        assert_eq!(parse_systemd_time("nothing"), None);
        let b = parse_systemd_blame("1.2s plymouth.service\n1min 2.1s NetworkManager-wait-online.service\n\n300ms x.service\njunk\n");
        assert_eq!(b[0].name, "NetworkManager-wait-online.service");
        assert!((b[0].secs - 62.1).abs() < 0.01);
        assert_eq!(b.len(), 3);
    }

    const XML: &str = "<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><EventID>100</EventID></System><EventData><Data Name='BootTime'>41234</Data><Data Name='MainPathBootTime'>30000</Data></EventData></Event>
<Event xmlns='x'><System><EventID>101</EventID></System><EventData><Data Name='Name'>Big App</Data><Data Name='FriendlyName'>Big App 2</Data><Data Name='TotalTime'>9100</Data></EventData></Event>
<Event xmlns='x'><System><EventID>103</EventID></System><EventData><Data Name='Name'>svc</Data><Data Name='TotalTime'>12000</Data></EventData></Event>
<Event xmlns='x'><System><EventID>200</EventID></System><EventData><Data Name='Name'>other</Data><Data Name='TotalTime'>1</Data></EventData></Event>";

    #[test]
    fn windows_events() {
        assert_eq!(parse_windows_boot_total(XML), Some(41.234));
        let d = parse_windows_boot_degradations(XML);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].name, "svc");
        assert_eq!(d[1].name, "Big App 2");
        assert_eq!(parse_windows_boot_total("<Event ></Event>"), None);
        assert!(parse_windows_boot_degradations("garbage <Event ").is_empty());
    }

    #[test]
    fn macos_boot_time() {
        let t = parse_macos_boottime("kern.boottime: { sec = 1792628306, usec = 91888 } Thu Oct 22 08:18:26 2026").unwrap();
        assert_eq!(t, Timestamp(1_792_628_306_000));
        assert_eq!(parse_macos_boottime("nope"), None);
        assert_eq!(parse_macos_boottime("sec = -1,"), None);
    }

    fn boot(at: i64, ready: Option<f32>) -> BootRow {
        BootRow {
            at: Timestamp(at),
            boot_secs: None,
            ready_secs: ready,
        }
    }

    #[test]
    fn the_desktop_settles_when_the_cpu_stays_quiet_for_the_hold_time() {
        let mut t = SettleTracker::new(Timestamp(0), 30.0, 30_000);
        assert_eq!(t.observe(Timestamp(10_000), 80.0), None);
        assert_eq!(t.observe(Timestamp(20_000), 10.0), None);
        // A spike resets the quiet stretch.
        assert_eq!(t.observe(Timestamp(30_000), 60.0), None);
        assert_eq!(t.observe(Timestamp(40_000), 5.0), None);
        assert_eq!(t.observe(Timestamp(60_000), 5.0), None);
        assert_eq!(t.observe(Timestamp(70_000), 5.0), Some(40.0));
        // A start in the future never gives a negative time.
        let mut f = SettleTracker::new(Timestamp(100_000), 30.0, 0);
        assert_eq!(f.observe(Timestamp(10), 1.0), Some(0.0));
    }

    #[test]
    fn the_verdict_compares_with_the_median() {
        let usual = [boot(1, Some(40.0)), boot(2, Some(42.0)), boot(3, Some(44.0)), boot(4, Some(41.0))];
        let mut slow = usual.to_vec();
        slow.push(boot(5, Some(90.0)));
        assert!(verdict(&slow).unwrap().contains("longer than the usual"));
        let mut same = usual.to_vec();
        same.push(boot(5, Some(43.0)));
        assert!(verdict(&same).unwrap().contains("about the usual"));
        let mut fast = usual.to_vec();
        fast.push(boot(5, Some(20.0)));
        assert!(verdict(&fast).unwrap().contains("faster"));
        assert_eq!(verdict(&usual[..3]), None);
        assert_eq!(verdict(&[]), None);
        assert_eq!(
            verdict(&[boot(9, None), boot(1, Some(1.0)), boot(2, Some(1.0)), boot(3, Some(1.0))]),
            None
        );
    }
}
