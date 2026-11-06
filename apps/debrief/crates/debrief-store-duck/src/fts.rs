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
use debrief_core::stages::ByteBudget;
use debrief_core::stages::Stages;
use debrief_core::threads::{lower_priority, threads, WorkKind};
use debrief_core::words::WordQuery;
use duckdb::{params, Connection};
use parquet::arrow::arrow_reader::{ArrowReaderMetadata, ArrowReaderOptions, ParquetRecordBatchReaderBuilder, RowSelection, RowSelector};
use parquet::arrow::ProjectionMask;
use parquet::file::metadata::PageIndexPolicy;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
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

    /// Indexes the rows of ready sources of `files` (segment files, in order) as one pipeline: the files are cut into
    /// slices of row groups, each slice is read and its documents built on `stages`' pool (IO and CPU, in parallel,
    /// ADR 0062 prototype A), and the documents are added to the writer in the order of the files and slices (so the
    /// result does not depend on the pool). The built documents the feed holds at once stay within `feed.memory` bytes
    /// (an estimate from the segments' footers; one slice is never cut below [`MIN_SLICE_ROWS`] rows). No further file
    /// is started once `deadline` has passed and one was started (the caller commits by the clock). `done` is called
    /// with the number of files finished so far. Returns what it did: how many files it started (all finished, unless
    /// the process is exiting, which [`Fed::stop`] says) or an error when `stages` was cancelled (the caller rolls
    /// back).
    fn index_files(
        &self,
        w: &mut IndexWriter,
        feed: Feed<'_>,
        stages: &Stages<'_>,
        window: usize,
        mut done: impl FnMut(usize) + Send,
    ) -> Result<Fed> {
        let Feed {
            seg_dir,
            files,
            ready,
            deadline,
            memory,
            down,
        } = feed;
        let ts_field = self.index.schema().get_field("ts")?;
        let keys: Vec<u64> = files.iter().map(|f| segment_key(f)).collect();
        let committed = AtomicUsize::new(0);
        let budget = ByteBudget::new(memory);
        let built = AtomicUsize::new(0);
        let peak_built = AtomicUsize::new(0);
        let exiting = AtomicBool::new(false);
        let mut units = SegmentUnits::new(seg_dir, files, deadline, &committed, (memory / window.max(1)).max(1));
        // Reading stops once the process exits (the exit handler waits for running steps only so long).
        let input = std::iter::from_fn(|| {
            if down() {
                exiting.store(true, Ordering::SeqCst);
                return None;
            }
            let unit = units.next_unit()?;
            // Waits for the writer to add what is read ahead; the adding thread gives the bytes back.
            budget.acquire(unit.bytes, stages.cancel).then_some(unit)
        });
        let mut first_error: Option<anyhow::Error> = None;
        let mut finished = 0usize;
        let mut docs_added = 0u64;
        let mut slices = 0usize;
        let mut seen = vec![false; files.len()];
        let completed = stages.run_windowed(
            window,
            input,
            |unit| {
                let (docs, bytes) = self
                    .documents(unit, keys[unit.file], ts_field, ready)
                    .with_context(|| format!("reading {}", files[unit.file]))?;
                peak_built.fetch_max(built.fetch_add(bytes, Ordering::SeqCst) + bytes, Ordering::SeqCst);
                Ok::<_, anyhow::Error>((docs, bytes))
            },
            |unit, built_docs, _| {
                committed.fetch_add(1, Ordering::Relaxed);
                slices += 1;
                let file = unit.file;
                let bytes = built_docs.as_ref().map_or(0, |(_, bytes)| *bytes);
                if first_error.is_none() {
                    if !std::mem::replace(&mut seen[file], true) {
                        w.delete_term(Term::from_field_u64(self.f.seg, keys[file]));
                        while finished < file {
                            finished += 1;
                            done(finished);
                        }
                    }
                    let added = built_docs.and_then(|(docs, _)| {
                        docs_added += docs.len() as u64;
                        docs.into_iter()
                            .try_for_each(|d| w.add_document(d).map(|_| ()).map_err(anyhow::Error::from))
                    });
                    if let Err(e) = added {
                        first_error = Some(e.context(format!("indexing {}", files[file])));
                    }
                }
                // The slice's documents are gone: the bytes of the feed and of the read-ahead are free again.
                built.fetch_sub(bytes, Ordering::SeqCst);
                budget.release(unit.bytes);
            },
        );
        if let Some(e) = first_error {
            return Err(e);
        }
        if let Some(e) = units.error() {
            return Err(e);
        }
        if !completed {
            anyhow::bail!("indexing was stopped");
        }
        let started = units.started();
        // Files without a single row group were never "seen" (empty segments): their old documents still go.
        for (file, seen) in seen.iter().enumerate().take(started) {
            if !seen {
                w.delete_term(Term::from_field_u64(self.f.seg, keys[file]));
            }
        }
        done(started);
        let stop = if exiting.load(Ordering::SeqCst) {
            StopCause::Shutdown
        } else {
            units.stop_cause()
        };
        Ok(Fed {
            started,
            docs: docs_added,
            slices,
            peak_bytes: budget.peak(),
            peak_built_bytes: peak_built.load(Ordering::SeqCst),
            stop,
        })
    }

    /// The documents of the ready rows of one slice of a row group (pure in the unit: any thread) and their size in bytes
    /// (text and [`DOC_OVERHEAD_BYTES`] each).
    fn documents(&self, unit: &Unit, key: u64, ts_field: Field, ready: &HashSet<u32>) -> Result<(Vec<tantivy::TantivyDocument>, usize)> {
        let file = std::fs::File::open(&unit.path).with_context(|| format!("opening {}", unit.path.display()))?;
        let mut builder = ParquetRecordBatchReaderBuilder::new_with_metadata(file, unit.meta.clone())
            .with_projection(unit.mask.clone())
            .with_row_groups(vec![unit.group])
            .with_batch_size(DOC_BATCH_ROWS);
        // The rest of the group is skipped (page by page where the file has a page index), not decoded.
        if unit.offset > 0 || unit.rows < unit.group_rows {
            let mut selectors = Vec::with_capacity(3);
            if unit.offset > 0 {
                selectors.push(RowSelector::skip(unit.offset));
            }
            selectors.push(RowSelector::select(unit.rows));
            builder = builder.with_row_selection(RowSelection::from(selectors));
        }
        let mut out = Vec::new();
        let mut bytes = 0usize;
        for rb in builder.build()? {
            bytes += self.documents_of(&rb?, key, ts_field, ready, &mut out)?;
        }
        Ok((out, bytes))
    }

    /// Appends the documents of the ready rows of one record batch; returns their size ([`DOC_OVERHEAD_BYTES`] and the
    /// text of each).
    fn documents_of(
        &self,
        rb: &duckdb::arrow::record_batch::RecordBatch,
        key: u64,
        ts_field: Field,
        ready: &HashSet<u32>,
        out: &mut Vec<tantivy::TantivyDocument>,
    ) -> Result<usize> {
        use duckdb::arrow::array::{Array, AsArray};
        use duckdb::arrow::datatypes::{Int64Type, UInt32Type, UInt64Type};
        let col = |name: &str| rb.column_by_name(name).with_context(|| format!("column {name}"));
        let ids = col("id")?.as_primitive_opt::<UInt64Type>().context("id type")?;
        let ts = col("ts")?.as_primitive_opt::<Int64Type>().context("ts type")?;
        let src = col("source")?.as_primitive_opt::<UInt32Type>().context("source type")?;
        let msg = col("message")?.as_string_opt::<i32>().context("message type")?;
        let exc = col("exception")?.as_string_opt::<i32>().context("exception type")?;
        let tpl = col("template")?.as_string_opt::<i32>().context("template type")?;
        out.reserve(rb.num_rows());
        let mut bytes = 0usize;
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
            bytes += DOC_OVERHEAD_BYTES + msg.value(i).len();
            if !exc.is_null(i) {
                d.add_text(self.f.text, exc.value(i));
                bytes += exc.value(i).len();
            }
            if !tpl.is_null(i) {
                d.add_text(self.f.text, tpl.value(i));
                bytes += tpl.value(i).len();
            }
            out.push(d);
        }
        Ok(bytes)
    }
}

/// What a step feeds the index: segment files (in `seg_dir`, in order), the ready sources, and the time after which no
/// further file is started.
#[derive(Clone, Copy)]
struct Feed<'a> {
    seg_dir: &'a Path,
    files: &'a [String],
    ready: &'a HashSet<u32>,
    deadline: Option<Instant>,
    /// What the feed may hold of built documents at once, in bytes ([`FEED_MEMORY`]).
    memory: usize,
    /// Whether the process is exiting ([`crate::shutdown::down`]; tests inject their own).
    down: fn() -> bool,
}

/// Document bytes the index feed may hold at once, read ahead of the writer: the documents of the slices being read,
/// waiting or being added. Counted as the text bytes of the rows plus [`DOC_OVERHEAD_BYTES`] each, estimated from the
/// segment's footer before a slice is read. It is on top of the writer's heaps ([`WriterBudget`]).
/// Why 64 MiB: a fixed amount that keeps the old 8-thread feed busy (about 8 MiB per slice with 8 slices in flight) while
/// staying a small part of the 768 MB import budget; it is a constant, not a Setting, because it is a safety bound and
/// not a speed knob (the read threads are the Setting). There is no constants register in the repo; the reason lives here.
pub const FEED_MEMORY: usize = 64 << 20;

/// What a built document takes besides its text (the id, source, segment and time values and the string headers).
const DOC_OVERHEAD_BYTES: usize = 128;

/// Fewest rows in a slice, however long the texts (a single event is never cut). Why 16: below it the per-slice cost of
/// opening the file and skipping to the rows outweighs the rows; a slice of 16 events of 1 MB would still be 16 MB.
const MIN_SLICE_ROWS: usize = 16;

/// What an index step did, for its log line.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Fed {
    /// Files started, all of them finished.
    started: usize,
    /// Documents added.
    docs: u64,
    /// Slices of row groups read.
    slices: usize,
    /// The most estimated document bytes held at once by the feed (read ahead, waiting, being added).
    peak_bytes: usize,
    /// The most document bytes (actual text bytes and per-document overhead) built and not yet added at once.
    peak_built_bytes: usize,
    /// Why the feed handed out no further file.
    stop: StopCause,
}

/// Why the feed handed out no further file.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum StopCause {
    /// Every file of the step was fed.
    #[default]
    Done,
    /// The step's time was used up ([`STEP_TIME`]).
    Time,
    /// The process is exiting: the input was cut short and nothing of the step may be kept.
    Shutdown,
}

impl StopCause {
    fn as_str(self) -> &'static str {
        match self {
            StopCause::Done => "all files of the step fed",
            StopCause::Time => "step time used up",
            StopCause::Shutdown => "the process is exiting",
        }
    }
}

/// Whether a step that began at `began` should start no further segment at `now`. The row groups already read but not
/// yet added (`emitted - committed`) wait in the pipeline and each takes about as long to add as the ones added so far,
/// so the step would end at about `now + ahead * per group`, which must stay within `deadline`.
fn out_of_time(deadline: Instant, began: Instant, now: Instant, emitted: usize, committed: usize) -> bool {
    let per_group = now.saturating_duration_since(began) / committed.max(1) as u32;
    let ahead = emitted.saturating_sub(committed) as u32;
    now + per_group * ahead >= deadline
}

/// Rows decoded at a time inside a row group.
const DOC_BATCH_ROWS: usize = 8192;

/// A slice of one row group of one segment file: the unit of reading (and of parallelism) of the index feed. A slice
/// is at most [`DOC_BATCH_ROWS`] rows and about a window's share of the feed's memory.
struct Unit {
    /// Index of the file in the step's list.
    file: usize,
    path: PathBuf,
    /// The file's footer, read once for all its groups.
    meta: ArrowReaderMetadata,
    /// The columns the index needs.
    mask: ProjectionMask,
    group: usize,
    /// Rows of the whole group, the rows skipped before the slice and the rows in it.
    group_rows: usize,
    offset: usize,
    rows: usize,
    /// The estimated size of the slice's documents, counted against the feed's memory.
    bytes: usize,
}

/// The slice being handed out: the rest of a row group.
struct Cursor {
    file: usize,
    meta: ArrowReaderMetadata,
    mask: ProjectionMask,
    group: usize,
    group_rows: usize,
    /// Rows of the group handed out so far.
    next_row: usize,
    /// Rows per slice of this group and the estimated size of a document in it.
    slice_rows: usize,
    row_bytes: usize,
}

/// The text columns' share of a row group and the rows of it: the estimated size of a built document, in bytes.
fn estimated_row_bytes(meta: &ArrowReaderMetadata, group: usize) -> (usize, usize) {
    let rg = meta.metadata().row_group(group);
    let rows = usize::try_from(rg.num_rows()).unwrap_or(0);
    let text: i64 = rg
        .columns()
        .iter()
        .filter(|c| matches!(c.column_path().string().as_str(), "message" | "exception" | "template"))
        .map(|c| c.uncompressed_size())
        .sum();
    let per_row = usize::try_from(text).unwrap_or(0) / rows.max(1);
    (rows, per_row.saturating_add(DOC_OVERHEAD_BYTES))
}

/// The slices of the row groups of several segment files, one after the other. A file whose footer cannot be read ends the
/// iteration and is kept for [`SegmentUnits::error`]. After `deadline` no further file is started (the first always is),
/// and once the deadline cut the iteration it stays cut.
struct SegmentUnits<'a> {
    dir: &'a Path,
    files: &'a [String],
    deadline: Option<Instant>,
    began: Instant,
    /// Slices handed out, and added to the index so far (the difference waits in the pipeline).
    emitted: usize,
    committed: &'a AtomicUsize,
    next_file: usize,
    /// The bytes one slice should come to at most (the feed's memory over its window).
    slice_bytes: usize,
    current: Option<Cursor>,
    failed: Option<anyhow::Error>,
    /// Set when the deadline ended the iteration: it never starts a file after that, however the clock reads.
    out_of_time: bool,
}

impl<'a> SegmentUnits<'a> {
    fn new(dir: &'a Path, files: &'a [String], deadline: Option<Instant>, committed: &'a AtomicUsize, slice_bytes: usize) -> Self {
        SegmentUnits {
            dir,
            files,
            deadline,
            began: Instant::now(),
            emitted: 0,
            committed,
            next_file: 0,
            slice_bytes,
            current: None,
            failed: None,
            out_of_time: false,
        }
    }

    fn open(&self, file: &str) -> Result<(ArrowReaderMetadata, ProjectionMask)> {
        let f = std::fs::File::open(self.dir.join(file))?;
        // The page index (when the file has one) lets a slice skip the pages before it.
        let options = ArrowReaderOptions::new().with_page_index_policy(PageIndexPolicy::Optional);
        let meta = ArrowReaderMetadata::load(&f, options)?;
        let names = ["id", "ts", "source", "message", "exception", "template"];
        let roots: Vec<usize> = names
            .iter()
            .map(|n| {
                meta.schema()
                    .index_of(n)
                    .with_context(|| format!("segment {file} has no column {n}"))
            })
            .collect::<Result<_>>()?;
        let mask = ProjectionMask::roots(meta.parquet_schema(), roots);
        Ok((meta, mask))
    }

    /// The cursor of the next non-empty row group of `meta`, from `group` on.
    fn cursor(file: usize, meta: &ArrowReaderMetadata, mask: &ProjectionMask, mut group: usize, slice_bytes: usize) -> Option<Cursor> {
        while group < meta.metadata().num_row_groups() {
            let (rows, row_bytes) = estimated_row_bytes(meta, group);
            if rows > 0 {
                return Some(Cursor {
                    file,
                    meta: meta.clone(),
                    mask: mask.clone(),
                    group,
                    group_rows: rows,
                    next_row: 0,
                    slice_rows: (slice_bytes / row_bytes).clamp(MIN_SLICE_ROWS, DOC_BATCH_ROWS),
                    row_bytes,
                });
            }
            group += 1;
        }
        None
    }

    fn next_unit(&mut self) -> Option<Unit> {
        if self.failed.is_some() || self.out_of_time {
            return None;
        }
        loop {
            if let Some(c) = &mut self.current {
                if c.next_row < c.group_rows {
                    let rows = c.slice_rows.min(c.group_rows - c.next_row);
                    let unit = Unit {
                        file: c.file,
                        path: self.dir.join(&self.files[c.file]),
                        meta: c.meta.clone(),
                        mask: c.mask.clone(),
                        group: c.group,
                        group_rows: c.group_rows,
                        offset: c.next_row,
                        rows,
                        bytes: rows * c.row_bytes,
                    };
                    c.next_row += rows;
                    self.emitted += 1;
                    return Some(unit);
                }
                let (file, group) = (c.file, c.group + 1);
                let (meta, mask) = (c.meta.clone(), c.mask.clone());
                self.current = Self::cursor(file, &meta, &mask, group, self.slice_bytes);
                continue;
            }
            let i = self.next_file;
            let file = self.files.get(i)?;
            if i > 0
                && self
                    .deadline
                    .is_some_and(|d| out_of_time(d, self.began, Instant::now(), self.emitted, self.committed.load(Ordering::Relaxed)))
            {
                self.out_of_time = true;
                return None;
            }
            self.next_file += 1;
            match self.open(file) {
                Ok((meta, mask)) => self.current = Self::cursor(i, &meta, &mask, 0, self.slice_bytes),
                Err(e) => {
                    self.failed = Some(e.context(format!("opening {file}")));
                    return None;
                }
            }
        }
    }

    /// Files started so far (all of them, once the iteration ended).
    fn started(&self) -> usize {
        self.next_file
    }

    /// Why the iteration ended: the time, or every file fed.
    fn stop_cause(&self) -> StopCause {
        if self.out_of_time {
            StopCause::Time
        } else {
            StopCause::Done
        }
    }

    fn error(&mut self) -> Option<anyhow::Error> {
        self.failed.take()
    }
}

/// State shared by the store and its indexing thread.
pub struct FtsShared {
    pub fts: Fts,
    /// How long a step keeps starting segments ([`STEP_TIME`]; tests shorten it).
    step_time: AtomicU64,
    /// The threads that read row groups and build documents for [`index_files`](Fts::index_files).
    pool: rayon::ThreadPool,
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
}

/// The indexing thread of a store; stopped and joined on drop.
pub struct FtsService {
    pub shared: Arc<FtsShared>,
    epoch: u64,
    handle: Option<std::thread::JoinHandle<()>>,
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
        read_threads: usize,
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
            step_time: AtomicU64::new(STEP_TIME.as_micros() as u64),
            pool: read_pool(read_threads),
            writer: Mutex::new(None),
            stop: AtomicBool::new(false),
            wake: (Mutex::new(true), Condvar::new()),
            pending: AtomicU64::new(0),
            failure: Mutex::new(None),
            bulk_import,
            gate: Gate::new(generation),
            #[cfg(test)]
            fail_writes: AtomicBool::new(false),
        });
        crate::shutdown::interrupt_on_exit(&conn);
        let s = shared.clone();
        let store_dir = dir.to_path_buf();
        let handle = std::thread::Builder::new()
            .name("debrief-fts".into())
            .spawn(move || run(&s, &store_dir, &conn, &write_lock, epoch))?;
        Ok(FtsService {
            shared,
            epoch,
            handle: Some(handle),
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
            // What was queued must not ride along with the next commit (the indexer's).
            if let Err(e) = w.rollback() {
                log::warn!("the full-text index writer could not roll back: {e}");
            }
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

impl Drop for FtsService {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.wake();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
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
/// Why 64: with tiny segments the clock alone would let a step start hundreds, and one `UPDATE ... IN (?)` marks them all;
/// the step's time ([`STEP_TIME`]) is what normally ends a step. Not a Setting: no user-visible effect either way.
const MAX_SEGMENTS_PER_STEP: usize = 64;

/// The time a step keeps starting segments: the commit follows the clock, so a faster feed does not commit more often
/// (every commit leaves a small index segment per writer thread for the merges) and a slower one still lets imports in.
const STEP_TIME: Duration = Duration::from_secs(2);

/// Most threads reading row groups and building documents when the setting is automatic (cores minus one up to this:
/// the work is CPU and file reads; past 8 the single in-order writer is the limit, performance.md).
const MAX_READ_THREADS: usize = 8;

/// Read threads chosen automatically: the cores minus one, at most [`MAX_READ_THREADS`], at least one.
pub fn auto_read_threads() -> usize {
    threads(WorkKind::Cpu, None).clamp(1, MAX_READ_THREADS)
}

/// The pool of the indexer: row group slices are read and turned into documents on it, committed in order. `threads` 0 is
/// automatic. Its threads run lowered ([`lower_priority`]).
fn read_pool(threads: usize) -> rayon::ThreadPool {
    let n = if threads == 0 { auto_read_threads() } else { threads.clamp(1, 256) };
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .thread_name(|i| format!("debrief-fts-docs-{i}"))
        .start_handler(|_| {
            lower_priority();
        })
        .build()
        .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().num_threads(1).build().expect("a one-thread pool"))
}

/// Slices read, built or waiting at once: two for each thread of the pool (the pool never waits for the writer while the
/// next slices wait in the queue); the byte budget ([`FEED_MEMORY`]) bounds them further.
fn feed_window(pool: &rayon::ThreadPool) -> usize {
    pool.current_num_threads() * 2
}

/// The pipeline of the indexer over `pool`.
fn stages<'a>(pool: &'a rayon::ThreadPool, cancel: &'a AtomicBool) -> Stages<'a> {
    Stages {
        pool,
        chunk: 1,
        cancel: Some(cancel),
    }
}

/// Indexes the next segments (as many as [`STEP_TIME`] allows, at most [`MAX_SEGMENTS_PER_STEP`]) in one commit, if the write lock is free and there are
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
    let batch = &files[..files.len().min(MAX_SEGMENTS_PER_STEP)];
    let mut slot = crate::lock(&s.writer);
    let w = match slot.as_mut() {
        Some(w) => w,
        None => slot.insert(s.fts.writer()?),
    };
    let total = files.len() as u64;
    let fed_at = Instant::now();
    let result = s.fts.index_files(
        w,
        Feed {
            seg_dir,
            files: batch,
            ready: &ready,
            deadline: Some(Instant::now() + Duration::from_micros(s.step_time.load(Ordering::Relaxed))),
            memory: FEED_MEMORY,
            down: crate::shutdown::down,
        },
        &stages(&s.pool, &s.stop),
        feed_window(&s.pool),
        |n| s.pending.store(total - n as u64, Ordering::SeqCst),
    );
    let fed_for = fed_at.elapsed();
    let fed = match result {
        Ok(f) => f,
        Err(e) => {
            let _ = w.rollback();
            if s.stop.load(Ordering::SeqCst) {
                return Ok(Turn::Idle);
            }
            // Not retried in a loop: a segment that cannot be indexed stays unsearchable and the failure is reported;
            // the others of the batch are tried again.
            let bad = failing_segment(&e, batch).unwrap_or(&batch[0]);
            conn.execute("UPDATE segments SET indexed = TRUE WHERE file = ?", params![bad])?;
            return Err(e.context(format!("indexing {bad}")));
        }
    };
    let batch = &batch[..fed.started];
    // The process is exiting: nothing more is written (the documents added go with the writer and the segments stay
    // unindexed, so they are indexed again next time). The input may have been cut short by it, which no other check
    // sees.
    if fed.stop == StopCause::Shutdown || crate::shutdown::down() {
        log::info!(
            "full-text index step abandoned: {} ({} segments started, {} documents added, not committed)",
            fed.stop.as_str(),
            fed.started,
            fed.docs
        );
        let _ = w.rollback();
        return Ok(Turn::Idle);
    }
    let committing = Instant::now();
    s.fts.commit(w, epoch)?;
    step_log(&fed, files.len(), fed_for, committing.elapsed());
    // One statement (one commit, one write-ahead-log sync) for the step's segments, not one per segment.
    let marks = vec!["?"; batch.len()].join(", ");
    conn.execute(
        &format!("UPDATE segments SET indexed = TRUE WHERE file IN ({marks})"),
        duckdb::params_from_iter(batch.iter()),
    )?;
    s.pending.store(total - batch.len() as u64, Ordering::SeqCst);
    Ok(Turn::Worked)
}

/// Steps whose summary is logged at info: the first of a run of them and then every [`STEP_LOG_EVERY`]th (the rest at debug),
/// so a million-event index leaves a few hundred lines, not one per second.
static STEPS_LOGGED: AtomicUsize = AtomicUsize::new(0);
/// Why 20: at one step a second that is a line every 20 s at info, enough to follow a long index in a bundle.
const STEP_LOG_EVERY: usize = 20;

/// The summary line of a committed step (ER-27): what was indexed, how it stopped and the memory the feed held.
fn step_log(fed: &Fed, files_todo: usize, read_and_added: Duration, committed_in: Duration) {
    let n = STEPS_LOGGED.fetch_add(1, Ordering::Relaxed);
    let level = if n.is_multiple_of(STEP_LOG_EVERY) {
        log::Level::Info
    } else {
        log::Level::Debug
    };
    log::log!(
        level,
        "full-text index step {n}: {} of {files_todo} segments, {} documents in {} slices, read and added in {read_and_added:.2?}, \
         committed in {committed_in:.2?}; stopped because: {}; feed held at most {} MB estimated ({} MB built)",
        fed.started,
        fed.docs,
        fed.slices,
        fed.stop.as_str(),
        fed.peak_bytes >> 20,
        fed.peak_built_bytes >> 20,
    );
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
fn drop_writer(s: &FtsShared) {
    let mut slot = crate::lock(&s.writer);
    if let Some(w) = slot.take() {
        if let Err(e) = w.wait_merging_threads() {
            log::warn!("full-text index merges failed: {e}");
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
                *crate::lock(&s.writer) = None;
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
        let dir = std::env::temp_dir().join(format!("debrief-fts-drop-{}", std::process::id()));
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
            pool: read_pool(1),
            step_time: AtomicU64::new(STEP_TIME.as_micros() as u64),
            fts,
            stop: AtomicBool::new(false),
            wake: (Mutex::new(false), Condvar::new()),
            pending: AtomicU64::new(1),
            failure: Mutex::new(None),
            bulk_import: Arc::new(AtomicBool::new(false)),
            gate: Gate::new(Arc::new(AtomicU64::new(0))),
            fail_writes: AtomicBool::new(false),
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
        let dir = std::env::temp_dir().join(format!("debrief-fts-idle-{}", std::process::id()));
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
            writer: Mutex::new(None),
            pool: read_pool(1),
            step_time: AtomicU64::new(STEP_TIME.as_micros() as u64),
            fts,
            stop: AtomicBool::new(false),
            wake: (Mutex::new(false), Condvar::new()),
            pending: AtomicU64::new(0),
            failure: Mutex::new(None),
            bulk_import: Arc::new(AtomicBool::new(false)),
            gate: Gate::new(Arc::new(AtomicU64::new(0))),
            fail_writes: AtomicBool::new(false),
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
        let _ = std::fs::remove_dir_all(&dir);
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
        let dir = std::env::temp_dir().join(format!("debrief-fts-shape-{}", std::process::id()));
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
        fn new(tag: &str) -> Indexed {
            use debrief_core::ports::Ingest;
            let dir = std::env::temp_dir().join(format!("debrief-fts-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
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
            let handle = f.handle.as_ref().unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            while !handle.is_finished() {
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

    /// A step that has used its time stops at a segment boundary, after at least one segment, and says how many it
    /// finished: the commit follows the clock, not a segment count.
    #[test]
    fn a_step_out_of_time_stops_after_the_segment_it_is_in() {
        let t = Indexed::new("deadline");
        let conn = t.store.inner.conn().unwrap();
        let files: Vec<String> = conn
            .prepare("SELECT file FROM segments ORDER BY first_id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        // The same file three times: one segment is all a small store has.
        let files = vec![files[0].clone(), files[0].clone(), files[0].clone()];
        let s = &t.fts().shared;
        let seg_dir = t.dir.join("store").join(SEGMENTS_DIR);
        let cancel = AtomicBool::new(false);
        let ready: HashSet<u32> = [t.source("a.log"), t.source("b.log")].into_iter().collect();
        let mut w = s.fts.writer().unwrap();
        let feed = |deadline| Feed {
            seg_dir: &seg_dir,
            files: &files,
            ready: &ready,
            deadline,
            memory: FEED_MEMORY,
            down: crate::shutdown::down,
        };
        let spent = std::time::Instant::now();
        let n = s
            .fts
            .index_files(&mut w, feed(Some(spent)), &stages(&s.pool, &cancel), feed_window(&s.pool), |_| {})
            .unwrap();
        assert_eq!(n.started, 1, "the deadline had passed: one segment, not {}", files.len());
        assert_eq!(n.stop, StopCause::Time);
        let n = s
            .fts
            .index_files(&mut w, feed(None), &stages(&s.pool, &cancel), feed_window(&s.pool), |_| {})
            .unwrap();
        assert_eq!(n.started, files.len(), "no deadline: all of them");
        assert_eq!(n.stop, StopCause::Done);
    }

    /// A segment of several row groups (read in parallel) is indexed whole and in order: every event once.
    #[test]
    fn a_segment_of_several_row_groups_is_indexed_whole_and_once() {
        use debrief_core::ports::Ingest;
        let dir = std::env::temp_dir().join(format!("debrief-fts-groups-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let logs = dir.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let rows = crate::segments::ROW_GROUP_ROWS * 3 + 1_000;
        let mut text = String::new();
        for i in 0..rows {
            text.push_str(&format!("2026-10-14 10:{:02}:{:02} INFO alpha line{i}\n", (i / 60) % 60, i % 60));
        }
        std::fs::write(logs.join("big.log"), text).unwrap();
        let store = crate::DuckStore::create(
            &dir.join("store"),
            debrief_core::Profile::default_profile(),
            debrief_core::PluginRegistry::new(),
        )
        .unwrap();
        store.open(&[logs]).unwrap().wait().unwrap();
        assert!(store.wait_for_index(Duration::from_secs(120)).unwrap().ready);
        let file: String = store
            .inner
            .conn()
            .unwrap()
            .query_row("SELECT file FROM segments ORDER BY first_id LIMIT 1", [], |r| r.get(0))
            .unwrap();
        let reader = parquet::file::reader::SerializedFileReader::new(
            std::fs::File::open(dir.join("store").join(SEGMENTS_DIR).join(&file)).unwrap(),
        )
        .unwrap();
        let groups = parquet::file::reader::FileReader::metadata(&reader).num_row_groups();
        assert!(groups >= 2, "the segment has several row groups: {groups}");
        assert_eq!(store.inner.fts.as_ref().unwrap().shared.fts.docs(), rows as u64, "every event once");
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The read-ahead of the pipeline counts against a step's time: groups read but not yet added are as good as spent.
    #[test]
    fn a_step_counts_the_groups_waiting_in_the_pipeline_against_its_time() {
        let began = Instant::now();
        let now = began + Duration::from_secs(1);
        let deadline = began + Duration::from_millis(2500);
        // 2 groups added in 1 s, 4 more waiting: about 2 s more, past the deadline.
        assert!(out_of_time(deadline, began, now, 6, 2));
        // Nothing waiting: time left.
        assert!(!out_of_time(deadline, began, now, 2, 2));
        // Nothing added yet counts the whole time so far per group.
        assert!(out_of_time(deadline, began, now, 3, 0));
        assert!(out_of_time(deadline, began, deadline, 0, 0));
    }

    /// The index feed is bounded by bytes, not by rows: long events (10 KB each) read from row groups of a thousand rows
    /// leave at most the feed's memory of documents built at once, however many the pool and the queue could hold.
    #[test]
    fn the_feed_holds_long_events_within_its_byte_budget() {
        let dir = std::env::temp_dir().join(format!("debrief-fts-bytes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let seg_dir = dir.join("segments");
        std::fs::create_dir_all(&seg_dir).unwrap();
        let (files, rows) = write_long_segments(&seg_dir, 2, 3, 600, 10 << 10);
        let (fts, _) = Fts::open_or_create(
            &dir.join(INDEX_DIR),
            1,
            WriterBudget {
                heap: 32 << 20,
                threads: 1,
            },
        )
        .unwrap();
        let pool = read_pool(4);
        let cancel = AtomicBool::new(false);
        let ready: HashSet<u32> = [1].into_iter().collect();
        let run = |memory: usize| {
            let mut w = fts.writer().unwrap();
            let fed = fts
                .index_files(
                    &mut w,
                    Feed {
                        seg_dir: &seg_dir,
                        files: &files,
                        ready: &ready,
                        deadline: None,
                        memory,
                        down: crate::shutdown::down,
                    },
                    &stages(&pool, &cancel),
                    feed_window(&pool),
                    |_| {},
                )
                .unwrap();
            w.rollback().unwrap();
            fed
        };
        let memory = 4 << 20;
        let bounded = run(memory);
        assert_eq!(bounded.docs, rows as u64, "every row is read");
        assert!(
            bounded.peak_built_bytes <= memory,
            "{} MB of documents were built at once against a budget of {} MB",
            bounded.peak_built_bytes >> 20,
            memory >> 20
        );
        assert!(bounded.peak_bytes <= memory, "the estimate held {} bytes", bounded.peak_bytes);
        // The probe can see a breach: with a budget of 1 GB the same files put far more than 4 MB in flight.
        let unbounded = run(1 << 30);
        assert!(
            unbounded.peak_built_bytes > 2 * memory,
            "the unbounded feed built {} MB at once; the test would not notice a breach",
            unbounded.peak_built_bytes >> 20
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Writes `files` segments of `groups` row groups of `rows` events of about `text` bytes each (source 1) and returns
    /// their names and the number of events.
    fn write_long_segments(dir: &Path, files: usize, groups: usize, rows: usize, text: usize) -> (Vec<String>, usize) {
        use duckdb::arrow::array::{ArrayRef, Int64Array, StringArray, UInt32Array, UInt64Array};
        use duckdb::arrow::datatypes::{DataType, Field as AField, Schema as ASchema};
        use duckdb::arrow::record_batch::RecordBatch;
        use parquet::arrow::ArrowWriter;
        let schema = Arc::new(ASchema::new(vec![
            AField::new("id", DataType::UInt64, false),
            AField::new("ts", DataType::Int64, true),
            AField::new("source", DataType::UInt32, false),
            AField::new("message", DataType::Utf8, false),
            AField::new("exception", DataType::Utf8, true),
            AField::new("template", DataType::Utf8, true),
        ]));
        let mut names = Vec::new();
        let mut id = 0u64;
        for f in 0..files {
            let name = format!("long-{f}.parquet");
            let props = parquet::file::properties::WriterProperties::builder()
                .set_max_row_group_row_count(Some(rows))
                .build();
            let mut w = ArrowWriter::try_new(std::fs::File::create(dir.join(&name)).unwrap(), schema.clone(), Some(props)).unwrap();
            for _ in 0..groups {
                let ids: Vec<u64> = (0..rows as u64).map(|i| id + i).collect();
                let message: Vec<String> = ids.iter().map(|i| format!("alpha {i} {}", "word ".repeat(text / 5))).collect();
                id += rows as u64;
                let cols: Vec<ArrayRef> = vec![
                    Arc::new(UInt64Array::from(ids.clone())),
                    Arc::new(Int64Array::from(ids.iter().map(|i| *i as i64).collect::<Vec<_>>())),
                    Arc::new(UInt32Array::from(vec![1u32; rows])),
                    Arc::new(StringArray::from(message)),
                    Arc::new(StringArray::from(vec![None::<&str>; rows])),
                    Arc::new(StringArray::from(vec![None::<&str>; rows])),
                ];
                w.write(&RecordBatch::try_new(schema.clone(), cols).unwrap()).unwrap();
                w.flush().unwrap();
            }
            w.close().unwrap();
            names.push(name);
        }
        (names, files * groups * rows)
    }

    /// A segment is cut into slices of a row group without losing or doubling a row, whatever the slice size.
    #[test]
    fn slices_of_a_row_group_cover_every_row_once() {
        let dir = std::env::temp_dir().join(format!("debrief-fts-slices-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (files, rows) = write_long_segments(&dir, 1, 3, 1000, 200);
        let committed = AtomicUsize::new(0);
        // 40 KB slices of 200-byte events: about 160 rows each.
        let mut units = SegmentUnits::new(&dir, &files, None, &committed, 40 << 10);
        let mut covered = vec![0u8; rows];
        let mut n = 0;
        while let Some(u) = units.next_unit() {
            assert!(u.rows >= 1 && u.rows <= DOC_BATCH_ROWS);
            for r in 0..u.rows {
                covered[u.group * 1000 + u.offset + r] += 1;
            }
            n += 1;
        }
        assert!(n > 3, "cut into several slices per group: {n}");
        assert!(covered.iter().all(|c| *c == 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Once the deadline has cut the iteration it stays cut, whatever the clock reads next.
    #[test]
    fn the_deadline_cut_is_sticky() {
        let dir = std::env::temp_dir().join(format!("debrief-fts-sticky-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (files, _) = write_long_segments(&dir, 3, 1, 10, 50);
        let committed = AtomicUsize::new(0);
        let mut units = SegmentUnits::new(&dir, &files, Some(Instant::now()), &committed, 1 << 20);
        while units.next_unit().is_some() {}
        assert_eq!(units.started(), 1, "the deadline had passed: the first file only");
        assert_eq!(units.stop_cause(), StopCause::Time);
        // The deadline moves away (a slow clock, a descheduled thread): no file is started after the cut.
        units.deadline = Some(Instant::now() + Duration::from_secs(3600));
        assert!(units.next_unit().is_none());
        assert_eq!(units.started(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The step's time cut at the store level: with no time at all each step indexes the first segment only; the turns after
    /// it index the rest, and every event ends up in the index once.
    #[test]
    fn a_step_out_of_time_marks_only_its_first_segment_and_later_turns_index_the_rest() {
        use debrief_core::ports::Ingest;
        let dir = std::env::temp_dir().join(format!("debrief-fts-cut-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let logs = dir.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let store = crate::DuckStore::create(
            &dir.join("store"),
            debrief_core::Profile::default_profile(),
            debrief_core::PluginRegistry::new(),
        )
        .unwrap();
        // The indexing thread is stopped and joined first: the test takes the turns.
        let svc = store.inner.fts.as_ref().unwrap();
        svc.shared.stop.store(true, Ordering::SeqCst);
        svc.wake();
        // A DuckStore keeps its FtsService until dropped; wait for the thread by its handle.
        let handle = svc.handle.as_ref().unwrap();
        while !handle.is_finished() {
            std::thread::yield_now();
        }
        for (i, name) in ["a", "b", "c", "d"].iter().enumerate() {
            let file = logs.join(format!("{name}.log"));
            std::fs::write(
                &file,
                format!("2026-10-14 10:00:0{i} INFO alpha {name}1\n2026-10-14 10:01:0{i} INFO alpha {name}2\n"),
            )
            .unwrap();
            store.open(&[file]).unwrap().wait().unwrap();
        }
        let conn = store.inner.conn().unwrap();
        let count = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
        let segments = count("SELECT count(*) FROM segments");
        assert!(segments >= 4, "one segment per import: {segments}");
        let s = &svc.shared;
        s.stop.store(false, Ordering::SeqCst);
        // No time at all for a step.
        s.step_time.store(0, Ordering::Relaxed);
        let write_lock = Mutex::new(());
        let seg_dir = dir.join("store").join(SEGMENTS_DIR);
        s.gate.news();
        assert_eq!(step(s, &seg_dir, &conn, &write_lock, svc.epoch).unwrap(), Turn::Worked);
        assert_eq!(count("SELECT count(*) FROM segments WHERE indexed"), 1, "the first segment only");
        let mut turns = 1;
        loop {
            s.gate.news();
            match step(s, &seg_dir, &conn, &write_lock, svc.epoch).unwrap() {
                Turn::Worked => turns += 1,
                _ => break,
            }
            assert!(turns <= segments + 1, "each turn indexes a segment");
        }
        assert_eq!(count("SELECT count(*) FROM segments WHERE NOT indexed"), 0);
        let all: Vec<u64> = {
            let mut stmt = conn.prepare("SELECT id FROM events ORDER BY id").unwrap();
            stmt.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
        };
        assert_eq!(s.fts.docs(), all.len() as u64, "every event once");
        let mut found = s
            .fts
            .all_ids(&debrief_core::words::WordQuery::parse("alpha"), 10_000)
            .unwrap()
            .unwrap();
        found.sort_unstable();
        assert_eq!(found, all);
        drop(conn);
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An input cut short by the process exiting is not reported as a finished step.
    #[test]
    fn an_input_cut_short_by_the_exit_says_so() {
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        fn exits_after_a_few_slices() -> bool {
            CALLS.fetch_add(1, Ordering::SeqCst) >= 5
        }
        let dir = std::env::temp_dir().join(format!("debrief-fts-exit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let seg_dir = dir.join("segments");
        std::fs::create_dir_all(&seg_dir).unwrap();
        let (files, rows) = write_long_segments(&seg_dir, 2, 3, 600, 100);
        let (fts, _) = Fts::open_or_create(
            &dir.join(INDEX_DIR),
            1,
            WriterBudget {
                heap: 32 << 20,
                threads: 1,
            },
        )
        .unwrap();
        let pool = read_pool(2);
        let cancel = AtomicBool::new(false);
        let ready: HashSet<u32> = [1].into_iter().collect();
        let mut w = fts.writer().unwrap();
        let fed = fts
            .index_files(
                &mut w,
                Feed {
                    seg_dir: &seg_dir,
                    files: &files,
                    ready: &ready,
                    deadline: None,
                    memory: FEED_MEMORY,
                    down: exits_after_a_few_slices,
                },
                &stages(&pool, &cancel),
                feed_window(&pool),
                |_| {},
            )
            .unwrap();
        assert_eq!(fed.stop, StopCause::Shutdown);
        assert!(fed.docs < rows as u64, "the input was cut: {} of {rows}", fed.docs);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
