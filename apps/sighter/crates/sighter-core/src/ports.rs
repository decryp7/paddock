//! The ports (dependency inversion): everything with a process, a socket, a model or an OS API sits behind one of these
//! traits, owned by the core. Adapters (the VNC client, the ONNX detector, Pitwall) implement them later; the pure crates
//! and the tests use fakes.
//!
//! Contracts every implementation honours: no panics on odd input, every call bounded by a timeout the adapter owns,
//! failures are the typed errors below (never a bare string), and an infrastructure fault maps to [`Outcome::Error`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::frame::Frame;
use crate::geometry::{Point, Rect};
use crate::outcome::Outcome;
use crate::score::RawScore;

/// The stop flag (design review 1.5): set by the hotkey thread, checked by the driver before every injected event.
#[derive(Clone, Debug, Default)]
pub struct StopFlag(Arc<AtomicBool>);

impl StopFlag {
    /// Requests a stop; irreversible for this flag.
    pub fn stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// True once a stop was requested.
    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The target (VM console, RDP session, local screen) cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TargetError {
    /// The connection dropped or was refused. Infrastructure, not a product failure.
    #[error("the target is not reachable: {0}")]
    Lost(String),
    /// The target did not answer in time.
    #[error("the target did not answer within {0} ms")]
    Timeout(u64),
    /// The server sent something that breaks the protocol.
    #[error("the target sent invalid data: {0}")]
    Protocol(String),
    /// A stop was requested; the call did nothing.
    #[error("stopped by the user")]
    Stopped,
}

impl TargetError {
    /// How a run ends because of this error: a stop is `Stopped`, everything else an infrastructure `Error`.
    pub fn outcome(&self) -> Outcome {
        match self {
            TargetError::Stopped => Outcome::Stopped,
            _ => Outcome::Error,
        }
    }
}

/// A source of frames of the target's own pixels.
pub trait FrameSource {
    /// The latest complete frame.
    fn grab(&mut self) -> Result<Frame, TargetError>;
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// Primary.
    Left,
    /// Context menu.
    Right,
    /// Middle.
    Middle,
}

/// A key to press; chords are sent as a slice and released in reverse order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// A printable character.
    Char(char),
    /// Return.
    Enter,
    /// Escape.
    Escape,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Control.
    Ctrl,
    /// Alt.
    Alt,
    /// Shift.
    Shift,
    /// Delete (with Ctrl and Alt: the secure attention sequence).
    Delete,
    /// A function key, 1..=24.
    Function(u8),
}

/// Input into the target. Implementations check the [`StopFlag`] before every event and release held keys on stop.
pub trait InputDriver {
    /// Moves the pointer.
    fn move_to(&mut self, at: Point) -> Result<(), TargetError>;
    /// Clicks at a point.
    fn click(&mut self, at: Point, button: Button) -> Result<(), TargetError>;
    /// Presses the keys together, then releases them in reverse order.
    fn chord(&mut self, keys: &[Key]) -> Result<(), TargetError>;
    /// Types text key by key.
    fn type_text(&mut self, text: &str) -> Result<(), TargetError>;
}

/// A remote desktop: frames and input of one machine. The VNC client (SG4) and later RDP implement it.
pub trait Target: FrameSource + InputDriver {
    /// A short name for the trace (never contains a password).
    fn name(&self) -> &str;
}

/// The detector or a model could not run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DetectError {
    /// No model is installed for this target.
    #[error("no detection model is available: {0}")]
    NoModel(String),
    /// The runtime failed on this frame.
    #[error("detection failed: {0}")]
    Failed(String),
}

impl DetectError {
    /// A missing or failing model is an infrastructure error, never a product failure.
    pub fn outcome(&self) -> Outcome {
        Outcome::Error
    }
}

/// One thing a layer found, before calibration.
#[derive(Clone, Debug, PartialEq)]
pub struct Detection {
    /// Where, in frame pixels.
    pub rect: Rect,
    /// The class or text found.
    pub label: String,
    /// The layer's own score.
    pub score: RawScore,
}

/// The trained UI-element detector (RF-DETR through `ort` later).
pub trait Detector {
    /// Finds elements in a frame at 1:1 scale; the adapter tiles large frames itself.
    fn detect(&self, frame: &Frame) -> Result<Vec<Detection>, DetectError>;
}

/// An optional accessibility or DOM source that speeds up labelling; nothing at run time depends on it.
pub trait LabelSource {
    /// Named elements for the screen that `frame` shows.
    fn labels(&self, frame: &Frame) -> Result<Vec<Detection>, DetectError>;
}

/// The optional vision-language model (Pitwall). Only a bounded request the user started (rule 46).
pub trait Vlm {
    /// Grounds a description in the frame; `Err(NeedsLocalAi)` when Pitwall is not running.
    fn ground(&self, frame: &Frame, description: &str) -> Result<Vec<Detection>, VlmError>;
}

/// Why the VLM gave no answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VlmError {
    /// Pitwall is not available; the UI shows "Needs local AI".
    #[error("needs local AI: start Pitwall to use this")]
    NeedsLocalAi,
    /// The request failed.
    #[error("the model request failed: {0}")]
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::Layer;

    struct Fake {
        frames: Vec<Frame>,
        stop: StopFlag,
        log: Vec<String>,
    }

    impl FrameSource for Fake {
        fn grab(&mut self) -> Result<Frame, TargetError> {
            self.frames.pop().ok_or_else(|| TargetError::Lost("connection closed".into()))
        }
    }

    impl InputDriver for Fake {
        fn move_to(&mut self, at: Point) -> Result<(), TargetError> {
            self.check()?;
            self.log.push(format!("move {},{}", at.x, at.y));
            Ok(())
        }
        fn click(&mut self, at: Point, _button: Button) -> Result<(), TargetError> {
            self.check()?;
            self.log.push(format!("click {},{}", at.x, at.y));
            Ok(())
        }
        fn chord(&mut self, keys: &[Key]) -> Result<(), TargetError> {
            self.check()?;
            self.log.push(format!("chord {}", keys.len()));
            Ok(())
        }
        fn type_text(&mut self, text: &str) -> Result<(), TargetError> {
            self.check()?;
            self.log.push(format!("type {}", text.len()));
            Ok(())
        }
    }

    impl Fake {
        fn check(&self) -> Result<(), TargetError> {
            if self.stop.is_stopped() {
                Err(TargetError::Stopped)
            } else {
                Ok(())
            }
        }
    }

    impl Target for Fake {
        fn name(&self) -> &str {
            "fake"
        }
    }

    #[test]
    fn a_target_is_usable_through_the_ports_and_stop_blocks_input() {
        let stop = StopFlag::default();
        let mut t = Fake {
            frames: vec![Frame::solid(2, 2, 0).unwrap()],
            stop: stop.clone(),
            log: vec![],
        };
        let target: &mut dyn Target = &mut t;
        assert_eq!(target.name(), "fake");
        assert!(target.grab().is_ok());
        assert_eq!(
            target.grab().unwrap_err().outcome(),
            Outcome::Error,
            "a dropped connection is `error`, not `failed`"
        );
        target.move_to(Point { x: 1, y: 2 }).unwrap();
        target.click(Point { x: 1, y: 2 }, Button::Left).unwrap();
        target.chord(&[Key::Ctrl, Key::Alt, Key::Delete]).unwrap();
        target.type_text("hi").unwrap();
        stop.clone().stop();
        for r in [
            target.move_to(Point { x: 0, y: 0 }),
            target.click(Point { x: 0, y: 0 }, Button::Right),
            target.chord(&[Key::Escape]),
            target.type_text("x"),
        ] {
            assert_eq!(r, Err(TargetError::Stopped));
        }
        assert_eq!(t.log, vec!["move 1,2", "click 1,2", "chord 3", "type 2"]);
        assert_eq!(TargetError::Stopped.outcome(), Outcome::Stopped);
    }

    #[test]
    fn errors_read_as_messages() {
        assert!(TargetError::Timeout(500).to_string().contains("500 ms"));
        assert!(TargetError::Protocol("bad".into()).to_string().contains("bad"));
        assert!(
            VlmError::NeedsLocalAi.to_string().contains("Needs local AI") || VlmError::NeedsLocalAi.to_string().contains("needs local AI")
        );
        assert!(VlmError::Failed("x".into()).to_string().contains('x'));
        assert_eq!(DetectError::NoModel("none".into()).outcome(), Outcome::Error);
        assert!(DetectError::Failed("boom".into()).to_string().contains("boom"));
    }

    struct Det;
    impl Detector for Det {
        fn detect(&self, f: &Frame) -> Result<Vec<Detection>, DetectError> {
            Ok(vec![Detection {
                rect: f.bounds(),
                label: "button".into(),
                score: RawScore::new(Layer::Detector, 0.9).unwrap(),
            }])
        }
    }
    impl LabelSource for Det {
        fn labels(&self, f: &Frame) -> Result<Vec<Detection>, DetectError> {
            self.detect(f)
        }
    }
    impl Vlm for Det {
        fn ground(&self, f: &Frame, _d: &str) -> Result<Vec<Detection>, VlmError> {
            self.detect(f).map_err(|e| VlmError::Failed(e.to_string()))
        }
    }

    #[test]
    fn detector_label_and_vlm_ports_are_object_safe() {
        let f = Frame::solid(4, 4, 0).unwrap();
        let d: &dyn Detector = &Det;
        assert_eq!(d.detect(&f).unwrap()[0].label, "button");
        let l: &dyn LabelSource = &Det;
        assert_eq!(l.labels(&f).unwrap().len(), 1);
        let v: &dyn Vlm = &Det;
        assert_eq!(v.ground(&f, "ok button").unwrap().len(), 1);
    }
}
