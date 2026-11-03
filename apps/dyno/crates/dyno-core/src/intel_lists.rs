//! The user's lists, as stored and edited (ADR 0031 amendments 1 and 2): several named lists, each typed by hand, imported from a
//! text file, or subscribed to by URL; one separate address allow list; and the switches of the network features. Pure: the agent
//! stores [`UserLists`] as `intel-lists.json`, applies [`IntelEdit`]s with [`apply_edit`] and builds the index from the result.
//!
//! A subscription's fetched text is not here (it lives in the cache folder); a typed or imported list's text is, bounded by
//! [`MAX_TEXT_BYTES`] per list.

use crate::intel::MAX_LISTS;
use crate::intel_sub::{validate_url, Hours, Schedule, SubscriptionStatus, TERMS_NOTICE};
use crate::intel_text::{dedupe, parse_list, Entry, Target};
use serde::{Deserialize, Serialize};

/// Largest text of one typed or imported list, in bytes.
pub const MAX_TEXT_BYTES: usize = 16 << 20;
/// Longest list name, in characters.
pub const MAX_NAME: usize = 60;
/// Most examples a "Remove duplicates" preview lists.
pub const PREVIEW_EXAMPLES: usize = 20;
/// What the first-run note says goes online (shown once, then under Settings; there is no Offline mode).
pub const FIRST_RUN_NOTE: &str =
    "The network map downloads two location databases from DB-IP once a month (download.db-ip.com) and asks Cloudflare, or \
     ipify if Cloudflare does not answer, for your public address to place you on the map; the address is then located on this \
     computer. Each has its own switch in Settings. Lists you subscribe to are fetched from the address you give. Nothing else \
     leaves this computer for these features.";

/// Where a list's entries come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ListSource {
    /// Typed by the user; the text is kept here.
    Manual,
    /// Imported from a text file the user chose (its name is shown; the text is kept here and not re-read).
    File {
        /// The file's name.
        name: String,
    },
    /// Fetched from an address the user gave; the text is kept in the cache folder.
    Url {
        /// The https address.
        url: String,
        /// When it is refreshed.
        schedule: Schedule,
    },
}

/// One list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserList {
    /// A stable id (`list-3`), kept when the list is renamed; also the folder name in the cache.
    pub id: String,
    /// The user's name for it.
    pub name: String,
    /// Off lists are not matched.
    pub enabled: bool,
    /// Where it comes from.
    pub source: ListSource,
    /// The text of a typed or imported list (empty for a subscription).
    pub text: String,
}

/// The switches of the network features: on by default, each its own (there is no Offline mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetSwitches {
    /// Download the DB-IP location databases (monthly).
    pub location_databases: bool,
    /// Ask a public-IP service for the user's own address to place them on the map.
    pub own_location: bool,
    /// Whether the first-run note was shown.
    pub first_run_note_seen: bool,
}

impl Default for NetSwitches {
    fn default() -> Self {
        NetSwitches {
            location_databases: true,
            own_location: true,
            first_run_note_seen: false,
        }
    }
}

/// Everything the user keeps for the bad-IP check and the map's data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UserLists {
    /// File format version.
    pub version: u32,
    /// The lists, in the order the user sees them.
    pub lists: Vec<UserList>,
    /// The address allow list, as text (addresses, ranges and host names, one per line, with notes). Addresses override every
    /// list; host names cancel only the user's own host name entries.
    pub allow: String,
    /// The network switches.
    pub switches: NetSwitches,
    /// The number the next list id takes.
    pub next_id: u32,
}

impl Default for UserLists {
    fn default() -> Self {
        UserLists {
            version: 1,
            lists: Vec::new(),
            allow: String::new(),
            switches: NetSwitches::default(),
            next_id: 1,
        }
    }
}

/// How a new list starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NewSource {
    /// Typed text (may be empty).
    Manual {
        /// The text.
        text: String,
    },
    /// The text of a file the user picked.
    File {
        /// Its name.
        file_name: String,
        /// Its text.
        text: String,
    },
    /// A subscription.
    Url {
        /// The https address.
        url: String,
        /// Hours between refreshes (1 to 720); `None` means only when asked.
        every_hours: Option<u32>,
    },
}

/// A change the user asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IntelEdit {
    /// Adds a list.
    AddList {
        /// Its name.
        name: String,
        /// Its source.
        source: NewSource,
    },
    /// Removes a list (and, for a subscription, its cached text).
    RemoveList {
        /// Which.
        id: String,
    },
    /// Renames a list.
    RenameList {
        /// Which.
        id: String,
        /// The new name.
        name: String,
    },
    /// Turns a list on or off.
    SetEnabled {
        /// Which.
        id: String,
        /// On or off.
        enabled: bool,
    },
    /// Replaces the text of a typed or imported list.
    SetText {
        /// Which.
        id: String,
        /// The text.
        text: String,
    },
    /// Changes how often a subscription is refreshed.
    SetSchedule {
        /// Which.
        id: String,
        /// Hours between refreshes; `None` for only when asked.
        every_hours: Option<u32>,
    },
    /// Shows what "Remove duplicates" would remove from a typed or imported list; changes nothing.
    DedupePreview {
        /// Which.
        id: String,
    },
    /// Removes the duplicates and covered entries of a typed or imported list.
    DedupeApply {
        /// Which.
        id: String,
    },
    /// Replaces the allow list text.
    SetAllow {
        /// The text.
        text: String,
    },
    /// Changes the network switches.
    SetSwitches {
        /// The new switches.
        switches: NetSwitches,
    },
}

/// What "Remove duplicates" would do to a list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupePreview {
    /// Entries now.
    pub before: u32,
    /// Entries after.
    pub after: u32,
    /// Exact duplicates that go.
    pub duplicates: u32,
    /// Entries a wider entry covers that go.
    pub covered: u32,
    /// The first few removals: `(removed entry, the entry that stays and covers it)`.
    pub examples: Vec<(String, String)>,
}

/// What an edit produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditOutcome {
    /// Done.
    Done,
    /// A list was added with this id.
    Added {
        /// The id.
        id: String,
    },
    /// The preview of a `DedupePreview`.
    Preview(DedupePreview),
    /// A list was removed; its cached text (if any) should be deleted with this id.
    Removed {
        /// The id.
        id: String,
    },
}

/// Why an edit was refused (the text says what to change).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    /// No such list.
    #[error("That list no longer exists.")]
    NoList,
    /// Bad name.
    #[error("A list name must be 1 to {MAX_NAME} characters, without control characters.")]
    Name,
    /// Another list has the name.
    #[error("Another list already has that name.")]
    NameTaken,
    /// Too many lists.
    #[error("You can have at most {MAX_LISTS} lists. Remove or merge a list first.")]
    TooManyLists,
    /// Text too large.
    #[error("The text is larger than {} MB. Split it into several lists.", MAX_TEXT_BYTES >> 20)]
    TooLarge,
    /// The edit does not fit this kind of list.
    #[error("That cannot be done to this kind of list.")]
    WrongKind,
    /// Bad address.
    #[error("{0}")]
    Url(String),
}

fn check_name(lists: &UserLists, name: &str, except: Option<&str>) -> Result<String, EditError> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > MAX_NAME || n.chars().any(char::is_control) {
        return Err(EditError::Name);
    }
    let taken = lists
        .lists
        .iter()
        .any(|l| Some(l.id.as_str()) != except && l.name.eq_ignore_ascii_case(n));
    if taken {
        return Err(EditError::NameTaken);
    }
    Ok(n.to_string())
}

fn find<'a>(lists: &'a mut UserLists, id: &str) -> Result<&'a mut UserList, EditError> {
    lists.lists.iter_mut().find(|l| l.id == id).ok_or(EditError::NoList)
}

fn text_ok(text: &str) -> Result<(), EditError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(EditError::TooLarge);
    }
    Ok(())
}

/// One entry as a line of a list's text.
pub fn entry_line(e: &Entry) -> String {
    match &e.note {
        Some(n) => format!("{} # {n}", e.target),
        None => e.target.to_string(),
    }
}

fn schedule_of(every_hours: Option<u32>) -> Schedule {
    every_hours.map_or(Schedule::Manual, |h| Schedule::Automatic { every: Hours::new(h) })
}

/// Applies `edit` to `lists`. Nothing changes when it is refused.
pub fn apply_edit(lists: &mut UserLists, edit: IntelEdit) -> Result<EditOutcome, EditError> {
    match edit {
        IntelEdit::AddList { name, source } => {
            if lists.lists.len() >= MAX_LISTS {
                return Err(EditError::TooManyLists);
            }
            let name = check_name(lists, &name, None)?;
            let (source, text) = match source {
                NewSource::Manual { text } => (ListSource::Manual, text),
                NewSource::File { file_name, text } => (
                    ListSource::File {
                        name: file_name.chars().filter(|c| !c.is_control()).take(120).collect(),
                    },
                    text,
                ),
                NewSource::Url { url, every_hours } => (
                    ListSource::Url {
                        url: validate_url(&url).map_err(|e| EditError::Url(e.to_string()))?,
                        schedule: schedule_of(every_hours.or(Some(24))),
                    },
                    String::new(),
                ),
            };
            text_ok(&text)?;
            let id = format!("list-{}", lists.next_id);
            lists.next_id += 1;
            lists.lists.push(UserList {
                id: id.clone(),
                name,
                enabled: true,
                source,
                text,
            });
            Ok(EditOutcome::Added { id })
        }
        IntelEdit::RemoveList { id } => {
            let before = lists.lists.len();
            lists.lists.retain(|l| l.id != id);
            if lists.lists.len() == before {
                return Err(EditError::NoList);
            }
            Ok(EditOutcome::Removed { id })
        }
        IntelEdit::RenameList { id, name } => {
            let name = check_name(lists, &name, Some(&id))?;
            find(lists, &id)?.name = name;
            Ok(EditOutcome::Done)
        }
        IntelEdit::SetEnabled { id, enabled } => {
            find(lists, &id)?.enabled = enabled;
            Ok(EditOutcome::Done)
        }
        IntelEdit::SetText { id, text } => {
            text_ok(&text)?;
            let l = find(lists, &id)?;
            if matches!(l.source, ListSource::Url { .. }) {
                return Err(EditError::WrongKind);
            }
            l.text = text;
            Ok(EditOutcome::Done)
        }
        IntelEdit::SetSchedule { id, every_hours } => {
            match &mut find(lists, &id)?.source {
                ListSource::Url { schedule, .. } => *schedule = schedule_of(every_hours),
                _ => return Err(EditError::WrongKind),
            }
            Ok(EditOutcome::Done)
        }
        IntelEdit::DedupePreview { id } => {
            let l = find(lists, &id)?;
            if matches!(l.source, ListSource::Url { .. }) {
                return Err(EditError::WrongKind);
            }
            let raw = raw_entries(&l.text);
            let before = u32::try_from(raw.len()).unwrap_or(u32::MAX);
            let d = dedupe(raw);
            Ok(EditOutcome::Preview(DedupePreview {
                before,
                after: u32::try_from(d.entries.len()).unwrap_or(u32::MAX),
                duplicates: d.duplicates,
                covered: d.covered,
                examples: d
                    .examples
                    .iter()
                    .take(PREVIEW_EXAMPLES)
                    .map(|r| (entry_line(&r.entry), r.by.to_string()))
                    .collect(),
            }))
        }
        IntelEdit::DedupeApply { id } => {
            let l = find(lists, &id)?;
            if matches!(l.source, ListSource::Url { .. }) {
                return Err(EditError::WrongKind);
            }
            let d = dedupe(raw_entries(&l.text));
            l.text = d.entries.iter().map(entry_line).collect::<Vec<_>>().join("\n");
            Ok(EditOutcome::Done)
        }
        IntelEdit::SetAllow { text } => {
            text_ok(&text)?;
            lists.allow = text;
            Ok(EditOutcome::Done)
        }
        IntelEdit::SetSwitches { switches } => {
            lists.switches = switches;
            Ok(EditOutcome::Done)
        }
    }
}

/// The entries of `text` as typed, in order, with exact repeats still there (what a preview and "Remove duplicates" work on).
fn raw_entries(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.len() > crate::intel_text::MAX_LINE {
            continue;
        }
        let (entry, note) = t
            .split_once('#')
            .map_or((t, None), |(e, n)| (e, Some(n.trim().to_string()).filter(|n| !n.is_empty())));
        let mut words = entry.split_whitespace();
        let (Some(w), None) = (words.next(), words.next()) else { continue };
        if let Ok(target) = crate::intel_text::parse_target(w) {
            out.push(Entry { target, note });
        }
    }
    out
}

/// The allow list as the index wants it: addresses and host names.
pub fn parse_allow(text: &str) -> (Vec<Entry>, Vec<Entry>) {
    let parsed = parse_list(text, crate::intel::MAX_ALLOW);
    let (mut a, mut h) = (Vec::new(), Vec::new());
    for e in parsed.entries {
        match e.target {
            Target::Range(_) => a.push(e),
            Target::Host(_) => h.push(e),
        }
    }
    (a, h)
}

/// What the page shows for one list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListStatus {
    /// The id.
    pub id: String,
    /// The name.
    pub name: String,
    /// On or off.
    pub enabled: bool,
    /// Where it comes from (a subscription's address is shown to its owner).
    pub source: ListSource,
    /// Entries, duplicates removed, covered entries removed, and lines skipped (for a subscription, from its last fetch).
    pub counts: SubscriptionStatus,
    /// When a subscription may be updated again (ms since the epoch), when that is later than now.
    pub next_allowed_ms: Option<i64>,
}

/// The location features' state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoStatus {
    /// The switches.
    pub switches: NetSwitches,
    /// Whether a database is loaded.
    pub database_on: bool,
    /// The database's build date.
    pub db_date: Option<String>,
    /// State of the country database download.
    pub country: SubscriptionStatus,
    /// State of the ASN database download.
    pub asn: SubscriptionStatus,
    /// Where the user's own location comes from, in words ("from your public IP (via Cloudflare)", "from your region setting", ...).
    pub home_source: String,
    /// The DB-IP credit that stays visible.
    pub attribution: String,
}

/// Everything the lists page shows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelStatus {
    /// The lists.
    pub lists: Vec<ListStatus>,
    /// The allow list text.
    pub allow: String,
    /// Index size: segments that match, distinct ranges, bytes.
    pub index_segments: u32,
    /// Distinct ranges in the index.
    pub index_ranges: u32,
    /// Memory the index holds.
    pub index_bytes: u64,
    /// Why the index could not be built, in words (a limit reached).
    pub index_error: Option<String>,
    /// The location features.
    pub geo: GeoStatus,
    /// The first-run note.
    pub first_run_note: String,
    /// The note shown where a list address is added.
    pub terms_notice: String,
}

impl IntelStatus {
    /// The constant texts.
    pub fn texts() -> (String, String) {
        (FIRST_RUN_NOTE.to_string(), TERMS_NOTICE.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(l: &mut UserLists, name: &str, text: &str) -> String {
        match apply_edit(
            l,
            IntelEdit::AddList {
                name: name.into(),
                source: NewSource::Manual { text: text.into() },
            },
        )
        .unwrap()
        {
            EditOutcome::Added { id } => id,
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn lists_are_added_renamed_switched_and_removed() {
        let mut l = UserLists::default();
        let a = add(&mut l, "Work blocklist", "8.8.8.8");
        let b = add(&mut l, "Home", "");
        assert_eq!((a.as_str(), b.as_str()), ("list-1", "list-2"));
        assert_eq!(
            apply_edit(
                &mut l,
                IntelEdit::RenameList {
                    id: a.clone(),
                    name: "work blocklist".into()
                }
            ),
            Ok(EditOutcome::Done),
            "its own name"
        );
        assert_eq!(
            apply_edit(
                &mut l,
                IntelEdit::RenameList {
                    id: b.clone(),
                    name: "WORK BLOCKLIST".into()
                }
            ),
            Err(EditError::NameTaken)
        );
        assert_eq!(
            apply_edit(
                &mut l,
                IntelEdit::RenameList {
                    id: b.clone(),
                    name: "  ".into()
                }
            ),
            Err(EditError::Name)
        );
        apply_edit(
            &mut l,
            IntelEdit::SetEnabled {
                id: b.clone(),
                enabled: false,
            },
        )
        .unwrap();
        assert!(!l.lists[1].enabled);
        assert_eq!(
            apply_edit(&mut l, IntelEdit::RemoveList { id: a.clone() }),
            Ok(EditOutcome::Removed { id: a.clone() })
        );
        assert_eq!(apply_edit(&mut l, IntelEdit::RemoveList { id: a }), Err(EditError::NoList));
        assert_eq!(add(&mut l, "New", ""), "list-3", "ids are never reused");
    }

    #[test]
    fn subscriptions_need_an_https_address_and_default_to_daily() {
        let mut l = UserLists::default();
        let bad = apply_edit(
            &mut l,
            IntelEdit::AddList {
                name: "u".into(),
                source: NewSource::Url {
                    url: "http://x.example/l".into(),
                    every_hours: None,
                },
            },
        );
        assert!(matches!(bad, Err(EditError::Url(_))));
        let EditOutcome::Added { id } = apply_edit(
            &mut l,
            IntelEdit::AddList {
                name: "u".into(),
                source: NewSource::Url {
                    url: "https://x.example/l".into(),
                    every_hours: None,
                },
            },
        )
        .unwrap() else {
            panic!()
        };
        assert!(matches!(&l.lists[0].source, ListSource::Url { schedule: Schedule::Automatic { every }, .. } if every.get() == 24));
        assert_eq!(
            apply_edit(
                &mut l,
                IntelEdit::SetText {
                    id: id.clone(),
                    text: "x".into()
                }
            ),
            Err(EditError::WrongKind)
        );
        apply_edit(
            &mut l,
            IntelEdit::SetSchedule {
                id: id.clone(),
                every_hours: None,
            },
        )
        .unwrap();
        assert!(matches!(
            &l.lists[0].source,
            ListSource::Url {
                schedule: Schedule::Manual,
                ..
            }
        ));
        let m = add(&mut l, "m", "");
        assert_eq!(
            apply_edit(
                &mut l,
                IntelEdit::SetSchedule {
                    id: m,
                    every_hours: Some(5)
                }
            ),
            Err(EditError::WrongKind)
        );
    }

    #[test]
    fn the_number_of_lists_and_the_size_of_a_text_are_bounded() {
        let mut l = UserLists::default();
        for i in 0..MAX_LISTS {
            add(&mut l, &format!("l{i}"), "");
        }
        let e = apply_edit(
            &mut l,
            IntelEdit::AddList {
                name: "x".into(),
                source: NewSource::Manual { text: String::new() },
            },
        );
        assert_eq!(e, Err(EditError::TooManyLists));
        assert!(e.unwrap_err().to_string().contains("32"));
        let id = l.lists[0].id.clone();
        assert_eq!(
            apply_edit(
                &mut l,
                IntelEdit::SetText {
                    id,
                    text: "x".repeat(MAX_TEXT_BYTES + 1)
                }
            ),
            Err(EditError::TooLarge)
        );
        assert_eq!(l.lists[0].text, "", "a refused edit changes nothing");
    }

    #[test]
    fn remove_duplicates_previews_first_and_applies_to_the_text() {
        let mut l = UserLists::default();
        let id = add(
            &mut l,
            "m",
            "8.8.8.8\n# note line\n8.8.0.0/16 # wide\n8.8.8.8\nx.example\ny.x.example\n9.9.9.9",
        );
        let EditOutcome::Preview(p) = apply_edit(&mut l, IntelEdit::DedupePreview { id: id.clone() }).unwrap() else {
            panic!()
        };
        assert_eq!((p.before, p.after, p.duplicates, p.covered), (6, 3, 0, 3));
        assert!(p.examples.iter().any(|(r, by)| r == "y.x.example" && by == "x.example"));
        assert!(l.lists[0].text.contains("y.x.example"), "a preview changes nothing");
        apply_edit(&mut l, IntelEdit::DedupeApply { id }).unwrap();
        assert_eq!(l.lists[0].text, "8.8.0.0/16 # wide\nx.example\n9.9.9.9");
    }

    #[test]
    fn the_allow_text_splits_into_addresses_and_host_names() {
        let (a, h) = parse_allow("8.8.8.8 # mine\nupdate.example\n10.0.0.1");
        assert_eq!((a.len(), h.len()), (1, 1));
        let mut l = UserLists::default();
        apply_edit(&mut l, IntelEdit::SetAllow { text: "8.8.8.8".into() }).unwrap();
        assert_eq!(l.allow, "8.8.8.8");
    }

    #[test]
    fn the_switches_are_on_by_default_and_the_file_survives_missing_fields() {
        let s = NetSwitches::default();
        assert!(s.location_databases && s.own_location && !s.first_run_note_seen);
        let l: UserLists = serde_json::from_str(r#"{"lists": []}"#).unwrap();
        assert_eq!(l.switches, s);
        assert_eq!(l.next_id, 1);
        let mut l = UserLists::default();
        apply_edit(
            &mut l,
            IntelEdit::SetSwitches {
                switches: NetSwitches {
                    location_databases: false,
                    ..s
                },
            },
        )
        .unwrap();
        assert!(!l.switches.location_databases);
        assert!(!FIRST_RUN_NOTE.to_lowercase().contains("offline"));
    }

    #[test]
    fn the_wire_types_survive_postcard_because_the_ipc_uses_it() {
        let mut l = UserLists::default();
        let id = add(&mut l, "Work", "8.8.8.8");
        let edits = [
            IntelEdit::AddList {
                name: "Feed".into(),
                source: NewSource::Url {
                    url: "https://x.example/l".into(),
                    every_hours: Some(24),
                },
            },
            IntelEdit::SetText {
                id: id.clone(),
                text: "x".into(),
            },
            IntelEdit::DedupePreview { id },
            IntelEdit::SetSwitches {
                switches: NetSwitches::default(),
            },
        ];
        for e in edits {
            let back: IntelEdit = postcard::from_bytes(&postcard::to_stdvec(&e).unwrap()).unwrap();
            assert_eq!(back, e);
        }
        let status = IntelStatus {
            lists: vec![ListStatus {
                id: "list-1".into(),
                name: "Work".into(),
                enabled: true,
                source: ListSource::Url {
                    url: "https://x.example/l".into(),
                    schedule: Schedule::Automatic { every: Hours::daily() },
                },
                counts: SubscriptionStatus::default(),
                next_allowed_ms: Some(5),
            }],
            ..IntelStatus::default()
        };
        let back: IntelStatus = postcard::from_bytes(&postcard::to_stdvec(&status).unwrap()).unwrap();
        assert_eq!(back, status);
        let out = EditOutcome::Preview(DedupePreview::default());
        assert_eq!(
            postcard::from_bytes::<EditOutcome>(&postcard::to_stdvec(&out).unwrap()).unwrap(),
            out
        );
        let geo = crate::geo::GeoSummary {
            countries: vec![crate::geo::CountryCount {
                code: "XA".into(),
                name: "Testland".into(),
                connections: 1,
                programs: 1,
                hosts: 1,
                flagged: 0,
                includes_content_networks: false,
            }],
            local: vec![crate::geo::LocalGroup {
                class: crate::ip_lookup::AddressClass::Private,
                connections: 2,
                programs: 1,
            }],
            ..Default::default()
        };
        assert_eq!(
            postcard::from_bytes::<crate::geo::GeoSummary>(&postcard::to_stdvec(&geo).unwrap()).unwrap(),
            geo
        );
    }
}
