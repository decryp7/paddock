//! The ports (dependency inversion): everything with a process, a socket, a model or an OS API sits behind one of these
//! traits, owned by the core. Adapters (the VNC client, the ONNX detector, OCR, Pitwall, the store) implement them later;
//! the pure crates and the tests use fakes. Positions cross the ports as the core's typed geometry: input takes
//! `Pt<Logical>` and results come back as `Rect<Logical>` (the adapter holds the [`Chain`]).
//!
//! Contracts every implementation honours: no panics on odd input, every call bounded by a timeout the adapter owns,
//! failures are the typed errors below (never a bare string), and an infrastructure fault maps to [`Outcome::Error`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::chain::Chain;
use crate::detect::Detection;
use crate::frame::Frame;
use crate::geometry::{Logical, Pt, Rect};
use crate::outcome::Outcome;
use crate::score::RawScore;

/// The stop flag: set by the hotkey thread, checked by the driver before every injected event.
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

/// A source of frames of the target's own pixels, in capture pixels.
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

/// A function key number, always within F1..=F24.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FunctionKey(u8);

impl FunctionKey {
    /// F`n`; `None` outside 1..=24.
    pub fn new(n: u8) -> Option<FunctionKey> {
        (1..=24).contains(&n).then_some(FunctionKey(n))
    }

    /// The number, 1..=24.
    pub fn number(self) -> u8 {
        self.0
    }
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
    /// The Windows / Super / Command key.
    Super,
    /// Insert.
    Insert,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Arrow left.
    Left,
    /// Arrow up.
    Up,
    /// Arrow right.
    Right,
    /// Arrow down.
    Down,
    /// The context-menu key.
    Menu,
    /// A function key.
    Function(FunctionKey),
}

/// Input into the target. Implementations check the [`StopFlag`] before every event and release held keys on stop.
pub trait InputDriver {
    /// Moves the pointer to a point in logical points.
    fn move_to(&mut self, at: Pt<Logical>) -> Result<(), TargetError>;
    /// Clicks at a point in logical points.
    fn click(&mut self, at: Pt<Logical>, button: Button) -> Result<(), TargetError>;
    /// Presses the keys together, then releases them in reverse order.
    fn chord(&mut self, keys: &[Key]) -> Result<(), TargetError>;
    /// Types text key by key.
    fn type_text(&mut self, text: &str) -> Result<(), TargetError>;
}

/// A desktop: frames and input of one machine. The VNC client and the local capture and input helpers implement it.
pub trait Target: FrameSource + InputDriver {
    /// A short name for the trace (never contains a password).
    fn name(&self) -> &str;
}

/// A model or a model runtime could not run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    /// No model is installed for this task.
    #[error("no model is available: {0}")]
    NoModel(String),
    /// The runtime failed on this frame.
    #[error("the model run failed: {0}")]
    Failed(String),
}

impl ModelError {
    /// A missing or failing model is an infrastructure error, never a product failure.
    pub fn outcome(&self) -> Outcome {
        Outcome::Error
    }
}

/// One thing a text-or-label layer found, before calibration: a box in logical points, its text or class, and the
/// layer's own score.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    /// Where, in logical points.
    pub rect: Rect<Logical>,
    /// The text or class found.
    pub label: String,
    /// The layer's own score.
    pub score: RawScore,
}

/// The trained UI-element detector (RF-DETR or D-FINE through `ort`).
pub trait Detector {
    /// Finds elements in `scope` (logical points) of the frame. The adapter cuts tiles with the core's tile planner and
    /// maps boxes back through `chain`.
    fn detect(&self, frame: &Frame, chain: &Chain, scope: &Rect<Logical>) -> Result<Vec<Detection>, ModelError>;
}

/// Text recognition.
pub trait Ocr {
    /// Words in `scope` of the frame, boxes in logical points.
    fn read(&self, frame: &Frame, chain: &Chain, scope: &Rect<Logical>) -> Result<Vec<Finding>, ModelError>;
}

/// A source of labelled boxes for dataset building (OCR, templates, the app's own detector); nothing at run time depends
/// on it.
pub trait LabelSource {
    /// Labelled elements for the screen that `frame` shows.
    fn labels(&self, frame: &Frame, chain: &Chain) -> Result<Vec<Finding>, ModelError>;
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

/// The optional vision-language model (Pitwall). Only a bounded request the user started; never used for bulk work.
pub trait Vlm {
    /// Grounds a description in the frame; `Err(NeedsLocalAi)` when Pitwall is not running.
    fn ground(&self, frame: &Frame, chain: &Chain, description: &str) -> Result<Vec<Finding>, VlmError>;
}

/// The store could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// Nothing is stored under the key.
    #[error("nothing is stored under {0:?}")]
    Missing(String),
    /// The medium failed (disk full, locked, unreadable).
    #[error("the store failed: {0}")]
    Failed(String),
}

/// Where frames, labels and run results live. Whatever reaches a store has already been redacted; a store never sees a
/// secret.
pub trait Store {
    /// Writes `bytes` under `key`, replacing any earlier value atomically.
    fn put(&mut self, key: &str, bytes: &[u8]) -> Result<(), StoreError>;
    /// Reads the bytes under `key`.
    fn get(&self, key: &str) -> Result<Vec<u8>, StoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::InputSpace;
    use crate::geometry::Scale;
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

    impl Fake {
        fn check(&self) -> Result<(), TargetError> {
            if self.stop.is_stopped() {
                Err(TargetError::Stopped)
            } else {
                Ok(())
            }
        }
    }

    impl InputDriver for Fake {
        fn move_to(&mut self, at: Pt<Logical>) -> Result<(), TargetError> {
            self.check()?;
            self.log.push(format!("move {},{}", at.x(), at.y()));
            Ok(())
        }
        fn click(&mut self, at: Pt<Logical>, _button: Button) -> Result<(), TargetError> {
            self.check()?;
            self.log.push(format!("click {},{}", at.x(), at.y()));
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

    impl Target for Fake {
        fn name(&self) -> &str {
            "fake"
        }
    }

    fn fake(frames: usize) -> Fake {
        Fake {
            frames: (0..frames).map(|_| Frame::solid(4, 4, 0).unwrap()).collect(),
            stop: StopFlag::default(),
            log: Vec::new(),
        }
    }

    fn pt(x: f64, y: f64) -> Pt<Logical> {
        Pt::new(x, y).unwrap()
    }

    #[test]
    fn a_stop_blocks_every_input_call_and_ends_the_run_as_stopped() {
        let mut t = fake(1);
        t.click(pt(1.0, 2.0), Button::Left).unwrap();
        t.move_to(pt(3.0, 4.0)).unwrap();
        t.chord(&[Key::Enter]).unwrap();
        t.type_text("ab").unwrap();
        t.stop.stop();
        let errors = [
            t.move_to(pt(0.0, 0.0)).unwrap_err(),
            t.click(pt(0.0, 0.0), Button::Right).unwrap_err(),
            t.chord(&[Key::Ctrl, Key::Char('c')]).unwrap_err(),
            t.type_text("x").unwrap_err(),
        ];
        assert!(errors.iter().all(|e| *e == TargetError::Stopped && e.outcome() == Outcome::Stopped));
        assert_eq!(t.log, vec!["click 1,2", "move 3,4", "chord 1", "type 2"]);
        assert_eq!(t.name(), "fake");
    }

    #[test]
    fn infrastructure_faults_are_errors_never_failures() {
        for e in [
            TargetError::Lost("x".into()),
            TargetError::Timeout(5),
            TargetError::Protocol("p".into()),
        ] {
            assert_eq!(e.outcome(), Outcome::Error, "{e}");
        }
        assert!(TargetError::Timeout(250).to_string().contains("250 ms"));
        assert_eq!(ModelError::NoModel("detector".into()).outcome(), Outcome::Error);
        assert_eq!(ModelError::Failed("oom".into()).outcome(), Outcome::Error);
        let mut t = fake(1);
        assert!(t.grab().is_ok());
        assert!(matches!(t.grab(), Err(TargetError::Lost(_))));
    }

    #[test]
    fn function_keys_stay_within_f1_to_f24() {
        assert!(FunctionKey::new(0).is_none() && FunctionKey::new(25).is_none());
        assert_eq!(FunctionKey::new(12).map(FunctionKey::number), Some(12));
        assert_eq!(FunctionKey::new(24).map(FunctionKey::number), Some(24));
    }

    #[test]
    fn the_stop_flag_is_shared_between_clones() {
        let a = StopFlag::default();
        let b = a.clone();
        assert!(!a.is_stopped());
        b.stop();
        assert!(a.is_stopped());
    }

    struct FakeDetector;
    impl Detector for FakeDetector {
        fn detect(&self, _f: &Frame, _c: &Chain, scope: &Rect<Logical>) -> Result<Vec<Detection>, ModelError> {
            use crate::detect::{ClassId, Score};
            Ok(vec![Detection {
                rect: *scope,
                class: ClassId(1),
                score: Score::new(0.9).map_err(|e| ModelError::Failed(e.to_string()))?,
                tile: 0,
            }])
        }
    }
    struct NoOcr;
    impl Ocr for NoOcr {
        fn read(&self, _f: &Frame, _c: &Chain, _s: &Rect<Logical>) -> Result<Vec<Finding>, ModelError> {
            Err(ModelError::NoModel("ocr".into()))
        }
    }
    struct NoPitwall;
    impl Vlm for NoPitwall {
        fn ground(&self, _f: &Frame, _c: &Chain, _d: &str) -> Result<Vec<Finding>, VlmError> {
            Err(VlmError::NeedsLocalAi)
        }
    }
    struct Labels;
    impl LabelSource for Labels {
        fn labels(&self, _f: &Frame, _c: &Chain) -> Result<Vec<Finding>, ModelError> {
            Ok(vec![Finding {
                rect: Rect::new(0.0, 0.0, 4.0, 4.0).unwrap(),
                label: "ok".into(),
                score: RawScore::new(Layer::Ocr, 0.9).unwrap(),
            }])
        }
    }

    #[test]
    fn the_model_ports_are_object_safe_and_return_typed_geometry() {
        let chain = Chain::local(Scale::unit(), Pt::from_origin(), Pt::from_origin(), InputSpace::WindowsPixels).unwrap();
        let frame = Frame::solid(4, 4, 0).unwrap();
        let scope = Rect::new(0.0, 0.0, 4.0, 4.0).unwrap();
        let det: &dyn Detector = &FakeDetector;
        let found = det.detect(&frame, &chain, &scope).unwrap();
        assert_eq!(found[0].rect, scope);
        let ocr: &dyn Ocr = &NoOcr;
        assert_eq!(ocr.read(&frame, &chain, &scope), Err(ModelError::NoModel("ocr".into())));
        let vlm: &dyn Vlm = &NoPitwall;
        let err = vlm.ground(&frame, &chain, "the OK button").unwrap_err();
        assert_eq!(err, VlmError::NeedsLocalAi);
        assert!(err.to_string().contains("needs local AI"));
        let labels: &dyn LabelSource = &Labels;
        assert_eq!(labels.labels(&frame, &chain).unwrap()[0].label, "ok");
    }

    struct Mem(std::collections::HashMap<String, Vec<u8>>);
    impl Store for Mem {
        fn put(&mut self, key: &str, bytes: &[u8]) -> Result<(), StoreError> {
            self.0.insert(key.to_owned(), bytes.to_vec());
            Ok(())
        }
        fn get(&self, key: &str) -> Result<Vec<u8>, StoreError> {
            self.0.get(key).cloned().ok_or_else(|| StoreError::Missing(key.to_owned()))
        }
    }

    #[test]
    fn a_store_replaces_and_reports_missing_keys() {
        let mut s = Mem(Default::default());
        assert_eq!(s.get("a"), Err(StoreError::Missing("a".into())));
        s.put("a", b"1").unwrap();
        s.put("a", b"2").unwrap();
        assert_eq!(s.get("a").unwrap(), b"2");
        assert!(StoreError::Failed("disk full".into()).to_string().contains("disk full"));
    }
}
