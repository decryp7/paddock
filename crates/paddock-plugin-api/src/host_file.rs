//! Reading a file the app opened for the plugin ("host files instead of paths", ADR 0017 §4): the plugin reads ranges
//! through the connection and never learns the path, so it needs no file-system right for it. The app answers
//! [`crate::protocol::HostCall::ReadFile`] only for files it opened for that plugin or paths inside the plugin's grants.

use crate::protocol::READ_CHUNK;
use crate::tools::{Host, ToolError};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A file the app opened for the plugin: quote `id` in reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostFileRef {
    /// Id the app gave the file.
    pub id: u64,
    /// Size of the file in bytes.
    pub len: u64,
}

/// A file read by ranges: a [`HostFile`] in a plugin process, a byte slice in tests.
pub trait RangeRead {
    /// Size of the file in bytes.
    fn len(&self) -> u64;
    /// Up to `len` bytes at `offset` (fewer at the end of the file).
    fn read_at(&self, offset: u64, len: u32) -> Result<Vec<u8>, ToolError>;
    /// Whether the file has no bytes.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl RangeRead for &[u8] {
    fn len(&self) -> u64 {
        <[u8]>::len(self) as u64
    }

    fn read_at(&self, offset: u64, len: u32) -> Result<Vec<u8>, ToolError> {
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(<[u8]>::len(self));
        let end = start.saturating_add(len as usize).min(<[u8]>::len(self));
        Ok(self[start..end].to_vec())
    }
}

/// A host file, read through the app's services.
pub struct HostFile {
    host: Arc<dyn Host>,
    file: HostFileRef,
}

impl HostFile {
    /// Wraps the host file `file`.
    pub fn new(host: Arc<dyn Host>, file: HostFileRef) -> HostFile {
        HostFile { host, file }
    }

    /// The whole file, refused when it is larger than `cap` bytes (the plugin's limit). `cancelled` is asked between
    /// chunks.
    pub fn read_all(&self, cap: u64, cancelled: &dyn Fn() -> bool) -> Result<Vec<u8>, ToolError> {
        if self.file.len > cap {
            return Err(ToolError::Failed(format!(
                "the file is {} bytes; this plugin reads at most {cap}",
                self.file.len
            )));
        }
        let mut out = Vec::with_capacity(usize::try_from(self.file.len).unwrap_or(0));
        while (out.len() as u64) < self.file.len {
            if cancelled() {
                return Err(ToolError::Cancelled);
            }
            let want = u32::try_from((self.file.len - out.len() as u64).min(u64::from(READ_CHUNK)))
                .unwrap_or(READ_CHUNK);
            let chunk = self.read_at(out.len() as u64, want)?;
            if chunk.is_empty() {
                return Err(ToolError::Failed(format!(
                    "the file ended early ({} of {} bytes)",
                    out.len(),
                    self.file.len
                )));
            }
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }

    /// The whole file as UTF-8 text.
    pub fn read_text(&self, cap: u64, cancelled: &dyn Fn() -> bool) -> Result<String, ToolError> {
        String::from_utf8(self.read_all(cap, cancelled)?)
            .map_err(|_| ToolError::Failed("the file is not UTF-8 text".into()))
    }
}

impl RangeRead for HostFile {
    fn len(&self) -> u64 {
        self.file.len
    }

    fn read_at(&self, offset: u64, len: u32) -> Result<Vec<u8>, ToolError> {
        let len = len.min(READ_CHUNK);
        let bytes = self.host.read_file(self.file.id, offset, len)?;
        if bytes.len() > len as usize {
            return Err(ToolError::Failed(
                "the app sent more than was asked for".into(),
            ));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Memory;
    use crate::tools::NoHost;

    fn file(bytes: &[u8], liar: bool) -> HostFile {
        let host = Arc::new(Memory {
            content: bytes.to_vec(),
            overlong: liar,
            ..Memory::default()
        });
        HostFile::new(
            host,
            HostFileRef {
                id: 1,
                len: bytes.len() as u64,
            },
        )
    }

    #[test]
    fn a_whole_file_is_read_in_chunks_and_decoded() {
        let data: Vec<u8> = (0..(READ_CHUNK as usize * 2 + 5))
            .map(|i| (i % 251) as u8)
            .collect();
        let f = file(&data, false);
        assert_eq!(f.len(), data.len() as u64);
        assert!(!f.is_empty());
        assert_eq!(f.read_all(u64::MAX, &|| false).unwrap(), data);
        assert_eq!(
            file("héllo".as_bytes(), false)
                .read_text(100, &|| false)
                .unwrap(),
            "héllo"
        );
        let bad = file(&[0xff, 0xfe], false)
            .read_text(100, &|| false)
            .unwrap_err();
        assert!(bad.to_string().contains("not UTF-8"));
    }

    #[test]
    fn caps_cancellation_early_ends_and_overlong_answers_are_refused() {
        let f = file(b"abcdef", false);
        assert!(
            f.read_all(3, &|| false)
                .unwrap_err()
                .to_string()
                .contains("reads at most 3")
        );
        assert_eq!(f.read_all(10, &|| true), Err(ToolError::Cancelled));
        assert!(
            file(b"abc", true)
                .read_at(0, 3)
                .unwrap_err()
                .to_string()
                .contains("more than was asked")
        );
        // The file claims more bytes than the host has: the host answers nothing at the end.
        let host = Memory::of(b"ab");
        let short = HostFile::new(host.clone(), HostFileRef { id: 1, len: 10 });
        assert!(
            short
                .read_all(100, &|| false)
                .unwrap_err()
                .to_string()
                .contains("ended early (2 of 10")
        );
        assert_eq!(host.log()[0], "read 1 0 10");
        let none = HostFile::new(Arc::new(NoHost), HostFileRef { id: 1, len: 1 });
        assert!(none.read_at(0, 1).is_err());
        assert!(file(b"", false).is_empty());
    }

    #[test]
    fn a_byte_slice_is_a_range_reader() {
        let bytes: &[u8] = b"0123456789";
        assert_eq!(RangeRead::len(&bytes), 10);
        assert_eq!(bytes.read_at(8, 5).unwrap(), b"89");
        assert_eq!(bytes.read_at(99, 5).unwrap(), b"");
        assert!(!RangeRead::is_empty(&bytes));
    }
}
