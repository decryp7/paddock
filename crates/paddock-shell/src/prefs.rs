//! The update preferences on the shell's `pd.updates` settings section: the settings are their one writer and source
//! (rule 40). The section's apply puts stored and changed values into effect in the updater; the Updates dialog's
//! `updates_set_preferences` stores through [`SettingsPreferences`], which ends in the same apply.

use paddock_settings::{
    ApplyError, Configurable, Settings, SettingsError, ValidValues, shell::UPDATES,
};
use paddock_updater::{PreferenceStore, Preferences, UpdateError, Updater};
use serde_json::json;
use std::sync::{Arc, Weak};

/// The preferences in a settings section's values (missing or wrong-typed keys: the defaults).
pub(crate) fn preferences_of(values: &ValidValues) -> Preferences {
    Preferences {
        auto_check: values.bool("autoCheck").unwrap_or(false),
        channel: values.text("channel").unwrap_or_default().to_string(),
    }
}

/// The `pd.updates` section, applied to `updater` (held weakly: the settings outlive nobody, but must not keep the
/// updater alive).
pub(crate) fn section(updater: &Arc<Updater>) -> Option<Arc<dyn Configurable>> {
    let channels = updater.status().channels;
    let pairs: Vec<(&str, &str)> = channels
        .iter()
        .map(|c| (c.id.as_str(), c.label.as_str()))
        .collect();
    let schema = paddock_settings::shell::updates(&pairs)?;
    let weak: Weak<Updater> = Arc::downgrade(updater);
    Some(paddock_settings::section(
        schema,
        move |values| -> Result<(), ApplyError> {
            match weak.upgrade() {
                Some(u) => u
                    .apply_preferences(preferences_of(values))
                    .map(drop)
                    .map_err(|e| Box::new(e) as ApplyError),
                None => Ok(()),
            }
        },
    ))
}

/// The updater's [`PreferenceStore`] on the settings: a change is a `settings_set` of `pd.updates`, so every window
/// and the Settings window hear it and the file keeps it.
pub struct SettingsPreferences {
    settings: Arc<Settings>,
}

impl SettingsPreferences {
    pub fn new(settings: Arc<Settings>) -> SettingsPreferences {
        SettingsPreferences { settings }
    }
}

impl PreferenceStore for SettingsPreferences {
    fn save(&self, prefs: &Preferences) -> Result<(), UpdateError> {
        let values = json!({ "autoCheck": prefs.auto_check, "channel": prefs.channel });
        match self.settings.set(UPDATES, &values) {
            Ok(r) if r.applied => Ok(()),
            Ok(r) => Err(UpdateError::Invalid {
                detail: r
                    .errors
                    .into_iter()
                    .map(|e| e.message)
                    .collect::<Vec<_>>()
                    .join(" "),
            }),
            Err(SettingsError::Apply { message, .. }) => {
                Err(UpdateError::Invalid { detail: message })
            }
            Err(e) => Err(UpdateError::Invalid {
                detail: e.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// The `pd.updates` section; its apply refuses once `refuse` is set (after the registration's first apply).
    fn updates_section(refuse: Arc<AtomicBool>) -> Arc<dyn Configurable> {
        let schema =
            paddock_settings::shell::updates(&[("stable", "Stable"), ("beta", "Beta")]).unwrap();
        paddock_settings::section(schema, move |_| -> Result<(), ApplyError> {
            if refuse.load(Ordering::SeqCst) {
                Err("busy downloading".into())
            } else {
                Ok(())
            }
        })
    }

    #[test]
    fn the_store_reports_each_kind_of_refusal_as_invalid() {
        let prefs = |channel: &str| Preferences {
            auto_check: true,
            channel: channel.into(),
        };
        let detail = |r: Result<(), UpdateError>| match r {
            Err(UpdateError::Invalid { detail }) => detail,
            other => panic!("{other:?}"),
        };
        // No section: the settings do not know it.
        let none = SettingsPreferences::new(Arc::new(Settings::in_memory()));
        assert!(detail(none.save(&prefs("stable"))).contains(UPDATES));
        let refuse = Arc::new(AtomicBool::new(false));
        let settings = Arc::new(Settings::in_memory());
        settings.register(updates_section(refuse.clone())).unwrap();
        let store = SettingsPreferences::new(settings);
        assert!(store.save(&prefs("beta")).is_ok());
        // Validation refuses an unknown channel; the section's apply may refuse too.
        assert!(!detail(store.save(&prefs("nightly"))).is_empty());
        refuse.store(true, Ordering::SeqCst);
        assert!(detail(store.save(&prefs("stable"))).contains("busy"));
    }
}
