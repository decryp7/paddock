//! The one rule for "is this folder on an external volume", and the boot volume's temp folder for programs that macOS treats as their
//! own responsible process (ER-21, ER-22). macOS TCC (Removable Volumes) asks the user, or denies silently, when such a program (a
//! child started with the responsibility disclaim, a `launchctl submit` job) touches a folder on an external disk; Full Disk Access of
//! the parent does not help (docs/lessons.md, 2026-11-04 and 2026-11-11).
//!
//! The rule is pure: what it reads from the machine (`df`, `diskutil`, `getconf` and the folders it looks at) sits behind [`Probe`], so
//! every branch is tested with a fake machine. Two adapters run the real programs, each with its own bound: `dev ci boot-volume` (the
//! CI tool, `tools/paddock-dev/src/ci_host/boot_volume.rs`) and `paddock_testkit::boot_volume`. Dyno's `check-bundle.sh` and CI's
//! job-tmpdir step use the command. It replaces `conformance/ci/boot-volume.sh`.

/// What the rule reads from the machine.
pub trait Probe {
    /// Whether `p` is a folder.
    fn is_dir(&self, p: &str) -> bool;
    /// `cd <p> && pwd -P`: the physical path of a folder; `None` when it cannot be entered.
    fn physical(&self, p: &str) -> Option<String>;
    /// The mount point `df -P <dir>` names (the text after its fifth field on the second line); `None` when it names none.
    fn mount_of(&self, dir: &str) -> Option<String>;
    /// `diskutil info -plist <mount>`; `None` when it fails or is not there.
    fn plist_of(&self, mount: &str) -> Option<String>;
    /// `getconf DARWIN_USER_TEMP_DIR`; `None` when it fails.
    fn user_temp(&self) -> Option<String>;
}

/// The folder itself when it is one, else its nearest existing parent (`.` for a bare relative name, `/` at the top): a folder that is
/// about to be made sits on the volume of that parent.
pub fn nearest_existing(probe: &dyn Probe, path: &str) -> String {
    let mut d = path.to_string();
    while !probe.is_dir(&d) {
        let up = match d.rfind('/') {
            Some(0) => "/".to_string(),
            Some(i) => d[..i].to_string(),
            None => ".".to_string(),
        };
        // The top ("/") and the current folder (".") are where the climb ends, whether or not they can be listed.
        if up == d {
            break;
        }
        d = up;
    }
    d
}

/// True when the plist says `Internal` is false (`<key>Internal</key>` followed by `<false/>`).
pub fn plist_says_external(plist: &str) -> bool {
    let mut lines = plist.lines();
    while let Some(l) = lines.next() {
        if l.contains("<key>Internal</key>") && lines.next().is_some_and(|n| n.contains("<false/>"))
        {
            return true;
        }
    }
    false
}

/// The mount point out of `df -P` output: the second line without its first five fields (it may hold spaces).
pub fn mount_from_df(text: &str) -> Option<String> {
    let mut rest = text.lines().nth(1)?;
    for _ in 0..5 {
        rest = rest
            .trim_start()
            .split_once(char::is_whitespace)
            .map_or("", |(_, r)| r);
    }
    let mount = rest.trim();
    (!mount.is_empty()).then(|| mount.to_string())
}

/// Whether the volume holding `folder` is external. When nothing can be judged, why goes to `notes` (standard error) and the answer is
/// "internal" (false).
pub fn volume_is_external(probe: &dyn Probe, folder: &str, notes: &mut Vec<String>) -> bool {
    let base = nearest_existing(probe, folder);
    let Some(dir) = probe.physical(&base) else {
        notes.push(format!(
            "boot-volume: cannot judge {folder}: its folder {base} cannot be entered; treated as internal"
        ));
        return false;
    };
    let Some(mount) = probe.mount_of(&dir) else {
        notes.push(format!(
            "boot-volume: cannot judge {dir}: df names no mount; treated as internal"
        ));
        return false;
    };
    probe
        .plist_of(&mount)
        .is_some_and(|p| plist_says_external(&p))
}

/// The folder to use for a program that is its own responsible process: `folder` (default `tmpdir`, else `/tmp`) when it is not on an
/// external volume, else the boot volume's per-user temp folder. Returns the folder and the lines for standard error.
pub fn boot_volume_temp(
    probe: &dyn Probe,
    folder: Option<&str>,
    tmpdir: Option<&str>,
) -> (String, Vec<String>) {
    let mut notes = Vec::new();
    let given = folder
        .filter(|f| !f.is_empty())
        .or(tmpdir.filter(|t| !t.is_empty()))
        .unwrap_or("/tmp");
    let mut dir = given.strip_suffix('/').unwrap_or(given).to_string();
    if dir.is_empty() {
        dir = "/".to_string();
    }
    if !volume_is_external(probe, &dir, &mut notes) {
        return (dir, notes);
    }
    let user = probe.user_temp().unwrap_or_default();
    let user = user.strip_suffix('/').unwrap_or(&user).to_string();
    if user.starts_with('/') && probe.is_dir(&user) && !volume_is_external(probe, &user, &mut notes)
    {
        notes.push(format!(
            "boot-volume: using {user} instead of {dir}: {dir} is on an external volume, where macOS (TCC, Removable Volumes) prompts for or denies programs that are their own responsible process"
        ));
        return (user, notes);
    }
    let shown = if user.is_empty() {
        "nothing"
    } else {
        user.as_str()
    };
    notes.push(format!(
        "::warning::boot-volume: {dir} is on an external volume and getconf DARWIN_USER_TEMP_DIR gave no internal folder ({shown}): macOS may prompt for removable-volume access. Next step: point TMPDIR at the internal disk"
    ));
    (dir, notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap as Map, HashSet};

    /// A scripted machine: the folders that exist, the mount each is on, which mounts are external, what getconf says.
    #[derive(Default)]
    struct Fake {
        dirs: HashSet<String>,
        mounts: Map<String, String>,
        external: HashSet<String>,
        plist_fails: bool,
        getconf: Option<String>,
    }

    impl Fake {
        fn folder(mut self, dir: &str, mount: &str) -> Fake {
            self.dirs.insert(dir.to_string());
            self.mounts.insert(dir.to_string(), mount.to_string());
            self
        }
    }

    impl Probe for Fake {
        fn is_dir(&self, p: &str) -> bool {
            self.dirs.contains(p)
        }
        fn physical(&self, p: &str) -> Option<String> {
            self.dirs.contains(p).then(|| p.to_string())
        }
        fn mount_of(&self, dir: &str) -> Option<String> {
            self.mounts.get(dir).filter(|m| !m.is_empty()).cloned()
        }
        fn plist_of(&self, mount: &str) -> Option<String> {
            if self.plist_fails {
                return None;
            }
            let flag = if self.external.contains(mount) {
                "<false/>"
            } else {
                "<true/>"
            };
            Some(format!("<dict>\n\t<key>Internal</key>\n\t{flag}\n</dict>"))
        }
        fn user_temp(&self) -> Option<String> {
            self.getconf.clone()
        }
    }

    fn external(f: &Fake, folder: &str) -> (bool, Vec<String>) {
        let mut notes = Vec::new();
        (volume_is_external(f, folder, &mut notes), notes)
    }

    #[test]
    fn a_folder_that_does_not_exist_yet_sits_on_the_volume_of_its_nearest_parent() {
        let mut f = Fake::default().folder("/Volumes/ci", "/Volumes/ci");
        f.external.insert("/Volumes/ci".into());
        assert_eq!(nearest_existing(&f, "/Volumes/ci/a/b/c"), "/Volumes/ci");
        assert_eq!(nearest_existing(&f, "/nothing/here"), "/");
        assert_eq!(nearest_existing(&f, "bare"), ".");
        assert_eq!(external(&f, "/Volumes/ci/a/b/c"), (true, vec![]));
    }

    #[test]
    fn what_cannot_be_judged_is_internal_and_says_why() {
        let none = Fake::default();
        let (ext, notes) = external(&none, "/x");
        assert!(!ext);
        assert_eq!(
            notes,
            ["boot-volume: cannot judge /x: its folder / cannot be entered; treated as internal"]
        );
        let nomount = Fake::default().folder("/a", "");
        let (ext, notes) = external(&nomount, "/a");
        assert!(!ext);
        assert_eq!(
            notes,
            ["boot-volume: cannot judge /a: df names no mount; treated as internal"]
        );
        // diskutil fails: internal, with no note (as the shell's `|| return 1`).
        let mut broken = Fake::default().folder("/a", "/a");
        broken.plist_fails = true;
        assert_eq!(external(&broken, "/a"), (false, vec![]));
    }

    #[test]
    fn the_temp_folder_moves_to_the_boot_volume_only_when_the_given_one_is_external() {
        let mut f = Fake::default()
            .folder("/Volumes/ci/tmp", "/Volumes/ci")
            .folder("/var/folders/u/T", "/");
        f.external.insert("/Volumes/ci".into());
        f.getconf = Some("/var/folders/u/T/".into());
        let (dir, notes) = boot_volume_temp(&f, Some("/Volumes/ci/tmp/"), None);
        assert_eq!(dir, "/var/folders/u/T");
        assert_eq!(notes.len(), 1);
        assert!(
            notes[0]
                .starts_with("boot-volume: using /var/folders/u/T instead of /Volumes/ci/tmp: "),
            "{notes:?}"
        );
        // An internal folder is kept as it is, and the default is $TMPDIR, then /tmp.
        let g = Fake::default()
            .folder("/var/folders/u/T", "/")
            .folder("/tmp", "/");
        assert_eq!(
            boot_volume_temp(&g, None, Some("/var/folders/u/T/")),
            ("/var/folders/u/T".into(), vec![])
        );
        assert_eq!(boot_volume_temp(&g, None, None), ("/tmp".into(), vec![]));
    }

    #[test]
    fn without_an_internal_folder_from_getconf_the_given_one_stays_and_macos_may_prompt() {
        for getconf in [
            None,
            Some("relative".to_string()),
            Some("/missing/".to_string()),
        ] {
            let mut f = Fake::default().folder("/Volumes/ci/tmp", "/Volumes/ci");
            f.external.insert("/Volumes/ci".into());
            f.getconf = getconf.clone();
            let (dir, notes) = boot_volume_temp(&f, Some("/Volumes/ci/tmp"), None);
            assert_eq!(dir, "/Volumes/ci/tmp", "{getconf:?}");
            let shown = getconf
                .map(|g| g.trim_end_matches('/').to_string())
                .unwrap_or_else(|| "nothing".into());
            assert!(
                notes.iter().any(|n| n.starts_with(
                    "::warning::boot-volume: /Volumes/ci/tmp is on an external volume"
                ) && n
                    .contains(&format!("gave no internal folder ({shown})"))
                    && n.ends_with("Next step: point TMPDIR at the internal disk")),
                "{notes:?}"
            );
        }
        // getconf names a folder that is itself external: not used.
        let mut f = Fake::default()
            .folder("/Volumes/ci/tmp", "/Volumes/ci")
            .folder("/Volumes/other", "/Volumes/other");
        f.external
            .extend(["/Volumes/ci".to_string(), "/Volumes/other".to_string()]);
        f.getconf = Some("/Volumes/other".into());
        assert_eq!(
            boot_volume_temp(&f, Some("/Volumes/ci/tmp"), None).0,
            "/Volumes/ci/tmp"
        );
    }

    #[test]
    fn df_output_gives_the_mount_after_its_five_fields_spaces_kept() {
        assert_eq!(
            mount_from_df(
                "Filesystem 512-blocks Used Available Capacity Mounted on\n/dev/disk3s5 100 1 99 1% /System/Volumes/Data\n"
            ),
            Some("/System/Volumes/Data".into())
        );
        assert_eq!(
            mount_from_df("h\n/dev/d 1 2 3 4% /Volumes/My Disk\n"),
            Some("/Volumes/My Disk".into())
        );
        assert_eq!(mount_from_df("only a header\n"), None);
        assert_eq!(mount_from_df("h\n/dev/d 1 2 3 4%\n"), None);
    }
}
