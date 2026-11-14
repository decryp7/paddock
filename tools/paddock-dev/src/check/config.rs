//! A run's config: `paddock.conformance.json` over the defaults, and the app name. The port of `loadConfig` in
//! `conformance/src/context.mjs`: a file that exists but cannot be read is an error, not a default.

use serde_json::{Map, Value, json};
use std::path::Path;

/// The file in the repository root.
pub const FILE: &str = "paddock.conformance.json";

/// The defaults, in the order of `DEFAULT_CONFIG` in `context.mjs`.
fn defaults() -> Map<String, Value> {
    let v = json!({
        "app": null,
        "appName": null,
        "docBlocks": null,
        "baselineSkip": [],
        "windowWeight": null,
        "screenshotDirs": ["docs/screenshots"],
        "i18n": null,
        "a11y": null,
        "appDir": "app",
        "tauriDir": "app/src-tauri",
        "workflowsDir": ".gitea/workflows",
        "sidecars": [],
        "untimedCommands": {},
        "parserCrates": [],
        "parserSkip": [],
        "nonGates": [],
        "commitRange": null,
        "timingLint": false,
    });
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

/// The merged config of a run.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    app: String,
    values: Map<String, Value>,
}

impl Config {
    /// The defaults and an app name (a run with no config file, a check's control).
    pub fn bare(app: &str) -> Config {
        let mut values = defaults();
        values.insert("app".into(), json!(app));
        Config {
            app: app.to_string(),
            values,
        }
    }

    /// Reads `paddock.conformance.json` under `root` over the defaults; `app` (from `--app`) wins over the file's. An error names
    /// the file or says the app name is needed.
    pub fn load(root: &Path, app: Option<&str>) -> Result<Config, String> {
        let mut values = defaults();
        match std::fs::read(root.join(FILE)) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                match serde_json::from_str::<Value>(&text) {
                    Ok(Value::Object(m)) => values.extend(m),
                    Ok(_) => return Err(format!("{FILE}: it is not a JSON object")),
                    Err(e) => return Err(format!("{FILE}: {e}")),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{FILE}: {e}")),
        }
        if let Some(a) = app.filter(|a| !a.is_empty()) {
            values.insert("app".into(), json!(a));
        }
        let app = values
            .get("app")
            .and_then(Value::as_str)
            .filter(|a| !a.is_empty())
            .ok_or_else(|| format!("the app name is needed: --app <name> or \"app\" in {FILE}"))?
            .to_string();
        Ok(Config { app, values })
    }

    /// The app name.
    pub fn app(&self) -> &str {
        &self.app
    }

    /// A key's value (`null` when the config has no such key).
    pub fn get(&self, key: &str) -> &Value {
        self.values.get(key).unwrap_or(&Value::Null)
    }

    /// A string key (empty when it is not a string).
    pub fn str(&self, key: &str) -> &str {
        self.get(key).as_str().unwrap_or("")
    }

    /// A list of strings (non-strings are skipped).
    pub fn strings(&self, key: &str) -> Vec<String> {
        self.get(key)
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_is_merged_over_the_defaults_and_the_flag_wins() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(FILE), r#"{"app":"x","appDir":"ui","timingLint":true}"#).unwrap();
        let c = Config::load(d.path(), None).unwrap();
        assert_eq!((c.app(), c.str("appDir"), c.str("tauriDir")), ("x", "ui", "app/src-tauri"));
        assert_eq!(c.get("timingLint"), &json!(true));
        assert_eq!(Config::load(d.path(), Some("y")).unwrap().app(), "y");
    }

    #[test]
    fn no_file_needs_an_app_and_a_broken_file_is_an_error_naming_it() {
        let d = tempfile::tempdir().unwrap();
        assert!(Config::load(d.path(), None).unwrap_err().contains("the app name is needed"));
        assert_eq!(
            Config::load(d.path(), Some("a")).unwrap().strings("screenshotDirs"),
            ["docs/screenshots"]
        );
        std::fs::write(d.path().join(FILE), "{ not json").unwrap();
        assert!(
            Config::load(d.path(), Some("a"))
                .unwrap_err()
                .starts_with("paddock.conformance.json:")
        );
    }
}
