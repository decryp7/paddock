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
/// Most events in one track.
pub const MAX_EVENTS: usize = 2_000_000;
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
    /// A version this build does not know.
    #[error("track version {found} is not supported (this build reads version {VERSION}); update Sighter to read it")]
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
}

/// The sidecar file name for a video file name: `run.mp4` becomes `run.mp4.hud.jsonl`.
pub fn sidecar_name(video_file_name: &str) -> String {
    format!("{video_file_name}{SUFFIX}")
}

fn ordered(prev: Option<&TraceEvent>, e: &TraceEvent) -> bool {
    prev.is_none_or(|p| e.seq > p.seq && e.at >= p.at)
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
        Ok(Self { events })
    }
    /// The events.
    pub fn events(&self) -> &[TraceEvent] {
        &self.events
    }
    /// Writes the track as JSON Lines.
    pub fn to_jsonl(&self) -> String {
        let mut out = String::new();
        let header = Header {
            format: FORMAT.to_owned(),
            version: VERSION,
        };
        // Serialising these plain types cannot fail; an empty line would be rejected on read rather than panic.
        let _ = writeln!(out, "{}", serde_json::to_string(&header).unwrap_or_default());
        for e in &self.events {
            let _ = writeln!(out, "{}", serde_json::to_string(e).unwrap_or_default());
        }
        out
    }
    /// Parses a track.
    pub fn parse(text: &str) -> Result<Self, SidecarError> {
        let mut lines = text.lines().enumerate().map(|(i, l)| (i + 1, l));
        let (_, first) = lines.next().ok_or(SidecarError::MissingHeader)?;
        if first.len() > MAX_LINE_BYTES {
            return Err(SidecarError::LineTooLong { line: 1 });
        }
        let header: Header = serde_json::from_str(first).map_err(|_| SidecarError::BadHeader)?;
        if header.format != FORMAT {
            return Err(SidecarError::BadHeader);
        }
        if header.version != VERSION {
            return Err(SidecarError::UnsupportedVersion { found: header.version });
        }
        let mut events: Vec<TraceEvent> = Vec::new();
        for (line, l) in lines {
            if l.len() > MAX_LINE_BYTES {
                return Err(SidecarError::LineTooLong { line });
            }
            if l.trim().is_empty() {
                continue;
            }
            if events.len() == MAX_EVENTS {
                return Err(SidecarError::TooManyEvents);
            }
            let e: TraceEvent = serde_json::from_str(l).map_err(|e| SidecarError::BadEvent {
                line,
                reason: e.to_string(),
            })?;
            if !ordered(events.last(), &e) {
                return Err(SidecarError::OutOfOrder { line });
            }
            events.push(e);
        }
        Ok(Self { events })
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
    use super::super::mask::{HudText, SecretName};
    use super::super::trace::{DetectionEvent, Layer, LayerOutcome, LocateEvent, LocateStatus, StepEvent, TraceKind};
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
        let text = t.to_jsonl();
        assert!(text.starts_with("{\"format\":\"sighter-hud\",\"version\":1}\n"));
        assert_eq!(text.lines().count(), 1 + 8);
        assert_eq!(Track::parse(&text).unwrap(), t);
        assert!(!text.contains("hunter2") && text.contains("DB_PASSWORD"));
        assert_eq!(sidecar_name("run.mp4"), "run.mp4.hud.jsonl");
    }

    #[test]
    fn bad_input_is_a_typed_error_never_a_panic() {
        assert_eq!(Track::parse(""), Err(SidecarError::MissingHeader));
        assert_eq!(Track::parse("nope"), Err(SidecarError::BadHeader));
        assert_eq!(Track::parse(r#"{"format":"other","version":1}"#), Err(SidecarError::BadHeader));
        assert_eq!(
            Track::parse(r#"{"format":"sighter-hud","version":2}"#),
            Err(SidecarError::UnsupportedVersion { found: 2 })
        );
        let head = "{\"format\":\"sighter-hud\",\"version\":1}\n";
        assert!(matches!(
            Track::parse(&format!("{head}{{\"seq\":0}}\n")),
            Err(SidecarError::BadEvent { line: 2, .. })
        ));
        assert_eq!(
            Track::parse(&format!("{head}{}\n", "x".repeat(MAX_LINE_BYTES + 1))),
            Err(SidecarError::LineTooLong { line: 2 })
        );
        assert_eq!(
            Track::parse(&"x".repeat(MAX_LINE_BYTES + 1)),
            Err(SidecarError::LineTooLong { line: 1 })
        );
        // Invalid values inside an event are refused at the edge: a score of 2 and a secret name with a space.
        let bad_score =
            r#"{"seq":0,"at":0,"event":{"kind":"detection","data":{"locate":1,"rect":[0,0,1,1],"class":1,"score":2.0,"chosen":false}}}"#;
        assert!(matches!(
            Track::parse(&format!("{head}{bad_score}\n")),
            Err(SidecarError::BadEvent { .. })
        ));
        let bad_name = r#"{"seq":0,"at":0,"event":{"kind":"step","data":{"stage":"typed","step":1,"text":{"secret":"a b"}}}}"#;
        assert!(matches!(
            Track::parse(&format!("{head}{bad_name}\n")),
            Err(SidecarError::BadEvent { .. })
        ));
        let bad_rect =
            r#"{"seq":0,"at":0,"event":{"kind":"detection","data":{"locate":1,"rect":[0,0,-1,1],"class":1,"score":0.5,"chosen":false}}}"#;
        assert!(matches!(
            Track::parse(&format!("{head}{bad_rect}\n")),
            Err(SidecarError::BadEvent { .. })
        ));
        assert!(SidecarError::UnsupportedVersion { found: 9 }.to_string().contains("update Sighter"));
    }

    #[test]
    fn order_is_enforced_on_write_and_read() {
        let mut v = sample();
        v.swap(1, 2);
        assert_eq!(Track::new(v.clone()), Err(SidecarError::OutOfOrder { line: 4 }));
        let mut ok = sample();
        ok[3].at = Micros(5); // time goes back
        assert!(Track::new(ok).is_err());
        let head = "{\"format\":\"sighter-hud\",\"version\":1}\n";
        let line = |s: u64, a: u64| {
            format!(
                "{}\n",
                serde_json::to_string(&ev(s, a, TraceKind::Step(StepEvent::Started { step: 0 }))).unwrap()
            )
        };
        assert_eq!(
            Track::parse(&format!("{head}{}{}", line(1, 5), line(1, 6))),
            Err(SidecarError::OutOfOrder { line: 3 })
        );
        assert!(Track::parse(&format!("{head}{}\n{}", line(1, 5), line(2, 5))).is_ok()); // blank lines skipped, equal times fine
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
        prop_oneof![
            any::<u32>().prop_map(|step| TraceKind::Step(StepEvent::Started { step })),
            (any::<u32>(), "[ -~]{0,40}").prop_map(|(step, t)| TraceKind::Step(StepEvent::Typed {
                step,
                text: HudText::plain(&t)
            })),
            (any::<u32>(), "[A-Za-z0-9_.-]{1,20}").prop_map(|(step, n)| TraceKind::Step(StepEvent::Typed {
                step,
                text: HudText::secret(SecretName::new(&n).unwrap())
            })),
            (any::<u64>(), 0u64..100_000).prop_map(|(id, took_us)| TraceKind::Locate(LocateEvent::Layer {
                id,
                layer: Layer::Template,
                outcome: LayerOutcome::Miss,
                took_us
            })),
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
            prop_assert_eq!(Track::parse(&t.to_jsonl()).unwrap(), t);
        }

        /// The parser never panics, whatever it is fed.
        #[test]
        fn parse_never_panics(s in "\\PC{0,300}") {
            let _ = Track::parse(&s);
            let _ = Track::parse(&format!("{{\"format\":\"sighter-hud\",\"version\":1}}\n{s}"));
        }
    }
}
