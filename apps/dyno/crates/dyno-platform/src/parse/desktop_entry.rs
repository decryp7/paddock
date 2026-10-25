//! XDG autostart `.desktop` files: the keys the startup manager reads, and the minimal edits it makes to a user's own
//! file when the user turns an entry on or off.

/// The keys of a desktop entry that matter at login.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DesktopEntry {
    /// `Name` (the unlocalised one).
    pub name: String,
    /// `Exec` line as written.
    pub exec: String,
    /// `Hidden=true`: the entry is deleted as far as the desktop is concerned (the standard way to disable it).
    pub hidden: bool,
    /// `X-GNOME-Autostart-enabled=false`.
    pub gnome_disabled: bool,
    /// `OnlyShowIn` / `NotShowIn` are not interpreted; `NoDisplay` is irrelevant for autostart.
    pub has_exec: bool,
}

impl DesktopEntry {
    /// Whether the desktop will run it at login.
    pub fn enabled(&self) -> bool {
        !self.hidden && !self.gnome_disabled
    }
}

fn truthy(v: &str) -> bool {
    v.trim().eq_ignore_ascii_case("true")
}

/// Parses the `[Desktop Entry]` group. Other groups and comments are ignored; never panics.
pub fn parse(text: &str) -> DesktopEntry {
    let mut e = DesktopEntry::default();
    let mut in_entry = false;
    for line in text.lines().take(2_000) {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        match k.trim() {
            "Name" => e.name = v.trim().to_string(),
            "Exec" => {
                e.exec = v.trim().to_string();
                e.has_exec = !e.exec.is_empty();
            }
            "Hidden" => e.hidden = truthy(v),
            "X-GNOME-Autostart-enabled" => e.gnome_disabled = !truthy(v),
            _ => {}
        }
    }
    e
}

/// The program an `Exec` line starts: the first word, quotes removed, with field codes (`%U`) ignored.
pub fn exec_program(exec: &str) -> Option<String> {
    let e = exec.trim();
    let first = if let Some(rest) = e.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        // `env VAR=x prog` is common: skip the env wrapper and assignments.
        let mut words = e.split_whitespace();
        let mut w = words.next()?;
        if w == "env" || w.ends_with("/env") {
            w = words.find(|w| !w.contains('='))?;
        }
        w
    };
    (!first.is_empty() && !first.starts_with('%')).then(|| first.to_string())
}

/// `text` with the entry turned off (`hidden`) or on: sets `Hidden` and removes `X-GNOME-Autostart-enabled` in the
/// `[Desktop Entry]` group, leaving every other line as it was. A file without the group gets one.
pub fn set_hidden(text: &str, hidden: bool) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_entry = false;
    let mut seen_group = false;
    let mut insert_at: Option<usize> = None;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            if in_entry && insert_at.is_none() {
                insert_at = Some(out.len());
            }
            in_entry = t == "[Desktop Entry]";
            seen_group |= in_entry;
            out.push(line.to_string());
            continue;
        }
        if in_entry {
            if let Some((k, _)) = t.split_once('=') {
                if matches!(k.trim(), "Hidden" | "X-GNOME-Autostart-enabled") {
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    let line = format!("Hidden={hidden}");
    if !seen_group {
        out.insert(0, "[Desktop Entry]".to_string());
        out.insert(1, line);
    } else {
        let at = insert_at.unwrap_or(out.len());
        // Keep trailing blank lines of the group after the new line.
        let mut at = at;
        while at > 0 && out[at - 1].trim().is_empty() {
            at -= 1;
        }
        out.insert(at, line);
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// A minimal user override that disables a system entry of the same file name.
pub fn override_hidden(name: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={}\nHidden=true\n",
        name.replace(['\n', '\r'], " ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# comment\n[Desktop Entry]\nType=Application\nName=Syncthing Tray\nName[de]=Sync\nExec=env FOO=1 /opt/sync/bin/tray --minimized %U\nX-GNOME-Autostart-enabled=false\n\n[Desktop Action x]\nName=Other\nExec=nope\n";

    #[test]
    fn parses_the_entry_group() {
        let e = parse(FILE);
        assert_eq!(e.name, "Syncthing Tray");
        assert!(e.exec.starts_with("env FOO=1"));
        assert!(e.has_exec && e.gnome_disabled && !e.hidden && !e.enabled());
        assert!(parse("[Desktop Entry]\nExec=x\nHidden=TRUE").hidden);
        assert!(parse("[Desktop Entry]\nExec=x").enabled());
        assert!(parse("[Desktop Entry]\nExec=x\nX-GNOME-Autostart-enabled=true").enabled());
        assert!(!parse("").has_exec);
        assert!(!parse("Exec=x\nHidden=true").hidden, "keys outside the group are ignored");
    }

    #[test]
    fn programs_of_exec_lines() {
        assert_eq!(
            exec_program("env FOO=1 /opt/sync/bin/tray --minimized %U"),
            Some("/opt/sync/bin/tray".into())
        );
        assert_eq!(exec_program("\"/opt/My App/run\" --x"), Some("/opt/My App/run".into()));
        assert_eq!(exec_program("firefox %u"), Some("firefox".into()));
        assert_eq!(exec_program("%U"), None);
        assert_eq!(exec_program(""), None);
        assert_eq!(exec_program("env A=b"), None);
    }

    #[test]
    fn hiding_and_showing_edits_only_the_flags() {
        let hidden = set_hidden(FILE, true);
        let e = parse(&hidden);
        assert!(e.hidden && !e.gnome_disabled);
        assert!(hidden.contains("Name[de]=Sync"), "{hidden}");
        assert!(hidden.contains("[Desktop Action x]\nName=Other"));
        // Hidden= lands inside the entry group, before the next group.
        let at = hidden.find("Hidden=true").unwrap();
        assert!(at < hidden.find("[Desktop Action x]").unwrap());
        let shown = set_hidden(&hidden, false);
        assert!(parse(&shown).enabled());
        assert_eq!(shown.matches("Hidden=").count(), 1);
        let bare = set_hidden("Name=x", true);
        assert!(bare.starts_with("[Desktop Entry]\nHidden=true\n") && parse(&bare).hidden, "{bare}");
        assert!(parse(&set_hidden("[Desktop Entry]\nName=x", true)).hidden);
        let ov = override_hidden("A\nB");
        assert!(parse(&ov).hidden && !parse(&ov).has_exec && ov.contains("Name=A B"));
    }

    #[test]
    fn junk_never_panics() {
        for s in ["", "[", "[Desktop Entry", "=\n=\n", "[Desktop Entry]\n=\nHidden", "\u{0}\u{ffff}"] {
            let _ = (parse(s), exec_program(s), set_hidden(s, true), set_hidden(s, false));
        }
    }
}
