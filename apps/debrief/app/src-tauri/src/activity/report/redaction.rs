//! Removes secrets and personal data from the logs of a report, with the shared redactor (`paddock_diag`): secrets
//! (tokens, passwords, keys, URL credentials, card numbers), user names, home folders, the machine's name, e-mail and
//! IP addresses. Debrief adds MAC addresses, which the shared layers leave alone.
//!
//! Every log line passes here before the report counts, previews or writes it (a CLEF line is parsed and each string
//! property redacted, so a replacement never breaks the line; anything else is redacted as text).

use paddock_diag::{RedactionCounts, Redactor};

/// What replaces a MAC address.
pub const MAC: &str = "<mac>";

/// A MAC address written with colons or with hyphens (`aa:bb:cc:dd:ee:ff`, `AA-BB-CC-DD-EE-FF`). The `regex` crate has no
/// back-references, so there is one pattern per separator.
const MAC_PATTERNS: [&str; 2] = [r"(?i)\b[0-9a-f]{2}(?::[0-9a-f]{2}){5}\b", r"(?i)\b[0-9a-f]{2}(?:-[0-9a-f]{2}){5}\b"];

/// The redactor for the given user names, home folders and machine names (tests pass fixed ones).
pub fn redactor_for(users: &[String], homes: &[String], machines: &[String]) -> Redactor {
    let mut r = Redactor::new().with_personal(users, homes, machines);
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
    machines.extend(sysinfo::System::host_name());
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
        add(&mut counts, &c);
    }
    (out.into_bytes(), counts)
}

fn add(into: &mut RedactionCounts, o: &RedactionCounts) {
    into.secrets += o.secrets;
    into.fields += o.fields;
    into.home_paths += o.home_paths;
    into.profile_paths += o.profile_paths;
    into.machine_names += o.machine_names;
    into.user_names += o.user_names;
    into.emails += o.emails;
    into.ip_addresses += o.ip_addresses;
}

/// " (N secrets and personal details removed)", or nothing when none were found.
pub fn removed_note(counts: &RedactionCounts) -> String {
    match counts.total() {
        0 => String::new(),
        n => format!("; {n} secrets and personal details removed"),
    }
}
