//! Timings of the name layers on 50,000 invented entities. Ignored by default (timings, not behaviour); run with
//! `cargo test -p debrief-core --release --test it names_perf:: -- --ignored --nocapture`.

use debrief_core::api::{Facet, FacetRequest};
use debrief_core::catalogue::{Candidate, Scope};
use debrief_core::catalogue::{CatalogueReply, CatalogueRequest};
use debrief_core::entities::EntityKind;
use debrief_core::exceptions::ExceptionGroup;
use debrief_core::names::alias_job::{self, AliasJobConfig, AliasModel};
use debrief_core::names::build::vocabulary_from;
use debrief_core::names::group::group;
use debrief_core::names::normalise::normalise;
use debrief_core::names::resolve::{NameSettings, Resolver};
use debrief_core::names::{NameBook, SearchContext, Vocabulary};
use debrief_core::ports::{PageQuery, PageReply, StoreError, StoreResult, SummaryReply};
use debrief_core::search_all::SearchSource;
use std::collections::HashSet;
use std::time::Instant;

const A: &[&str] = &[
    "graphic", "report", "tray", "backup", "media", "sync", "print", "disk", "event", "session", "video", "font", "mail", "cache",
    "device", "update",
];
const B: &[&str] = &[
    "builder",
    "agent",
    "worker",
    "player",
    "spooler",
    "indexer",
    "collector",
    "broker",
    "encoder",
    "relay",
    "cleaner",
    "watcher",
    "helper",
    "host",
    "service",
    "manager",
];

fn names(n: usize) -> Vec<Candidate> {
    (0..n)
        .map(|i| {
            let (a, b) = (A[i % 16], B[(i / 16) % 16]);
            let (c, d) = (A[(i / 256) % 16], B[(i / 4096) % 16]);
            let name = match i % 3 {
                0 => format!("{c}{d}{a}{b}.exe"),
                1 => format!("Acme.{c}.{d}.{a}.{b}.exe"),
                _ => format!("{c}-{d}-{a}-{b}_x64.exe"),
            };
            Candidate {
                scope: Scope::Entity(EntityKind::Executable),
                name,
                count: 1 + (i as u64 % 97),
                first_ts: None,
                last_ts: None,
                seen_in: vec![],
            }
        })
        .collect()
}

struct Echo;
impl AliasModel for Echo {
    fn version(&self) -> String {
        "echo".into()
    }
    fn complete(&self, prompt: &str) -> Result<String, String> {
        let items: Vec<String> = (1..=prompt.lines().filter(|l| l.contains(". [")).count())
            .map(|i| format!("{{\"id\":{i},\"name\":\"Thing {i}\",\"aliases\":[\"alias one\",\"alias two\"]}}"))
            .collect();
        Ok(format!("{{\"items\":[{}]}}", items.join(",")))
    }
}

#[test]
#[ignore]
fn timings_at_fifty_thousand_entities() {
    let cands = names(50_000);
    let vocab = vocabulary_from(&cands);
    let t = Instant::now();
    let vendors = HashSet::new();
    for c in &cands {
        std::hint::black_box(normalise(&c.name, &vocab, &vendors));
    }
    let per_name = t.elapsed() / cands.len() as u32;
    let t = Instant::now();
    let groups = group(&cands, &vocab);
    let grouping = t.elapsed();
    let mut book = NameBook::default();
    book.rebuild(groups);
    let n_groups = book.groups().len();
    let ctx = SearchContext::default();
    let r = Resolver {
        book: &book,
        vocab: &vocab,
        ctx: &ctx,
        settings: NameSettings::default(),
    };
    let mut worst = std::time::Duration::ZERO;
    let mut total = std::time::Duration::ZERO;
    let queries = [
        "graphic builder",
        "graphicbuilder",
        "grafic bilder",
        "builder",
        "zzzz qqqq",
        "tray agent 17",
    ];
    for q in queries {
        let t = Instant::now();
        std::hint::black_box(r.resolve(q, 5, None));
        let d = t.elapsed();
        worst = worst.max(d);
        total += d;
    }
    let cfg = AliasJobConfig {
        min_count: 1,
        max_groups: 2_000,
        pause: std::time::Duration::ZERO,
        ..AliasJobConfig::default()
    };
    let t = Instant::now();
    let rep = alias_job::run(&mut book, Some(&Echo), &cfg, &|| false);
    let job = t.elapsed();
    println!("segmentation+normalise per name: {per_name:?}");
    println!("grouping 50k names -> {n_groups} groups: {grouping:?}");
    println!(
        "resolve over {n_groups} groups: mean {:?}, worst {worst:?}",
        total / queries.len() as u32
    );
    println!(
        "alias job (fake model, excluding model time): {} groups in {job:?} = {:.0} groups/s",
        rep.stored,
        rep.stored as f64 / job.as_secs_f64()
    );
    let _ = Vocabulary::builtin();
}

/// Cost of the co-occurrence merge (`group::merge_cooccurring_stats`) against the number of groups of one scope: only
/// groups that have events can join, and only pairs that share a trigram are compared, so the comparisons follow the
/// number of candidates (200 here) and not the number of groups (the old all-pairs loop made 32 million at 8,000).
/// The assertion counts comparisons, never time; the timings are printed (`--nocapture`).
#[test]
fn merge_scaling() {
    use debrief_core::names::group::merge_cooccurring_stats;
    use debrief_core::names::Cooccurrence;
    const CANDIDATES: usize = 200;
    let cands = names(50_000);
    let vocab = vocabulary_from(&cands);
    let all = group(&cands, &vocab);
    println!("groups available: {}", all.len());
    let mut cooc = Cooccurrence::default();
    for g in all.iter().take(CANDIDATES) {
        cooc.add_event(&[g.canonical().to_string(), format!("{}.dll", g.key.replace(' ', ""))]);
    }
    for n in [500usize, 1_000, 2_000, 4_000, 8_000] {
        let mut groups: Vec<_> = all.iter().take(n).cloned().collect();
        let t = Instant::now();
        let stats = merge_cooccurring_stats(&mut groups, &cooc, 0.6);
        let d = t.elapsed();
        println!("merge_cooccurring over {n} groups: {d:?} ({stats:?})");
        assert!(
            stats.candidates <= CANDIDATES.min(n),
            "{n} groups: {stats:?}; only groups with events are candidates"
        );
        let pairs = stats.candidates * stats.candidates.saturating_sub(1) / 2;
        assert!(
            stats.comparisons <= pairs,
            "{n} groups: {stats:?}; at most every pair of candidates is compared"
        );
    }
}

/// A store whose catalogue is not built yet: it counts the requests for the whole catalogue (H2).
struct Unbuilt(std::sync::atomic::AtomicUsize);

impl SearchSource for Unbuilt {
    fn summary(&self) -> StoreResult<SummaryReply> {
        Err(StoreError::Invalid("not part of the fixture".into()))
    }
    fn page(&self, _: &PageQuery) -> StoreResult<PageReply> {
        Err(StoreError::Invalid("not part of the fixture".into()))
    }
    fn facet(&self, _: &FacetRequest) -> StoreResult<Facet> {
        Err(StoreError::Invalid("not part of the fixture".into()))
    }
    fn exception_groups(&self) -> StoreResult<Vec<ExceptionGroup>> {
        Err(StoreError::Invalid("not part of the fixture".into()))
    }
    fn catalogue(&self, req: &CatalogueRequest) -> StoreResult<CatalogueReply> {
        if req.all {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(CatalogueReply {
            built: false,
            pending: 3,
            candidates: Vec::new(),
        })
    }
}

/// H2: the import watcher asks every 500 ms whether the catalogue is built, and each ask read the whole catalogue.
/// While it is not built, a refresh asks the cheap status question and never for the names.
#[test]
fn a_refresh_while_the_catalogue_is_not_built_reads_no_names() {
    let store = Unbuilt(Default::default());
    let names = debrief_core::names::Names::default();
    for _ in 0..20 {
        names.refresh(&store).unwrap();
    }
    let asked = store.0.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        asked, 0,
        "20 refreshes of an unbuilt catalogue read the whole catalogue {asked} times"
    );
}
