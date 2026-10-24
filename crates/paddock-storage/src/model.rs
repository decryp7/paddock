//! The wire types of Settings > Storage (camelCase JSON, generated into `@decryp7/paddock-ui/shell/storage`). Paths are for display
//! only: the webview names a category by its id, never a path.

use serde::{Deserialize, Serialize};

/// Whether the app can rebuild what a category holds. `cache` is cleared without further questions once the user presses "Clear
/// cache"; `data` is the user's and needs its own confirmation.
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "StorageKind"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Rebuilt by the app when it needs it.
    Cache,
    /// The user's: each category needs its own confirmation.
    Data,
}

/// One of the biggest things in a category, for the preview before clearing.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    /// The file or folder name.
    pub name: String,
    /// Its size.
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    /// Why it stays when clearing, or `None` when clearing removes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub protected: Option<String>,
}

/// One row of the Storage table.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageCategory {
    /// The category's id (what `storage_clear` names).
    pub id: String,
    /// What it is called.
    pub label: String,
    /// Cache or the user's data.
    pub kind: Kind,
    /// The folder, for display and "Show in folder".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub path: Option<String>,
    /// Whether the folder exists (nothing to show in the file manager otherwise).
    pub exists: bool,
    /// Bytes in the category.
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    /// Files in the category.
    #[cfg_attr(test, ts(type = "number"))]
    pub files: u64,
    /// What clearing would remove now (protected items left out).
    #[cfg_attr(test, ts(type = "number"))]
    pub clearable_bytes: u64,
    /// How many top-level items clearing would remove.
    #[cfg_attr(test, ts(type = "number"))]
    pub clearable_items: u64,
    /// The walk stopped at its entry or time limit: the sizes are lower bounds ("at least").
    pub capped: bool,
    /// Whether it can be cleared.
    pub clearable: bool,
    /// Why it cannot be cleared, or what clearing keeps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub note: Option<String>,
    /// The biggest items (at most [`crate::MAX_PREVIEW_ITEMS`]), biggest first.
    pub items: Vec<ItemView>,
}

/// The reply of a scan.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    /// One row per category, in the app's order.
    pub categories: Vec<StorageCategory>,
    /// Bytes in all categories.
    #[cfg_attr(test, ts(type = "number"))]
    pub total_bytes: u64,
    /// Any category's sizes are lower bounds.
    pub capped: bool,
}

/// Something clearing left in place, and why.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    /// What was left.
    pub path: String,
    /// Why, in words.
    pub reason: String,
}

/// What clearing one category did.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryClearResult {
    /// The category.
    pub id: String,
    /// Bytes freed.
    #[cfg_attr(test, ts(type = "number"))]
    pub freed_bytes: u64,
    /// Files removed.
    #[cfg_attr(test, ts(type = "number"))]
    pub removed_files: u64,
    /// The first items left in place (at most [`crate::MAX_SKIPPED_LISTED`]).
    pub skipped: Vec<Skipped>,
    /// All items left in place.
    #[cfg_attr(test, ts(type = "number"))]
    pub skipped_count: u64,
    /// The category could not be cleared at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub error: Option<String>,
}

impl CategoryClearResult {
    pub(crate) fn new(id: &str) -> Self {
        CategoryClearResult {
            id: id.to_string(),
            freed_bytes: 0,
            removed_files: 0,
            skipped: Vec::new(),
            skipped_count: 0,
            error: None,
        }
    }

    pub(crate) fn failed(id: &str, error: impl Into<String>) -> Self {
        CategoryClearResult {
            error: Some(error.into()),
            ..CategoryClearResult::new(id)
        }
    }
}

/// What the user asked to clear in one category.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearChoice {
    /// The category's id.
    pub id: String,
    /// The user confirmed clearing this category. Required for `data` categories, ignored for `cache`.
    #[serde(default)]
    pub confirmed: bool,
}

/// `storage_clear`'s request: the categories to clear, each with its own confirmation.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearRequest {
    /// The categories (at most [`crate::MAX_CATEGORIES`]).
    pub categories: Vec<ClearChoice>,
}

/// The reply of a clear.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageClearResult {
    /// Bytes freed in all categories.
    #[cfg_attr(test, ts(type = "number"))]
    pub freed_bytes: u64,
    /// One entry per requested category.
    pub categories: Vec<CategoryClearResult>,
    /// Stopped by Cancel before everything was cleared (what was done stays done).
    pub cancelled: bool,
}
