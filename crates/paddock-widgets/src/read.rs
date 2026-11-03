//! Versioned, lenient reading: a stored layout never fails its settings file.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::catalogue::Catalogue;
use crate::check;
use crate::error::{Counted, Note, NoteCode, WidgetError};
use crate::id::PresetId;
use crate::layout::{Layout, Placement, Preset};
use crate::view::{self, ViewOf};

/// The longest preset name, in characters.
pub const MAX_NAME: usize = 64;

/// What reading a stored layout found.
pub struct Read<C: Catalogue> {
    /// The layout to use.
    pub layout: Layout<C::Widget, C::Place>,
    /// What was wrong, if anything: one note for a fallback, one for dropped entries.
    pub notes: Vec<Note>,
    /// The version the stored layout was upgraded from ([`Catalogue::upgrade`]), when it was; the store owner writes
    /// the upgraded layout at once, under the surface's new key, and logs the upgrade once.
    pub upgraded: Option<u32>,
    /// The stored value as read when it was upgraded: kept unchanged under the old key until the first edit, so an
    /// older app version still finds its own layout ([`Stored::legacy`]).
    pub legacy: Option<Value>,
    /// A value that could not be read cleanly (damaged, too large or a newer version's), kept as read and written back
    /// unchanged. [`Stored::edit`] replaces a damaged or too-large one; a newer one stays and is never edited.
    pub unread: Option<Value>,
}

impl<C: Catalogue> Clone for Read<C> {
    fn clone(&self) -> Self {
        Read {
            layout: self.layout.clone(),
            notes: self.notes.clone(),
            upgraded: self.upgraded,
            legacy: self.legacy.clone(),
            unread: self.unread.clone(),
        }
    }
}

impl<C: Catalogue> std::fmt::Debug for Read<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Read")
            .field("layout", &self.layout)
            .field("notes", &self.notes)
            .field("upgraded", &self.upgraded)
            .field("unread", &self.unread)
            .finish()
    }
}

impl<C: Catalogue> PartialEq for Read<C> {
    fn eq(&self, other: &Self) -> bool {
        self.layout == other.layout
            && self.notes == other.notes
            && self.upgraded == other.upgraded
            && self.legacy == other.legacy
            && self.unread == other.unread
    }
}

/// "1 widget in the saved popup layout was not valid and was removed." (plural: "3 widgets ... were").
fn dropped_message(count: u32, noun: &str) -> String {
    let (widgets, was) = if count == 1 {
        ("widget", "was")
    } else {
        ("widgets", "were")
    };
    format!("{count} {widgets} in the saved {noun} layout {was} not valid and {was} removed.")
}

/// The defaults, one note, and the value kept to be written back unchanged.
fn unreadable<C: Catalogue>(code: NoteCode, message: String, value: &Value) -> Read<C> {
    Read {
        layout: Layout::defaults::<C>(),
        notes: vec![Note { code, message }],
        upgraded: None,
        legacy: None,
        unread: Some(value.clone()),
    }
}

fn cannot_read<C: Catalogue>(code: NoteCode, value: &Value) -> Read<C> {
    let message = format!(
        "The {} layout could not be read; the defaults are shown and the old layout is kept until you change this one.",
        C::SURFACE.noun
    );
    unreadable::<C>(code, message, value)
}

/// The stored version of a layout written by a newer version of the app, if `value` is one.
fn newer_version<C: Catalogue>(value: &Value) -> Option<u64> {
    value
        .get("version")
        .and_then(Value::as_u64)
        .filter(|v| *v > u64::from(C::SURFACE.schema_version))
}

/// Reads a stored layout. Never fails and never panics. A value that cannot be read cleanly is never lost: the
/// defaults are used and the value is kept in [`Read::unread`], which [`Stored`] writes back unchanged.
///
/// - missing (`null`) gives the defaults and no note;
/// - a layout of a newer version gives the defaults and one note; [`Stored`] refuses edits (`NewerLayout`);
/// - damaged (not an object, no version, no placements) or larger than the surface's `max_bytes` gives the defaults
///   and one note; the first edit through [`Stored::edit`] replaces it;
/// - each invalid, unknown, duplicate or surplus placement or preset is dropped on its own, with one note counting them
///   (at most twice the limit is looked at; the rest is dropped unread);
/// - a layout whose entries were all dropped, or that falls under `min_placements`, takes the defaults' placements.
pub fn read<C: Catalogue>(value: &Value) -> Read<C> {
    let s = &C::SURFACE;
    if value.is_null() {
        return Read {
            layout: Layout::defaults::<C>(),
            notes: Vec::new(),
            upgraded: None,
            legacy: None,
            unread: None,
        };
    }
    if let Some(version) = newer_version::<C>(value) {
        let message = format!(
            "The {} layout was saved by a newer version ({version}) and is kept unchanged; the defaults are shown.",
            s.noun
        );
        let code = NoteCode::Newer {
            version: u32::try_from(version).unwrap_or(u32::MAX),
        };
        return unreadable::<C>(code, message, value);
    }
    let Some(object) = value.as_object() else {
        return cannot_read::<C>(NoteCode::Damaged, value);
    };
    if value.to_string().len() > s.max_bytes {
        return cannot_read::<C>(NoteCode::TooLarge, value);
    }
    if !object
        .get("version")
        .and_then(Value::as_u64)
        .is_some_and(|v| v >= 1)
    {
        return cannot_read::<C>(NoteCode::Damaged, value);
    }
    let Some(stored) = object.get("placements").and_then(Value::as_array) else {
        return cannot_read::<C>(NoteCode::Damaged, value);
    };
    let version = object.get("version").and_then(Value::as_u64).unwrap_or(0);
    let mut upgraded = None;
    let mut converted: Option<Value> = None;
    if version < u64::from(s.schema_version) {
        let from = u32::try_from(version).unwrap_or(0);
        match C::upgrade(from, value.clone()) {
            Ok(next) if next.get("version").and_then(Value::as_u64) != Some(version) => {
                upgraded = Some(from);
                converted = Some(next);
            }
            Ok(_) => {}
            Err(_) => return cannot_read::<C>(NoteCode::Damaged, value),
        }
    }
    let source = converted.as_ref().unwrap_or(value);
    let (Some(object), Some(entries)) = (
        source.as_object(),
        source.get("placements").and_then(Value::as_array),
    ) else {
        return cannot_read::<C>(NoteCode::Damaged, value);
    };
    let mut dropped = u32::try_from(stored.len().saturating_sub(entries.len())).unwrap_or(u32::MAX);
    let mut placements = read_placements::<C>(
        entries,
        s.max_placements as usize,
        Counted::Placements,
        &mut dropped,
    );
    if placements.len() < s.min_placements as usize
        || (placements.is_empty() && !entries.is_empty())
    {
        placements = Layout::defaults::<C>().placements;
    }
    let presets = match object.get("presets") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(list)) => read_presets::<C>(list, &mut dropped),
        Some(_) => {
            dropped += 1;
            Vec::new()
        }
    };
    let layout = Layout::new(s.schema_version, placements, presets);
    let notes = if dropped > 0 {
        vec![Note {
            code: NoteCode::Dropped { count: dropped },
            message: dropped_message(dropped, s.noun),
        }]
    } else {
        Vec::new()
    };
    Read {
        layout,
        notes,
        upgraded,
        legacy: upgraded.map(|_| value.clone()),
        unread: None,
    }
}

/// Looks at no more than twice `max` entries; each that does not parse or is not valid is dropped and counted.
fn read_placements<C: Catalogue>(
    entries: &[Value],
    max: usize,
    counted: Counted,
    dropped: &mut u32,
) -> Vec<Placement<C::Widget, C::Place>> {
    let scan = max.saturating_mul(2);
    let unread = entries.len().saturating_sub(scan);
    *dropped = dropped.saturating_add(u32::try_from(unread).unwrap_or(u32::MAX));
    let mut parsed = Vec::new();
    for entry in entries.iter().take(scan) {
        match Placement::<C::Widget, C::Place>::deserialize(entry) {
            Ok(p) => parsed.push(p),
            Err(_) => *dropped = dropped.saturating_add(1),
        }
    }
    check::keep_valid::<C>(parsed, max, counted, dropped)
}

#[derive(Deserialize)]
struct PresetShell {
    id: PresetId,
    name: String,
    #[serde(default)]
    builtin: bool,
    placements: Vec<Value>,
}

fn read_presets<C: Catalogue>(
    list: &[Value],
    dropped: &mut u32,
) -> Vec<Preset<C::Widget, C::Place>> {
    let s = &C::SURFACE;
    let max = s.max_user_presets as usize;
    let scan = max.saturating_mul(2);
    *dropped =
        dropped.saturating_add(u32::try_from(list.len().saturating_sub(scan)).unwrap_or(u32::MAX));
    let builtin = view::builtin_presets::<C>();
    let mut out: Vec<Preset<C::Widget, C::Place>> = Vec::new();
    for entry in list.iter().take(scan) {
        let Ok(shell) = PresetShell::deserialize(entry) else {
            *dropped = dropped.saturating_add(1);
            continue;
        };
        if shell.builtin {
            // Built-in presets are regenerated from the catalogue, whatever the file says.
            continue;
        }
        let name = clean_name(&shell.name);
        let clash =
            builtin.iter().any(|b| b.id == shell.id) || out.iter().any(|o| o.id == shell.id);
        if name.is_empty() || clash || out.len() >= max {
            *dropped = dropped.saturating_add(1);
            continue;
        }
        let placements = read_placements::<C>(
            &shell.placements,
            s.max_preset_placements as usize,
            Counted::PresetPlacements,
            dropped,
        );
        if placements.len() < s.min_placements as usize {
            // Applying it would leave the surface with too few widgets.
            *dropped = dropped.saturating_add(1);
            continue;
        }
        out.push(Preset {
            id: shell.id,
            name,
            builtin: false,
            placements,
        });
    }
    out
}

/// A name without control characters, trimmed, at most [`MAX_NAME`] characters.
pub(crate) fn clean_name(name: &str) -> String {
    let text: String = name.chars().filter(|c| !c.is_control()).collect();
    text.trim()
        .chars()
        .take(MAX_NAME)
        .collect::<String>()
        .trim()
        .to_string()
}

/// A settings field holding one surface's layout that never fails its settings file:
/// `#[serde(default)] popover: Stored<PopoverCatalogue>`.
///
/// Deserialising goes through [`read`]; serialising writes the layout, or, for a newer version's layout, that value
/// unchanged ([`Read::unread`]). The notes of the last read stay until taken.
pub struct Stored<C: Catalogue> {
    /// What the last read found.
    pub read: Read<C>,
    /// Counts up with every applied edit; in memory only. A reload starts a new value (a new `Stored`).
    revision: u64,
    /// What the last arrangement edit did, for the view (in memory only).
    pub(crate) outcome: Option<crate::view::Outcome>,
}

impl<C: Catalogue> Stored<C> {
    /// A stored field holding `layout`, with no notes. The layout is checked ([`Layout::validate`]): a layout that
    /// breaks a limit is refused with the first problem.
    pub fn new(layout: Layout<C::Widget, C::Place>) -> Result<Stored<C>, WidgetError> {
        layout.validate::<C>()?;
        Ok(Stored::from_valid(layout))
    }

    /// A stored field holding `layout` at a known revision (a view relayed from the store owner). Checked like
    /// [`Stored::new`].
    pub fn at(
        layout: Layout<C::Widget, C::Place>,
        revision: u64,
    ) -> Result<Stored<C>, WidgetError> {
        let mut stored = Stored::new(layout)?;
        stored.revision = revision;
        Ok(stored)
    }

    fn from_valid(layout: Layout<C::Widget, C::Place>) -> Stored<C> {
        Stored {
            read: Read {
                layout,
                notes: Vec::new(),
                upgraded: None,
                legacy: None,
                unread: None,
            },
            revision: 1,
            outcome: None,
        }
    }

    /// The revision: a number that changes with every applied edit.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The value the layout was upgraded from, to be written back unchanged under the old key until the first edit.
    pub fn legacy(&self) -> Option<&Value> {
        self.read.legacy.as_ref()
    }

    /// The version the layout was upgraded from, once: the store owner writes the new layout and logs it.
    pub fn take_upgraded(&mut self) -> Option<u32> {
        self.read.upgraded.take()
    }

    /// The layout.
    pub fn layout(&self) -> &Layout<C::Widget, C::Place> {
        &self.read.layout
    }

    /// The notes of the read, until [`Stored::take_notes`].
    pub fn notes(&self) -> &[Note] {
        &self.read.notes
    }

    /// The notes of the read, once: later calls return none.
    pub fn take_notes(&mut self) -> Vec<Note> {
        std::mem::take(&mut self.read.notes)
    }

    /// Whether the layout can be edited: false while a newer version's layout is kept.
    pub fn editable(&self) -> bool {
        self.read
            .unread
            .as_ref()
            .is_none_or(|kept| newer_version::<C>(kept).is_none())
    }

    /// Applies `edit` and keeps the result, returning the new view. [`WidgetError::NewerLayout`] while a newer
    /// version's layout is kept, so it is never saved over. The first edit that applies replaces a layout that could
    /// not be read (the old value is no longer written back, and its note is gone).
    pub fn edit(&mut self, edit: crate::Edit<C::Widget>) -> Result<ViewOf<C>, WidgetError> {
        if !self.editable() {
            return Err(WidgetError::NewerLayout);
        }
        let gesture = match &edit {
            crate::Edit::Arrange { gesture } => Some(gesture.clone()),
            _ => None,
        };
        self.read.layout = crate::apply::<C>(&self.read.layout, edit)?;
        self.read.unread = None;
        self.read.notes.clear();
        self.read.legacy = None;
        self.revision += 1;
        self.outcome = gesture.and_then(|g| view::outcome_of::<C>(&self.read.layout, &g));
        Ok(view::view(self))
    }

    /// [`Stored::edit`] for an edit computed on revision `base`: [`WidgetError::Stale`] when the layout has changed
    /// since (another window edited meanwhile), so the caller reloads the view.
    pub fn edit_at(
        &mut self,
        base: u64,
        edit: crate::Edit<C::Widget>,
    ) -> Result<ViewOf<C>, WidgetError> {
        if base != self.revision {
            return Err(WidgetError::Stale);
        }
        self.edit(edit)
    }

    /// What `gesture` would give on revision `base`, without applying it: the view of the result (its arrangements
    /// and rectangles are the preview). The stored layout and the revision are untouched. [`WidgetError::Stale`] on
    /// an older revision. Pure, so the app's own process answers it in microseconds.
    pub fn preview(
        &self,
        base: u64,
        gesture: crate::GridGesture,
    ) -> Result<ViewOf<C>, WidgetError> {
        if base != self.revision {
            return Err(WidgetError::Stale);
        }
        let mut copy = self.clone();
        copy.edit(crate::Edit::Arrange { gesture })
    }
}

impl<C: Catalogue> Default for Stored<C> {
    fn default() -> Self {
        Stored::from_valid(Layout::defaults::<C>())
    }
}

impl<C: Catalogue> Clone for Stored<C> {
    fn clone(&self) -> Self {
        Stored {
            read: self.read.clone(),
            revision: self.revision,
            outcome: self.outcome.clone(),
        }
    }
}

impl<C: Catalogue> std::fmt::Debug for Stored<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stored").field("read", &self.read).finish()
    }
}

impl<C: Catalogue> PartialEq for Stored<C> {
    fn eq(&self, other: &Self) -> bool {
        self.read == other.read
    }
}

impl<C: Catalogue> Serialize for Stored<C> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.read.unread {
            Some(kept) => kept.serialize(serializer),
            None => self.read.layout.serialize(serializer),
        }
    }
}

impl<'de, C: Catalogue> Deserialize<'de> for Stored<C> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Ok(Stored {
            read: read::<C>(&value),
            revision: 1,
            outcome: None,
        })
    }
}
