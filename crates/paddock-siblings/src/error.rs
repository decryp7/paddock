//! Why a hand-off was refused or failed. "Not installed" is not an error: it is a plain answer ([`crate::Outcome`]).

use std::fmt;

/// A refused or failed hand-off, with a sentence that says what to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiblingError {
    /// The app is not one of the family.
    UnknownApp(String),
    /// An app does not hand off to itself.
    ToSelf(String),
    /// The app takes no such hand-off.
    NotAccepted {
        /// The app's name.
        app: String,
        /// What was offered (`files`, `a machine`).
        what: String,
    },
    /// A file is not a plain absolute path to an existing file, or has an extension the app does not take.
    BadFile {
        /// The file, shortened.
        file: String,
        /// Why.
        why: String,
    },
    /// More files than one hand-off carries.
    TooManyFiles(usize),
    /// No file at all.
    NoFiles,
    /// The item's kind or id is not allowed.
    BadItem(String),
    /// The program was found but could not be started.
    LaunchFailed {
        /// The app's name.
        app: String,
        /// The system's reason.
        why: String,
    },
}

impl fmt::Display for SiblingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SiblingError::UnknownApp(id) => write!(
                f,
                "{id:?} is not an app of the family, so it is not opened."
            ),
            SiblingError::ToSelf(app) => write!(f, "{app} cannot hand something to itself."),
            SiblingError::NotAccepted { app, what } => write!(f, "{app} does not take {what}."),
            SiblingError::BadFile { file, why } => {
                write!(f, "The file {file:?} cannot be handed over: {why}.")
            }
            SiblingError::TooManyFiles(n) => write!(
                f,
                "{n} files are too many for one hand-off (the most is {}).",
                crate::MAX_FILES
            ),
            SiblingError::NoFiles => f.write_str("There is no file to hand over."),
            SiblingError::BadItem(why) => write!(f, "The item cannot be handed over: {why}."),
            SiblingError::LaunchFailed { app, why } => {
                write!(f, "{app} could not be started: {why}")
            }
        }
    }
}

impl std::error::Error for SiblingError {}
