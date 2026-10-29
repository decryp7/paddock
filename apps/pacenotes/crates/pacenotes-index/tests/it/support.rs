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
    pub cancel_on: Option<(&'static str, CancelToken)>,
}

impl SymbolSource for ParseSource {
    fn extract(&self, path: &RepoPath, bytes: &[u8], cancel: &CancelToken) -> Result<Option<FileFacts>, SourceError> {
        self.extracted.lock().unwrap().push(path.to_string());
        if self.fail_on == Some(path.as_str()) {
            return Err(SourceError::Failed("the worker crashed on this file".into()));
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
