//! Versioned, lenient reading: a stored layout never fails its settings file.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::catalogue::Catalogue;
use crate::check;
use crate::error::{Counted, Note, NoteCode, WidgetError};
use crate::id::PresetId;
use crate::layout::{Layout, Placement, Preset};
use crate::view;

/// The longest preset name, in characters.
pub const MAX_NAME: usize = 64;

/// What reading a stored layout found.
pub struct Read<C: Catalogue> {
    /// The layout to use.
    pub layout: Layout<C::Widget>,
    /// What was wrong, if anything: one note for a fallback, one for dropped entries.
    pub notes: Vec<Note>,
    /// A newer version's value, kept as read and written back unchanged (`NoteCode::Newer`).
    pub unread: Option<Value>,
}

impl<C: Catalogue> Clone for Read<C> {
    fn clone(&self) -> Self {
        Read {
            layout: self.layout.clone(),
            notes: self.notes.clone(),
            unread: self.unread.clone(),
        }
    }
}

impl<C: Catalogue> std::fmt::Debug for Read<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Read")
            .field("layout", &self.layout)
            .field("notes", &self.notes)
            .field("unread", &self.unread)
            .finish()
    }
}

impl<C: Catalogue> PartialEq for Read<C> {
    fn eq(&self, other: &Self) -> bool {
        self.layout == other.layout && self.notes == other.notes && self.unread == other.unread
    }
}

fn fallback<C: Catalogue>(code: NoteCode, what: &str) -> Read<C> {
    Read {
        layout: Layout::defaults::<C>(),
        notes: vec![Note {
            code,
            message: format!("The saved widget layout {what}; using the defaults."),
        }],
        unread: None,
    }
}

/// Reads a stored layout. Never fails and never panics:
///
/// - missing (`null`) gives the defaults and no note;
/// - damaged (not an object, no version, no placements) or larger than the surface's `max_bytes` gives the defaults and
///   one note;
/// - newer than the surface's version gives the defaults, one note and the value in [`Read::unread`], which [`Stored`]
///   writes back unchanged and refuses to edit over;
/// - each invalid, unknown, duplicate or surplus placement or preset is dropped on its own, with one note counting them
///   (at most twice the limit is looked at; the rest is dropped unread);
/// - a layout whose entries were all dropped, or that falls under `min_placements`, takes the defaults' placements.
pub fn read<C: Catalogue>(value: &Value) -> Read<C> {
    let s = &C::SURFACE;
    if value.is_null() {
        return Read {
            layout: Layout::defaults::<C>(),
            notes: Vec::new(),
            unread: None,
        };
    }
    let Some(object) = value.as_object() else {
        return fallback::<C>(NoteCode::Damaged, "is damaged");
    };
    if value.to_string().len() > s.max_bytes {
        return fallback::<C>(NoteCode::TooLarge, "is too large");
    }
    let version = match object.get("version").and_then(Value::as_u64) {
        Some(v) if v >= 1 => v,
        _ => return fallback::<C>(NoteCode::Damaged, "is damaged"),
    };
    if version > u64::from(s.schema_version) {
        let mut kept = fallback::<C>(
            NoteCode::Newer {
                version: u32::try_from(version).unwrap_or(u32::MAX),
            },
            &format!("was saved by a newer version ({version}) and is kept unchanged"),
        );
        kept.unread = Some(value.clone());
        return kept;
    }
    let Some(entries) = object.get("placements").and_then(Value::as_array) else {
        return fallback::<C>(NoteCode::Damaged, "is damaged");
    };
    let mut dropped = 0u32;
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
    let layout = Layout::new::<C>(placements, presets);
    let notes = if dropped > 0 {
        vec![Note {
            code: NoteCode::Dropped { count: dropped },
            message: format!(
                "{dropped} item(s) in the saved widget layout were not valid and were removed."
            ),
        }]
    } else {
        Vec::new()
    };
    Read {
        layout,
        notes,
        unread: None,
    }
}

/// Looks at no more than twice `max` entries; each that does not parse or is not valid is dropped and counted.
fn read_placements<C: Catalogue>(
    entries: &[Value],
    max: usize,
    counted: Counted,
    dropped: &mut u32,
) -> Vec<Placement<C::Widget>> {
    let scan = max.saturating_mul(2);
    let unread = entries.len().saturating_sub(scan);
    *dropped = dropped.saturating_add(u32::try_from(unread).unwrap_or(u32::MAX));
    let mut parsed = Vec::new();
    for entry in entries.iter().take(scan) {
        match Placement::<C::Widget>::deserialize(entry) {
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

fn read_presets<C: Catalogue>(list: &[Value], dropped: &mut u32) -> Vec<Preset<C::Widget>> {
    let s = &C::SURFACE;
    let max = s.max_user_presets as usize;
    let scan = max.saturating_mul(2);
    *dropped =
        dropped.saturating_add(u32::try_from(list.len().saturating_sub(scan)).unwrap_or(u32::MAX));
    let builtin = view::builtin_presets::<C>();
    let mut out: Vec<Preset<C::Widget>> = Vec::new();
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
}

impl<C: Catalogue> Stored<C> {
    /// A stored field holding `layout`, with no notes (after an edit).
    pub fn new(layout: Layout<C::Widget>) -> Stored<C> {
        Stored {
            read: Read {
                layout,
                notes: Vec::new(),
                unread: None,
            },
        }
    }

    /// The layout.
    pub fn layout(&self) -> &Layout<C::Widget> {
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
        self.read.unread.is_none()
    }

    /// Applies `edit` and keeps the result; [`WidgetError::NewerLayout`] while a newer version's layout is kept, so it
    /// is never saved over.
    pub fn edit(&mut self, edit: crate::Edit<C::Widget>) -> Result<(), WidgetError> {
        if !self.editable() {
            return Err(WidgetError::NewerLayout);
        }
        self.read.layout = crate::apply::<C>(&self.read.layout, edit)?;
        Ok(())
    }
}

impl<C: Catalogue> Default for Stored<C> {
    fn default() -> Self {
        Stored::new(Layout::defaults::<C>())
    }
}

impl<C: Catalogue> Clone for Stored<C> {
    fn clone(&self) -> Self {
        Stored {
            read: self.read.clone(),
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
        })
    }
}
