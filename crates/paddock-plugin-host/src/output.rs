//! Output files plugins write through the app ([`paddock_plugin_api::output`]): the app writes the pieces it receives into a
//! folder it chose, `<root>/<plugin id>/<name>`, as `.<name>.part` and renames it into place only at the end, so a plugin
//! never writes at a path and a file that was not finished leaves nothing at its name.

use paddock_plugin_api::protocol::{
    MAX_OUTPUT_BYTES, MAX_OUTPUTS, OUTPUT_CHUNK, SavedOutput, check_output_name,
};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

struct Part {
    file: File,
    part: PathBuf,
    path: PathBuf,
    written: u64,
}

impl Drop for Part {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.part);
    }
}

/// Receives the output files of one plugin. Cheap to clone; clones share the open files.
#[derive(Clone)]
pub struct OutputSink {
    dir: PathBuf,
    max_bytes: u64,
    open: Arc<Mutex<HashMap<String, Part>>>,
}

impl OutputSink {
    /// A sink writing into `dir` (created on first use), each file at most [`MAX_OUTPUT_BYTES`].
    pub fn new(dir: impl Into<PathBuf>) -> OutputSink {
        OutputSink {
            dir: dir.into(),
            max_bytes: MAX_OUTPUT_BYTES,
            open: Arc::default(),
        }
    }

    /// A lower size limit per file.
    pub fn with_limit(mut self, max_bytes: u64) -> OutputSink {
        self.max_bytes = max_bytes.min(MAX_OUTPUT_BYTES);
        self
    }

    /// The folder files land in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// One piece of the file `name`; `last` finishes it and returns where it was saved. A failure discards the file.
    pub fn piece(
        &self,
        name: &str,
        bytes: &[u8],
        last: bool,
    ) -> Result<Option<SavedOutput>, String> {
        let result = self.piece_inner(name, bytes, last);
        if result.is_err() {
            lock(&self.open).remove(name);
        }
        result
    }

    fn piece_inner(
        &self,
        name: &str,
        bytes: &[u8],
        last: bool,
    ) -> Result<Option<SavedOutput>, String> {
        check_output_name("file name", name)?;
        if bytes.len() > OUTPUT_CHUNK {
            return Err(format!(
                "a piece of {} bytes is larger than {OUTPUT_CHUNK}",
                bytes.len()
            ));
        }
        let mut open = lock(&self.open);
        if !open.contains_key(name) {
            if open.len() >= MAX_OUTPUTS {
                return Err(format!("{MAX_OUTPUTS} output files are already open"));
            }
            open.insert(name.to_string(), self.start(name)?);
        }
        let part = open.get_mut(name).ok_or("the output file is gone")?;
        if part.written + bytes.len() as u64 > self.max_bytes {
            return Err(format!(
                "the output exceeds its limit of {} bytes and was cut off",
                self.max_bytes
            ));
        }
        part.file
            .write_all(bytes)
            .map_err(|e| format!("cannot write the output file: {e}"))?;
        part.written += bytes.len() as u64;
        if !last {
            return Ok(None);
        }
        let done = open.remove(name).ok_or("the output file is gone")?;
        done.file
            .sync_all()
            .map_err(|e| format!("cannot write the output file to disk: {e}"))?;
        if done.path.symlink_metadata().is_ok() {
            return Err(format!(
                "{} already exists; it is not replaced",
                done.path.display()
            ));
        }
        std::fs::rename(&done.part, &done.path)
            .map_err(|e| format!("cannot save {}: {e}", done.path.display()))?;
        Ok(Some(SavedOutput {
            name: name.into(),
            bytes: done.written,
        }))
    }

    fn start(&self, name: &str) -> Result<Part, String> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("cannot create {}: {e}", self.dir.display()))?;
        let dir = paddock_env::winpath::real_verbatim(&self.dir)
            .map_err(|e| format!("cannot use {}: {e}", self.dir.display()))?;
        let path = dir.join(name);
        if path.symlink_metadata().is_ok() {
            return Err(format!(
                "{} already exists; it is not replaced",
                path.display()
            ));
        }
        let part = dir.join(format!(".{name}.part"));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)
            .map_err(|e| format!("cannot create {}: {e}", part.display()))?;
        Ok(Part {
            file,
            part,
            path,
            written: 0,
        })
    }

    /// Discards every file that was not finished (the call or the process ended).
    pub fn discard_open(&self) {
        lock(&self.open).clear();
    }

    /// How many files are being written.
    pub fn open_count(&self) -> usize {
        lock(&self.open).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_are_written_to_a_part_file_and_renamed_at_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let sink = OutputSink::new(dir.path().join("plugin"));
        assert_eq!(sink.dir(), dir.path().join("plugin"));
        assert_eq!(sink.piece("cap.bin", b"abc", false).unwrap(), None);
        assert_eq!(sink.open_count(), 1);
        assert!(
            dir.path().join("plugin/.cap.bin.part").exists()
                && !dir.path().join("plugin/cap.bin").exists()
        );
        let saved = sink.piece("cap.bin", b"def", true).unwrap().unwrap();
        assert_eq!((saved.name.as_str(), saved.bytes), ("cap.bin", 6));
        assert_eq!(
            std::fs::read(dir.path().join("plugin/cap.bin")).unwrap(),
            b"abcdef"
        );
        assert!(!dir.path().join("plugin/.cap.bin.part").exists());
        assert_eq!(sink.open_count(), 0);
        // It is never replaced.
        assert!(
            sink.piece("cap.bin", b"x", true)
                .unwrap_err()
                .contains("already exists")
        );
        assert_eq!(sink.open_count(), 0);
    }

    #[test]
    fn a_file_that_appears_while_one_is_written_is_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let sink = OutputSink::new(dir.path());
        sink.piece("cap.bin", b"abc", false).unwrap();
        std::fs::write(dir.path().join("cap.bin"), "somebody else").unwrap();
        let e = sink.piece("cap.bin", b"def", true).unwrap_err();
        assert!(e.contains("already exists; it is not replaced"), "{e}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("cap.bin")).unwrap(),
            "somebody else"
        );
        assert!(!dir.path().join(".cap.bin.part").exists());
    }

    #[test]
    fn bad_names_sizes_counts_and_limits_discard_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let sink = OutputSink::new(dir.path()).with_limit(10);
        assert!(
            sink.piece("../x", b"", true)
                .unwrap_err()
                .contains("invalid output file name")
        );
        assert!(
            sink.piece("a.bin", &vec![0; OUTPUT_CHUNK + 1], false)
                .unwrap_err()
                .contains("larger than")
        );
        sink.piece("b.bin", b"123456", false).unwrap();
        let over = sink.piece("b.bin", b"123456", false).unwrap_err();
        assert!(over.contains("exceeds its limit of 10"));
        // A failure discards what there was: no part file is left.
        assert_eq!(sink.open_count(), 0);
        assert!(!dir.path().join(".b.bin.part").exists());
        // The limit never exceeds the platform's.
        assert_eq!(
            OutputSink::new(dir.path()).with_limit(u64::MAX).max_bytes,
            MAX_OUTPUT_BYTES
        );
        // Only so many files at once.
        for i in 0..MAX_OUTPUTS {
            sink.piece(&format!("f{i}.bin"), b"x", false).unwrap();
        }
        assert!(
            sink.piece("one-more.bin", b"x", false)
                .unwrap_err()
                .contains("already open")
        );
        sink.discard_open();
        assert_eq!(sink.open_count(), 0);
        assert!(!dir.path().join(".f0.bin.part").exists());
    }

    #[test]
    fn a_folder_that_cannot_be_made_or_an_existing_name_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-folder");
        std::fs::write(&file, "x").unwrap();
        let sink = OutputSink::new(file.join("sub"));
        assert!(
            sink.piece("a.bin", b"x", false)
                .unwrap_err()
                .contains("cannot create")
        );
        let ok = OutputSink::new(dir.path().join("o"));
        ok.piece("a.bin", b"x", true).unwrap();
        // A part file from an earlier run blocks the name until it is removed: the error says which file.
        std::fs::write(dir.path().join("o/.c.bin.part"), "old").unwrap();
        assert!(
            ok.piece("c.bin", b"x", false)
                .unwrap_err()
                .contains(".c.bin.part")
        );
    }
}
