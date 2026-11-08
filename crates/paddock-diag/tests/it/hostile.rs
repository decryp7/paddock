//! Hostile input: invented lines a real machine's logs and settings could hold (ER-14: nothing here is real). Every line is
//! judged, in plain text, in a CLEF log line and nested in a CLEF array: the secret and personal details must not survive, and
//! what the redactor deliberately leaves is listed (and shown to the user by the dialog), so the list stays true.

use paddock_diag::Redactor;
use paddock_env::MapVars;
use serde_json::json;

/// What the redactor does with a line.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Verdict {
    /// The needle must be gone.
    Removed,
    /// The needle is left on purpose: the reason is the line in the "Not in the report" list that tells the user.
    Left(&'static str),
}
use Verdict::{Left, Removed};

const NAMES: &str = "File and folder names in paths stay";
const OTHER: &str = "Other machines' names and identifiers stay";
const CONTENT: &str = "Content the app imported or showed is not rewritten";

/// (label, text, needle, verdict).
const HOSTILE: &[(&str, &str, &str, Verdict)] = &[
    (
        "this user's home path",
        "/Users/alex/Documents/taxes.pdf",
        "alex",
        Removed,
    ),
    (
        "file name under home",
        "/Users/alex/Documents/taxes.pdf",
        "taxes.pdf",
        Left(NAMES),
    ),
    (
        "other user's profile",
        "/Users/sam/Library/x",
        "sam",
        Removed,
    ),
    (
        "profile with a space (mac)",
        "/Users/Alex Doe/Documents/plan.docx",
        "Doe",
        Removed,
    ),
    (
        "profile with a space (win)",
        r"C:\Users\Bob Smith\AppData\x",
        "Smith",
        Removed,
    ),
    (
        "Debug-escaped win path, other user",
        r#"open "C:\\Users\\carol\\salary.xlsx" failed"#,
        "carol",
        Removed,
    ),
    ("upper-case USERS", r"C:\USERS\dave\x", "dave", Removed),
    (
        "file on another volume",
        "/Volumes/Work/Client-Acme/contract.docx",
        "Client-Acme",
        Left(NAMES),
    ),
    (
        "UNC server name",
        r"\\FILESRV01\hr\payroll.xlsx",
        "FILESRV01",
        Left(OTHER),
    ),
    (
        "machine name (env has no HOSTNAME)",
        "connected to ALEX-MBP.local",
        "ALEX-MBP",
        Removed,
    ),
    (
        "other host FQDN in URL",
        "https://intranet.corp.example.com/x",
        "intranet.corp",
        Left(OTHER),
    ),
    (
        "MAC address",
        "en0 aa:bb:cc:dd:ee:ff up",
        "aa:bb:cc:dd:ee:ff",
        Removed,
    ),
    (
        "MAC address, Windows form",
        "adapter AA-BB-CC-DD-EE-FF up",
        "AA-BB-CC-DD-EE-FF",
        Removed,
    ),
    (
        "machine or hardware UUID",
        "machine id 123e4567-e89b-12d3-a456-426614174000",
        "123e4567",
        Left(OTHER),
    ),
    (
        "disk serial",
        "disk serial S4EVNF0M123456A",
        "S4EVNF0M123456A",
        Left(OTHER),
    ),
    ("IPv4 with port", "peer 10.1.2.3:8080", "10.1.2.3", Removed),
    ("IPv6 with zone", "peer fe80::1%en0", "fe80::1", Removed),
    ("e-mail", "from alex.doe@example.com", "alex.doe", Removed),
    (
        "keyed API key",
        "apiKey=pw_abc123def456",
        "abc123def456",
        Removed,
    ),
    (
        "bare Pitwall key",
        "using pw_Q2hhbmdlTWVQbGVhc2VJbnZlbnRlZEtleURhdGExMjM0NTY3",
        "Q2hhbmdlTWVQ",
        Removed,
    ),
    (
        "bare MCP key",
        "pasted pmk_Zm9vYmFyYmF6cXV4MTIzNDU2Nzg5MEFCQ0RFRg into a chat",
        "Zm9vYmFyYmF6",
        Removed,
    ),
    (
        "MCP key in an environment line",
        "DYNO_MCP_KEY=pmk_Zm9vYmFyYmF6cXV4MTIzNDU2Nzg5MEFCQ0RFRg",
        "Zm9vYmFyYmF6",
        Removed,
    ),
    (
        "bare GitHub token",
        "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
        "ghp_abcdefgh",
        Removed,
    ),
    (
        "bare AWS key id",
        "AKIAIOSFODNN7EXAMPLE",
        "AKIAIOSFODNN7",
        Removed,
    ),
    (
        "PEM private key body",
        "-----BEGIN OPENSSH PRIVATE KEY----- b3BlbnNzaC1rZXktdjEAAAAA",
        "b3BlbnNzaC1rZXk",
        Removed,
    ),
    (
        "Gitea 40-hex token",
        "0123456789abcdef0123456789abcdef01234567",
        "0123456789abcdef",
        Removed,
    ),
    (
        "runner registration token",
        "--registration-token Ab12Cd34Ef56",
        "Ab12Cd34Ef56",
        Removed,
    ),
    (
        "x-api-key header",
        "x-api-key: invented123",
        "invented123",
        Removed,
    ),
    (
        "feed URL apikey param",
        "https://feeds.example.com/f.xml?apikey=invented456&sig=zzzsig",
        "invented456",
        Removed,
    ),
    (
        "feed URL sig param",
        "https://feeds.example.com/f.xml?apikey=invented456&sig=zzzsig",
        "zzzsig",
        Removed,
    ),
    (
        "feed URL path (private feed name)",
        "https://feeds.example.com/private/alice-watchlist.xml",
        "alice-watchlist",
        Left(NAMES),
    ),
    ("pass= form", "login pass=hunter2", "hunter2", Removed),
    (
        "client_secret",
        "client_secret=invented789",
        "invented789",
        Removed,
    ),
    (
        "non-http URL credentials",
        "postgres://u:hunter3@db/x",
        "hunter3",
        Removed,
    ),
    (
        "second cookie value",
        "Cookie: sessionid=abc123; csrftoken=def456",
        "def456",
        Removed,
    ),
    (
        "password with a space",
        "password: correct horse",
        "horse",
        Removed,
    ),
    (
        "imported log line: user",
        "2026-11-05 ERROR user jdoe failed login",
        "jdoe",
        Left(CONTENT),
    ),
    (
        "imported log line: card",
        "charge card 4111 1111 1111 1111 refused",
        "4111 1111",
        Removed,
    ),
    (
        "imported log line: SSN",
        "ssn 078-05-1120 rejected",
        "078-05-1120",
        Left(CONTENT),
    ),
    (
        "window title",
        "Window title: Merger plan - Confidential.docx",
        "Merger plan",
        Left(CONTENT),
    ),
    (
        "process command line",
        "/opt/acme/bin/backup --session-key abcdef012345",
        "abcdef012345",
        Removed,
    ),
    (
        "PEM block over lines",
        "-----BEGIN RSA PRIVATE KEY-----\nMIIEinventedbody\n-----END RSA PRIVATE KEY-----",
        "MIIEinvented",
        Removed,
    ),
];

fn redactor() -> Redactor {
    // A GUI app has no HOSTNAME in its environment; the host name comes from the OS call, named here.
    Redactor::for_machine_named(
        &MapVars::new()
            .with("USER", "alex")
            .with("HOME", "/Users/alex"),
        Some("ALEX-MBP.local"),
    )
}

#[test]
fn every_hostile_line_is_judged_in_every_form() {
    let r = redactor();
    let mut failures = Vec::new();
    for (label, text, needle, verdict) in HOSTILE {
        let plain = r.text(text).0;
        let clef = r.log_line(&json!({"@t": "t", "@m": text}).to_string()).0;
        let nested = r
            .log_line(
                &json!({"@m": "x", "files": [text], "o": {"deep": [{"p": text}]}}).to_string(),
            )
            .0;
        for (form, out) in [
            ("plain text", &plain),
            ("CLEF @m", &clef),
            ("CLEF nested", &nested),
        ] {
            // The CLEF forms are JSON: look for the needle in the unescaped values.
            let seen = if form == "plain text" {
                out.contains(needle)
            } else {
                unescape(out).contains(needle)
            };
            match (verdict, seen) {
                (Removed, true) => failures.push(format!("LEAK {label} [{form}]: {out}")),
                (Left(why), false) => {
                    failures.push(format!("STALE verdict '{why}' for {label} [{form}]: {out}"))
                }
                _ => {}
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} problems:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn unescape(json_text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(json_text)
        .map(|v| v.to_string())
        .unwrap_or_default()
        .replace("\\\\", "\\")
}

#[test]
fn secret_keys_mask_any_value_type_and_settings_keys_pass_and_pw() {
    let r = redactor();
    let line = json!({"@m": "x", "apiKey": {"v": "sk-invented"}, "token": ["tok-invented"], "pass": "hunter5", "pw": "hunter4"});
    let out = r.log_line(&line.to_string()).0;
    for needle in ["sk-invented", "tok-invented", "hunter5", "hunter4"] {
        assert!(!out.contains(needle), "{needle} survived: {out}");
    }
    let (settings, _) = r.json(&json!({"pass": "hunter5", "pw": "hunter4", "sig": "zzz", "proxy": "http://proxy.corp.example.com:8080"}));
    for key in ["pass", "pw", "sig"] {
        assert_eq!(settings[key], "***", "{key}");
    }
}

#[test]
fn the_list_of_what_is_left_is_the_list_the_dialog_shows() {
    // One reason per kind of thing left: the dialog's "Not in the report" list is built from these.
    let mut reasons: Vec<&str> = HOSTILE
        .iter()
        .filter_map(|h| if let Left(w) = h.3 { Some(w) } else { None })
        .collect();
    reasons.sort();
    reasons.dedup();
    assert_eq!(reasons.len(), 3, "{reasons:?}");
}

#[test]
fn opted_in_loopback_and_rfc1918_addresses_stay_in_every_form_and_the_rest_go() {
    let r = redactor().keep_local_addresses();
    for text in [
        "bind 10.1.2.3:8080",
        "bind 127.0.0.1:8080",
        "any 0.0.0.0",
        "lan 192.168.1.5",
        "peer [::1]:443",
    ] {
        let line = json!({"@m": text, "Peers": [{"Addr": text}]}).to_string();
        let (out, c) = r.log_line(&line);
        assert_eq!(c.ip_addresses, 0, "{text}: {out}");
        assert!(out.contains(text), "{text} was changed: {out}");
        assert_eq!(r.text(text).0, text);
    }
    for (text, needle) in [
        ("tailnet fd7a:115c:a1e0::1", "fd7a:115c:a1e0"),
        ("unique-local fd00::5", "fd00::5"),
        ("carrier-grade 100.64.1.2", "100.64.1.2"),
        ("public 203.0.113.9", "203.0.113.9"),
    ] {
        let line = json!({"@m": text, "Peers": [{"Addr": text}]}).to_string();
        let (out, _) = r.log_line(&line);
        assert!(!out.contains(needle), "{text} survived: {out}");
        assert!(!r.text(text).0.contains(needle));
    }
}
