//! Why the helper service did not start, in the words and codes the outside can see (ER-27). In Refuse mode a refused start
//! leaves the service STOPPED until Repair, and the service has no console and, until the data folder is verified, no log
//! file: the reason goes to the Windows Event Log (source [`EVENT_SOURCE`]) and into the service's exit code, which `sc query`
//! and the agent read back. This file is the pure part (the text and the code); `eventlog.rs` is the Windows adapter.

use crate::logfile::EarlySink;
use dyno_core::helper_wire::StartFailure;
use log::Level;
use std::fmt;
use std::sync::OnceLock;

/// The Event Log source of the helper (registered by the install; the Event Viewer shows it as the entry's source).
pub const EVENT_SOURCE: &str = "Dyno helper";
/// The longest text sent to the Event Log (the API takes 31 839 characters; the reason is short, a runaway one is cut).
pub const MAX_EVENT_TEXT: usize = 2000;

/// Why [`crate::serve::serve`] returned an error, in the table of [`StartFailure`] (the exit code) and words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartError {
    /// Which refusal.
    pub failure: StartFailure,
    /// The sentence for the user and the log, with the next step.
    pub message: String,
}

impl StartError {
    /// A start error of `failure` with `message`.
    pub fn new(failure: StartFailure, message: impl Into<String>) -> Self {
        StartError {
            failure,
            message: message.into(),
        }
    }

    /// The service-specific exit code the Service Control Manager keeps.
    pub fn exit_code(&self) -> u32 {
        self.failure.code()
    }

    /// The Event Log entry: the reason, the exit code and what to do (one text; no user content, only the helper's own paths).
    pub fn event_text(&self) -> String {
        cut(
            &format!(
                "The Dyno helper did not start (service exit code {}): {} Next step: {}.",
                self.exit_code(),
                self.message.trim(),
                self.failure.words()
            ),
            MAX_EVENT_TEXT,
        )
    }
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for StartError {}

impl From<StartError> for String {
    fn from(e: StartError) -> String {
        e.message
    }
}

/// The registry key that makes the Event Viewer show the helper's entries under [`EVENT_SOURCE`].
pub const EVENT_KEY: &str = r"HKLM\SYSTEM\CurrentControlSet\Services\EventLog\Application\Dyno helper";

/// The `reg.exe` calls (arguments of each) that register the source at install: the entries use the generic message table of
/// `EventCreate.exe` (event ids 1 to 1000 print their one text). Run elevated; a failure only means the entry shows with
/// Windows' "description cannot be found" wrapper around the same text.
pub fn register_source_commands() -> Vec<Vec<String>> {
    let s = |v: &str| v.to_string();
    vec![
        vec![
            s("add"),
            s(EVENT_KEY),
            s("/v"),
            s("EventMessageFile"),
            s("/t"),
            s("REG_EXPAND_SZ"),
            s("/d"),
            s(r"%SystemRoot%\System32\EventCreate.exe"),
            s("/f"),
        ],
        vec![
            s("add"),
            s(EVENT_KEY),
            s("/v"),
            s("TypesSupported"),
            s("/t"),
            s("REG_DWORD"),
            s("/d"),
            s("7"),
            s("/f"),
        ],
    ]
}

/// The `reg.exe` call that removes the source at uninstall.
pub fn unregister_source_command() -> Vec<String> {
    vec!["delete".into(), EVENT_KEY.into(), "/f".into()]
}

/// `eventcreate` takes ids 1 to 1000 and a description of about 1000 characters.
const EVENT_ID: &str = "1";
const MAX_DESCRIPTION: usize = 900;

/// The `eventcreate.exe` arguments that report `text` as `level` under [`EVENT_SOURCE`] (the Windows adapter runs them).
pub fn eventcreate_arguments(level: Level, text: &str) -> Vec<String> {
    let kind = match level {
        Level::Error => "ERROR",
        Level::Warn => "WARNING",
        _ => "INFORMATION",
    };
    ["/L", "APPLICATION", "/T", kind, "/SO", EVENT_SOURCE, "/ID", EVENT_ID, "/D"]
        .iter()
        .map(|s| (*s).to_string())
        .chain(std::iter::once(cut(text, MAX_DESCRIPTION)))
        .collect()
}

/// `eventcreate.exe` in the system folder the OS names: never `%SystemRoot%` from the environment, which an elevated install
/// may inherit from a user-writable place. `None` when the OS does not name it (the report is then skipped, it is a courtesy).
pub fn eventcreate_program(folders: &dyn paddock_service::privileged::KnownFolders) -> Option<std::path::PathBuf> {
    folders.system_directory().ok().map(|dir| dir.join("eventcreate.exe"))
}

/// Where events that must outlive the log file go (the Windows Event Log in the service; none elsewhere), set once at start.
static EVENT_SINK: OnceLock<EarlySink> = OnceLock::new();

/// Sets the event sink (the first call wins).
pub fn set_event_sink(sink: EarlySink) {
    let _ = EVENT_SINK.set(sink);
}

/// Reports `text` to the event sink, when there is one. Never panics.
pub fn event(level: Level, text: &str) {
    if let Some(sink) = EVENT_SINK.get() {
        sink(level, &cut(text, MAX_EVENT_TEXT));
    }
}

/// The words for a panic: what, where and on which thread (no user content: only the helper's own message and source line).
pub fn panic_text(message: &str, location: Option<(&str, u32)>, thread: &str) -> String {
    let at = location.map_or_else(String::new, |(file, line)| format!(" at {file}:{line}"));
    format!("The Dyno helper panicked on thread '{thread}'{at}: {}", message.trim())
}

/// Makes a panic leave a trace where it can be read after the process is gone: the helper's log (the file once the data folder
/// is verified, standard error before) and the event sink. The default hook (standard error) still runs: a service has no
/// console, so on its own it would leave nothing (ER-27).
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let thread = std::thread::current();
        let text = panic_text(
            &message,
            info.location().map(|l| (l.file(), l.line())),
            thread.name().unwrap_or("unnamed"),
        );
        log::error!("Helper: {text}");
        event(Level::Error, &text);
        previous(info);
    }));
}

/// `text` cut to `max` characters (on a character boundary), with an ellipsis when it was cut.
pub fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// What the Event Log is told about a finished service run: `None` for a clean stop, else the entry.
pub fn stop_event(result: &Result<(), StartError>) -> Option<String> {
    result.as_ref().err().map(StartError::event_text)
}

/// The exit code a finished service run reports: 0 for a clean stop, else the failure's own code.
pub fn exit_code_of(result: &Result<(), StartError>) -> u32 {
    result.as_ref().err().map_or(0, StartError::exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_names_its_cause_the_exit_code_and_the_next_step() {
        let e = StartError::new(
            StartFailure::NotProtected,
            "The helper will not start: the data folder (C:\\ProgramData\\Dyno\\Helper) is not protected: Users may add files.",
        );
        let text = e.event_text();
        for part in ["exit code 3", "data folder", "Users may add files", "Next step", "Repair"] {
            assert!(text.contains(part), "{part} missing: {text}");
        }
        assert_eq!(e.exit_code(), StartFailure::NotProtected.code());
        assert_eq!(e.to_string(), e.message);
        assert_eq!(String::from(e.clone()), e.message);
    }

    #[test]
    fn a_clean_stop_has_exit_code_zero_and_no_event_and_a_failure_never_does() {
        assert_eq!(exit_code_of(&Ok(())), 0);
        assert_eq!(stop_event(&Ok(())), None);
        for f in [
            StartFailure::Other,
            StartFailure::NotProtected,
            StartFailure::BadConfig,
            StartFailure::Endpoint,
        ] {
            let r = Err(StartError::new(f, "x"));
            assert_eq!(exit_code_of(&r), f.code());
            assert_ne!(exit_code_of(&r), 0);
            assert!(stop_event(&r).is_some());
        }
    }

    #[test]
    fn the_source_is_registered_with_a_message_file_and_removed_by_its_key_only() {
        let add = register_source_commands();
        assert_eq!(add.len(), 2);
        assert!(add
            .iter()
            .all(|c| c[0] == "add" && c[1] == EVENT_KEY && c.last().map(String::as_str) == Some("/f")));
        assert!(add[0].iter().any(|a| a.ends_with("EventCreate.exe")));
        assert!(EVENT_KEY.ends_with(EVENT_SOURCE), "the key is named after the source");
        assert_eq!(unregister_source_command(), ["delete", EVENT_KEY, "/f"]);
    }

    #[test]
    fn the_event_is_reported_by_eventcreate_under_the_helpers_source_within_its_limits() {
        let a = eventcreate_arguments(Level::Error, &"x".repeat(5000));
        assert_eq!(&a[..9], ["/L", "APPLICATION", "/T", "ERROR", "/SO", EVENT_SOURCE, "/ID", "1", "/D"]);
        assert_eq!(a[9].chars().count(), MAX_DESCRIPTION);
        assert_eq!(eventcreate_arguments(Level::Warn, "w")[3], "WARNING");
        assert_eq!(eventcreate_arguments(Level::Info, "i")[3], "INFORMATION");
    }

    #[test]
    fn a_panic_names_the_thread_the_place_and_the_message() {
        let t = panic_text("index out of bounds ", Some(("src/dispatch.rs", 42)), "helper-session");
        assert_eq!(
            t,
            "The Dyno helper panicked on thread 'helper-session' at src/dispatch.rs:42: index out of bounds"
        );
        assert!(panic_text("x", None, "main").ends_with("'main': x"));
    }

    #[test]
    fn a_panic_reaches_the_event_sink_and_the_process_goes_on() {
        let heard = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let h = heard.clone();
        set_event_sink(std::sync::Arc::new(move |_, text| h.lock().unwrap().push(text.to_string())));
        install_panic_hook();
        let _ = std::thread::Builder::new()
            .name("probe-thread".into())
            .spawn(|| panic!("probe failure"))
            .unwrap()
            .join();
        let _ = std::panic::take_hook();
        let heard = heard.lock().unwrap();
        assert!(
            heard
                .iter()
                .any(|t| t.contains("probe-thread") && t.contains("probe failure") && t.contains("refusal.rs")),
            "{heard:?}"
        );
    }

    #[test]
    fn a_runaway_reason_is_cut_on_a_character_boundary() {
        let long = "é".repeat(MAX_EVENT_TEXT * 2);
        let text = StartError::new(StartFailure::Other, long).event_text();
        assert_eq!(text.chars().count(), MAX_EVENT_TEXT);
        assert!(text.ends_with('…'));
        assert_eq!(cut("short", 10), "short");
    }
}
