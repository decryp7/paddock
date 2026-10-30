//! Reading parts of a zip without trusting it.

use std::io::{Cursor, Read};

use zip::ZipArchive;

use super::{ImportLimits, ReqError, malformed};

/// An opened archive, with a running total of what has been decompressed.
pub(super) struct Archive<'b> {
    zip: ZipArchive<Cursor<&'b [u8]>>,
    total: u64,
    format: &'static str,
}

impl<'b> Archive<'b> {
    pub(super) fn open(bytes: &'b [u8], format: &'static str, limits: &ImportLimits) -> Result<Archive<'b>, ReqError> {
        let zip = ZipArchive::new(Cursor::new(bytes)).map_err(|e| malformed(format, format!("not a zip package ({e})")))?;
        if zip.len() > limits.max_zip_entries {
            return Err(ReqError::Limit {
                what: "zip entries",
                got: zip.len() as u64,
                max: limits.max_zip_entries as u64,
            });
        }
        Ok(Archive { zip, total: 0, format })
    }

    /// The text of a part, or `None` when it is not in the archive. The sizes the archive declares are not believed: the part is
    /// read through a limit, and the ratio is checked against what was really read.
    pub(super) fn text(&mut self, name: &str, limits: &ImportLimits) -> Result<Option<String>, ReqError> {
        let mut entry = match self.zip.by_name(name) {
            Ok(e) => e,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(malformed(self.format, format!("the part {name:?} cannot be opened ({e})"))),
        };
        let stored = entry.compressed_size().max(1);
        let room = limits.max_part_bytes.min(limits.max_total_bytes.saturating_sub(self.total));
        let mut buf = Vec::new();
        let mut reader = (&mut entry).take(room + 1);
        let mut chunk = [0u8; 16 * 1024];
        loop {
            let n = reader
                .read(&mut chunk)
                .map_err(|e| malformed(self.format, format!("the part {name:?} is damaged ({e})")))?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            let read = buf.len() as u64;
            if read > room {
                return Err(ReqError::Limit {
                    what: "decompressed bytes",
                    got: read,
                    max: room,
                });
            }
            // the ratio is checked while reading, so a bomb is stopped early, not after it has filled memory
            if read > 1024 * 1024 && read / stored > limits.max_ratio {
                return Err(ReqError::ZipBomb {
                    part: name.to_owned(),
                    max_ratio: limits.max_ratio,
                });
            }
        }
        self.total += buf.len() as u64;
        String::from_utf8(buf)
            .map(Some)
            .map_err(|_| malformed(self.format, format!("the part {name:?} is not UTF-8 text")))
    }
}
