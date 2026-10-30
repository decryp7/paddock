//! The pure parts of the app's own start-at-login entry (ADR 0010 amendment, "the single route"): where the app may
//! register itself, which old file is ours to delete, whether a start counts as hidden, and which route an OS uses.
//! `login.rs` implements the macOS registration over `SMAppService.mainApp`; nothing here touches the OS, so every
//! OS runs these tests.

use std::path::Path;

/// The one `launchd` label and file name `tauri-plugin-autostart` (through `auto-launch`) used on macOS: the app name.
pub const LEGACY_LABEL: &str = "Dyno";

/// The program the old entry started (`dyno-app`, the app's executable inside the bundle).
const APP_EXE: &str = "dyno-app";

/// Which mechanism registers the app itself at login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppLoginRoute {
    /// macOS: `SMAppService.mainApp`, one row in Login Items, nothing written under `~/Library/LaunchAgents`.
    MainApp,
    /// Linux and Windows: `tauri-plugin-autostart` (an autostart file or the `Run` entry).
    Plugin,
}

/// The route for an OS name (`std::env::consts::OS`).
pub fn route_for(os: &str) -> AppLoginRoute {
    if os == "macos" {
        AppLoginRoute::MainApp
    } else {
        AppLoginRoute::Plugin
    }
}

/// Refuses a registration for an executable that will not exist after the next login: Gatekeeper path randomisation
/// (`/AppTranslocation/`) or a mounted disk image or other volume (`/Volumes/`). The message says what to do.
pub fn refuse_unstable_location(exe: &Path) -> Result<(), String> {
    let path = exe.to_string_lossy();
    if path.contains("/AppTranslocation/") || path.starts_with("/Volumes/") {
        return Err(
            "Dyno is running from a temporary location (a disk image, another disk or a quarantined download). Move Dyno to the \
             Applications folder on this Mac's startup disk (not another disk or a disk image), open it from there, and turn on Start at login again."
                .into(),
        );
    }
    Ok(())
}

/// Whether a start is hidden in the tray: the `--hidden` flag, or the OS says the app was launched as a login item.
pub fn hidden_at_start(flag: bool, launched_as_login_item: bool) -> bool {
    flag || launched_as_login_item
}

/// Whether `xml` is exactly the file `tauri-plugin-autostart` wrote for Dyno on macOS: label `Dyno`, a `dyno-app` program
/// (optionally with `--hidden`), `RunAtLoad`, an empty `AssociatedBundleIdentifiers` and nothing else. Anything that
/// differs is not ours to delete.
pub fn is_legacy_plugin_plist(xml: &str) -> bool {
    let flat: String = xml.lines().map(str::trim).collect();
    let Some(body) = flat.split_once("<plist version=\"1.0\">").map(|(_, b)| b) else {
        return false;
    };
    let Some(rest) = body.strip_prefix(&format!(
        "<dict><key>Label</key><string>{LEGACY_LABEL}</string><key>AssociatedBundleIdentifiers</key><array></array>\
         <key>ProgramArguments</key><array>"
    )) else {
        return false;
    };
    let Some(args) = rest.strip_suffix("</array><key>RunAtLoad</key><true/></dict></plist>") else {
        return false;
    };
    let mut items = Vec::new();
    let mut tail = args;
    while !tail.is_empty() {
        let Some(after) = tail.strip_prefix("<string>") else {
            return false;
        };
        let Some((value, next)) = after.split_once("</string>") else {
            return false;
        };
        if value.contains('<') {
            return false;
        }
        items.push(value);
        tail = next;
    }
    match items.as_slice() {
        [program] => is_app_path(program),
        [program, flag] => is_app_path(program) && *flag == paddock_window::launch::HIDDEN_FLAG,
        _ => false,
    }
}

/// `/<folder>/Dyno.app/Contents/MacOS/dyno-app`: the app's executable inside its bundle, nothing looser. The plist is a
/// macOS file whatever OS reads it, so the path is split on `/` as text instead of with the host's `Path` rules (on
/// Windows a leading `/` is not an absolute path), which keeps this matcher and its tests the same on every OS.
fn is_app_path(program: &str) -> bool {
    let Some(rest) = program.strip_prefix('/') else {
        return false;
    };
    let parts: Vec<&str> = rest.split('/').collect();
    matches!(parts.as_slice(), [.., bundle, "Contents", "MacOS", exe] if *bundle == "Dyno.app" && *exe == APP_EXE)
}

/// What [`remove_legacy_plist`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum LegacyPlist {
    /// No such file.
    Absent,
    /// The file is not the one the old route wrote: left alone.
    Foreign,
    /// Deleted.
    Removed,
}

/// Deletes `<launch_agents>/Dyno.plist` only when it has exactly the old route's shape. Any other file, a link, a
/// folder or an unreadable file is left alone.
pub fn remove_legacy_plist(launch_agents: &Path) -> std::io::Result<LegacyPlist> {
    let file = launch_agents.join(format!("{LEGACY_LABEL}.plist"));
    let meta = match std::fs::symlink_metadata(&file) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(LegacyPlist::Absent),
        Err(e) => return Err(e),
    };
    // A bounded read of a regular file; a link, a folder or a large file is not what the plugin wrote.
    if !meta.is_file() || meta.len() > 16 * 1024 {
        return Ok(LegacyPlist::Foreign);
    }
    let Ok(text) = std::fs::read_to_string(&file) else {
        return Ok(LegacyPlist::Foreign);
    };
    if !is_legacy_plugin_plist(&text) {
        return Ok(LegacyPlist::Foreign);
    }
    std::fs::remove_file(&file)?;
    Ok(LegacyPlist::Removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `auto-launch` 0.6.0 writes (`build_launch_agent_plist`), program and arguments filled in.
    fn plugin_plist(label: &str, strings: &[&str], extra: &str) -> String {
        let args: String = strings.iter().map(|s| format!("<string>{s}</string>")).collect();
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n  <dict>\n  <key>Label</key>\n  \
             <string>{label}</string>\n  <key>AssociatedBundleIdentifiers</key>\n  <array></array>\n  <key>ProgramArguments</key>\n  \
             <array>{args}</array>\n  <key>RunAtLoad</key>\n  <true/>\n  {extra}</dict>\n</plist>"
        )
    }

    const EXE: &str = "/Applications/Dyno.app/Contents/MacOS/dyno-app";

    #[test]
    fn translocated_and_mounted_paths_are_refused_with_a_next_step() {
        for bad in [
            "/private/var/folders/ab/T/AppTranslocation/1F2E/d/Dyno.app/Contents/MacOS/dyno-app",
            "/Volumes/Dyno/Dyno.app/Contents/MacOS/dyno-app",
        ] {
            let m = refuse_unstable_location(Path::new(bad)).unwrap_err();
            assert!(m.contains("Applications") && m.contains("another disk"), "{m}");
        }
        for good in [EXE, "/Users/a/Applications/Dyno.app/Contents/MacOS/dyno-app", "/home/a/dyno"] {
            assert!(refuse_unstable_location(Path::new(good)).is_ok(), "{good}");
        }
    }

    #[test]
    fn a_volume_name_inside_a_path_is_not_a_mounted_volume() {
        assert!(refuse_unstable_location(Path::new("/Users/a/Volumes/Dyno.app/Contents/MacOS/dyno-app")).is_ok());
    }

    #[test]
    fn only_macos_uses_the_main_app_route() {
        assert_eq!(route_for("macos"), AppLoginRoute::MainApp);
        assert_eq!(route_for("linux"), AppLoginRoute::Plugin);
        assert_eq!(route_for("windows"), AppLoginRoute::Plugin);
    }

    #[test]
    fn a_start_is_hidden_by_the_flag_or_the_login_item_event() {
        assert!(hidden_at_start(true, false));
        assert!(hidden_at_start(false, true));
        assert!(!hidden_at_start(false, false));
    }

    #[test]
    fn the_plugins_own_file_matches() {
        assert!(is_legacy_plugin_plist(&plugin_plist("Dyno", &[EXE, "--hidden"], "")));
        assert!(is_legacy_plugin_plist(&plugin_plist("Dyno", &[EXE], "")));
    }

    #[test]
    fn any_other_shape_is_not_ours() {
        let cases = [
            plugin_plist("Other", &[EXE, "--hidden"], ""),
            plugin_plist("Dyno", &["/usr/bin/evil", "--hidden"], ""),
            plugin_plist("Dyno", &["dyno-app"], ""),
            plugin_plist("Dyno", &["/usr/local/bin/dyno-app"], ""),
            plugin_plist("Dyno", &["/Applications/Other.app/Contents/MacOS/dyno-app"], ""),
            plugin_plist("Dyno", &["/Applications/Dyno.app/Contents/Resources/dyno-app"], ""),
            plugin_plist("Dyno", &[EXE, "--other"], ""),
            plugin_plist("Dyno", &[EXE, "--hidden", "x"], ""),
            plugin_plist("Dyno", &[], ""),
            plugin_plist("Dyno", &[EXE, "--hidden"], "<key>KeepAlive</key>\n  <true/>\n  "),
            String::new(),
            "<plist version=\"1.0\"><dict></dict></plist>".into(),
        ];
        for c in cases {
            assert!(!is_legacy_plugin_plist(&c), "{c}");
        }
    }

    #[test]
    fn only_a_matching_file_is_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Dyno.plist");
        assert_eq!(remove_legacy_plist(dir.path()).unwrap(), LegacyPlist::Absent);

        std::fs::write(&file, "mine").unwrap();
        assert_eq!(remove_legacy_plist(dir.path()).unwrap(), LegacyPlist::Foreign);
        assert!(file.exists());

        std::fs::write(&file, plugin_plist("Dyno", &[EXE, "--hidden"], "")).unwrap();
        assert_eq!(remove_legacy_plist(dir.path()).unwrap(), LegacyPlist::Removed);
        assert!(!file.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_or_folder_is_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.plist");
        std::fs::write(&target, plugin_plist("Dyno", &[EXE], "")).unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("Dyno.plist")).unwrap();
        assert_eq!(remove_legacy_plist(dir.path()).unwrap(), LegacyPlist::Foreign);
        assert!(target.exists());

        let other = tempfile::tempdir().unwrap();
        std::fs::create_dir(other.path().join("Dyno.plist")).unwrap();
        assert_eq!(remove_legacy_plist(other.path()).unwrap(), LegacyPlist::Foreign);
    }
}
