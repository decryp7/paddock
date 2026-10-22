//! Pitwall's settings (`settings.toml` in the data folder): versioned, every key optional with a default, validated
//! once when loaded or saved, and tolerant of damage (the backup, then the defaults; ADR 0002). The UI edits the
//! [`UiSettings`] part; model preferences and the engine choice are changed through their own commands.

use crate::ids::ModelId;
use crate::memory::{Policy, Preset};
use crate::slots::Slots;
use crate::time::SECOND;
use crate::update_state::UpdateCheckSettings;
use crate::{GIB, MIB};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The settings file's schema version. A breaking change bumps it and adds a migration with a test.
pub const SCHEMA_VERSION: u32 = 1;
/// The default API port (Ollama's is 11434; Pitwall uses its own so both can run).
pub const DEFAULT_PORT: u16 = 11_435;
/// Largest settings file read.
pub const MAX_SETTINGS_BYTES: usize = 1024 * 1024;

/// The UI's theme choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    /// Follow the system.
    #[default]
    System,
    /// Dark.
    Dark,
    /// Light.
    Light,
}

/// General settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneralSettings {
    /// The UI theme.
    pub theme: Theme,
    /// Keep the server running when the window and tray app close.
    pub keep_serving_when_closed: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        GeneralSettings {
            theme: Theme::System,
            keep_serving_when_closed: true,
        }
    }
}

/// API server settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerSettings {
    /// The loopback port.
    pub port: u16,
    /// Largest request body.
    pub max_request_bytes: u64,
    /// Longest a request may take (queueing, loading and generating).
    pub request_timeout_secs: u64,
    /// Requests running at once.
    pub max_concurrent: u32,
    /// Accept requests without a key from this machine (for Ollama-style tools).
    pub allow_keyless_local: bool,
}

impl Default for ServerSettings {
    fn default() -> Self {
        ServerSettings {
            port: DEFAULT_PORT,
            max_request_bytes: 32 * MIB,
            request_timeout_secs: 600,
            max_concurrent: 4,
            allow_keyless_local: false,
        }
    }
}

/// Memory settings (ADR 0006).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MemorySettings {
    /// The preset these values came from (`custom` once edited).
    pub preset: Preset,
    /// Unload an idle model after this long.
    pub idle_unload_secs: u64,
    /// Unload everything after this long without requests.
    pub deep_idle_secs: u64,
    /// Release the context first.
    pub release_context_first: bool,
    /// The pressure threshold.
    pub pressure_available_percent: u8,
    /// A load never leaves less than this free.
    pub min_free_bytes: u64,
    /// Most hot models.
    pub max_hot_models: u32,
    /// Most memory hot models may hold.
    pub hot_cap_bytes: Option<u64>,
    /// Shorter idle times while a foreground app is busy.
    pub yield_to_foreground: bool,
    /// A notification for each release.
    pub notify_on_release: bool,
}

impl MemorySettings {
    /// The settings of a preset.
    pub fn preset(p: Preset) -> MemorySettings {
        let pol = Policy::preset(p);
        MemorySettings {
            preset: p,
            idle_unload_secs: pol.idle_unload / SECOND,
            deep_idle_secs: pol.deep_idle / SECOND,
            release_context_first: pol.release_context_first,
            pressure_available_percent: pol.pressure_available_percent,
            min_free_bytes: pol.min_free,
            max_hot_models: pol.max_hot as u32,
            hot_cap_bytes: pol.hot_cap,
            yield_to_foreground: pol.yield_to_foreground,
            notify_on_release: false,
        }
    }

    /// As the policy the memory rules use.
    pub fn policy(&self) -> Policy {
        Policy {
            idle_unload: self.idle_unload_secs * SECOND,
            deep_idle: self.deep_idle_secs * SECOND,
            release_context_first: self.release_context_first,
            pressure_available_percent: self.pressure_available_percent,
            min_free: self.min_free_bytes,
            max_hot: self.max_hot_models as usize,
            hot_cap: self.hot_cap_bytes,
            yield_to_foreground: self.yield_to_foreground,
        }
    }
}

impl Default for MemorySettings {
    fn default() -> Self {
        MemorySettings::preset(Preset::Balanced)
    }
}

/// Network settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetworkSettings {
    /// No downloads at all.
    pub offline: bool,
    /// Serve the API on the local network too (TLS, keys required).
    pub lan_enabled: bool,
    /// The LAN port.
    pub lan_port: u16,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        NetworkSettings {
            offline: false,
            lan_enabled: false,
            lan_port: 11_443,
        }
    }
}

/// Privacy settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PrivacySettings {
    /// Keep request and response text for the inspector (on this machine only).
    pub keep_request_content: bool,
    /// Days of metrics kept.
    pub metrics_retention_days: u32,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        PrivacySettings {
            keep_request_content: false,
            metrics_retention_days: 30,
        }
    }
}

/// Where a cache keeps its entries (ADR 0019).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CacheStorage {
    /// In memory only: gone when Pitwall stops.
    Memory,
    /// In `cache.sqlite` in Pitwall's data folder, on this computer.
    Disk,
}

/// Caching (ADR 0019). The prompt-prefix cache (llama-server's) is always on; these settings size and steer it, and
/// configure the response cache (apps opt in) and the embeddings cache (on by default).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CacheSettings {
    /// Send requests of one conversation to the same llama-server slot so its prompt cache hits.
    pub prefix_affinity: bool,
    /// Host-memory prompt cache per model in MiB (`--cache-ram`); `None` sizes it from this machine's memory.
    pub prompt_cache_mib: Option<u32>,
    /// Cache embeddings (deterministic, so safe).
    pub embeddings: bool,
    /// Where embeddings are kept.
    pub embeddings_storage: CacheStorage,
    /// Largest embeddings cache (MiB).
    pub embeddings_max_mib: u32,
    /// How long an embedding is kept after it was last used (seconds; 0: never, only the size cap removes it).
    pub embeddings_ttl_secs: u64,
    /// Where cached answers are kept (once an app or preset opts in).
    pub responses_storage: CacheStorage,
    /// Largest response cache (MiB).
    pub responses_max_mib: u32,
    /// How long an answer is kept after it was stored (seconds; 0: never, only the size cap removes it).
    pub responses_ttl_secs: u64,
    /// Days a saved prompt prefix (keep-warm file) is kept unused.
    pub keep_warm_days: u32,
}

impl Default for CacheSettings {
    fn default() -> Self {
        CacheSettings {
            prefix_affinity: true,
            prompt_cache_mib: None,
            embeddings: true,
            embeddings_storage: CacheStorage::Disk,
            embeddings_max_mib: 512,
            embeddings_ttl_secs: 90 * 24 * 3600,
            responses_storage: CacheStorage::Disk,
            responses_max_mib: 256,
            responses_ttl_secs: 7 * 24 * 3600,
            keep_warm_days: 30,
        }
    }
}

impl CacheSettings {
    /// The cache's expiry rules: answers expire a fixed time after they were stored, embeddings a time after their last
    /// use (sliding).
    pub fn expiry(&self, kind: crate::cache::Kind) -> crate::cache::Expiry {
        let (secs, sliding) = match kind {
            crate::cache::Kind::Response => (self.responses_ttl_secs, false),
            crate::cache::Kind::Embedding => (self.embeddings_ttl_secs, true),
        };
        crate::cache::Expiry {
            ttl: (secs > 0).then(|| secs.saturating_mul(1000)),
            sliding,
        }
    }

    /// Bounds of a cache.
    pub fn limits(&self, kind: crate::cache::Kind) -> crate::cache::Limits {
        let mib = match kind {
            crate::cache::Kind::Response => self.responses_max_mib,
            crate::cache::Kind::Embedding => self.embeddings_max_mib,
        };
        crate::cache::Limits {
            max_bytes: u64::from(mib) * MIB,
            expiry: self.expiry(kind),
        }
    }

    /// Where `kind` is kept.
    pub fn storage(&self, kind: crate::cache::Kind) -> CacheStorage {
        match kind {
            crate::cache::Kind::Response => self.responses_storage,
            crate::cache::Kind::Embedding => self.embeddings_storage,
        }
    }
}

/// Per-model preferences.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelPrefs {
    /// "Keep loaded".
    pub pinned: bool,
    /// Its own idle time.
    pub idle_secs: Option<u64>,
    /// Its own context length.
    pub context: Option<u32>,
    /// The engine pack that serves it (`None`: llama.cpp).
    pub engine: Option<String>,
    /// Its own number of parallel slots (`None`: [`ModelSettings::default_slots`]). A value that is out of range in
    /// the file is read as unset. Applies the next time the model loads.
    #[serde(deserialize_with = "crate::slots::lenient")]
    pub slots: Option<Slots>,
}

/// Model settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelSettings {
    /// The model `default` / unnamed requests use.
    pub default_model: Option<ModelId>,
    /// Context length models load with unless a request or the model's preferences say otherwise.
    pub default_context: u32,
    /// Parallel slots a model loads with unless its preferences say otherwise (each slot has `default_context`
    /// tokens of its own, see `slots`).
    pub default_slots: Slots,
    /// Checking Hugging Face for newer revisions and better quantisations of the models you downloaded (off until
    /// you turn it on; weekly or daily once on). It only ever tells you; downloads stay your decision.
    pub update_check: UpdateCheckSettings,
    /// Per-model preferences.
    pub prefs: BTreeMap<ModelId, ModelPrefs>,
}

impl Default for ModelSettings {
    fn default() -> Self {
        ModelSettings {
            default_model: None,
            default_context: 8192,
            default_slots: Slots::DEFAULT,
            update_check: UpdateCheckSettings::default(),
            prefs: BTreeMap::new(),
        }
    }
}

/// Engine settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EngineSettings {
    /// `auto` or a llama.cpp build id: the build automatic selection prefers. Pins (all models, or one model) are not
    /// settings; they live in `engine-pins.json` (`runtimes::pins`, ADR 0018) and win over this.
    pub build_choice: String,
}

impl Default for EngineSettings {
    fn default() -> Self {
        EngineSettings {
            build_choice: "auto".into(),
        }
    }
}

/// Everything in `settings.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The schema version.
    pub version: u32,
    /// General.
    pub general: GeneralSettings,
    /// Server.
    pub server: ServerSettings,
    /// Memory.
    pub memory: MemorySettings,
    /// Network.
    pub network: NetworkSettings,
    /// Privacy.
    pub privacy: PrivacySettings,
    /// Models.
    pub models: ModelSettings,
    /// Engines.
    pub engines: EngineSettings,
    /// Caching.
    pub cache: CacheSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SCHEMA_VERSION,
            general: GeneralSettings::default(),
            server: ServerSettings::default(),
            memory: MemorySettings::default(),
            network: NetworkSettings::default(),
            privacy: PrivacySettings::default(),
            models: ModelSettings::default(),
            engines: EngineSettings::default(),
            cache: CacheSettings::default(),
        }
    }
}

/// The part of the settings the UI edits (`Settings` in `api.types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UiSettings {
    /// General.
    pub general: GeneralSettings,
    /// Server.
    pub server: ServerSettings,
    /// Memory.
    pub memory: MemorySettings,
    /// Network.
    pub network: NetworkSettings,
    /// Privacy.
    pub privacy: PrivacySettings,
    /// Caching.
    pub cache: CacheSettings,
}

impl Settings {
    /// The UI's part.
    pub fn ui(&self) -> UiSettings {
        UiSettings {
            general: self.general.clone(),
            server: self.server.clone(),
            memory: self.memory.clone(),
            network: self.network.clone(),
            privacy: self.privacy.clone(),
            cache: self.cache.clone(),
        }
    }

    /// These settings with the UI's part replaced, validated.
    pub fn with_ui(&self, ui: UiSettings) -> Result<Settings, String> {
        let s = Settings {
            general: ui.general,
            server: ui.server,
            memory: ui.memory,
            network: ui.network,
            privacy: ui.privacy,
            cache: ui.cache,
            ..self.clone()
        };
        s.validate()?;
        Ok(s)
    }

    /// Switches the memory settings to a preset.
    pub fn apply_preset(&mut self, p: Preset) {
        let notify = self.memory.notify_on_release;
        self.memory = MemorySettings {
            notify_on_release: notify,
            ..MemorySettings::preset(p)
        };
    }

    /// Checks every value's range; the message names the setting and the allowed range.
    pub fn validate(&self) -> Result<(), String> {
        fn range<T: PartialOrd + std::fmt::Display>(name: &str, v: T, lo: T, hi: T) -> Result<(), String> {
            if v < lo || v > hi {
                Err(format!("{name} must be between {lo} and {hi} (it is {v})"))
            } else {
                Ok(())
            }
        }
        range("Server ▸ Port", self.server.port, 1024, 65535)?;
        range("Server ▸ Largest request (bytes)", self.server.max_request_bytes, 1024, 256 * MIB)?;
        range("Server ▸ Request timeout (s)", self.server.request_timeout_secs, 5, 3600)?;
        range("Server ▸ Requests at once", self.server.max_concurrent, 1, 64)?;
        range("Memory ▸ Idle release (s)", self.memory.idle_unload_secs, 10, 7 * 24 * 3600)?;
        range(
            "Memory ▸ Release everything after (s)",
            self.memory.deep_idle_secs,
            self.memory.idle_unload_secs,
            30 * 24 * 3600,
        )?;
        range("Memory ▸ Pressure threshold (%)", self.memory.pressure_available_percent, 1, 90)?;
        range("Memory ▸ Keep free (bytes)", self.memory.min_free_bytes, 0, 64 * GIB)?;
        range("Memory ▸ Models loaded at once", self.memory.max_hot_models, 1, 16)?;
        if let Some(cap) = self.memory.hot_cap_bytes {
            range("Memory ▸ Memory for models (bytes)", cap, GIB, 4096 * GIB)?;
        }
        range("Network ▸ LAN port", self.network.lan_port, 1024, 65535)?;
        if self.network.lan_enabled && self.network.lan_port == self.server.port {
            return Err("Network ▸ LAN port must differ from Server ▸ Port".into());
        }
        range("Privacy ▸ Keep metrics (days)", self.privacy.metrics_retention_days, 1, 365)?;
        range("Caching ▸ Prompt cache (MiB)", self.cache.prompt_cache_mib.unwrap_or(0), 0, 64 * 1024)?;
        range("Caching ▸ Embeddings cache (MiB)", self.cache.embeddings_max_mib, 1, 64 * 1024)?;
        if self.cache.embeddings_ttl_secs != 0 {
            range("Caching ▸ Embeddings kept (s)", self.cache.embeddings_ttl_secs, 60, 3650 * 24 * 3600)?;
        }
        range("Caching ▸ Answers cache (MiB)", self.cache.responses_max_mib, 1, 64 * 1024)?;
        if self.cache.responses_ttl_secs != 0 {
            range("Caching ▸ Answers kept (s)", self.cache.responses_ttl_secs, 60, 3650 * 24 * 3600)?;
        }
        range("Caching ▸ Keep-warm files kept (days)", self.cache.keep_warm_days, 1, 365)?;
        range("Models ▸ Default context", self.models.default_context, 512, 1 << 20)?;
        Ok(())
    }

    /// The slots a model loads with: its own preference or the default.
    pub fn slots_for(&self, id: &ModelId) -> Slots {
        self.models.prefs.get(id).and_then(|p| p.slots).unwrap_or(self.models.default_slots)
    }

    /// The preferences of a model (defaults when it has none).
    pub fn prefs(&self, id: &ModelId) -> ModelPrefs {
        self.models.prefs.get(id).cloned().unwrap_or_default()
    }

    /// As TOML.
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

/// How the settings were loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loaded {
    /// From the file.
    File,
    /// The file was missing: defaults.
    Defaults,
    /// The file was damaged or invalid: from the backup; the message says why.
    Backup(String),
    /// File and backup were unusable: defaults; the message says why.
    Reset(String),
}

fn parse(text: &str) -> Result<Settings, String> {
    if text.len() > MAX_SETTINGS_BYTES {
        return Err("the settings file is too large".into());
    }
    let s: Settings = toml::from_str(text).map_err(|e| format!("the settings file is damaged: {}", e.message()))?;
    if s.version > SCHEMA_VERSION {
        return Err(format!(
            "the settings file is from a newer Pitwall (version {}); using defaults",
            s.version
        ));
    }
    // Version 0 (no version key) and 1 share the schema: nothing to migrate yet.
    let s = Settings {
        version: SCHEMA_VERSION,
        ..s
    };
    s.validate()?;
    Ok(s)
}

/// Loads the settings from the file's text and its backup's (either may be missing).
pub fn load(primary: Option<&str>, backup: Option<&str>) -> (Settings, Loaded) {
    match primary.map(parse) {
        Some(Ok(s)) => (s, Loaded::File),
        None if backup.is_none() => (Settings::default(), Loaded::Defaults),
        other => {
            let why = match other {
                Some(Err(e)) => e,
                _ => "the settings file is missing".into(),
            };
            match backup.map(parse) {
                Some(Ok(s)) => (s, Loaded::Backup(why)),
                _ => (Settings::default(), Loaded::Reset(why)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_round_trip() {
        let s = Settings::default();
        assert!(s.validate().is_ok());
        assert_eq!(s.server.port, DEFAULT_PORT);
        assert!(!s.server.allow_keyless_local && !s.privacy.keep_request_content && !s.network.lan_enabled);
        let (back, how) = load(Some(&s.to_toml()), None);
        assert_eq!((back, how), (s.clone(), Loaded::File));
        let json = serde_json::to_value(s.ui()).unwrap();
        assert_eq!(json["memory"]["idleUnloadSecs"], 600);
        assert_eq!(json["server"]["allowKeylessLocal"], false);
    }

    #[test]
    fn missing_keys_take_defaults() {
        let (s, how) = load(Some("[server]\nport = 12000\n"), None);
        assert_eq!(how, Loaded::File);
        assert_eq!(s.server.port, 12000);
        assert_eq!(s.memory, MemorySettings::default());
        let (s, _) = load(Some(""), None);
        assert_eq!(s.version, SCHEMA_VERSION);
    }

    #[test]
    fn damage_falls_back_to_the_backup_then_defaults() {
        let good = Settings {
            server: ServerSettings {
                port: 12001,
                ..ServerSettings::default()
            },
            ..Settings::default()
        }
        .to_toml();
        let (s, how) = load(Some("[[[ not toml"), Some(&good));
        assert_eq!(s.server.port, 12001);
        assert!(matches!(how, Loaded::Backup(m) if m.contains("damaged")));
        let (s, how) = load(Some("[server]\nport = 80\n"), Some("also bad ["));
        assert_eq!(s, Settings::default());
        assert!(matches!(how, Loaded::Reset(m) if m.contains("Port")));
        let (_, how) = load(None, Some(&good));
        assert!(matches!(how, Loaded::Backup(m) if m.contains("missing")));
        assert_eq!(load(None, None).1, Loaded::Defaults);
        let (_, how) = load(Some("version = 99\n"), None);
        assert!(matches!(how, Loaded::Reset(m) if m.contains("newer")));
        let huge = "#".repeat(MAX_SETTINGS_BYTES + 1);
        assert!(matches!(load(Some(&huge), None).1, Loaded::Reset(m) if m.contains("too large")));
    }

    #[test]
    fn validation_names_the_setting() {
        let base = Settings::default();
        let cases: Vec<(Settings, &str)> = vec![
            (
                Settings {
                    server: ServerSettings {
                        max_concurrent: 0,
                        ..base.server.clone()
                    },
                    ..base.clone()
                },
                "Requests at once",
            ),
            (
                Settings {
                    server: ServerSettings {
                        request_timeout_secs: 1,
                        ..base.server.clone()
                    },
                    ..base.clone()
                },
                "timeout",
            ),
            (
                Settings {
                    server: ServerSettings {
                        max_request_bytes: 1,
                        ..base.server.clone()
                    },
                    ..base.clone()
                },
                "Largest request",
            ),
            (
                Settings {
                    memory: MemorySettings {
                        idle_unload_secs: 1,
                        ..base.memory.clone()
                    },
                    ..base.clone()
                },
                "Idle release",
            ),
            (
                Settings {
                    memory: MemorySettings {
                        deep_idle_secs: 10,
                        ..base.memory.clone()
                    },
                    ..base.clone()
                },
                "Release everything",
            ),
            (
                Settings {
                    memory: MemorySettings {
                        pressure_available_percent: 95,
                        ..base.memory.clone()
                    },
                    ..base.clone()
                },
                "Pressure",
            ),
            (
                Settings {
                    memory: MemorySettings {
                        min_free_bytes: 100 * GIB,
                        ..base.memory.clone()
                    },
                    ..base.clone()
                },
                "Keep free",
            ),
            (
                Settings {
                    memory: MemorySettings {
                        max_hot_models: 0,
                        ..base.memory.clone()
                    },
                    ..base.clone()
                },
                "Models loaded",
            ),
            (
                Settings {
                    memory: MemorySettings {
                        hot_cap_bytes: Some(1),
                        ..base.memory.clone()
                    },
                    ..base.clone()
                },
                "Memory for models",
            ),
            (
                Settings {
                    network: NetworkSettings {
                        lan_port: 80,
                        ..base.network.clone()
                    },
                    ..base.clone()
                },
                "LAN port",
            ),
            (
                Settings {
                    network: NetworkSettings {
                        lan_enabled: true,
                        lan_port: DEFAULT_PORT,
                        ..base.network.clone()
                    },
                    ..base.clone()
                },
                "must differ",
            ),
            (
                Settings {
                    privacy: PrivacySettings {
                        metrics_retention_days: 0,
                        ..base.privacy.clone()
                    },
                    ..base.clone()
                },
                "metrics",
            ),
            (
                Settings {
                    models: ModelSettings {
                        default_context: 1,
                        ..base.models.clone()
                    },
                    ..base.clone()
                },
                "Default context",
            ),
        ];
        for (s, needle) in cases {
            let e = s.validate().unwrap_err();
            assert!(e.contains(needle), "{e}");
        }
        assert!(Settings {
            memory: MemorySettings {
                hot_cap_bytes: Some(8 * GIB),
                ..base.memory.clone()
            },
            ..base.clone()
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn update_checks_are_off_until_enabled_and_survive_a_restart() {
        use crate::update_state::UpdateSchedule;
        let mut s = Settings::default();
        assert!(
            !s.models.update_check.enabled,
            "nothing is asked of Hugging Face before the user opts in"
        );
        assert_eq!(s.models.update_check.schedule, UpdateSchedule::Weekly);
        s.models.update_check.enabled = true;
        s.models.update_check.schedule = UpdateSchedule::Daily;
        let (back, how) = load(Some(&s.to_toml()), None);
        assert_eq!((back.models.update_check, how), (s.models.update_check, Loaded::File));
        // Only part of it set: the rest keeps its default.
        let (s, how) = load(Some("[models.updateCheck]\nenabled = true\n"), None);
        assert_eq!(
            (s.models.update_check.enabled, s.models.update_check.schedule, how),
            (true, UpdateSchedule::Weekly, Loaded::File)
        );
        // A schedule that is not one: the file is invalid, the backup (here: defaults) is used.
        let (s, how) = load(Some("[models.updateCheck]\nschedule = \"hourly\"\n"), None);
        assert!(!s.models.update_check.enabled && how != Loaded::File);
    }

    #[test]
    fn slot_preferences_default_persist_and_tolerate_damage() {
        let id = ModelId::parse("m").unwrap();
        let mut s = Settings::default();
        assert_eq!(s.slots_for(&id), Slots::DEFAULT);
        s.models.default_slots = Slots::new(3).unwrap();
        assert_eq!(s.slots_for(&id).get(), 3);
        s.models.prefs.insert(
            id.clone(),
            ModelPrefs {
                slots: Slots::new(8).ok(),
                ..ModelPrefs::default()
            },
        );
        assert_eq!(s.slots_for(&id).get(), 8);
        let (back, how) = load(Some(&s.to_toml()), None);
        assert_eq!((back.slots_for(&id).get(), how), (8, Loaded::File), "survives a restart");
        // Damaged values: the preference reads as unset, nothing else is lost.
        for bad in ["slots = 0", "slots = 99", "slots = \"many\"", "slots = -1"] {
            let text = format!("[models]\ndefaultContext = 4096\n[models.prefs.m]\npinned = true\n{bad}\n");
            let (s, how) = load(Some(&text), None);
            assert_eq!(how, Loaded::File, "{bad}");
            assert!(s.prefs(&id).pinned && s.prefs(&id).slots.is_none(), "{bad}");
            assert_eq!((s.models.default_context, s.slots_for(&id)), (4096, Slots::DEFAULT));
        }
        // A damaged default falls back through the backup like any other invalid setting.
        let (s, how) = load(Some("[models]\ndefaultSlots = 99\n"), Some(&Settings::default().to_toml()));
        assert_eq!(s.models.default_slots, Slots::DEFAULT);
        assert!(matches!(how, Loaded::Backup(_)));
    }

    #[test]
    fn presets_ui_merge_and_prefs() {
        let mut s = Settings::default();
        s.memory.notify_on_release = true;
        s.apply_preset(Preset::LowMemory);
        assert_eq!(s.memory.preset, Preset::LowMemory);
        assert_eq!(s.memory.max_hot_models, 1);
        assert!(s.memory.notify_on_release, "kept");
        assert_eq!(
            s.memory.policy(),
            Policy {
                ..Policy::preset(Preset::LowMemory)
            }
        );
        let id = ModelId::parse("m").unwrap();
        s.models.prefs.insert(
            id.clone(),
            ModelPrefs {
                pinned: true,
                idle_secs: Some(30),
                context: None,
                engine: None,
                slots: None,
            },
        );
        let mut ui = s.ui();
        ui.server.port = 12345;
        let merged = s.with_ui(ui.clone()).unwrap();
        assert_eq!(merged.server.port, 12345);
        assert!(merged.prefs(&id).pinned, "model preferences survive a UI save");
        assert_eq!(merged.prefs(&ModelId::parse("other").unwrap()), ModelPrefs::default());
        ui.server.port = 1;
        assert!(s.with_ui(ui).is_err());
    }
}
