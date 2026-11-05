//! Report a problem for Spotter (plan X2): the shell's report dialog with Spotter's redaction.
//!
//! The report is the shell's (system facts, the shell's settings file, the crash marker, the newest part of the log) and is
//! only ever saved where the user chooses; nothing is sent. What Spotter adds is a redaction rule: the addresses of the
//! feeds, listings and pages it watches can hold a private token (`/feed/3f9a…/atom`, `?token=…`), so a report keeps the
//! scheme and the host of an address and nothing after it.

use paddock_diag::tauri::Setup;
use paddock_diag::{AppInfo, Redactor};
use paddock_env::{AppDirs, AppId};
use paddock_log::LogHandle;

/// The patterns Spotter adds to the shared redactor: the path, the query values and the fragment of every address.
const URL_PATTERNS: [(&str, &str); 3] = [
    // The path of an address: `https://host/a/b` becomes `https://host/…` (the query is left to the next pattern).
    (r#"(?i)(\b[a-z][a-z0-9+.-]*://[^/\s"'<>?#\\]+)/[^\s"'<>?#\\]*"#, "${1}/…"),
    // The value of every query parameter, in any text: `?token=abc&page=2` becomes `?token=***&page=***`.
    (r#"([?&;][^=&;\s"'<>#\\]+)=[^&;\s"'<>#\\]*"#, "${1}=***"),
    // The fragment of an address.
    (r#"(?i)(\b[a-z][a-z0-9+.-]*://[^\s"'<>#\\]*)#[^\s"'<>\\]*"#, "${1}#…"),
];

/// The shared redactor for this machine plus Spotter's address rules.
pub fn redactor() -> Redactor {
    let mut redactor = Redactor::for_machine(&paddock_env::ProcessVars);
    for (pattern, with) in URL_PATTERNS {
        // The patterns are constants within the shared limits: a refusal is a bug the test below finds.
        redactor = redactor
            .with_pattern(pattern, with)
            .unwrap_or_else(|e| unreachable!("Spotter's fixed pattern is valid: {e}"));
    }
    redactor
}

/// What the report says about this build.
pub fn app_info() -> AppInfo {
    AppInfo::new(
        crate::APP_ID,
        crate::APP_NAME,
        crate::VERSION,
        option_env!("PADDOCK_BUILD_COMMIT").unwrap_or(""),
    )
}

/// Puts Report a problem and the Logging settings page on `builder` (the installed app's start; a test's place leaves them
/// off, since they would read the user's folders).
pub fn install<R: tauri::Runtime>(builder: paddock_shell::Builder<R>, log: Option<LogHandle>) -> paddock_shell::Builder<R> {
    builder.diagnostics(move |_| setup(log))
}

/// What the shell needs to set up Report a problem and the Logging settings page: Spotter's folders, the shell's settings
/// file, Spotter's redaction and the running log (`None` when the log could not start: the report then has no Logging page).
///
/// The error is the start's: the shell names "Report a problem" and fails the start rather than ship an app whose reports
/// would silently be empty.
pub fn setup(log: Option<LogHandle>) -> Result<Setup, Box<dyn std::error::Error + Send + Sync>> {
    let id = AppId::new(crate::APP_ID, crate::APP_NAME)?;
    let dirs = AppDirs::for_app(&id)?;
    let setup = Setup::new(app_info(), dirs)
        .settings_file(paddock_shell::logging::SHELL_SETTINGS_FILE)
        .redactor(redactor());
    Ok(match log {
        Some(handle) => setup.log(handle),
        None => setup,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(text: &str) -> String {
        redactor().text(text).0
    }

    /// A feed address keeps its scheme and host and loses the path, where a private token often sits.
    #[test]
    fn a_feed_path_is_removed_and_the_host_kept() {
        assert_eq!(
            clean("fetching https://git.example.org/feeds/3f9a1c77/atom failed"),
            "fetching https://git.example.org/… failed"
        );
        assert_eq!(clean("HTTP://Host.Example:8080/a/b.json"), "HTTP://Host.Example:8080/…");
    }

    /// Every query value goes, whatever the parameter is called; the names stay so the report still says what was sent. A
    /// parameter the shared secret rules know (`token`, `access_key`) is marked `[redacted]` by them, the rest `***`.
    #[test]
    fn query_values_are_removed_and_the_names_kept() {
        assert_eq!(
            clean("https://h.example/x?token=abc123&page=2"),
            "https://h.example/…?token=[redacted]&page=***"
        );
        assert_eq!(
            clean("GET /releases?access_key=k1;v=2 done"),
            "GET /releases?access_key=[redacted];v=*** done"
        );
        assert_eq!(
            clean("https://h.example/x?channel=beta&arch=arm64"),
            "https://h.example/…?channel=***&arch=***"
        );
    }

    /// An address with only a host and a fragment loses the fragment (single-page apps keep tokens there).
    #[test]
    fn a_fragment_is_removed() {
        assert_eq!(clean("https://h.example#access=zz9"), "https://h.example#…");
        assert_eq!(clean("https://h.example/p#frag"), "https://h.example/…#…");
    }

    /// A line of the log is JSON; the address inside a property is cleaned and the line stays valid JSON.
    #[test]
    fn a_log_line_with_a_feed_address_stays_valid_json() {
        let line = r#"{"@t":"2026-11-05T01:02:03Z","@m":"check failed","url":"https://h.example/f/secret-token/a.atom?key=1"}"#;
        let (out, _) = redactor().log_line(line);
        let value: serde_json::Value = serde_json::from_str(&out).expect("still one JSON object");
        let url = value["url"].as_str().expect("the url property");
        assert_eq!(url, "https://h.example/…?key=***");
        assert!(!out.contains("secret-token"), "{out}");
    }

    /// Text that is not an address is not touched, and a plain local path is left to the shared personal-data rules.
    #[test]
    fn other_text_is_left_alone() {
        assert_eq!(clean("3 watches, next check in 5 min; a=b"), "3 watches, next check in 5 min; a=b");
        assert_eq!(clean("a folder watch on /srv/releases/app"), "a folder watch on /srv/releases/app");
    }

    /// The report names Spotter and this build.
    #[test]
    fn the_report_names_the_app() {
        let info = app_info();
        assert_eq!((info.id.as_str(), info.name.as_str()), ("spotter", "Spotter"));
        assert_eq!(info.version, crate::VERSION);
    }
}
