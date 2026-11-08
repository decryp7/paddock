//! Scheduled wake and Wake-on-LAN state through the helper (Dyno ADR 0032 D2, `HELPER_API` 3): the typed arguments, the pure
//! policy (the caps), and the ports the helper's handlers plug the OS into. The helper's first writes, so they are the narrowest
//! possible: only wake events Dyno itself tagged, owner and administrators only, at most [`MAX_WAKES`] pending, never in the past
//! and at most [`MAX_WAKE_AHEAD_SECS`] ahead. Nothing here reads a clock: `now` is passed in.

use crate::ports::PlatformError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The most wake events Dyno has pending at once.
pub const MAX_WAKES: usize = 8;
/// How far ahead a wake may be set: 31 days.
pub const MAX_WAKE_AHEAD_SECS: i64 = 31 * 24 * 3_600;
/// The longest tag.
pub const MAX_TAG: usize = 32;
/// The prefix of every wake event Dyno owns, on macOS (the owner string) and as the Linux unit's name part.
pub const MAC_OWNER_PREFIX: &str = "net.decryptology.dyno.";
/// The prefix of every Linux unit Dyno owns.
pub const LINUX_UNIT_PREFIX: &str = "dyno-wake-";

/// Why a tag or interface name is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameError(pub String);

impl std::fmt::Display for NameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A wake event's tag: `[a-z0-9-]{1,32}`, never starting or ending with `-`. Parsed once at the edge (serde decodes through
/// [`WakeTag::parse`]), so every later use is a safe unit or owner name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WakeTag(String);

impl WakeTag {
    /// A valid tag, or why not.
    pub fn parse(s: &str) -> Result<WakeTag, NameError> {
        let ok = !s.is_empty()
            && s.len() <= MAX_TAG
            && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !s.starts_with('-')
            && !s.ends_with('-');
        if ok {
            Ok(WakeTag(s.to_string()))
        } else {
            Err(NameError(format!(
                "A wake tag is 1 to {MAX_TAG} characters of a-z, 0-9 and '-', not starting or ending with '-'."
            )))
        }
    }

    /// The tag's text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for WakeTag {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for WakeTag {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        WakeTag::parse(&s).map_err(|e| serde::de::Error::custom(e.0))
    }
}

/// A network interface name as Linux allows it (at most 15 bytes, no `/`, no spaces, not `.` or `..`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IfName(String);

impl IfName {
    /// A valid name, or why not.
    pub fn parse(s: &str) -> Result<IfName, NameError> {
        let ok = !s.is_empty()
            && s.len() <= 15
            && s != "."
            && s != ".."
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':' | b'@'))
            && !s.starts_with('-');
        if ok {
            Ok(IfName(s.to_string()))
        } else {
            Err(NameError(
                "An interface name is 1 to 15 characters of letters, digits and . _ - : @, not starting with '-'.".into(),
            ))
        }
    }

    /// The name's text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for IfName {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for IfName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        IfName::parse(&s).map_err(|e| serde::de::Error::custom(e.0))
    }
}

/// One pending wake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeEntry {
    /// Its tag.
    pub tag: WakeTag,
    /// When the computer wakes, seconds since 1970 (UTC).
    pub at_utc: i64,
}

/// The wake-on-LAN modes an interface has switched on (`ethtool`'s letters). Read only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WolMode {
    /// `p`: wake on PHY activity.
    Phy,
    /// `u`: wake on unicast messages.
    Unicast,
    /// `m`: wake on multicast messages.
    Multicast,
    /// `b`: wake on broadcast messages.
    Broadcast,
    /// `a`: wake on ARP.
    Arp,
    /// `g`: wake on a magic packet.
    Magic,
    /// `s`: a magic packet with a password.
    MagicSecure,
}

/// What `WolState` answers: the modes now switched on (empty: disabled), and the modes the interface supports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WolReply {
    /// Switched on now.
    pub enabled: Vec<WolMode>,
    /// Supported by the interface.
    pub supported: Vec<WolMode>,
}

/// Why a wake request is refused before anything runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeRefusal {
    /// The time is not in the future.
    InThePast,
    /// More than [`MAX_WAKE_AHEAD_SECS`] ahead.
    TooFarAhead,
    /// [`MAX_WAKES`] are pending already.
    TooMany,
}

impl WakeRefusal {
    /// The sentence for the wire and the log.
    pub fn message(self) -> String {
        match self {
            WakeRefusal::InThePast => "That time is not in the future.".to_string(),
            WakeRefusal::TooFarAhead => format!("A wake can be set at most {} days ahead.", MAX_WAKE_AHEAD_SECS / 86_400),
            WakeRefusal::TooMany => format!("Dyno already has {MAX_WAKES} wakes pending. Cancel one first."),
        }
    }
}

/// Whether a wake of `tag` at `at_utc` may be set, given what is `pending` (a wake with the same tag is replaced, so it does not
/// count against the cap).
pub fn plan_schedule(pending: &[WakeEntry], tag: &WakeTag, at_utc: i64, now: i64) -> Result<(), WakeRefusal> {
    if at_utc <= now {
        return Err(WakeRefusal::InThePast);
    }
    if at_utc - now > MAX_WAKE_AHEAD_SECS {
        return Err(WakeRefusal::TooFarAhead);
    }
    if pending.iter().filter(|w| &w.tag != tag).count() >= MAX_WAKES {
        return Err(WakeRefusal::TooMany);
    }
    Ok(())
}

/// The helper's side of the wake requests: the OS's wake events, only those Dyno tagged. An implementation never touches an event
/// that does not carry Dyno's prefix.
pub trait WakeScheduler: Send + Sync {
    /// Sets the wake `tag` at `at_utc`, replacing one with the same tag.
    fn schedule(&self, tag: &WakeTag, at_utc: i64) -> Result<(), PlatformError>;
    /// Cancels the wake `tag`; `false` when there was none.
    fn cancel(&self, tag: &WakeTag) -> Result<bool, PlatformError>;
    /// Dyno's pending wakes, soonest first.
    fn list(&self) -> Result<Vec<WakeEntry>, PlatformError>;
}

/// Reads an interface's wake-on-LAN modes (Linux; needs the helper's capabilities).
pub trait WolReader: Send + Sync {
    /// The modes of `ifname`.
    fn read(&self, ifname: &IfName) -> Result<WolReply, PlatformError>;
}

/// What a system without scheduled wake says to every request (Windows schedules in the agent; an AppImage has no helper).
pub struct NoWakes;

impl WakeScheduler for NoWakes {
    fn schedule(&self, _: &WakeTag, _: i64) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported(
            "This system sets scheduled wakes without the helper.".into(),
        ))
    }
    fn cancel(&self, _: &WakeTag) -> Result<bool, PlatformError> {
        Err(PlatformError::Unsupported(
            "This system sets scheduled wakes without the helper.".into(),
        ))
    }
    fn list(&self) -> Result<Vec<WakeEntry>, PlatformError> {
        Err(PlatformError::Unsupported(
            "This system sets scheduled wakes without the helper.".into(),
        ))
    }
}

impl WolReader for NoWakes {
    fn read(&self, _: &IfName) -> Result<WolReply, PlatformError> {
        Err(PlatformError::Unsupported(
            "Wake-on-LAN modes are read through the helper on Linux only.".into(),
        ))
    }
}

/// The seconds since 1970 of a civil UTC time (proleptic Gregorian).
pub fn utc_seconds(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> i64 {
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + h * 3_600 + mi * 60 + s
}

/// The civil UTC time of `secs` since 1970: (year, month, day, hour, minute, second).
pub fn utc_civil(secs: i64) -> (i64, i64, i64, i64, i64, i64) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, rem / 3_600, rem % 3_600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(s: &str) -> WakeTag {
        WakeTag::parse(s).unwrap()
    }

    #[test]
    fn tags_are_lowercase_words_and_nothing_a_shell_or_a_unit_name_could_misread() {
        for good in ["a", "nightly-backup", "wake-1", &"x".repeat(32)] {
            assert!(WakeTag::parse(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "A",
            "a b",
            "a_b",
            "-a",
            "a-",
            "a/b",
            "a.timer",
            "a;b",
            "é",
            &"x".repeat(33),
            "../x",
            "a\n",
        ] {
            assert!(WakeTag::parse(bad).is_err(), "{bad:?}");
        }
        // Decoding applies the same rule: a bad tag on the wire is a decode error, never a value.
        let bytes = postcard::to_stdvec(&"Bad Tag".to_string()).unwrap();
        assert!(postcard::from_bytes::<WakeTag>(&bytes).is_err());
        let good = postcard::to_stdvec(&tag("ok")).unwrap();
        assert_eq!(postcard::from_bytes::<WakeTag>(&good).unwrap(), tag("ok"));
    }

    #[test]
    fn interface_names_are_linux_names() {
        for good in ["eth0", "enp3s0", "wlan0", "br-lan", "veth1@if2", "eth0.100"] {
            assert!(IfName::parse(good).is_ok(), "{good}");
        }
        for bad in ["", ".", "..", "-x", "a b", "eth/0", &"e".repeat(16), "eth0;rm", "eth0\n"] {
            assert!(IfName::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_caps_decide_before_anything_runs() {
        let now = 1_800_000_000;
        let pending: Vec<_> = (0..MAX_WAKES)
            .map(|i| WakeEntry {
                tag: tag(&format!("w{i}")),
                at_utc: now + 100 + i as i64,
            })
            .collect();
        assert_eq!(plan_schedule(&[], &tag("a"), now + 60, now), Ok(()));
        assert_eq!(plan_schedule(&[], &tag("a"), now, now), Err(WakeRefusal::InThePast));
        assert_eq!(plan_schedule(&[], &tag("a"), now - 1, now), Err(WakeRefusal::InThePast));
        assert_eq!(plan_schedule(&[], &tag("a"), now + MAX_WAKE_AHEAD_SECS, now), Ok(()));
        assert_eq!(
            plan_schedule(&[], &tag("a"), now + MAX_WAKE_AHEAD_SECS + 1, now),
            Err(WakeRefusal::TooFarAhead)
        );
        assert_eq!(plan_schedule(&pending, &tag("new"), now + 60, now), Err(WakeRefusal::TooMany));
        assert_eq!(
            plan_schedule(&pending, &tag("w3"), now + 60, now),
            Ok(()),
            "replacing a tag does not add one"
        );
        for r in [WakeRefusal::InThePast, WakeRefusal::TooFarAhead, WakeRefusal::TooMany] {
            assert!(r.message().ends_with('.'));
        }
    }

    #[test]
    fn civil_time_round_trips_at_the_edges() {
        for secs in [0, 86_399, 951_782_400, 1_709_251_199, 1_709_251_200, 1_793_706_000, 4_102_444_800] {
            let (y, mo, d, h, mi, s) = utc_civil(secs);
            assert_eq!(utc_seconds(y, mo, d, h, mi, s), secs, "{secs}");
        }
        assert_eq!(utc_civil(1_709_251_200), (2024, 3, 1, 0, 0, 0));
        assert_eq!(utc_civil(951_782_400), (2000, 2, 29, 0, 0, 0));
    }
}
