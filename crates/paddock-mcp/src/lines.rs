//! Newline-delimited JSON: the framing of the stdio transport (and the loopback one). Lines are read with a bound, so a client
//! cannot make the server buffer without limit.

use crate::server::{Server, Session};
use std::io::{self, BufRead, Write};

/// What one read produced.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// A complete line (lossy UTF-8, without the newline).
    Text(String),
    /// The line was longer than the bound; it was skipped up to its newline.
    TooLong,
    /// The input ended.
    Eof,
}

/// Reads one line of at most `max` bytes.
pub(crate) fn read_line(r: &mut impl BufRead, max: usize) -> io::Result<Line> {
    let mut buf: Vec<u8> = Vec::new();
    let mut too_long = false;
    let mut seen_any = false;
    loop {
        let chunk = r.fill_buf()?;
        if chunk.is_empty() {
            return Ok(if !seen_any {
                Line::Eof
            } else if too_long {
                Line::TooLong
            } else {
                Line::Text(String::from_utf8_lossy(&buf).into_owned())
            });
        }
        seen_any = true;
        let (take, done) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        if !too_long {
            let body = &chunk[..if done { take - 1 } else { take }];
            if buf.len() + body.len() > max {
                too_long = true;
                buf.clear();
            } else {
                buf.extend_from_slice(body);
            }
        }
        r.consume(take);
        if done {
            return Ok(if too_long {
                Line::TooLong
            } else {
                Line::Text(String::from_utf8_lossy(&buf).into_owned())
            });
        }
    }
}

/// Serves one client until its input ends: a line in, at most one line out.
pub fn serve_lines(
    server: &Server,
    session: &mut Session,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> io::Result<()> {
    loop {
        let reply = match read_line(input, server.limits().max_line_bytes)? {
            Line::Eof => return Ok(()),
            Line::TooLong => Some(server.too_long()),
            Line::Text(t) if t.trim().is_empty() => None,
            Line::Text(t) => server.handle(session, &t),
        };
        if let Some(r) = reply {
            output.write_all(r.as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}

/// The stdio transport: the host starts the app's `--mcp` mode and talks over its standard input and output. The client's name
/// comes from `initialize`. Nothing but protocol lines may ever be written to `output`; logs go to the log file.
pub fn serve_stdio(
    server: &Server,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> io::Result<()> {
    serve_lines(server, &mut Session::new(""), input, output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn lines(bytes: &[u8], max: usize) -> Vec<Line> {
        let mut c = Cursor::new(bytes.to_vec());
        let mut out = vec![];
        loop {
            match read_line(&mut c, max).unwrap() {
                Line::Eof => return out,
                l => out.push(l),
            }
        }
    }

    #[test]
    fn lines_are_split_bounded_and_the_rest_of_a_long_line_is_skipped() {
        let t = |s: &str| Line::Text(s.into());
        assert_eq!(lines(b"a\nbb\n", 10), [t("a"), t("bb")]);
        assert_eq!(
            lines(b"a\nlast", 10),
            [t("a"), t("last")],
            "a final line without a newline counts"
        );
        assert_eq!(lines(b"\n\n", 10), [t(""), t("")]);
        assert_eq!(lines(b"aaaaaaaaaaaaaaa\nok\n", 5), [Line::TooLong, t("ok")]);
        assert_eq!(
            lines(b"aaaaaaaaaaaaaaa", 5),
            [Line::TooLong],
            "too long and never ended"
        );
        assert_eq!(
            lines(b"abcde\n", 5),
            [t("abcde")],
            "exactly the bound is fine"
        );
        assert_eq!(
            lines(b"\xff\xfe\n", 5),
            [t("\u{fffd}\u{fffd}")],
            "invalid UTF-8 is replaced, not trusted"
        );
        // Chunk boundaries: a reader that hands out one byte at a time.
        let mut slow = io::BufReader::with_capacity(1, Cursor::new(b"abc\nde\n".to_vec()));
        assert_eq!(read_line(&mut slow, 10).unwrap(), t("abc"));
        assert_eq!(read_line(&mut slow, 10).unwrap(), t("de"));
    }
}
