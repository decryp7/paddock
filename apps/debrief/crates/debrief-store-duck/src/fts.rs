//! Full-text index (ADR 0014 §1 `Search`): Tantivy in the store's `index/` folder, a rebuildable cache.
//!
//! - Words: [`debrief_core::words`] (smart search) as a Tantivy tokenizer, so the index and the in-memory oracle
//!   agree: runs of letters and digits, lower-cased, and their `camelCase`/`HTTPServer`/digit parts, so
//!   `System.TimeoutException` is `system`, `timeoutexception`, `timeout` and `exception`. An event's text is
//!   its message, exception and template (what the search box looks at).
//! - A search asks for every word, the last as a prefix ([`debrief_core::words::WordQuery`]).
//! - Indexing runs on a background thread from the registered segments of ready sources, up to four segments
//!   at a time (read, document building on a small pool and adding in order, as a pipeline) under the store's
//!   write lock (imports and compaction wait for at most one batch), so the grid is
//!   usable at once and import speed does not depend on indexing speed. A segment is indexed by deleting
//!   the documents of its key, adding its rows and committing, then marking it `indexed` in the catalog: a
//!   crash in between is repaired by indexing it again, never by duplicates.
//! - The writer runs one thread per usable core, at most [`MAX_AUTO_WRITER_THREADS`], with [`WRITER_HEAP_PER_THREAD`] each, as
//!   many as fit in [`WRITER_HEAP_MAX`]
//!   (the import memory: the index is built when an import has finished); the heaps only exist while indexing. Its
//!   threads run at lowered priority (QoS utility on macOS, nice 10 on Linux; they take the indexing thread's).
//! - The commit payload names the catalog's `fts_epoch`; an index that is missing, damaged or from another
//!   epoch is recreated and every segment indexed again.

use crate::segments::SEGMENTS_DIR;
use crate::turns::{wait_after, Gate, Turn};
use crate::writer::{meta_u64, set_meta};
use anyhow::{Context, Result};
use debrief_core::batch::EventId;
use debrief_core::stages::Stages;
use debrief_core::threads::{lower_priority, threads, WorkKind};
use debrief_core::words::WordQuery;
use duckdb::{params, Connection};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ProjectionMask;
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use tantivy::collector::{Collector, Count, SegmentCollector, TopDocs};
use tantivy::query::{BooleanQuery, EmptyQuery, Occur, Query, RegexQuery, TermQuery};
use tantivy::schema::{Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, FAST, INDEXED};
use tantivy::tokenizer::{Token, TokenStream, Tokenizer};
use tantivy::{doc, DocId, Index, IndexReader, IndexWriter, Order, ReloadPolicy, Score, SegmentReader, Term};

/// The writer's threads (at least one) and each thread's heap (the budget's share, at least
/// [`MIN_HEAP_PER_THREAD`]). Unlike Tantivy's `writer_with_num_threads`, not capped at 8 threads.
fn writer_shape(budget: WriterBudget) -> (usize, usize) {
    let threads = budget.threads.max(1);
    (threads, (budget.heap / threads).max(MIN_HEAP_PER_THREAD))
}

/// Folder of the index in a store folder.
pub const INDEX_DIR: &str = "index";

/// Heap of each index writer thread by default (Settings ▸ Performance ▸ Search index memory per thread).
pub const WRITER_HEAP_PER_THREAD: usize = 64 << 20;
/// The most the writer's heaps take together by default: the import memory budget (ADR 0014, 768 MB).
pub const WRITER_HEAP_MAX: usize = 768 << 20;
/// Most writer threads chosen automatically: past 4, the single reader and in-order feeder of the indexing pipeline
/// (and its commit every few segments) are the limit, and 13 threads indexed slower than 4 (performance.md).
pub const MAX_AUTO_WRITER_THREADS: usize = 4;

/// What the index writer may use while indexing: its heap (bytes, shared by its threads; Tantivy needs at least
/// 15 MB per thread) and threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriterBudget {
    pub heap: usize,
    pub threads: usize,
}

/// The least heap Tantivy's index writer accepts per thread.
pub const MIN_HEAP_PER_THREAD: usize = 15 << 20;

impl WriterBudget {
    /// At least one thread and at least [`MIN_HEAP_PER_THREAD`] for each.
    pub fn floored(self) -> WriterBudget {
        let threads = self.threads.max(1);
        WriterBudget {
            heap: self.heap.max(MIN_HEAP_PER_THREAD * threads),
            threads,
        }
    }
}

impl Default for WriterBudget {
    fn default() -> Self {
        // ER-20: CPU-bound work gets the cores minus one, here also bounded by the memory.
        let threads = threads(WorkKind::Cpu, None).clamp(1, (WRITER_HEAP_MAX / WRITER_HEAP_PER_THREAD).min(MAX_AUTO_WRITER_THREADS));
        WriterBudget {
            heap: threads * WRITER_HEAP_PER_THREAD,
            threads,
        }
    }
}

/// Candidates checked for phrases at most; beyond, the total is the candidate count (not exact).
pub const MAX_VERIFY: usize = 1_000;

/// Hits returned by default and at most.
pub const DEFAULT_HITS: usize = 1000;
pub const MAX_HITS: usize = 10_000;

/// Most ids a word search hands to SQL as the search box's filter; beyond, the filter falls back to substrings
/// of the words (16 MB of ids).
pub const MAX_FILTER_HITS: usize = 2_000_000;

/// The name the words tokenizer is registered under.
const TOKENIZER: &str = "debrief_words";

/// [`debrief_core::words::words`] as a Tantivy tokenizer.
#[derive(Clone, Default)]
struct WordsTokenizer;

struct WordsStream {
    tokens: Vec<Token>,
    at: usize,
}

impl Tokenizer for WordsTokenizer {
    type TokenStream<'a> = WordsStream;
    fn token_stream<'a>(&'a mut self, text: &'a str) -> WordsStream {
        let mut tokens = Vec::new();
        debrief_core::words::words(text, &mut |w| {
            tokens.push(Token {
                offset_from: 0,
                offset_to: 0,
                position: tokens.len(),
                text: w,
                position_length: 1,
            })
        });
        WordsStream { tokens, at: 0 }
    }
}

impl TokenStream for WordsStream {
    fn advance(&mut self) -> bool {
        self.at += 1;
        self.at <= self.tokens.len()
    }
    fn token(&self) -> &Token {
        &self.tokens[self.at - 1]
    }
    fn token_mut(&mut self) -> &mut Token {
        &mut self.tokens[self.at - 1]
    }
}

/// Collects the `id` fast field of every hit, up to a cap (one more than the cap tells it was exceeded).
struct Ids(usize);

struct SegmentIds {
    col: tantivy::fastfield::Column<u64>,
    ids: Vec<u64>,
    cap: usize,
}

impl Collector for Ids {
    type Fruit = Vec<u64>;
    type Child = SegmentIds;
    fn for_segment(&self, _ord: u32, segment: &SegmentReader) -> tantivy::Result<SegmentIds> {
        Ok(SegmentIds {
            col: segment.fast_fields().u64("id")?,
            ids: Vec::new(),
            cap: self.0,
        })
    }
    fn requires_scoring(&self) -> bool {
        false
    }
    fn merge_fruits(&self, fruits: Vec<Vec<u64>>) -> tantivy::Result<Vec<u64>> {
        let mut all: Vec<u64> = fruits.into_iter().flatten().take(self.0 + 1).collect();
        all.sort_unstable();
        Ok(all)
    }
}

impl SegmentCollector for SegmentIds {
    type Fruit = Vec<u64>;
    fn collect(&mut self, doc: DocId, _score: Score) {
        if self.ids.len() <= self.cap {
            if let Some(id) = self.col.first(doc) {
                self.ids.push(id);
            }
        }
    }
    fn harvest(self) -> Vec<u64> {
        self.ids
    }
}

#[derive(Clone, Copy)]
struct Fields {
    id: Field,
    source: Field,
    seg: Field,
    text: Field,
}

fn schema() -> (Schema, Fields) {
    let mut b = Schema::builder();
    let id = b.add_u64_field("id", FAST);
    let source = b.add_u64_field("source", INDEXED);
    let seg = b.add_u64_field("seg", INDEXED);
    b.add_i64_field("ts", FAST);
    // Words only, no counts or positions: hits are ordered by time and never scored, and phrases are checked on the
    // rows. (An index made with counts keeps them; it answers the same.)
    let words = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(TOKENIZER)
            .set_index_option(IndexRecordOption::Basic),
    );
    let text = b.add_text_field("text", words);
    (b.build(), Fields { id, source, seg, text })
}

fn fields(index: &Index) -> Result<Fields> {
    let s = index.schema();
    Ok(Fields {
        id: s.get_field("id")?,
        source: s.get_field("source")?,
        seg: s.get_field("seg")?,
        text: s.get_field("text")?,
    })
}

/// The key of a segment file in the index (FNV-1a of its name).
pub fn segment_key(file: &str) -> u64 {
    file.bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Layout version of the index (2: no positions; 3: smart-search words of message, exception and template; 4: joined parts of names);
/// an index of another version is rebuilt.
const LAYOUT: u32 = 4;

fn payload(epoch: u64) -> String {
    format!("fts_epoch={epoch};layout={LAYOUT}")
}

/// An opened index.
pub struct Fts {
    index: Index,
    reader: IndexReader,
    f: Fields,
    budget: WriterBudget,
}

impl Fts {
    /// Opens the index in `dir` for `epoch`, or recreates it (empty) when it is missing, damaged or of
    /// another epoch; the flag says whether it was recreated.
    pub fn open_or_create(dir: &Path, epoch: u64, budget: WriterBudget) -> Result<(Fts, bool)> {
        if let Ok(index) = Index::open_in_dir(dir) {
            let current = index.load_metas().ok().and_then(|m| m.payload).is_some_and(|p| p == payload(epoch));
            if current {
                if let Ok(f) = fields(&index) {
                    index.tokenizers().register(TOKENIZER, WordsTokenizer);
                    let reader = index.reader_builder().reload_policy(ReloadPolicy::Manual).try_into()?;
                    return Ok((Fts { index, reader, f, budget }, false));
                }
            }
        }
        if dir.exists() {
            std::fs::remove_dir_all(dir).with_context(|| format!("removing the old index in {}", dir.display()))?;
        }
        std::fs::create_dir_all(dir)?;
        let (schema, f) = schema();
        let index = Index::create_in_dir(dir, schema).context("creating the full-text index")?;
        index.tokenizers().register(TOKENIZER, WordsTokenizer);
        {
            let mut w: IndexWriter = index.writer_with_num_threads(1, 15 << 20)?;
            let mut prepared = w.prepare_commit()?;
            prepared.set_payload(&payload(epoch));
            prepared.commit()?;
        }
        let reader = index.reader_builder().reload_policy(ReloadPolicy::Manual).try_into()?;
        Ok((Fts { index, reader, f, budget }, true))
    }

    /// The index writer, with [`writer_shape`]'s threads and heaps. Its threads are started by the calling thread and
    /// take its priority on macOS and Linux (the indexing thread runs lowered; Windows starts them at normal priority).
    fn writer(&self) -> Result<IndexWriter> {
        let (threads, heap_per_thread) = writer_shape(self.budget);
        let options = tantivy::indexer::IndexWriterOptions::builder()
            .num_worker_threads(threads)
            .memory_budget_per_thread(heap_per_thread)
            .build();
        Ok(self.index.writer_with_options(options)?)
    }

    fn commit(&self, w: &mut IndexWriter, epoch: u64) -> Result<()> {
        let mut prepared = w.prepare_commit()?;
        prepared.set_payload(&payload(epoch));
        prepared.commit()?;
        // Committed: a reader that cannot reload now shows the change after a later commit reloads it.
        if let Err(e) = self.reader.reload() {
            log::warn!("the full-text index reader could not reload after a commit: {e}");
        }
        Ok(())
    }

    /// Documents in the index.
    pub fn docs(&self) -> u64 {
        self.reader.searcher().num_docs()
    }

    /// Tantivy's segments in the index (the Store pane).
    pub fn segments(&self) -> usize {
        self.reader.searcher().segment_readers().len()
    }

    /// The writer's heap and threads.
    pub fn budget(&self) -> WriterBudget {
        self.budget
    }

    /// The index query of a word search: every term, and every prefix as the start of a word.
    fn query(&self, q: &WordQuery) -> Result<Box<dyn Query>> {
        if q.is_empty() {
            return Ok(Box::new(EmptyQuery));
        }
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        for t in &q.terms {
            let term = Term::from_field_text(self.f.text, t);
            clauses.push((Occur::Must, Box::new(TermQuery::new(term, IndexRecordOption::Basic))));
        }
        for p in &q.prefixes {
            // Words are letters and digits only: nothing in them needs escaping.
            let re = RegexQuery::from_pattern(&format!("{p}.*"), self.f.text)?;
            clauses.push((Occur::Must, Box::new(re)));
        }
        Ok(Box::new(BooleanQuery::new(clauses)))
    }

    /// Hits of a word search (phrases are not checked) and the ids of the first `limit` (at most
    /// [`MAX_VERIFY`]), in time order.
    pub fn search(&self, q: &WordQuery, limit: usize) -> Result<(u64, Vec<EventId>)> {
        let searcher = self.reader.searcher();
        let query = self.query(q)?;
        let top = TopDocs::with_limit(limit.clamp(1, MAX_HITS.max(MAX_VERIFY))).order_by_fast_field::<i64>("ts", Order::Asc);
        let (count, hits) = searcher.search(&*query, &(Count, top))?;
        let mut ids = Vec::with_capacity(hits.len());
        for (_ts, addr) in hits {
            let col = searcher.segment_reader(addr.segment_ord).fast_fields().u64("id")?;
            if let Some(id) = col.first(addr.doc_id) {
                ids.push(EventId(id));
            }
        }
        Ok((count as u64, ids))
    }

    /// The ids (sorted) of every hit of a word search (phrases not checked), or `None` beyond `cap`.
    pub fn all_ids(&self, q: &WordQuery, cap: usize) -> Result<Option<Vec<u64>>> {
        let searcher = self.reader.searcher();
        let ids = searcher.search(&*self.query(q)?, &Ids(cap))?;
        Ok((ids.len() <= cap).then_some(ids))
    }

    /// Indexes the rows of ready sources of `files` (segment files, in order) as one pipeline: the files are read
    /// batch by batch (IO, on this thread), the documents are built on `stages`' pool (CPU), and they are added to
    /// the writer in the order of the files (so the result does not depend on the pool). `done` is called with the
    /// number of files finished so far. Stops when `stages` is cancelled (the caller rolls back).
    fn index_files(
        &self,
        w: &mut IndexWriter,
        seg_dir: &Path,
        files: &[String],
        ready: &HashSet<u32>,
        stages: &Stages<'_>,
        mut done: impl FnMut(usize) + Send,
    ) -> Result<()> {
        let ts_field = self.index.schema().get_field("ts")?;
        let keys: Vec<u64> = files.iter().map(|f| segment_key(f)).collect();
        let mut batches = SegmentBatches::new(seg_dir, files);
        // Reading stops once the process exits (the exit handler waits for running steps only so long).
        let input = std::iter::from_fn(|| if crate::shutdown::down() { None } else { batches.next_batch() });
        let mut first_error: Option<anyhow::Error> = None;
        let mut finished = 0usize;
        let mut seen = vec![false; files.len()];
        let completed = stages.run(
            input,
            |(file, rb)| match rb {
                Ok(rb) => self.documents(rb, keys[*file], ts_field, ready),
                Err(e) => Err(anyhow::anyhow!("{e:#}")),
            },
            |(file, _), docs, _| {
                if first_error.is_some() {
                    return;
                }
                if !std::mem::replace(&mut seen[file], true) {
                    w.delete_term(Term::from_field_u64(self.f.seg, keys[file]));
                    while finished < file {
                        finished += 1;
                        done(finished);
                    }
                }
                let added = docs.and_then(|docs| docs.into_iter().try_for_each(|d| w.add_document(d).map(|_| ()).map_err(Into::into)));
                if let Err(e) = added {
                    first_error = Some(e.context(format!("indexing {}", files[file])));
                }
            },
        );
        if let Some(e) = first_error {
            return Err(e);
        }
        if let Some(e) = batches.error() {
            return Err(e);
        }
        if !completed {
            anyhow::bail!("indexing was stopped");
        }
        // Files without a single batch were never "seen" (empty segments): their old documents still go.
        for (file, seen) in seen.iter().enumerate() {
            if !seen {
                w.delete_term(Term::from_field_u64(self.f.seg, keys[file]));
            }
        }
        done(files.len());
        Ok(())
    }

    /// The documents of the ready rows of one batch of a segment (pure in the batch: any thread).
    fn documents(
        &self,
        rb: &duckdb::arrow::record_batch::RecordBatch,
        key: u64,
        ts_field: Field,
        ready: &HashSet<u32>,
    ) -> Result<Vec<tantivy::TantivyDocument>> {
        use duckdb::arrow::array::{Array, AsArray};
        use duckdb::arrow::datatypes::{Int64Type, UInt32Type, UInt64Type};
        let col = |name: &str| rb.column_by_name(name).with_context(|| format!("column {name}"));
        let ids = col("id")?.as_primitive_opt::<UInt64Type>().context("id type")?;
        let ts = col("ts")?.as_primitive_opt::<Int64Type>().context("ts type")?;
        let src = col("source")?.as_primitive_opt::<UInt32Type>().context("source type")?;
        let msg = col("message")?.as_string_opt::<i32>().context("message type")?;
        let exc = col("exception")?.as_string_opt::<i32>().context("exception type")?;
        let tpl = col("template")?.as_string_opt::<i32>().context("template type")?;
        let mut out = Vec::with_capacity(rb.num_rows());
        for i in 0..rb.num_rows() {
            if !ready.contains(&src.value(i)) {
                continue;
            }
            let mut d = doc!(
                self.f.id => ids.value(i),
                self.f.source => src.value(i) as u64,
                self.f.seg => key,
                ts_field => if ts.is_null(i) { i64::MAX } else { ts.value(i) },
                self.f.text => msg.value(i),
            );
            if !exc.is_null(i) {
                d.add_text(self.f.text, exc.value(i));
            }
            if !tpl.is_null(i) {
                d.add_text(self.f.text, tpl.value(i));
            }
            out.push(d);
        }
        Ok(out)
    }
}

type Batch = duckdb::arrow::record_batch::RecordBatch;

/// The projected record batches of several segment files, one after the other, tagged with the file's index.
/// A read failure ends the iteration and is kept for [`SegmentBatches::error`].
struct SegmentBatches<'a> {
    dir: &'a Path,
    files: &'a [String],
    next_file: usize,
    current: Option<(usize, parquet::arrow::arrow_reader::ParquetRecordBatchReader)>,
    failed: Option<anyhow::Error>,
}

impl<'a> SegmentBatches<'a> {
    fn new(dir: &'a Path, files: &'a [String]) -> Self {
        SegmentBatches {
            dir,
            files,
            next_file: 0,
            current: None,
            failed: None,
        }
    }

    fn open(&self, file: &str) -> Result<parquet::arrow::arrow_reader::ParquetRecordBatchReader> {
        let builder = ParquetRecordBatchReaderBuilder::try_new(std::fs::File::open(self.dir.join(file))?)?;
        let names = ["id", "ts", "source", "message", "exception", "template"];
        let schema = builder.parquet_schema();
        let roots: Vec<usize> = names
            .iter()
            .map(|n| {
                builder
                    .schema()
                    .index_of(n)
                    .with_context(|| format!("segment {file} has no column {n}"))
            })
            .collect::<Result<_>>()?;
        let mask = ProjectionMask::roots(schema, roots);
        Ok(builder.with_projection(mask).with_batch_size(8192).build()?)
    }

    fn next_batch(&mut self) -> Option<(usize, Result<Batch, anyhow::Error>)> {
        if self.failed.is_some() {
            return None;
        }
        loop {
            if let Some((i, reader)) = &mut self.current {
                match reader.next() {
                    Some(Ok(rb)) => return Some((*i, Ok(rb))),
                    Some(Err(e)) => {
                        let i = *i;
                        self.failed = Some(anyhow::Error::new(e).context(format!("reading {}", self.files[i])));
                        return None;
                    }
                    None => self.current = None,
                }
            }
            let i = self.next_file;
            let file = self.files.get(i)?;
            self.next_file += 1;
            match self.open(file) {
                Ok(r) => self.current = Some((i, r)),
                Err(e) => {
                    self.failed = Some(e.context(format!("opening {file}")));
                    return None;
                }
            }
        }
    }

    fn error(&mut self) -> Option<anyhow::Error> {
        self.failed.take()
    }
}

/// State shared by the store and its indexing thread.
pub struct FtsShared {
    pub fts: Fts,
    /// The one writer (Tantivy allows one per index), while there is work.
    writer: Mutex<Option<IndexWriter>>,
    stop: AtomicBool,
    wake: (Mutex<bool>, Condvar),
    /// Segments not indexed yet, as last seen.
    pub pending: AtomicU64,
    /// The last indexing failure.
    pub failure: Mutex<Option<String>>,
    /// The store's flag of a running bulk import (`Inner::bulk_import`; not a live refresh).
    bulk_import: Arc<AtomicBool>,
    /// Whether anything changed since the last look; without news a turn runs no statement.
    gate: Gate,
    /// Tests: the next writes through [`FtsService`] fail after their operations are queued.
    #[cfg(test)]
    pub(crate) fail_writes: AtomicBool,
    /// Tests: the thread kept (leaked) its connection because exit had begun.
    #[cfg(test)]
    pub(crate) kept_at_exit: AtomicBool,
}

/// The indexing thread of a store; stopped and joined on drop.
pub struct FtsService {
    pub shared: Arc<FtsShared>,
    epoch: u64,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl FtsService {
    /// Opens (or recreates) the index of the store in `dir` and starts indexing what is missing.
    pub fn start(
        dir: &Path,
        conn: Connection,
        write_lock: Arc<Mutex<()>>,
        bulk_import: Arc<AtomicBool>,
        generation: Arc<AtomicU64>,
        budget: WriterBudget,
    ) -> Result<FtsService> {
        let epoch = match meta_u64(&conn, "fts_epoch") {
            0 => {
                set_meta(&conn, "fts_epoch", 1)?;
                1
            }
            e => e,
        };
        let (fts, recreated) = Fts::open_or_create(&dir.join(INDEX_DIR), epoch, budget)?;
        if recreated {
            conn.execute_batch("UPDATE segments SET indexed = FALSE")?;
        }
        let shared = Arc::new(FtsShared {
            fts,
            writer: Mutex::new(None),
            stop: AtomicBool::new(false),
            wake: (Mutex::new(true), Condvar::new()),
            pending: AtomicU64::new(0),
            failure: Mutex::new(None),
            bulk_import,
            gate: Gate::new(generation),
            #[cfg(test)]
            fail_writes: AtomicBool::new(false),
            #[cfg(test)]
            kept_at_exit: AtomicBool::new(false),
        });
        crate::shutdown::interrupt_on_exit(&conn);
        let s = shared.clone();
        let store_dir = dir.to_path_buf();
        let for_exit = shared.clone();
        // Listed before it starts, so the exit drain and the tests' guard always see it; the thread drops it last.
        let alive = crate::shutdown::register_thread(
            "debrief-fts",
            Arc::new(move || {
                for_exit.stop.store(true, Ordering::SeqCst);
                for_exit.gate.news();
                *crate::lock(&for_exit.wake.0) = true;
                for_exit.wake.1.notify_all();
            }),
        );
        let handle = std::thread::Builder::new().name("debrief-fts".into()).spawn(move || {
            // Declared first, so it is dropped last: the connection goes before the thread leaves the exit drain's list.
            let _alive = alive;
            let conn = conn;
            run(&s, &store_dir, &conn, &write_lock, epoch);
            if crate::shutdown::down() {
                // The process is exiting: duckdb_disconnect here would race C++ destroying DuckDB's statics (as in
                // catalogue.rs). The OS reclaims what is leaked.
                #[cfg(test)]
                s.kept_at_exit.store(true, Ordering::SeqCst);
                std::mem::forget(conn);
                return;
            }
            drop(conn);
        })?;
        Ok(FtsService {
            shared,
            epoch,
            handle: Mutex::new(Some(handle)),
        })
    }

    /// Asks the thread to look for new segments.
    pub fn wake(&self) {
        self.shared.gate.news();
        *crate::lock(&self.shared.wake.0) = true;
        self.shared.wake.1.notify_all();
    }

    /// Runs `f` with the writer and commits; when either fails, what `f` queued is rolled back.
    fn with_writer(&self, f: impl FnOnce(&mut IndexWriter) -> Result<()>) -> Result<()> {
        let fts = &self.shared.fts;
        let mut slot = crate::lock(&self.shared.writer);
        let w = match slot.as_mut() {
            Some(w) => w,
            None => slot.insert(fts.writer()?),
        };
        let done = f(w).and_then(|()| self.injected_failure()).and_then(|()| fts.commit(w, self.epoch));
        if done.is_err() {
            // What was queued must not ride along with the next commit (the indexer's). The writer is ended, not rolled
            // back: `rollback` builds a new writer and abandons the old one's merges, which go on writing into the folder.
            drop(slot);
            drop_writer(&self.shared);
        }
        done
    }

    /// Tests: fails while [`FtsShared::fail_writes`] is set.
    #[cfg(test)]
    fn injected_failure(&self) -> Result<()> {
        if self.shared.fail_writes.load(Ordering::SeqCst) {
            anyhow::bail!("an injected index write failure");
        }
        Ok(())
    }

    #[cfg(not(test))]
    fn injected_failure(&self) -> Result<()> {
        Ok(())
    }

    /// Deletes a source's documents (it was removed).
    pub fn delete_source(&self, source: u32) -> Result<()> {
        let field = self.shared.fts.f.source;
        self.with_writer(|w| {
            w.delete_term(Term::from_field_u64(field, source as u64));
            Ok(())
        })
    }

    /// Deletes the documents of the id blocks `first .. end` (rows rewritten into new segments, indexed again from
    /// those): a range query on the `id` fast field, so it does not depend on which file held the rows.
    pub fn delete_blocks(&self, blocks: &[(u64, u64)]) -> Result<()> {
        let field = self.shared.fts.f.id;
        self.with_writer(|w| {
            for (first, end) in blocks {
                w.delete_query(Box::new(tantivy::query::RangeQuery::new(
                    std::ops::Bound::Included(Term::from_field_u64(field, *first)),
                    std::ops::Bound::Excluded(Term::from_field_u64(field, *end)),
                )))?;
            }
            Ok(())
        })
    }

    /// Deletes every document (the store was cleared).
    pub fn clear(&self) -> Result<()> {
        self.with_writer(|w| {
            w.delete_all_documents()?;
            Ok(())
        })
    }

    /// Indexes every segment again ("Rebuild index"): empties the index, marks the segments unindexed and wakes
    /// the indexing thread. Run under the store's write lock. Returns the segments to index.
    pub fn rebuild(&self, conn: &Connection) -> Result<u64> {
        self.clear()?;
        *crate::lock(&self.shared.failure) = None;
        let n = conn.execute("UPDATE segments SET indexed = FALSE", [])?;
        self.shared.pending.store(n as u64, Ordering::SeqCst);
        self.wake();
        Ok(n as u64)
    }
}

impl FtsService {
    /// Whether the thread was joined (tests).
    #[cfg(test)]
    pub(crate) fn joined(&self) -> bool {
        crate::lock(&self.handle).is_none()
    }

    /// Asks the thread to end, without waiting.
    pub(crate) fn request_stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.wake();
    }

    /// Stops the thread and waits for it to end (idempotent; the caller bounds the wait, see [`crate::DuckStore::stop_for_exit`]).
    pub(crate) fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.wake();
        let handle = crate::lock(&self.handle).take();
        if let Some(h) = handle {
            let _ = h.join();
        }
    }
}

impl Drop for FtsService {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Segments to index and the ready sources, read under the write lock.
fn todo(conn: &Connection) -> Result<(Vec<String>, HashSet<u32>)> {
    let mut stmt = conn.prepare("SELECT file FROM segments WHERE NOT indexed ORDER BY first_id")?;
    let files = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<duckdb::Result<Vec<_>>>()?;
    let mut stmt = conn.prepare("SELECT id FROM sources WHERE ready")?;
    let ready = stmt
        .query_map([], |r| r.get::<_, u32>(0))?
        .collect::<duckdb::Result<HashSet<_>>>()?;
    Ok((files, ready))
}

/// Segments indexed under one lock and one commit at most (the lock is held that long; imports and compaction wait).
const SEGMENTS_PER_STEP: usize = 4;

/// The pipeline of the indexer: batches of rows are read in order, their documents built on a small shared pool.
fn stages(cancel: &AtomicBool) -> Stages<'_> {
    static POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();
    let pool = POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads(WorkKind::Cpu, None).clamp(1, 4))
            .thread_name(|i| format!("debrief-fts-docs-{i}"))
            .start_handler(|_| {
                lower_priority();
            })
            .build()
            .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().num_threads(1).build().expect("a one-thread pool"))
    });
    Stages {
        pool,
        chunk: 4,
        cancel: Some(cancel),
    }
}

/// Indexes the next segments (up to [`SEGMENTS_PER_STEP`]) in one commit, if the write lock is free and there are
/// some.
fn step(s: &FtsShared, seg_dir: &Path, conn: &Connection, write_lock: &Mutex<()>, epoch: u64) -> Result<Turn> {
    // Counted by the exit handler like a borrowed connection: no step starts once the process exits, and a running
    // one is waited for (a step's statement during exit crashed DuckDB in its static destructors).
    let Some(_running) = crate::shutdown::Active::enter() else {
        return Ok(Turn::Idle);
    };
    // Nothing changed since the last look: nothing to index (and no statement to run).
    if !s.gate.take() {
        return Ok(Turn::Idle);
    }
    let Ok(_writing) = write_lock.try_lock() else {
        // An import holds the store, with its own memory budget: the index writer's heaps are given back now, so the
        // two never add up; the next step starts a new writer. A brief holder (a live refresh) keeps the writer.
        if s.bulk_import.load(Ordering::SeqCst) {
            drop_writer(s);
        }
        s.gate.news();
        return Ok(Turn::Blocked);
    };
    let (files, ready) = todo(conn)?;
    s.pending.store(files.len() as u64, Ordering::SeqCst);
    if files.is_empty() {
        return Ok(Turn::Idle);
    }
    let batch = &files[..files.len().min(SEGMENTS_PER_STEP)];
    let mut slot = crate::lock(&s.writer);
    let w = match slot.as_mut() {
        Some(w) => w,
        None => slot.insert(s.fts.writer()?),
    };
    let total = files.len() as u64;
    let result = s.fts.index_files(w, seg_dir, batch, &ready, &stages(&s.stop), |n| {
        s.pending.store(total - n as u64, Ordering::SeqCst)
    });
    if let Err(e) = result {
        // Not `rollback`: it builds a new writer and abandons the old one's merges, which go on writing into the folder.
        // Dropping the writer after its merges ended discards the documents queued since the last commit just the same
        // (their segments stay unindexed and are indexed again).
        drop(slot);
        drop_writer(s);
        if s.stop.load(Ordering::SeqCst) {
            return Ok(Turn::Idle);
        }
        // Not retried in a loop: a segment that cannot be indexed stays unsearchable and the failure is reported;
        // the others of the batch are tried again.
        let bad = failing_segment(&e, batch).unwrap_or(&batch[0]);
        conn.execute("UPDATE segments SET indexed = TRUE WHERE file = ?", params![bad])?;
        return Err(e.context(format!("indexing {bad}")));
    }
    // The process is exiting: nothing more is written (the documents added go with the writer and the segments stay
    // unindexed, so they are indexed again next time).
    if crate::shutdown::down() {
        drop(slot);
        drop_writer(s);
        return Ok(Turn::Idle);
    }
    s.fts.commit(w, epoch)?;
    // One statement (one commit, one write-ahead-log sync) for the step's segments, not one per segment.
    let marks = vec!["?"; batch.len()].join(", ");
    conn.execute(
        &format!("UPDATE segments SET indexed = TRUE WHERE file IN ({marks})"),
        duckdb::params_from_iter(batch.iter()),
    )?;
    s.pending.store(total - batch.len() as u64, Ordering::SeqCst);
    Ok(Turn::Worked)
}

/// The segment a failure names (its message holds the file name), when it does.
fn failing_segment<'a>(e: &anyhow::Error, batch: &'a [String]) -> Option<&'a String> {
    let text = format!("{e:#}");
    batch.iter().find(|f| text.contains(f.as_str()))
}

/// Whether the indexing thread (and the writer's threads it starts) run lowered. Rule 52: work a person waits for is
/// never starved. This thread holds the store's write lock for a whole step, and live follow waits for that lock. On
/// macOS (QoS utility) and Linux (nice 10) a lowered thread still gets its turn; on Windows, where the writer's threads
/// would not inherit it anyway, a below-normal thread holding the lock could keep a waiting refresh behind other work,
/// so it stays at normal priority there until steps can be cancelled.
const LOWER_INDEXING_THREAD: bool = !cfg!(windows);

/// Lets the background merges finish (they hold memory and allocate until they do), drops the index writer and gives its
/// heaps and its threads' pages back to the OS. The slot stays locked until the writer is gone: a clear or a source
/// removal in that window would find the slot empty, try to open a second writer and fail on the index's lock.
/// A wait for the merges longer than this is logged as a warning.
const SLOW_MERGE_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

fn drop_writer(s: &FtsShared) {
    let mut slot = crate::lock(&s.writer);
    if let Some(w) = slot.take() {
        let started = std::time::Instant::now();
        let waited = w.wait_merging_threads();
        let took = started.elapsed();
        match waited {
            Err(e) => log::warn!("full-text index merges failed after {took:?}: {e}"),
            Ok(()) if took >= SLOW_MERGE_WAIT => log::warn!("waiting for the full-text index merges took {took:?}"),
            Ok(()) => log::info!("full-text index merges ended after {took:?}"),
        }
        debrief_alloc::release();
    }
}

fn run(s: &FtsShared, dir: &Path, conn: &Connection, write_lock: &Mutex<()>, epoch: u64) {
    if LOWER_INDEXING_THREAD {
        // For this thread and the writer's threads it starts (they inherit it on macOS and Linux).
        lower_priority();
    }
    let seg_dir = dir.join(SEGMENTS_DIR);
    while !s.stop.load(Ordering::SeqCst) {
        // One segment per turn, under the write lock (imports and compaction wait for at most one).
        let turn = match step(s, &seg_dir, conn, write_lock, epoch) {
            Ok(Turn::Worked) => {
                s.gate.after(Turn::Worked);
                continue;
            }
            Ok(t) => t,
            Err(e) => {
                log::warn!("full-text indexing failed: {e:#}");
                *crate::lock(&s.failure) = Some(format!("{e:#}"));
                // Not just dropped: a dropped writer leaves its merge threads writing into the folder.
                drop_writer(s);
                Turn::Blocked
            }
        };
        // Idle: free the writer's heap and wait for news (or retry a busy lock soon).
        if s.pending.load(Ordering::SeqCst) == 0 {
            drop_writer(s);
        }
        // A turn that found nothing waits for a wake-up; a blocked or failed one tries again soon.
        let turn = if turn == Turn::Idle && s.pending.load(Ordering::SeqCst) != 0 {
            Turn::Blocked
        } else {
            turn
        };
        s.gate.after(turn);
        let wait = wait_after(turn).unwrap_or_default();
        let mut woken = crate::lock(&s.wake.0);
        if !*woken {
            woken = s.wake.1.wait_timeout(woken, wait).map(|r| r.0).unwrap_or_else(|p| p.into_inner().0);
        }
        *woken = false;
    }
    // Stopping (the store is dropped): the merges must end before the folder can be removed or reopened.
    drop_writer(s);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn segment_keys_differ_and_the_tokenizer_gives_the_words() {
        assert_ne!(segment_key("a"), segment_key("b"));
        let mut t = WordsTokenizer;
        let mut stream = t.token_stream("System.TimeoutException");
        let mut words = Vec::new();
        while stream.advance() {
            assert!(!stream.token().text.is_empty());
            words.push(std::mem::take(&mut stream.token_mut().text));
        }
        assert_eq!(
            words,
            [
                "system",
                "timeoutexception",
                "timeout",
                "exception",
                "systemtimeout",
                "systemtimeoutexception"
            ]
        );
        assert!(!WordsTokenizer.token_stream("").advance());
    }

    #[test]
    fn the_writer_is_dropped_only_when_an_import_holds_the_store() {
        let dir_scratch = tempfile::tempdir().unwrap();
        let dir = dir_scratch.path().join(format!("debrief-fts-drop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&crate::schema::ddl(&debrief_core::batch::EventSchema::plain()))
            .unwrap();
        let (fts, _) = Fts::open_or_create(
            &dir.join(INDEX_DIR),
            1,
            WriterBudget {
                heap: 16 << 20,
                threads: 1,
            },
        )
        .unwrap();
        let s = FtsShared {
            writer: Mutex::new(Some(fts.writer().unwrap())),
            fts,
            stop: AtomicBool::new(false),
            wake: (Mutex::new(false), Condvar::new()),
            pending: AtomicU64::new(1),
            failure: Mutex::new(None),
            bulk_import: Arc::new(AtomicBool::new(false)),
            gate: Gate::new(Arc::new(AtomicU64::new(0))),
            fail_writes: AtomicBool::new(false),
            #[cfg(test)]
            kept_at_exit: AtomicBool::new(false),
        };
        let write_lock = Mutex::new(());
        let held = write_lock.lock().unwrap();
        // A brief holder (a live refresh): the writer stays for the next step.
        assert_eq!(step(&s, &dir.join(SEGMENTS_DIR), &conn, &write_lock, 1).unwrap(), Turn::Blocked);
        assert!(crate::lock(&s.writer).is_some(), "a live refresh does not cost the writer");
        // An import: its budget and the writer's heaps never add up.
        s.bulk_import.store(true, Ordering::SeqCst);
        assert_eq!(step(&s, &dir.join(SEGMENTS_DIR), &conn, &write_lock, 1).unwrap(), Turn::Blocked);
        assert!(
            crate::lock(&s.writer).is_none(),
            "an import's budget and the writer's heaps never add up"
        );
        drop(held);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// H6: an index loop with nothing to index and no news ran a query every turn. An idle turn runs no statement.
    #[test]
    fn a_hundred_idle_turns_run_no_statement() {
        let dir = debrief_ipc::testing::scratch_dir("fts-idle");
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&crate::schema::ddl(&debrief_core::batch::EventSchema::plain()))
            .unwrap();
        let (fts, _) = Fts::open_or_create(
            &dir.join(INDEX_DIR),
            1,
            WriterBudget {
                heap: 16 << 20,
                threads: 1,
            },
        )
        .unwrap();
        let s = FtsShared {
            writer: Mutex::new(None),
            fts,
            stop: AtomicBool::new(false),
            wake: (Mutex::new(false), Condvar::new()),
            pending: AtomicU64::new(0),
            failure: Mutex::new(None),
            bulk_import: Arc::new(AtomicBool::new(false)),
            gate: Gate::new(Arc::new(AtomicU64::new(0))),
            fail_writes: AtomicBool::new(false),
            #[cfg(test)]
            kept_at_exit: AtomicBool::new(false),
        };
        let write_lock = Mutex::new(());
        let seg_dir = dir.join(SEGMENTS_DIR);
        // The first turn looks (it has never looked); after it the store is idle.
        assert_eq!(step(&s, &seg_dir, &conn, &write_lock, 1).unwrap(), Turn::Idle);
        crate::turns::statement_log::start(&conn);
        for _ in 0..100 {
            assert_eq!(step(&s, &seg_dir, &conn, &write_lock, 1).unwrap(), Turn::Idle);
        }
        let ran = crate::turns::statement_log::count(&conn);
        assert!(
            ran.is_empty(),
            "100 idle index turns ran {} statements, the first: {:?}",
            ran.len(),
            ran.first()
        );
        crate::test_scratch::remove_or_defer(&dir);
    }

    #[test]
    fn a_stopped_indexing_thread_leaves_no_writer_and_so_no_merge_thread_behind() {
        // Tantivy's merge threads outlive a dropped writer unless `wait_merging_threads` ran, and then keep writing into
        // the store's folder after its owner removed it (CI 7813 left folders behind).
        let dir = tempfile::Builder::new().prefix("debrief-fts-stop-").tempdir().unwrap();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&crate::schema::ddl(&debrief_core::batch::EventSchema::plain()))
            .unwrap();
        let (fts, _) = Fts::open_or_create(
            &dir.path().join(INDEX_DIR),
            1,
            WriterBudget {
                heap: 16 << 20,
                threads: 1,
            },
        )
        .unwrap();
        let s = FtsShared {
            writer: Mutex::new(Some(fts.writer().unwrap())),
            fts,
            stop: AtomicBool::new(true),
            wake: (Mutex::new(false), Condvar::new()),
            pending: AtomicU64::new(1),
            failure: Mutex::new(None),
            bulk_import: Arc::new(AtomicBool::new(false)),
            fail_writes: AtomicBool::new(false),
            #[cfg(test)]
            kept_at_exit: AtomicBool::new(false),
            gate: Gate::new(Arc::new(AtomicU64::new(0))),
        };
        run(&s, dir.path(), &conn, &Mutex::new(()), 1);
        assert!(
            crate::lock(&s.writer).is_none(),
            "the writer, and with it its merges, is gone when the thread ends"
        );
    }

    /// A shared state over a fresh index in `dir`, with a writer in the slot.
    fn shared_with_writer(dir: &Path, stop: bool) -> FtsShared {
        let (fts, _) = Fts::open_or_create(
            &dir.join(INDEX_DIR),
            1,
            WriterBudget {
                heap: 32 << 20,
                threads: 1,
            },
        )
        .unwrap();
        FtsShared {
            writer: Mutex::new(Some(fts.writer().unwrap())),
            fts,
            stop: AtomicBool::new(stop),
            wake: (Mutex::new(false), Condvar::new()),
            pending: AtomicU64::new(1),
            failure: Mutex::new(None),
            bulk_import: Arc::new(AtomicBool::new(false)),
            fail_writes: AtomicBool::new(false),
            #[cfg(test)]
            kept_at_exit: AtomicBool::new(false),
            gate: Gate::new(Arc::new(AtomicU64::new(0))),
        }
    }

    /// A store connection whose catalog lists one unindexed segment file that does not exist.
    fn conn_with_missing_segment() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&crate::schema::ddl(&debrief_core::batch::EventSchema::plain()))
            .unwrap();
        conn.execute_batch("INSERT INTO segments VALUES ('missing.parquet', 1, 2, 1, 1, FALSE, '[]')")
            .unwrap();
        conn
    }

    #[test]
    fn a_failed_step_ends_its_writer_after_its_merges_instead_of_rolling_it_back() {
        // `rollback` builds a new writer and abandons the old one's merges, which go on writing into the folder.
        let dir = tempfile::Builder::new().prefix("debrief-fts-fail-").tempdir().unwrap();
        let s = shared_with_writer(dir.path(), false);
        let conn = conn_with_missing_segment();
        let e = step(&s, &dir.path().join(SEGMENTS_DIR), &conn, &Mutex::new(()), 1).unwrap_err();
        assert!(format!("{e:#}").contains("missing.parquet"), "{e:#}");
        assert!(
            crate::lock(&s.writer).is_none(),
            "the failed step's writer is gone, its merges ended"
        );
    }

    #[test]
    fn a_step_cut_short_by_the_stop_ends_its_writer_after_its_merges() {
        let dir = tempfile::Builder::new().prefix("debrief-fts-cut-").tempdir().unwrap();
        let s = shared_with_writer(dir.path(), true);
        let conn = conn_with_missing_segment();
        let turn = step(&s, &dir.path().join(SEGMENTS_DIR), &conn, &Mutex::new(()), 1).unwrap();
        assert_eq!(turn, Turn::Idle);
        assert!(
            crate::lock(&s.writer).is_none(),
            "the stopped step's writer is gone, its merges ended"
        );
    }

    #[test]
    fn words_are_indexed_without_frequencies_or_positions() {
        // Searches order hits by time and never score them: the postings only need the documents.
        let (schema, f) = schema();
        let tantivy::schema::FieldType::Str(options) = schema.get_field_entry(f.text).field_type() else {
            panic!("text is a text field")
        };
        let indexing = options.get_indexing_options().expect("indexed");
        assert_eq!(indexing.index_option(), IndexRecordOption::Basic);
        assert!(
            !schema.fields().any(|(_, e)| e.is_stored()),
            "nothing is stored: DuckDB has the rows"
        );
    }

    #[test]
    fn the_writer_runs_the_configured_threads_each_with_its_share_of_the_heap() {
        let mb = 1 << 20;
        assert_eq!(
            writer_shape(WriterBudget {
                heap: 6 * 64 * mb,
                threads: 6
            }),
            (6, 64 * mb)
        );
        // More than Tantivy's 8-thread convenience constructor allows.
        assert_eq!(
            writer_shape(WriterBudget {
                heap: 13 * 32 * mb,
                threads: 13
            }),
            (13, 32 * mb)
        );
        // Tantivy's floor of 15 MB a thread; at least one thread.
        assert_eq!(writer_shape(WriterBudget { heap: mb, threads: 4 }), (4, 15 * mb));
        assert_eq!(writer_shape(WriterBudget { heap: 0, threads: 0 }), (1, 15 * mb));
        // The writer is built with that shape.
        let dir_scratch = tempfile::tempdir().unwrap();
        let dir = dir_scratch.path().join(format!("debrief-fts-shape-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (fts, _) = Fts::open_or_create(
            &dir,
            1,
            WriterBudget {
                heap: 10 * 16 * mb,
                threads: 10,
            },
        )
        .unwrap();
        let w = fts.writer().expect("10 threads of 16 MB");
        drop(w);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A store with two small invented logs, indexed, with its indexing thread stopped, so a test decides every write.
    struct Indexed {
        dir: std::path::PathBuf,
        store: crate::DuckStore,
    }

    impl Drop for Indexed {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    impl Indexed {
        fn new(_tag: &str) -> Indexed {
            use debrief_core::ports::Ingest;
            let dir = debrief_ipc::testing::scratch_dir("fts");
            let logs = dir.join("logs");
            std::fs::create_dir_all(&logs).unwrap();
            std::fs::write(
                logs.join("a.log"),
                "2026-10-14 10:00:00 INFO alpha a1\n2026-10-14 10:00:10 INFO alpha a2\n",
            )
            .unwrap();
            std::fs::write(
                logs.join("b.log"),
                "2026-10-14 10:00:05 INFO alpha b1\n2026-10-14 10:00:15 INFO alpha b2\n",
            )
            .unwrap();
            let store = crate::DuckStore::create(
                &dir.join("store"),
                debrief_core::Profile::default_profile(),
                debrief_core::PluginRegistry::new(),
            )
            .unwrap();
            store.open(&[logs]).unwrap().wait().unwrap();
            assert!(store.wait_for_index(Duration::from_secs(60)).unwrap().ready);
            let f = store.inner.fts.as_ref().unwrap();
            f.shared.stop.store(true, Ordering::SeqCst);
            f.wake();
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            while !crate::lock(&f.handle).as_ref().unwrap().is_finished() {
                assert!(std::time::Instant::now() < deadline, "the indexing thread did not stop");
                std::thread::yield_now();
            }
            Indexed { dir, store }
        }

        fn fts(&self) -> &FtsService {
            self.store.inner.fts.as_ref().unwrap()
        }

        fn source(&self, file: &str) -> u32 {
            use debrief_core::ports::Query;
            self.store
                .summary()
                .unwrap()
                .sources
                .iter()
                .find(|s| s.path.ends_with(file))
                .unwrap()
                .id
        }

        fn hits(&self) -> Vec<EventId> {
            use debrief_core::ports::{Search, SearchRequest};
            let r = self
                .store
                .search(&SearchRequest {
                    text: "alpha".into(),
                    limit: 0,
                })
                .unwrap();
            assert_eq!(r.total as usize, r.ids.len());
            r.ids
        }

        fn ids_of(&self, source: u32) -> Vec<EventId> {
            let conn = self.store.inner.conn().unwrap();
            let mut stmt = conn.prepare("SELECT id FROM events WHERE source = ? ORDER BY id").unwrap();
            stmt.query_map(params![source], |r| r.get::<_, u64>(0))
                .unwrap()
                .map(|r| EventId(r.unwrap()))
                .collect()
        }
    }

    /// A write that fails after its operations were queued is rolled back: the next commit does not carry them.
    #[test]
    fn a_failed_index_write_is_rolled_back_not_committed_with_the_next_one() {
        let t = Indexed::new("rollback");
        assert_eq!(t.hits().len(), 4);
        t.fts().shared.fail_writes.store(true, Ordering::SeqCst);
        assert!(t.fts().delete_source(t.source("a.log")).is_err());
        // Ended, not `rollback`: that abandons the old writer's merges, which go on writing into the folder.
        assert!(
            crate::lock(&t.fts().shared.writer).is_none(),
            "the failed write's writer is gone, its merges ended"
        );
        t.fts().shared.fail_writes.store(false, Ordering::SeqCst);
        t.fts().delete_source(u32::MAX).unwrap();
        assert_eq!(t.hits().len(), 4, "the failed delete was not committed later");
    }

    /// A clock offset whose index delete fails keeps the moved events' documents (old times) and marks the new files
    /// indexed, so every event is found once, also after the next commit.
    #[test]
    fn a_clock_offset_whose_index_delete_fails_keeps_one_hit_per_event() {
        use debrief_core::ports::Skew;
        use debrief_core::skew::{OffsetSpec, SetOffsetRequest};
        let t = Indexed::new("skew-fail");
        t.fts().shared.fail_writes.store(true, Ordering::SeqCst);
        t.store
            .set_source_offset(&SetOffsetRequest {
                spec: OffsetSpec::Manual {
                    source: t.source("b.log"),
                    offset_ms: 20_000,
                },
            })
            .unwrap();
        t.fts().shared.fail_writes.store(false, Ordering::SeqCst);
        let conn = t.store.inner.conn().unwrap();
        let indexed: bool = conn.query_row("SELECT bool_and(indexed) FROM segments", [], |r| r.get(0)).unwrap();
        assert!(indexed, "the new files keep the old documents instead of being indexed twice");
        t.fts().delete_source(u32::MAX).unwrap();
        let mut hits = t.hits();
        hits.sort();
        hits.dedup();
        assert_eq!(hits.len(), 4, "every event once");
    }

    /// The skew rewrite deletes the moved blocks' documents also for a segment that is not marked indexed (its
    /// documents may already be committed), so indexing it again cannot add them twice.
    #[test]
    fn a_clock_offset_deletes_the_old_documents_of_an_unindexed_segment_too() {
        use debrief_core::ports::Skew;
        use debrief_core::skew::{OffsetSpec, SetOffsetRequest};
        let t = Indexed::new("skew-unindexed");
        let b = t.source("b.log");
        let moved = t.ids_of(b);
        assert!(moved.iter().all(|id| t.hits().contains(id)));
        t.store
            .inner
            .conn()
            .unwrap()
            .execute_batch("UPDATE segments SET indexed = FALSE")
            .unwrap();
        t.store
            .set_source_offset(&SetOffsetRequest {
                spec: OffsetSpec::Manual {
                    source: b,
                    offset_ms: 20_000,
                },
            })
            .unwrap();
        let hits = t.hits();
        assert!(
            moved.iter().all(|id| !hits.contains(id)),
            "old documents of the moved events: {hits:?}"
        );
    }
}

/// The indexer thread's end when the process is exiting: it keeps its DuckDB connection (dropping it would run
/// duckdb_disconnect while C++ destroys DuckDB's statics) and leaves the exit drain's list only after that.
#[cfg(test)]
mod exit_tests {
    use super::*;
    use debrief_core::ports::Ingest;
    use debrief_core::{PluginRegistry, Profile};

    const EXIT_CHILD: &str = "DEBRIEF_FTS_EXIT_CHILD";

    /// Runs in the child process only.
    #[test]
    fn an_indexer_turn_after_exit_began_child() {
        let Some(dir) = std::env::var_os(EXIT_CHILD) else { return };
        let store = crate::DuckStore::create(std::path::Path::new(&dir), Profile::default_profile(), PluginRegistry::new()).unwrap();
        let samples = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/serilog-text");
        store.open(&[samples]).unwrap().wait().unwrap();
        assert!(
            store.wait_for_index(std::time::Duration::from_secs(120)).unwrap().ready,
            "index built"
        );
        let service = store.inner.fts.as_ref().expect("an indexing thread");
        let shared = service.shared.clone();
        crate::shutdown::begin_exit();
        service.wake();
        assert!(
            paddock_testkit::poll_until_within(std::time::Duration::from_secs(60), || {
                crate::lock(&service.handle)
                    .as_ref()
                    .is_some_and(std::thread::JoinHandle::is_finished)
            }),
            "the indexing thread did not end once exit began"
        );
        assert!(
            shared.kept_at_exit.load(Ordering::SeqCst),
            "the indexing thread did not keep its connection once exit began (duckdb_disconnect during exit)"
        );
        std::mem::forget(store);
    }

    #[test]
    fn an_indexer_turn_after_exit_began_keeps_its_connection() {
        let dir = debrief_ipc::testing::scratch_dir("fts-exit-turn");
        let out = debrief_core::process::quiet_command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "fts::exit_tests::an_indexer_turn_after_exit_began_child",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(EXIT_CHILD, &dir)
            .output()
            .unwrap();
        crate::test_scratch::remove_or_defer(&dir);
        assert!(
            out.status.success(),
            "{:?}\n{}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
