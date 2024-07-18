use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use figment::{Error, Metadata, Profile, Provider};
use figment::value::{Dict, Map};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct VersionCheckerConfig {
    pub path: String,
    pub file_regex: String
}

impl Default for VersionCheckerConfig {
    fn default() -> Self {
        VersionCheckerConfig {
            path: String::from_utf8(BASE64_STANDARD
                .decode(b"XFxzZ2RjZnNcUHJvamVjdHNcMDA2X1BhbGFudGlyXEZyb21ZSFFcQ0VOVFVNX1ZQ").unwrap()).unwrap_or(String::from("")),
            file_regex: String::from(r".*HashInfo.txt"),
        }
    }
}

impl Provider for VersionCheckerConfig {
    fn metadata(&self) -> Metadata {
        Metadata::named("Library Config")
    }

    fn data(&self) -> Result<Map<Profile, Dict>, Error>  {
        figment::providers::Serialized::defaults(VersionCheckerConfig::default()).data()
    }

    fn profile(&self) -> Option<Profile> {
        // Optionally, a profile that's selected by default.
        None
    }
}