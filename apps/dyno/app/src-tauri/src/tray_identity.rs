//! The stable operating-system identity of a tray / menu-bar item (ADR 0028): what lets the OS remember where the user put
//! each icon and whether it is shown. Pure: the names depend on the item id only, never on time, the run or the machine.
//!
//! - macOS: `NSStatusItem.autosaveName`, [`autosave_name`] (AppKit keeps the position and visibility under that name).
//! - Windows: `NOTIFYICONDATA.guidItem`, [`guid_for`] (a name-based GUID; the shell keys the "always show" setting on it).
//! - Linux: the StatusNotifierItem / appindicator id is the tray id, already stable ([`crate::tray::tray_id`]); the host
//!   decides the order, so there is nothing more to give.

use sha2::{Digest, Sha256};

/// The reverse-DNS prefix that makes the names unique to Dyno (the bundle identifier in `tauri.conf.json`).
const NAMESPACE: &str = "net.decryptology.dyno";

/// The macOS autosave name of an item, e.g. `net.decryptology.dyno.menubar.cpu-1`.
pub fn autosave_name(item: &str) -> String {
    format!("{NAMESPACE}.menubar.{item}")
}

/// The Windows GUID of an item: the first 16 bytes of SHA-256 over a Dyno-only name, with the version (5, name-based) and
/// variant bits of RFC 4122 set so it reads as a GUID. Equal for equal ids, different for different ids, and in Dyno's own
/// name space so it cannot meet another app's GUID.
pub fn guid_for(item: &str) -> u128 {
    let digest = Sha256::digest(format!("{NAMESPACE}.tray-icon.{item}").as_bytes());
    let mut b = [0u8; 16];
    b.copy_from_slice(&digest[..16]);
    b[6] = (b[6] & 0x0f) | 0x50;
    b[8] = (b[8] & 0x3f) | 0x80;
    u128::from_be_bytes(b)
}

/// The GUID as text (`8-4-4-4-12` lower-case hex), for the log.
pub fn guid_text(guid: u128) -> String {
    let h = format!("{guid:032x}");
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

/// The provider the patched `tray-icon` asks with the tray id (`dyno-<item>`): a GUID for Dyno's own icons, nothing for
/// any other id.
pub fn guid_for_tray_id(tray_id: &str) -> Option<u128> {
    tray_id.strip_prefix("dyno-").filter(|i| !i.is_empty()).map(guid_for)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_names_are_fixed_by_the_id_and_never_change() {
        // Pinned values: a change here moves every user's icons, so it must be deliberate (an ADR amendment).
        assert_eq!(autosave_name("cpu-1"), "net.decryptology.dyno.menubar.cpu-1");
        assert_eq!(guid_for("cpu-1"), guid_for("cpu-1"));
        assert_eq!(guid_text(guid_for("cpu-1")), "8f6f7e9f-9bfc-5b3b-bca6-6519140f1430");
    }

    #[test]
    fn many_ids_give_unique_names_and_guids() {
        let ids: Vec<String> = (0..200)
            .map(|n| format!("item-{n}"))
            .chain(["cpu-1".into(), "memory-2".into()])
            .collect();
        let names: HashSet<String> = ids.iter().map(|i| autosave_name(i)).collect();
        let guids: HashSet<u128> = ids.iter().map(|i| guid_for(i)).collect();
        assert_eq!(names.len(), ids.len());
        assert_eq!(guids.len(), ids.len());
    }

    #[test]
    fn a_guid_has_the_name_based_version_and_variant() {
        for i in ["a", "cpu-1", "gpu-12"] {
            let t = guid_text(guid_for(i));
            let parts: Vec<&str> = t.split('-').collect();
            assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), [8, 4, 4, 4, 12], "{t}");
            assert!(parts[2].starts_with('5'), "{t}");
            assert!("89ab".contains(parts[3].chars().next().unwrap()), "{t}");
        }
    }

    #[test]
    fn only_dynos_own_tray_ids_get_a_guid() {
        assert_eq!(guid_for_tray_id("dyno-cpu-1"), Some(guid_for("cpu-1")));
        assert_eq!(guid_for_tray_id("dyno-"), None);
        assert_eq!(guid_for_tray_id("other-cpu-1"), None);
        assert_eq!(guid_for_tray_id("main"), None);
    }
}
