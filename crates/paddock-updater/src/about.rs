//! About and What's New: the running version as the engine knows it (rule 6: the UI never has its own copy), build facts,
//! the legal texts (third-party notices and licences, compiled into the binary and handed over on request, so none of them
//! is part of the UI's JavaScript), and the release notes of the running version.

use crate::notes::{self, WhatsNew, WhatsNewDecision};
use crate::record::RecordStore;
use serde::Serialize;
use std::borrow::Cow;
use std::sync::Arc;

/// Longest legal document id.
const MAX_ID: usize = 32;

/// What the About window shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AboutInfo {
    pub app_name: String,
    /// The running version (the app's package version).
    pub version: String,
    /// `macos`, `windows`, `linux`.
    pub os: String,
    pub arch: String,
    /// The commit the build was made from, when the build says.
    pub commit: Option<String>,
    /// The build date (`YYYY-MM-DD`), when the build says.
    pub built: Option<String>,
    pub homepage: Option<String>,
    pub releases_page: Option<String>,
    /// The legal documents [`About::legal`] serves, in order.
    pub legal: Vec<LegalDocInfo>,
    /// Whether this build can update itself (it has an update key and a release address).
    pub updates_available: bool,
}

/// A legal document's id and title (its text is fetched on request).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct LegalDocInfo {
    pub id: String,
    pub title: String,
}

/// Why About could not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AboutError {
    /// No legal document has this id (shown shortened).
    UnknownDocument(String),
}

impl std::fmt::Display for AboutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AboutError::UnknownDocument(id) => write!(f, "there is no legal document '{id}'"),
        }
    }
}

impl std::error::Error for AboutError {}

struct LegalDoc {
    id: String,
    title: String,
    text: Cow<'static, str>,
}

/// The About facts of an app. Built once at start-up; cheap to share.
pub struct About {
    info: AboutInfo,
    docs: Vec<LegalDoc>,
    changelog: Cow<'static, str>,
    records: Arc<RecordStore>,
}

impl About {
    /// `version` is the app's package version (Tauri's `package_info().version`).
    pub fn new(app_name: &str, version: &str, records: Arc<RecordStore>) -> About {
        About {
            info: AboutInfo {
                app_name: app_name.into(),
                version: version.into(),
                os: std::env::consts::OS.into(),
                arch: std::env::consts::ARCH.into(),
                commit: None,
                built: None,
                homepage: None,
                releases_page: None,
                legal: Vec::new(),
                updates_available: false,
            },
            docs: Vec::new(),
            changelog: Cow::Borrowed(""),
            records,
        }
    }

    /// The commit and build date (e.g. from `option_env!` set by CI); empty values count as unknown.
    pub fn build(mut self, commit: Option<&str>, built: Option<&str>) -> About {
        let known = |s: Option<&str>| {
            s.map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().take(64).collect())
        };
        self.info.commit = known(commit);
        self.info.built = known(built);
        self
    }

    pub fn homepage(mut self, url: &str) -> About {
        self.info.homepage = Some(url.into());
        self
    }

    pub fn releases_page(mut self, url: &str) -> About {
        self.info.releases_page = Some(url.into());
        self
    }

    /// Whether this build updates itself (set by the updater's adapter).
    pub fn updates_available(mut self, available: bool) -> About {
        self.info.updates_available = available;
        self
    }

    /// Adds a legal document (`notices`, `mit`, `apache`, ...): an id of up to 32 lower-case letters, digits and dashes, a
    /// title and its text (usually `include_str!`). A second document with the same id replaces the first.
    pub fn legal(
        mut self,
        id: &str,
        title: &str,
        text: impl Into<Cow<'static, str>>,
    ) -> Result<About, AboutError> {
        let ok = !id.is_empty()
            && id.len() <= MAX_ID
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !ok {
            return Err(AboutError::UnknownDocument(short(id)));
        }
        self.docs.retain(|d| d.id != id);
        self.docs.push(LegalDoc {
            id: id.into(),
            title: title.into(),
            text: text.into(),
        });
        self.info.legal = self
            .docs
            .iter()
            .map(|d| LegalDocInfo {
                id: d.id.clone(),
                title: d.title.clone(),
            })
            .collect();
        Ok(self)
    }

    /// The app's `CHANGELOG.md` (usually `include_str!`), for What's New.
    pub fn changelog(mut self, text: impl Into<Cow<'static, str>>) -> About {
        self.changelog = text.into();
        self
    }

    pub fn info(&self) -> AboutInfo {
        self.info.clone()
    }

    /// The text of legal document `id`.
    pub fn legal_text(&self, id: &str) -> Result<String, AboutError> {
        self.docs
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.text.to_string())
            .ok_or_else(|| AboutError::UnknownDocument(short(id)))
    }

    /// The running version's release notes (none when the changelog has no section for it: never another version's).
    pub fn whats_new(&self) -> WhatsNew {
        WhatsNew {
            version: self.info.version.clone(),
            release: notes::release_for(&self.changelog, &self.info.version),
        }
    }

    /// Decides once per start whether to open What's New, and records the running version as seen (unless it was).
    pub fn whats_new_on_start(&self) -> WhatsNewDecision {
        let has_notes = notes::release_for(&self.changelog, &self.info.version).is_some();
        let version = self.info.version.clone();
        let (decision, _) = self.records.update(|r| {
            let d = notes::decide(r.whats_new_seen.as_deref(), &version, has_notes);
            r.whats_new_seen = Some(version.clone());
            d
        });
        decision
    }
}

fn short(id: &str) -> String {
    id.chars().take(MAX_ID).collect()
}
