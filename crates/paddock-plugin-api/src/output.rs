//! Output files of a tool (ADR 0017 §7): a tool writes a capture through [`OutputWriter`] instead of at a path. The
//! bytes cross the connection ([`crate::protocol::HostCall::Output`]) and the *app* writes them into the folder it chose,
//! so a plugin without a write grant (or an elevated one) never writes at a path.
//!
//! An [`OutputWriter`] dropped without [`OutputWriter::finish`] is discarded: the app removes what it had of it when the
//! call ends.

use crate::protocol::{OUTPUT_CHUNK, SavedOutput, check_output_name};
use crate::tools::{Host, ToolError};
use std::io::{self, Write};
use std::sync::Arc;

/// Writes one output file through the app.
pub struct OutputWriter {
    host: Arc<dyn Host>,
    name: String,
    buf: Vec<u8>,
    sent: u64,
}

impl OutputWriter {
    /// Starts the output file `name` (letters, digits, `-`, `_`, `.`).
    pub fn create(host: Arc<dyn Host>, name: &str) -> Result<OutputWriter, ToolError> {
        check_output_name("file name", name).map_err(ToolError::Failed)?;
        Ok(OutputWriter {
            host,
            name: name.into(),
            buf: Vec::new(),
            sent: 0,
        })
    }

    /// Bytes handed to the app so far.
    pub fn sent(&self) -> u64 {
        self.sent
    }

    fn send(&mut self, last: bool) -> Result<Option<SavedOutput>, ToolError> {
        let bytes = std::mem::take(&mut self.buf);
        self.sent += bytes.len() as u64;
        self.host.output(&self.name, &bytes, last)
    }

    /// Ends the file and returns where the app saved it.
    pub fn finish(mut self) -> Result<SavedOutput, ToolError> {
        let saved = self.send(true)?;
        saved.ok_or_else(|| ToolError::Failed("the app did not say where it saved the output".into()))
    }
}

impl Write for OutputWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        while self.buf.len() >= OUTPUT_CHUNK {
            let rest = self.buf.split_off(OUTPUT_CHUNK);
            self.send(false).map_err(io::Error::other)?;
            self.buf = rest;
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{NoHost, ToolTask};
    use std::sync::Mutex;

    #[derive(Default)]
    struct Rec(Mutex<Vec<(String, usize, bool)>>);
    impl Host for Rec {
        fn read_file(&self, _: u64, _: u64, _: u32) -> Result<Vec<u8>, ToolError> {
            Ok(Vec::new())
        }
        fn report(&self, _: &str, _: Option<f32>, _: &str) {}
        fn task_started(&self, _: &str, _: &ToolTask) {}
        fn task_finished(&self, _: &str) {}
        fn output(&self, name: &str, bytes: &[u8], last: bool) -> Result<Option<SavedOutput>, ToolError> {
            self.0.lock().unwrap().push((name.into(), bytes.len(), last));
            Ok(last.then(|| SavedOutput { name: name.into(), bytes: 0 }))
        }
        fn elevated(&self, _: &str, _: &str, _: serde_json::Value, _: std::time::Duration) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::Value::Null)
        }
    }

    #[test]
    fn output_is_sent_in_chunks_and_ended_by_finish() {
        let host = Arc::new(Rec::default());
        let mut w = OutputWriter::create(host.clone(), "capture.bin").unwrap();
        w.write_all(&vec![1u8; OUTPUT_CHUNK * 2 + 10]).unwrap();
        w.flush().unwrap();
        assert_eq!(w.sent(), OUTPUT_CHUNK as u64 * 2);
        let saved = w.finish().unwrap();
        assert_eq!(saved.name, "capture.bin");
        let seen = host.0.lock().unwrap().clone();
        assert_eq!(
            seen,
            [
                ("capture.bin".to_string(), OUTPUT_CHUNK, false),
                ("capture.bin".to_string(), OUTPUT_CHUNK, false),
                ("capture.bin".to_string(), 10, true)
            ]
        );
    }

    #[test]
    fn bad_names_failing_hosts_and_missing_answers_are_errors() {
        assert!(OutputWriter::create(Arc::new(NoHost), "../x").is_err());
        assert!(OutputWriter::create(Arc::new(NoHost), "CON").is_err());
        let mut w = OutputWriter::create(Arc::new(NoHost), "a.bin").unwrap();
        assert!(w.write_all(&vec![0u8; OUTPUT_CHUNK]).is_err());
        let ended = OutputWriter::create(Arc::new(NoHost), "a.bin").unwrap();
        assert!(ended.finish().is_err());
        // A host that never says where it saved the file.
        struct Silent;
        impl Host for Silent {
            fn read_file(&self, _: u64, _: u64, _: u32) -> Result<Vec<u8>, ToolError> {
                Ok(Vec::new())
            }
            fn report(&self, _: &str, _: Option<f32>, _: &str) {}
            fn task_started(&self, _: &str, _: &ToolTask) {}
            fn task_finished(&self, _: &str) {}
            fn output(&self, _: &str, _: &[u8], _: bool) -> Result<Option<SavedOutput>, ToolError> {
                Ok(None)
            }
            fn elevated(&self, _: &str, _: &str, _: serde_json::Value, _: std::time::Duration) -> Result<serde_json::Value, ToolError> {
                Ok(serde_json::Value::Null)
            }
        }
        let w = OutputWriter::create(Arc::new(Silent), "a.bin").unwrap();
        assert!(w.finish().unwrap_err().to_string().contains("did not say"));
    }
}
