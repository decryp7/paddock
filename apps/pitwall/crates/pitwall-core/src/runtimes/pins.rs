//! Engine version pins, selection and rollback (ADR 0018). Pure: the server supplies the installed builds and writes
//! the file.
//!
//! Which installed build serves a model, first match wins:
//! 1. the model's own pin,
//! 2. the global pin,
//! 3. automatic: the build `inference_build` prefers, else the best for this machine (Metal, CUDA, Vulkan, CPU).
//!
//! A pin is strict: a pin to a build that is not installed, or whose self-test did not pass, is a typed error that says
//! to install it in Engines or remove the pin; it never silently falls back, because someone pinned it on purpose.
//! Automatic selection only ever picks builds that passed their self-test, and so does rollback.

use super::builds::{tag_number, try_order, BuildChoice, BuildId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The file's schema version.
pub const VERSION: u32 = 1;
/// The most per-model pins kept.
pub const MAX_MODEL_PINS: usize = 256;
/// The most builds remembered for rollback.
pub const MAX_HISTORY: usize = 8;
/// The largest file read.
pub const MAX_BYTES: usize = 256 * 1024;

/// An installed build, as selection sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// Its id.
    pub id: BuildId,
    /// The llama.cpp release tag (`b11669`).
    pub tag: String,
    /// Whether it passed its self-test (builds that did not are never chosen automatically or rolled back to).
    pub passed: bool,
}

impl Installed {
    /// The release number, when the tag has one.
    pub fn number(&self) -> Option<u64> {
        tag_number(&self.tag)
    }
}

/// Why a build was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The model's own pin.
    ModelPin(String),
    /// The global pin.
    GlobalPin,
    /// The automatic order.
    Automatic,
}

impl Source {
    /// Plain words for messages and the UI.
    pub fn words(&self) -> String {
        match self {
            Source::ModelPin(m) => format!("pinned for {m}"),
            Source::GlobalPin => "pinned for all models".into(),
            Source::Automatic => "chosen automatically".into(),
        }
    }
}

/// The build that serves, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// The build.
    pub build: BuildId,
    /// Why it was chosen.
    pub source: Source,
}

/// Why no build could be chosen, or a pin was refused. Every message says what to do.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PinError {
    /// The pinned build is not installed.
    #[error("The engine build {build} is {scope} but is not installed. Install it in Engines, or remove the pin.")]
    NotInstalled {
        /// The pinned build.
        build: BuildId,
        /// `pinned for all models`, `pinned for <model>`.
        scope: String,
    },
    /// The pinned build is installed but did not pass its self-test.
    #[error("The engine build {build} is {scope} but failed its self-test. Reinstall it in Engines, or remove the pin.")]
    FailedSelfTest {
        /// The pinned build.
        build: BuildId,
        /// `pinned for all models`, `pinned for <model>`.
        scope: String,
    },
    /// No installed build passed its self-test.
    #[error("No llama.cpp engine is installed. Open Engines and install one.")]
    NoGoodBuild,
    /// Too many per-model pins.
    #[error("There are already {MAX_MODEL_PINS} per-model pins. Remove some in Models first.")]
    TooMany,
}

/// Why rollback is not possible. Every message says what to do.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RollbackError {
    /// No other build was ever in use.
    #[error("There is no earlier engine build to go back to.")]
    NothingToRollBackTo,
    /// The earlier builds are gone or failed their self-test.
    #[error("The earlier engine build(s) {} failed their self-test, so Pitwall will not go back to them. Reinstall one in Engines.", join(.0))]
    OnlyFailedBuilds(Vec<BuildId>),
    /// The earlier builds were uninstalled.
    #[error("The earlier engine builds are no longer installed. Install one in Engines.")]
    NotInstalled,
}

fn join(v: &[BuildId]) -> String {
    v.iter().map(BuildId::as_str).collect::<Vec<_>>().join(", ")
}

/// The pins and the builds that were in use before (newest first).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnginePins {
    global: Option<BuildId>,
    models: BTreeMap<String, BuildId>,
    history: Vec<BuildId>,
}

impl EnginePins {
    /// The global pin.
    pub fn global(&self) -> Option<&BuildId> {
        self.global.as_ref()
    }

    /// The per-model pins, by model id.
    pub fn models(&self) -> &BTreeMap<String, BuildId> {
        &self.models
    }

    /// The pin of one model.
    pub fn model(&self, model: &str) -> Option<&BuildId> {
        self.models.get(model)
    }

    /// The builds that were in use before, newest first.
    pub fn history(&self) -> &[BuildId] {
        &self.history
    }

    /// Pins (or, with `None`, unpins) all models. The build must be installed and good.
    pub fn set_global(&mut self, build: Option<BuildId>, installed: &[Installed]) -> Result<(), PinError> {
        if let Some(b) = &build {
            check(b, installed, &Source::GlobalPin)?;
        }
        self.global = build;
        Ok(())
    }

    /// Pins (or unpins) one model. The build must be installed and good.
    pub fn set_model(&mut self, model: &str, build: Option<BuildId>, installed: &[Installed]) -> Result<(), PinError> {
        match build {
            Some(b) => {
                check(&b, installed, &Source::ModelPin(model.to_string()))?;
                if !self.models.contains_key(model) && self.models.len() >= MAX_MODEL_PINS {
                    return Err(PinError::TooMany);
                }
                self.models.insert(model.to_string(), b);
            }
            None => {
                self.models.remove(model);
            }
        }
        Ok(())
    }

    /// Forgets the pins of models that are no longer installed.
    pub fn retain_models(&mut self, keep: impl Fn(&str) -> bool) {
        self.models.retain(|m, _| keep(m));
    }

    /// Notes that `build` was in use before a change (newest first, once each, bounded).
    pub fn remember(&mut self, build: BuildId) {
        self.history.retain(|b| *b != build);
        self.history.insert(0, build);
        self.history.truncate(MAX_HISTORY);
    }

    /// The pins that point at builds that are not installed or not good, as the errors selection would give. For the
    /// Engines page and the event log.
    pub fn problems(&self, installed: &[Installed]) -> Vec<PinError> {
        self.global
            .iter()
            .map(|b| (b, Source::GlobalPin))
            .chain(self.models.iter().map(|(m, b)| (b, Source::ModelPin(m.clone()))))
            .filter_map(|(b, s)| check(b, installed, &s).err())
            .collect()
    }
}

fn check(build: &BuildId, installed: &[Installed], source: &Source) -> Result<(), PinError> {
    match installed.iter().find(|i| i.id == *build) {
        None => Err(PinError::NotInstalled {
            build: build.clone(),
            scope: source.words(),
        }),
        Some(i) if !i.passed => Err(PinError::FailedSelfTest {
            build: build.clone(),
            scope: source.words(),
        }),
        Some(_) => Ok(()),
    }
}

/// The build that serves `model` (`None`: models in general). See the module comment for the order.
pub fn select(pins: &EnginePins, model: Option<&str>, installed: &[Installed], choice: &BuildChoice) -> Result<Selection, PinError> {
    if let Some((m, b)) = model.and_then(|m| pins.models.get(m).map(|b| (m, b))) {
        let source = Source::ModelPin(m.to_string());
        check(b, installed, &source)?;
        return Ok(Selection { build: b.clone(), source });
    }
    if let Some(b) = &pins.global {
        check(b, installed, &Source::GlobalPin)?;
        return Ok(Selection {
            build: b.clone(),
            source: Source::GlobalPin,
        });
    }
    let good: Vec<String> = installed.iter().filter(|i| i.passed).map(|i| i.id.to_string()).collect();
    try_order(&good, choice)
        .into_iter()
        .next()
        .and_then(|id| BuildId::parse(&id).ok())
        .map(|build| Selection {
            build,
            source: Source::Automatic,
        })
        .ok_or(PinError::NoGoodBuild)
}

/// The build one-click rollback goes to: the most recent earlier build (not `current`) that is installed and passed
/// its self-test. A build that failed its self-test is never chosen.
pub fn rollback_target(history: &[BuildId], current: &BuildId, installed: &[Installed]) -> Result<BuildId, RollbackError> {
    let mut failed = vec![];
    let mut uninstalled = false;
    for b in history.iter().filter(|b| *b != current) {
        match installed.iter().find(|i| i.id == *b) {
            Some(i) if i.passed => return Ok(b.clone()),
            Some(_) => failed.push(b.clone()),
            None => uninstalled = true,
        }
    }
    if !failed.is_empty() {
        Err(RollbackError::OnlyFailedBuilds(failed))
    } else if uninstalled {
        Err(RollbackError::NotInstalled)
    } else {
        Err(RollbackError::NothingToRollBackTo)
    }
}

/// The loaded models (model id, build they run on) that a new selection would move to another build. They keep
/// running on the old one (an engine is never swapped under a busy model); the change applies at their next load.
pub fn moved(running: &[(String, BuildId)], pins: &EnginePins, installed: &[Installed], choice: &BuildChoice) -> Vec<String> {
    running
        .iter()
        .filter(|(m, on)| select(pins, Some(m), installed, choice).is_ok_and(|s| s.build != *on))
        .map(|(m, _)| m.clone())
        .collect()
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    #[serde(default)]
    global: Option<String>,
    #[serde(default)]
    models: BTreeMap<String, String>,
    #[serde(default)]
    history: Vec<String>,
}

/// What loading found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Loaded {
    /// The usable pins.
    pub pins: EnginePins,
    /// Entries that were damaged or over the limit and were left out.
    pub skipped: usize,
    /// The primary file existed but could not be used (the backup, or nothing, was used instead).
    pub damaged: bool,
}

fn parse_file(text: &str) -> Option<Loaded> {
    let f: File = serde_json::from_str(text).ok()?;
    if f.version > VERSION {
        return None;
    }
    let mut skipped = 0;
    let mut out = EnginePins::default();
    if let Some(g) = f.global {
        match BuildId::parse(&g) {
            Ok(b) => out.global = Some(b),
            Err(_) => skipped += 1,
        }
    }
    for (m, b) in f.models {
        match BuildId::parse(&b) {
            Ok(b) if !m.is_empty() && m.len() <= 200 && out.models.len() < MAX_MODEL_PINS => {
                out.models.insert(m, b);
            }
            _ => skipped += 1,
        }
    }
    for b in f.history.into_iter().take(MAX_HISTORY * 4) {
        match BuildId::parse(&b) {
            Ok(b) if out.history.len() < MAX_HISTORY && !out.history.contains(&b) => out.history.push(b),
            _ => skipped += 1,
        }
    }
    Some(Loaded {
        pins: out,
        skipped,
        damaged: false,
    })
}

/// The pins from the primary file, else the backup, else none. Damaged entries are skipped, not fatal; a file from a
/// newer Pitwall is not guessed at.
pub fn load(primary: Option<&str>, backup: Option<&str>) -> Loaded {
    let first = primary.and_then(parse_file);
    let damaged = primary.is_some() && first.is_none();
    let mut loaded = first.or_else(|| backup.and_then(parse_file)).unwrap_or_default();
    loaded.damaged = damaged;
    loaded
}

impl EnginePins {
    /// The file's text.
    pub fn to_json(&self) -> String {
        let f = File {
            version: VERSION,
            global: self.global.as_ref().map(ToString::to_string),
            models: self.models.iter().map(|(m, b)| (m.clone(), b.to_string())).collect(),
            history: self.history.iter().map(ToString::to_string).collect(),
        };
        serde_json::to_string(&f).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> BuildId {
        BuildId::parse(s).unwrap()
    }

    fn inst(id_: &str, tag: &str, passed: bool) -> Installed {
        Installed {
            id: id(id_),
            tag: tag.into(),
            passed,
        }
    }

    fn all() -> Vec<Installed> {
        vec![
            inst("cpu", "b11000", true),
            inst("cuda-13", "b11669", true),
            inst("vulkan", "b11669", true),
            inst("rocm", "b11669", false),
        ]
    }

    #[test]
    fn selection_order_is_model_pin_then_global_pin_then_automatic() {
        let mut p = EnginePins::default();
        let auto = BuildChoice::Auto;
        let s = select(&p, Some("m"), &all(), &auto).unwrap();
        assert_eq!((s.build.as_str(), &s.source), ("cuda-13", &Source::Automatic));
        p.set_global(Some(id("vulkan")), &all()).unwrap();
        let s = select(&p, Some("m"), &all(), &auto).unwrap();
        assert_eq!((s.build.as_str(), &s.source), ("vulkan", &Source::GlobalPin));
        p.set_model("m", Some(id("cpu")), &all()).unwrap();
        let s = select(&p, Some("m"), &all(), &auto).unwrap();
        assert_eq!((s.build.as_str(), &s.source), ("cpu", &Source::ModelPin("m".into())));
        // Another model still follows the global pin; no model follows it too.
        assert_eq!(select(&p, Some("other"), &all(), &auto).unwrap().build.as_str(), "vulkan");
        assert_eq!(select(&p, None, &all(), &auto).unwrap().build.as_str(), "vulkan");
        // Unpinning goes back down the order.
        p.set_model("m", None, &all()).unwrap();
        p.set_global(None, &all()).unwrap();
        assert_eq!(select(&p, Some("m"), &all(), &auto).unwrap().source, Source::Automatic);
        // The preferred build steers the automatic choice only.
        let s = select(&p, None, &all(), &BuildChoice::Fixed("cpu".into())).unwrap();
        assert_eq!(s.build.as_str(), "cpu");
        p.set_global(Some(id("vulkan")), &all()).unwrap();
        assert_eq!(
            select(&p, None, &all(), &BuildChoice::Fixed("cpu".into())).unwrap().build.as_str(),
            "vulkan"
        );
    }

    #[test]
    fn automatic_never_picks_a_failed_build_and_says_when_none_is_left() {
        let only_bad = vec![inst("rocm", "b1", false), inst("cpu", "b1", false)];
        assert_eq!(
            select(&EnginePins::default(), None, &only_bad, &BuildChoice::Auto),
            Err(PinError::NoGoodBuild)
        );
        assert_eq!(
            select(&EnginePins::default(), None, &[], &BuildChoice::Auto),
            Err(PinError::NoGoodBuild)
        );
        let rocm_only = vec![inst("rocm", "b1", true)];
        // Builds that are only used when chosen are not picked automatically.
        assert_eq!(
            select(&EnginePins::default(), None, &rocm_only, &BuildChoice::Auto),
            Err(PinError::NoGoodBuild)
        );
    }

    #[test]
    fn a_pin_to_a_missing_or_failed_build_is_a_typed_error_that_says_what_to_do() {
        let mut p = EnginePins::default();
        let e = p.set_global(Some(id("metal")), &all()).unwrap_err();
        assert!(matches!(&e, PinError::NotInstalled { build, .. } if build.as_str() == "metal"));
        assert!(e.to_string().contains("Install it in Engines"), "{e}");
        let e = p.set_model("m", Some(id("rocm")), &all()).unwrap_err();
        assert!(
            matches!(&e, PinError::FailedSelfTest { .. }) && e.to_string().contains("pinned for m"),
            "{e}"
        );
        assert!(p.global().is_none() && p.models().is_empty());
        // A pin made earlier whose build was uninstalled since fails at selection (strict, no silent fallback).
        p.set_global(Some(id("vulkan")), &all()).unwrap();
        let gone = vec![inst("cpu", "b1", true)];
        let e = select(&p, Some("m"), &gone, &BuildChoice::Auto).unwrap_err();
        assert!(matches!(e, PinError::NotInstalled { .. }) && e.to_string().contains("pinned for all models"));
        assert_eq!(p.problems(&gone).len(), 1);
        assert!(p.problems(&all()).is_empty());
        let mut q = EnginePins::default();
        q.models.insert("m".into(), id("rocm"));
        assert!(matches!(
            select(&q, Some("m"), &all(), &BuildChoice::Auto),
            Err(PinError::FailedSelfTest { .. })
        ));
        assert_eq!(q.problems(&all()).len(), 1);
    }

    #[test]
    fn model_pins_are_bounded_and_can_be_pruned() {
        let mut p = EnginePins::default();
        for i in 0..MAX_MODEL_PINS {
            p.set_model(&format!("m{i}"), Some(id("cpu")), &all()).unwrap();
        }
        assert_eq!(p.set_model("extra", Some(id("cpu")), &all()), Err(PinError::TooMany));
        // Re-pinning an existing model is fine at the limit.
        p.set_model("m0", Some(id("vulkan")), &all()).unwrap();
        assert_eq!(p.model("m0").map(BuildId::as_str), Some("vulkan"));
        p.retain_models(|m| m == "m0");
        assert_eq!(p.models().len(), 1);
    }

    #[test]
    fn rollback_goes_to_the_most_recent_good_build() {
        let mut p = EnginePins::default();
        p.remember(id("cpu"));
        p.remember(id("vulkan"));
        p.remember(id("cpu"));
        assert_eq!(p.history(), &[id("cpu"), id("vulkan")]);
        assert_eq!(rollback_target(p.history(), &id("cuda-13"), &all()), Ok(id("cpu")));
        // The current build is skipped.
        assert_eq!(rollback_target(p.history(), &id("cpu"), &all()), Ok(id("vulkan")));
    }

    #[test]
    fn rollback_refuses_a_build_that_failed_its_self_test() {
        let hist = vec![id("rocm"), id("cpu")];
        // rocm failed, so the older good cpu is the target.
        assert_eq!(rollback_target(&hist, &id("cuda-13"), &all()), Ok(id("cpu")));
        // With only the failed one in history, rollback is refused and says why.
        let e = rollback_target(&[id("rocm")], &id("cuda-13"), &all()).unwrap_err();
        assert_eq!(e, RollbackError::OnlyFailedBuilds(vec![id("rocm")]));
        assert!(e.to_string().contains("rocm") && e.to_string().contains("failed their self-test"));
        assert_eq!(
            rollback_target(&[id("metal")], &id("cpu"), &all()),
            Err(RollbackError::NotInstalled)
        );
        assert_eq!(rollback_target(&[], &id("cpu"), &all()), Err(RollbackError::NothingToRollBackTo));
        assert_eq!(
            rollback_target(&[id("cpu")], &id("cpu"), &all()),
            Err(RollbackError::NothingToRollBackTo)
        );
        assert!(RollbackError::NotInstalled.to_string().contains("Install"));
    }

    #[test]
    fn history_is_bounded() {
        let mut p = EnginePins::default();
        for i in 0..20 {
            p.remember(id(&format!("b{i}")));
        }
        assert_eq!(p.history().len(), MAX_HISTORY);
        assert_eq!(p.history()[0], id("b19"));
    }

    #[test]
    fn a_busy_model_keeps_its_build_and_is_reported_as_moved() {
        let mut p = EnginePins::default();
        p.set_global(Some(id("vulkan")), &all()).unwrap();
        let running = vec![
            ("a".to_string(), id("vulkan")),
            ("b".to_string(), id("cuda-13")),
            ("c".to_string(), id("cpu")),
        ];
        p.set_model("c", Some(id("cpu")), &all()).unwrap();
        assert_eq!(moved(&running, &p, &all(), &BuildChoice::Auto), vec!["b".to_string()]);
        // A model whose selection fails is not reported (the load will say why).
        let gone = vec![inst("cuda-13", "b1", true)];
        assert!(moved(&running, &p, &gone, &BuildChoice::Auto).is_empty());
    }

    #[test]
    fn the_file_round_trips_and_tolerates_damage() {
        let mut p = EnginePins::default();
        p.set_global(Some(id("vulkan")), &all()).unwrap();
        p.set_model("m", Some(id("cpu")), &all()).unwrap();
        p.remember(id("cuda-13"));
        let text = p.to_json();
        assert!(text.contains("\"version\":1"));
        let l = load(Some(&text), None);
        assert_eq!((l.pins.clone(), l.skipped, l.damaged), (p.clone(), 0, false));
        // Damaged primary: the backup is used and the damage is reported.
        let l = load(Some("{not json"), Some(&text));
        assert_eq!((l.pins.clone(), l.damaged), (p.clone(), true));
        // Nothing usable: defaults.
        let l = load(Some("{"), Some("also bad"));
        assert!(l.damaged && l.pins == EnginePins::default());
        assert_eq!(load(None, None), Loaded::default());
        // A newer file is not guessed at.
        assert!(load(Some(r#"{"version":9,"global":"cpu"}"#), None).damaged);
        // Bad entries are dropped one by one.
        let l = load(
            Some(r#"{"version":1,"global":"../x","models":{"a":"cpu","":"cpu","b":"A B"},"history":["cpu","cpu","??"]}"#),
            None,
        );
        assert_eq!(l.skipped, 5);
        assert!(l.pins.global().is_none() && l.pins.models().len() == 1 && l.pins.history() == [id("cpu")]);
        // Over the limits.
        let many: Vec<String> = (0..MAX_MODEL_PINS + 3).map(|i| format!("\"m{i}\":\"cpu\"")).collect();
        let l = load(Some(&format!(r#"{{"version":1,"models":{{{}}}}}"#, many.join(","))), None);
        assert_eq!((l.pins.models().len(), l.skipped), (MAX_MODEL_PINS, 3));
        let hist: Vec<String> = (0..MAX_HISTORY + 3).map(|i| format!("\"b{i}\"")).collect();
        let l = load(Some(&format!(r#"{{"version":1,"history":[{}]}}"#, hist.join(","))), None);
        assert_eq!(l.pins.history().len(), MAX_HISTORY);
        assert_eq!(l.skipped, 3);
    }

    #[test]
    fn labels_and_numbers() {
        assert_eq!(inst("cpu", "b11000", true).number(), Some(11000));
        assert_eq!(inst("cpu", "custom", true).number(), None);
        assert!(Source::Automatic.words().contains("automatically"));
    }
}
