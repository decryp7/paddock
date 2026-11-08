use serde::{Deserialize, Serialize};

/// What a hold keeps awake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// The system stays awake; the display may sleep.
    System,
    /// The system and the display stay awake.
    SystemAndDisplay,
}

/// The longest `who` or `why` the OS gets; longer text is cut (on a character boundary).
pub const MAX_TEXT_CHARS: usize = 200;

/// A request to keep the computer awake. `who` and `why` are what `pmset -g assertions`, `powercfg /requests` and
/// `systemd-inhibit --list` show, so they name the program and the reason in plain words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldRequest {
    /// What is kept awake.
    pub scope: Scope,
    /// The program's name, as the OS lists it.
    pub who: String,
    /// The reason, as the OS lists it.
    pub why: String,
}

impl HoldRequest {
    /// Builds a request; control characters become spaces and the texts are cut to [`MAX_TEXT_CHARS`].
    pub fn new(scope: Scope, who: &str, why: &str) -> Self {
        Self {
            scope,
            who: clean(who),
            why: clean(why),
        }
    }
}

/// Replaces control characters by spaces, trims and cuts to [`MAX_TEXT_CHARS`] characters.
pub(crate) fn clean(text: &str) -> String {
    let spaced: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    spaced.trim().chars().take(MAX_TEXT_CHARS).collect()
}

/// What a live hold looks like to the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldInfo {
    /// What is kept awake.
    pub scope: Scope,
    /// The reason the OS shows.
    pub why: String,
    /// The OS's own reference (assertion ids, request handle, inhibitor fd) for logs and the Power page; no user content.
    pub os_ref: String,
}

/// A sleep or wake notification from the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PowerEvent {
    /// The system is about to sleep (or hibernate).
    Sleeping,
    /// The system woke.
    Woke,
}
