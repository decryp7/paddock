//! Hand-off between the Paddock apps (plan X45): "open this file in Debrief", "show this machine in Pitcrew", "add these
//! to Spotter's watch list". Pitcrew's `debrief_link.rs` and `handoff.rs`, generalised to the family.
//!
//! - [`AppSpec`] and [`FAMILY`]: the apps and what each accepts, once (the hand-off rows of `docs/ecosystem.md`). An app
//!   that is not in the table is never launched ([`SiblingError::UnknownApp`]), and an app is never handed something its
//!   row does not list ([`SiblingError::NotAccepted`]).
//! - [`Handoff`]: `Open`, `Import { files }` or `Show { item }`, parsed once into values that cannot disagree: absolute
//!   existing files (at most [`MAX_FILES`], no `..`, an extension the app takes), an [`Item`] of plain words.
//! - [`discover`]: the program on `PATH`, then in the usual install places per OS, with the pre-rename name answered as
//!   "needs update". Pure over an injected environment, OS and file probe.
//! - [`Siblings`]: what an app holds. [`Siblings::open`] checks, finds and starts the other app detached and answers
//!   [`Outcome`]: `Opened`, or the plain answers `NotInstalled` and `NeedsUpdate` with the words to show (never an error).
//!   [`Siblings::all`] lists the family with who is installed.
//!
//! The transport is the command line, which works on every OS today and needs no registered scheme: the paths, or one
//! deep link of the target's scheme (`pitcrew://show/machine/mac-mini`). A running instance takes the arguments over
//! (`paddock-window`'s single instance), so the receiving app gets the same call either way. The receiving side parses
//! with [`Handoff::from_paths`] and [`Handoff::from_link`] over the output of `paddock_window::launch::parse_args`.
//!
//! Tests use [`testing`] (feature `testing`): a file system and a spawner that start nothing (rule 7).
//!
//! ```
//! use paddock_siblings::{Siblings, Handoff, Item, Outcome, testing::{FakeFiles, FakeSpawner}};
//! use paddock_env::Os;
//! use std::sync::Arc;
//!
//! let files = Arc::new(FakeFiles::with(["/Applications/Pitcrew.app/Contents/MacOS/pitcrew"]));
//! let spawner = Arc::new(FakeSpawner::new());
//! let siblings = Siblings::new("dyno", Os::MacOs, None, Box::new(|_| None), files, spawner.clone()).unwrap();
//! let item = Handoff::show(Item::new("machine", "mac-mini").unwrap());
//! let outcome = siblings.open("pitcrew", &item).unwrap();
//! assert!(matches!(outcome, Outcome::Opened { .. }));
//! assert_eq!(spawner.launched()[0].1, ["pitcrew://show/machine/mac-mini"]);
//! // An app outside the family is refused.
//! assert!(siblings.open("calculator", &Handoff::Open).is_err());
//! ```

mod app;
mod discover;
mod error;
mod handoff;
#[cfg(feature = "native")]
mod native;
mod probe;
mod siblings;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use app::{Accepts, AppSpec, FAMILY};
pub use discover::{Found, discover, install_candidates};
pub use error::SiblingError;
pub use handoff::{Handoff, Item, MAX_FILES, MAX_ITEM_ID_LEN, MAX_KIND_LEN};
#[cfg(feature = "native")]
pub use native::ProcessSpawner;
pub use probe::{DiskProbe, FileProbe, Spawner};
pub use siblings::{EnvFn, Outcome, Presence, Siblings};

#[cfg(test)]
mod tests;
