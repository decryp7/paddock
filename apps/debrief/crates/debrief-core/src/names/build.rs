//! Building the name book from the catalogue and keeping it for the app: [`Names`] is the shared handle the assistant
//! tools and the commands use (book, workspace vocabulary, context and Settings switches behind locks that recover
//! from poisoning).

use super::alias_job::{self, AliasJobConfig, AliasJobReport, AliasModel};
use super::book::NameBook;
use super::context::{Cooccurrence, SearchContext};
use super::group::{group, joinable_look_alikes, merge_cooccurring, NameGroup};
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

/// The first and last time a name was seen (each unknown when no event of it has a time).
type TimeRange = (Option<i64>, Option<i64>);

/// The entities named by each of the first [`MERGE_EVENTS`] events that hold `name`, or none when the store cannot
/// answer. A substring search stops at the thirtieth hit, where the phrase search through the full-text index checks every
/// candidate event first (1,074 ms against 20 ms a read on 133,120 events; on 10 million events 330-630 ms against
/// 550-890 ms, for names of 5 to 30,000 events: `docs/performance.md`). A name with fewer than thirty events is a scan of
/// every event whatever the search, so the scan is bounded by the time range the catalogue saw the name in (`range`:
/// from and to, when known).
fn events_naming(store: &(impl SearchSource + Sync + ?Sized), name: &str, range: TimeRange) -> Option<Vec<Vec<String>>> {
    let page = store
        .page(&PageQuery {
            query: Filter {
                text: name.to_string(),
                text_mode: TextMode::Substring,
                from: range.0,
                to: range.1,
                ..Filter::default()
            },
            offset: 0,
            limit: MERGE_EVENTS,
            ..PageQuery::default()
        })
        .ok()?;
    Some(
        page.rows
            .iter()
            .map(|e| {
                crate::entities::extract(&[e.message.as_str()])
                    .entities
                    .into_iter()
                    .map(|x| x.name)
                    .collect()
            })
            .collect(),
    )
}

/// The most reads of look-alike names that run side by side (a registered constant, rule 51, not a Setting: nothing
/// else about a rebuild is tunable, and a rebuild is background work nobody waits on). The bound is the store's query
/// memory budget: each read is a scan on a connection of its own, and on 10 million events 8 side by side failed 7 of 16
/// reads with "Out of Memory Error ... (182.1 MiB/183.1 MiB used)", where 1, 2 and 4 side by side did not and 4 took 41 and
/// 80 % of the time of 1 in two runs (`docs/performance.md`, 2026-11-06). A read that fails ends the best-effort join, so the bound errs low.
pub const MAX_READ_THREADS: usize = 4;

/// Reads for the look-alike names run side by side, this many at most: the cores minus one, and never more than
/// [`MAX_READ_THREADS`].
fn read_workers(reads: usize) -> usize {
    workers_for(crate::threads::threads(crate::threads::WorkKind::Cpu, None), reads)
}

/// `cpu_threads` side by side, at least one and at most [`MAX_READ_THREADS`], and no more than there are reads.
fn workers_for(cpu_threads: usize, reads: usize) -> usize {
    cpu_threads.clamp(1, MAX_READ_THREADS).min(reads.max(1))
}

/// Reads the events of the look-alike groups that could appear together (see [`joinable_look_alikes`]), side by side,
/// and joins the ones that do (best effort: a store that cannot answer leaves the groups as they are). Returns the number
/// of merges, or none when cancelled. Events are counted in the groups' order whatever order the reads finish in.
fn merge_by_cooccurrence(
    store: &(impl SearchSource + Sync + ?Sized),
    groups: &mut Vec<NameGroup>,
    ctl: &RebuildControl<'_>,
) -> Option<usize> {
    let wanted = joinable_look_alikes(groups, MERGE_GROUPS);
    if wanted.len() < 2 {
        return Some(0);
    }
    // The most frequent spelling is what is searched for, within the time range the catalogue saw the group in.
    let names: Vec<(String, TimeRange)> = wanted
        .iter()
        .map(|i| (groups[*i].canonical().to_string(), (groups[*i].first_ts, groups[*i].last_ts)))
        .collect();
    let total = names.len() as u64;
    let report = |done: u64| {
        (ctl.progress)(RebuildProgress {
            phase: RebuildPhase::Events,
            done,
            total,
        })
    };
    report(0);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let stop = std::sync::atomic::AtomicBool::new(false);
    let (tx, rx) = std::sync::mpsc::channel::<(usize, Option<Vec<Vec<String>>>)>();
    let mut results: Vec<Option<Vec<Vec<String>>>> = vec![None; names.len()];
    let mut failed = false;
    let mut cancelled = false;
    std::thread::scope(|scope| {
        for _ in 0..read_workers(names.len()) {
            let tx = tx.clone();
            scope.spawn(|| {
                let tx = tx;
                loop {
                    let n = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if n >= names.len() || stop.load(std::sync::atomic::Ordering::SeqCst) {
                        return;
                    }
                    if tx.send((n, events_naming(store, &names[n].0, names[n].1))).is_err() {
                        return;
                    }
                }
            });
        }
        drop(tx);
        let mut done = 0u64;
        for (n, rows) in rx {
            done += 1;
            failed |= rows.is_none();
            results[n] = rows;
            report(done);
            if failed || (ctl.cancelled)() {
                cancelled = !failed;
                stop.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
    });
    if cancelled {
        return None;
    }
    if failed {
        return Some(0);
    }
    let mut cooc = Cooccurrence::default();
    for event in results.into_iter().flatten().flatten() {
        cooc.add_event(&event);
    }
    report(total);
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
    pub fn refresh(&self, store: &(impl SearchSource + Sync + ?Sized)) -> StoreResult<usize> {
        Ok(match self.rebuild(store, &RebuildControl::none())? {
            RebuildEnd::Built { groups } => groups,
            RebuildEnd::NotBuilt | RebuildEnd::Cancelled => self.read(|b, _| b.groups().len()),
        })
    }

    /// Rebuilds the table from the store's catalogue in phases ([`RebuildPhase`]) with progress, stopping at the next
    /// step when `ctl` says cancelled. The new table is built off the lock and swapped in at the end, so a search
    /// during a rebuild answers from the last table at once and a cancelled rebuild changes nothing. AI aliases and
    /// user facts are kept. It is the caller's job to run this off the request and UI threads.
    pub fn rebuild(&self, store: &(impl SearchSource + Sync + ?Sized), ctl: &RebuildControl<'_>) -> StoreResult<RebuildEnd> {
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

    /// [`Together`] that counts the event reads and the most that ran at once.
    struct Counted {
        inner: Together,
        reads: std::sync::atomic::AtomicUsize,
        live: std::sync::atomic::AtomicUsize,
        most: std::sync::atomic::AtomicUsize,
    }

    impl Counted {
        fn of(cands: Vec<Candidate>) -> Counted {
            Counted {
                inner: Together(cands),
                reads: Default::default(),
                live: Default::default(),
                most: Default::default(),
            }
        }
    }

    impl SearchSource for Counted {
        fn summary(&self) -> StoreResult<crate::ports::SummaryReply> {
            self.inner.summary()
        }
        fn page(&self, q: &crate::ports::PageQuery) -> StoreResult<crate::ports::PageReply> {
            use std::sync::atomic::Ordering::SeqCst;
            self.reads.fetch_add(1, SeqCst);
            let now = self.live.fetch_add(1, SeqCst) + 1;
            self.most.fetch_max(now, SeqCst);
            let reply = self.inner.page(q);
            self.live.fetch_sub(1, SeqCst);
            reply
        }
        fn facet(&self, q: &crate::api::FacetRequest) -> StoreResult<crate::api::Facet> {
            self.inner.facet(q)
        }
        fn exception_groups(&self) -> StoreResult<Vec<crate::exceptions::ExceptionGroup>> {
            self.inner.exception_groups()
        }
        fn catalogue(&self, q: &CatalogueRequest) -> StoreResult<CatalogueReply> {
            self.inner.catalogue(q)
        }
    }

    fn in_source(name: &str, n: u64, source: &str) -> Candidate {
        let mut c = cand(name, n);
        c.seen_in = vec![source.to_string()];
        c.first_ts = Some(0);
        c.last_ts = Some(100);
        c
    }

    #[test]
    fn look_alikes_in_other_sources_are_never_read_and_the_others_are_read_side_by_side() {
        // Two look-alikes in different sources cannot appear in one event: no read at all, nothing joins.
        let apart = Counted::of(vec![
            in_source("gfxbuilder.exe", 9, "a.log"),
            in_source("gfx-bilder.exe", 8, "b.log"),
        ]);
        let names = Names::default();
        assert_eq!(
            names.rebuild(&apart, &RebuildControl::none()).unwrap(),
            RebuildEnd::Built { groups: 2 }
        );
        assert_eq!(
            apart.reads.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "no read for names that cannot meet"
        );
        // In one source they are read (once each), and join as before.
        let same = Counted::of(vec![
            in_source("gfxbuilder.exe", 9, "a.log"),
            in_source("gfx-bilder.exe", 8, "a.log"),
        ]);
        assert_eq!(
            names.rebuild(&same, &RebuildControl::none()).unwrap(),
            RebuildEnd::Built { groups: 1 }
        );
        assert_eq!(same.reads.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(same.most.load(std::sync::atomic::Ordering::SeqCst) <= read_workers(2));
    }

    /// What a recorded search asked for: the text, its mode and the time range.
    type Asked = (String, TextMode, Option<i64>, Option<i64>);

    /// [`Together`] that records how each look-alike was searched for, and can fail every search.
    struct Recording {
        inner: Together,
        asked: Mutex<Vec<Asked>>,
        fail: bool,
    }

    impl Recording {
        fn of(cands: Vec<Candidate>, fail: bool) -> Recording {
            Recording {
                inner: Together(cands),
                asked: Mutex::new(Vec::new()),
                fail,
            }
        }
    }

    impl SearchSource for Recording {
        fn summary(&self) -> StoreResult<crate::ports::SummaryReply> {
            self.inner.summary()
        }
        fn page(&self, q: &crate::ports::PageQuery) -> StoreResult<crate::ports::PageReply> {
            self.asked
                .lock()
                .unwrap()
                .push((q.query.text.clone(), q.query.text_mode, q.query.from, q.query.to));
            if self.fail {
                return Err(crate::ports::StoreError::Invalid("the search failed".into()));
            }
            self.inner.page(q)
        }
        fn facet(&self, q: &crate::api::FacetRequest) -> StoreResult<crate::api::Facet> {
            self.inner.facet(q)
        }
        fn exception_groups(&self) -> StoreResult<Vec<crate::exceptions::ExceptionGroup>> {
            self.inner.exception_groups()
        }
        fn catalogue(&self, q: &CatalogueRequest) -> StoreResult<CatalogueReply> {
            self.inner.catalogue(q)
        }
    }

    /// [`Recording`] of the whole query, for the time range of a read.
    #[test]
    fn a_read_is_bounded_by_the_time_range_the_catalogue_saw_the_group_in() {
        let store = Recording::of(
            vec![
                ranged("gfxbuilder.exe", 9, Some(100), Some(200)),
                ranged("gfx-bilder.exe", 8, Some(150), Some(300)),
            ],
            false,
        );
        Names::default().rebuild(&store, &RebuildControl::none()).unwrap();
        let asked = store.asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 2);
        let mut ranges: Vec<_> = asked
            .iter()
            .map(|(text, mode, from, to)| (text.as_str(), *mode, *from, *to))
            .collect();
        ranges.sort_by_key(|r| r.0);
        assert_eq!(
            ranges,
            vec![
                ("gfx-bilder.exe", TextMode::Substring, Some(150), Some(300)),
                ("gfxbuilder.exe", TextMode::Substring, Some(100), Some(200))
            ]
        );
        // A group whose times are unknown is read without a bound.
        let unknown = Recording::of(
            vec![
                ranged("gfxbuilder.exe", 9, None, None),
                ranged("gfx-bilder.exe", 8, Some(1), Some(2)),
            ],
            false,
        );
        Names::default().rebuild(&unknown, &RebuildControl::none()).unwrap();
        let asked = unknown.asked.lock().unwrap().clone();
        assert!(asked
            .iter()
            .any(|(t, _, from, to)| t == "gfxbuilder.exe" && from.is_none() && to.is_none()));
    }

    #[test]
    fn a_search_that_fails_leaves_the_groups_as_they_are_and_is_not_a_cancel() {
        let names = Names::default();
        let store = Recording::of(
            vec![cand("gfxbuilder.exe", 9), cand("gfx-bilder.exe", 8), cand("tray.exe", 5)],
            false,
        );
        assert_eq!(
            names.rebuild(&store, &RebuildControl::none()).unwrap(),
            RebuildEnd::Built { groups: 2 }
        );
        let failing = Recording::of(
            vec![cand("gfxbuilder.exe", 9), cand("gfx-bilder.exe", 8), cand("tray.exe", 5)],
            true,
        );
        assert_eq!(
            Names::default().rebuild(&failing, &RebuildControl::none()).unwrap(),
            RebuildEnd::Built { groups: 3 },
            "built without the join: best effort, and not reported as cancelled"
        );
    }

    fn ranged(name: &str, n: u64, first: Option<i64>, last: Option<i64>) -> Candidate {
        let mut c = in_source(name, n, "a.log");
        c.first_ts = first;
        c.last_ts = last;
        c
    }

    #[test]
    fn look_alikes_that_never_overlap_in_time_are_not_read_and_touching_or_unknown_ranges_are() {
        let reads = |a: Candidate, b: Candidate| {
            let store = Counted::of(vec![a, b]);
            Names::default().rebuild(&store, &RebuildControl::none()).unwrap();
            store.reads.load(std::sync::atomic::Ordering::SeqCst)
        };
        // Disjoint, either way round: one ends before the other starts.
        assert_eq!(
            reads(
                ranged("gfxbuilder.exe", 9, Some(0), Some(10)),
                ranged("gfx-bilder.exe", 8, Some(20), Some(30))
            ),
            0
        );
        assert_eq!(
            reads(
                ranged("gfxbuilder.exe", 9, Some(20), Some(30)),
                ranged("gfx-bilder.exe", 8, Some(0), Some(10))
            ),
            0
        );
        // They touch at one instant: possible.
        assert_eq!(
            reads(
                ranged("gfxbuilder.exe", 9, Some(0), Some(10)),
                ranged("gfx-bilder.exe", 8, Some(10), Some(20))
            ),
            2
        );
        assert_eq!(
            reads(
                ranged("gfxbuilder.exe", 9, Some(10), Some(20)),
                ranged("gfx-bilder.exe", 8, Some(0), Some(10))
            ),
            2
        );
        // One range is unknown: possible, whichever end is missing.
        assert_eq!(
            reads(
                ranged("gfxbuilder.exe", 9, None, None),
                ranged("gfx-bilder.exe", 8, Some(20), Some(30))
            ),
            2
        );
        assert_eq!(
            reads(
                ranged("gfxbuilder.exe", 9, Some(0), None),
                ranged("gfx-bilder.exe", 8, Some(20), Some(30))
            ),
            2
        );
        assert_eq!(
            reads(
                ranged("gfxbuilder.exe", 9, Some(0), Some(10)),
                ranged("gfx-bilder.exe", 8, None, Some(30))
            ),
            2
        );
    }

    #[test]
    fn the_side_by_side_reads_are_bounded() {
        assert_eq!(read_workers(0), 1);
        assert_eq!(read_workers(1), 1);
        assert!(read_workers(1_000) >= 1 && read_workers(1_000) <= MAX_READ_THREADS);
        // The arithmetic, whatever machine this runs on.
        assert_eq!(workers_for(13, 1_000), MAX_READ_THREADS, "the cores do not raise it past the bound");
        assert_eq!(workers_for(2, 1_000), 2, "fewer cores lower it");
        assert_eq!(workers_for(0, 1_000), 1, "never none");
        assert_eq!(workers_for(13, 3), 3, "no more than there are reads");
        assert_eq!(workers_for(13, 0), 1);
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
        status: std::sync::atomic::AtomicUsize,
        all: std::sync::atomic::AtomicUsize,
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
                self.all.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            Ok(CatalogueReply::default())
        }
        fn catalogue_status(&self, _: &CatalogueStatusRequest) -> StoreResult<crate::catalogue::CatalogueStatus> {
            self.status.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(crate::catalogue::CatalogueStatus {
                generation: 7,
                built: false,
                pending: 2,
                failed: 0,
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
        assert_eq!(
            (
                store.status.load(std::sync::atomic::Ordering::SeqCst),
                store.all.load(std::sync::atomic::Ordering::SeqCst)
            ),
            (3, 0),
            "asked cheaply, never aggregated"
        );
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
