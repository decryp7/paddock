//! "Why won't it sleep?" and "what woke it?": parsers for `pmset -g assertions` and `pmset -g log` (macOS),
//! `powercfg /requests` and `powercfg /lastwake` (Windows) and `systemd-inhibit --list` (Linux). Read only; the agent
//! runs the tools and shows what these functions return.

use super::{clean, clip};
use crate::helper_extras::{Requester, WakeRequest};
use serde::{Deserialize, Serialize};

/// Most entries of each list.
pub const MAX_ENTRIES: usize = 50;

/// What a blocker prevents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Prevents {
    /// The computer going to sleep when idle.
    Sleep,
    /// The display turning off.
    DisplaySleep,
    /// Something else (a network, a shutdown, a lid switch).
    Other,
}

/// Something that keeps the computer awake.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SleepBlocker {
    /// The program, service or driver.
    pub name: String,
    /// Its process, when the tool says.
    pub pid: Option<u32>,
    /// What it prevents.
    pub prevents: Prevents,
    /// Why, in the program's words (clipped).
    pub reason: String,
    /// How long it has held on, when the tool says ("00:12:03").
    pub held_for: Option<String>,
}

/// One wake of the computer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeEvent {
    /// When, as the tool printed it (local time), when it says.
    pub when: Option<String>,
    /// What woke it.
    pub reason: String,
}

/// The "Sleep" view: blockers and recent wakes, each possibly unavailable with a reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SleepReport {
    /// What keeps the computer awake now.
    pub blockers: Vec<SleepBlocker>,
    /// Recent wakes, newest first.
    pub wakes: Vec<WakeEvent>,
    /// Why the blockers could not be read.
    pub blockers_unavailable: Option<String>,
    /// Why the wakes could not be read.
    pub wakes_unavailable: Option<String>,
    /// Who read the blockers.
    #[serde(default)]
    pub blockers_via: crate::read_via::ReadVia,
}

fn prevents_from(kind: &str) -> Prevents {
    let k = kind.to_ascii_lowercase();
    if k.contains("display") {
        Prevents::DisplaySleep
    } else if k.contains("sleep") || k.contains("idle") {
        Prevents::Sleep
    } else {
        Prevents::Other
    }
}

/// `pmset -g assertions`: the lines under "Listed by owning process", e.g.
/// `pid 123(coreaudiod): [0x0000000100019f7a] 00:12:03 PreventUserIdleSystemSleep named: "audio playing"`.
/// Assertions that do not prevent sleep (BackgroundTask, UserIsActive...) are dropped.
pub fn parse_pmset_assertions(text: &str) -> Vec<SleepBlocker> {
    let mut out = Vec::new();
    for line in text.lines().map(clean) {
        let Some(rest) = line.strip_prefix("pid ") else { continue };
        let Some((pid_name, tail)) = rest.split_once("): ") else { continue };
        let Some((pid, name)) = pid_name.split_once('(') else { continue };
        let Ok(pid) = pid.trim().parse::<u32>() else { continue };
        let mut parts = tail.splitn(4, ' ');
        let (Some(_id), Some(held), Some(kind), Some(rest)) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if !kind.starts_with("Prevent") || !kind.contains("Sleep") {
            continue;
        }
        let prevents = prevents_from(kind);
        let reason = rest
            .split_once("named:")
            .map(|(_, r)| r.trim().trim_matches('"').to_string())
            .unwrap_or_default();
        if out.len() >= MAX_ENTRIES {
            break;
        }
        out.push(SleepBlocker {
            name: clip(name, 100),
            pid: Some(pid),
            prevents,
            reason: clip(&reason, 200),
            held_for: held.contains(':').then(|| held.to_string()),
        });
    }
    out
}

/// `pmset -g log`: lines such as `2026-10-23 08:01:12 +0800 Wake  Wake from Normal Sleep [CDNVA] : due to EC.LidOpen/Lid Open`.
/// Newest first.
pub fn parse_pmset_wakes(text: &str) -> Vec<WakeEvent> {
    let mut out: Vec<WakeEvent> = text
        .lines()
        .map(clean)
        .filter_map(|line| {
            if !line.is_char_boundary(25) {
                return None;
            }
            let (stamp, rest) = line.split_at_checked(25)?;
            let kind = rest.trim_start();
            if !stamp.starts_with("20") || !(kind.starts_with("Wake ") || kind.starts_with("DarkWake ")) {
                return None;
            }
            // The "Wake Requests" summary lines have no cause; real wakes say "due to".
            let (_, reason) = rest.split_once("due to")?;
            Some(WakeEvent {
                when: Some(stamp.trim().to_string()),
                reason: clip(reason.trim(), 200),
            })
        })
        .collect();
    out.reverse();
    out.truncate(MAX_ENTRIES);
    out
}

/// The sleep blockers `powercfg /requests` listed. The text is read once, by [`crate::helper_extras::parse_requests`] (the
/// helper's parser and the agent's own run of the tool share it, rule 55); this turns its requests into the report's blockers.
pub fn blockers_from_requests(requests: &[WakeRequest]) -> Vec<SleepBlocker> {
    requests
        .iter()
        .take(MAX_ENTRIES)
        .map(|r| {
            let prevents = match r.category.as_str() {
                "DISPLAY" => Prevents::DisplaySleep,
                "SYSTEM" | "AWAYMODE" => Prevents::Sleep,
                _ => Prevents::Other,
            };
            let short = r.name.rsplit(['\\', '/']).next().unwrap_or(&r.name);
            let kind = match r.requester {
                Requester::Process => "process",
                Requester::Driver => "driver",
                Requester::Service => "service",
                Requester::Other => "other",
            };
            SleepBlocker {
                name: clip(short, 100),
                pid: None,
                prevents,
                reason: match &r.reason {
                    Some(why) => clip(why, 200),
                    None => format!("{kind} request ({})", r.category.to_ascii_lowercase()),
                },
                held_for: None,
            }
        })
        .collect()
}

/// `powercfg /lastwake`: `Wake History Count - 1`, then `Wake Source [0]` blocks with `Type:` and `Friendly Name:`.
pub fn parse_powercfg_lastwake(text: &str) -> Vec<WakeEvent> {
    let mut out = Vec::new();
    let (mut kind, mut name): (Option<String>, Option<String>) = (None, None);
    let flush = |kind: &mut Option<String>, name: &mut Option<String>, out: &mut Vec<WakeEvent>| {
        let reason = match (kind.take(), name.take()) {
            (Some(k), Some(n)) => format!("{k}: {n}"),
            (Some(k), None) => k,
            (None, Some(n)) => n,
            (None, None) => return,
        };
        out.push(WakeEvent { when: None, reason });
    };
    for line in text.lines().map(clean) {
        if line.starts_with("Wake Source [") {
            flush(&mut kind, &mut name, &mut out);
        } else if let Some(v) = line.strip_prefix("Type:") {
            kind = Some(clip(v.trim(), 100));
        } else if let Some(v) = line.strip_prefix("Friendly Name:") {
            name = Some(clip(v.trim(), 200));
        } else if let Some(v) = line.strip_prefix("Instance Path:") {
            // Used only when there is no friendly name.
            if name.is_none() {
                name = Some(clip(v.trim(), 200));
            }
        }
    }
    flush(&mut kind, &mut name, &mut out);
    out.truncate(MAX_ENTRIES);
    out
}

/// `systemd-inhibit --list`: a table `WHO UID USER PID COMM WHAT WHY MODE`. `WHO` and `WHY` may hold spaces, so the row is
/// anchored on the numeric `UID` and `PID` columns. Only `block` inhibitors that mention sleep or idle count.
pub fn parse_systemd_inhibit(text: &str) -> Vec<SleepBlocker> {
    let mut out = Vec::new();
    for line in text.lines().skip(1).map(clean) {
        let t: Vec<&str> = line.split_whitespace().collect();
        let num = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        let Some(uid) = (1..t.len().saturating_sub(5)).find(|&i| num(t[i]) && num(t[i + 2])) else {
            continue;
        };
        let (who, pid, what) = (t[..uid].join(" "), t[uid + 2], t[uid + 4]);
        let mode = t[t.len() - 1];
        let why = t[uid + 5..t.len() - 1].join(" ");
        if mode != "block" {
            continue;
        }
        let prevents = if what.contains("sleep") {
            Prevents::Sleep
        } else if what.contains("idle") {
            Prevents::DisplaySleep
        } else {
            continue;
        };
        if out.len() < MAX_ENTRIES {
            out.push(SleepBlocker {
                name: clip(&who, 100),
                pid: pid.parse().ok(),
                prevents,
                reason: clip(&why, 200),
                held_for: None,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASSERTIONS: &str = "2026-10-23 10:15:01 +0800
Assertion status system-wide:
   BackgroundTask                 0
   UserIsActive                   1
   PreventUserIdleDisplaySleep    1
   PreventSystemSleep             0
   PreventUserIdleSystemSleep     1
Listed by owning process:
   pid 412(coreaudiod): [0x0000000100019f7a] 00:12:03 PreventUserIdleSystemSleep named: \"com.apple.audio.context412.preventuseridlesleep\"
\tCreated for PID: 801.
   pid 977(Zoom): [0x000000010001abcd] 01:02:03 PreventUserIdleDisplaySleep named: \"Zoom meeting\"
   pid 55(backupd): [0x0000000100011111] 00:00:12 BackgroundTask named: \"com.apple.backupd\"
Kernel Assertions: 0x4=USB
";

    #[test]
    fn pmset_assertions_keep_only_what_prevents_sleep() {
        let b = parse_pmset_assertions(ASSERTIONS);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].name, "coreaudiod");
        assert_eq!(b[0].pid, Some(412));
        assert_eq!(b[0].prevents, Prevents::Sleep);
        assert_eq!(b[0].held_for.as_deref(), Some("00:12:03"));
        assert!(b[0].reason.contains("preventuseridlesleep"));
        assert_eq!(b[1].prevents, Prevents::DisplaySleep);
        assert_eq!(b[1].reason, "Zoom meeting");
    }

    #[test]
    fn pmset_wakes_are_newest_first_and_need_a_reason() {
        let log = "2026-10-22 23:01:00 +0800 Sleep   Entering Sleep state due to 'Idle Sleep':TCPKeepAlive=active
2026-10-23 08:01:12 +0800 Wake    Wake from Normal Sleep [CDNVA] : due to EC.LidOpen/Lid Open: Using AC
2026-10-23 03:00:00 +0800 DarkWake DarkWake from Normal Sleep [CDNPB] : due to RTC/Maintenance Using AC
2026-10-23 09:00:00 +0800 Wake Requests [Wake Requests]
garbage
";
        let w = parse_pmset_wakes(log);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].when.as_deref(), Some("2026-10-23 03:00:00 +0800"));
        assert!(w[1].reason.starts_with("EC.LidOpen"));
    }

    #[test]
    fn powercfg_requests_per_section() {
        let t = "DISPLAY:
None.

SYSTEM:
[PROCESS] \\Device\\HarddiskVolume3\\Program Files\\App\\player.exe
Playing a video

[DRIVER] Realtek High Definition Audio

AWAYMODE:
None.
";
        let b = blockers_from_requests(&crate::helper_extras::parse_requests(t));
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].name, "player.exe");
        assert_eq!(b[0].prevents, Prevents::Sleep);
        assert_eq!(b[0].reason, "Playing a video");
        assert_eq!(b[1].name, "Realtek High Definition Audio");
        assert!(b[1].reason.contains("driver request (system)"));
        assert!(blockers_from_requests(&crate::helper_extras::parse_requests("DISPLAY:\nNone.\n")).is_empty());
    }

    #[test]
    fn powercfg_lastwake() {
        let t = "Wake History Count - 1
Wake History [0]
  Wake Source Count - 1
  Wake Source [0]
    Type: Device
    Instance Path: PCI\\VEN_8086&DEV_15F3\\4&1A2B3C
    Friendly Name: Intel(R) Ethernet Controller
";
        let w = parse_powercfg_lastwake(t);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].reason, "Device: Intel(R) Ethernet Controller");
        assert!(parse_powercfg_lastwake("Wake History Count - 0\n").is_empty());
    }

    #[test]
    fn systemd_inhibitors_that_block_sleep() {
        let t = "WHO            UID  USER PID  COMM            WHAT                 WHY                       MODE
Firefox        1000 sam  2210 firefox        sleep                Playing audio             block
NetworkManager 0    root 800  NetworkManager  sleep                NetworkManager needs to turn off networks delay
GNOME Shell    1000 sam  1500 gnome-shell     handle-power-key     Handling keypress         block
3 inhibitors listed.
";
        let b = parse_systemd_inhibit(t);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].name, "Firefox");
        assert_eq!(b[0].pid, Some(2210));
        assert_eq!(b[0].reason, "Playing audio");
    }

    #[test]
    fn garbage_and_huge_lines_never_panic() {
        let junk = "pid (: \npid x(y): z\n\u{0}\n".to_string() + &"a".repeat(5_000);
        assert!(parse_pmset_assertions(&junk).is_empty());
        assert!(parse_pmset_wakes(&junk).is_empty());
        assert!(blockers_from_requests(&crate::helper_extras::parse_requests(&junk)).is_empty());
        assert!(parse_systemd_inhibit(&junk).is_empty());
        assert!(parse_pmset_wakes("é".repeat(30).as_str()).is_empty());
    }
}
