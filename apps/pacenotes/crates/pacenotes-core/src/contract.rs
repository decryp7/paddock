//! The contract test of each port and an in-memory fake that passes it (ADR 0002).
//!
//! An adapter's own tests call the matching function with an instance prepared as the function documents; the core's
//! fakes do the same, so a fake and a real adapter are held to one definition of the port.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_panics_doc)]

use std::collections::BTreeMap;
use std::sync::Mutex;

use paddock_jobs::CancelToken;

use crate::ids::{ChunkId, FileId};
use crate::path::RepoPath;
use crate::ports::{
    ChangeKind, ChangeSet, Completion, EmbedError, Embedder, FileChange, History, HistoryError, LanguageServer, Llm, LlmError, LspError,
    LspReply, LspRequest, MAX_EMBED_TEXT_BYTES, MAX_EMBED_TEXTS, ModelId, Store, StoreError, SymbolPage, SymbolRecord, TokenBudget,
    VectorError, VectorHit, VectorIndex,
};
use crate::rev::{Generation, Rev};

fn cancelled() -> CancelToken {
    let t = CancelToken::new();
    t.cancel();
    t
}

// ---- History -------------------------------------------------------------------------------------------------------

/// An in-memory [`History`]: two revisions with the files given to [`MemoryHistory::new`].
pub struct MemoryHistory {
    head: Rev,
    files: BTreeMap<(Rev, RepoPath), Vec<u8>>,
    known: Vec<Rev>,
}

impl MemoryHistory {
    /// A history whose revisions are `revs`; `files` maps (revision index, path) to content. `head` is `revs.last()`.
    pub fn new(revs: Vec<Rev>, files: Vec<(usize, RepoPath, Vec<u8>)>) -> MemoryHistory {
        let head = revs.last().cloned().expect("at least one revision");
        let files = files.into_iter().map(|(i, p, c)| ((revs[i].clone(), p), c)).collect();
        MemoryHistory { head, files, known: revs }
    }

    fn known(&self, rev: &Rev) -> Result<(), HistoryError> {
        if self.known.contains(rev) {
            Ok(())
        } else {
            Err(HistoryError::UnknownRev(rev.to_string()))
        }
    }
}

impl History for MemoryHistory {
    fn head(&self, cancel: &CancelToken) -> Result<Rev, HistoryError> {
        if cancel.is_cancelled() {
            return Err(HistoryError::Cancelled);
        }
        Ok(self.head.clone())
    }

    fn read_at(&self, rev: &Rev, path: &RepoPath, max_bytes: u64, cancel: &CancelToken) -> Result<Vec<u8>, HistoryError> {
        if cancel.is_cancelled() {
            return Err(HistoryError::Cancelled);
        }
        self.known(rev)?;
        let content = self
            .files
            .get(&(rev.clone(), path.clone()))
            .ok_or_else(|| HistoryError::NotFound(path.clone()))?;
        if content.len() as u64 > max_bytes {
            return Err(HistoryError::TooLarge {
                path: path.clone(),
                limit: max_bytes,
            });
        }
        Ok(content.clone())
    }

    fn changed_between(&self, from: &Rev, to: &Rev, limit: usize, cancel: &CancelToken) -> Result<ChangeSet, HistoryError> {
        if cancel.is_cancelled() {
            return Err(HistoryError::Cancelled);
        }
        self.known(from)?;
        self.known(to)?;
        let paths: std::collections::BTreeSet<&RepoPath> = self.files.keys().map(|(_, p)| p).collect();
        let mut changes = Vec::new();
        for p in paths {
            let before = self.files.get(&(from.clone(), p.clone()));
            let after = self.files.get(&(to.clone(), p.clone()));
            let kind = match (before, after) {
                (None, Some(_)) => ChangeKind::Added,
                (Some(_), None) => ChangeKind::Deleted,
                (Some(a), Some(b)) if a != b => ChangeKind::Modified,
                _ => continue,
            };
            changes.push(FileChange { path: p.clone(), kind });
        }
        let truncated = changes.len() > limit;
        changes.truncate(limit);
        Ok(ChangeSet {
            changes,
            truncated,
            skipped_names: 0,
        })
    }
}

/// What [`history_contract`] needs: `rev_b` is `head`; `present` exists at `rev_b` with that content (under 1 MiB);
/// `empty` is an empty file at `rev_a`; `missing` exists nowhere; and at least two files differ between the revisions.
pub struct HistoryFixture<'a> {
    /// The adapter under test.
    pub history: &'a dyn History,
    /// The older revision.
    pub rev_a: Rev,
    /// The newer revision (HEAD).
    pub rev_b: Rev,
    /// A path that exists, with this content.
    pub present: (RepoPath, Vec<u8>),
    /// A path that exists and is empty at `rev_a`.
    pub empty: RepoPath,
    /// A path that does not exist.
    pub missing: RepoPath,
}

/// Runs the contract of [`History`].
pub fn history_contract(fx: &HistoryFixture<'_>) {
    let live = CancelToken::new();
    let h = fx.history;
    assert_eq!(h.head(&live).unwrap(), fx.rev_b, "head is the newest revision");
    // not found is not empty
    assert_eq!(
        h.read_at(&fx.rev_a, &fx.missing, 1 << 20, &live),
        Err(HistoryError::NotFound(fx.missing.clone()))
    );
    assert_eq!(
        h.read_at(&fx.rev_a, &fx.empty, 1 << 20, &live).unwrap(),
        Vec::<u8>::new(),
        "an empty file is empty, not missing"
    );
    assert_eq!(
        h.read_at(&fx.rev_b, &fx.present.0, 1 << 20, &live).unwrap(),
        fx.present.1,
        "reads the content at that revision"
    );
    // size cap
    if !fx.present.1.is_empty() {
        let cap = fx.present.1.len() as u64 - 1;
        assert_eq!(
            h.read_at(&fx.rev_b, &fx.present.0, cap, &live),
            Err(HistoryError::TooLarge {
                path: fx.present.0.clone(),
                limit: cap
            })
        );
    }
    // sorted, capped, truncation reported
    let all = h.changed_between(&fx.rev_a, &fx.rev_b, usize::MAX, &live).unwrap();
    assert!(!all.truncated);
    assert!(all.changes.len() >= 2, "the fixture changes at least two files");
    assert!(all.changes.windows(2).all(|w| w[0].path < w[1].path), "changes are sorted by path");
    let one = h.changed_between(&fx.rev_a, &fx.rev_b, 1, &live).unwrap();
    assert_eq!((one.changes.len(), one.truncated), (1, true));
    assert_eq!(one.changes[0], all.changes[0]);
    // cancel before work
    let dead = cancelled();
    assert_eq!(h.head(&dead), Err(HistoryError::Cancelled));
    assert_eq!(h.read_at(&fx.rev_a, &fx.present.0, 1 << 20, &dead), Err(HistoryError::Cancelled));
    assert_eq!(h.changed_between(&fx.rev_a, &fx.rev_b, 10, &dead), Err(HistoryError::Cancelled));
}

// ---- Store ---------------------------------------------------------------------------------------------------------

type StoreState = (Generation, BTreeMap<RepoPath, FileId>, BTreeMap<FileId, Vec<SymbolRecord>>);

/// An in-memory read-side [`Store`].
#[derive(Default)]
pub struct MemoryStore {
    state: Mutex<StoreState>,
}

impl MemoryStore {
    /// Adds a file and its symbols, moving to the next generation.
    pub fn put(&self, path: RepoPath, file: FileId, mut symbols: Vec<SymbolRecord>) {
        symbols.sort_by_key(|s| s.span.start());
        let mut g = self.state.lock().unwrap();
        g.0 = g.0.next();
        g.1.insert(path, file);
        g.2.insert(file, symbols);
    }
}

impl Store for MemoryStore {
    fn generation(&self, cancel: &CancelToken) -> Result<Generation, StoreError> {
        if cancel.is_cancelled() {
            return Err(StoreError::Cancelled);
        }
        Ok(self.state.lock().unwrap().0)
    }

    fn file_id(&self, path: &RepoPath, cancel: &CancelToken) -> Result<Option<FileId>, StoreError> {
        if cancel.is_cancelled() {
            return Err(StoreError::Cancelled);
        }
        Ok(self.state.lock().unwrap().1.get(path).copied())
    }

    fn symbols_in_file(&self, file: FileId, limit: usize, cancel: &CancelToken) -> Result<SymbolPage, StoreError> {
        if cancel.is_cancelled() {
            return Err(StoreError::Cancelled);
        }
        let mut symbols = self.state.lock().unwrap().2.get(&file).cloned().unwrap_or_default();
        let truncated = symbols.len() > limit;
        symbols.truncate(limit);
        Ok(SymbolPage::new(symbols, truncated).expect("a test store holds few symbols"))
    }
}

/// The contract of [`Store`]: `known` is a file the store holds (with at least two symbols), `unknown` is not.
pub fn store_contract(store: &dyn Store, known: &RepoPath, unknown: &RepoPath) {
    let live = CancelToken::new();
    let g1 = store.generation(&live).unwrap();
    let file = store.file_id(known, &live).unwrap().expect("the known file has an id");
    assert_eq!(
        store.file_id(unknown, &live).unwrap(),
        None,
        "an unknown path has no id, and is not an error"
    );
    let page = store.symbols_in_file(file, 1000, &live).unwrap();
    assert!(!page.truncated() && page.symbols().len() >= 2);
    assert!(
        page.symbols().windows(2).all(|w| w[0].span.start() <= w[1].span.start()),
        "ordered by span start"
    );
    assert!(page.symbols().iter().all(|s| s.file == file));
    let cut = store.symbols_in_file(file, 1, &live).unwrap();
    assert_eq!(
        (cut.symbols().len(), cut.truncated()),
        (1, true),
        "the limit bounds the page and says it cut"
    );
    assert_eq!(cut.symbols()[0], page.symbols()[0]);
    let unknown_file = FileId::from_bytes([0xee; 16]);
    assert!(
        store.symbols_in_file(unknown_file, 10, &live).unwrap().symbols().is_empty(),
        "an unknown file has no symbols"
    );
    assert_eq!(store.generation(&live).unwrap(), g1, "reads do not move the generation");
    assert_eq!(
        store.symbols_in_file(file, 1000, &live).unwrap(),
        page,
        "within a generation the same question has the same answer"
    );
    let dead = cancelled();
    assert_eq!(store.generation(&dead), Err(StoreError::Cancelled));
    assert_eq!(store.file_id(known, &dead), Err(StoreError::Cancelled));
    assert_eq!(store.symbols_in_file(file, 10, &dead), Err(StoreError::Cancelled));
}

// ---- Embedder ------------------------------------------------------------------------------------------------------

/// A deterministic [`Embedder`]: a hashed bag of bytes, normalised.
pub struct HashEmbedder {
    model: ModelId,
    dim: usize,
}

impl HashEmbedder {
    /// An embedder of `dim` dimensions.
    pub fn new(model: &str, dim: usize) -> HashEmbedder {
        HashEmbedder {
            model: ModelId::parse(model).unwrap(),
            dim,
        }
    }
}

impl Embedder for HashEmbedder {
    fn model(&self) -> &ModelId {
        &self.model
    }

    fn dimension(&self) -> usize {
        self.dim
    }

    fn embed(&self, texts: &[&str], cancel: &CancelToken) -> Result<Vec<Vec<f32>>, EmbedError> {
        if cancel.is_cancelled() {
            return Err(EmbedError::Cancelled);
        }
        if texts.len() > MAX_EMBED_TEXTS || texts.iter().any(|t| t.len() > MAX_EMBED_TEXT_BYTES) {
            return Err(EmbedError::TooLarge);
        }
        Ok(texts
            .iter()
            .map(|t| {
                let mut v = vec![0f32; self.dim];
                for (i, b) in t.bytes().enumerate() {
                    v[(usize::from(b) * 31 + i) % self.dim] += 1.0;
                }
                let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                if norm > 0.0 {
                    v.iter_mut().for_each(|x| *x /= norm);
                }
                v
            })
            .collect())
    }
}

/// The contract of [`Embedder`].
pub fn embedder_contract(e: &dyn Embedder) {
    let live = CancelToken::new();
    assert!(e.dimension() > 0);
    assert!(!e.model().as_str().is_empty());
    let texts = ["fn parse() {}", "", "a much longer text with several words in it"];
    let out = e.embed(&texts, &live).unwrap();
    assert_eq!(out.len(), texts.len(), "one vector per text");
    assert!(
        out.iter().all(|v| v.len() == e.dimension()),
        "every vector has `dimension()` entries"
    );
    assert!(out.iter().flatten().all(|x| x.is_finite()));
    let again = e.embed(&texts[..1], &live).unwrap();
    assert_eq!(again[0], out[0], "the same text embeds the same way, and order does not matter");
    assert_eq!(e.embed(&[], &live).unwrap().len(), 0);
    assert_eq!(e.embed(&texts, &cancelled()), Err(EmbedError::Cancelled));
    // the bounds are refused before any work
    let many = vec!["x"; MAX_EMBED_TEXTS + 1];
    assert_eq!(e.embed(&many, &live), Err(EmbedError::TooLarge));
    let long = "x".repeat(MAX_EMBED_TEXT_BYTES + 1);
    assert_eq!(e.embed(&[long.as_str()], &live), Err(EmbedError::TooLarge));
    assert!(e.embed(&vec!["x"; MAX_EMBED_TEXTS], &live).is_ok(), "the bound itself is allowed");
}

// ---- VectorIndex ---------------------------------------------------------------------------------------------------

/// A brute-force in-memory [`VectorIndex`].
pub struct MemoryVectorIndex {
    model: ModelId,
    dim: usize,
    vectors: BTreeMap<ChunkId, Vec<f32>>,
}

impl MemoryVectorIndex {
    /// An empty index for `model` with `dim` dimensions.
    pub fn new(model: ModelId, dim: usize) -> MemoryVectorIndex {
        MemoryVectorIndex {
            model,
            dim,
            vectors: BTreeMap::new(),
        }
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}

impl VectorIndex for MemoryVectorIndex {
    fn model(&self) -> &ModelId {
        &self.model
    }

    fn dimension(&self) -> usize {
        self.dim
    }

    fn insert(&mut self, chunk: ChunkId, model: &ModelId, vector: &[f32]) -> Result<(), VectorError> {
        if model != &self.model {
            return Err(VectorError::ModelMismatch {
                index: self.model.as_str().to_owned(),
                given: model.as_str().to_owned(),
            });
        }
        if vector.len() != self.dim {
            return Err(VectorError::DimensionMismatch {
                expected: self.dim,
                got: vector.len(),
            });
        }
        if vector.iter().any(|x| !x.is_finite()) {
            return Err(VectorError::NotFinite);
        }
        self.vectors.insert(chunk, vector.to_vec());
        Ok(())
    }

    fn remove(&mut self, chunk: ChunkId) -> Result<bool, VectorError> {
        Ok(self.vectors.remove(&chunk).is_some())
    }

    fn nearest(&self, query: &[f32], k: usize, cancel: &CancelToken) -> Result<Vec<VectorHit>, VectorError> {
        if cancel.is_cancelled() {
            return Err(VectorError::Cancelled);
        }
        if query.len() != self.dim {
            return Err(VectorError::DimensionMismatch {
                expected: self.dim,
                got: query.len(),
            });
        }
        let mut hits: Vec<VectorHit> = self
            .vectors
            .iter()
            .map(|(id, v)| VectorHit {
                chunk: *id,
                score: cosine(query, v),
            })
            .collect();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.chunk.cmp(&b.chunk)));
        hits.truncate(k);
        Ok(hits)
    }
}

/// The contract of [`VectorIndex`]; `index` must be empty and have dimension 4.
pub fn vector_index_contract(index: &mut dyn VectorIndex, other_model: &ModelId) {
    assert_eq!(index.dimension(), 4);
    let model = index.model().clone();
    let id = |n: u8| ChunkId::from_bytes([n; 16]);
    // a vector of another model, of another length or not finite is refused, and nothing is stored
    assert!(matches!(
        index.insert(id(1), other_model, &[1.0, 0.0, 0.0, 0.0]),
        Err(VectorError::ModelMismatch { .. })
    ));
    assert!(matches!(
        index.insert(id(1), &model, &[1.0, 0.0]),
        Err(VectorError::DimensionMismatch { expected: 4, got: 2 })
    ));
    assert_eq!(index.insert(id(1), &model, &[f32::NAN, 0.0, 0.0, 0.0]), Err(VectorError::NotFinite));
    let live = CancelToken::new();
    assert!(
        index.nearest(&[1.0, 0.0, 0.0, 0.0], 5, &live).unwrap().is_empty(),
        "the refused inserts stored nothing"
    );
    // equal scores are ordered by chunk id: deterministic
    for n in [3u8, 1, 2] {
        index.insert(id(n), &model, &[1.0, 0.0, 0.0, 0.0]).unwrap();
    }
    index.insert(id(9), &model, &[0.0, 1.0, 0.0, 0.0]).unwrap();
    let hits = index.nearest(&[1.0, 0.0, 0.0, 0.0], 3, &live).unwrap();
    assert_eq!(hits.iter().map(|h| h.chunk).collect::<Vec<_>>(), vec![id(1), id(2), id(3)]);
    assert!(hits.iter().all(|h| (h.score - 1.0).abs() < 1e-6));
    assert_eq!(index.nearest(&[1.0, 0.0, 0.0, 0.0], 0, &live).unwrap().len(), 0);
    // replace, then remove: absent afterwards
    index.insert(id(1), &model, &[0.0, 0.0, 1.0, 0.0]).unwrap();
    assert!(index.remove(id(2)).unwrap());
    assert!(!index.remove(id(2)).unwrap(), "removing twice reports it was not there");
    let after = index.nearest(&[1.0, 0.0, 0.0, 0.0], 10, &live).unwrap();
    assert!(after.iter().all(|h| h.chunk != id(2)));
    assert_eq!(after[0].chunk, id(3), "id 1 was replaced by an orthogonal vector");
    assert_eq!(index.nearest(&[1.0, 0.0, 0.0, 0.0], 1, &cancelled()), Err(VectorError::Cancelled));
    assert!(matches!(
        index.nearest(&[1.0], 1, &live),
        Err(VectorError::DimensionMismatch { .. })
    ));
}

// ---- LanguageServer ------------------------------------------------------------------------------------------------

/// What a [`ScriptedServer`] does.
#[derive(Debug, Clone)]
pub enum Script {
    /// Answer with this reply.
    Reply(LspReply),
    /// Fail with this error.
    Fail(LspError),
}

/// A [`LanguageServer`] that answers from a script.
pub struct ScriptedServer(pub Script);

impl LanguageServer for ScriptedServer {
    fn request(&self, _request: &LspRequest, cancel: &CancelToken) -> Result<LspReply, LspError> {
        if cancel.is_cancelled() {
            return Err(LspError::Cancelled);
        }
        match &self.0 {
            Script::Reply(r) => Ok(r.clone()),
            Script::Fail(e) => Err(e.clone()),
        }
    }
}

/// The contract of [`LanguageServer`] for a server that is up: it answers a request, and a cancelled call is
/// `Cancelled`. [`language_server_failures`] covers the failure modes.
pub fn language_server_contract(server: &dyn LanguageServer, request: &LspRequest) {
    assert!(server.request(request, &CancelToken::new()).is_ok());
    assert_eq!(server.request(request, &cancelled()), Err(LspError::Cancelled));
}

/// A predicate over the error a failing server must return.
pub type ErrorCheck = fn(&LspError) -> bool;

/// The contract of a server that is down, quarantined, slow or sending garbage: each failure is a typed error, never a
/// panic, and tells the caller to fall back to tier 2.
pub fn language_server_failures(servers: [(&dyn LanguageServer, ErrorCheck); 3], request: &LspRequest) {
    for (server, expected) in servers {
        let err = server.request(request, &CancelToken::new()).expect_err("the server is failing");
        assert!(expected(&err), "unexpected error {err:?}");
    }
}

// ---- Llm -----------------------------------------------------------------------------------------------------------

/// An [`Llm`] that echoes the first words of the prompt, one token per word, or says it is unavailable.
pub struct EchoLlm {
    /// When set, every call fails as unavailable with this next step.
    pub unavailable: Option<String>,
}

impl Llm for EchoLlm {
    fn complete(&self, prompt: &str, budget: TokenBudget, cancel: &CancelToken) -> Result<Completion, LlmError> {
        if cancel.is_cancelled() {
            return Err(LlmError::Cancelled);
        }
        if let Some(step) = &self.unavailable {
            return Err(LlmError::Unavailable(step.clone()));
        }
        let words: Vec<&str> = prompt.split_whitespace().take(budget.get() as usize).collect();
        Ok(Completion {
            text: words.join(" "),
            tokens_used: words.len() as u32,
            model: ModelId::parse("test/echo").unwrap(),
        })
    }
}

/// The contract of [`Llm`] for a model that is up.
pub fn llm_contract(llm: &dyn Llm) {
    let live = CancelToken::new();
    let prompt = "one two three four five six";
    let three = TokenBudget::new(3).unwrap();
    let c = llm.complete(prompt, three, &live).unwrap();
    assert!(c.tokens_used <= 3, "the budget is a hard cap");
    assert!(!c.model.as_str().is_empty(), "the reply names the model");
    assert_eq!(llm.complete(prompt, three, &cancelled()), Err(LlmError::Cancelled));
}

/// The contract of [`Llm`] when no model is reachable: `Unavailable` with a next step, not a hang or a panic.
pub fn llm_unavailable_contract(llm: &dyn Llm) {
    match llm.complete("x", TokenBudget::new(10).unwrap(), &CancelToken::new()) {
        Err(LlmError::Unavailable(step)) => assert!(!step.is_empty(), "the error names the next step"),
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{SymbolId, SymbolKind};
    use crate::language::{KnownLanguage, Language};
    use crate::location::Span;
    use crate::rev::CommitHash;
    use crate::tier::Confidence;

    fn p(s: &str) -> RepoPath {
        RepoPath::parse(s).unwrap()
    }

    fn rev(n: char) -> Rev {
        Rev::Commit(CommitHash::parse(&n.to_string().repeat(40)).unwrap())
    }

    #[test]
    fn memory_history_passes_the_history_contract() {
        let (a, b) = (rev('a'), rev('b'));
        let files = vec![
            (0, p("src/keep.rs"), b"fn keep() {}\n".to_vec()),
            (1, p("src/keep.rs"), b"fn keep() {}\n".to_vec()),
            (0, p("src/empty.txt"), Vec::new()),
            (1, p("src/empty.txt"), Vec::new()),
            (0, p("src/changed.rs"), b"v1".to_vec()),
            (1, p("src/changed.rs"), b"v2".to_vec()),
            (1, p("src/added.rs"), b"new".to_vec()),
            (0, p("src/gone.rs"), b"old".to_vec()),
        ];
        let h = MemoryHistory::new(vec![a.clone(), b.clone()], files);
        history_contract(&HistoryFixture {
            history: &h,
            rev_a: a,
            rev_b: b.clone(),
            present: (p("src/changed.rs"), b"v2".to_vec()),
            empty: p("src/empty.txt"),
            missing: p("nope.rs"),
        });
        assert_eq!(
            h.read_at(&rev('c'), &p("src/keep.rs"), 10, &CancelToken::new()),
            Err(HistoryError::UnknownRev(rev('c').to_string()))
        );
    }

    #[test]
    fn memory_store_passes_the_store_contract() {
        let store = MemoryStore::default();
        let path = p("src/a.rs");
        let file = FileId::from_sequence(1);
        let lang = Language::Known(KnownLanguage::Rust);
        let sym = |name: &str, line: u32| SymbolRecord {
            id: SymbolId::derive(file, &lang, SymbolKind::Function, name, 0),
            file,
            kind: SymbolKind::Function,
            qualified_name: name.to_owned(),
            span: Span::lines(line, line).unwrap(),
            confidence: Confidence::Resolved,
        };
        store.put(path.clone(), file, vec![sym("b", 9), sym("a", 1)]);
        store_contract(&store, &path, &p("src/other.rs"));
        assert_eq!(store.generation(&CancelToken::new()).unwrap(), Generation(1));
    }

    #[test]
    fn hash_embedder_passes_the_embedder_contract() {
        embedder_contract(&HashEmbedder::new("test/hash-8", 8));
    }

    #[test]
    fn memory_vector_index_passes_the_vector_index_contract() {
        let mut idx = MemoryVectorIndex::new(ModelId::parse("test/hash-4").unwrap(), 4);
        vector_index_contract(&mut idx, &ModelId::parse("other/model").unwrap());
    }

    #[test]
    fn scripted_servers_pass_the_language_server_contracts() {
        let req = LspRequest {
            feature: crate::pack::LspFeature::Definition,
            path: p("a.rs"),
            position: None,
        };
        language_server_contract(&ScriptedServer(Script::Reply(LspReply::default())), &req);
        let down = ScriptedServer(Script::Fail(LspError::Unavailable("quarantined after 3 crashes".into())));
        let bad = ScriptedServer(Script::Fail(LspError::BadReply("reply larger than 8 MiB".into())));
        let slow = ScriptedServer(Script::Fail(LspError::TimedOut));
        language_server_failures(
            [
                (&down, |e| matches!(e, LspError::Unavailable(_))),
                (&bad, |e| matches!(e, LspError::BadReply(_))),
                (&slow, |e| matches!(e, LspError::TimedOut)),
            ],
            &req,
        );
    }

    #[test]
    fn echo_llm_passes_the_llm_contracts() {
        llm_contract(&EchoLlm { unavailable: None });
        llm_unavailable_contract(&EchoLlm {
            unavailable: Some("Set up Pitwall in Settings > Local AI".into()),
        });
    }
}
