//! `run` and `test`: start the built plugin program the way the app does, against a fake host.
//!
//! `run` prints what the plugin describes. `test` runs the platform's conformance checks (`paddock_plugin_test::conformance`)
//! and compares what the plugin describes with its `manifest.toml`; any finding ends with exit 5 so CI can gate on it.

use crate::Context;
use crate::args::CheckArgs;
use crate::build;
use crate::project::Project;
use paddock_cli::{CliError, Exit, Report};
use paddock_plugin_api::Described;
use paddock_plugin_api::IpcError;
use paddock_plugin_package::manifest::host_target;
use paddock_plugin_package::{Contribution, Manifest};
use paddock_plugin_test::FakeHost;
use paddock_plugin_test::conformance::{self, Finding};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The program to start: `--program`, else the build of this computer's target (made first unless `--no-build`).
fn program(ctx: &Context, project: &Project, a: &CheckArgs) -> Result<PathBuf, CliError> {
    if let Some(p) = &a.program {
        let p = ctx.cwd.join(p);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(
                CliError::failure(format!("{} does not exist.", p.display())).with_hint(
                    "Check the path of --program, or leave it out to build the project.",
                ),
            )
        };
    }
    if !a.no_build {
        let steps = build::plan(project, &[])?;
        let mut built = build::run_steps(ctx, project, &steps)?;
        return Ok(built.remove(0).1);
    }
    project.find_built(host_target()).ok_or_else(|| {
        CliError::failure("the plugin has not been built.")
            .with_hint("Run `cargo paddock-plugin build`, or leave out --no-build.")
    })
}

/// Starts `program` and asks it to describe itself, then runs `with` against the live plugin.
fn with_plugin<T>(
    program: &Path,
    with: impl FnOnce(&FakeHost, Described) -> T,
) -> Result<T, CliError> {
    let host = FakeHost::launch(program, &[]).map_err(|e| match e {
        // Somebody must update: the text says who.
        IpcError::Version(m) => CliError::refused(m).with_hint("Rebuild the plugin against the current paddock-plugin-api."),
        other => CliError::failure(format!("the plugin could not be started ({other})."))
            .with_hint("It must call paddock_plugin_api::serve in main, and print nothing to its standard input."),
    })?;
    let described = host.describe().map_err(|e| {
        CliError::failure(format!("the plugin did not describe itself ({e}).")).with_hint(
            "`main` must be `paddock_plugin_api::serve(...)` and register must not fail.",
        )
    })?;
    Ok(with(&host, described))
}

/// The differences between what the plugin describes and what its manifest declares.
pub fn manifest_findings(manifest: &Manifest, d: &Described) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut add = |problem: String| {
        out.push(Finding {
            check: "manifest",
            problem,
        })
    };
    if d.id != manifest.id.as_str() {
        add(format!(
            "the plugin says its id is {} but manifest.toml says {}",
            d.id, manifest.id
        ));
    }
    if d.version != manifest.version.to_string() {
        add(format!(
            "the plugin says its version is {} but manifest.toml says {}",
            d.version, manifest.version
        ));
    }
    if d.protocol.to_string() != manifest.protocol.to_string() {
        add(format!(
            "the plugin was built for protocol {} but manifest.toml says {}",
            d.protocol, manifest.protocol
        ));
    }
    let declared = |pick: fn(&Contribution) -> Option<&str>| -> BTreeSet<String> {
        manifest
            .contributions
            .iter()
            .filter_map(pick)
            .map(String::from)
            .collect()
    };
    let compare = |what: &str,
                   declared: BTreeSet<String>,
                   described: BTreeSet<String>,
                   add: &mut dyn FnMut(String)| {
        for id in declared.difference(&described) {
            add(format!(
                "manifest.toml declares the {what} {id}, which the plugin does not contribute"
            ));
        }
        for id in described.difference(&declared) {
            add(format!(
                "the plugin contributes the {what} {id}, which manifest.toml does not declare"
            ));
        }
    };
    compare(
        "tool",
        declared(|c| match c {
            Contribution::Tool { id, .. } => Some(id),
            _ => None,
        }),
        d.tools.iter().map(|t| t.id.clone()).collect(),
        &mut add,
    );
    compare(
        "view",
        declared(|c| match c {
            Contribution::View { id, .. } => Some(id),
            _ => None,
        }),
        d.views.iter().map(|v| v.id.clone()).collect(),
        &mut add,
    );
    compare(
        "MCP tool",
        declared(|c| match c {
            Contribution::McpTool { id } => Some(id),
            _ => None,
        }),
        d.mcp_tools.iter().map(|t| t.name.clone()).collect(),
        &mut add,
    );
    out
}

/// What a plugin describes, for people.
fn summary(d: &Described) -> String {
    let list = |what: &str, items: Vec<String>| {
        if items.is_empty() {
            format!("{what}: none")
        } else {
            format!("{what}: {}", items.join(", "))
        }
    };
    [
        format!(
            "{} {} ({}), platform protocol {}",
            d.name, d.version, d.id, d.protocol
        ),
        list("tools", d.tools.iter().map(|t| t.id.clone()).collect()),
        list("views", d.views.iter().map(|v| v.id.clone()).collect()),
        list(
            "MCP tools",
            d.mcp_tools.iter().map(|t| t.name.clone()).collect(),
        ),
        list(
            "contracts",
            d.extensions
                .iter()
                .map(|e| format!("{}/{}", e.ns, e.minor))
                .collect(),
        ),
        format!(
            "settings: {}",
            d.settings.as_ref().map_or("none", |s| s.id.as_str())
        ),
    ]
    .join("\n")
}

/// `run`.
///
/// # Errors
///
/// When the program cannot be built or started or does not describe itself.
pub fn run(ctx: &Context, a: &CheckArgs) -> Result<Report, CliError> {
    let project = Project::load(&ctx.dir(a.dir.as_deref()))?;
    let program = program(ctx, &project, a)?;
    (ctx.note)(&format!("Starting {}...", program.display()));
    let d = with_plugin(&program, |_, d| d)?;
    Ok(Report::new(
        json!({"program": program.to_string_lossy(), "described": serde_json::to_value(&d).unwrap_or_default()}),
        summary(&d),
    ))
}

/// `test`.
///
/// # Errors
///
/// When the program cannot be built or started. Findings are not errors: they end the report with exit 5.
pub fn test(ctx: &Context, a: &CheckArgs) -> Result<Report, CliError> {
    let project = Project::load(&ctx.dir(a.dir.as_deref()))?;
    let program = program(ctx, &project, a)?;
    (ctx.note)(&format!(
        "Checking {} against a fake host...",
        program.display()
    ));
    let findings = with_plugin(&program, |host, d| {
        let mut findings = manifest_findings(&project.manifest, &d);
        findings.extend(conformance::run(host));
        findings
    })?;
    let rows: Vec<_> = findings
        .iter()
        .map(|f| json!({"check": f.check, "problem": f.problem}))
        .collect();
    let (text, exit) = if findings.is_empty() {
        (
            format!("{} conforms: no findings", project.manifest.id),
            Exit::Ok,
        )
    } else {
        let lines: Vec<String> = findings
            .iter()
            .map(|f| format!("[{}] {}", f.check, f.problem))
            .collect();
        (
            format!(
                "{} finding(s) in {}:\n{}",
                findings.len(),
                project.manifest.id,
                lines.join("\n")
            ),
            Exit::Findings,
        )
    };
    Ok(Report::new(
        json!({"program": program.to_string_lossy(), "findings": rows, "count": findings.len()}),
        text,
    )
    .with_exit(exit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::project;
    use crate::testutil::{FakeCargo, context};
    use paddock_plugin_api::protocol::PLATFORM_PROTOCOL;
    use paddock_plugin_api::tools::{OpSpec, ToolInfo};
    use paddock_plugin_test::fixture;

    fn described() -> Described {
        Described {
            id: fixture::ID.into(),
            name: "Fixture".into(),
            version: "0.1.0".into(),
            protocol: PLATFORM_PROTOCOL,
            memory_limit: None,
            tools: vec![ToolInfo::new(fixture::TOOL, "T").op(OpSpec::new("echo", "E"))],
            settings: None,
            views: vec![paddock_plugin_api::protocol::ViewInfo {
                id: fixture::VIEW.into(),
                title: "V".into(),
                writes: false,
            }],
            mcp_tools: vec![paddock_plugin_api::protocol::McpToolInfo {
                name: fixture::MCP_TOOL.into(),
                description: "d".into(),
                args: paddock_plugin_api::tools::Schema::object(),
            }],
            extensions: vec![],
        }
    }

    fn manifest() -> Manifest {
        fixture::manifest("2.0", "prog", "")
    }

    #[test]
    fn what_the_plugin_describes_is_compared_with_the_manifest() {
        assert_eq!(manifest_findings(&manifest(), &described()), vec![]);
        let mut d = described();
        d.id = "other.id".into();
        d.version = "9.9.9".into();
        d.protocol = paddock_plugin_api::ProtocolVersion::new(1, 0);
        d.tools = vec![ToolInfo::new("extra.tool", "T")];
        d.views.clear();
        d.mcp_tools.clear();
        let problems: Vec<String> = manifest_findings(&manifest(), &d)
            .into_iter()
            .map(|f| f.problem)
            .collect();
        assert_eq!(problems.len(), 7, "{problems:#?}");
        assert!(problems.iter().any(|p| p.contains("id is other.id")));
        assert!(problems.iter().any(|p| p.contains("version is 9.9.9")));
        assert!(problems.iter().any(|p| p.contains("protocol 1.0")));
        assert!(
            problems
                .iter()
                .any(|p| p.contains("declares the tool fixture.tool"))
        );
        assert!(
            problems
                .iter()
                .any(|p| p.contains("contributes the tool extra.tool"))
        );
        assert!(problems.iter().any(|p| p.contains("declares the view")));
        assert!(problems.iter().any(|p| p.contains("declares the MCP tool")));
    }

    #[test]
    fn the_summary_lists_what_is_contributed() {
        let s = summary(&described());
        assert!(s.contains("Fixture 0.1.0 (net.example.fixture), platform protocol"));
        assert!(
            s.contains("tools: fixture.tool")
                && s.contains("contracts: none")
                && s.contains("settings: none")
        );
        let mut d = described();
        d.extensions = vec![paddock_plugin_api::protocol::ExtensionInfo {
            ns: "a.b".into(),
            minor: 3,
        }];
        assert!(summary(&d).contains("contracts: a.b/3"));
    }

    #[test]
    fn the_program_is_the_given_one_the_built_one_or_a_clear_refusal() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), "prog");
        let cargo = FakeCargo::making(&p);
        let (ctx, _) = context(dir.path(), &cargo, false);
        let a = CheckArgs {
            dir: None,
            program: None,
            no_build: true,
        };
        let e = program(&ctx, &p, &a).unwrap_err();
        assert!(
            e.message.contains("not been built") && e.hint.contains("build"),
            "{e:?}"
        );
        assert!(cargo.calls().is_empty());
        let built = program(
            &ctx,
            &p,
            &CheckArgs {
                no_build: false,
                ..a.clone()
            },
        )
        .unwrap();
        assert!(built.is_file() && cargo.calls().len() == 1);
        assert_eq!(program(&ctx, &p, &a).unwrap(), built);
        let given = CheckArgs {
            program: Some("nowhere".into()),
            ..a.clone()
        };
        assert!(
            program(&ctx, &p, &given)
                .unwrap_err()
                .message
                .contains("does not exist")
        );
        std::fs::write(dir.path().join("mine"), "x").unwrap();
        let given = CheckArgs {
            program: Some("mine".into()),
            ..a
        };
        assert_eq!(program(&ctx, &p, &given).unwrap(), dir.path().join("mine"));
    }

    #[test]
    fn a_program_that_cannot_start_is_a_failure_with_a_hint() {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path(), "prog");
        let (ctx, _) = context(dir.path(), FakeCargo::making_nothing(), false);
        // A file that is not a program.
        std::fs::write(dir.path().join("junk"), "not a program").unwrap();
        for command in [run, test] {
            let a = CheckArgs {
                dir: None,
                program: Some("junk".into()),
                no_build: true,
            };
            let e = command(&ctx, &a).unwrap_err();
            assert!(
                e.message.contains("could not be started") && e.hint.contains("serve"),
                "{e:?}"
            );
        }
    }
}
