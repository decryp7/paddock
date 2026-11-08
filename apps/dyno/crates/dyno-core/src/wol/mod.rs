//! Wake-on-LAN (ADR 0032 section 3, decisions D6 and D8). The pure side: the hardware address and magic packet, the neighbour
//! tables' parsers, where the packets go, the wake-and-wait run over ports, this PC's readiness, the saved devices and their
//! validation. The sockets, the tables and the echo are ports the platform implements; the agent runs a wake as a job.
//!
//! Devices are added only by the user: picked from the neighbour table or typed by hand. Nothing is stored without a click.
//! Relaying through another Dyno is not built (D8).

#[cfg(any(test, feature = "test-support"))]
pub mod fake;
pub mod mac;
pub mod neighbours;
pub mod packet;
pub mod plan;
pub mod readiness;
pub mod wake;

use crate::power::Capability;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

pub use mac::{Mac, MacError, SecureOn};
pub use neighbours::{Neighbour, NeighbourState};
pub use plan::{NetIf, SendPlan};
pub use readiness::{ItemState, ReadinessItem, ReadinessKind, ThisPcWol};
pub use wake::{Answer, AnswerHow, WolOutcome};

/// The most saved devices.
pub const MAX_DEVICES: usize = 64;
/// The most candidates one "Add from network" lists.
pub const MAX_CANDIDATES: usize = 256;
/// The longest device name, in characters.
pub const MAX_NAME_CHARS: usize = 64;

/// The clock a wake runs on: a monotonic reading and a sleep, so a test drives time without waiting (ER-12).
pub trait WolClock: Send + Sync {
    /// The concrete adapter behind this port (for the wiring test and the agent's start log). Nobody overrides it.
    fn adapter(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
    /// Time since an arbitrary origin; never goes backwards.
    fn now(&self) -> std::time::Duration;
    /// Waits `d` (a fake only advances its own time).
    fn sleep(&self, d: std::time::Duration);
}

/// The network cards of this computer (IPv4 only: a magic packet is an IPv4 broadcast).
pub trait Interfaces: Send + Sync {
    /// The concrete adapter behind this port (for the wiring test and the agent's start log). Nobody overrides it.
    fn adapter(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
    /// Each card's addresses; empty when none is connected.
    fn ipv4(&self) -> Vec<NetIf>;
}

/// This PC's wake settings, read from the OS.
pub trait ReadinessSource: Send + Sync {
    /// The concrete adapter behind this port (for the wiring test and the agent's start log). Nobody overrides it.
    fn adapter(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
    /// Reads them now; what cannot be read is a "can't tell" item, never an error.
    fn read(&self) -> ThisPcWol;
}

/// How the last wake of a device ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastWake {
    /// When.
    pub at: Timestamp,
    /// It answered (a test), or was sent without error (a plain wake).
    pub ok: bool,
    /// A test wake rather than a plain one.
    pub tested: bool,
    /// The result in words.
    pub summary: String,
}

/// A saved computer to wake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WolDevice {
    /// Its id (assigned by the agent).
    pub id: u64,
    /// The name the person gave it.
    pub name: String,
    /// Its hardware address.
    pub mac: Mac,
    /// The address it had last time.
    pub last_ip: Option<String>,
    /// The network card it is reached through, when known.
    pub interface: Option<String>,
    /// The UDP port.
    pub port: u16,
    /// Whether a SecureOn password is stored for it (the password itself is in the credential store).
    pub secure_on: bool,
    /// How its last wake ended.
    pub last_wake: Option<LastWake>,
}

impl Default for WolDevice {
    fn default() -> Self {
        WolDevice {
            id: 0,
            name: String::new(),
            mac: Mac([2, 0, 0, 0, 0, 0]),
            last_ip: None,
            interface: None,
            port: packet::WOL_PORT,
            secure_on: false,
            last_wake: None,
        }
    }
}

/// What to do with the SecureOn password when saving.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SecureOnEdit {
    /// Leave it as it is.
    Keep,
    /// Remove it.
    Clear,
    /// Store this one (4 or 6 bytes of hex).
    Set {
        /// The password.
        password: String,
    },
}

impl std::fmt::Debug for SecureOnEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecureOnEdit::Keep => f.write_str("Keep"),
            SecureOnEdit::Clear => f.write_str("Clear"),
            SecureOnEdit::Set { .. } => f.write_str("Set(..)"),
        }
    }
}

/// What the person typed or picked when adding or changing a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WolDeviceInput {
    /// The id of the device being changed; none when adding.
    pub id: Option<u64>,
    /// The name.
    pub name: String,
    /// The hardware address, as typed.
    pub mac: String,
    /// The address, when known.
    pub last_ip: Option<String>,
    /// The card, when known.
    pub interface: Option<String>,
    /// What to do with the password.
    pub secure_on: SecureOnEdit,
}

/// An input that passed every check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidDevice {
    /// The id when changing.
    pub id: Option<u64>,
    /// The trimmed name.
    pub name: String,
    /// The address.
    pub mac: Mac,
    /// The IP, normalised.
    pub last_ip: Option<String>,
    /// The card.
    pub interface: Option<String>,
    /// The password change, parsed.
    pub secure_on: SecureChange,
}

/// A parsed password change.
#[derive(Clone, PartialEq, Eq)]
pub enum SecureChange {
    /// Leave it.
    Keep,
    /// Remove it.
    Clear,
    /// Store it.
    Set(SecureOn),
}

impl std::fmt::Debug for SecureChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SecureChange::Keep => "Keep",
            SecureChange::Clear => "Clear",
            SecureChange::Set(_) => "Set(..)",
        })
    }
}

/// Checks what the UI sent against `saved` (the agent re-validates whatever the webview says). The message says what to fix.
pub fn validate_input(input: &WolDeviceInput, saved: &[WolDevice]) -> Result<ValidDevice, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("Give the computer a name.".into());
    }
    if name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err(format!(
            "A name is at most {MAX_NAME_CHARS} characters, with no control characters."
        ));
    }
    let mac = Mac::parse(&input.mac).map_err(|e| e.to_string())?;
    if let Some(other) = saved.iter().find(|d| d.mac == mac && Some(d.id) != input.id) {
        return Err(format!("That hardware address is already saved as \"{}\".", other.name));
    }
    match input.id {
        Some(id) if !saved.iter().any(|d| d.id == id) => return Err("That computer is no longer in the list.".into()),
        None if saved.len() >= MAX_DEVICES => return Err(format!("At most {MAX_DEVICES} computers can be saved; remove one first.")),
        _ => {}
    }
    let last_ip = match input.last_ip.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(ip) => Some(
            ip.parse::<std::net::IpAddr>()
                .map_err(|_| "The address is not an IP address, like 192.168.1.20.".to_string())?
                .to_string(),
        ),
        None => None,
    };
    let interface = input
        .interface
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(64).collect::<String>());
    let secure_on = match &input.secure_on {
        SecureOnEdit::Keep => SecureChange::Keep,
        SecureOnEdit::Clear => SecureChange::Clear,
        SecureOnEdit::Set { password } => SecureChange::Set(SecureOn::parse(password).map_err(|e| e.to_string())?),
    };
    Ok(ValidDevice {
        id: input.id,
        name: name.to_string(),
        mac,
        last_ip,
        interface,
        secure_on,
    })
}

/// The entries "Add from network" offers: one per hardware address not yet saved, an IPv4 address preferred over IPv6 and a
/// fresher state over an older one, sorted by address.
pub fn candidates(all: Vec<Neighbour>, saved: &[WolDevice]) -> Vec<Neighbour> {
    use std::collections::HashMap;
    let rank = |n: &Neighbour| (n.ip.contains(':') as u8, matches!(n.state, NeighbourState::Reachable) as u8 ^ 1);
    let mut best: HashMap<Mac, Neighbour> = HashMap::new();
    for n in all {
        if saved.iter().any(|d| d.mac == n.mac) {
            continue;
        }
        match best.get(&n.mac) {
            Some(cur) if rank(cur) <= rank(&n) => {}
            _ => {
                best.insert(n.mac, n);
            }
        }
    }
    let mut out: Vec<Neighbour> = best.into_values().collect();
    let key = |n: &Neighbour| (n.ip.contains(':'), n.ip.parse::<std::net::IpAddr>().ok(), n.mac);
    out.sort_by_key(key);
    out.truncate(MAX_CANDIDATES);
    out
}

/// What the Wake devices tab shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WolView {
    /// The `power.wol.enabled` setting.
    pub enabled: bool,
    /// The saved computers, by name.
    pub devices: Vec<WolDevice>,
    /// Whether this computer can send a wake at all (a network card is connected).
    pub can_send: Capability,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: u64, name: &str, mac: [u8; 6]) -> WolDevice {
        WolDevice {
            id,
            name: name.into(),
            mac: Mac(mac),
            ..WolDevice::default()
        }
    }

    fn input(name: &str, mac: &str) -> WolDeviceInput {
        WolDeviceInput {
            id: None,
            name: name.into(),
            mac: mac.into(),
            last_ip: None,
            interface: None,
            secure_on: SecureOnEdit::Keep,
        }
    }

    #[test]
    fn a_good_input_becomes_typed_values() {
        let mut i = input("  Test box ", "02:00:5E:10:00:01");
        i.last_ip = Some(" 192.0.2.40 ".into());
        i.interface = Some("en0".into());
        i.secure_on = SecureOnEdit::Set {
            password: "01:02:03:04".into(),
        };
        let v = validate_input(&i, &[]).unwrap();
        assert_eq!(v.name, "Test box");
        assert_eq!(v.mac.to_string(), "02:00:5e:10:00:01");
        assert_eq!(v.last_ip.as_deref(), Some("192.0.2.40"));
        assert!(matches!(v.secure_on, SecureChange::Set(_)));
        assert_eq!(format!("{:?}", v.secure_on), "Set(..)");
        assert_eq!(format!("{:?}", i.secure_on), "Set(..)");
    }

    #[test]
    fn bad_inputs_say_what_to_fix() {
        let saved = [dev(1, "Desk", [2, 0, 0x5e, 0x10, 0, 1])];
        let msg = |i: &WolDeviceInput| validate_input(i, &saved).unwrap_err();
        assert!(msg(&input("", "02:00:5e:10:00:09")).contains("name"));
        assert!(msg(&input(&"n".repeat(65), "02:00:5e:10:00:09")).contains("at most 64"));
        assert!(msg(&input("a\u{7}b", "02:00:5e:10:00:09")).contains("control"));
        assert!(msg(&input("x", "nope")).contains("six pairs"));
        assert!(msg(&input("x", "02:00:5e:10:00:01")).contains("already saved as \"Desk\""));
        let mut ip = input("x", "02:00:5e:10:00:09");
        ip.last_ip = Some("not an ip".into());
        assert!(msg(&ip).contains("not an IP address"));
        let mut pw = input("x", "02:00:5e:10:00:09");
        pw.secure_on = SecureOnEdit::Set { password: "12".into() };
        assert!(msg(&pw).contains("SecureOn"));
        let mut gone = input("x", "02:00:5e:10:00:09");
        gone.id = Some(9);
        assert!(msg(&gone).contains("no longer"));
    }

    #[test]
    fn changing_a_device_may_keep_its_own_address_and_the_list_is_capped() {
        let saved = [dev(1, "Desk", [2, 0, 0x5e, 0x10, 0, 1])];
        let mut i = input("Desk 2", "02:00:5e:10:00:01");
        i.id = Some(1);
        assert!(validate_input(&i, &saved).is_ok());
        let full: Vec<WolDevice> = (0..MAX_DEVICES as u64)
            .map(|n| dev(n + 1, "d", [2, 0, 0, 0, 0, n as u8 + 1]))
            .collect();
        assert!(validate_input(&input("x", "02:00:5e:10:00:ff"), &full)
            .unwrap_err()
            .contains("At most 64"));
    }

    fn nb(mac: [u8; 6], ip: &str, state: NeighbourState) -> Neighbour {
        Neighbour {
            name: None,
            mac: Mac(mac),
            ip: ip.into(),
            interface: "en0".into(),
            state,
        }
    }

    #[test]
    fn candidates_are_unsaved_one_per_address_ipv4_first_and_sorted() {
        let saved = [dev(1, "Desk", [2, 0, 0x5e, 0x10, 0, 1])];
        let all = vec![
            nb([2, 0, 0x5e, 0x10, 0, 1], "192.0.2.1", NeighbourState::Reachable),
            nb([2, 0, 0x5e, 0x10, 0, 3], "2001:db8::3", NeighbourState::Reachable),
            nb([2, 0, 0x5e, 0x10, 0, 3], "192.0.2.30", NeighbourState::Stale),
            nb([2, 0, 0x5e, 0x10, 0, 2], "192.0.2.9", NeighbourState::Stale),
            nb([2, 0, 0x5e, 0x10, 0, 2], "192.0.2.9", NeighbourState::Reachable),
            nb([2, 0, 0x5e, 0x10, 0, 4], "192.0.2.10", NeighbourState::Unknown),
        ];
        let c = candidates(all, &saved);
        let ips: Vec<&str> = c.iter().map(|n| n.ip.as_str()).collect();
        assert_eq!(ips, ["192.0.2.9", "192.0.2.10", "192.0.2.30"]);
        assert_eq!(c[0].state, NeighbourState::Reachable);
    }

    #[test]
    fn a_device_stored_with_missing_keys_takes_defaults() {
        let d: WolDevice = serde_json::from_str(r#"{"id":3,"name":"a","mac":"02:00:5e:10:00:01"}"#).unwrap();
        assert_eq!((d.port, d.secure_on, d.last_ip), (9, false, None));
    }
}
