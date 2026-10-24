//! Test writers shared by the unit tests.

use std::io::Write;

/// A writer that accepts `ok` writes and then fails like a closed pipe.
pub(crate) struct FailAfter(pub usize);

impl Write for FailAfter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.0 == 0 {
            return Err(std::io::ErrorKind::BrokenPipe.into());
        }
        self.0 -= 1;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fails_after_the_allowed_writes() {
        let mut w = FailAfter(1);
        assert!(w.write(b"a").is_ok());
        assert!(w.flush().is_ok());
        assert!(w.write(b"b").is_err());
    }
}
