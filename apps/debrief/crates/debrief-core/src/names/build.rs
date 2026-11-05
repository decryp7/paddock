//! Building the name book from the catalogue and keeping it for the app: [`Names`] is the shared handle the assistant
//! tools and the commands use (book, workspace vocabulary, context and Settings switches behind locks that recover
//! from poisoning).

use super::alias_job::{self, AliasJobConfig, AliasJobReport, AliasModel};
use super::book::NameBook;
use super::context::{Cooccurrence, SearchContext};
use super::group::{group, look_alikes, merge_cooccurring, NameGroup};
use super::normalise::{file_name, strip_extension};
use super::resolve::NameSettings;
use super::segment::{split_raw, Vocabulary};
use crate::catalogue::{Candidate, CatalogueRequest, CatalogueStatusRequest};
use crate::find::{self, NameLayer, Resolved};
use crate::ports::{Filter, PageQuery, StoreResult};
use crate::query::TextMode;
use crate::search_all::SearchSource;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, RwLock};

/// The workspace's own words: pieces of names that are separated in the logs (`Graphic` and `Builder` in
/// `CompanyA.Graphic.Builder.exe`). A name written as one piece teaches nothing, so run-together spellings never
/// vouch for themselves.
pub fn vocabulary_from(candidates: &[Candidate]) -> Vocabulary {
    let mut v = Vocabulary::builtin();
    for c in candidates {
        let stem = strip_extension(file_name(&c.name));
        let pieces = split_raw(stem);
        if pieces.len() < 2 {
            continue;
        }
        for p in pieces
            .iter()
            .filter(|p| p.chars().count() >= 3 && p.chars().all(char::is_alphabetic))
        {
            v.add(p, c.count.clamp(1, 1000));
        }
    }
    v
}

/// Groups (most events first) whose events are read to join look-alike spellings that appear together.
const MERGE_GROUPS: usize = 200;
/// Events read per such group.
const MERGE_EVENTS: usize = 30;
/// Share of the rarer name's events that must hold the other for two look-alike names to be one.
const MERGE_STRENGTH: f64 = 0.6;

/// The phases of a rebuild, in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RebuildPhase {
    /// Reading the names from the catalogue.
    Catalogue,
    /// Grouping the spellings of one name.
    Grouping,
    /// Reading the events of look-alike names (a count of names read).
    Events,
    /// Joining look-alikes that appear in the same events.
    Joining,
    /// Replacing the table.
    Publishing,
}

impl RebuildPhase {
    /// The label shown next to the progress bar.
    pub fn label(self) -> &'static str {
        match self {
            RebuildPhase::Catalogue => "Reading names",
            RebuildPhase::Grouping => "Grouping names",
            RebuildPhase::Events => "Comparing look-alike names",
            RebuildPhase::Joining => "Joining look-alike names",
            RebuildPhase::Publishing => "Updating the name table",
        }
    }
}

/// How far a rebuild is: `done` of `total` in the phase's own scale (`total` 0: not known, `done` is a growing count).
/// Within a phase `done` never goes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RebuildProgress {
    pub phase: RebuildPhase,
    pub done: u64,
    pub total: u64,
}

/// What a rebuild reports to and asks of its caller.
pub struct RebuildControl<'a> {
    /// Polled between steps (and between event queries); true stops the rebuild with the table untouched.
    pub cancelled: &'a dyn Fn() -> bool,
    pub progress: &'a dyn Fn(RebuildProgress),
}

impl RebuildControl<'static> {
    /// No progress, no cancel.
    pub fn none() -> Self {
        RebuildControl {
            cancelled: &|| false,
            progress: &|_| {},
        }
    }
}

/// How a rebuild ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebuildEnd {
    /// The table was replaced; it has this many groups.
    Built { groups: usize },
    /// The catalogue is not built yet (new sources are being catalogued): the table stays as it was.
    NotBuilt,
    /// Cancelled: the table stays as it was.
    Cancelled,
}

/// Reads the events of the look-alike groups and joins the ones that appear together (best effort: a store that cannot
/// answer leaves the groups as they are). Returns the number of merges, or none when cancelled.
fn merge_by_cooccurrence(store: &(impl SearchSource + ?Sized), groups: &mut Vec<NameGroup>, ctl: &RebuildControl<'_>) -> Option<usize> {
    let wanted = look_alikes(groups, MERGE_GROUPS);
    if wanted.len() < 2 {
        return Some(0);
    }
    let mut cooc = Cooccurrence::default();
    let total = wanted.len() as u64;
    for (n, i) in wanted.into_iter().enumerate() {
        if (ctl.cancelled)() {
            return None;
        }
        (ctl.progress)(RebuildProgress {
            phase: RebuildPhase::Events,
            done: n as u64,
            total,
        });
        let Ok(page) = store.page(&PageQuery {
            query: Filter {
                text: format!("\"{}\"", groups[i].canonical().replace('"', " ")),
                text_mode: TextMode::Words,
                ..Filter::default()
            },
            offset: 0,
            limit: MERGE_EVENTS,
            ..PageQuery::default()
        }) else {
            return Some(0);
        };
        for e in &page.rows {
            let names: Vec<String> = crate::entities::extract(&[e.message.as_str()])
                .entities
                .into_iter()
                .map(|x| x.name)
                .collect();
            cooc.add_event(&names);
        }
    }
    (ctl.progress)(RebuildProgress {
        phase: RebuildPhase::Events,
        done: total,
        total,
    });
    if cooc.is_empty() {
        return Some(0);
    }
    (ctl.progress)(RebuildProgress {
        phase: RebuildPhase::Joining,
        done: 0,
        total: 1,
    });
    let merged = merge_cooccurring(groups, &cooc, MERGE_STRENGTH);
    (ctl.progress)(RebuildProgress {
        phase: RebuildPhase::Joining,
        done: 1,
        total: 1,
    });
    Some(merged)
}

/// What the app shares: the book, its vocabulary, the user's context and the Settings switches.
#[derive(Default)]
pub struct Names {
    book: RwLock<NameBook>,
    vocab: RwLock<Vocabulary>,
    ctx: Mutex<SearchContext>,
    settings: Mutex<NameSettings>,
    /// The last [`Names::refresh`] found the catalogue built.
    built: AtomicBool,
}

impl Names {
    /// Whether the table was built from a finished catalogue (false until then, and while new sources are catalogued).
    pub fn is_built(&self) -> bool {
        self.built.load(Ordering::Relaxed)
    }

    /// Rebuilds the table from the store's catalogue (when it is built) and returns the number of groups; no progress,
    /// no cancel. For tools and tests: the app rebuilds through [`Names::rebuild`] in a job, never on a request.
    pub fn refresh(&self, store: &(impl SearchSource + ?Sized)) -> StoreResult<usize> {
        Ok(match self.rebuild(store, &RebuildControl::none())? {
            RebuildEnd::Built { groups } => groups,
            RebuildEnd::NotBuilt | RebuildEnd::Cancelled => self.read(|b, _| b.groups().len()),
        })
    }

    /// Rebuilds the table from the store's catalogue in phases ([`RebuildPhase`]) with progress, stopping at the next
    /// step when `ctl` says cancelled. The new table is built off the lock and swapped in at the end, so a search
    /// during a rebuild answers from the last table at once and a cancelled rebuild changes nothing. AI aliases and
    /// user facts are kept. It is the caller's job to run this off the request and UI threads.
    pub fn rebuild(&self, store: &(impl SearchSource + ?Sized), ctl: &RebuildControl<'_>) -> StoreResult<RebuildEnd> {
        let report = |phase, done, total| (ctl.progress)(RebuildProgress { phase, done, total });
        report(RebuildPhase::Catalogue, 0, 0);
        // The cheap question first: the 50,000-name aggregate is never run while sources are still being catalogued.
        if !store.catalogue_status(&CatalogueStatusRequest::default())?.built {
            self.built.store(false, Ordering::Relaxed);
            return Ok(if (ctl.cancelled)() {
                RebuildEnd::Cancelled
            } else {
                RebuildEnd::NotBuilt
            });
        }
        let reply = store.catalogue(&CatalogueRequest {
            all: true,
            ..CatalogueRequest::default()
        })?;
        if (ctl.cancelled)() {
            return Ok(RebuildEnd::Cancelled);
        }
        if !reply.built {
            self.built.store(false, Ordering::Relaxed);
            return Ok(RebuildEnd::NotBuilt);
        }
        report(
            RebuildPhase::Catalogue,
            reply.candidates.len() as u64,
            reply.candidates.len() as u64,
        );
        report(RebuildPhase::Grouping, 0, 1);
        let vocab = vocabulary_from(&reply.candidates);
        let mut groups = group(&reply.candidates, &vocab);
        drop(reply);
        report(RebuildPhase::Grouping, 1, 1);
        if (ctl.cancelled)() {
            return Ok(RebuildEnd::Cancelled);
        }
        if merge_by_cooccurrence(store, &mut groups, ctl).is_none() || (ctl.cancelled)() {
            return Ok(RebuildEnd::Cancelled);
        }
        report(RebuildPhase::Publishing, 0, 1);
        let n = groups.len();
        {
            // Both locks, in the readers' order (book, then vocabulary): a search sees the old pair or the new pair.
            let mut book = self.book.write().unwrap_or_else(|p| p.into_inner());
            let mut vocabulary = self.vocab.write().unwrap_or_else(|p| p.into_inner());
            book.rebuild(groups);
            *vocabulary = vocab;
        }
        self.built.store(true, Ordering::Relaxed);
        report(RebuildPhase::Publishing, 1, 1);
        Ok(RebuildEnd::Built { groups: n })
    }

    /// What the search box and the assistant ask: the values and entities the words refer to, with how and why they
    /// matched and a clarifying question on a tie. Remembers the words as context; changes nothing else.
    pub fn search(&self, store: &(impl SearchSource + ?Sized), text: &str, limit: usize) -> StoreResult<Resolved> {
        self.context().remember(text);
        let ctx = self.context().clone();
        let settings = self.settings();
        self.read(|book, vocab| {
            find::resolve_names(
                store,
                text,
                limit,
                Some(&NameLayer {
                    book,
                    vocab,
                    ctx: &ctx,
                    settings,
                }),
            )
        })
    }

    /// Reads the book and vocabulary.
    pub fn read<R>(&self, f: impl FnOnce(&NameBook, &Vocabulary) -> R) -> R {
        let b = self.book.read().unwrap_or_else(|p| p.into_inner());
        let v = self.vocab.read().unwrap_or_else(|p| p.into_inner());
        f(&b, &v)
    }

    /// Changes the book (user feedback, AI entries).
    pub fn write<R>(&self, f: impl FnOnce(&mut NameBook) -> R) -> R {
        f(&mut self.book.write().unwrap_or_else(|p| p.into_inner()))
    }

    /// Replaces the book (loaded from its file); the table is rebuilt at the next [`Names::refresh`].
    pub fn set_book(&self, book: NameBook) {
        *self.book.write().unwrap_or_else(|p| p.into_inner()) = book;
    }

    pub fn context(&self) -> MutexGuard<'_, SearchContext> {
        self.ctx.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn settings(&self) -> NameSettings {
        *self.settings.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn set_settings(&self, s: NameSettings) {
        *self.settings.lock().unwrap_or_else(|p| p.into_inner()) = s;
    }

    /// Runs the alias job (see [`alias_job::run`]); the book is locked only while storing answers, never while the
    /// model thinks. Does nothing when the Settings switch is off.
    pub fn run_alias_job(&self, model: Option<&dyn AliasModel>, cfg: &AliasJobConfig, cancelled: &dyn Fn() -> bool) -> AliasJobReport {
        let cfg = AliasJobConfig {
            enabled: cfg.enabled && self.settings().ai_aliases,
            ..cfg.clone()
        };
        let mut snapshot = self.read(|b, _| b.clone());
        let before: std::collections::BTreeSet<String> = snapshot.groups().iter().map(|g| g.id()).collect();
        let rep = alias_job::run(&mut snapshot, model, &cfg, cancelled);
        self.write(|b| {
            for id in before {
                if let Some(e) = snapshot.ai(&id) {
                    if !b.ai_current(&id, &e.model) {
                        b.set_ai(&id, e.clone());
                    }
                }
            }
        });
        rep
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::{CatalogueReply, Scope};
    use crate::entities::EntityKind;

    fn cand(name: &str, n: u64) -> Candidate {
        Candidate {
            scope: Scope::Entity(EntityKind::Executable),
            name: name.into(),
            count: n,
            first_ts: None,
            last_ts: None,
            seen_in: vec![],
        }
    }

    #[test]
    fn vocabulary_learns_only_separated_words() {
        let v = vocabulary_from(&[cand("Acme.Quasar.Hub.exe", 9), cand("zorblaxfoo.exe", 50), cand("a-b.exe", 1)]);
        assert!(v.knows("quasar") && v.knows("acme"));
        assert!(!v.knows("zorblaxfoo"));
        assert!(!v.knows("a"));
    }

    /// A catalogue of fixed candidates (built or not); its events page is empty (`.2`) or fails.
    struct Fixed(Vec<Candidate>, bool, bool);

    impl SearchSource for Fixed {
        fn summary(&self) -> StoreResult<crate::ports::SummaryReply> {
            Err(crate::ports::StoreError::Invalid("not part of the fixture".into()))
        }
        fn page(&self, _: &crate::ports::PageQuery) -> StoreResult<crate::ports::PageReply> {
            if self.2 {
                return Ok(crate::ports::PageReply {
                    total: 0,
                    offset: 0,
                    rows: Vec::new(),
                    anchor_index: None,
                });
            }
            Err(crate::ports::StoreError::Invalid("the events cannot be read".into()))
        }
        fn facet(&self, _: &crate::api::FacetRequest) -> StoreResult<crate::api::Facet> {
            Err(crate::ports::StoreError::Invalid("not part of the fixture".into()))
        }
        fn exception_groups(&self) -> StoreResult<Vec<crate::exceptions::ExceptionGroup>> {
            Err(crate::ports::StoreError::Invalid("not part of the fixture".into()))
        }
        fn catalogue(&self, req: &CatalogueRequest) -> StoreResult<CatalogueReply> {
            let _ = req;
            Ok(CatalogueReply {
                built: self.1,
                pending: 0,
                candidates: self.0.clone(),
            })
        }
    }

    #[test]
    fn refresh_builds_groups_and_keeps_learning() {
        let names = Names::default();
        let store = Fixed(
            vec![cand("Acme.Graphic.Builder.exe", 5), cand("graphicbuilder.exe", 9)],
            true,
            false,
        );
        assert!(!names.is_built());
        assert_eq!(names.refresh(&store).unwrap(), 1);
        assert!(names.is_built());
        names.write(|b| b.accept("gfx", "executable|graphic builder", false));
        assert_eq!(names.refresh(&store).unwrap(), 1);
        assert_eq!(names.read(|b, _| b.facts().len()), 1);
        // A catalogue that is not built keeps the old table.
        let unbuilt = Fixed(vec![], false, false);
        assert_eq!(names.refresh(&unbuilt).unwrap(), 1);
        assert!(!names.is_built(), "new sources are being catalogued");
        names.context().remember("graphic");
        assert_eq!(names.context().recent.len(), 1);
        names.set_settings(NameSettings {
            ai_aliases: false,
            context: true,
        });
        assert!(!names.settings().ai_aliases);
        names.set_book(NameBook::default());
        assert_eq!(names.read(|b, _| b.groups().len()), 0);
    }

    #[test]
    fn the_fixture_answers_only_the_catalogue_and_events() {
        let store = look_alikes_of(true);
        assert!(store.summary().is_err() && store.exception_groups().is_err());
        assert!(store
            .facet(&crate::api::FacetRequest {
                query: Default::default(),
                field: "f".into(),
                limit: 1,
            })
            .is_err());
    }

    fn look_alikes_of(page_works: bool) -> Fixed {
        Fixed(
            vec![cand("Graphic.Builder.exe", 9), cand("Graphic.Builders.exe", 8)],
            true,
            page_works,
        )
    }

    #[test]
    fn look_alikes_stay_apart_when_their_events_cannot_be_read_or_say_nothing() {
        for page_works in [false, true] {
            let names = Names::default();
            let groups = names.refresh(&look_alikes_of(page_works)).unwrap();
            assert_eq!(groups, 2, "no co-occurrence evidence, no merge (page works: {page_works})");
            assert!(names.is_built());
        }
    }

    struct M;
    impl AliasModel for M {
        fn version(&self) -> String {
            "m1".into()
        }
        fn complete(&self, _: &str) -> Result<String, String> {
            Ok(r#"{"items":[{"id":1,"name":"Graphic Builder","aliases":["image editor"]}]}"#.into())
        }
    }

    #[test]
    fn alias_job_respects_the_settings_switch() {
        let names = Names::default();
        names.refresh(&Fixed(vec![cand("graphicbuilder.exe", 50)], true, false)).unwrap();
        let cfg = AliasJobConfig {
            pause: std::time::Duration::ZERO,
            ..AliasJobConfig::default()
        };
        names.set_settings(NameSettings {
            ai_aliases: false,
            context: true,
        });
        assert_eq!(names.run_alias_job(Some(&M), &cfg, &|| false).skipped, "off");
        names.set_settings(NameSettings::default());
        assert_eq!(names.run_alias_job(Some(&M), &cfg, &|| false).stored, 1);
        assert_eq!(
            names.read(|b, _| b.ai("executable|graphic builder").map(|e| e.aliases.clone())),
            Some(vec!["image editor".to_string()])
        );
    }

    /// A catalogue whose every event query answers with events naming all the candidates together.
    struct Together(Vec<Candidate>);

    impl SearchSource for Together {
        fn summary(&self) -> StoreResult<crate::ports::SummaryReply> {
            Err(crate::ports::StoreError::Invalid("not part of the fixture".into()))
        }
        fn page(&self, _: &crate::ports::PageQuery) -> StoreResult<crate::ports::PageReply> {
            let message = self.0.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(" and ");
            let rows = (0..3)
                .map(|i| crate::ports::EventView {
                    id: crate::batch::EventId(i),
                    ts: None,
                    ts_state: crate::batch::TsState::Parsed,
                    level: crate::model::Level::Information,
                    message: format!("started {message}"),
                    template: None,
                    exception: None,
                    fields: Default::default(),
                    source: crate::batch::SourceId(0),
                    source_name: "s".into(),
                    line: 1,
                    byte_offset: 0,
                    group: String::new(),
                    markers: Vec::new(),
                    search_match: None,
                })
                .collect();
            Ok(crate::ports::PageReply {
                total: 3,
                offset: 0,
                rows,
                anchor_index: None,
            })
        }
        fn facet(&self, _: &crate::api::FacetRequest) -> StoreResult<crate::api::Facet> {
            Err(crate::ports::StoreError::Invalid("not part of the fixture".into()))
        }
        fn exception_groups(&self) -> StoreResult<Vec<crate::exceptions::ExceptionGroup>> {
            Err(crate::ports::StoreError::Invalid("not part of the fixture".into()))
        }
        fn catalogue(&self, _: &CatalogueRequest) -> StoreResult<CatalogueReply> {
            Ok(CatalogueReply {
                built: true,
                pending: 0,
                candidates: self.0.clone(),
            })
        }
    }

    fn together() -> Together {
        Together(vec![cand("gfxbuilder.exe", 9), cand("gfx-bilder.exe", 8), cand("tray.exe", 5)])
    }

    #[test]
    fn a_rebuild_reports_each_phase_in_order_and_never_goes_back() {
        let names = Names::default();
        let seen = std::cell::RefCell::new(Vec::new());
        let ctl = RebuildControl {
            cancelled: &|| false,
            progress: &|p| seen.borrow_mut().push(p),
        };
        assert_eq!(
            names.rebuild(&together(), &ctl).unwrap(),
            RebuildEnd::Built { groups: 2 },
            "the look-alikes joined"
        );
        let seen = seen.into_inner();
        let phases: Vec<RebuildPhase> = seen.iter().map(|p| p.phase).collect();
        let mut sorted = phases.clone();
        sorted.sort();
        assert_eq!(phases, sorted, "phases run in their order");
        for phase in [
            RebuildPhase::Catalogue,
            RebuildPhase::Grouping,
            RebuildPhase::Events,
            RebuildPhase::Joining,
            RebuildPhase::Publishing,
        ] {
            let own: Vec<&RebuildProgress> = seen.iter().filter(|p| p.phase == phase).collect();
            assert!(!own.is_empty(), "{phase:?} is reported");
            assert!(own.windows(2).all(|w| w[0].done <= w[1].done), "{phase:?} never goes back");
            assert!(!phase.label().is_empty());
        }
        let last = seen.last().unwrap();
        assert_eq!((last.phase, last.done, last.total), (RebuildPhase::Publishing, 1, 1));
        assert!(
            seen.iter()
                .any(|p| p.phase == RebuildPhase::Events && p.total > 0 && p.done == p.total),
            "the events phase has a total"
        );
    }

    /// A store whose catalogue is not built: counts the cheap status questions and the every-name aggregates.
    #[derive(Default)]
    struct Unbuilt {
        status: std::cell::Cell<usize>,
        all: std::cell::Cell<usize>,
    }

    impl SearchSource for Unbuilt {
        fn summary(&self) -> StoreResult<crate::ports::SummaryReply> {
            Err(crate::ports::StoreError::Invalid("not part of the fixture".into()))
        }
        fn page(&self, _: &crate::ports::PageQuery) -> StoreResult<crate::ports::PageReply> {
            Err(crate::ports::StoreError::Invalid("not part of the fixture".into()))
        }
        fn facet(&self, _: &crate::api::FacetRequest) -> StoreResult<crate::api::Facet> {
            Err(crate::ports::StoreError::Invalid("not part of the fixture".into()))
        }
        fn exception_groups(&self) -> StoreResult<Vec<crate::exceptions::ExceptionGroup>> {
            Err(crate::ports::StoreError::Invalid("not part of the fixture".into()))
        }
        fn catalogue(&self, req: &CatalogueRequest) -> StoreResult<CatalogueReply> {
            if req.all {
                self.all.set(self.all.get() + 1);
            }
            Ok(CatalogueReply::default())
        }
        fn catalogue_status(&self, _: &CatalogueStatusRequest) -> StoreResult<crate::catalogue::CatalogueStatus> {
            self.status.set(self.status.get() + 1);
            Ok(crate::catalogue::CatalogueStatus {
                generation: 7,
                built: false,
                pending: 2,
            })
        }
    }

    #[test]
    fn a_catalogue_that_is_not_built_never_runs_the_every_name_aggregate() {
        let names = Names::default();
        let store = Unbuilt::default();
        let ctl = RebuildControl {
            cancelled: &|| false,
            progress: &|_| {},
        };
        for _ in 0..3 {
            assert_eq!(names.rebuild(&store, &ctl).unwrap(), RebuildEnd::NotBuilt);
        }
        assert_eq!((store.status.get(), store.all.get()), (3, 0), "asked cheaply, never aggregated");
        assert!(!names.is_built());
    }

    #[test]
    fn a_cancelled_rebuild_leaves_the_table_as_it_was() {
        let names = Names::default();
        names
            .refresh(&Fixed(vec![cand("Acme.Graphic.Builder.exe", 5)], true, false))
            .unwrap();
        let before = names.read(|b, _| b.groups().iter().map(|g| g.id()).collect::<Vec<_>>());
        // Cancel as soon as the events of the first look-alike are read: nothing was swapped in yet.
        let stop = std::cell::Cell::new(false);
        let ctl = RebuildControl {
            cancelled: &|| stop.get(),
            progress: &|p| stop.set(stop.get() || (p.phase == RebuildPhase::Events && p.done >= 1)),
        };
        assert_eq!(names.rebuild(&together(), &ctl).unwrap(), RebuildEnd::Cancelled);
        assert_eq!(names.read(|b, _| b.groups().iter().map(|g| g.id()).collect::<Vec<_>>()), before);
        assert!(names.is_built(), "the old table is still the current one");
        // Cancelled before anything: also untouched.
        let ctl = RebuildControl {
            cancelled: &|| true,
            progress: &|_| {},
        };
        assert_eq!(names.rebuild(&together(), &ctl).unwrap(), RebuildEnd::Cancelled);
        assert_eq!(names.read(|b, _| b.groups().len()), before.len());
    }

    #[test]
    fn a_catalogue_that_is_not_built_is_reported_and_keeps_the_table() {
        let names = Names::default();
        names.refresh(&Fixed(vec![cand("graphicbuilder.exe", 5)], true, false)).unwrap();
        assert_eq!(
            names.rebuild(&Fixed(vec![], false, false), &RebuildControl::none()).unwrap(),
            RebuildEnd::NotBuilt
        );
        assert!(!names.is_built());
        assert_eq!(names.read(|b, _| b.groups().len()), 1);
    }
}
