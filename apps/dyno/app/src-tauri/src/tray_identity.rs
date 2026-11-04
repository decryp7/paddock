//! The stable operating-system identity of a tray / menu-bar item (ADR 0028): what lets the OS remember where the user put
//! each icon and whether it is shown. Pure: the names depend on the item id only, never on time, the run or the machine.
//!
//! - macOS: `NSStatusItem.autosaveName`, [`autosave_name`] (AppKit keeps the position and visibility under that name).
//! - Windows: `NOTIFYICONDATA.guidItem`, [`guid_for`] (a stable GUID; the shell keys the "always show" setting on it).
//! - Linux: the StatusNotifierItem / appindicator id is the tray id, already stable ([`crate::tray::tray_id`]); the host
//!   decides the order, so there is nothing more to give.

use sha2::{Digest, Sha256};

/// The reverse-DNS prefix that makes the names unique to Dyno (the bundle identifier in `tauri.conf.json`).
const NAMESPACE: &str = "net.decryptology.dyno";

/// Dyno's bundle identifier (`tauri.conf.json`), the name an Apple event addresses the app by.
pub const BUNDLE_ID: &str = "net.decryptology.dyno";

/// The macOS autosave name of an item, e.g. `net.decryptology.dyno.menubar.cpu-1`.
pub fn autosave_name(item: &str) -> String {
    format!("{NAMESPACE}.menubar.{item}")
}

/// The Windows GUID of an item: the first 16 bytes of SHA-256 over a Dyno-only name, with the version (8, "custom", RFC 9562:
/// it is not the SHA-1 of version 5) and the variant bits set so it reads as a GUID. Equal for equal ids, different for different ids, and in Dyno's own
/// name space so it cannot meet another app's GUID.
pub fn guid_for(item: &str) -> u128 {
    let digest = Sha256::digest(format!("{NAMESPACE}.tray-icon.{item}").as_bytes());
    let mut b = [0u8; 16];
    b.copy_from_slice(&digest[..16]);
    b[6] = (b[6] & 0x0f) | 0x80;
    b[8] = (b[8] & 0x3f) | 0x80;
    u128::from_be_bytes(b)
}

/// The `NSUserDefaults` keys AppKit keeps for a status item with this autosave name: its position and, once the user has hidden
/// or shown it, its visibility. AppKit is believed to delete them when the item is removed (ADR 0028, to be verified on the
/// Mac), so Dyno keeps a copy under [`saved_key`] and puts it back before the item is made again.
pub fn appkit_defaults_keys(autosave_name: &str) -> [String; 2] {
    [
        format!("NSStatusItem Preferred Position {autosave_name}"),
        format!("NSStatusItem Visible {autosave_name}"),
    ]
}

/// The key Dyno puts back: the position only. The visibility key is copied too (so it is not lost) but never restored, or an item
/// the user once hid would stay hidden every time Dyno made it again.
pub fn restored_keys(autosave_name: &str) -> [String; 1] {
    [appkit_defaults_keys(autosave_name)[0].clone()]
}

/// Where Dyno keeps its copy of an AppKit key.
pub fn saved_key(appkit_key: &str) -> String {
    format!("{NAMESPACE}.saved.{appkit_key}")
}

/// Whether the `n`th draw of an item (1-based) also saves its AppKit position: the first, then every 30th. Counted, not timed
/// (a drag in the menu bar changes the key without telling us).
pub fn should_save_position(n: u64) -> bool {
    n == 1 || (n > 0 && n.is_multiple_of(30))
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

/// The provider the patched `tray-icon` asks on macOS with the tray id (`dyno-<item>`): the autosave name for Dyno's own icons,
/// given to the status item before it connects to the menu bar.
pub fn autosave_name_for_tray_id(tray_id: &str) -> Option<String> {
    tray_id.strip_prefix("dyno-").filter(|i| !i.is_empty()).map(autosave_name)
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
        assert_eq!(guid_text(guid_for("cpu-1")), "8f6f7e9f-9bfc-8b3b-bca6-6519140f1430");
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
    fn a_guid_has_the_custom_version_and_the_rfc_variant() {
        for i in ["a", "cpu-1", "gpu-12"] {
            let t = guid_text(guid_for(i));
            let parts: Vec<&str> = t.split('-').collect();
            assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), [8, 4, 4, 4, 12], "{t}");
            assert!(parts[2].starts_with('8'), "{t}");
            assert!("89ab".contains(parts[3].chars().next().unwrap()), "{t}");
        }
    }

    #[test]
    fn the_appkit_keys_and_their_copies_are_distinct_and_fixed() {
        let keys = appkit_defaults_keys("net.decryptology.dyno.menubar.cpu-1");
        assert_eq!(keys[0], "NSStatusItem Preferred Position net.decryptology.dyno.menubar.cpu-1");
        assert_eq!(keys[1], "NSStatusItem Visible net.decryptology.dyno.menubar.cpu-1");
        assert_eq!(
            saved_key(&keys[0]),
            "net.decryptology.dyno.saved.NSStatusItem Preferred Position net.decryptology.dyno.menubar.cpu-1"
        );
        assert_ne!(saved_key(&keys[0]), saved_key(&keys[1]));
    }

    #[test]
    fn only_the_position_is_restored_never_a_hidden_state() {
        let name = "net.decryptology.dyno.menubar.cpu-1";
        assert_eq!(restored_keys(name).to_vec(), [appkit_defaults_keys(name)[0].clone()]);
        assert!(restored_keys(name).iter().all(|k| !k.contains("Visible")));
    }

    #[test]
    fn positions_are_saved_on_the_first_draw_and_every_thirtieth() {
        let saved: Vec<u64> = (1..=100).filter(|n| should_save_position(*n)).collect();
        assert_eq!(saved, [1, 30, 60, 90]);
        assert!(!should_save_position(0));
    }

    #[test]
    fn only_dynos_own_tray_ids_get_a_guid() {
        assert_eq!(guid_for_tray_id("dyno-cpu-1"), Some(guid_for("cpu-1")));
        assert_eq!(guid_for_tray_id("dyno-"), None);
        assert_eq!(guid_for_tray_id("other-cpu-1"), None);
        assert_eq!(guid_for_tray_id("main"), None);
        assert_eq!(autosave_name_for_tray_id("dyno-cpu-1"), Some(autosave_name("cpu-1")));
        assert_eq!(autosave_name_for_tray_id("dyno-"), None);
        assert_eq!(autosave_name_for_tray_id("main"), None);
    }
}
