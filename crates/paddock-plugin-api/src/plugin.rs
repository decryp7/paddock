//! What a plugin is and what it contributes: [`Plugin`], and the [`Contributions`] it registers (tools, one settings
//! section, views, MCP tools, and handlers for the app contracts it speaks).

use crate::extension::{Contract, ExtensionHandler, Registered};
use crate::protocol::{
    Described, ExtensionInfo, McpBudget, McpToolInfo, PLATFORM_PROTOCOL, ViewInfo,
};
use crate::tools::{Tool, ToolCtx, ToolError, ToolInfo, check_id};
use paddock_settings::Configurable;
use paddock_view::{ViewAction, ViewDoc, ViewPatch};
use serde_json::Value;
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

/// Registration failed: the plugin is listed as inactive with this message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterError(pub String);

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RegisterError {}

impl From<String> for RegisterError {
    fn from(s: String) -> Self {
        RegisterError(s)
    }
}

impl From<&str> for RegisterError {
    fn from(s: &str) -> Self {
        RegisterError(s.into())
    }
}

/// A plugin. Its process serves it with [`crate::serve::serve`]; its permissions are not here but in its manifest, which
/// the app verified before it started the process.
pub trait Plugin: Send + Sync + 'static {
    /// Stable, unique id, reverse-DNS style (`net.example.registry`); the same as the manifest's.
    fn id(&self) -> &str;
    /// Display name shown in the plugin list.
    fn name(&self) -> &str;
    /// Semver version of the plugin.
    fn version(&self) -> &str;
    /// Adds the plugin's contributions. Return an error instead of panicking.
    fn register(&self, c: &mut Contributions) -> Result<(), RegisterError>;
    /// Memory it needs at most, in bytes (`None`: the app's default).
    fn memory_limit(&self) -> Option<u64> {
        None
    }
}

/// What a view call tells the view.
pub struct ViewCtx {
    /// What the app passes (selection, page, ...), a JSON object.
    pub args: Value,
    /// The app's services (host files, reports).
    pub tool: ToolCtx,
}

/// A view a plugin contributes (ADR 0002): it describes, the app renders. Every text of a [`ViewDoc`] is bound as text,
/// never as HTML.
pub trait View: Send + Sync {
    /// What the app lists.
    fn info(&self) -> ViewInfo;
    /// The document for the current state; validated (`paddock_view::validate`) before it leaves the plugin.
    fn render(&self, ctx: &ViewCtx) -> Result<ViewDoc, ToolError>;
    /// Handles the user's action and answers what changed.
    fn action(&self, ctx: &ViewCtx, action: &ViewAction) -> Result<ViewPatch, ToolError>;
}

/// An MCP tool a plugin contributes. Read-only by contract: writes are for the UI and the command line.
pub trait McpTool: Send + Sync {
    /// Name, description and argument schema.
    fn info(&self) -> McpToolInfo;
    /// Answers within `budget`; an answer over `budget.max_bytes` is refused by the platform.
    fn call(&self, args: Value, budget: McpBudget, ctx: &ToolCtx) -> Result<Value, ToolError>;
}

/// Everything a plugin contributes.
#[derive(Default)]
pub struct Contributions {
    pub(crate) tools: Vec<(ToolInfo, Arc<dyn Tool>)>,
    pub(crate) settings: Option<Arc<dyn Configurable>>,
    pub(crate) views: Vec<(ViewInfo, Arc<dyn View>)>,
    pub(crate) mcp: Vec<(McpToolInfo, Arc<dyn McpTool>)>,
    pub(crate) extensions: Vec<Registered>,
}

impl Contributions {
    /// Adds a tool; its description is taken now.
    pub fn add_tool(&mut self, tool: impl Tool + 'static) -> &mut Self {
        let info = tool.describe();
        self.tools.push((info, Arc::new(tool)));
        self
    }

    /// The plugin's settings (one section); the app validates values against its schema before they arrive.
    pub fn set_settings(&mut self, settings: Arc<dyn Configurable>) -> &mut Self {
        self.settings = Some(settings);
        self
    }

    /// Adds a view.
    pub fn add_view(&mut self, view: impl View + 'static) -> &mut Self {
        let info = view.info();
        self.views.push((info, Arc::new(view)));
        self
    }

    /// Adds an MCP tool.
    pub fn add_mcp_tool(&mut self, tool: impl McpTool + 'static) -> &mut Self {
        let info = tool.info();
        self.mcp.push((info, Arc::new(tool)));
        self
    }

    /// Speaks the app contract `C`: `handler` answers its requests.
    pub fn add_extension<C: Contract>(&mut self, handler: impl ExtensionHandler<C>) -> &mut Self {
        self.extensions.push(Registered::new::<C>(handler));
        self
    }

    /// Checks the contributions against the rules the app enforces: unique valid ids, defined schemas, no namespace twice.
    pub fn check(&self) -> Result<(), RegisterError> {
        let mut seen = HashSet::new();
        for (info, _) in &self.tools {
            info.check()?;
            if !seen.insert(format!("tool:{}", info.id)) {
                return Err(format!("tool \"{}\" is registered twice", info.id).into());
            }
        }
        for (info, _) in &self.views {
            check_id("view id", &info.id)?;
            if !seen.insert(format!("view:{}", info.id)) {
                return Err(format!("view \"{}\" is registered twice", info.id).into());
            }
        }
        for (info, _) in &self.mcp {
            check_id("MCP tool name", &info.name)?;
            if !seen.insert(format!("mcp:{}", info.name)) {
                return Err(format!("MCP tool \"{}\" is registered twice", info.name).into());
            }
        }
        for e in &self.extensions {
            if !seen.insert(format!("ext:{}", e.ns)) {
                return Err(format!("contract \"{}\" is registered twice", e.ns).into());
            }
        }
        if let Some(s) = &self.settings {
            s.section()
                .check()
                .map_err(|e| RegisterError(format!("settings: {e}")))?;
        }
        Ok(())
    }

    /// What `plugin` says about itself on `Describe`.
    pub fn describe(&self, plugin: &dyn Plugin) -> Described {
        Described {
            id: plugin.id().into(),
            name: plugin.name().into(),
            version: plugin.version().into(),
            protocol: PLATFORM_PROTOCOL,
            memory_limit: plugin.memory_limit(),
            tools: self.tools.iter().map(|(i, _)| i.clone()).collect(),
            settings: self.settings.as_ref().map(|s| s.section()),
            views: self.views.iter().map(|(i, _)| i.clone()).collect(),
            mcp_tools: self.mcp.iter().map(|(i, _)| i.clone()).collect(),
            extensions: self
                .extensions
                .iter()
                .map(|e| ExtensionInfo {
                    ns: e.ns.into(),
                    minor: e.minor,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::McpBudget;
    use crate::tools::{OpSpec, ToolCtx};
    use paddock_settings::{FieldKind, SectionOwner, SettingField, SettingsSection};

    struct T(&'static str);
    impl Tool for T {
        fn id(&self) -> &str {
            self.0
        }
        fn describe(&self) -> ToolInfo {
            ToolInfo::new(self.0, "T").op(OpSpec::new("o", "O"))
        }
        fn invoke(&self, _: &str, _: Value, _: &ToolCtx) -> Result<Value, ToolError> {
            Ok(Value::Null)
        }
    }
    struct V(&'static str);
    impl View for V {
        fn info(&self) -> ViewInfo {
            ViewInfo {
                id: self.0.into(),
                title: "V".into(),
                writes: false,
            }
        }
        fn render(&self, _: &ViewCtx) -> Result<ViewDoc, ToolError> {
            Err(ToolError::Cancelled)
        }
        fn action(&self, _: &ViewCtx, _: &ViewAction) -> Result<ViewPatch, ToolError> {
            Err(ToolError::Cancelled)
        }
    }
    struct M(&'static str);
    impl McpTool for M {
        fn info(&self) -> McpToolInfo {
            McpToolInfo {
                name: self.0.into(),
                description: String::new(),
                args: crate::tools::Schema::object(),
            }
        }
        fn call(&self, _: Value, _: McpBudget, _: &ToolCtx) -> Result<Value, ToolError> {
            Ok(Value::Null)
        }
    }
    struct Bad(SettingsSection);
    impl Configurable for Bad {
        fn section(&self) -> SettingsSection {
            self.0.clone()
        }
        fn apply(
            &self,
            _: &paddock_settings::ValidValues,
        ) -> Result<(), paddock_settings::service::ApplyError> {
            Ok(())
        }
    }
    struct P;
    impl Plugin for P {
        fn id(&self) -> &str {
            "net.example.p"
        }
        fn name(&self) -> &str {
            "P"
        }
        fn version(&self) -> &str {
            "1.0.0"
        }
        fn register(&self, _: &mut Contributions) -> Result<(), RegisterError> {
            Ok(())
        }
    }

    #[test]
    fn duplicate_ids_bad_ids_and_bad_settings_are_refused_at_registration() {
        let mut c = Contributions::default();
        c.add_tool(T("a")).add_view(V("v")).add_mcp_tool(M("m"));
        c.check().unwrap();
        let d = c.describe(&P);
        assert_eq!(
            (
                d.id.as_str(),
                d.tools.len(),
                d.views.len(),
                d.mcp_tools.len(),
                d.memory_limit
            ),
            ("net.example.p", 1, 1, 1, None)
        );
        assert!(d.settings.is_none() && d.extensions.is_empty());
        let twice = |f: &dyn Fn(&mut Contributions), what: &str| {
            let mut c = Contributions::default();
            f(&mut c);
            assert!(c.check().unwrap_err().0.contains(what), "{what}");
        };
        twice(
            &|c| {
                c.add_tool(T("a")).add_tool(T("a"));
            },
            "tool \"a\" is registered twice",
        );
        twice(
            &|c| {
                c.add_view(V("v")).add_view(V("v"));
            },
            "view \"v\" is registered twice",
        );
        twice(
            &|c| {
                c.add_mcp_tool(M("m")).add_mcp_tool(M("m"));
            },
            "MCP tool \"m\" is registered twice",
        );
        twice(
            &|c| {
                c.add_view(V("bad id"));
            },
            "view id",
        );
        twice(
            &|c| {
                c.add_mcp_tool(M("bad id"));
            },
            "MCP tool name",
        );
        twice(
            &|c| {
                c.add_tool(T("bad id"));
            },
            "tool id",
        );
        twice(
            &|c| {
                c.add_extension::<crate::extension::tests_support::Calc>(
                    crate::extension::tests_support::echo,
                )
                .add_extension::<crate::extension::tests_support::Calc>(
                    crate::extension::tests_support::echo,
                );
            },
            "contract \"test.calc.v1\" is registered twice",
        );
        let broken = SettingsSection::new("s", "S", SectionOwner::App).field(SettingField::new(
            "k",
            "K",
            FieldKind::Int {
                min: 5,
                max: 1,
                step: 1,
                unit: None,
            },
            3,
        ));
        twice(
            &|c| {
                c.set_settings(Arc::new(Bad(broken.clone())));
            },
            "settings:",
        );
        let mut with_extension = Contributions::default();
        with_extension.add_extension::<crate::extension::tests_support::Calc>(
            crate::extension::tests_support::echo,
        );
        assert_eq!(
            with_extension.describe(&P).extensions,
            [ExtensionInfo {
                ns: "test.calc.v1".into(),
                minor: 1
            }]
        );
    }

    #[test]
    fn the_stubs_behave_as_the_checks_above_assume() {
        let ctx = ToolCtx::new("t", None, Arc::new(crate::tools::NoHost));
        assert_eq!(T("a").invoke("o", Value::Null, &ctx), Ok(Value::Null));
        assert_eq!(T("a").id(), "a");
        let vctx = ViewCtx {
            args: Value::Null,
            tool: ctx.clone(),
        };
        assert!(V("v").render(&vctx).is_err());
        let action = ViewAction {
            view: "v".into(),
            action: "a".into(),
            args: paddock_ipc::Json(Value::Null),
        };
        assert!(V("v").action(&vctx, &action).is_err());
        assert_eq!(
            M("m").call(
                Value::Null,
                McpBudget {
                    max_bytes: 1,
                    max_ms: 1
                },
                &ctx
            ),
            Ok(Value::Null)
        );
        let section = SettingsSection::new("s", "S", SectionOwner::App);
        let bad = Bad(section);
        assert!(bad.apply(&paddock_settings::ValidValues::default()).is_ok());
        let mut c = Contributions::default();
        assert!(P.register(&mut c).is_ok());
        assert!(P.memory_limit().is_none());
    }

    #[test]
    fn registration_errors_convert_from_text() {
        assert_eq!(RegisterError::from("x"), RegisterError("x".into()));
        assert_eq!(RegisterError::from(String::from("y")).to_string(), "y");
    }
}
