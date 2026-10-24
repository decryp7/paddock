//! The typed hand-off: parsed once, at the edge, into values that cannot disagree (parse, don't validate).

use crate::app::AppSpec;
use crate::error::SiblingError;
use crate::probe::FileProbe;
use paddock_window::launch::DeepLink;
use serde::Serialize;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// Most files one hand-off carries.
pub const MAX_FILES: usize = 32;
/// Longest item kind.
pub const MAX_KIND_LEN: usize = 32;
/// Longest item id.
pub const MAX_ITEM_ID_LEN: usize = 128;

/// An item of the other app to show: a kind (`machine`) and an id of that kind. Both are plain words so they travel in a
/// link without escaping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    kind: String,
    id: String,
}

impl Item {
    /// An item. `kind`: lowercase letters, digits and dashes, starting with a letter; `id`: letters, digits and `. _ : -`.
    pub fn new(kind: &str, id: &str) -> Result<Item, SiblingError> {
        let kind_ok = !kind.is_empty()
            && kind.len() <= MAX_KIND_LEN
            && kind.starts_with(|c: char| c.is_ascii_lowercase())
            && kind
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !kind_ok {
            return Err(SiblingError::BadItem(format!(
                "the kind {kind:?} is not a lowercase word"
            )));
        }
        let id_ok = !id.is_empty()
            && id.len() <= MAX_ITEM_ID_LEN
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'));
        if !id_ok {
            return Err(SiblingError::BadItem(format!(
                "the id {:?} has characters other than letters, digits, '.', '_', ':' and '-'",
                id.chars().take(40).collect::<String>()
            )));
        }
        Ok(Item {
            kind: kind.to_string(),
            id: id.to_string(),
        })
    }

    /// The kind.
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// The id.
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// What one app asks another to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Handoff {
    /// Just open (or bring forward) the app.
    Open,
    /// Import these files (absolute paths of existing files).
    Import {
        /// The files.
        files: Vec<PathBuf>,
    },
    /// Show this item.
    Show {
        /// The item.
        item: Item,
    },
}

fn short(p: &Path) -> String {
    let s = p.display().to_string();
    if s.chars().count() > 80 {
        let tail: String = s
            .chars()
            .rev()
            .take(77)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("...{tail}")
    } else {
        s
    }
}

fn bad(p: &Path, why: &str) -> SiblingError {
    SiblingError::BadFile {
        file: short(p),
        why: why.to_string(),
    }
}

impl Handoff {
    /// An import of `files`: each an absolute path (no `..`) of an existing file; at least one, at most [`MAX_FILES`].
    pub fn import(files: Vec<PathBuf>, probe: &dyn FileProbe) -> Result<Handoff, SiblingError> {
        if files.is_empty() {
            return Err(SiblingError::NoFiles);
        }
        if files.len() > MAX_FILES {
            return Err(SiblingError::TooManyFiles(files.len()));
        }
        for f in &files {
            if !f.is_absolute() {
                return Err(bad(f, "it is not an absolute path"));
            }
            if f.components().any(|c| matches!(c, Component::ParentDir)) {
                return Err(bad(f, "it goes up through '..'"));
            }
            if f.to_string_lossy().chars().any(char::is_control) {
                return Err(bad(f, "its name has control characters"));
            }
            if !probe.is_file(f) {
                return Err(bad(f, "there is no such file"));
            }
        }
        Ok(Handoff::Import { files })
    }

    /// A request to show `item`.
    pub fn show(item: Item) -> Handoff {
        Handoff::Show { item }
    }

    /// What the receiving `app` takes: the contract of `docs/ecosystem.md`. Refuses what it does not accept.
    pub fn check_accepted(&self, app: &AppSpec) -> Result<(), SiblingError> {
        let refuse = |what: String| SiblingError::NotAccepted {
            app: app.name.to_string(),
            what,
        };
        match self {
            Handoff::Open => Ok(()),
            Handoff::Import { files } => {
                let Some(extensions) = app.import_extensions() else {
                    return Err(refuse("files".into()));
                };
                if extensions.is_empty() {
                    return Ok(());
                }
                for f in files {
                    let ext = f
                        .extension()
                        .map(|e| e.to_string_lossy().to_ascii_lowercase());
                    if !ext.as_deref().is_some_and(|e| extensions.contains(&e)) {
                        return Err(bad(
                            f,
                            &format!("{} takes only .{} files", app.name, extensions.join(", .")),
                        ));
                    }
                }
                Ok(())
            }
            Handoff::Show { item } => {
                if app.show_kinds().contains(&item.kind()) {
                    Ok(())
                } else {
                    Err(refuse(format!("{} items", item.kind())))
                }
            }
        }
    }

    /// The command line that carries this to `app`: nothing, the paths, or one deep link of the app's scheme
    /// (`pitcrew://show/machine/mac-mini`). A running instance takes the arguments over (single instance).
    pub fn to_args(&self, app: &AppSpec) -> Result<Vec<OsString>, SiblingError> {
        self.check_accepted(app)?;
        Ok(match self {
            Handoff::Open => vec![],
            Handoff::Import { files } => {
                files.iter().map(|f| f.as_os_str().to_os_string()).collect()
            }
            Handoff::Show { item } => vec![OsString::from(format!(
                "{}://show/{}/{}",
                app.scheme,
                item.kind(),
                item.id()
            ))],
        })
    }

    /// What arrived at `app` as a deep link (`<scheme>://show/<kind>/<id>`), or `None` for any other link. The receiving
    /// app calls this with the links `paddock_window::launch::parse_args` found.
    pub fn from_link(link: &DeepLink, app: &AppSpec) -> Option<Handoff> {
        if link.scheme != app.scheme || link.host != "show" || !link.query.is_empty() {
            return None;
        }
        let mut parts = link.path.trim_start_matches('/').split('/');
        let (kind, id) = (parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let item = Item::new(kind, id).ok()?;
        let h = Handoff::Show { item };
        h.check_accepted(app).ok()?;
        Some(h)
    }

    /// What arrived at `app` as paths on its command line: an import of those the app takes. Others are left out.
    pub fn from_paths(paths: &[PathBuf], app: &AppSpec, probe: &dyn FileProbe) -> Option<Handoff> {
        let mine: Vec<PathBuf> = paths
            .iter()
            .filter(|p| {
                Handoff::Import {
                    files: vec![(*p).clone()],
                }
                .check_accepted(app)
                .is_ok()
            })
            .cloned()
            .collect();
        Handoff::import(mine, probe).ok()
    }
}
