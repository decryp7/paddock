//! macOS launch agent property lists (`~/Library/LaunchAgents/*.plist`).

use serde::Deserialize;

/// The keys of a launch agent that the startup manager uses.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LaunchAgent {
    /// The job's label (its identity for `launchctl`).
    pub label: String,
    /// The program, when given separately from the arguments.
    #[serde(default)]
    pub program: Option<String>,
    /// The program and its arguments.
    #[serde(default)]
    pub program_arguments: Vec<String>,
    /// Disabled in the plist itself.
    #[serde(default)]
    pub disabled: bool,
    /// Started when the user logs in.
    #[serde(default)]
    pub run_at_load: bool,
    /// Kept running (`true` or a dictionary of conditions).
    #[serde(default)]
    pub keep_alive: Option<plist::Value>,
}

impl LaunchAgent {
    /// The executable it starts.
    pub fn executable(&self) -> Option<&str> {
        self.program.as_deref().or(self.program_arguments.first().map(String::as_str))
    }

    /// The command line it runs (arguments with spaces are quoted).
    pub fn command(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(p) = &self.program {
            if self.program_arguments.first() != Some(p) {
                parts.push(p.clone());
            }
        }
        parts.extend(self.program_arguments.iter().cloned());
        parts.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" ")
    }

    /// Whether launchd starts it at login (RunAtLoad, or KeepAlive true / with conditions).
    pub fn starts_at_login(&self) -> bool {
        self.run_at_load
            || match &self.keep_alive {
                Some(plist::Value::Boolean(b)) => *b,
                Some(plist::Value::Dictionary(_)) => true,
                _ => false,
            }
    }
}

/// Largest plist read.
pub const MAX_PLIST_BYTES: usize = 1 << 20;

/// Parses a launch agent plist (XML or binary). `None` when it isn't one.
pub fn parse_launch_agent(bytes: &[u8]) -> Option<LaunchAgent> {
    if bytes.len() > MAX_PLIST_BYTES {
        return None;
    }
    plist::from_bytes::<LaunchAgent>(bytes).ok().filter(|a| !a.label.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xml(body: &str) -> Vec<u8> {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>{body}</dict></plist>"
        )
        .into_bytes()
    }

    #[test]
    fn reads_agents() {
        let a = parse_launch_agent(&xml(
            "<key>Label</key><string>com.example.sync</string><key>ProgramArguments</key><array>\
<string>/Applications/Sync App.app/Contents/MacOS/sync</string><string>--bg</string></array>\
<key>RunAtLoad</key><true/>",
        ))
        .unwrap();
        assert_eq!(a.label, "com.example.sync");
        assert_eq!(a.executable(), Some("/Applications/Sync App.app/Contents/MacOS/sync"));
        assert_eq!(a.command(), "\"/Applications/Sync App.app/Contents/MacOS/sync\" --bg");
        assert!(a.starts_at_login());
        assert!(!a.disabled);

        let b = parse_launch_agent(&xml(
            "<key>Label</key><string>b</string><key>Program</key><string>/usr/local/bin/b</string>\
<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict><key>Disabled</key><true/>",
        ))
        .unwrap();
        assert_eq!(b.command(), "/usr/local/bin/b");
        assert!(b.starts_at_login());
        assert!(b.disabled);

        let c = parse_launch_agent(&xml("<key>Label</key><string>c</string><key>KeepAlive</key><false/>")).unwrap();
        assert!(!c.starts_at_login());
        assert_eq!(c.executable(), None);
        let d = parse_launch_agent(&xml("<key>Label</key><string>d</string><key>KeepAlive</key><true/>")).unwrap();
        assert!(d.starts_at_login());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_launch_agent(b"").is_none());
        assert!(parse_launch_agent(b"<plist><array/></plist>").is_none());
        assert!(parse_launch_agent(&xml("<key>Label</key><string></string>")).is_none());
        assert!(parse_launch_agent(&vec![b' '; MAX_PLIST_BYTES + 1]).is_none());
    }
}
