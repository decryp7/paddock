//! A category the app declares: where it lives, what it holds and what clearing must keep. The app builds its categories each time
//! they are asked for (so the lists are fresh), through [`Source`].

use crate::model::Kind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// Top-level items one category lists at most (a folder with more is cleared in several goes).
pub const MAX_ITEMS: usize = 10_000;
/// The longest category id.
pub const MAX_ID_LEN: usize = 40;

/// An app's own removal of an item.
pub type Remover = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

/// How an item is removed.
#[derive(Clone)]
pub enum How {
    /// Deleted from disk, confined to the category's folder.
    Path,
    /// Through the app (a model or runtime the engine must stop using first). Called after the re-check, with the item's path;
    /// the error is shown to the user.
    Custom(Remover),
}

impl std::fmt::Debug for How {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            How::Path => f.write_str("Path"),
            How::Custom(_) => f.write_str("Custom"),
        }
    }
}

/// One top-level thing in a category.
#[derive(Debug, Clone)]
pub struct Item {
    /// Where it is.
    pub path: PathBuf,
    /// How it is removed.
    pub how: How,
    /// Why it must stay; `None` when clearing may remove it.
    pub protect: Option<String>,
}

impl Item {
    /// An item removed from disk.
    pub fn path(path: impl Into<PathBuf>) -> Item {
        Item {
            path: path.into(),
            how: How::Path,
            protect: None,
        }
    }

    /// Keeps the item, for `reason`, unless it is already kept for another.
    #[must_use]
    pub fn protected(mut self, reason: Option<String>) -> Item {
        if self.protect.is_none() {
            self.protect = reason;
        }
        self
    }

    /// Removes the item through `f` instead of from disk.
    #[must_use]
    pub fn removed_by(
        mut self,
        f: impl Fn(&Path) -> Result<(), String> + Send + Sync + 'static,
    ) -> Item {
        self.how = How::Custom(Arc::new(f));
        self
    }
}

/// A category with what it holds.
#[derive(Debug, Clone)]
pub struct Category {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) kind: Kind,
    pub(crate) shown: Option<PathBuf>,
    pub(crate) root: Option<PathBuf>,
    pub(crate) items: Vec<Item>,
    pub(crate) blocked: Option<String>,
    pub(crate) note: Option<String>,
    bad_id: bool,
}

/// Whether `id` is a category id: lower-case letters, digits and dashes, 1 to [`MAX_ID_LEN`] characters, starting with a letter.
pub fn valid_id(id: &str) -> bool {
    (1..=MAX_ID_LEN).contains(&id.len())
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl Category {
    /// A category with no folder yet: not clearable until [`Category::folder`] names one. Panics-free: an invalid `id` is
    /// replaced by `invalid-id` (and the category cannot be cleared), so a typo never reaches the wire.
    pub fn new(id: &str, label: &str, kind: Kind) -> Category {
        let valid = valid_id(id);
        Category {
            id: if valid {
                id.to_string()
            } else {
                "invalid-id".to_string()
            },
            label: label.to_string(),
            kind,
            shown: None,
            root: None,
            items: Vec::new(),
            blocked: Some(if valid {
                "This folder cannot be found on this computer".to_string()
            } else {
                format!("\"{id}\" is not a valid category id")
            }),
            note: None,
            bad_id: !valid,
        }
    }

    /// The folder: shown to the user, and the only place anything of this category is removed from.
    #[must_use]
    pub fn folder(mut self, root: impl Into<PathBuf>) -> Category {
        let root = root.into();
        if !self.bad_id {
            self.blocked = None;
        }
        self.shown = Some(root.clone());
        self.root = Some(root);
        self
    }

    /// Shows another folder than the one removals are confined to (a category whose items are some of the folder's children).
    #[must_use]
    pub fn shown(mut self, shown: impl Into<PathBuf>) -> Category {
        self.shown = Some(shown.into());
        self
    }

    /// The top-level entries of the folder as items (at most [`MAX_ITEMS`], sorted), `keep` deciding which stay.
    #[must_use]
    pub fn entries(mut self, keep: impl Fn(&Path) -> Option<String>) -> Category {
        if let Some(root) = &self.root {
            self.items = entries(root)
                .into_iter()
                .map(|p| Item::path(&p).protected(keep(&p)))
                .collect();
        }
        self
    }

    /// Adds one item.
    #[must_use]
    pub fn item(mut self, item: Item) -> Category {
        if self.items.len() < MAX_ITEMS {
            self.items.push(item);
        }
        self
    }

    /// Says the category cannot be cleared, and why.
    #[must_use]
    pub fn not_clearable(mut self, reason: &str) -> Category {
        self.blocked = Some(reason.to_string());
        self
    }

    /// What clearing keeps, shown under the row.
    #[must_use]
    pub fn note(mut self, note: &str) -> Category {
        self.note = Some(note.to_string());
        self
    }

    /// The category's id.
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// What the app declares: its categories, built fresh at every call.
pub trait Source: Send + Sync {
    /// Every category, in display order.
    fn categories(&self) -> Vec<Category>;
}

/// A fixed list of categories (tests, and apps whose categories never change).
impl Source for Vec<Category> {
    fn categories(&self) -> Vec<Category> {
        self.clone()
    }
}

/// The entries of `dir` (at most [`MAX_ITEMS`]), sorted; nothing when it is missing.
pub fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|it| it.flatten().take(MAX_ITEMS).map(|e| e.path()).collect())
        .unwrap_or_default();
    out.sort();
    out
}

/// Keeps what was written in this run, or when that cannot be told (unknown start or modification time): the reason to give to
/// [`Item::protected`]. `what` finishes "<what> in this session".
pub fn from_this_run(path: &Path, start: Option<SystemTime>, what: &str) -> Option<String> {
    let modified = |p: &Path| std::fs::symlink_metadata(p).and_then(|m| m.modified()).ok();
    let newest = if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
        entries(path)
            .iter()
            .filter(|e| std::fs::symlink_metadata(e).is_ok_and(|m| m.is_file()))
            .filter_map(|e| modified(e))
            .max()
            .or_else(|| modified(path))
    } else {
        modified(path)
    };
    match (start, newest) {
        (Some(start), Some(t)) if t < start => None,
        _ => Some(format!("{what} in this session")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk::tests::write;
    use std::time::Duration;

    #[test]
    fn ids_are_short_lower_case_words() {
        for ok in ["logs", "ai-models", "a1", "webview-cache"] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in [
            "",
            "Logs",
            "1logs",
            "../logs",
            "logs ",
            "a_b",
            &"a".repeat(MAX_ID_LEN + 1),
        ] {
            assert!(!valid_id(bad), "{bad:?}");
        }
        let c = Category::new("Logs", "Logs", Kind::Data).folder("/x");
        assert_eq!(c.id(), "invalid-id");
        assert!(
            c.blocked.is_some(),
            "a folder does not make an invalid id clearable"
        );
        assert!(
            Category::new("logs", "Logs", Kind::Data).blocked.is_some(),
            "no folder, no clearing"
        );
    }

    #[test]
    fn entries_are_sorted_bounded_and_kept_by_the_callers_rule() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("b.log"), 1);
        write(&d.path().join("a.log"), 1);
        write(&d.path().join("today.log"), 1);
        let c = Category::new("logs", "Logs", Kind::Data)
            .folder(d.path())
            .entries(|p| {
                p.ends_with("today.log")
                    .then(|| "being written".to_string())
            })
            .note("Today's log is kept.");
        let names: Vec<_> = c
            .items
            .iter()
            .map(|i| i.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.log", "b.log", "today.log"]);
        assert_eq!(c.items[2].protect.as_deref(), Some("being written"));
        assert_eq!(c.note.as_deref(), Some("Today's log is kept."));
        assert!(entries(&d.path().join("missing")).is_empty());
        let none = Category::new("logs", "Logs", Kind::Data).entries(|_| None);
        assert!(none.items.is_empty(), "no folder, no entries");
    }

    #[test]
    fn items_are_added_kept_and_removed_through_the_app() {
        let item = Item::path("/x/a")
            .protected(Some("first".into()))
            .protected(Some("second".into()));
        assert_eq!(
            item.protect.as_deref(),
            Some("first"),
            "the first reason stays"
        );
        let custom = Item::path("/x/b").removed_by(|_| Ok(()));
        assert_eq!(format!("{:?} {:?}", item.how, custom.how), "Path Custom");
        let c = Category::new("models", "Models", Kind::Data)
            .folder("/x")
            .shown("/x/shown")
            .item(item)
            .item(custom)
            .not_clearable("a set-up is downloading one");
        assert_eq!(c.items.len(), 2);
        assert_eq!(c.shown.as_deref(), Some(Path::new("/x/shown")));
        assert_eq!(c.blocked.as_deref(), Some("a set-up is downloading one"));
        let full = (0..MAX_ITEMS + 5).fold(Category::new("m", "M", Kind::Cache), |c, i| {
            c.item(Item::path(format!("/x/{i}")))
        });
        assert_eq!(full.items.len(), MAX_ITEMS);
        assert_eq!(vec![c.clone()].categories().len(), 1);
    }

    #[test]
    fn what_was_written_in_this_run_is_kept() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("old/a"), 1);
        let file = d.path().join("f");
        write(&file, 1);
        let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
        let later = modified + Duration::from_secs(60);
        let earlier = modified - Duration::from_secs(60);
        assert_eq!(
            from_this_run(&file, Some(later), "opened"),
            None,
            "older than the start"
        );
        assert_eq!(
            from_this_run(&file, Some(earlier), "opened").as_deref(),
            Some("opened in this session")
        );
        assert!(
            from_this_run(&file, None, "opened").is_some(),
            "unknown start: keep"
        );
        assert_eq!(
            from_this_run(&d.path().join("old"), Some(later), "opened"),
            None,
            "a folder: its newest file"
        );
        assert!(from_this_run(&d.path().join("old"), Some(earlier), "opened").is_some());
        let empty = d.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        assert_eq!(
            from_this_run(&empty, Some(later), "opened"),
            None,
            "an empty folder: its own time"
        );
        assert!(
            from_this_run(&d.path().join("missing"), Some(later), "opened").is_some(),
            "unknown time: keep"
        );
    }
}
