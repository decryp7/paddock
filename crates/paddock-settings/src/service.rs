//! The settings service: the registry of sections (the shell's and the app's), their current values, and the one place
//! that validates, applies, persists and announces a change.
//!
//! A section is a [`Configurable`]: its schema (asked once, when it is registered) and how its values take effect. The
//! service validates every change in Rust ([`crate::validate`]), however it arrives (a window, the app's own code, a
//! stored file), applies it, writes the file atomically and tells every listener ([`ChangeSink`]; the Tauri adapter sends
//! it to every window). Values reach the app only as [`ValidValues`] (parse, don't validate).

use crate::file::{self, LoadProblem, SettingsFile};
use crate::schema::{SchemaError, SettingsSection};
use crate::values::{FieldError, ValidValues, sanitize, validate};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// What a [`Configurable::apply`] may fail with: a message for the user.
pub type ApplyError = Box<dyn std::error::Error + Send + Sync>;

/// A settings section and how its values take effect.
pub trait Configurable: Send + Sync {
    /// The schema; asked once, when the section is registered.
    fn section(&self) -> SettingsSection;

    /// Puts validated values into effect: at registration with the stored values, and after every change. An error
    /// refuses the change (nothing is saved). Runs on the caller's thread with the service's write lock held, so it must
    /// not change settings itself; a panic is contained and refuses the change.
    fn apply(&self, values: &ValidValues) -> Result<(), ApplyError>;
}

/// A section whose values are only stored: the app reads them with [`Settings::values`] or [`Settings::decode`].
impl Configurable for SettingsSection {
    fn section(&self) -> SettingsSection {
        self.clone()
    }

    fn apply(&self, _values: &ValidValues) -> Result<(), ApplyError> {
        Ok(())
    }
}

/// A section with a closure that applies its values ([`section`]).
pub struct FnSection<F> {
    section: SettingsSection,
    apply: F,
}

/// A [`Configurable`] from a schema and a closure: `paddock_settings::section(schema, |v| { ...; Ok(()) })`.
pub fn section<F>(section: SettingsSection, apply: F) -> Arc<FnSection<F>>
where
    F: Fn(&ValidValues) -> Result<(), ApplyError> + Send + Sync + 'static,
{
    Arc::new(FnSection { section, apply })
}

impl<F> Configurable for FnSection<F>
where
    F: Fn(&ValidValues) -> Result<(), ApplyError> + Send + Sync,
{
    fn section(&self) -> SettingsSection {
        self.section.clone()
    }

    fn apply(&self, values: &ValidValues) -> Result<(), ApplyError> {
        (self.apply)(values)
    }
}

/// A change that took effect: what every window hears.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SettingsChanged {
    pub section: String,
    /// The section's values now, every field present.
    #[cfg_attr(test, ts(type = "Record<string, SettingValue>"))]
    pub values: Value,
    /// A change made in this run takes effect after a restart.
    pub restart_pending: bool,
}

/// Hears every change that took effect (the Tauri adapter emits it to every window).
pub trait ChangeSink: Send + Sync {
    fn changed(&self, change: &SettingsChanged);
}

/// A section as a settings page shows it: the schema and the current values.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SectionView {
    #[serde(flatten)]
    #[cfg_attr(test, ts(flatten))]
    pub section: SettingsSection,
    /// Every field present.
    #[cfg_attr(test, ts(type = "Record<string, SettingValue>"))]
    pub values: Value,
}

/// Every section with its values: what a Settings window loads.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SettingsOverview {
    /// In registration order.
    pub sections: Vec<SectionView>,
    /// A change made in this run takes effect after a restart.
    pub restart_pending: bool,
    /// Why the settings file was not used at start-up, if it was not.
    pub problem: Option<LoadProblem>,
}

/// What a change did.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SetResult {
    /// False when validation refused the values (see `errors`); nothing changed then.
    pub applied: bool,
    /// The change needs a restart to take effect.
    pub restart_required: bool,
    /// One per refused field (`key` empty: the whole set).
    pub errors: Vec<FieldError>,
    /// The section's values now.
    #[cfg_attr(test, ts(type = "Record<string, SettingValue>"))]
    pub values: Value,
}

/// Why a change could not be made (validation problems are not errors: they come back in [`SetResult::errors`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    /// No section has this id.
    UnknownSection(String),
    /// The section has no field with this key.
    UnknownField { section: String, key: String },
    /// The section's [`Configurable::apply`] refused the values (or panicked); nothing was saved.
    Apply { section: String, message: String },
    /// The values took effect but could not be written to the settings file.
    Save(String),
}

/// The stable codes of [`SettingsError`] on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum SettingsErrorCode {
    UnknownSection,
    UnknownField,
    ApplyFailed,
    SaveFailed,
    /// The app did not set up the settings state (Tauri adapter).
    NotSetUp,
}

impl SettingsError {
    pub fn code(&self) -> SettingsErrorCode {
        match self {
            SettingsError::UnknownSection(_) => SettingsErrorCode::UnknownSection,
            SettingsError::UnknownField { .. } => SettingsErrorCode::UnknownField,
            SettingsError::Apply { .. } => SettingsErrorCode::ApplyFailed,
            SettingsError::Save(_) => SettingsErrorCode::SaveFailed,
        }
    }

    /// Whether trying again may work (a full disk, a locked file).
    pub fn retryable(&self) -> bool {
        matches!(self, SettingsError::Save(_))
    }
}

fn shown(id: &str) -> String {
    id.chars().take(crate::schema::MAX_ID_LEN).collect()
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingsError::UnknownSection(id) => {
                write!(f, "There is no settings section \"{}\".", shown(id))
            }
            SettingsError::UnknownField { section, key } => {
                write!(
                    f,
                    "The settings section \"{}\" has no setting \"{}\".",
                    shown(section),
                    shown(key)
                )
            }
            SettingsError::Apply { section, message } => write!(
                f,
                "The settings of \"{section}\" could not be applied: {message}"
            ),
            SettingsError::Save(why) => {
                write!(f, "The settings took effect but could not be saved: {why}")
            }
        }
    }
}

impl std::error::Error for SettingsError {}

/// On the wire: `{ code, message, retryable }`, like every shell command error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SettingsCommandError {
    pub code: SettingsErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl From<&SettingsError> for SettingsCommandError {
    fn from(e: &SettingsError) -> Self {
        SettingsCommandError {
            code: e.code(),
            message: e.to_string(),
            retryable: e.retryable(),
        }
    }
}

impl Serialize for SettingsError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SettingsCommandError::from(self).serialize(s)
    }
}

/// Why a section was not registered: a bug in the app or the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    Schema(SchemaError),
    /// A section with this id is already registered.
    Duplicate(String),
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegisterError::Schema(e) => write!(f, "invalid settings schema: {e}"),
            RegisterError::Duplicate(id) => {
                write!(f, "a settings section \"{id}\" is already registered")
            }
        }
    }
}

impl std::error::Error for RegisterError {}

struct Registered {
    section: SettingsSection,
    configurable: Arc<dyn Configurable>,
    values: ValidValues,
}

#[derive(Default)]
struct State {
    file: SettingsFile,
    sections: Vec<Registered>,
    restart_pending: bool,
}

impl State {
    fn find(&self, id: &str) -> Result<usize, SettingsError> {
        self.sections
            .iter()
            .position(|r| r.section.id == id)
            .ok_or_else(|| SettingsError::UnknownSection(id.to_string()))
    }
}

/// The app's settings: managed once per app (the Tauri adapter puts it in the app state).
pub struct Settings {
    path: Option<PathBuf>,
    problem: Option<LoadProblem>,
    state: RwLock<State>,
    /// One change at a time: validate, apply, persist and announce happen in order.
    writer: Mutex<()>,
    sink: RwLock<Option<Arc<dyn ChangeSink>>>,
}

fn read<T>(l: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn write<T>(l: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn lock<T>(l: &Mutex<T>) -> MutexGuard<'_, T> {
    l.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Settings {
    /// The settings kept in the file at `path` (usually `<config dir>/settings.json`). A missing or damaged file gives
    /// defaults, and [`Settings::problem`] says what happened (logged as a warning too).
    pub fn open(path: impl Into<PathBuf>) -> Settings {
        let path = path.into();
        let loaded = file::load(&path);
        if let Some(p) = &loaded.problem {
            log::warn!(target: "paddock_settings", "{}", p.message);
        }
        Settings::with(Some(path), loaded.file, loaded.problem)
    }

    /// Settings that live in memory only (tests, a portable or safe mode that must not write).
    pub fn in_memory() -> Settings {
        Settings::with(None, SettingsFile::default(), None)
    }

    fn with(path: Option<PathBuf>, file: SettingsFile, problem: Option<LoadProblem>) -> Settings {
        Settings {
            path,
            problem,
            state: RwLock::new(State {
                file,
                ..State::default()
            }),
            writer: Mutex::new(()),
            sink: RwLock::new(None),
        }
    }

    /// The settings file, if there is one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Why the file was not used at start-up: report it once (the Tauri adapter posts one notification).
    pub fn problem(&self) -> Option<&LoadProblem> {
        self.problem.as_ref()
    }

    /// Sets who hears the changes (one sink; a second call replaces it).
    pub fn set_sink(&self, sink: Arc<dyn ChangeSink>) {
        *write(&self.sink) = Some(sink);
    }

    /// Registers a section: checks its schema, puts its stored values (each invalid one replaced by its default and
    /// logged) into effect and returns them. A failing `apply` is logged and the section stays registered with those
    /// values, so a bad stored value never stops the app.
    pub fn register(
        &self,
        configurable: Arc<dyn Configurable>,
    ) -> Result<ValidValues, RegisterError> {
        let section = configurable.section();
        section.check().map_err(RegisterError::Schema)?;
        let _one = lock(&self.writer);
        let stored = {
            let state = read(&self.state);
            if state.sections.iter().any(|r| r.section.id == section.id) {
                return Err(RegisterError::Duplicate(section.id));
            }
            state
                .file
                .sections
                .get(&section.id)
                .cloned()
                .unwrap_or(Value::Null)
        };
        let (values, errors) = sanitize(&section, &stored);
        for e in &errors {
            log::warn!(target: "paddock_settings", "settings \"{}\": the stored value of \"{}\" is replaced by its default: {}", section.id, e.key, e.message);
        }
        if let Err(message) = contained(&*configurable, &values) {
            log::warn!(target: "paddock_settings", "settings \"{}\": the stored values could not be applied: {message}", section.id);
        }
        write(&self.state).sections.push(Registered {
            section,
            configurable,
            values: values.clone(),
        });
        Ok(values)
    }

    /// The registered schemas, in registration order.
    pub fn sections(&self) -> Vec<SettingsSection> {
        read(&self.state)
            .sections
            .iter()
            .map(|r| r.section.clone())
            .collect()
    }

    /// Every section with its current values.
    pub fn overview(&self) -> SettingsOverview {
        let state = read(&self.state);
        SettingsOverview {
            sections: state
                .sections
                .iter()
                .map(|r| SectionView {
                    section: r.section.clone(),
                    values: r.values.to_json(),
                })
                .collect(),
            restart_pending: state.restart_pending,
            problem: self.problem.clone(),
        }
    }

    /// A change made in this run takes effect after a restart.
    pub fn restart_pending(&self) -> bool {
        read(&self.state).restart_pending
    }

    /// The current values of section `id`.
    pub fn values(&self, id: &str) -> Option<ValidValues> {
        let state = read(&self.state);
        state
            .find(id)
            .ok()
            .map(|i| state.sections[i].values.clone())
    }

    /// The current values of section `id` as the app's type (see [`ValidValues::decode`]).
    pub fn decode<T: DeserializeOwned>(&self, id: &str) -> Result<T, SettingsError> {
        let values = self
            .values(id)
            .ok_or_else(|| SettingsError::UnknownSection(id.to_string()))?;
        values.decode().map_err(|e| SettingsError::Apply {
            section: id.to_string(),
            message: e.to_string(),
        })
    }

    /// Changes section `id`: `values` (a JSON object) is laid over the current values (a key left out keeps its value,
    /// `null` puts it back to the default), validated, applied, saved and announced. Refused values come back in
    /// [`SetResult::errors`] and change nothing; the same values again change nothing and announce nothing.
    pub fn set(&self, id: &str, values: &Value) -> Result<SetResult, SettingsError> {
        let _one = lock(&self.writer);
        let (index, section, configurable, before) = {
            let state = read(&self.state);
            let i = state.find(id)?;
            let r = &state.sections[i];
            (
                i,
                r.section.clone(),
                r.configurable.clone(),
                r.values.clone(),
            )
        };
        let merged = match values {
            Value::Object(changes) => {
                let mut m: Map<String, Value> =
                    before.to_json().as_object().cloned().unwrap_or_default();
                for (k, v) in changes {
                    m.insert(k.clone(), v.clone());
                }
                Value::Object(m)
            }
            other => other.clone(),
        };
        let valid = match validate(&section, &merged) {
            Ok(v) => v,
            Err(errors) => {
                return Ok(SetResult {
                    applied: false,
                    restart_required: false,
                    errors,
                    values: before.to_json(),
                });
            }
        };
        if valid == before {
            return Ok(SetResult {
                applied: true,
                restart_required: false,
                errors: Vec::new(),
                values: valid.to_json(),
            });
        }
        contained(&*configurable, &valid).map_err(|message| SettingsError::Apply {
            section: id.to_string(),
            message,
        })?;
        let restart_required = section
            .fields
            .iter()
            .any(|f| f.restart_required && before.get(&f.key) != valid.get(&f.key));
        let (next_file, restart_pending) = {
            let mut state = write(&self.state);
            state.sections[index].values = valid.clone();
            state.file.sections.insert(id.to_string(), valid.to_json());
            state.restart_pending |= restart_required;
            (state.file.clone(), state.restart_pending)
        };
        let saved = match &self.path {
            Some(path) => {
                file::save(path, &next_file).map_err(|e| SettingsError::Save(e.to_string()))
            }
            None => Ok(()),
        };
        let change = SettingsChanged {
            section: id.to_string(),
            values: valid.to_json(),
            restart_pending,
        };
        let sink = read(&self.sink).clone();
        if let Some(sink) = sink {
            sink.changed(&change);
        }
        log::info!(target: "paddock_settings", "settings \"{id}\" changed (restart required: {restart_required})");
        saved.map(|()| SetResult {
            applied: true,
            restart_required,
            errors: Vec::new(),
            values: change.values,
        })
    }

    /// Puts section `id` back to its defaults, or only its field `key`.
    pub fn reset(&self, id: &str, key: Option<&str>) -> Result<SetResult, SettingsError> {
        let section = {
            let state = read(&self.state);
            state.sections[state.find(id)?].section.clone()
        };
        let changes: Map<String, Value> = match key {
            Some(k) if section.find(k).is_some() => {
                [(k.to_string(), Value::Null)].into_iter().collect()
            }
            Some(k) => {
                return Err(SettingsError::UnknownField {
                    section: id.to_string(),
                    key: k.to_string(),
                });
            }
            None => section
                .fields
                .iter()
                .map(|f| (f.key.clone(), Value::Null))
                .collect(),
        };
        self.set(id, &Value::Object(changes))
    }
}

/// Runs `apply` with a panic contained; the error is the message for the user.
fn contained(configurable: &dyn Configurable, values: &ValidValues) -> Result<(), String> {
    match catch_unwind(AssertUnwindSafe(|| configurable.apply(values))) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("the section's code failed unexpectedly".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{FieldKind, SectionOwner, SettingField};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn schema() -> SettingsSection {
        SettingsSection::new("sd.general", "General", SectionOwner::App)
            .field(SettingField::new(
                "greeting",
                "Greeting",
                FieldKind::Text {
                    max_len: 20,
                    pattern: None,
                },
                "Hello",
            ))
            .field(
                SettingField::new(
                    "count",
                    "Count",
                    FieldKind::Int {
                        min: 0,
                        max: 9,
                        step: 1,
                        unit: None,
                    },
                    3,
                )
                .needs_restart(),
            )
    }

    #[derive(Default)]
    struct Recorder(Mutex<Vec<SettingsChanged>>);

    impl ChangeSink for Recorder {
        fn changed(&self, change: &SettingsChanged) {
            lock(&self.0).push(change.clone());
        }
    }

    #[test]
    fn a_change_is_validated_applied_saved_and_announced_once() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("settings.json");
        let settings = Settings::open(&path);
        assert_eq!(settings.path(), Some(path.as_path()));
        assert!(settings.problem().is_none());
        let applied = Arc::new(Mutex::new(Vec::new()));
        let seen = applied.clone();
        let values = settings
            .register(section(schema(), move |v| {
                lock(&seen).push(v.to_json());
                Ok(())
            }))
            .unwrap();
        assert_eq!(values.to_json(), json!({ "greeting": "Hello", "count": 3 }));
        let sink = Arc::new(Recorder::default());
        settings.set_sink(sink.clone());

        let r = settings
            .set("sd.general", &json!({ "greeting": "Hi" }))
            .unwrap();
        assert_eq!((r.applied, r.restart_required), (true, false));
        assert_eq!(
            r.values,
            json!({ "greeting": "Hi", "count": 3 }),
            "a key left out keeps its value"
        );
        let r = settings
            .set("sd.general", &json!({ "greeting": "Hi" }))
            .unwrap();
        assert!(r.applied, "the same values again");
        assert_eq!(
            lock(&sink.0).len(),
            1,
            "the same values again announce nothing"
        );

        let refused = settings
            .set("sd.general", &json!({ "count": 99, "greeting": 5 }))
            .unwrap();
        assert!(!refused.applied);
        assert_eq!(refused.errors.len(), 2);
        assert_eq!(
            refused.values,
            json!({ "greeting": "Hi", "count": 3 }),
            "nothing changed"
        );

        let r = settings.set("sd.general", &json!({ "count": 5 })).unwrap();
        assert!(r.restart_required);
        assert!(settings.restart_pending() && settings.overview().restart_pending);
        let events = lock(&sink.0).clone();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[1],
            SettingsChanged {
                section: "sd.general".into(),
                values: json!({ "greeting": "Hi", "count": 5 }),
                restart_pending: true
            }
        );
        assert_eq!(lock(&applied).len(), 3, "registration and two changes");

        // The file holds the values; a new run reads them back.
        let again = Settings::open(&path);
        assert_eq!(
            again.register(Arc::new(schema())).unwrap().to_json(),
            json!({ "greeting": "Hi", "count": 5 })
        );
        assert!(!again.restart_pending());
    }

    #[test]
    fn reset_puts_one_field_or_the_whole_section_back() {
        let settings = Settings::in_memory();
        settings.register(Arc::new(schema())).unwrap();
        settings
            .set("sd.general", &json!({ "greeting": "Hi", "count": 1 }))
            .unwrap();
        let r = settings.reset("sd.general", Some("greeting")).unwrap();
        assert_eq!(r.values, json!({ "greeting": "Hello", "count": 1 }));
        let r = settings.reset("sd.general", None).unwrap();
        assert_eq!(r.values, json!({ "greeting": "Hello", "count": 3 }));
        assert_eq!(
            settings.reset("sd.general", Some("nope")),
            Err(SettingsError::UnknownField {
                section: "sd.general".into(),
                key: "nope".into()
            })
        );
        assert_eq!(
            settings.reset("sd.none", None),
            Err(SettingsError::UnknownSection("sd.none".into()))
        );
        assert_eq!(
            settings.set("sd.none", &json!({})),
            Err(SettingsError::UnknownSection("sd.none".into()))
        );
        let r = settings.set("sd.general", &json!("text")).unwrap();
        assert!(!r.applied && r.errors[0].key.is_empty(), "not an object");
    }

    #[test]
    fn stored_values_are_sanitized_and_unknown_sections_kept() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{ "version": 1, "sections": {
            "sd.general": { "greeting": 42, "count": 4, "gone": true },
            "sd.later": { "x": 1 } } }"#,
        )
        .unwrap();
        let settings = Settings::open(&path);
        let values = settings.register(Arc::new(schema())).unwrap();
        assert_eq!(
            values.to_json(),
            json!({ "greeting": "Hello", "count": 4 }),
            "the bad value is the default"
        );
        settings.set("sd.general", &json!({ "count": 2 })).unwrap();
        let written: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            written["sections"]["sd.later"],
            json!({ "x": 1 }),
            "a section nobody registered is kept"
        );
        assert_eq!(
            written["sections"]["sd.general"],
            json!({ "greeting": "Hello", "count": 2 })
        );
        assert_eq!(written["version"], 1);
    }

    #[test]
    fn a_corrupt_file_gives_defaults_and_one_problem() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("settings.json");
        std::fs::write(&path, "{ not json").unwrap();
        let settings = Settings::open(&path);
        let values = settings.register(Arc::new(schema())).unwrap();
        assert_eq!(values.to_json(), json!({ "greeting": "Hello", "count": 3 }));
        let overview = settings.overview();
        assert_eq!(
            overview.problem.as_ref().map(|p| p.kind),
            Some(crate::LoadProblemKind::Damaged)
        );
        assert_eq!(overview.problem.as_ref(), settings.problem());
        // The next save writes a good file; the damaged one stays aside.
        settings.set("sd.general", &json!({ "count": 1 })).unwrap();
        assert!(t.path().join("settings.json.corrupt").exists());
        assert!(Settings::open(&path).problem().is_none());
    }

    #[test]
    fn registration_refuses_bad_schemas_and_duplicates() {
        let settings = Settings::in_memory();
        let bad = SettingsSection::new("bad id", "T", SectionOwner::App);
        assert!(matches!(
            settings.register(Arc::new(bad)),
            Err(RegisterError::Schema(_))
        ));
        settings.register(Arc::new(schema())).unwrap();
        let again = settings.register(Arc::new(schema())).unwrap_err();
        assert_eq!(again, RegisterError::Duplicate("sd.general".into()));
        assert!(again.to_string().contains("already registered"));
        assert!(
            RegisterError::Schema(SchemaError("x".into()))
                .to_string()
                .contains("invalid settings schema: x")
        );
        assert_eq!(settings.sections().len(), 1);
        assert!(settings.values("sd.none").is_none());
    }

    #[test]
    fn apply_errors_and_panics_refuse_the_change_and_save_nothing() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("settings.json");
        let settings = Settings::open(&path);
        let calls = Arc::new(AtomicUsize::new(0));
        let n = calls.clone();
        settings
            .register(section(schema(), move |v| {
                match (n.fetch_add(1, Ordering::SeqCst), v.int("count")) {
                    (0, _) => Err("not at start".into()),
                    (_, Some(7)) => panic!("boom"),
                    (_, Some(8)) => Err("eight is not allowed".into()),
                    _ => Ok(()),
                }
            }))
            .unwrap();
        assert_eq!(
            settings.values("sd.general").unwrap().int("count"),
            Some(3),
            "a failing apply at start keeps the section"
        );
        let e = settings
            .set("sd.general", &json!({ "count": 8 }))
            .unwrap_err();
        assert_eq!(
            e,
            SettingsError::Apply {
                section: "sd.general".into(),
                message: "eight is not allowed".into()
            }
        );
        assert_eq!(e.code(), SettingsErrorCode::ApplyFailed);
        assert!(!e.retryable());
        let e = settings
            .set("sd.general", &json!({ "count": 7 }))
            .unwrap_err();
        assert!(e.to_string().contains("failed unexpectedly"), "{e}");
        assert_eq!(settings.values("sd.general").unwrap().int("count"), Some(3));
        assert!(!path.exists(), "nothing was saved");
        settings.set("sd.general", &json!({ "count": 1 })).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn a_failed_save_still_applies_and_says_so() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("settings.json");
        std::fs::create_dir(&path).unwrap(); // a folder where the file should be
        let settings = Settings::open(&path);
        assert_eq!(
            settings.problem().map(|p| p.kind),
            Some(crate::LoadProblemKind::Unreadable)
        );
        settings.register(Arc::new(schema())).unwrap();
        let sink = Arc::new(Recorder::default());
        settings.set_sink(sink.clone());
        let e = settings
            .set("sd.general", &json!({ "count": 1 }))
            .unwrap_err();
        assert!(matches!(e, SettingsError::Save(_)));
        assert!(e.retryable() && e.to_string().contains("could not be saved"));
        assert_eq!(
            settings.values("sd.general").unwrap().int("count"),
            Some(1),
            "in effect"
        );
        assert_eq!(lock(&sink.0).len(), 1, "and announced");
    }

    #[test]
    fn typed_values_and_wire_errors() {
        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct General {
            greeting: String,
            count: u32,
        }
        let settings = Settings::in_memory();
        settings.register(Arc::new(schema())).unwrap();
        assert_eq!(
            settings.decode::<General>("sd.general").unwrap(),
            General {
                greeting: "Hello".into(),
                count: 3
            }
        );
        assert!(matches!(
            settings.decode::<General>("sd.none"),
            Err(SettingsError::UnknownSection(_))
        ));
        assert!(matches!(
            settings.decode::<Vec<u8>>("sd.general"),
            Err(SettingsError::Apply { .. })
        ));
        let wire = serde_json::to_value(SettingsError::UnknownSection("x".repeat(300))).unwrap();
        assert_eq!(wire["code"], "unknownSection");
        assert_eq!(wire["retryable"], false);
        assert!(
            wire["message"].as_str().unwrap().len() < 200,
            "a long id is shortened"
        );
        let field = SettingsError::UnknownField {
            section: "a".into(),
            key: "b".into(),
        };
        assert_eq!(
            (field.code(), field.to_string()),
            (
                SettingsErrorCode::UnknownField,
                "The settings section \"a\" has no setting \"b\".".into()
            )
        );
        assert_eq!(
            SettingsError::Save("disk full".into()).code(),
            SettingsErrorCode::SaveFailed
        );
    }

    #[test]
    fn a_poisoned_lock_recovers() {
        let settings = Arc::new(Settings::in_memory());
        settings.register(Arc::new(schema())).unwrap();
        let s = settings.clone();
        let _ = std::thread::spawn(move || {
            let _held = s.state.write().unwrap();
            panic!("poison");
        })
        .join();
        assert!(settings.state.is_poisoned());
        assert_eq!(settings.values("sd.general").unwrap().int("count"), Some(3));
        settings.set("sd.general", &json!({ "count": 2 })).unwrap();
    }
}
