//! The tray's Wake submenu (ADR 0032 section 13), as pure functions: what the submenu offers for the agent's saved computers, and
//! what a chosen entry asks for. The tray draws the entries ([`crate::tray`]); the shell sends the request.

use dyno_core::power::Capability;
use dyno_core::wol::WolView;

/// Entry ids; the shell's tray hands a chosen id back to [`action_of`].
pub const PREFIX: &str = "dy.wol.";
const MANAGE: &str = "dy.wol.manage";

/// The most computers the submenu lists (the rest are on the Power page).
pub const MAX_ENTRIES: usize = 10;

/// One entry of the submenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The id handed back when chosen.
    pub id: String,
    /// The words.
    pub text: String,
    /// Whether it can be chosen.
    pub enabled: bool,
}

/// The submenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    /// The computers, then "Manage devices…".
    pub entries: Vec<Entry>,
}

/// A chosen entry's request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Wake this saved computer.
    Wake(u64),
    /// Open the Power page's Wake devices.
    Manage,
}

/// The menu for `view`: none when Wake-on-LAN is off or nothing is saved (the Power page is where one starts).
pub fn menu(view: &WolView) -> Option<Menu> {
    if !view.enabled || view.devices.is_empty() {
        return None;
    }
    let usable = matches!(view.can_send, Capability::Known);
    let mut entries: Vec<Entry> = view
        .devices
        .iter()
        .take(MAX_ENTRIES)
        .map(|d| Entry {
            id: format!("{PREFIX}{}", d.id),
            text: d.name.clone(),
            enabled: usable,
        })
        .collect();
    entries.push(Entry {
        id: MANAGE.into(),
        text: "Manage devices…".into(),
        enabled: true,
    });
    Some(Menu { entries })
}

/// What a chosen id asks for; `None` for an id that is not ours.
pub fn action_of(id: &str) -> Option<Action> {
    if id == MANAGE {
        return Some(Action::Manage);
    }
    id.strip_prefix(PREFIX)?.parse().ok().map(Action::Wake)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::wol::{Mac, WolDevice};

    fn dev(id: u64, name: &str) -> WolDevice {
        WolDevice {
            id,
            name: name.into(),
            mac: Mac([2, 0, 0x5e, 0x10, 0, id as u8]),
            ..WolDevice::default()
        }
    }

    fn view(devices: Vec<WolDevice>) -> WolView {
        WolView {
            enabled: true,
            devices,
            can_send: Capability::Known,
        }
    }

    #[test]
    fn lists_the_computers_then_manage() {
        let m = menu(&view(vec![dev(1, "Media PC"), dev(2, "NAS")])).unwrap();
        let texts: Vec<&str> = m.entries.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["Media PC", "NAS", "Manage devices…"]);
        assert!(m.entries.iter().all(|e| e.enabled));
    }

    #[test]
    fn has_no_menu_when_off_or_empty() {
        assert_eq!(menu(&view(vec![])), None);
        let mut v = view(vec![dev(1, "x")]);
        v.enabled = false;
        assert_eq!(menu(&v), None);
    }

    #[test]
    fn a_computer_cannot_be_woken_when_nothing_can_send_but_manage_stays() {
        let mut v = view(vec![dev(1, "x")]);
        v.can_send = Capability::CantTell { reason: "no card".into() };
        let m = menu(&v).unwrap();
        assert!(!m.entries[0].enabled);
        assert!(m.entries[1].enabled);
    }

    #[test]
    fn the_list_is_capped_and_ids_map_back() {
        let many: Vec<WolDevice> = (1..=25).map(|i| dev(i, "d")).collect();
        let m = menu(&view(many)).unwrap();
        assert_eq!(m.entries.len(), MAX_ENTRIES + 1);
        assert_eq!(action_of("dy.wol.7"), Some(Action::Wake(7)));
        assert_eq!(action_of("dy.wol.manage"), Some(Action::Manage));
        assert_eq!(action_of("dy.wol.x"), None);
        assert_eq!(action_of("dy.awake.30"), None);
        assert_eq!(action_of(""), None);
    }
}
