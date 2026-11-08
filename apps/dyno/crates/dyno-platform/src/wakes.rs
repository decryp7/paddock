//! The helper's scheduled wakes and Wake-on-LAN modes (Dyno ADR 0032 D2), over a [`ToolRunner`] with fixed arguments (never text a
//! caller chose beyond a validated tag, a time and an interface name). Linux uses `systemd-run` with a timer that has
//! `WakeSystem=true`; macOS uses `pmset schedule wake` with an owner string, so an event is Dyno's only when its owner starts with
//! Dyno's prefix, and listing and cancelling look at those alone. The parsers are pure and compile everywhere.
#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]

use crate::runner::{tool, Tool, ToolRunner};
use dyno_core::ports::PlatformError;
use dyno_core::wake::{
    utc_civil, utc_seconds, IfName, WakeEntry, WakeScheduler, WakeTag, WolMode, WolReader, WolReply, LINUX_UNIT_PREFIX, MAC_OWNER_PREFIX,
};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);
const SYSTEMD_RUN: &str = "/usr/bin/systemd-run";
const SYSTEMCTL: &str = "/usr/bin/systemctl";
const PMSET: &str = "/usr/bin/pmset";
const ETHTOOL: &str = "/usr/sbin/ethtool";

// ---------------------------------------------------------------- Linux

/// `systemd-run`'s calendar for a UTC time: `2026-11-05 03:00:00 UTC`.
pub fn calendar_utc(at_utc: i64) -> String {
    let (y, mo, d, h, mi, s) = utc_civil(at_utc);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC")
}

/// The command that sets a wake: a transient timer unit whose only job is to wake the computer.
pub fn systemd_schedule(tag: &WakeTag, at_utc: i64) -> Tool {
    tool(
        SYSTEMD_RUN,
        &[
            &format!("--unit={LINUX_UNIT_PREFIX}{}", tag.as_str()),
            &format!("--on-calendar={}", calendar_utc(at_utc)),
            "--timer-property=WakeSystem=true",
            "--timer-property=RemainAfterElapse=no",
            "/bin/true",
        ],
    )
}

/// The command that cancels a wake.
pub fn systemd_cancel(tag: &WakeTag) -> Tool {
    tool(SYSTEMCTL, &["stop", &format!("{LINUX_UNIT_PREFIX}{}.timer", tag.as_str())])
}

/// Dyno's timers in `systemctl list-timers --all --output=json`: only units named `dyno-wake-<tag>.timer`, the next elapse in
/// microseconds since 1970 (`0` or missing: none pending, skipped), soonest first.
pub fn parse_list_timers(json: &str) -> Vec<WakeEntry> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let mut out: Vec<WakeEntry> = v
        .as_array()
        .into_iter()
        .flatten()
        .take(4096)
        .filter_map(|t| {
            let unit = t["unit"].as_str()?;
            let tag = unit.strip_prefix(LINUX_UNIT_PREFIX)?.strip_suffix(".timer")?;
            let next = t["next"].as_i64().filter(|n| *n > 0)?;
            Some(WakeEntry {
                tag: WakeTag::parse(tag).ok()?,
                at_utc: next / 1_000_000,
            })
        })
        .collect();
    out.sort_by_key(|w| w.at_utc);
    out
}

/// Scheduled wakes through systemd.
pub struct SystemdWakes(pub Box<dyn ToolRunner>);

impl WakeScheduler for SystemdWakes {
    fn schedule(&self, tag: &WakeTag, at_utc: i64) -> Result<(), PlatformError> {
        // The same tag replaces: stop a timer of that name first (a missing one is fine).
        if let Err(e) = self.0.run_tool(&systemd_cancel(tag), TIMEOUT) {
            log::debug!(Tag = tag.as_str(), error:% = e; "wake: the old timer could not be stopped first");
        }
        let out = self.0.run_tool(&systemd_schedule(tag, at_utc), TIMEOUT)?;
        if out.ok {
            log::info!(Tag = tag.as_str(); "wake scheduled (systemd timer)");
            Ok(())
        } else {
            Err(PlatformError::Failed(format!(
                "systemd-run failed ({}).",
                if out.error.is_empty() { "no message" } else { out.error.as_str() }
            )))
        }
    }

    fn cancel(&self, tag: &WakeTag) -> Result<bool, PlatformError> {
        let existed = self.list()?.iter().any(|w| &w.tag == tag);
        if existed {
            let out = self.0.run_tool(&systemd_cancel(tag), TIMEOUT)?;
            if !out.ok {
                return Err(PlatformError::Failed(format!("systemctl stop failed ({}).", out.error)));
            }
            log::info!(Tag = tag.as_str(); "wake cancelled (systemd timer)");
        }
        Ok(existed)
    }

    fn list(&self) -> Result<Vec<WakeEntry>, PlatformError> {
        let out = self.0.run_tool(
            &tool(
                SYSTEMCTL,
                &["list-timers", "--all", "--no-pager", "--output=json", "dyno-wake-*.timer"],
            ),
            TIMEOUT,
        )?;
        if !out.ok {
            return Err(PlatformError::Failed(format!("systemctl list-timers failed ({}).", out.error)));
        }
        Ok(parse_list_timers(&out.stdout))
    }
}

/// `ethtool <if>`: the modes after `Supports Wake-on:` and `Wake-on:`. `d` (disabled) means none.
pub fn parse_ethtool_wol(text: &str) -> WolReply {
    fn modes(letters: &str) -> Vec<WolMode> {
        letters
            .trim()
            .chars()
            .filter_map(|c| match c {
                'p' => Some(WolMode::Phy),
                'u' => Some(WolMode::Unicast),
                'm' => Some(WolMode::Multicast),
                'b' => Some(WolMode::Broadcast),
                'a' => Some(WolMode::Arp),
                'g' => Some(WolMode::Magic),
                's' => Some(WolMode::MagicSecure),
                _ => None,
            })
            .collect()
    }
    let after = |key: &str| {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(key).map(str::to_string))
            .unwrap_or_default()
    };
    WolReply {
        enabled: modes(&after("Wake-on:")),
        supported: modes(&after("Supports Wake-on:")),
    }
}

/// Wake-on-LAN modes through `ethtool` (the helper holds the capability the netlink query needs).
pub struct EthtoolWol(pub Box<dyn ToolRunner>);

impl WolReader for EthtoolWol {
    fn read(&self, ifname: &IfName) -> Result<WolReply, PlatformError> {
        let out = self.0.run_tool(&tool(ETHTOOL, &[ifname.as_str()]), TIMEOUT)?;
        if !out.ok {
            return Err(PlatformError::Failed(format!(
                "ethtool could not read {} ({}).",
                ifname.as_str(),
                out.error
            )));
        }
        Ok(parse_ethtool_wol(&out.stdout))
    }
}

// ---------------------------------------------------------------- macOS

/// `pmset`'s date for a local time: `MM/dd/yy HH:mm:ss`. `local_offset_secs` is the zone's offset at that time.
pub fn pmset_date(at_utc: i64, local_offset_secs: i64) -> String {
    let (y, mo, d, h, mi, s) = utc_civil(at_utc + local_offset_secs);
    format!("{mo:02}/{d:02}/{:02} {h:02}:{mi:02}:{s:02}", y % 100)
}

/// The command that sets a wake owned by Dyno.
pub fn pmset_schedule(tag: &WakeTag, at_utc: i64, local_offset_secs: i64) -> Tool {
    tool(
        PMSET,
        &[
            "schedule",
            "wake",
            &pmset_date(at_utc, local_offset_secs),
            &format!("{MAC_OWNER_PREFIX}{}", tag.as_str()),
        ],
    )
}

/// The command that cancels a wake owned by Dyno.
pub fn pmset_cancel(tag: &WakeTag, at_utc: i64, local_offset_secs: i64) -> Tool {
    tool(
        PMSET,
        &[
            "schedule",
            "cancel",
            "wake",
            &pmset_date(at_utc, local_offset_secs),
            &format!("{MAC_OWNER_PREFIX}{}", tag.as_str()),
        ],
    )
}

/// `pmset -g sched`: lines like `[0] wake at 11/05/2026 03:00:00 by 'net.decryptology.dyno.nightly'`. Only events owned by Dyno are
/// returned; the date is local, read as `(year, month, day, hour, minute, second)` and turned into UTC with `offset_at`.
pub fn parse_pmset_sched(text: &str, offset_at: &dyn Fn(i64) -> i64) -> Vec<WakeEntry> {
    let mut out: Vec<WakeEntry> = text
        .lines()
        .take(4096)
        .filter_map(|line| {
            let rest = line.split_once("wake at ")?.1;
            let (when, owner) = rest.split_once(" by ")?;
            let tag = owner.trim().trim_matches('\'').strip_prefix(MAC_OWNER_PREFIX)?;
            let (date, time) = when.trim().split_once(' ')?;
            let mut d = date.split('/').map(|x| x.parse::<i64>().ok());
            let (mo, day, y) = (d.next()??, d.next()??, d.next()??);
            let y = if y < 100 { 2000 + y } else { y };
            let mut t = time.split(':').map(|x| x.parse::<i64>().ok());
            let (h, mi, s) = (t.next()??, t.next()??, t.next().flatten().unwrap_or(0));
            let as_if_utc = utc_seconds(y, mo, day, h, mi, s);
            Some(WakeEntry {
                tag: WakeTag::parse(tag).ok()?,
                at_utc: as_if_utc - offset_at(as_if_utc),
            })
        })
        .collect();
    out.sort_by_key(|w| w.at_utc);
    out
}

/// The system zone's offset from UTC at `utc`, in seconds (0 when the zone cannot be read: the wake is then set in UTC terms).
pub fn system_offset_at(utc: i64) -> i64 {
    jiff::Timestamp::from_second(utc)
        .map(|t| i64::from(jiff::tz::TimeZone::system().to_offset(t).seconds()))
        .unwrap_or(0)
}

/// Scheduled wakes through `pmset`. `offset_at(utc)` is the local zone's offset in seconds at that instant (the system zone).
pub struct PmsetWakes {
    runner: Box<dyn ToolRunner>,
    offset_at: Box<dyn Fn(i64) -> i64 + Send + Sync>,
}

impl PmsetWakes {
    /// Wakes over `runner`, with the zone's offset given.
    pub fn new(runner: Box<dyn ToolRunner>, offset_at: Box<dyn Fn(i64) -> i64 + Send + Sync>) -> Self {
        PmsetWakes { runner, offset_at }
    }
}

impl WakeScheduler for PmsetWakes {
    fn schedule(&self, tag: &WakeTag, at_utc: i64) -> Result<(), PlatformError> {
        // The same tag replaces: cancel the event of that tag first.
        if let Err(e) = self.cancel(tag) {
            log::debug!(Tag = tag.as_str(), error:% = e; "wake: the old event could not be cancelled first");
        }
        let out = self
            .runner
            .run_tool(&pmset_schedule(tag, at_utc, (self.offset_at)(at_utc)), TIMEOUT)?;
        if out.ok {
            log::info!(Tag = tag.as_str(); "wake scheduled (pmset)");
            Ok(())
        } else {
            Err(PlatformError::Failed(format!(
                "pmset failed ({}).",
                if out.error.is_empty() { "no message" } else { out.error.as_str() }
            )))
        }
    }

    fn cancel(&self, tag: &WakeTag) -> Result<bool, PlatformError> {
        let mine: Vec<_> = self.list()?.into_iter().filter(|w| &w.tag == tag).collect();
        for w in &mine {
            let out = self
                .runner
                .run_tool(&pmset_cancel(tag, w.at_utc, (self.offset_at)(w.at_utc)), TIMEOUT)?;
            if !out.ok {
                return Err(PlatformError::Failed(format!("pmset could not cancel the wake ({}).", out.error)));
            }
        }
        if !mine.is_empty() {
            log::info!(Tag = tag.as_str(); "wake cancelled (pmset)");
        }
        Ok(!mine.is_empty())
    }

    fn list(&self) -> Result<Vec<WakeEntry>, PlatformError> {
        let out = self.runner.run_tool(&tool(PMSET, &["-g", "sched"]), TIMEOUT)?;
        if !out.ok {
            return Err(PlatformError::Failed(format!("pmset -g sched failed ({}).", out.error)));
        }
        Ok(parse_pmset_sched(&out.stdout, &*self.offset_at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::ToolOutput;
    use std::sync::{Arc, Mutex};

    fn tag(s: &str) -> WakeTag {
        WakeTag::parse(s).unwrap()
    }

    // 2026-11-05 03:00:00 UTC
    const AT: i64 = 1_793_847_600;

    #[test]
    fn linux_commands_are_fixed_and_carry_only_a_valid_tag_and_a_calendar() {
        assert_eq!(calendar_utc(AT), "2026-11-05 03:00:00 UTC");
        let t = systemd_schedule(&tag("nightly-backup"), AT);
        assert_eq!(t.program, "/usr/bin/systemd-run");
        assert_eq!(
            t.args,
            [
                "--unit=dyno-wake-nightly-backup",
                "--on-calendar=2026-11-05 03:00:00 UTC",
                "--timer-property=WakeSystem=true",
                "--timer-property=RemainAfterElapse=no",
                "/bin/true"
            ]
        );
        assert_eq!(systemd_cancel(&tag("a")).args, ["stop", "dyno-wake-a.timer"]);
    }

    #[test]
    fn only_dyno_timers_are_listed_soonest_first_and_junk_is_ignored() {
        let json = r#"[{"next":1793851200000000,"unit":"dyno-wake-b.timer"},{"next":1793847600000000,"unit":"dyno-wake-a.timer"},
{"next":1793847600000000,"unit":"apt-daily.timer"},{"next":0,"unit":"dyno-wake-idle.timer"},{"unit":"dyno-wake-none.timer"},
{"next":1793847600000000,"unit":"dyno-wake-Bad Tag.timer"},{"next":1793847600000000,"unit":"dyno-wake-x.service"}]"#;
        let got = parse_list_timers(json);
        assert_eq!(
            got.iter().map(|w| (w.tag.as_str(), w.at_utc)).collect::<Vec<_>>(),
            [("a", AT), ("b", AT + 3_600)]
        );
        for junk in ["", "not json", "{}", "[1,2,null]"] {
            assert!(parse_list_timers(junk).is_empty(), "{junk:?}");
        }
    }

    #[test]
    fn ethtool_modes_are_read_as_enabled_and_supported() {
        let text = "Settings for eth0:\n\tSupports Wake-on: pumbg\n\tWake-on: g\n\tLink detected: yes\n";
        let r = parse_ethtool_wol(text);
        assert_eq!(r.enabled, [WolMode::Magic]);
        assert_eq!(
            r.supported,
            [
                WolMode::Phy,
                WolMode::Unicast,
                WolMode::Multicast,
                WolMode::Broadcast,
                WolMode::Magic
            ]
        );
        let off = parse_ethtool_wol("\tSupports Wake-on: g\n\tWake-on: d\n");
        assert!(off.enabled.is_empty() && off.supported == [WolMode::Magic]);
        assert_eq!(
            parse_ethtool_wol("garbage"),
            WolReply {
                enabled: vec![],
                supported: vec![]
            }
        );
    }

    #[test]
    fn pmset_dates_follow_the_zone_and_only_dyno_owned_events_are_listed() {
        let plus8 = |_: i64| 8 * 3_600;
        assert_eq!(pmset_date(AT, 8 * 3_600), "11/05/26 11:00:00");
        let t = pmset_schedule(&tag("nightly"), AT, 8 * 3_600);
        assert_eq!(t.args, ["schedule", "wake", "11/05/26 11:00:00", "net.decryptology.dyno.nightly"]);
        let text = "Scheduled power events:\n [0]  wake at 11/05/2026 11:00:00 by 'net.decryptology.dyno.nightly'\n [1]  wake at 11/06/2026 07:30:00 by 'com.apple.alarm.user-visible-Weekly'\n [2]  wake at 11/04/2026 11:00:00 by 'net.decryptology.dyno.early'\n [3]  wake at garbage by 'net.decryptology.dyno.x'\n";
        let got = parse_pmset_sched(text, &plus8);
        assert_eq!(
            got.iter().map(|w| (w.tag.as_str(), w.at_utc)).collect::<Vec<_>>(),
            [("early", AT - 86_400), ("nightly", AT)]
        );
    }

    /// Records the commands and answers from a script.
    struct Fake(Arc<Mutex<Vec<String>>>, Mutex<Vec<ToolOutput>>);
    impl ToolRunner for Fake {
        fn run_tool(&self, t: &Tool, _: Duration) -> Result<ToolOutput, PlatformError> {
            self.0.lock().unwrap().push(format!("{} {}", t.program, t.args.join(" ")));
            let mut a = self.1.lock().unwrap();
            Ok(if a.is_empty() {
                ToolOutput {
                    ok: true,
                    stdout: String::new(),
                    error: String::new(),
                }
            } else {
                a.remove(0)
            })
        }
    }
    fn say(ok: bool, stdout: &str) -> ToolOutput {
        ToolOutput {
            ok,
            stdout: stdout.into(),
            error: if ok { String::new() } else { "denied".into() },
        }
    }
    fn fake(answers: Vec<ToolOutput>) -> (Box<dyn ToolRunner>, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        (Box::new(Fake(log.clone(), Mutex::new(answers))), log)
    }

    #[test]
    fn scheduling_replaces_a_wake_of_the_same_tag_and_reports_a_failure_with_its_reason() {
        let (runner, log) = fake(vec![say(true, ""), say(true, "")]);
        SystemdWakes(runner).schedule(&tag("a"), AT).unwrap();
        let log = log.lock().unwrap();
        assert!(
            log[0].contains("stop dyno-wake-a.timer") && log[1].contains("--unit=dyno-wake-a"),
            "{log:?}"
        );
        let (runner, _) = fake(vec![say(true, ""), say(false, "")]);
        assert!(matches!(SystemdWakes(runner).schedule(&tag("a"), AT), Err(PlatformError::Failed(m)) if m.contains("denied")));
    }

    #[test]
    fn cancelling_stops_only_a_timer_that_is_listed_and_says_whether_there_was_one() {
        let listed = r#"[{"next":1793847600000000,"unit":"dyno-wake-a.timer"}]"#;
        let (runner, log) = fake(vec![say(true, listed), say(true, "")]);
        assert!(SystemdWakes(runner).cancel(&tag("a")).unwrap());
        assert!(log.lock().unwrap()[1].contains("stop dyno-wake-a.timer"));
        let (runner, log) = fake(vec![say(true, "[]")]);
        assert!(!SystemdWakes(runner).cancel(&tag("a")).unwrap());
        assert_eq!(log.lock().unwrap().len(), 1, "nothing to stop, nothing run");
    }

    #[test]
    fn pmset_cancel_only_touches_events_with_dynos_owner() {
        let sched =
            " [0] wake at 11/05/2026 11:00:00 by 'net.decryptology.dyno.a'\n [1] wake at 11/05/2026 11:00:00 by 'com.apple.alarm'\n";
        let (runner, log) = fake(vec![say(true, sched), say(true, "")]);
        let w = PmsetWakes::new(runner, Box::new(|_| 8 * 3_600));
        assert!(w.cancel(&tag("a")).unwrap());
        let log = log.lock().unwrap();
        assert!(
            log[1].contains("schedule cancel wake 11/05/26 11:00:00 net.decryptology.dyno.a"),
            "{log:?}"
        );
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn ethtool_failures_name_the_interface() {
        let (runner, _) = fake(vec![say(false, "")]);
        let e = EthtoolWol(runner).read(&IfName::parse("eth0").unwrap()).unwrap_err();
        assert!(matches!(e, PlatformError::Failed(m) if m.contains("eth0")));
    }
}
