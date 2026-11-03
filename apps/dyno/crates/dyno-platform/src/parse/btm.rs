//! `sfltool dumpbtm`: the Background Task Management database that System Settings > General > Login Items & Extensions shows.
//! Its text is not a documented format, so the parser is strict about what it must see (a `Records for UID` header) and says so
//! when it does not, instead of returning an empty list as if the user had no login items.

/// What kind of background item a record is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtmKind {
    /// An app registered as a login item (`Type: login item`).
    LoginItem,
    /// A launch agent an app registered through `SMAppService` (`Type: agent`).
    Agent,
}

/// One login item or agent of one user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtmItem {
    /// The record's kind.
    pub kind: BtmKind,
    /// The name shown in System Settings (`Name`), or the identifier when it has none.
    pub name: String,
    /// BTM's identifier (`Identifier`), unique per kind.
    pub identifier: String,
    /// Whether the user has it switched on (`Disposition` has `enabled`).
    pub enabled: bool,
    /// The program, when BTM names one (`Executable Path`, else a file `URL`).
    pub exe: Option<String>,
}

/// Why the text could not be read as a BTM dump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtmError {
    /// No `Records for UID` line: the tool printed something else (an error, a refusal, a changed format).
    NoRecords {
        /// The first non-empty line of the text, for the log (never user data: BTM prints tool text before records).
        first_line: String,
    },
}

fn value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.trim_start().strip_prefix(key)?.strip_prefix(':')?;
    let v = rest.trim();
    (!v.is_empty() && v != "(null)").then_some(v)
}

/// The login items and agents recorded for user `uid`, in file order. `Ok(vec![])` means the dump has that user's records and
/// they hold none; a dump without any records is an error.
pub fn parse_dumpbtm(text: &str, uid: u32) -> Result<Vec<BtmItem>, BtmError> {
    struct Raw {
        kind: Option<BtmKind>,
        name: Option<String>,
        identifier: Option<String>,
        enabled: bool,
        exe: Option<String>,
        url: Option<String>,
    }
    let mut seen_header = false;
    let mut in_user = false;
    let mut cur: Option<Raw> = None;
    let mut out: Vec<BtmItem> = Vec::new();
    let mut finish = |cur: &mut Option<Raw>| {
        if let Some(r) = cur.take() {
            if let (Some(kind), Some(identifier)) = (r.kind, r.identifier) {
                let exe = r
                    .exe
                    .or_else(|| r.url.map(|u| u.strip_prefix("file://").map(str::to_string).unwrap_or(u)));
                let name = r.name.unwrap_or_else(|| identifier.clone());
                let item = BtmItem {
                    kind,
                    name,
                    identifier,
                    enabled: r.enabled,
                    exe,
                };
                // The dump lists a user's records twice on some builds: keep the first of each identifier.
                if !out.iter().any(|o| o.kind == item.kind && o.identifier == item.identifier) {
                    out.push(item);
                }
            }
        }
    };
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("Records for UID") {
            finish(&mut cur);
            seen_header = true;
            in_user = rest.split_whitespace().next().and_then(|n| n.parse::<i64>().ok()) == Some(i64::from(uid));
            continue;
        }
        if !in_user {
            continue;
        }
        let t = line.trim();
        let indent = line.len() - line.trim_start().len();
        // "#12:" alone starts an item; "#1: 16.com.x" deeper in is an embedded identifier of the item above.
        if indent <= 2 && t.starts_with('#') && t.ends_with(':') && t[1..t.len() - 1].chars().all(|c| c.is_ascii_digit()) {
            finish(&mut cur);
            cur = Some(Raw {
                kind: None,
                name: None,
                identifier: None,
                enabled: false,
                exe: None,
                url: None,
            });
            continue;
        }
        let Some(r) = cur.as_mut() else { continue };
        if let Some(v) = value(line, "Type") {
            r.kind = if v.starts_with("login item") {
                Some(BtmKind::LoginItem)
            } else if v.starts_with("agent") {
                Some(BtmKind::Agent)
            } else {
                None
            };
        } else if let Some(v) = value(line, "Name") {
            r.name = Some(v.to_string());
        } else if let Some(v) = value(line, "Identifier") {
            r.identifier = Some(v.to_string());
        } else if let Some(v) = value(line, "Disposition") {
            r.enabled = v.trim_start_matches('[').split([',', ']']).next().map(str::trim) == Some("enabled");
        } else if let Some(v) = value(line, "Executable Path") {
            r.exe = Some(v.to_string());
        } else if let Some(v) = value(line, "URL") {
            r.url = Some(v.to_string());
        }
    }
    finish(&mut cur);
    if !seen_header {
        let first_line = text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .chars()
            .take(120)
            .collect();
        return Err(BtmError::NoRecords { first_line });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Invented, same-shape dump: two users, a login item, an agent, a legacy agent and an app record.
    const DUMP: &str = "========================\n Records for UID -2 : FFFFEEEE-DDDD-CCCC-BBBB-AAAAFFFFFFFE\n========================\n\n\
 ServiceManagement migrated: true\n\n Items:\n\n #1:\n                 UUID: AAAA\n                 Name: Shared Thing\n\
                 Type: login item (0x4)\n          Disposition: [enabled, allowed, notified] (0xb)\n           Identifier: 4.com.shared.thing\n\n\
========================\n Records for UID 501 : DB36FC03-D7FF-4307-86BC-A24C35EC3330\n========================\n\n Items:\n\n #1:\n\
                 UUID: BBBB\n                 Name: Example Sync\n       Developer Name: Example Ltd.\n                 Type: login item (0x4)\n\
                Flags: [  ] (0)\n          Disposition: [enabled, allowed, notified] (0xb)\n           Identifier: 4.com.example.sync\n\
                  URL: Contents/Library/LoginItems/Sync.app\n      Executable Path: /Applications/Example Sync.app/Contents/Library/LoginItems/Sync.app/Contents/MacOS/Sync\n\n #2:\n\
                 UUID: CCCC\n                 Name: (null)\n                 Type: agent (0x8)\n\
          Disposition: [disabled, allowed, not notified] (0x2)\n           Identifier: 8.com.example.helper.agent\n\
                  URL: file:///Applications/Example.app/Contents/Library/LaunchAgents/helper.plist\n\n #3:\n\
                 UUID: DDDD\n                 Name: Example\n                 Type: app (0x2)\n           Identifier: 2.com.example\n\
  Embedded Item Identifiers:\n    #1: 4.com.example.sync\n    #2: 8.com.example.helper.agent\n\n #4:\n\
                 UUID: EEEE\n                 Name: oldstyle\n                 Type: legacy agent (0x10008)\n\
          Disposition: [enabled, allowed, notified] (0xb)\n           Identifier: 16.com.example.old\n";

    #[test]
    fn reads_login_items_and_agents_of_one_user() {
        let items = parse_dumpbtm(DUMP, 501).unwrap();
        assert_eq!(
            items.len(),
            2,
            "the app record, the legacy agent and the other user's item are not login items"
        );
        assert_eq!(
            (items[0].kind, items[0].name.as_str(), items[0].enabled),
            (BtmKind::LoginItem, "Example Sync", true)
        );
        assert_eq!(
            items[0].exe.as_deref(),
            Some("/Applications/Example Sync.app/Contents/Library/LoginItems/Sync.app/Contents/MacOS/Sync")
        );
        assert_eq!(
            (items[1].kind, items[1].name.as_str(), items[1].enabled),
            (BtmKind::Agent, "8.com.example.helper.agent", false)
        );
        assert_eq!(
            items[1].exe.as_deref(),
            Some("/Applications/Example.app/Contents/Library/LaunchAgents/helper.plist")
        );
    }

    #[test]
    fn a_user_without_login_items_is_an_empty_list_not_an_error() {
        assert_eq!(parse_dumpbtm(DUMP, 777).unwrap(), vec![]);
    }

    #[test]
    fn text_without_records_is_an_error_naming_what_the_tool_said() {
        let err = parse_dumpbtm("\nsfltool: unrecognized subcommand\n", 501).unwrap_err();
        assert_eq!(
            err,
            BtmError::NoRecords {
                first_line: "sfltool: unrecognized subcommand".into()
            }
        );
        assert!(parse_dumpbtm("", 501).is_err());
    }

    #[test]
    fn a_repeated_user_section_does_not_repeat_items() {
        let twice = format!(
            "{DUMP}{}",
            DUMP.split("========================\n Records for UID 501")
                .nth(1)
                .map(|s| format!("========================\n Records for UID 501{s}"))
                .unwrap()
        );
        assert_eq!(parse_dumpbtm(&twice, 501).unwrap().len(), 2);
    }
}
