//! Whether this PC can be woken over the network (ADR 0032 section 3): the OS settings Dyno can read, each with an honest state.
//! Every item is Ok, a Problem with the fix, or "can't tell" with the reason; Dyno never guesses, and the firmware (BIOS/UEFI)
//! setting is always "can't tell". The parsers take the text of `pmset -g`, `powercfg /a`, `reg query` and `sysfs`.

use serde::{Deserialize, Serialize};

/// What an item is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReadinessKind {
    /// A network card's own wake setting.
    NicWake,
    /// The operating system's network-wake switch (macOS "Wake for network access").
    OsSetting,
    /// Windows Fast startup, which disarms wake from shut down.
    FastStartup,
    /// The kind of sleep the computer uses (classic S3 or Modern Standby).
    SleepMode,
    /// The BIOS/UEFI setting.
    Firmware,
}

/// One finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ItemState {
    /// Fine for Wake-on-LAN.
    Ok {
        /// What it means, in plain words.
        note: String,
    },
    /// Stops or limits Wake-on-LAN.
    Problem {
        /// What is wrong.
        note: String,
        /// What to do about it.
        fix: String,
    },
    /// Dyno cannot read it here.
    CantTell {
        /// Why not.
        reason: String,
    },
}

/// One row of the readiness panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessItem {
    /// What it is about.
    pub kind: ReadinessKind,
    /// The card it concerns, for a card's setting.
    pub subject: Option<String>,
    /// What Dyno found.
    pub state: ItemState,
}

/// This PC's Wake-on-LAN readiness.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThisPcWol {
    /// The findings.
    pub items: Vec<ReadinessItem>,
}

/// The most cards one readiness read lists.
pub const MAX_ADAPTERS: usize = 32;

fn ok(note: &str) -> ItemState {
    ItemState::Ok { note: note.into() }
}

fn problem(note: &str, fix: &str) -> ItemState {
    ItemState::Problem {
        note: note.into(),
        fix: fix.into(),
    }
}

/// An item that cannot be read here.
pub fn cant_tell(reason: &str) -> ItemState {
    ItemState::CantTell { reason: reason.into() }
}

/// The firmware item, the same on every OS.
pub fn firmware_item() -> ReadinessItem {
    ReadinessItem {
        kind: ReadinessKind::Firmware,
        subject: None,
        state: cant_tell("Waking from shut down also depends on a BIOS/UEFI setting that Dyno cannot read. Check the firmware settings page for \"Wake on LAN\" or \"Power on by PCI-E\"."),
    }
}

/// macOS `pmset -g`: the line `womp 1` is "Wake for network access".
pub fn parse_pmset_womp(text: &str) -> ItemState {
    let value = text
        .lines()
        .take(200)
        .map(str::trim)
        .find_map(|l| l.strip_prefix("womp").map(str::trim))
        .and_then(|v| v.split_whitespace().next());
    match value {
        Some("1") => ok("Wake for network access is on."),
        Some("0") => problem(
            "Wake for network access is off, so this Mac will not wake for a magic packet.",
            "Turn on Wake for network access in System Settings, Battery (or Energy), Options.",
        ),
        _ => cant_tell("The power settings do not list Wake for network access (this Mac may not offer it)."),
    }
}

/// Windows `powercfg /a`: which sleep states the computer has. Only the part before "are not available" counts.
pub fn parse_powercfg_a(text: &str) -> ItemState {
    let mut available = String::new();
    for line in text.lines().take(200) {
        let l = line.trim();
        if l.contains("not available") {
            break;
        }
        available.push_str(l);
        available.push('\n');
    }
    if available.contains("Standby (S3)") {
        ok("This PC uses classic sleep (S3), which a magic packet can wake.")
    } else if available.contains("S0 Low Power Idle") {
        cant_tell(
            "This PC uses Modern Standby (S0). Waking it from the network depends on the network driver; Dyno cannot tell if it will work.",
        )
    } else {
        cant_tell("Dyno could not tell which kind of sleep this PC uses.")
    }
}

/// A `REG_DWORD` value from `reg query` output: `    HiberbootEnabled    REG_DWORD    0x1`.
pub fn parse_reg_dword(text: &str, name: &str) -> Option<u32> {
    text.lines().take(100).find_map(|l| {
        let p: Vec<&str> = l.split_whitespace().collect();
        if p.len() >= 3 && p[0].eq_ignore_ascii_case(name) && p[1] == "REG_DWORD" {
            u32::from_str_radix(p[2].trim_start_matches("0x"), 16).ok()
        } else {
            None
        }
    })
}

/// Windows Fast startup (`HiberbootEnabled`): on, a shut down is a hibernate and the cards are not armed.
pub fn fast_startup_item(value: Option<u32>) -> ReadinessItem {
    let state = match value {
        Some(0) => ok("Fast startup is off, so a shut down arms the network card."),
        Some(_) => problem(
            "Fast startup is on: a normal shut down does not arm the network card, so Wake-on-LAN works from sleep, not from shut down.",
            "Turn off Fast startup in Control Panel, Power Options, Choose what the power buttons do.",
        ),
        None => {
            cant_tell("Dyno could not read the Fast startup setting. Check Control Panel, Power Options, Choose what the power buttons do.")
        }
    };
    ReadinessItem {
        kind: ReadinessKind::FastStartup,
        subject: None,
        state,
    }
}

/// The sleep-mode item from the text of `powercfg /a` (or its absence).
pub fn sleep_mode_item(state: ItemState) -> ReadinessItem {
    ReadinessItem {
        kind: ReadinessKind::SleepMode,
        subject: None,
        state,
    }
}

/// A Windows card's "Allow this device to wake the computer" setting.
pub fn adapter_item(name: &str, allowed: Option<bool>) -> ReadinessItem {
    let state = match allowed {
        Some(true) => ok("The card may wake the computer."),
        Some(false) => problem(
            "The card may not wake the computer.",
            "In Device Manager, open the card's Power Management tab and allow it to wake the computer (and only for a magic packet).",
        ),
        None => cant_tell("This card does not list a wake setting Dyno can read without administrator rights. Check Device Manager, the card, Power Management."),
    };
    ReadinessItem {
        kind: ReadinessKind::NicWake,
        subject: Some(name.chars().take(120).collect()),
        state,
    }
}

/// A Linux card's `power/wakeup` file (`enabled` or `disabled`). The card's own Wake-on-LAN mode needs the helper to read.
pub fn linux_wakeup_item(name: &str, text: &str) -> ReadinessItem {
    let state = match text.trim() {
        "enabled" => ok("The card may wake the computer. Which packets it wakes for is its Wake-on-LAN mode, which Dyno cannot read without the helper."),
        "disabled" => problem(
            "The card is not allowed to wake the computer.",
            "Allow it with `ethtool -s <card> wol g`, and set the card's power/wakeup to enabled.",
        ),
        _ => cant_tell("The card does not say whether it may wake the computer."),
    };
    ReadinessItem {
        kind: ReadinessKind::NicWake,
        subject: Some(name.chars().take(120).collect()),
        state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pmset_womp_is_on_off_or_unknown() {
        assert!(matches!(
            parse_pmset_womp("System-wide power settings:\n womp                 1\n sleep 1"),
            ItemState::Ok { .. }
        ));
        assert!(matches!(parse_pmset_womp(" womp 0\n"), ItemState::Problem { .. }));
        assert!(matches!(parse_pmset_womp(" sleep 1\n"), ItemState::CantTell { .. }));
        assert!(matches!(parse_pmset_womp(" womp x\n"), ItemState::CantTell { .. }));
        assert!(matches!(parse_pmset_womp(""), ItemState::CantTell { .. }));
    }

    #[test]
    fn powercfg_tells_classic_sleep_from_modern_standby() {
        let s3 = "The following sleep states are available on this system:\n    Standby (S3)\n    Hibernate\n\nThe following sleep states are not available on this system:\n    Standby (S0 Low Power Idle)\n        The system firmware does not support this standby state.\n";
        assert!(matches!(parse_powercfg_a(s3), ItemState::Ok { .. }));
        let s0 = "The following sleep states are available on this system:\n    Standby (S0 Low Power Idle) Network Connected\n\nThe following sleep states are not available on this system:\n    Standby (S3)\n        The system firmware does not support this standby state.\n";
        let st = parse_powercfg_a(s0);
        assert!(matches!(&st, ItemState::CantTell { reason } if reason.contains("Modern Standby")));
        assert!(matches!(parse_powercfg_a("Access denied"), ItemState::CantTell { .. }));
    }

    #[test]
    fn reg_dword_and_fast_startup() {
        let out = "\nHKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Power\n    HiberbootEnabled    REG_DWORD    0x1\n\n";
        assert_eq!(parse_reg_dword(out, "HiberbootEnabled"), Some(1));
        assert_eq!(parse_reg_dword(out, "Other"), None);
        assert_eq!(parse_reg_dword("HiberbootEnabled REG_SZ x", "HiberbootEnabled"), None);
        assert!(matches!(fast_startup_item(Some(1)).state, ItemState::Problem { .. }));
        assert!(matches!(fast_startup_item(Some(0)).state, ItemState::Ok { .. }));
        assert!(matches!(fast_startup_item(None).state, ItemState::CantTell { .. }));
    }

    #[test]
    fn card_items_name_the_card_and_say_what_to_do() {
        let a = adapter_item("Ethernet", Some(false));
        assert_eq!(a.subject.as_deref(), Some("Ethernet"));
        assert!(matches!(a.state, ItemState::Problem { .. }));
        assert!(matches!(adapter_item("x", Some(true)).state, ItemState::Ok { .. }));
        assert!(matches!(adapter_item("x", None).state, ItemState::CantTell { .. }));
        assert!(matches!(linux_wakeup_item("eth0", "enabled\n").state, ItemState::Ok { .. }));
        assert!(matches!(linux_wakeup_item("eth0", "disabled").state, ItemState::Problem { .. }));
        assert!(matches!(linux_wakeup_item("eth0", "").state, ItemState::CantTell { .. }));
        assert_eq!(adapter_item(&"n".repeat(500), None).subject.unwrap().len(), 120);
    }

    #[test]
    fn firmware_is_always_cant_tell() {
        assert!(matches!(firmware_item().state, ItemState::CantTell { .. }));
        assert_eq!(sleep_mode_item(cant_tell("r")).kind, ReadinessKind::SleepMode);
    }
}
