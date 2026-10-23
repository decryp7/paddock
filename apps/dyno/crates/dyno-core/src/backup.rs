//! Backup health checks (WP-14): did the backups run, and did they succeed? Dyno only **looks**: it never starts, stops
//! or manages a backup. The agent runs the tools (`tmutil`, `wbadmin`, `wevtutil`, `restic`, `borg`) with timeouts and the
//! user's login-shell `PATH`; everything here is parsing and judgement over their output. "No backup tool found" is a
//! normal state, not an error.

use crate::alarms::feed::{Observation, Source};
use crate::health::Severity;
use crate::issues::days_from_civil;
use crate::time::{Timestamp, DAY, HOUR, MINUTE, SECOND};
use serde::{Deserialize, Serialize};

/// A backup tool Dyno knows how to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackupTool {
    /// macOS Time Machine (`tmutil`).
    TimeMachine,
    /// Windows Backup (`wbadmin`).
    WindowsBackup,
    /// Windows File History (its event log).
    FileHistory,
    /// restic (`restic snapshots`).
    Restic,
    /// BorgBackup (`borg list`).
    Borg,
    /// Duplicati (found, not yet readable).
    Duplicati,
}

impl BackupTool {
    /// Every tool, in the order they are shown.
    pub const ALL: [BackupTool; 6] = [
        BackupTool::TimeMachine,
        BackupTool::WindowsBackup,
        BackupTool::FileHistory,
        BackupTool::Restic,
        BackupTool::Borg,
        BackupTool::Duplicati,
    ];

    /// The name the user knows.
    pub fn name(self) -> &'static str {
        match self {
            BackupTool::TimeMachine => "Time Machine",
            BackupTool::WindowsBackup => "Windows Backup",
            BackupTool::FileHistory => "File History",
            BackupTool::Restic => "restic",
            BackupTool::Borg => "Borg",
            BackupTool::Duplicati => "Duplicati",
        }
    }

    /// The program that is looked up on the `PATH` (none for tools read from the OS's own logs).
    pub fn binary(self) -> Option<&'static str> {
        match self {
            BackupTool::TimeMachine => Some("tmutil"),
            BackupTool::WindowsBackup => Some("wbadmin"),
            BackupTool::FileHistory => None,
            BackupTool::Restic => Some("restic"),
            BackupTool::Borg => Some("borg"),
            BackupTool::Duplicati => Some("duplicati-cli"),
        }
    }

    /// Whether the tool exists on this OS (`macos`, `windows`, `linux` as `std::env::consts::OS` says).
    pub fn on_os(self, os: &str) -> bool {
        match self {
            BackupTool::TimeMachine => os == "macos",
            BackupTool::WindowsBackup | BackupTool::FileHistory => os == "windows",
            BackupTool::Restic | BackupTool::Borg | BackupTool::Duplicati => true,
        }
    }
}

/// How a tool's backups look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackupState {
    /// The last backup is recent.
    Healthy,
    /// The last backup is older than the limit.
    Stale,
    /// The tool is set up but has no backup yet.
    NoBackupYet,
    /// The tool is installed but nothing is set up to back up.
    NotConfigured,
    /// Dyno could not read the tool's state (reason in the detail).
    CannotCheck,
}

/// One tool's status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatus {
    /// The tool.
    pub tool: BackupTool,
    /// Its name.
    pub name: String,
    /// The verdict.
    pub state: BackupState,
    /// When the last successful backup finished, when known.
    pub last_backup: Option<Timestamp>,
    /// One plain sentence.
    pub summary: String,
    /// What to do next, when there is something.
    pub next_step: Option<String>,
}

/// The Backup health view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BackupReport {
    /// The tools that were found and read.
    pub statuses: Vec<BackupStatus>,
    /// When it was checked (none yet: the first check is still running).
    pub checked_at: Option<Timestamp>,
    /// A backup check is running now.
    pub checking: bool,
    /// The `PATH` came from the user's login shell (false: the process's own, or an override).
    pub login_path_used: bool,
}

/// The settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupSettings {
    /// Check in the background (every 6 hours, inside the maintenance windows) and raise an alert for a stale backup.
    pub alert_when_stale: bool,
    /// A backup older than this many hours is stale (1 to 2160).
    pub stale_after_hours: u32,
    /// Per-tool folders to look in before the `PATH`, for tools installed somewhere unusual.
    pub overrides: Vec<ToolOverride>,
}

impl Default for BackupSettings {
    fn default() -> Self {
        BackupSettings {
            alert_when_stale: true,
            stale_after_hours: 48,
            overrides: Vec::new(),
        }
    }
}

/// Folders to look in for one tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOverride {
    /// The tool.
    pub tool: BackupTool,
    /// Absolute folders, searched first.
    pub dirs: Vec<String>,
}

impl BackupSettings {
    /// Checks the setting; the agent refuses to save one that fails.
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=2160).contains(&self.stale_after_hours) {
            return Err("A backup counts as stale after 1 hour to 90 days (2160 hours).".into());
        }
        if self.overrides.len() > BackupTool::ALL.len() {
            return Err("One override per backup tool.".into());
        }
        for o in &self.overrides {
            crate::toolpath::validate_dirs(&o.dirs).map_err(|e| format!("{}: {e}", o.tool.name()))?;
        }
        Ok(())
    }

    /// The override folders for `tool`.
    pub fn dirs_for(&self, tool: BackupTool) -> &[String] {
        self.overrides.iter().find(|o| o.tool == tool).map_or(&[], |o| o.dirs.as_slice())
    }
}

/// Parses an RFC 3339 time: `2026-10-23T02:00:01.123456789+08:00`, `...Z`, or `...` without a zone (local time at
/// `default_offset_minutes`, which is what Borg prints).
pub fn parse_rfc3339(text: &str, default_offset_minutes: i32) -> Option<Timestamp> {
    let t = text.trim();
    if t.len() < 19 || !t.is_char_boundary(19) || t.as_bytes()[10] != b'T' && t.as_bytes()[10] != b' ' {
        return None;
    }
    let n = |a: usize, z: usize| t.get(a..z)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, s) = (n(0, 4)?, n(5, 7)?, n(8, 10)?, n(11, 13)?, n(14, 16)?, n(17, 19)?);
    if t.as_bytes()[4] != b'-' || t.as_bytes()[7] != b'-' || t.as_bytes()[13] != b':' || t.as_bytes()[16] != b':' {
        return None;
    }
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    let mut rest = &t[19..];
    let mut ms = 0;
    if let Some(frac) = rest.strip_prefix('.') {
        let digits: String = frac.chars().take_while(char::is_ascii_digit).collect();
        ms = format!("{digits:0<3}")[..3].parse::<i64>().ok()?;
        rest = &frac[digits.len()..];
    }
    let offset = match rest {
        "" => i64::from(default_offset_minutes),
        "Z" | "z" => 0,
        r => {
            let sign = match r.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let z: String = r[1..].chars().filter(|c| *c != ':').collect();
            if z.len() != 4 || !z.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            sign * (z[..2].parse::<i64>().ok()? * 60 + z[2..].parse::<i64>().ok()?)
        }
    };
    Some(Timestamp(
        days_from_civil(y, mo as u32, d as u32) * DAY + h * HOUR + mi * MINUTE + s * SECOND + ms - offset * MINUTE,
    ))
}

/// A backup folder name such as `2026-10-23-151501.backup` (also inside a longer path) as a local time.
pub fn parse_tmutil_backup_name(text: &str, utc_offset_minutes: i32) -> Option<Timestamp> {
    let name = text.trim().rsplit('/').next()?;
    let stamp = name.split(['.']).next()?;
    let b = stamp.as_bytes();
    if b.len() != 17 || b[4] != b'-' || b[7] != b'-' || b[10] != b'-' {
        return None;
    }
    let iso = format!(
        "{}T{}:{}:{}",
        &stamp[..10],
        stamp.get(11..13)?,
        stamp.get(13..15)?,
        stamp.get(15..17)?
    );
    parse_rfc3339(&iso, utc_offset_minutes)
}

/// How a `tmutil destinationinfo` answer reads: any destination configured?
pub fn tmutil_has_destination(text: &str) -> bool {
    !text.contains("No destinations configured") && text.lines().any(|l| l.trim_start().starts_with("Name"))
}

/// Time Machine's status from `tmutil destinationinfo` and `tmutil latestbackup`.
pub fn time_machine_status(
    destinations: Result<&str, &str>,
    latest: Result<&str, &str>,
    now: Timestamp,
    utc_offset_minutes: i32,
    stale_after_hours: u32,
) -> BackupStatus {
    let tool = BackupTool::TimeMachine;
    match destinations {
        Ok(d) if !tmutil_has_destination(d) => {
            return not_configured(
                tool,
                "Time Machine has no backup disk set up.",
                "Open System Settings > General > Time Machine and choose a disk.",
            )
        }
        Err(e) => return cannot_check(tool, e),
        Ok(_) => {}
    }
    match latest {
        Ok(path) => match parse_tmutil_backup_name(path, utc_offset_minutes) {
            Some(at) => judged(tool, at, now, stale_after_hours),
            None => cannot_check(tool, "Time Machine's answer was not recognised."),
        },
        Err(e) if e.contains("Full Disk Access") => BackupStatus {
            next_step: Some("Give Dyno's agent Full Disk Access in System Settings > Privacy & Security, or read the state in Time Machine's own settings.".into()),
            ..cannot_check(tool, "macOS does not let Dyno read the latest backup without Full Disk Access.")
        },
        Err(e) if e.contains("No machine directory") || e.contains("No backups") => no_backup(tool, "Time Machine has not made a backup of this Mac yet."),
        Err(_) => BackupStatus {
            next_step: Some("Connect the backup disk (or the network share) and check again.".into()),
            ..no_backup(tool, "Time Machine's backup disk is not connected, so the latest backup cannot be read.")
        },
    }
}

/// `wbadmin get versions`: `Version identifier: 10/23/2026-02:00` lines (the identifier is UTC). The newest.
pub fn parse_wbadmin_latest(text: &str) -> Option<Timestamp> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("Version identifier:"))
        .filter_map(|id| {
            let id = id.trim();
            let (date, time) = id.split_once('-')?;
            let mut d = date.split('/');
            let (mo, day, y) = (
                d.next()?.parse::<u32>().ok()?,
                d.next()?.parse::<u32>().ok()?,
                d.next()?.parse::<i64>().ok()?,
            );
            let (hh, mm) = time.split_once(':')?;
            let (hh, mm) = (hh.parse::<i64>().ok()?, mm.get(..2)?.parse::<i64>().ok()?);
            if !(1..=12).contains(&mo) || !(1..=31).contains(&day) || hh > 23 || mm > 59 {
                return None;
            }
            Some(Timestamp(days_from_civil(y, mo, day) * DAY + hh * HOUR + mm * MINUTE))
        })
        .max()
}

/// The newest `SystemTime='...'` of an event log query (`wevtutil qe ... /f:xml`), and whether the newest event is an
/// error (Level 1 or 2).
pub fn parse_event_log_latest(xml: &str) -> Option<(Timestamp, bool)> {
    let mut best: Option<(Timestamp, bool)> = None;
    for ev in xml.split("<Event ").skip(1).take(64) {
        let time = ev
            .split("SystemTime='")
            .nth(1)
            .and_then(|r| r.split('\'').next())
            .and_then(|t| parse_rfc3339(t, 0));
        let level = ev
            .split("<Level>")
            .nth(1)
            .and_then(|r| r.split('<').next())
            .and_then(|l| l.trim().parse::<u8>().ok());
        if let Some(t) = time {
            if best.is_none_or(|(b, _)| t > b) {
                best = Some((t, matches!(level, Some(1 | 2))));
            }
        }
    }
    best
}

/// `restic snapshots --json --latest 1`: the newest snapshot's `time`.
pub fn parse_restic_latest(json: &str) -> Option<Timestamp> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.as_array()?
        .iter()
        .filter_map(|s| parse_rfc3339(s.get("time")?.as_str()?, 0))
        .max()
}

/// `borg list --last 1 --json`: the newest archive's `time` (Borg prints local time without a zone).
pub fn parse_borg_latest(json: &str, utc_offset_minutes: i32) -> Option<Timestamp> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.get("archives")?
        .as_array()?
        .iter()
        .filter_map(|a| parse_rfc3339(a.get("time")?.as_str()?, utc_offset_minutes))
        .max()
}

/// A status for a tool whose last backup finished at `at`: healthy, or stale past the limit.
pub fn judged(tool: BackupTool, at: Timestamp, now: Timestamp, stale_after_hours: u32) -> BackupStatus {
    let age = now.since(at).max(0);
    let state = if age > i64::from(stale_after_hours) * HOUR {
        BackupState::Stale
    } else {
        BackupState::Healthy
    };
    let ago = age_sentence(age);
    BackupStatus {
        tool,
        name: tool.name().into(),
        state,
        last_backup: Some(at),
        summary: match state {
            BackupState::Stale => format!(
                "The last {} backup finished {ago}, longer than the {} you allow.",
                tool.name(),
                age_sentence_limit(stale_after_hours)
            ),
            _ => format!("The last {} backup finished {ago}.", tool.name()),
        },
        next_step: (state == BackupState::Stale).then(|| format!("Run a backup in {} and check that it finishes.", tool.name())),
    }
}

fn age_sentence_limit(hours: u32) -> String {
    if hours % 24 == 0 {
        let d = hours / 24;
        format!("{d} day{}", if d == 1 { "" } else { "s" })
    } else {
        format!("{hours} hour{}", if hours == 1 { "" } else { "s" })
    }
}

/// "3 hours ago", "2 days ago", "just now".
pub fn age_sentence(age_ms: i64) -> String {
    let (h, d) = (age_ms / HOUR, age_ms / DAY);
    if age_ms < 10 * MINUTE {
        "just now".into()
    } else if h < 1 {
        format!("{} minutes ago", age_ms / MINUTE)
    } else if h < 48 {
        format!("{h} hour{} ago", if h == 1 { "" } else { "s" })
    } else {
        format!("{d} days ago")
    }
}

/// A tool that is installed but not set up.
pub fn not_configured(tool: BackupTool, summary: &str, next: &str) -> BackupStatus {
    BackupStatus {
        tool,
        name: tool.name().into(),
        state: BackupState::NotConfigured,
        last_backup: None,
        summary: summary.into(),
        next_step: Some(next.into()),
    }
}

/// A tool with no backup yet.
pub fn no_backup(tool: BackupTool, summary: &str) -> BackupStatus {
    BackupStatus {
        tool,
        name: tool.name().into(),
        state: BackupState::NoBackupYet,
        last_backup: None,
        summary: summary.into(),
        next_step: Some(format!("Run a backup in {}.", tool.name())),
    }
}

/// A tool Dyno could not read.
pub fn cannot_check(tool: BackupTool, why: &str) -> BackupStatus {
    BackupStatus {
        tool,
        name: tool.name().into(),
        state: BackupState::CannotCheck,
        last_backup: None,
        summary: why.chars().take(300).collect(),
        next_step: None,
    }
}

/// The alarm for a status that needs attention (a stale backup or none at all), or `None` when it is fine. One alarm per
/// tool, cleared (`active: false`) when the backup is healthy again.
pub fn observation(status: &BackupStatus) -> Option<Observation> {
    let needs = matches!(status.state, BackupState::Stale | BackupState::NoBackupYet);
    let healthy = status.state == BackupState::Healthy;
    if !needs && !healthy {
        return None;
    }
    Some(Observation {
        source: Source::Alert,
        name: "backup".into(),
        subject: status.name.clone(),
        severity: Severity::Warning,
        title: match status.state {
            BackupState::NoBackupYet => format!("{} has no backup yet", status.name),
            _ => format!("{} backup is out of date", status.name),
        },
        detail: status.summary.clone(),
        value: None,
        metric: None,
        active: needs,
        one_off: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-10-23T08:00:00Z
    const NOW: Timestamp = Timestamp(1_792_742_400_000);

    #[test]
    fn rfc3339_forms() {
        let z = parse_rfc3339("2026-10-23T08:00:00Z", 0).unwrap();
        assert_eq!(z, NOW);
        assert_eq!(
            parse_rfc3339("2026-10-23T16:00:00.123456789+08:00", 0).unwrap(),
            Timestamp(NOW.0 + 123)
        );
        // No zone: local time at the default offset (+08:00 means 8 hours earlier in UTC).
        assert_eq!(parse_rfc3339("2026-10-23T16:00:00", 480).unwrap(), NOW);
        assert_eq!(parse_rfc3339("2026-13-23T16:00:00", 0), None);
        assert_eq!(parse_rfc3339("garbage", 0), None);
        assert_eq!(parse_rfc3339("2026-10-23T16:00:00+8", 0), None);
        assert_eq!(parse_rfc3339("é".repeat(12).as_str(), 0), None);
    }

    #[test]
    fn time_machine_names() {
        let at = parse_tmutil_backup_name("/Volumes/.timemachine/ABC/2026-10-23-160000.backup/2026-10-23-160000.backup\n", 480).unwrap();
        assert_eq!(at, NOW);
        assert_eq!(parse_tmutil_backup_name("2026-10-23.backup", 0), None);
        assert_eq!(parse_tmutil_backup_name("", 0), None);
    }

    const DEST: &str = "====================================================\nName          : TimeMachine\nKind          : Network\n";

    #[test]
    fn time_machine_verdicts() {
        let now = NOW.plus(5 * HOUR);
        let ok = time_machine_status(Ok(DEST), Ok("/x/2026-10-23-160000.backup"), now, 480, 48);
        assert_eq!(ok.state, BackupState::Healthy);
        assert!(ok.summary.contains("5 hours ago"));
        let stale = time_machine_status(Ok(DEST), Ok("/x/2026-10-23-160000.backup"), now.plus(5 * DAY), 480, 48);
        assert_eq!(stale.state, BackupState::Stale);
        assert!(stale.summary.contains("2 days you allow"));
        assert!(stale.next_step.is_some());
        let none = time_machine_status(Ok("No destinations configured.\n"), Ok(""), now, 0, 48);
        assert_eq!(none.state, BackupState::NotConfigured);
        let fda = time_machine_status(
            Ok(DEST),
            Err("tmutil: latestbackup requires Full Disk Access privileges."),
            now,
            0,
            48,
        );
        assert_eq!(fda.state, BackupState::CannotCheck);
        assert!(fda.next_step.unwrap().contains("Full Disk Access"));
        let first = time_machine_status(Ok(DEST), Err("tmutil: No machine directory found for host."), now, 0, 48);
        assert_eq!(first.state, BackupState::NoBackupYet);
        let off = time_machine_status(Ok(DEST), Err("tmutil: Failed to mount"), now, 0, 48);
        assert_eq!(off.state, BackupState::NoBackupYet);
        assert!(off.summary.contains("not connected"));
        assert_eq!(
            time_machine_status(Err("timed out"), Ok(""), now, 0, 48).state,
            BackupState::CannotCheck
        );
        assert_eq!(
            time_machine_status(Ok(DEST), Ok("nonsense"), now, 0, 48).state,
            BackupState::CannotCheck
        );
    }

    #[test]
    fn windows_backup_versions_and_event_logs() {
        let t = "Backup time: 10/22/2026 2:00 AM\nVersion identifier: 10/22/2026-18:00\nCan recover: Volume(s), File(s)\n\nBackup time: 10/23/2026 2:00 AM\nVersion identifier: 10/23/2026-00:00\n";
        assert_eq!(parse_wbadmin_latest(t).unwrap(), Timestamp(NOW.0 - 8 * HOUR));
        assert_eq!(parse_wbadmin_latest("Version identifier: 13/40/2026-00:00"), None);
        assert_eq!(parse_wbadmin_latest(""), None);
        let xml = "<Event xmlns='x'><System><Level>4</Level><TimeCreated SystemTime='2026-10-23T01:00:00.000000000Z'/></System></Event><Event xmlns='x'><System><Level>2</Level><TimeCreated SystemTime='2026-10-23T07:00:00.000000000Z'/></System></Event>";
        let (at, error) = parse_event_log_latest(xml).unwrap();
        assert_eq!(at, Timestamp(NOW.0 - HOUR));
        assert!(error);
        assert_eq!(parse_event_log_latest("none"), None);
    }

    #[test]
    fn restic_and_borg() {
        let r = r#"[{"time":"2026-10-23T16:00:00.5+08:00","hostname":"h"}]"#;
        assert_eq!(parse_restic_latest(r).unwrap(), Timestamp(NOW.0 + 500));
        assert_eq!(parse_restic_latest("[]"), None);
        assert_eq!(parse_restic_latest("Fatal: repository does not exist"), None);
        let b = r#"{"archives":[{"time":"2026-10-23T16:00:00.000000","name":"a"},{"time":"2026-10-22T16:00:00.000000"}]}"#;
        assert_eq!(parse_borg_latest(b, 480).unwrap(), NOW);
        assert_eq!(parse_borg_latest("{}", 0), None);
    }

    #[test]
    fn judgement_and_sentences() {
        assert_eq!(age_sentence(MINUTE), "just now");
        assert_eq!(age_sentence(30 * MINUTE), "30 minutes ago");
        assert_eq!(age_sentence(HOUR), "1 hour ago");
        assert_eq!(age_sentence(3 * DAY), "3 days ago");
        assert_eq!(age_sentence_limit(1), "1 hour");
        assert_eq!(age_sentence_limit(24), "1 day");
        let s = judged(BackupTool::Restic, NOW, NOW.plus(DAY), 12);
        assert_eq!(s.state, BackupState::Stale);
        assert!(s.summary.contains("12 hours"));
        // A time in the future (clock skew) is healthy, not negative.
        assert_eq!(judged(BackupTool::Restic, NOW.plus(HOUR), NOW, 12).state, BackupState::Healthy);
    }

    #[test]
    fn alarms_only_for_stale_or_missing_backups() {
        let stale = judged(BackupTool::Borg, NOW, NOW.plus(10 * DAY), 24);
        let o = observation(&stale).unwrap();
        assert!(o.active && o.title.contains("out of date") && o.subject == "Borg");
        let fresh = judged(BackupTool::Borg, NOW, NOW, 24);
        assert!(!observation(&fresh).unwrap().active);
        assert!(observation(&no_backup(BackupTool::Restic, "x"))
            .unwrap()
            .title
            .contains("no backup yet"));
        assert!(observation(&cannot_check(BackupTool::Restic, "x")).is_none());
        assert!(observation(&not_configured(BackupTool::Restic, "x", "y")).is_none());
    }

    #[test]
    fn tools_and_settings() {
        assert!(BackupTool::TimeMachine.on_os("macos") && !BackupTool::TimeMachine.on_os("linux"));
        assert!(BackupTool::Restic.on_os("linux"));
        assert_eq!(BackupTool::FileHistory.binary(), None);
        let mut s = BackupSettings::default();
        assert!(s.validate().is_ok());
        s.stale_after_hours = 0;
        assert!(s.validate().is_err());
        s.stale_after_hours = 24;
        s.overrides = vec![ToolOverride {
            tool: BackupTool::Restic,
            dirs: vec!["relative/dir".into()],
        }];
        assert!(s.validate().unwrap_err().starts_with("restic:"));
        s.overrides[0].dirs = vec!["/opt/restic/bin".into()];
        assert!(s.validate().is_ok());
        assert_eq!(s.dirs_for(BackupTool::Restic).len(), 1);
        assert!(s.dirs_for(BackupTool::Borg).is_empty());
        assert_eq!(serde_json::from_str::<BackupSettings>("{}").unwrap(), BackupSettings::default());
    }
}
