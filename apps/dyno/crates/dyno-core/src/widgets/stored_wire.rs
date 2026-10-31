//! How a [`Stored`] layout travels inside `Settings`: as the layout's own JSON in human-readable formats (the settings
//! file, the webview) and as one JSON string in binary ones (the postcard link between the app and the agent), because
//! reading a stored layout leniently needs a self-describing value and postcard has none. Use with
//! `#[serde(with = "...")]`.

use paddock_widgets::{Catalogue, Stored};
use serde::de::Error as _;
use serde::ser::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Writes the layout.
pub fn serialize<C: Catalogue, S: Serializer>(stored: &Stored<C>, serializer: S) -> Result<S::Ok, S::Error> {
    if serializer.is_human_readable() {
        stored.serialize(serializer)
    } else {
        serializer.serialize_str(&serde_json::to_string(stored).map_err(S::Error::custom)?)
    }
}

/// Reads the layout; never fails on its content (see `Stored`).
pub fn deserialize<'de, C: Catalogue, D: Deserializer<'de>>(deserializer: D) -> Result<Stored<C>, D::Error> {
    if deserializer.is_human_readable() {
        Stored::deserialize(deserializer)
    } else {
        let text = String::deserialize(deserializer)?;
        serde_json::from_str(&text).map_err(D::Error::custom)
    }
}
