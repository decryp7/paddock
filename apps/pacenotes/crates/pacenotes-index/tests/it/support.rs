//! Doubles: a file system that records what it reads, and a symbol source that runs the parser library in the test process
//! (the app never does; its source is the A3 worker).

use std::path::PathBuf;
use std::sync::Mutex;

use pacenotes_core::{Language, RepoPath};
use pacenotes_index::{
    CallFact, ChunkFact, DirEntry, DiskFs, FileFacts, FsError, ImportFact, SourceError, SymbolFact, SymbolSource, WorkspaceFs,
};
use pacenotes_parse::{Limits, ParseLanguage, parse};
use paddock_jobs::CancelToken;
use paddock_testkit::Scratch;

pub fn p(s: &str) -> RepoPath {
    RepoPath::parse(s).unwrap()
}

/// A file system that remembers every file it was asked to read, by real path.
pub struct RecordingFs {
    inner: DiskFs,
    pub reads: Mutex<Vec<PathBuf>>,
}

impl RecordingFs {
    pub fn new(root: &std::path::Path) -> RecordingFs {
        RecordingFs {
            inner: DiskFs::new(root).unwrap(),
            reads: Mutex::new(Vec::new()),
        }
    }

    pub fn reads(&self) -> Vec<PathBuf> {
        self.reads.lock().unwrap().clone()
    }
}

impl WorkspaceFs for RecordingFs {
    fn list(&self, dir: Option<&RepoPath>) -> Result<Vec<DirEntry>, FsError> {
        self.inner.list(dir)
    }

    fn read(&self, path: &RepoPath, max_bytes: u64) -> Result<Vec<u8>, FsError> {
        let mut at = self.inner.root().to_path_buf();
        at.extend(path.as_str().split('/'));
        self.reads.lock().unwrap().push(at);
        self.inner.read(path, max_bytes)
    }
}

/// Parses with the parser library and counts its calls; `fail_on` makes one file fail, `cancel_on` cancels the token when a
/// file is reached.
#[derive(Default)]
pub struct ParseSource {
    pub extracted: Mutex<Vec<String>>,
    pub fail_on: Option<&'static str>,
    pub crash_on: Option<&'static str>,
    pub cancel_on: Option<(&'static str, CancelToken)>,
}

impl SymbolSource for ParseSource {
    fn extract(&self, path: &RepoPath, bytes: &[u8], cancel: &CancelToken) -> Result<Option<FileFacts>, SourceError> {
        self.extracted.lock().unwrap().push(path.to_string());
        if self.fail_on == Some(path.as_str()) {
            return Err(SourceError::Failed("the worker crashed on this file".into()));
        }
        if self.crash_on == Some(path.as_str()) {
            return Err(SourceError::Crashed("the worker hung on this file and was ended".into()));
        }
        if let Some((name, token)) = &self.cancel_on
            && path.as_str() == *name
        {
            token.cancel();
        }
        if cancel.is_cancelled() {
            return Err(SourceError::Cancelled);
        }
        let Some(language) = ParseLanguage::from_path(path) else {
            return Ok(None);
        };
        let outline = parse(language, bytes, &Limits::default(), cancel).map_err(|e| SourceError::Failed(e.to_string()))?;
        Ok(Some(FileFacts {
            language: Language::parse(language.id()).unwrap(),
            symbols: outline
                .symbols
                .iter()
                .map(|s| SymbolFact {
                    kind: s.kind,
                    name: s.name.clone(),
                    qualified_name: s.qualified_name.clone(),
                    span: s.span,
                    depth: s.depth,
                })
                .collect(),
            imports: outline
                .imports
                .iter()
                .map(|i| ImportFact {
                    target: i.target.clone(),
                    span: i.span,
                })
                .collect(),
            calls: outline
                .calls
                .iter()
                .map(|c| CallFact {
                    caller: c.caller.clone(),
                    callee: c.callee.clone(),
                    span: c.span,
                })
                .collect(),
            chunks: outline
                .chunks
                .iter()
                .map(|c| ChunkFact {
                    span: c.span,
                    symbol: c.symbol.clone(),
                    ordinal: c.ordinal,
                })
                .collect(),
            partial: outline.partial,
            has_errors: outline.has_errors,
        }))
    }
}

pub fn workspace(files: &[(&str, &str)]) -> Scratch {
    let s = Scratch::new();
    for (name, content) in files {
        s.write(&format!("ws/{name}"), content).unwrap();
    }
    s
}

/// A file system that cancels the run's token after a given read, or fails reads of a path from its nth read on (0 is the first), to
/// put a Cancel or a read error at an exact point of a pass.
pub struct StopFs {
    inner: DiskFs,
    pub cancel_after_read_of: Option<(&'static str, CancelToken)>,
    pub fail_from: Option<(&'static str, usize)>,
    seen: Mutex<std::collections::BTreeMap<String, usize>>,
}

impl StopFs {
    pub fn new(root: &std::path::Path) -> StopFs {
        StopFs {
            inner: DiskFs::new(root).unwrap(),
            cancel_after_read_of: None,
            fail_from: None,
            seen: Mutex::new(std::collections::BTreeMap::new()),
        }
    }
}

impl WorkspaceFs for StopFs {
    fn list(&self, dir: Option<&RepoPath>) -> Result<Vec<DirEntry>, FsError> {
        self.inner.list(dir)
    }

    fn read(&self, path: &RepoPath, max_bytes: u64) -> Result<Vec<u8>, FsError> {
        let nth = {
            let mut seen = self.seen.lock().unwrap();
            let n = seen.entry(path.to_string()).or_insert(0);
            *n += 1;
            *n - 1
        };
        if let Some((name, from)) = self.fail_from
            && path.as_str() == name
            && nth >= from
        {
            return Err(FsError::Io("the file is locked by another program".into()));
        }
        let bytes = self.inner.read(path, max_bytes);
        if let Some((name, token)) = &self.cancel_after_read_of
            && path.as_str() == *name
        {
            token.cancel();
        }
        bytes
    }
}
