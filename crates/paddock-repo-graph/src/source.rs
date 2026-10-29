use crate::GraphError;
use std::collections::BTreeMap;

/// The port to the repository's files: the text of a tracked file by its repository-relative path.
pub trait FileSource {
    /// The file's text.
    fn read_to_string(&self, path: &str) -> Result<String, GraphError>;
}

/// Files in memory, for tests and for callers that already hold the text.
#[derive(Debug, Default, Clone)]
pub struct MemoryFiles(BTreeMap<String, String>);

impl MemoryFiles {
    /// An empty set.
    pub fn new() -> MemoryFiles {
        MemoryFiles::default()
    }

    /// Adds or replaces a file.
    pub fn with(mut self, path: &str, text: &str) -> MemoryFiles {
        self.0.insert(path.to_string(), text.to_string());
        self
    }

    /// Every path, sorted (what `git ls-files` would list).
    pub fn paths(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }
}

impl FileSource for MemoryFiles {
    fn read_to_string(&self, path: &str) -> Result<String, GraphError> {
        self.0.get(path).cloned().ok_or_else(|| GraphError::Read {
            path: path.to_string(),
            reason: "not found".to_string(),
        })
    }
}
