//! The files of a new plugin project. Pure: [`Template::files`] returns paths and texts; [`crate::ops::new`] writes them.
//!
//! A project holds a `Cargo.toml` that depends on `paddock-plugin-api` alone (plus `serde_json` for tool values, and the
//! test kit as a dev-dependency), a `manifest.toml`, a library with the plugin, a `main` that is one `serve` call, and a test
//! that runs the conformance checks against the plugin in a fake host.

use paddock_cli::CliError;
use paddock_plugin_package::Manifest;
use paddock_plugin_package::manifest::{host_target, valid_binary};
use std::path::PathBuf;

/// Where the platform crates come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Api {
    /// The `decryp7` cargo registry (what a plugin author uses).
    Registry,
    /// A checkout's `crates` folder (how the platform's own tests and its developers build a plugin without a network).
    Path(PathBuf),
}

/// What `new` knows about the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// The crate and program name.
    pub name: String,
    /// The plugin id.
    pub id: String,
    /// The publisher shown to users.
    pub publisher: String,
    /// Where the platform crates come from.
    pub api: Api,
}

impl Template {
    /// A template for `name` with the defaults for everything else.
    pub fn new(name: &str) -> Template {
        Template {
            name: name.into(),
            id: format!("example.{name}"),
            publisher: "Example".into(),
            api: Api::Registry,
        }
    }

    /// The crate name as a Rust identifier.
    pub fn ident(&self) -> String {
        self.name.replace('-', "_")
    }

    /// The plugin type's name (`my-plugin` is `MyPlugin`).
    pub fn type_name(&self) -> String {
        self.name
            .split(['-', '_'])
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                    .unwrap_or_default()
            })
            .collect()
    }

    /// The tool the template plugin contributes.
    pub fn tool_id(&self) -> String {
        format!("{}.tool", self.name)
    }

    fn dependency(&self, crate_name: &str) -> String {
        match &self.api {
            Api::Registry => {
                format!("{crate_name} = {{ version = \"0.1\", registry = \"decryp7\" }}")
            }
            Api::Path(dir) => {
                let path = toml::Value::String(dir.join(crate_name).to_string_lossy().into_owned());
                format!("{crate_name} = {{ path = {path} }}")
            }
        }
    }

    /// The `manifest.toml` text.
    pub fn manifest(&self) -> String {
        format!(
            r#"format = 1

[plugin]
id = "{id}"
name = "{display}"
version = "0.1.0"
publisher = {publisher}
description = "A plugin made with cargo paddock-plugin new."
protocol = "{protocol}"
binary = "{name}"
targets = ["{target}"]

[[contributions]]
kind = "tool"
id = "{tool}"
actions = [{{ id = "hello" }}]
"#,
            id = self.id,
            display = self.type_name(),
            publisher = toml::Value::String(self.publisher.clone()),
            protocol = paddock_plugin_api::PLATFORM_PROTOCOL,
            name = self.name,
            target = host_target(),
            tool = self.tool_id(),
        )
    }

    fn cargo_toml(&self) -> String {
        format!(
            r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
publish = false

# A plugin is its own workspace, so building it never depends on the folder it sits in.
[workspace]

[dependencies]
# A plugin links the platform's API crate alone (ADR 0001): no host or app crate.
{api}
serde_json = "1"

[dev-dependencies]
{test_kit}
"#,
            name = self.name,
            api = self.dependency("paddock-plugin-api"),
            test_kit = self.dependency("paddock-plugin-test"),
        )
    }

    fn lib_rs(&self) -> String {
        format!(
            r#"//! {name}: a Paddock plugin. It runs out of process; the app starts the program `main` serves.

use paddock_plugin_api::prelude::*;
use serde_json::{{Value, json}};
use std::sync::Arc;

/// The tool's id; the same as the one in `manifest.toml`.
pub const TOOL: &str = "{tool}";

/// The plugin.
pub struct {ty};

struct Hello;

impl Tool for Hello {{
    fn id(&self) -> &str {{
        TOOL
    }}

    fn describe(&self) -> ToolInfo {{
        ToolInfo::new(TOOL, "Hello").op(OpSpec::new("hello", "Say hello"))
    }}

    fn invoke(&self, op: &str, _args: Value, _ctx: &ToolCtx) -> Result<Value, ToolError> {{
        match op {{
            "hello" => Ok(json!({{ "text": "Hello from {name}" }})),
            other => Err(ToolError::Missing(format!("{{TOOL}} has no operation \"{{other}}\""))),
        }}
    }}
}}

impl Plugin for {ty} {{
    fn id(&self) -> &str {{
        "{id}"
    }}

    fn name(&self) -> &str {{
        "{ty}"
    }}

    fn version(&self) -> &str {{
        env!("CARGO_PKG_VERSION")
    }}

    fn register(&self, c: &mut Contributions) -> Result<(), RegisterError> {{
        c.add_tool(Hello);
        Ok(())
    }}
}}

/// The plugin as the process serves it.
pub fn plugin() -> Arc<dyn Plugin> {{
    Arc::new({ty})
}}
"#,
            name = self.name,
            tool = self.tool_id(),
            ty = self.type_name(),
            id = self.id,
        )
    }

    fn main_rs(&self) -> String {
        format!(
            "fn main() -> Result<(), paddock_plugin_api::IpcError> {{\n    paddock_plugin_api::serve({}::plugin())\n}}\n",
            self.ident()
        )
    }

    fn test_rs(&self) -> String {
        format!(
            "//! The platform's conformance checks, run against the plugin in a fake host.\n\n#[test]\nfn the_plugin_conforms() {{\n    paddock_plugin_test::conformance::assert_conforms({}::plugin());\n}}\n",
            self.ident()
        )
    }

    /// Every file of the project, relative to the project folder.
    ///
    /// # Errors
    ///
    /// When the name is not a valid program name or the id and publisher do not make a valid manifest (the text says what to
    /// write).
    pub fn files(&self) -> Result<Vec<(PathBuf, String)>, CliError> {
        let first_letter = self.name.starts_with(|c: char| c.is_ascii_lowercase());
        if !valid_binary(&self.name) || !first_letter || self.name.contains(['.', '/']) {
            return Err(CliError::usage(format!(
                "\"{}\" cannot be a plugin name.",
                self.name
            ))
            .with_hint("Use lowercase letters, digits, '-' and '_', starting with a letter (my-plugin)."));
        }
        let manifest = self.manifest();
        Manifest::parse(&manifest).map_err(|e| {
            CliError::usage(format!("The plugin id or publisher is not valid: {e}."))
                .with_hint("Pass --id com.example.my-plugin and --publisher \"Your name\".")
        })?;
        // The registry is declared in both forms: the platform crates' own manifests name it, also when they are used by path.
        let files = vec![
            (PathBuf::from("Cargo.toml"), self.cargo_toml()),
            (PathBuf::from("manifest.toml"), manifest),
            (PathBuf::from("src/lib.rs"), self.lib_rs()),
            (PathBuf::from("src/main.rs"), self.main_rs()),
            (PathBuf::from("tests/conformance.rs"), self.test_rs()),
            (PathBuf::from(".gitignore"), "/target\n/dist\n*.key\n".into()),
            (
                PathBuf::from(".cargo/config.toml"),
                "[registries.decryp7]\nindex = \"sparse+https://dev.decryptology.net/api/packages/decryp7/cargo/\"\n".into(),
            ),
        ];
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &Template, name: &str) -> String {
        t.files()
            .unwrap()
            .into_iter()
            .find(|(p, _)| p == std::path::Path::new(name))
            .unwrap()
            .1
    }

    #[test]
    fn the_generated_manifest_and_cargo_toml_are_valid_and_depend_on_the_api_alone() {
        for name in ["hello", "my-plugin", "a_b1"] {
            let t = Template::new(name);
            let m = Manifest::parse(&text(&t, "manifest.toml")).unwrap();
            assert_eq!(m.binary, name);
            assert_eq!(m.id.as_str(), format!("example.{name}"));
            let cargo: toml::Table = text(&t, "Cargo.toml").parse().unwrap();
            let deps = cargo["dependencies"].as_table().unwrap();
            let mut names: Vec<_> = deps.keys().map(String::as_str).collect();
            names.sort_unstable();
            assert_eq!(names, ["paddock-plugin-api", "serde_json"]);
            assert_eq!(cargo["package"]["name"].as_str(), Some(name));
            assert!(cargo.contains_key("workspace"));
            assert!(text(&t, ".cargo/config.toml").contains("decryp7"));
        }
    }

    #[test]
    fn names_are_derived_and_the_path_form_has_no_registry() {
        let mut t = Template::new("my-big_plugin");
        assert_eq!(t.type_name(), "MyBigPlugin");
        assert_eq!(t.ident(), "my_big_plugin");
        assert_eq!(t.tool_id(), "my-big_plugin.tool");
        assert!(text(&t, "src/main.rs").contains("my_big_plugin::plugin()"));
        assert!(text(&t, "tests/conformance.rs").contains("my_big_plugin::plugin()"));
        assert!(text(&t, "src/lib.rs").contains("pub struct MyBigPlugin;"));
        t.api = Api::Path(PathBuf::from("/checkout/crates"));
        let cargo = text(&t, "Cargo.toml");
        assert!(
            // A TOML string in double quotes, or in single quotes where the joined path holds a backslash (Windows).
            (cargo.contains("paddock-plugin-api = { path = \"/checkout/crates")
                || cargo.contains("paddock-plugin-api = { path = '/checkout/crates"))
                && !cargo.contains("registry"),
            "{cargo}"
        );
        assert!(text(&t, ".cargo/config.toml").contains("decryp7"));
        assert_eq!(Template::new("-").type_name(), "");
    }

    #[test]
    fn a_bad_name_id_or_publisher_says_what_to_write() {
        for bad in ["", "Hello", "1abc", "a.b", "a/b", "a b", "-x"] {
            let e = Template::new(bad).files().unwrap_err();
            assert_eq!(e.exit, paddock_cli::Exit::Usage, "{bad}");
            assert!(e.message.contains("cannot be a plugin name"), "{bad}");
        }
        let mut t = Template::new("ok");
        t.id = "not valid!".into();
        let e = t.files().unwrap_err();
        assert!(
            e.message.contains("not valid") && e.hint.contains("--id"),
            "{e:?}"
        );
        let mut t = Template::new("ok");
        t.publisher = "  ".into();
        assert!(t.files().is_err());
        t.publisher = "Quote \" Ltd".into();
        assert!(t.files().is_ok());
    }
}
