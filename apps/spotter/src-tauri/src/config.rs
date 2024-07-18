use figment::{Error, Metadata, Profile, Provider};
use figment::value::{Dict, Map};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct VersionCheckerConfig {
    pub path: String,
    pub file_regex: String,
    pub interval_seconds: u32
}

impl Default for VersionCheckerConfig {
    fn default() -> Self {
        VersionCheckerConfig {
            path: String::from(r"/Volumes/Data/Test"),
            file_regex: String::from(r".*.txt"),
            interval_seconds: 60
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