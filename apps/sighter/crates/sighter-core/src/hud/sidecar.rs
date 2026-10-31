//! The HUD sidecar track of a run video: the trace events of the run, stored **beside** the video and never burnt into
//! its frames, so training data stays clean.
//!
//! Format `sighter-hud` version 1 (this is a proposal of wave 0; ADR 0001 and the design review do not fix a byte
//! format): UTF-8 JSON Lines. Line 1 is the header `{"format":"sighter-hud","version":1}`; every further line is one
//! [`TraceEvent`]. Times must not go backwards and sequence numbers must increase. The parser enforces limits, never
//! panics, and refuses unknown versions with a message that says what to do. Text in events is [`HudText`], so a track
//! cannot hold a raw secret that went through the masking types.
//!
//! [`HudText`]: super::mask::HudText

use super::trace::{Micros, TraceEvent};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

/// The format name in the header.
pub const FORMAT: &str = "sighter-hud";
/// The version this code reads and writes.
pub const VERSION: u32 = 1;
/// Longest accepted line in bytes.
pub const MAX_LINE_BYTES: usize = 64 * 1024;
/// Most lines in one file, the header included.
pub const MAX_LINES: usize = 1_000_000;
/// Most events in one track (every line but the header).
pub const MAX_EVENTS: usize = MAX_LINES - 1;
/// Largest file in bytes (256 MiB).
pub const MAX_FILE_BYTES: usize = 256 * 1024 * 1024;
/// The sidecar's file-name suffix.
pub const SUFFIX: &str = ".hud.jsonl";

/// Why a track could not be read or built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SidecarError {
    /// The input has no header line.
    #[error("the track is empty: expected a '{FORMAT}' header line")]
    MissingHeader,
    /// The header is not valid or names another format.
    #[error("line 1 is not a '{FORMAT}' header")]
    BadHeader,
    /// A higher major version than this build reads.
    #[error(
        "This HUD file was written by a newer Sighter (format version {found}; this build reads version {VERSION}); update Sighter to read it"
    )]
    UnsupportedVersion {
        /// The version in the file.
        found: u32,
    },
    /// A line is not a valid event.
    #[error("line {line} is not a valid trace event: {reason}")]
    BadEvent {
        /// 1-based line number.
        line: usize,
        /// Why.
        reason: String,
    },
    /// A line is longer than [`MAX_LINE_BYTES`].
    #[error("line {line} is longer than {MAX_LINE_BYTES} bytes")]
    LineTooLong {
        /// 1-based line number.
        line: usize,
    },
    /// The file is larger than [`MAX_FILE_BYTES`].
    #[error("the HUD file is larger than {MAX_FILE_BYTES} bytes")]
    FileTooLarge,
    /// More lines than [`MAX_LINES`].
    #[error("the HUD file has more than {MAX_LINES} lines")]
    TooManyLines,
    /// More events than [`MAX_EVENTS`].
    #[error("the track holds more than {MAX_EVENTS} events")]
    TooManyEvents,
    /// Events out of order.
    #[error("line {line}: events must not go back in time or reuse a sequence number")]
    OutOfOrder {
        /// 1-based line number.
        line: usize,
    },
}

#[derive(Serialize, Deserialize)]
struct Header {
    format: String,
    version: u32,
}

/// The events of one run, in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Track {
    events: Vec<TraceEvent>,
    skipped: usize,
}

/// The sidecar file name for a video file name: `run.mp4` becomes `run.mp4.hud.jsonl`.
pub fn sidecar_name(video_file_name: &str) -> String {
    format!("{video_file_name}{SUFFIX}")
}

fn ordered(prev: Option<&TraceEvent>, e: &TraceEvent) -> bool {
    prev.is_none_or(|p| e.seq > p.seq && e.at >= p.at)
}

/// Refuses a line over [`MAX_LINE_BYTES`], or one that would take a file of `written` bytes over [`MAX_FILE_BYTES`].
fn within_limits(line: usize, text_len: usize, written: usize) -> Result<(), SidecarError> {
    if text_len > MAX_LINE_BYTES {
        return Err(SidecarError::LineTooLong { line });
    }
    if written + text_len + 1 > MAX_FILE_BYTES {
        return Err(SidecarError::FileTooLarge);
    }
    Ok(())
}

impl Track {
    /// A track from events; they must be in order (increasing `seq`, non-decreasing time).
    pub fn new(events: Vec<TraceEvent>) -> Result<Self, SidecarError> {
        if events.len() > MAX_EVENTS {
            return Err(SidecarError::TooManyEvents);
        }
        for (i, w) in events.windows(2).enumerate() {
            if !ordered(Some(&w[0]), &w[1]) {
                return Err(SidecarError::OutOfOrder { line: i + 3 });
            }
        }
        Ok(Self { events, skipped: 0 })
    }
    /// Lines the reader skipped because they were malformed, unknown or out of order.
    pub fn skipped(&self) -> usize {
        self.skipped
    }
    /// The events.
    pub fn events(&self) -> &[TraceEvent] {
        &self.events
    }
    /// Writes the track as JSON Lines. Refuses what [`Track::parse`] would refuse: a line over [`MAX_LINE_BYTES`] or a file
    /// over [`MAX_FILE_BYTES`] is an error, so a track that was written can always be read back.
    pub fn to_jsonl(&self) -> Result<String, SidecarError> {
        let mut out = String::new();
        let header = Header {
            format: FORMAT.to_owned(),
            version: VERSION,
        };
        let mut push = |line: usize, json: serde_json::Result<String>| -> Result<(), SidecarError> {
            // Serialising these plain types cannot fail; an empty line would be skipped and counted by the reader.
            let text = json.unwrap_or_default();
            within_limits(line, text.len(), out.len())?;
            let _ = writeln!(out, "{text}");
            Ok(())
        };
        push(1, serde_json::to_string(&header))?;
        for (i, e) in self.events.iter().enumerate() {
            push(i + 2, serde_json::to_string(e))?;
        }
        Ok(out)
    }
    /// Parses a track. A malformed line, an event this build does not know, or an event out of order is **skipped and
    /// counted** ([`Track::skipped`]), never a panic; unknown fields are ignored. A file or line over a limit, a bad
    /// header or a newer major version stops the read with an error that says what to do.
    pub fn parse(text: &str) -> Result<Self, SidecarError> {
        if text.len() > MAX_FILE_BYTES {
            return Err(SidecarError::FileTooLarge);
        }
        let mut lines = text.lines().enumerate().map(|(i, l)| (i + 1, l));
        let (_, first) = lines.next().ok_or(SidecarError::MissingHeader)?;
        if first.len() > MAX_LINE_BYTES {
            return Err(SidecarError::LineTooLong { line: 1 });
        }
        let header: Header = serde_json::from_str(first).map_err(|_| SidecarError::BadHeader)?;
        if header.format != FORMAT || header.version == 0 {
            return Err(SidecarError::BadHeader);
        }
        if header.version > VERSION {
            return Err(SidecarError::UnsupportedVersion { found: header.version });
        }
        let mut events: Vec<TraceEvent> = Vec::new();
        let mut skipped = 0;
        for (line, l) in lines {
            if line > MAX_LINES {
                return Err(SidecarError::TooManyLines);
            }
            if l.len() > MAX_LINE_BYTES {
                return Err(SidecarError::LineTooLong { line });
            }
            if l.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<TraceEvent>(l) {
                Ok(e) if ordered(events.last(), &e) => events.push(e),
                _ => skipped += 1,
            }
        }
        Ok(Self { events, skipped })
    }
    /// The events whose time lies in `[from, to]`, for drawing the HUD over a video frame (a viewer asks for the span
    /// around the frame's time). Events are sorted, so this is a binary search.
    pub fn between(&self, from: Micros, to: Micros) -> &[TraceEvent] {
        let start = self.events.partition_point(|e| e.at < from);
        let end = self.events.partition_point(|e| e.at <= to);
        &self.events[start..end.max(start)]
    }
}

#[cfg(test)]
mod tests {
    use super::super::mask::{HudText, SecretName, mask};
    use super::super::trace::{DetectionEvent, HintEvent, Layer, LayerOutcome, LocateEvent, LocateStatus, StepEvent, TraceKind};
    use super::*;
    use crate::detect::{ClassId, Score};
    use crate::geometry::{Pt, Rect};
    use proptest::prelude::*;

    fn ev(seq: u64, at: u64, kind: TraceKind) -> TraceEvent {
        TraceEvent {
            seq,
            at: Micros(at),
            event: kind,
        }
    }
    fn sample() -> Vec<TraceEvent> {
        vec![
            ev(0, 0, TraceKind::Step(StepEvent::Started { step: 1 })),
            ev(1, 10, TraceKind::Locate(LocateEvent::Started { id: 7 })),
            ev(
                2,
                20,
                TraceKind::Locate(LocateEvent::Layer {
                    id: 7,
                    layer: Layer::Detector,
                    outcome: LayerOutcome::Hit,
                    took_us: 31_000,
                }),
            ),
            ev(
                3,
                30,
                TraceKind::Detection(DetectionEvent {
                    locate: 7,
                    rect: Rect::new(10.5, 20.0, 80.0, 24.0).unwrap(),
                    class: ClassId(3),
                    score: Score::new(0.92).unwrap(),
                    chosen: true,
                }),
            ),
            ev(
                4,
                30,
                TraceKind::Locate(LocateEvent::Finished {
                    id: 7,
                    status: LocateStatus::Found,
                }),
            ),
            ev(
                5,
                40,
                TraceKind::Step(StepEvent::Typed {
                    step: 1,
                    text: HudText::secret(SecretName::new("DB_PASSWORD").unwrap()),
                }),
            ),
            ev(
                6,
                50,
                TraceKind::Step(StepEvent::Clicked {
                    step: 1,
                    at: Pt::new(50.5, 32.0).unwrap(),
                }),
            ),
            ev(7, 60, TraceKind::Step(StepEvent::Finished { step: 1, ok: true })),
        ]
    }

    #[test]
    fn round_trips_every_event_kind() {
        let t = Track::new(sample()).unwrap();
        let text = t.to_jsonl().unwrap();
        assert!(text.starts_with("{\"format\":\"sighter-hud\",\"version\":1}\n"));
        assert_eq!(text.lines().count(), 1 + 8);
        assert_eq!(Track::parse(&text).unwrap(), t);
        assert!(!text.contains("hunter2") && text.contains("DB_PASSWORD"));
        assert_eq!(sidecar_name("run.mp4"), "run.mp4.hud.jsonl");
    }

    const HEAD: &str = "{\"format\":\"sighter-hud\",\"version\":1}\n";

    #[test]
    fn bad_headers_limits_and_newer_versions_stop_the_read() {
        assert_eq!(Track::parse(""), Err(SidecarError::MissingHeader));
        assert_eq!(Track::parse("nope"), Err(SidecarError::BadHeader));
        assert_eq!(Track::parse(r#"{"format":"other","version":1}"#), Err(SidecarError::BadHeader));
        assert_eq!(
            Track::parse(r#"{"format":"sighter-hud","version":0}"#),
            Err(SidecarError::BadHeader)
        );
        assert_eq!(
            Track::parse(r#"{"format":"sighter-hud","version":2}"#),
            Err(SidecarError::UnsupportedVersion { found: 2 })
        );
        assert!(
            SidecarError::UnsupportedVersion { found: 9 }
                .to_string()
                .starts_with("This HUD file was written by a newer Sighter")
        );
        assert_eq!(
            Track::parse(&format!("{HEAD}{}\n", "x".repeat(MAX_LINE_BYTES + 1))),
            Err(SidecarError::LineTooLong { line: 2 })
        );
        assert_eq!(
            Track::parse(&"x".repeat(MAX_LINE_BYTES + 1)),
            Err(SidecarError::LineTooLong { line: 1 })
        );
        assert_eq!(
            Track::parse(&format!("{HEAD}{}", "\n".repeat(MAX_LINES))),
            Err(SidecarError::TooManyLines)
        );
        assert_eq!((MAX_LINE_BYTES, MAX_LINES, MAX_FILE_BYTES), (65_536, 1_000_000, 268_435_456));
    }

    #[test]
    fn writing_enforces_the_limits_the_reader_enforces() {
        // HudText is cut to 512 characters when built, so a line cannot be made long by text alone; many 4-byte characters
        // in a secret-free label still fit. The limit is reachable only through a huge track, so test the file limit with
        // the smallest possible event repeated, and the line limit through the error type.
        let one = TraceEvent {
            seq: 0,
            at: Micros(0),
            event: TraceKind::Step(StepEvent::Started { step: 0 }),
        };
        let max_text = HudText::plain(&"\u{10FFFF}".repeat(10_000)); // cut to 512 characters
        assert_eq!(max_text.render().chars().count(), super::super::mask::MAX_HUD_TEXT);
        let typed = TraceEvent {
            seq: 1,
            at: Micros(1),
            event: TraceKind::Step(StepEvent::Typed { step: 1, text: max_text }),
        };
        let t = Track::new(vec![one, typed]).unwrap();
        let text = t.to_jsonl().unwrap();
        assert!(text.lines().all(|l| l.len() <= MAX_LINE_BYTES));
        assert_eq!(Track::parse(&text).unwrap(), t);
        assert!(SidecarError::LineTooLong { line: 2 }.to_string().contains("65536"));
        // The limits themselves, without building a 64 KiB line or a 256 MiB file.
        assert_eq!(within_limits(7, MAX_LINE_BYTES, 0), Ok(()));
        assert_eq!(within_limits(7, MAX_LINE_BYTES + 1, 0), Err(SidecarError::LineTooLong { line: 7 }));
        assert_eq!(within_limits(7, 10, MAX_FILE_BYTES - 11), Ok(()));
        assert_eq!(within_limits(7, 10, MAX_FILE_BYTES - 10), Err(SidecarError::FileTooLarge));
        // And the event limit: one event too many is refused before anything else is looked at.
        let many = vec![
            TraceEvent {
                seq: 0,
                at: Micros(0),
                event: TraceKind::Step(StepEvent::Started { step: 0 })
            };
            MAX_EVENTS + 1
        ];
        assert_eq!(Track::new(many), Err(SidecarError::TooManyEvents));
    }

    #[test]
    fn the_file_size_limit_is_checked_before_anything_is_parsed() {
        // One byte over 256 MiB of spaces: refused up front. (Allocated once, only in this test.)
        let big = " ".repeat(MAX_FILE_BYTES + 1);
        assert_eq!(Track::parse(&big), Err(SidecarError::FileTooLarge));
    }

    #[test]
    fn malformed_unknown_and_out_of_order_lines_are_skipped_and_counted() {
        let ok = |s: u64, a: u64| {
            format!(
                "{}\n",
                serde_json::to_string(&ev(s, a, TraceKind::Step(StepEvent::Started { step: 0 }))).unwrap()
            )
        };
        // Invalid values inside an event (a score of 2, a secret name with a space, a negative width), a line that is not
        // JSON, an event kind this build does not know, and an event that goes back in time.
        let bad_score =
            r#"{"seq":1,"at":0,"event":{"kind":"detection","data":{"locate":1,"rect":[0,0,1,1],"class":1,"score":2.0,"chosen":false}}}"#;
        let bad_name = r#"{"seq":2,"at":0,"event":{"kind":"step","data":{"stage":"typed","step":1,"text":{"secret":"a b"}}}}"#;
        let bad_rect =
            r#"{"seq":3,"at":0,"event":{"kind":"detection","data":{"locate":1,"rect":[0,0,-1,1],"class":1,"score":0.5,"chosen":false}}}"#;
        let future = r#"{"seq":4,"at":0,"event":{"kind":"hologram","data":{}}}"#;
        let text = format!(
            "{HEAD}{}{bad_score}\n{bad_name}\n{bad_rect}\n{future}\nnot json\n{}{}",
            ok(5, 10),
            ok(5, 20),
            ok(6, 5)
        );
        let t = Track::parse(&text).unwrap();
        assert_eq!((t.events().len(), t.skipped()), (1, 7)); // seq 5 once; its repeat, bad lines and the backwards event skipped
        assert_eq!(Track::new(sample()).unwrap().skipped(), 0);
    }

    #[test]
    fn unknown_fields_and_header_extras_are_ignored() {
        let head = "{\"format\":\"sighter-hud\",\"version\":1,\"video\":\"run.mp4\",\"future\":[1,2]}\n";
        let line = r#"{"seq":0,"at":3,"note":"extra","event":{"kind":"step","data":{"stage":"started","step":9,"extra":true}}}"#;
        let t = Track::parse(&format!("{head}{line}\n")).unwrap();
        assert_eq!((t.events().len(), t.skipped()), (1, 0));
        assert_eq!(t.events()[0].at, Micros(3));
    }

    #[test]
    fn order_is_enforced_on_write() {
        let mut v = sample();
        v.swap(1, 2);
        assert_eq!(Track::new(v), Err(SidecarError::OutOfOrder { line: 4 }));
        let mut ok = sample();
        ok[3].at = Micros(5); // time goes back
        assert!(Track::new(ok).is_err());
        let line = |s: u64, a: u64| {
            format!(
                "{}\n",
                serde_json::to_string(&ev(s, a, TraceKind::Step(StepEvent::Started { step: 0 }))).unwrap()
            )
        };
        assert!(Track::parse(&format!("{HEAD}{}\n{}", line(1, 5), line(2, 5))).is_ok()); // blank lines skipped, equal times fine
    }

    #[test]
    fn between_selects_a_time_span() {
        let t = Track::new(sample()).unwrap();
        let seqs = |s: &[TraceEvent]| s.iter().map(|e| e.seq).collect::<Vec<_>>();
        assert_eq!(seqs(t.between(Micros(20), Micros(30))), vec![2, 3, 4]);
        assert!(t.between(Micros(61), Micros(99)).is_empty());
        assert!(t.between(Micros(30), Micros(10)).is_empty());
        assert_eq!(t.events().len(), 8);
    }

    fn arb_kind() -> impl Strategy<Value = TraceKind> {
        let layer = prop_oneof![
            Just(Layer::Cache),
            Just(Layer::Template),
            Just(Layer::OcrAnchor),
            Just(Layer::Detector),
            Just(Layer::Vlm)
        ];
        let outcome = prop_oneof![Just(LayerOutcome::Hit), Just(LayerOutcome::Miss), Just(LayerOutcome::Skipped)];
        let status = prop_oneof![
            Just(LocateStatus::Found),
            Just(LocateStatus::Ambiguous),
            Just(LocateStatus::NotFound),
            Just(LocateStatus::NeedsLabelling)
        ];
        prop_oneof![
            any::<u32>().prop_map(|step| TraceKind::Step(StepEvent::Started { step })),
            (any::<u32>(), any::<bool>()).prop_map(|(step, ok)| TraceKind::Step(StepEvent::Finished { step, ok })),
            (any::<u32>(), 0.0f64..5000.0, 0.0f64..5000.0).prop_map(|(step, x, y)| TraceKind::Step(StepEvent::Clicked {
                step,
                at: Pt::new(x, y).unwrap()
            })),
            (any::<u32>(), "[ -~]{0,40}").prop_map(|(step, t)| TraceKind::Step(StepEvent::Typed { step, text: mask(&t, &[]) })),
            (any::<u32>(), "[A-Za-z0-9_.-]{1,20}").prop_map(|(step, n)| TraceKind::Step(StepEvent::Typed {
                step,
                text: HudText::secret(SecretName::new(&n).unwrap())
            })),
            any::<u64>().prop_map(|id| TraceKind::Locate(LocateEvent::Started { id })),
            (any::<u64>(), layer, outcome, 0u64..100_000).prop_map(|(id, layer, outcome, took_us)| TraceKind::Locate(LocateEvent::Layer {
                id,
                layer,
                outcome,
                took_us
            })),
            (any::<u64>(), status).prop_map(|(id, status)| TraceKind::Locate(LocateEvent::Finished { id, status })),
            (any::<u64>(), any::<u32>()).prop_map(|(locate, count)| TraceKind::Hint(HintEvent::AllOutsideScope { locate, count })),
            (
                any::<u64>(),
                0.0f64..5000.0,
                0.0f64..5000.0,
                0.0f64..800.0,
                0.0f64..800.0,
                any::<u32>(),
                0.0f64..=1.0,
                any::<bool>()
            )
                .prop_map(|(locate, x, y, w, h, c, s, chosen)| TraceKind::Detection(DetectionEvent {
                    locate,
                    rect: Rect::new(x, y, w, h).unwrap(),
                    class: ClassId(c),
                    score: Score::new(s).unwrap(),
                    chosen
                })),
        ]
    }

    proptest! {
        /// serialise then parse returns the same track, for any events in order.
        #[test]
        fn any_track_round_trips(kinds in prop::collection::vec(arb_kind(), 0..30), gaps in prop::collection::vec(0u64..1000, 30)) {
            let mut at = 0u64;
            let events: Vec<TraceEvent> = kinds.into_iter().enumerate().map(|(i, k)| { at += gaps[i]; ev(i as u64, at, k) }).collect();
            let t = Track::new(events).unwrap();
            prop_assert_eq!(Track::parse(&t.to_jsonl().unwrap()).unwrap(), t);
        }

        /// The parser never panics, whatever it is fed.
        #[test]
        fn parse_never_panics(s in "\\PC{0,300}") {
            let _ = Track::parse(&s);
            let _ = Track::parse(&format!("{{\"format\":\"sighter-hud\",\"version\":1}}\n{s}"));
        }
    }
}
