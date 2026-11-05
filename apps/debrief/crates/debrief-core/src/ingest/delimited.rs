//! The delimited-text (CSV) importer, streaming: the same reading as [`crate::importers::CsvImporter`] (column
//! roles from the header or the head of the file, continuation rows, an unquoted delimiter in the last column),
//! fed line by line from a [`LineReader`] so a file is never held whole (plan package 3.5, L2/L6).
//!
//! Record boundaries come from `csv_core`'s state machine, one physical line at a time, so a quoted field with
//! a line break is one row exactly as in the in-memory importer. The column roles and the day order are decided
//! once from the head the file plan read ([`crate::importers::csv_stream_head`]); the rows are folded into events
//! by the same [`CsvLayout::fold`] the in-memory importer uses, so both give the same events.
//!
//! Differences from the in-memory importer: a row (all its lines) over [`MAX_EVENT_BYTES`] is dropped and
//! reported instead of read whole, a lone `\r` inside a line does not end a row, and the line break inside a
//! quoted field is `\n` whatever the file used.

use super::chunk::LineReader;
use super::problems::ProblemKind;
use super::sink::{EventSink, ParsedEvent};
use super::MAX_EVENT_BYTES;
use crate::importers::{CsvFold, CsvFolded, CsvStreamHead};
use anyhow::Result;
use csv_core::{ReadRecordResult, Reader, ReaderBuilder, Terminator};
use std::time::Instant;

/// Reads the rows of a delimited file into `sink`; returns the number of lines skipped.
pub fn import_delimited(head: CsvStreamHead, lines: &mut LineReader, sink: &mut dyn EventSink) -> Result<usize> {
    let started = Instant::now();
    let CsvStreamHead { delim, has_header, layout } = head;
    log::debug!(
        "delimited import: delimiter byte {delim}, header {has_header}, {}",
        layout.summary()
    );
    let mut seen = 0u32;
    let mut dropped = 0usize;
    let mut reader = new_reader(delim);
    let mut rows = Rows::default();
    let mut fold = CsvFold::default();
    let mut cells = csv::StringRecord::new();
    // The event being built: its text (all its lines) and where it starts.
    let mut event_raw = String::new();
    let mut event_offset = 0u64;
    let mut skipped = 0usize;
    let mut header_pending = has_header;

    while let Some(l) = lines.next_line()? {
        seen += 1;
        if seen.is_multiple_of(CANCEL_EVERY) {
            sink.check()?;
        }
        if l.too_long {
            skipped += 1;
            sink.problem(ProblemKind::LineTooLong, l.number);
            if rows.open {
                // The row cannot be finished without this line: drop it and start again on the next one.
                drop_row(&rows, sink, &mut skipped);
                dropped += 1;
                reader = new_reader(delim);
                rows.reset();
            }
            continue;
        }
        let was_open = rows.open;
        if !was_open {
            rows.begin(l.number, l.byte_offset);
        }
        let done = rows.feed(&mut reader, l.text);
        if !done {
            // Still inside a quoted field (or a blank line outside any row).
            if !was_open && l.text.is_empty() {
                rows.open = false;
                if fold.has_event() {
                    push_capped(&mut event_raw, "");
                }
                continue;
            }
            rows.open = true;
            rows.push_text(l.text, was_open);
            if rows.raw.len() > MAX_EVENT_BYTES {
                // A row this large is an unclosed quote: drop it and start again on the next line.
                drop_row(&rows, sink, &mut skipped);
                dropped += 1;
                reader = new_reader(delim);
                rows.reset();
            }
            continue;
        }
        rows.push_text(l.text, was_open);
        rows.open = false;
        if rows.wide {
            drop_row(&rows, sink, &mut skipped);
            dropped += 1;
            rows.reset();
            continue;
        }
        rows.fill(&mut cells);
        if header_pending {
            header_pending = false;
            continue;
        }
        let first_raw = rows.raw.split('\n').next().unwrap_or("").trim_end_matches('\r');
        match layout.fold(&mut fold, rows.line, &cells, first_raw) {
            CsvFolded::Skipped => {
                if fold.has_event() {
                    push_capped(&mut event_raw, &rows.raw);
                }
            }
            CsvFolded::Continued => push_capped(&mut event_raw, &rows.raw),
            CsvFolded::Started(prev) => {
                if let Some(rec) = prev {
                    emit(rec, &mut event_raw, event_offset, sink)?;
                }
                event_raw.clear();
                event_raw.push_str(&rows.raw);
                event_offset = rows.offset;
            }
        }
    }
    // A row still open at the end of the file (an unclosed quote) is flushed as it stands.
    if rows.open && rows.flush(&mut reader) {
        rows.fill(&mut cells);
        if !header_pending {
            let first_raw = rows.raw.split('\n').next().unwrap_or("");
            match layout.fold(&mut fold, rows.line, &cells, first_raw) {
                CsvFolded::Continued => push_capped(&mut event_raw, &rows.raw),
                CsvFolded::Started(prev) => {
                    if let Some(rec) = prev {
                        emit(rec, &mut event_raw, event_offset, sink)?;
                    }
                    event_raw.clear();
                    event_raw.push_str(&rows.raw);
                    event_offset = rows.offset;
                }
                CsvFolded::Skipped => {}
            }
        }
    }
    if let Some(rec) = layout.finish(&mut fold) {
        emit(rec, &mut event_raw, event_offset, sink)?;
    }
    log::debug!(
        "delimited import: {seen} lines in {} ms, {skipped} skipped, {dropped} rows dropped",
        started.elapsed().as_millis()
    );
    Ok(skipped)
}

/// How often (in lines) the import looks at the cancel flag.
const CANCEL_EVERY: u32 = 1024;
/// Most cells a row may have; a wider row is dropped.
const MAX_CELLS: usize = 4096;

fn new_reader(delim: u8) -> Reader {
    ReaderBuilder::new().delimiter(delim).terminator(Terminator::Any(b'\n')).build()
}

/// Reports every line of a row that is dropped.
fn drop_row(rows: &Rows, sink: &mut dyn EventSink, skipped: &mut usize) {
    for i in 0..rows.lines {
        sink.problem(ProblemKind::EventTooLarge, rows.line + i);
    }
    *skipped += rows.lines as usize;
}

fn push_capped(raw: &mut String, line: &str) {
    if raw.len() < MAX_EVENT_BYTES {
        raw.push('\n');
        raw.push_str(line);
    }
}

fn emit(record: crate::model::Record, raw: &mut String, byte_offset: u64, sink: &mut dyn EventSink) -> Result<()> {
    let mut text = std::mem::take(raw);
    text.truncate(text.trim_end().len());
    sink.event(ParsedEvent {
        record,
        raw: text,
        byte_offset,
    })
}

/// The row being read: its lines, where it starts and the cells `csv_core` found.
#[derive(Default)]
struct Rows {
    open: bool,
    line: u32,
    lines: u32,
    wide: bool,
    offset: u64,
    raw: String,
    out: Vec<u8>,
    out_len: usize,
    ends: Vec<usize>,
    ends_len: usize,
}

impl Rows {
    fn begin(&mut self, line: u32, offset: u64) {
        self.line = line;
        self.lines = 0;
        self.wide = false;
        self.offset = offset;
        self.raw.clear();
        self.out_len = 0;
        self.ends_len = 0;
        if self.out.is_empty() {
            self.out.resize(4096, 0);
            self.ends.resize(64, 0);
        }
    }

    fn reset(&mut self) {
        self.open = false;
        self.lines = 0;
        self.wide = false;
        self.raw.clear();
        self.out_len = 0;
        self.ends_len = 0;
    }

    fn push_text(&mut self, text: &str, continued: bool) {
        self.lines += 1;
        if continued {
            self.raw.push('\n');
        }
        if self.raw.len() < MAX_EVENT_BYTES + 1 {
            self.raw.push_str(text);
        }
    }

    /// Feeds one line and its line break; true when a row ended with it.
    fn feed(&mut self, reader: &mut Reader, text: &str) -> bool {
        // An empty `run` is the end of the input to csv_core, which would end a row inside a quote.
        let first = !text.is_empty() && self.run(reader, text.as_bytes());
        self.run(reader, b"\n") || first
    }

    /// Feeds the reader the end of the input, finishing a row left open.
    fn flush(&mut self, reader: &mut Reader) -> bool {
        self.run(reader, &[])
    }

    /// Runs `input` through the reader; true when it completed a row.
    fn run(&mut self, reader: &mut Reader, mut input: &[u8]) -> bool {
        loop {
            let (res, nin, nout, nend) = reader.read_record(input, &mut self.out[self.out_len..], &mut self.ends[self.ends_len..]);
            input = &input[nin..];
            self.out_len += nout;
            self.ends_len += nend;
            match res {
                ReadRecordResult::InputEmpty => return false,
                ReadRecordResult::OutputFull => {
                    let n = self.out.len() * 2;
                    self.out.resize(n, 0);
                }
                ReadRecordResult::OutputEndsFull if self.ends.len() >= MAX_CELLS => {
                    // Too many cells: keep reading to the row's end but keep none of them.
                    self.wide = true;
                    self.out_len = 0;
                    self.ends_len = 0;
                }
                ReadRecordResult::OutputEndsFull => {
                    let n = self.ends.len() * 2;
                    self.ends.resize(n, 0);
                }
                ReadRecordResult::Record => return true,
                ReadRecordResult::End => return false,
            }
        }
    }

    /// The cells of the finished row (trimmed) into `cells`.
    fn fill(&mut self, cells: &mut csv::StringRecord) {
        cells.clear();
        let mut start = 0;
        for &end in &self.ends[..self.ends_len] {
            cells.push_field(String::from_utf8_lossy(&self.out[start..end]).trim());
            start = end;
        }
        self.out_len = 0;
        self.ends_len = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importers::{csv_stream_head, CsvImporter, ImportCtx};
    use crate::ingest::chunk::{Encoding, ReaderChunks};
    use crate::ingest::legacy::import_text_whole;
    use crate::profile::CompiledProfile;
    use std::io::Cursor;
    use std::path::Path;

    #[derive(Default)]
    struct Events {
        events: Vec<ParsedEvent>,
        problems: Vec<(ProblemKind, u32)>,
    }

    impl EventSink for Events {
        fn event(&mut self, ev: ParsedEvent) -> Result<()> {
            self.events.push(ev);
            Ok(())
        }
        fn problem(&mut self, kind: ProblemKind, line: u32) {
            self.problems.push((kind, line));
        }
    }

    fn profile(extra: &str) -> CompiledProfile {
        // Rows with a quoted line break would defeat the delimiter guess, so the default is explicit.
        crate::parser::testing::compiled(if extra.is_empty() { "[format.csv]\ndelimiter = ','" } else { extra })
    }

    fn ctx(p: &CompiledProfile) -> ImportCtx<'_> {
        ImportCtx {
            profile: p,
            path: Path::new("x.csv"),
            file_date: None,
        }
    }

    /// The streaming read of `text`, in reads of `block` bytes.
    fn streamed(extra: &str, text: &str, block: usize, max_line: usize) -> (Events, usize) {
        let p = profile(extra);
        let head = csv_stream_head(&ctx(&p), text).unwrap();
        let mut chunks = ReaderChunks::with_limits(Cursor::new(text.as_bytes().to_vec()), Encoding::Utf8, 0, block, max_line);
        let mut lines = LineReader::new(&mut chunks);
        let mut out = Events::default();
        let skipped = import_delimited(head, &mut lines, &mut out).unwrap();
        (out, skipped)
    }

    /// What the in-memory importer makes of `text`.
    fn whole(extra: &str, text: &str) -> Events {
        let p = profile(extra);
        let mut out = Events::default();
        import_text_whole(&CsvImporter, &ctx(&p), text, 0, &mut out).unwrap();
        out
    }

    fn view(e: &Events) -> Vec<String> {
        e.events
            .iter()
            .map(|e| format!("{:?} | {:?} | {}", e.record, e.raw, e.byte_offset))
            .collect()
    }

    const SHAPES: &[(&str, &str)] = &[
        ("", "Timestamp,Level,Message\n2026-10-14 10:00:00,Info,first\n2026-10-14 10:00:01,Error,second\n"),
        ("", "Timestamp,Level,Message\r\n2026-10-14 10:00:00,Info,first\r\n2026-10-14 10:00:01,Error,second\r\n"),
        ("", "Timestamp,Level,Message\n2026-10-14 10:00:00,Info,\"a\nquoted, line\"\n2026-10-14 10:00:01,Warn,\"b\"\"c\"\n"),
        ("", "Timestamp,Level,Message\n2026-10-14 10:00:00,Info,héllo wörld ✓\n2026-10-14 10:00:01,Info,日本語のログ\n"),
        ("", "Timestamp,Level,Message\n2026-10-14 10:00:00,Error,boom\n  at Foo.Bar()\n  at Main()\n2026-10-14 10:00:01,Info,ok\n"),
        ("", "Timestamp,Level,Message\n\n2026-10-14 10:00:00,Info,a\n\n\n2026-10-14 10:00:01,Info,b\n,,\n"),
        ("", "Timestamp,Level,Message\n2026-10-14 10:00:00,Info,first\n2026-10-14 10:00:01,Info,no newline at the end"),
        ("", "Timestamp,Level,Message\n2026-10-14 10:00:00,Info,stops, the service, now\n"),
        ("[format.csv]\nhas_header = false", "WEB01,2026-10-14 10:00:00,5\nWEB02,2026-10-14 10:01:00,7\n,,\n"),
        ("", "Timestamp;Level;Machine;Cpu;Message\n14/10/2026 10:00:00;Warning;WEB01;71.5;\"High CPU; throttling\"\n15/10/2026 10:00:00;Information;WEB02;12;ok\n"),
        ("[format.csv]\ndelimiter = '\\t'", "time\tmsg\n2026-10-14 10:00:00\tb\n2026-10-14 10:00:01\tc\n"),
    // A blank line inside a quoted field, then a field after the quote (LF and CRLF).
    ("", "Timestamp,Level,Message,Extra\n2026-10-14 10:00:00,Info,\"a\n\nb\",x\n2026-10-14 10:00:01,Info,ok,y\n"),
    ("", "Timestamp,Level,Message,Extra\r\n2026-10-14 10:00:00,Info,\"a\r\n\r\nb\",x\r\n2026-10-14 10:00:01,Info,ok,y\r\n"),
    // Spaces around the cells are not part of the values.
    ("", "Timestamp,Level,Message\n2026-10-14 10:00:00 , Info ,  padded  \n2026-10-14 10:00:01,Info,x\n"),
];

    #[test]
    fn streaming_gives_the_events_of_the_in_memory_importer() {
        for (extra, text) in SHAPES {
            let (got, skipped) = streamed(extra, text, CHUNK_FOR_TESTS, crate::importers::MAX_LINE_LEN);
            assert_eq!(skipped, 0, "{text:?}");
            // The in-memory importer keeps the CRLF inside quoted fields and the events' text as written: the
            // streaming one reads lines, so compare after the line breaks are made alike.
            let mut want = whole(extra, text);
            for e in &mut want.events {
                e.raw = e.raw.replace("\r\n", "\n");
                e.record.message = e.record.message.replace("\r\n", "\n");
            }
            assert_eq!(view(&got), view(&want), "{extra:?} {text:?}");
        }
    }

    const CHUNK_FOR_TESTS: usize = 1 << 20;

    #[test]
    fn the_events_do_not_depend_on_where_the_reads_end() {
        for (extra, text) in SHAPES {
            let (want, _) = streamed(extra, text, CHUNK_FOR_TESTS, crate::importers::MAX_LINE_LEN);
            for block in [2, 3, 5, 7, 16, 64] {
                let (got, _) = streamed(extra, text, block, crate::importers::MAX_LINE_LEN);
                assert_eq!(view(&got), view(&want), "block {block} {text:?}");
            }
        }
    }

    #[test]
    fn a_quoted_line_break_and_a_multi_byte_character_cross_a_read_boundary() {
        let text = "Timestamp,Level,Message\n2026-10-14 10:00:00,Info,\"first\nsecond é\"\n2026-10-14 10:00:01,Info,é\n";
        // Every block size puts the break between the quote and the line break, or inside the two bytes of `é`.
        for block in 2..text.len() {
            let (got, _) = streamed("", text, block, crate::importers::MAX_LINE_LEN);
            assert_eq!(got.events.len(), 2, "block {block}");
            assert_eq!(got.events[0].record.message, "first\nsecond é", "block {block}");
            assert_eq!(got.events[1].record.message, "é", "block {block}");
        }
    }

    #[test]
    fn a_line_longer_than_a_read_and_a_line_over_the_limit() {
        let long = "x".repeat(200);
        let text = format!("Timestamp,Level,Message\n2026-10-14 10:00:00,Info,{long}\n2026-10-14 10:00:01,Info,short\n");
        let (got, skipped) = streamed("", &text, 16, crate::importers::MAX_LINE_LEN);
        assert_eq!((skipped, got.events.len()), (0, 2));
        assert_eq!(got.events[0].record.message, long);
        let (got, skipped) = streamed("", &text, 16, 100);
        assert_eq!((skipped, got.events.len()), (1, 1));
        assert_eq!(got.problems, [(ProblemKind::LineTooLong, 2)]);
        assert_eq!(got.events[0].record.message, "short");
    }

    #[test]
    fn an_unclosed_quote_is_dropped_at_the_event_limit_not_read_to_the_end() {
        let mut text = String::from("Timestamp,Level,Message\n2026-10-14 10:00:00,Info,\"never closed\n");
        let filler = "y".repeat(1 << 16);
        for _ in 0..(MAX_EVENT_BYTES >> 16) + 2 {
            text.push_str(&filler);
            text.push('\n');
        }
        let (got, skipped) = streamed("", &text, CHUNK_FOR_TESTS, crate::importers::MAX_LINE_LEN);
        assert!(skipped >= 1);
        assert!(
            got.problems.iter().any(|(k, _)| *k == ProblemKind::EventTooLarge),
            "{:?}",
            got.problems
        );
    }

    #[test]
    fn a_header_only_file_and_a_row_without_a_newline() {
        let (got, _) = streamed(
            "",
            "Timestamp,Level,Message\n2026-10-14 10:00:00,Info,only",
            8,
            crate::importers::MAX_LINE_LEN,
        );
        assert_eq!(got.events.len(), 1);
        assert_eq!(got.events[0].record.message, "only");
        assert_eq!(got.events[0].raw, "2026-10-14 10:00:00,Info,only");
    }

    /// A sink whose cancel check fails the first time it is asked.
    #[derive(Default)]
    struct Cancelling {
        events: usize,
    }

    impl EventSink for Cancelling {
        fn event(&mut self, _ev: ParsedEvent) -> Result<()> {
            self.events += 1;
            Ok(())
        }
        fn check(&mut self) -> Result<()> {
            anyhow::bail!("cancelled")
        }
        fn problem(&mut self, _kind: ProblemKind, _line: u32) {}
    }

    fn row_text(rows: usize) -> String {
        let mut text = String::from("Timestamp,Level,Message\n");
        for n in 0..rows {
            text.push_str(&format!("2026-10-14 10:00:00,Info,row {n}\n"));
        }
        text
    }

    #[test]
    fn the_cancel_flag_is_looked_at_every_so_many_lines_not_only_per_event_batch() {
        let text = row_text(5000);
        let p = profile("");
        let head = csv_stream_head(&ctx(&p), &text).unwrap();
        let mut chunks = ReaderChunks::with_limits(
            Cursor::new(text.as_bytes().to_vec()),
            Encoding::Utf8,
            0,
            1 << 16,
            crate::importers::MAX_LINE_LEN,
        );
        let mut lines = LineReader::new(&mut chunks);
        let mut sink = Cancelling::default();
        assert!(import_delimited(head, &mut lines, &mut sink).is_err());
        assert!(sink.events <= 2 * CANCEL_EVERY as usize, "{} events before the stop", sink.events);
    }

    #[test]
    fn a_blank_line_inside_quotes_is_part_of_the_field() {
        let text = "Timestamp,Level,Message,Extra\n2026-10-14 10:00:00,Info,\"a\n\nb\",x\n2026-10-14 10:00:01,Info,ok,y\n";
        let (got, _) = streamed("", text, 7, crate::importers::MAX_LINE_LEN);
        assert_eq!(got.events.len(), 2);
        assert_eq!(got.events[0].record.message, "a\n\nb");
        assert_eq!(got.events[1].record.message, "ok");
    }

    #[test]
    fn an_event_keeps_its_own_text_and_no_other_rows() {
        let text = "Timestamp,Level,Message\n2026-10-14 10:00:00,Info,\"a\nb\"\n2026-10-14 10:00:01,Info,c\n2026-10-14 10:00:02,Info,d\n";
        let (got, _) = streamed("", text, 5, crate::importers::MAX_LINE_LEN);
        let raws: Vec<&str> = got.events.iter().map(|e| e.raw.as_str()).collect();
        assert_eq!(
            raws,
            [
                "2026-10-14 10:00:00,Info,\"a\nb\"",
                "2026-10-14 10:00:01,Info,c",
                "2026-10-14 10:00:02,Info,d"
            ]
        );
    }

    #[test]
    fn a_row_open_at_the_end_of_the_file_is_flushed() {
        let text = "Timestamp,Level,Message\n2026-10-14 10:00:00,Info,\"never closed at the end";
        let (got, _) = streamed("", text, 16, crate::importers::MAX_LINE_LEN);
        assert_eq!(got.events.len(), 1);
        assert!(
            got.events[0].record.message.contains("never closed at the end"),
            "{:?}",
            got.events[0].record.message
        );
    }

    #[test]
    fn the_rows_after_a_dropped_row_are_read_from_a_fresh_reader() {
        let mut text = String::from("Timestamp,Level,Message\n2026-10-14 10:00:00,Info,\"never closed\n");
        let filler = "y".repeat(1 << 16);
        for _ in 0..(MAX_EVENT_BYTES >> 16) + 2 {
            text.push_str(&filler);
            text.push('\n');
        }
        text.push_str("2026-10-14 10:00:01,Info,after one\n2026-10-14 10:00:02,Info,after two\n");
        let (got, skipped) = streamed("", &text, CHUNK_FOR_TESTS, crate::importers::MAX_LINE_LEN);
        let messages: Vec<&str> = got.events.iter().map(|e| e.record.message.as_str()).collect();
        assert_eq!(messages.last(), Some(&"after two"), "{}", messages.len());
        assert!(messages.contains(&"after one"));
        let lines_reported = got.problems.iter().filter(|(k, _)| *k == ProblemKind::EventTooLarge).count();
        assert!(
            lines_reported > 1 && lines_reported <= skipped,
            "{lines_reported} lines reported, {skipped} skipped"
        );
    }

    #[test]
    fn a_line_over_the_limit_inside_quotes_drops_the_open_row_and_the_reader_starts_over() {
        let long = "z".repeat(200);
        let text = format!("Timestamp,Level,Message\n2026-10-14 10:00:00,Info,\"open\n{long}\nmore\"\n2026-10-14 10:00:01,Info,after\n");
        let (got, _) = streamed("", &text, CHUNK_FOR_TESTS, 100);
        let messages: Vec<&str> = got.events.iter().map(|e| e.record.message.as_str()).collect();
        assert_eq!(messages.last(), Some(&"after"), "{messages:?}");
        assert!(!messages.iter().any(|m| m.contains("open")), "{messages:?}");
        assert!(
            got.problems.iter().any(|(k, n)| *k == ProblemKind::LineTooLong && *n == 3),
            "{:?}",
            got.problems
        );
        assert!(
            got.problems.iter().any(|(k, n)| *k == ProblemKind::EventTooLarge && *n == 2),
            "{:?}",
            got.problems
        );
    }

    #[test]
    fn a_row_with_too_many_cells_is_dropped_and_reported() {
        let wide = ",x".repeat(MAX_CELLS * 2);
        let text = format!("Timestamp,Level,Message\n2026-10-14 10:00:00,Info,wide{wide}\n2026-10-14 10:00:01,Info,after\n");
        let (got, skipped) = streamed("", &text, CHUNK_FOR_TESTS, crate::importers::MAX_LINE_LEN * 8);
        let messages: Vec<&str> = got.events.iter().map(|e| e.record.message.as_str()).collect();
        assert_eq!(messages, ["after"]);
        assert_eq!(skipped, 1);
        assert!(
            got.problems.iter().any(|(k, n)| *k == ProblemKind::EventTooLarge && *n == 2),
            "{:?}",
            got.problems
        );
    }
}
