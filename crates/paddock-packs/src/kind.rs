use crate::PackError;
use serde::de::DeserializeOwned;
use std::fmt::Debug;

/// Who wrote a pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum Origin {
    /// Shipped inside the app.
    Builtin,
    /// Dropped into the user's packs folder; apps apply stricter rules to these.
    User,
}

/// What an app says about its packs. One implementation per kind of pack (cleanup categories, model catalogue, ...).
pub trait PackKind: Send + Sync + 'static {
    /// The pack file as `serde` reads it. Unknown keys must not be denied here: the loader reports them as warnings
    /// (a `#[serde(deny_unknown_fields)]` would refuse the whole pack instead). Do not declare `format`; the loader
    /// reads and removes it.
    type Raw: DeserializeOwned;
    /// The value the app uses from one pack.
    type Pack: Clone + Debug + PartialEq + Send + Sync + 'static;

    /// The pack format this build reads (the file's `format = N`).
    const FORMAT: u32;

    /// Checks the app's rules and turns one parsed file into the app's value. An `Err` refuses the whole pack.
    fn convert(raw: Self::Raw, origin: Origin) -> Result<Self::Pack, PackError>;
}
