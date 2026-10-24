//! The wire types of the first-run checklist and the port an item implements.

use paddock_pitwall::NeedsAiView;
use serde::Serialize;

/// How one item stands, as a closed set the UI never guesses from text.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemStatus {
    /// Nothing left to do.
    Done,
    /// Not done yet; the primary button does it.
    Todo,
    /// Something is wrong; Repair puts it right (rule 19: a problem always has a working Repair).
    Problem,
}

/// What an item's own check found. [`Check::NotApplicable`] leaves the item out of the list altogether.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// Done.
    Done,
    /// Not done.
    Todo,
    /// Broken, with the words for the user.
    Problem(String),
    /// This computer or this app has no such thing (no service, a system that does not ask for notifications).
    NotApplicable,
}

/// One step of the checklist. An item's methods block briefly (the OS is asked, with bounded helpers): the checklist calls
/// them on a worker, never on the UI or IPC thread (rule 10).
pub trait SetupItem: Send + Sync {
    /// A stable id: lowercase letters, digits, `.`, `-` and `_`, at most [`MAX_ID_LEN`](super::MAX_ID_LEN) long.
    fn id(&self) -> &str;
    /// The step's name, as the user reads it.
    fn title(&self) -> String;
    /// One sentence on what it does and why it is asked.
    fn detail(&self) -> String;
    /// The primary button's label: a verb and an object (Install, Turn on, Allow, Connect).
    fn action_label(&self) -> String {
        "Set up".into()
    }
    /// Where the step stands now.
    fn check(&self) -> Check;
    /// The step's primary action (Allow, Install, Connect). The outcome is read back with [`SetupItem::check`].
    fn act(&self) -> Result<(), String>;
    /// What puts a [`Check::Problem`] right; by default the primary action again. It must not depend on the thing it
    /// repairs (rule 19).
    fn repair(&self) -> Result<(), String> {
        self.act()
    }
    /// A sentence on what the user does next while the step waits (the pairing code to compare in Pitwall).
    fn hint(&self) -> Option<String> {
        None
    }
    /// The shared "needs AI" view of the step, for the Pitwall step.
    fn needs_ai(&self) -> Option<NeedsAiView> {
        None
    }
}

/// One item as the UI shows it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    /// The item's id.
    pub id: String,
    /// The step's name.
    pub title: String,
    /// What it does and why.
    pub detail: String,
    /// The primary button's label.
    pub action: String,
    /// Where it stands.
    pub status: ItemStatus,
    /// What is wrong, or why the last action did not work.
    pub message: Option<String>,
    /// The user chose to skip it for now (it stays in Settings).
    pub skipped: bool,
    /// An action runs for it now.
    pub working: bool,
    /// What to do next while it waits (a code to compare in another app).
    pub hint: Option<String>,
    /// The shared needs-AI view (Pitwall's step only).
    #[cfg_attr(
        test,
        ts(type = "import('@decryp7/paddock-ui/shell/pitwall').NeedsAiView | null")
    )]
    pub needs_ai: Option<NeedsAiView>,
}

/// The whole checklist.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupState {
    /// The steps, in order.
    pub items: Vec<ItemView>,
    /// The checklist is finished or dismissed: it does not open by itself any more (Settings still has it).
    pub finished: bool,
    /// The window opens the checklist by itself now: not finished and a step is open (not done, not skipped).
    pub show_on_start: bool,
}

/// Why a request was refused, as a closed set.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SetupErrorCode {
    /// The app has no checklist.
    NotSetUp,
    /// No such step.
    Unknown,
    /// The step is running already.
    Busy,
    /// The step did not work; the message says why.
    Failed,
    /// The answer could not be kept on disk.
    NotSaved,
}

/// What a command rejects with.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupCommandError {
    /// The kind.
    pub code: SetupErrorCode,
    /// A sentence that says what to do.
    pub message: String,
    /// Trying again can help.
    pub retryable: bool,
}

impl std::fmt::Display for SetupCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SetupCommandError {}
