//! What a file dialog is asked for, and the port that shows it. The request comes from the webview, so it is checked first
//! ([`PickRequest::validated`]); the answer is a list of paths that the caller grants to the window ([`crate::Files::pick`]).

use crate::grants::Access;
use paddock_commands::CommandError;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// The longest dialog title or file-type name, in characters.
pub const MAX_TITLE: usize = 200;
/// Most file types offered in one dialog.
pub const MAX_FILTERS: usize = 16;
/// Most extensions in one file type.
pub const MAX_EXTENSIONS: usize = 16;
/// The longest extension, in characters.
pub const MAX_EXTENSION_LEN: usize = 16;

/// What a dialog asks for.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PickKind {
    /// One existing file.
    OpenFile,
    /// Several existing files.
    OpenFiles,
    /// One existing folder.
    OpenFolder,
    /// Several existing folders.
    OpenFolders,
    /// A file to write (it may not exist yet).
    Save,
}

/// A file type offered in a dialog.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PickFilter {
    /// What the user sees ("Log files").
    pub name: String,
    /// Extensions without the dot (`log`, `txt`); letters and digits only.
    pub extensions: Vec<String>,
}

/// A request for a native open or save dialog (camelCase on the wire).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickRequest {
    /// What to choose.
    pub kind: PickKind,
    /// The dialog's title (empty: the OS's).
    #[serde(default)]
    pub title: String,
    /// The file types offered.
    #[serde(default)]
    #[cfg_attr(test, ts(as = "Option<Vec<PickFilter>>", optional))]
    pub filters: Vec<PickFilter>,
    /// The suggested file name of a save dialog (a name, not a path).
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub default_name: Option<String>,
}

impl PickRequest {
    /// An open request for one file.
    pub fn open_file(title: &str) -> PickRequest {
        PickRequest::new(PickKind::OpenFile, title)
    }

    /// An open request for several files.
    pub fn open_files(title: &str) -> PickRequest {
        PickRequest::new(PickKind::OpenFiles, title)
    }

    /// An open request for one folder.
    pub fn open_folder(title: &str) -> PickRequest {
        PickRequest::new(PickKind::OpenFolder, title)
    }

    /// A save request with a suggested name.
    pub fn save(title: &str, default_name: &str) -> PickRequest {
        PickRequest {
            default_name: Some(default_name.to_string()),
            ..PickRequest::new(PickKind::Save, title)
        }
    }

    fn new(kind: PickKind, title: &str) -> PickRequest {
        PickRequest {
            kind,
            title: title.to_string(),
            filters: Vec::new(),
            default_name: None,
        }
    }

    /// Adds a file type.
    #[must_use]
    pub fn filter(mut self, name: &str, extensions: &[&str]) -> PickRequest {
        self.filters.push(PickFilter {
            name: name.to_string(),
            extensions: extensions.iter().map(ToString::to_string).collect(),
        });
        self
    }

    /// Checks the request (bounded text, extensions of letters and digits, a plain default name).
    pub fn validated(self) -> Result<PickRequest, CommandError> {
        if self.title.chars().count() > MAX_TITLE || self.filters.len() > MAX_FILTERS {
            return Err(CommandError::invalid("invalid dialog request"));
        }
        for f in &self.filters {
            let ext_ok = |e: &String| {
                !e.is_empty()
                    && e.len() <= MAX_EXTENSION_LEN
                    && e.chars().all(|c| c.is_ascii_alphanumeric())
            };
            if f.name.chars().count() > MAX_TITLE
                || f.extensions.len() > MAX_EXTENSIONS
                || !f.extensions.iter().all(ext_ok)
            {
                return Err(CommandError::invalid(
                    "invalid file type in a dialog request",
                ));
            }
        }
        if let Some(n) = &self.default_name {
            let mut parts = Path::new(n).components();
            let plain = matches!(
                (parts.next(), parts.next()),
                (Some(Component::Normal(_)), None)
            );
            if n.len() > crate::path::MAX_NAME_LEN || n.contains('\0') || !plain {
                return Err(CommandError::invalid(
                    "the suggested file name must be a name, not a path",
                ));
            }
        }
        Ok(self)
    }

    /// What the paths this dialog returns are granted for.
    pub fn access(&self) -> Access {
        if self.kind == PickKind::Save {
            Access::Write
        } else {
            Access::Read
        }
    }
}

/// Shows a native dialog. Blocking: called on a worker thread, never on the IPC or main thread.
pub trait Picker: Send + Sync {
    /// The paths the user chose in a dialog owned by `window` (the window's label); empty when they closed it.
    fn pick(&self, window: &str, request: &PickRequest) -> Vec<PathBuf>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save() -> PickRequest {
        PickRequest::save("Export", "diagnostics.zip").filter("Zip", &["zip"])
    }

    #[test]
    fn requests_are_checked() {
        let ok = save();
        assert_eq!(ok.clone().validated().unwrap().access(), Access::Write);
        let open = PickRequest {
            kind: PickKind::OpenFolders,
            ..ok.clone()
        };
        assert_eq!(open.access(), Access::Read);
        for name in [
            "../x.zip",
            "/etc/x.zip",
            "a/b.zip",
            "",
            "..",
            "a\0.zip",
            &"n".repeat(300),
        ] {
            let bad = PickRequest {
                default_name: Some(name.into()),
                ..ok.clone()
            };
            assert!(bad.validated().is_err(), "{name:?}");
        }
        for ext in ["*.exe", "", "a.b", "x/y", "waytoolongextension1"] {
            let bad = PickRequest::open_file("x").filter("x", &[ext]);
            assert!(bad.validated().is_err(), "{ext:?}");
        }
        let long_title = PickRequest::open_file(&"t".repeat(MAX_TITLE + 1));
        assert!(long_title.validated().is_err());
        let many_types =
            (0..=MAX_FILTERS).fold(PickRequest::open_file("x"), |r, _| r.filter("t", &["a"]));
        assert!(many_types.validated().is_err());
        let many_ext = PickRequest::open_file("x").filter("t", &vec!["a"; MAX_EXTENSIONS + 1]);
        assert!(many_ext.validated().is_err());
        let long_name = PickRequest::open_file("x").filter(&"n".repeat(MAX_TITLE + 1), &["a"]);
        assert!(long_name.validated().is_err());
    }

    #[test]
    fn constructors_and_the_wire_form() {
        assert_eq!(PickRequest::open_file("t").kind, PickKind::OpenFile);
        assert_eq!(PickRequest::open_files("t").kind, PickKind::OpenFiles);
        assert_eq!(PickRequest::open_folder("t").kind, PickKind::OpenFolder);
        let wire: PickRequest =
            serde_json::from_value(serde_json::json!({ "kind": "openFiles", "title": "Open" }))
                .unwrap();
        assert!(wire.filters.is_empty() && wire.default_name.is_none());
        let back = serde_json::to_value(save()).unwrap();
        assert_eq!(back["defaultName"], "diagnostics.zip");
        assert_eq!(back["kind"], "save");
    }
}
