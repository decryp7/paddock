//! Removes secrets and personal data from everything in a report but the user's own description, with the shared redactor
//! (`paddock_diag`): secrets (tokens, passwords, keys, URL credentials, card numbers), user names, home folders, the machine's
//! name, e-mail and IP addresses (loopback and private network addresses are kept: they show how the machine is set up).
//! Debrief adds MAC addresses, which the shared layers leave alone.
//!
//! Every log line and every JSON value passes here before the report counts, previews or writes it (a CLEF line is parsed and
//! each string property redacted, so a replacement never breaks the line; a JSON file is redacted as a value, string by
//! string, then printed; anything else is redacted as text).

use paddock_diag::{RedactionCounts, Redactor};
use serde::Serialize;

/// What replaces a MAC address.
pub const MAC: &str = "<mac>";

/// A MAC address written with colons or with hyphens (`aa:bb:cc:dd:ee:ff`, `AA-BB-CC-DD-EE-FF`). The `regex` crate has no
/// back-references, so there is one pattern per separator.
const MAC_PATTERNS: [&str; 2] = [r"(?i)\b[0-9a-f]{2}(?::[0-9a-f]{2}){5}\b", r"(?i)\b[0-9a-f]{2}(?:-[0-9a-f]{2}){5}\b"];

/// The redactor for the given user names, home folders and machine names (tests pass fixed ones). The report is read and saved
/// on this machine, so loopback and private network addresses stay in it (they show how the machine is set up); public
/// ones, CGNAT and unique-local IPv6 (Tailscale) go.
pub fn redactor_for(users: &[String], homes: &[String], machines: &[String]) -> Redactor {
    let mut r = Redactor::new().with_personal(users, homes, machines).keep_local_addresses();
    for pattern in MAC_PATTERNS {
        // Constant patterns well inside the shared limits: a refusal is a bug caught by every test of this module.
        r = r.with_pattern(pattern, MAC).expect("the MAC patterns are valid");
    }
    r
}

/// Values of the first of `names` that are set in the environment.
fn env_values(names: &[&str]) -> Vec<String> {
    names.iter().filter_map(|n| std::env::var(n).ok()).collect()
}

/// The redactor for this machine: the user, home folder and name found in the environment, the OS home folder and the
/// host name.
pub fn machine_redactor() -> Redactor {
    let users = env_values(&["USER", "USERNAME", "LOGNAME"]);
    let mut homes = env_values(&["HOME", "USERPROFILE"]);
    homes.extend(dirs::home_dir().map(|h| h.to_string_lossy().into_owned()));
    let mut machines = env_values(&["HOSTNAME", "COMPUTERNAME", "HOST"]);
    machines.extend(sysinfo::System::host_name().ok());
    redactor_for(&users, &homes, &machines)
}

/// A log's bytes with every line redacted. Line breaks (and a missing final one) are kept; a cut first line, as a tail
/// starts mid-line, is redacted as text.
pub fn redact_log(redactor: &Redactor, bytes: &[u8]) -> (Vec<u8>, RedactionCounts) {
    let text = String::from_utf8_lossy(bytes);
    let mut counts = RedactionCounts::default();
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if line.trim().is_empty() {
            out.push_str(line);
            continue;
        }
        let (redacted, c) = redactor.log_line(line);
        out.push_str(&redacted);
        counts.add(&c);
    }
    (out.into_bytes(), counts)
}

/// What a JSON file of a report holds: `value` (any serialisable data) redacted as a [`Value`], then pretty-printed. The
/// text is never redacted after printing, where a JSON-escaped path (`C:\\Users\\x`) would read differently.
pub fn redacted_json(redactor: &Redactor, value: &impl Serialize) -> (Vec<u8>, RedactionCounts) {
    let Ok(value) = serde_json::to_value(value) else {
        return (Vec::new(), RedactionCounts::default());
    };
    // Values only: the keys of these files are fixed names, and redacting a key could merge two into one.
    let (value, counts) = redactor.text_in_json(&value);
    (serde_json::to_vec_pretty(&value).unwrap_or_default(), counts)
}

/// The steps to reproduce with each step redacted.
pub fn redact_steps(redactor: &Redactor, steps: &[String]) -> (Vec<String>, RedactionCounts) {
    let mut counts = RedactionCounts::default();
    let out = steps
        .iter()
        .map(|s| {
            let (t, c) = redactor.text(s);
            counts.add(&c);
            t
        })
        .collect();
    (out, counts)
}

/// " (N secrets and personal details removed)", or nothing when none were found.
pub fn removed_note(counts: &RedactionCounts) -> String {
    match counts.total() {
        0 => String::new(),
        n => format!("; {n} secrets and personal details removed"),
    }
}
