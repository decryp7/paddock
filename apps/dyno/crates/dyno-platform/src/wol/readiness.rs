//! This PC's Wake-on-LAN readiness per OS (ADR 0032 section 3). Every setting that cannot be read is a "can't tell" item with the
//! reason, never a guess; the firmware setting is always "can't tell". The tool text is parsed by `dyno_core::wol::readiness`.
//!
//! macOS: `pmset -g` (`womp`). Windows: `powercfg /a` (sleep kind), `reg query` (Fast startup), `powercfg /devicequery` (which
//! physical cards may wake the computer; the cards come from WMI `Win32_NetworkAdapter`). Linux: `sysfs` `power/wakeup` of each
//! physical card (the card's own `ethtool` mode needs the helper and is not read here).

use crate::runner::{tool, ToolRunner, TOOL_TIMEOUT};
use dyno_core::wol::readiness::{
    adapter_item, cant_tell, fast_startup_item, firmware_item, linux_wakeup_item, parse_pmset_womp, parse_powercfg_a, parse_reg_dword,
    sleep_mode_item, ReadinessItem, ReadinessKind, MAX_ADAPTERS,
};
use dyno_core::wol::{ReadinessSource, ThisPcWol};
use std::path::PathBuf;

/// Reads the settings the OS lets a user read.
pub struct OsReadiness {
    runner: Box<dyn ToolRunner>,
    sysfs_net: PathBuf,
}

impl OsReadiness {
    /// Over `runner`.
    pub fn new(runner: Box<dyn ToolRunner>) -> OsReadiness {
        OsReadiness {
            runner,
            sysfs_net: "/sys/class/net".into(),
        }
    }

    fn text(&self, program: &str, args: &[&str]) -> Result<String, String> {
        match self.runner.run_tool(&tool(program, args), TOOL_TIMEOUT) {
            Ok(o) if o.ok => Ok(o.stdout),
            Ok(o) => Err(format!("{program} failed: {}", o.error)),
            Err(e) => Err(format!("{program} is not available: {e}")),
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn macos(&self) -> Vec<ReadinessItem> {
        let state = match self.text("/usr/bin/pmset", &["-g"]) {
            Ok(t) => parse_pmset_womp(&t),
            Err(e) => cant_tell(&format!("Dyno could not read the power settings ({e}).")),
        };
        vec![
            ReadinessItem {
                kind: ReadinessKind::OsSetting,
                subject: None,
                state,
            },
            firmware_item(),
        ]
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    fn windows(&self, cards: Result<Vec<String>, String>) -> Vec<ReadinessItem> {
        let mut items = Vec::new();
        let sleep = match self.text("powercfg", &["/a"]) {
            Ok(t) => parse_powercfg_a(&t),
            Err(e) => cant_tell(&format!("Dyno could not read the sleep states ({e}).")),
        };
        items.push(sleep_mode_item(sleep));
        let fast = self
            .text(
                "reg",
                &[
                    "query",
                    r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Power",
                    "/v",
                    "HiberbootEnabled",
                ],
            )
            .ok()
            .and_then(|t| parse_reg_dword(&t, "HiberbootEnabled"));
        items.push(fast_startup_item(fast));
        match cards {
            Ok(names) => {
                let armed = self.text("powercfg", &["/devicequery", "wake_armed"]);
                let programmable = self.text("powercfg", &["/devicequery", "wake_programmable"]);
                for name in names.into_iter().take(MAX_ADAPTERS) {
                    let allowed = match (&armed, &programmable) {
                        (Ok(a), _) if lists(a, &name) => Some(true),
                        (_, Ok(p)) if lists(p, &name) => Some(false),
                        _ => None,
                    };
                    items.push(adapter_item(&name, allowed));
                }
            }
            Err(e) => items.push(ReadinessItem {
                kind: ReadinessKind::NicWake,
                subject: None,
                state: cant_tell(&format!("Dyno could not list the network cards ({e}).")),
            }),
        }
        items.push(firmware_item());
        items
    }

    #[cfg_attr(target_os = "macos", allow(dead_code))]
    #[cfg_attr(windows, allow(dead_code))]
    fn linux(&self) -> Vec<ReadinessItem> {
        let mut items = Vec::new();
        let mut names: Vec<String> = match std::fs::read_dir(&self.sysfs_net) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect(),
            Err(e) => {
                items.push(ReadinessItem {
                    kind: ReadinessKind::NicWake,
                    subject: None,
                    state: cant_tell(&format!("Dyno could not list the network cards ({e}).")),
                });
                Vec::new()
            }
        };
        names.sort();
        for name in names {
            let dir = self.sysfs_net.join(&name);
            // A card with no `device` link is virtual (loopback, bridge, VPN): it has nothing to wake.
            if !dir.join("device").exists() || items.len() >= MAX_ADAPTERS {
                continue;
            }
            let state = std::fs::read_to_string(dir.join("device/power/wakeup")).unwrap_or_default();
            items.push(linux_wakeup_item(&name, &state));
        }
        items.push(firmware_item());
        items
    }
}

/// Whether `powercfg /devicequery` output lists the device `name` (a whole line, without case).
fn lists(output: &str, name: &str) -> bool {
    output.lines().take(500).any(|l| l.trim().eq_ignore_ascii_case(name.trim()))
}

#[cfg(windows)]
fn physical_cards() -> Result<Vec<String>, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Card {
        name: String,
    }
    let wmi = wmi::WMIConnection::new().map_err(|e| e.to_string())?;
    let cards: Vec<Card> = wmi
        .raw_query("SELECT Name FROM Win32_NetworkAdapter WHERE PhysicalAdapter = TRUE")
        .map_err(|e| e.to_string())?;
    Ok(cards.into_iter().map(|c| c.name).collect())
}

impl ReadinessSource for OsReadiness {
    fn read(&self) -> ThisPcWol {
        #[cfg(windows)]
        let items = self.windows(physical_cards());
        #[cfg(target_os = "macos")]
        let items = self.macos();
        #[cfg(not(any(windows, target_os = "macos")))]
        let items = self.linux();
        ThisPcWol { items }
    }
}

#[cfg(test)]
mod tests {
    use super::super::script::{fail, out, Script};
    use super::*;
    use dyno_core::wol::ItemState;

    fn over(answers: Vec<(&'static str, Result<crate::runner::ToolOutput, dyno_core::ports::PlatformError>)>) -> OsReadiness {
        OsReadiness::new(Box::new(Script {
            answers,
            calls: Default::default(),
        }))
    }

    #[test]
    fn macos_reports_womp_and_always_the_firmware_hint() {
        let items = over(vec![("pmset", out(" womp 0\n"))]).macos();
        assert!(matches!(items[0].state, ItemState::Problem { .. }));
        assert_eq!(items[1].kind, ReadinessKind::Firmware);
        let items = over(vec![("pmset", fail("denied"))]).macos();
        assert!(matches!(&items[0].state, ItemState::CantTell { reason } if reason.contains("could not read")));
        let items = over(vec![]).macos();
        assert!(matches!(items[0].state, ItemState::CantTell { .. }));
    }

    #[test]
    fn windows_joins_the_sleep_kind_fast_startup_and_each_card() {
        let r = over(vec![
            (
                "powercfg",
                out("The following sleep states are available on this system:\n    Standby (S3)\n"),
            ),
            ("reg", out("    HiberbootEnabled    REG_DWORD    0x1\n")),
        ]);
        // `powercfg` answers the same text to every call in this script: the card lists are matched by whole lines only.
        let items = r.windows(Ok(vec!["Test GbE Controller".into(), "Other Card".into()]));
        assert!(matches!(items[0].state, ItemState::Ok { .. }));
        assert!(matches!(items[1].state, ItemState::Problem { .. }));
        assert_eq!(items[2].subject.as_deref(), Some("Test GbE Controller"));
        assert!(matches!(items[2].state, ItemState::CantTell { .. }));
        assert_eq!(items.last().map(|i| i.kind), Some(ReadinessKind::Firmware));
        let r = over(vec![]);
        let items = r.windows(Err("no WMI".into()));
        assert!(items
            .iter()
            .any(|i| matches!(&i.state, ItemState::CantTell { reason } if reason.contains("could not list"))));
    }

    #[test]
    fn a_device_list_names_a_card_by_whole_line() {
        let text = "Test GbE Controller\nOther Card\n";
        assert!(lists(text, "test gbe controller"));
        assert!(!lists(text, "Test GbE"));
        assert!(!lists("NONE\n", "x"));
    }

    #[test]
    fn linux_lists_only_physical_cards() {
        let dir = tempfile::tempdir().unwrap();
        let net = dir.path();
        std::fs::create_dir_all(net.join("eth0/device/power")).unwrap();
        std::fs::write(net.join("eth0/device/power/wakeup"), "enabled\n").unwrap();
        std::fs::create_dir_all(net.join("eth1/device/power")).unwrap();
        std::fs::create_dir_all(net.join("lo")).unwrap();
        let mut r = over(vec![]);
        r.sysfs_net = net.to_path_buf();
        let items = r.linux();
        assert_eq!(items.len(), 3);
        assert!(matches!(items[0].state, ItemState::Ok { .. }));
        assert_eq!(items[1].subject.as_deref(), Some("eth1"));
        assert!(matches!(items[1].state, ItemState::CantTell { .. }));
        let mut r = over(vec![]);
        r.sysfs_net = net.join("missing");
        assert!(matches!(&r.linux()[0].state, ItemState::CantTell { .. }));
    }
}
