//! The launch agent `tauri-plugin-autostart` (through `auto-launch` 0.6) wrote on macOS for an app before it moved to its
//! [`super::MainAppItem`] (`SMAppService.mainApp`): recognising exactly that file, deleting it, and registering the new item
//! in its place so the switch never says on with nothing registered. Moved from Dyno's `login_item.rs` and `login.rs`
//! (Dyno 1.0 used the plugin's `MacosLauncher::LaunchAgent`; Spotter, Pitwall and Pitcrew still do).

use super::Autostart;
use std::path::Path;

/// The largest file the plugin could have written; anything bigger is not read.
const MAX_PLIST: u64 = 16 * 1024;

/// The app's old launch agent: `<launch agents>/<label>.plist` starting `/<folder>/<bundle>/Contents/MacOS/<exe>`,
/// optionally with `flag`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyLaunchAgent<'a> {
    /// The plist's label and file name, the app name (`Dyno`).
    pub label: &'a str,
    /// The bundle folder (`Dyno.app`).
    pub bundle: &'a str,
    /// The executable inside the bundle (`dyno-app`).
    pub exe: &'a str,
    /// The one argument the plugin may have added (`--hidden`).
    pub flag: &'a str,
}

/// What [`LegacyLaunchAgent::remove`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyPlist {
    /// No such file.
    Absent,
    /// The file is not the one the old route wrote: left alone.
    Foreign,
    /// Deleted.
    Removed,
}

impl LegacyLaunchAgent<'_> {
    /// Whether `xml` is exactly the file the plugin wrote: this label, the app's program (optionally with the flag),
    /// `RunAtLoad`, an empty `AssociatedBundleIdentifiers` and nothing else. Anything that differs is not ours to delete.
    pub fn matches(&self, xml: &str) -> bool {
        let flat: String = xml.lines().map(str::trim).collect();
        let Some(body) = flat.split_once("<plist version=\"1.0\">").map(|(_, b)| b) else {
            return false;
        };
        let Some(rest) = body.strip_prefix(&format!(
            "<dict><key>Label</key><string>{}</string><key>AssociatedBundleIdentifiers</key><array></array>\
             <key>ProgramArguments</key><array>",
            self.label
        )) else {
            return false;
        };
        let Some(args) = rest.strip_suffix("</array><key>RunAtLoad</key><true/></dict></plist>")
        else {
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
            [program] => self.is_app_path(program),
            [program, flag] => self.is_app_path(program) && *flag == self.flag,
            _ => false,
        }
    }

    /// `/<folder>/<bundle>/Contents/MacOS/<exe>`: the app's executable inside its bundle, nothing looser. The plist is a
    /// macOS file whatever OS reads it, so the path is split on `/` as text instead of with the host's `Path` rules (on
    /// Windows a leading `/` is not an absolute path), which keeps this matcher and its tests the same on every OS.
    fn is_app_path(&self, program: &str) -> bool {
        let Some(rest) = program.strip_prefix('/') else {
            return false;
        };
        let parts: Vec<&str> = rest.split('/').collect();
        matches!(parts.as_slice(), [.., bundle, "Contents", "MacOS", exe] if *bundle == self.bundle && *exe == self.exe)
    }

    /// Deletes `<launch_agents>/<label>.plist` only when it has exactly the old route's shape ([`Self::matches`]). Any
    /// other file, a link, a folder, a large or an unreadable file is left alone.
    ///
    /// # Errors
    /// The folder could not be looked into, or the file could not be deleted.
    pub fn remove(&self, launch_agents: &Path) -> std::io::Result<LegacyPlist> {
        let file = launch_agents.join(format!("{}.plist", self.label));
        let meta = match std::fs::symlink_metadata(&file) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(LegacyPlist::Absent),
            Err(e) => return Err(e),
        };
        // A bounded read of a regular file; a link, a folder or a large file is not what the plugin wrote.
        if !meta.is_file() || meta.len() > MAX_PLIST {
            return Ok(LegacyPlist::Foreign);
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            return Ok(LegacyPlist::Foreign);
        };
        if !self.matches(&text) {
            return Ok(LegacyPlist::Foreign);
        }
        std::fs::remove_file(&file)?;
        Ok(LegacyPlist::Removed)
    }

    /// Removes the old file from `launch_agents` ([`Self::remove`]) and, because its existence means the switch was on,
    /// registers the app's new item through `autostart`, so the switch never says on with nothing registered. An unstable
    /// `location` ([`crate::launch::refuse_unstable_location`]) removes the file but registers nothing (the user is told at
    /// the next toggle).
    ///
    /// # Errors
    /// The file could not be looked at or removed, or the new item was refused.
    pub fn migrate(
        &self,
        launch_agents: &Path,
        location: Result<(), String>,
        autostart: &mut dyn Autostart,
    ) -> Result<LegacyPlist, String> {
        let result = self.remove(launch_agents).map_err(|e| e.to_string())?;
        if result == LegacyPlist::Removed && location.is_ok() {
            autostart.set(true)?;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::login::Autostart;

    /// What `auto-launch` 0.6.0 writes (`build_launch_agent_plist`), program and arguments filled in.
    fn plugin_plist(label: &str, strings: &[&str], extra: &str) -> String {
        let args: String = strings
            .iter()
            .map(|s| format!("<string>{s}</string>"))
            .collect();
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n  <dict>\n  <key>Label</key>\n  \
             <string>{label}</string>\n  <key>AssociatedBundleIdentifiers</key>\n  <array></array>\n  <key>ProgramArguments</key>\n  \
             <array>{args}</array>\n  <key>RunAtLoad</key>\n  <true/>\n  {extra}</dict>\n</plist>"
        )
    }

    const DEMO: LegacyLaunchAgent<'static> = LegacyLaunchAgent {
        label: "Demo",
        bundle: "Demo.app",
        exe: "demo-app",
        flag: "--hidden",
    };
    const EXE: &str = "/Applications/Demo.app/Contents/MacOS/demo-app";

    #[test]
    fn the_plugins_own_file_matches_with_or_without_the_flag() {
        assert!(DEMO.matches(&plugin_plist("Demo", &[EXE, "--hidden"], "")));
        assert!(DEMO.matches(&plugin_plist("Demo", &[EXE], "")));
        assert!(DEMO.matches(&plugin_plist(
            "Demo",
            &["/Users/u/Applications/Demo.app/Contents/MacOS/demo-app"],
            ""
        )));
    }

    #[test]
    fn any_other_shape_is_not_ours() {
        let cases = [
            plugin_plist("Other", &[EXE, "--hidden"], ""),
            plugin_plist("Demo", &["/usr/bin/evil", "--hidden"], ""),
            plugin_plist("Demo", &["demo-app"], ""),
            plugin_plist("Demo", &["/usr/local/bin/demo-app"], ""),
            plugin_plist(
                "Demo",
                &["/Applications/Other.app/Contents/MacOS/demo-app"],
                "",
            ),
            plugin_plist(
                "Demo",
                &["/Applications/Demo.app/Contents/Resources/demo-app"],
                "",
            ),
            plugin_plist("Demo", &[EXE, "--other"], ""),
            plugin_plist("Demo", &[EXE, "--hidden", "x"], ""),
            plugin_plist("Demo", &[], ""),
            plugin_plist("Demo", &["<b>"], ""),
            plugin_plist(
                "Demo",
                &[EXE, "--hidden"],
                "<key>KeepAlive</key>\n  <true/>\n  ",
            ),
            plugin_plist("Demo", &[EXE], "").replace("<string>/", "<strin>/"),
            plugin_plist("Demo", &[EXE], "").replace("demo-app</string>", "demo-app"),
            String::new(),
            "<plist version=\"1.0\"><dict></dict></plist>".into(),
        ];
        for c in cases {
            assert!(!DEMO.matches(&c), "{c}");
        }
    }

    #[test]
    fn only_a_matching_file_is_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Demo.plist");
        assert_eq!(DEMO.remove(dir.path()).unwrap(), LegacyPlist::Absent);

        std::fs::write(&file, "mine").unwrap();
        assert_eq!(DEMO.remove(dir.path()).unwrap(), LegacyPlist::Foreign);
        assert!(file.exists());

        std::fs::write(&file, plugin_plist("Demo", &[EXE, "--hidden"], "")).unwrap();
        assert_eq!(DEMO.remove(dir.path()).unwrap(), LegacyPlist::Removed);
        assert!(!file.exists());
    }

    #[test]
    fn a_large_file_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Demo.plist");
        let big = plugin_plist("Demo", &[EXE], &" ".repeat(17 * 1024));
        std::fs::write(&file, big).unwrap();
        assert_eq!(DEMO.remove(dir.path()).unwrap(), LegacyPlist::Foreign);
        assert!(file.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_a_folder_or_an_unreadable_file_is_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.plist");
        std::fs::write(&target, plugin_plist("Demo", &[EXE], "")).unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("Demo.plist")).unwrap();
        assert_eq!(DEMO.remove(dir.path()).unwrap(), LegacyPlist::Foreign);
        assert!(target.exists());

        let other = tempfile::tempdir().unwrap();
        std::fs::create_dir(other.path().join("Demo.plist")).unwrap();
        assert_eq!(DEMO.remove(other.path()).unwrap(), LegacyPlist::Foreign);

        // Not valid UTF-8: not the plugin's file.
        let third = tempfile::tempdir().unwrap();
        std::fs::write(third.path().join("Demo.plist"), [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(DEMO.remove(third.path()).unwrap(), LegacyPlist::Foreign);
    }

    struct Calls(Vec<bool>, bool);
    impl Autostart for Calls {
        fn set(&mut self, on: bool) -> Result<(), String> {
            self.0.push(on);
            if self.1 { Err("denied".into()) } else { Ok(()) }
        }
    }

    #[test]
    fn a_removed_legacy_file_registers_the_new_entry_once_and_nothing_else_does() {
        let dir = tempfile::tempdir().unwrap();
        let old = plugin_plist("Demo", &[EXE, "--hidden"], "");
        let mut a = Calls(Vec::new(), false);
        // Nothing there: nothing registered.
        assert_eq!(
            DEMO.migrate(dir.path(), Ok(()), &mut a),
            Ok(LegacyPlist::Absent)
        );
        // A foreign file is kept and nothing is registered.
        std::fs::write(dir.path().join("Demo.plist"), "mine").unwrap();
        assert_eq!(
            DEMO.migrate(dir.path(), Ok(()), &mut a),
            Ok(LegacyPlist::Foreign)
        );
        assert!(a.0.is_empty());
        // The old file: removed, new entry registered.
        std::fs::write(dir.path().join("Demo.plist"), &old).unwrap();
        assert_eq!(
            DEMO.migrate(dir.path(), Ok(()), &mut a),
            Ok(LegacyPlist::Removed)
        );
        assert_eq!(a.0, vec![true]);
        // From an unstable place the file goes but nothing is registered.
        std::fs::write(dir.path().join("Demo.plist"), &old).unwrap();
        assert_eq!(
            DEMO.migrate(dir.path(), Err("x".into()), &mut a),
            Ok(LegacyPlist::Removed)
        );
        assert_eq!(a.0.len(), 1);
        // A refused registration is the caller's error.
        std::fs::write(dir.path().join("Demo.plist"), &old).unwrap();
        let mut refused = Calls(Vec::new(), true);
        assert_eq!(
            DEMO.migrate(dir.path(), Ok(()), &mut refused),
            Err("denied".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_cannot_be_read_is_the_callers_error() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the folder should be: looking inside it fails with "not a directory".
        let not_a_dir = dir.path().join("agents");
        std::fs::write(&not_a_dir, "x").unwrap();
        let mut a = Calls(Vec::new(), false);
        assert!(DEMO.migrate(&not_a_dir, Ok(()), &mut a).is_err());
        assert!(a.0.is_empty());
    }
}
