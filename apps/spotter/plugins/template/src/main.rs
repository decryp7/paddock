//! A starting point for a Spotter plugin (docs/plugins.md).
//!
//! 1. Copy this folder, rename the package in `Cargo.toml` and the `id` in `plugin.toml` and below (they must agree).
//! 2. Describe your source kinds and their settings in `manifest`; Spotter renders the form from it.
//! 3. Return the releases in `list`. Never print to standard output: it is the protocol's pipe.
//! 4. `cargo build --release`, then copy the program and `plugin.toml` to `<Spotter data folder>/plugins/<id>/`.

use spotter_plugin_api::{serve, Config, ErrorKind, FieldKind, FieldSpec, Manifest, Plugin, PluginError, SourceKindSpec, WireRelease};
use std::process::ExitCode;

struct MyPlugin;

impl Plugin for MyPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new("my-plugin", "My plugin", env!("CARGO_PKG_VERSION")).with_kind(
            SourceKindSpec::new("things", "My releases", "One sentence about where the releases come from.")
                .with_field(FieldSpec::new("address", "Address", FieldKind::Url).required())
                .with_field(FieldSpec::new("token", "Access token", FieldKind::Secret).help("Kept in the credential store.")),
        )
    }

    fn list(&self, kind: &str, config: &Config, limit: usize) -> Result<Vec<WireRelease>, PluginError> {
        if kind != "things" {
            return Err(PluginError::new(ErrorKind::Unsupported, format!("unknown kind \"{kind}\"")));
        }
        let address = config
            .require("address")
            .map_err(|e| PluginError::new(ErrorKind::InvalidConfig, e))?;
        // Fetch and parse your releases here. Report an offline server as `ErrorKind::Unreachable` (Spotter backs
        // off and shows it as a status) and refused credentials as `ErrorKind::Unauthorized`.
        let releases = vec![WireRelease::new("1.0.0", "Example 1.0.0", address)];
        Ok(releases.into_iter().take(limit).collect())
    }
}

fn main() -> ExitCode {
    serve(MyPlugin)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn lists_and_refuses_unknown_kinds() {
        let cfg = Config::new().with("address", "https://example.org");
        assert_eq!(MyPlugin.list("things", &cfg, 10).unwrap().len(), 1);
        assert_eq!(MyPlugin.list("other", &cfg, 10).unwrap_err().kind, ErrorKind::Unsupported);
        assert_eq!(
            MyPlugin.list("things", &Config::new(), 10).unwrap_err().kind,
            ErrorKind::InvalidConfig
        );
        assert_eq!(MyPlugin.manifest().kinds.len(), 1);
    }
}
