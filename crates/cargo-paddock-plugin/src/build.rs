//! `build`: `cargo build --release` for this computer or for each `--target`.

use crate::Context;
use crate::args::BuildArgs;
use crate::project::Project;
use paddock_cli::{CliError, Report};
use paddock_plugin_package::manifest::{host_target, valid_target};
use serde_json::json;
use std::path::PathBuf;

/// One `cargo build`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The target triple the program is for.
    pub target: String,
    /// Whether `--target` is passed (the host build is not, so a plain `cargo build` cache is reused).
    pub explicit: bool,
    /// The arguments of `cargo`.
    pub args: Vec<String>,
}

/// The builds `targets` ask for (none: this computer).
///
/// # Errors
///
/// When a target is not a target triple.
pub fn plan(project: &Project, targets: &[String]) -> Result<Vec<Step>, CliError> {
    let wanted: Vec<(String, bool)> = if targets.is_empty() {
        vec![(host_target().to_string(), false)]
    } else {
        targets.iter().map(|t| (t.clone(), true)).collect()
    };
    wanted
        .into_iter()
        .map(|(target, explicit)| {
            if !valid_target(&target) {
                return Err(CliError::usage(format!("\"{target}\" is not a target triple."))
                    .with_hint("Write for example x86_64-unknown-linux-gnu; `rustc --print target-list` lists them."));
            }
            let mut args: Vec<String> = [
                "build",
                "--release",
                "--bin",
                &project.manifest.binary,
                "--manifest-path",
                &project.dir.join("Cargo.toml").to_string_lossy(),
                "--target-dir",
                &project.target_dir().to_string_lossy(),
            ]
            .map(String::from)
            .into();
            if explicit {
                args.extend(["--target".into(), target.clone()]);
            }
            Ok(Step { target, explicit, args })
        })
        .collect()
}

/// Builds the program for each step and returns where each one is.
///
/// # Errors
///
/// When cargo fails, or does not leave the program where the manifest says it is called.
pub fn run_steps(
    ctx: &Context,
    project: &Project,
    steps: &[Step],
) -> Result<Vec<(String, PathBuf)>, CliError> {
    let mut built = Vec::new();
    for step in steps {
        (ctx.note)(&format!(
            "Building {} for {}...",
            project.manifest.binary, step.target
        ));
        let mut args = step.args.clone();
        if ctx.quiet {
            args.push("--quiet".into());
        }
        ctx.cargo.run(&args, &project.dir).map_err(|m| {
            let error = CliError::failure(m);
            if step.explicit {
                error.with_hint(format!(
                    "A target other than this computer's needs it installed: `rustup target add {}`.",
                    step.target
                ))
            } else {
                error.with_hint("Fix the errors above, then build again.")
            }
        })?;
        let path = project.built(&step.target, step.explicit);
        if !path.is_file() {
            return Err(CliError::failure(format!("cargo did not make {}.", path.display()))
                .with_hint(format!(
                    "The program is named after `binary = \"{0}\"` in manifest.toml; make that the crate's binary (name = \"{0}\" in Cargo.toml or a [[bin]]).",
                    project.manifest.binary
                )));
        }
        built.push((step.target.clone(), path));
    }
    Ok(built)
}

/// `build`.
///
/// # Errors
///
/// See [`plan`] and [`run_steps`].
pub fn build(ctx: &Context, a: &BuildArgs) -> Result<Report, CliError> {
    let project = Project::load(&ctx.dir(a.dir.as_deref()))?;
    let steps = plan(&project, &a.targets)?;
    let built = run_steps(ctx, &project, &steps)?;
    let text = built
        .iter()
        .map(|(t, p)| format!("built {t}: {}", p.display()))
        .collect::<Vec<_>>()
        .join("\n");
    let programs: Vec<_> = built
        .iter()
        .map(|(t, p)| json!({"target": t, "path": p.to_string_lossy()}))
        .collect();
    Ok(Report::new(json!({"programs": programs}), text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::project;
    use crate::testutil::FakeCargo;

    #[test]
    fn the_host_build_has_no_target_flag_and_others_have() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), "prog");
        let host = plan(&p, &[]).unwrap();
        assert_eq!(host.len(), 1);
        assert!(!host[0].explicit && !host[0].args.contains(&"--target".to_string()));
        assert_eq!(host[0].args[..4], ["build", "--release", "--bin", "prog"]);
        assert!(host[0].args.iter().any(|a| a.ends_with("target")));
        let two = plan(
            &p,
            &[
                "aarch64-apple-darwin".into(),
                "x86_64-pc-windows-msvc".into(),
            ],
        )
        .unwrap();
        assert_eq!(two.len(), 2);
        assert!(
            two[0].explicit
                && two[0]
                    .args
                    .ends_with(&["--target".into(), "aarch64-apple-darwin".into()])
        );
        let e = plan(&p, &["Nonsense".into()]).unwrap_err();
        assert_eq!(e.exit, paddock_cli::Exit::Usage);
        assert!(e.message.contains("not a target triple"));
    }

    #[test]
    fn building_runs_cargo_and_reports_where_the_program_is() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), "prog");
        let cargo = FakeCargo::making(&p);
        let (ctx, notes) = crate::testutil::context(dir.path(), &cargo, false);
        let a = BuildArgs {
            dir: None,
            targets: vec![],
        };
        let report = build(&ctx, &a).unwrap();
        assert!(report.text.starts_with("built "), "{}", report.text);
        assert_eq!(report.data["programs"][0]["target"], host_target());
        assert_eq!(cargo.calls().len(), 1);
        assert!(
            notes
                .borrow()
                .iter()
                .any(|n| n.starts_with("Building prog"))
        );
        // Quiet passes --quiet on to cargo.
        let (ctx, _) = crate::testutil::context(dir.path(), &cargo, true);
        build(&ctx, &a).unwrap();
        assert_eq!(cargo.calls()[1].last().map(String::as_str), Some("--quiet"));
    }

    #[test]
    fn a_failed_build_or_a_missing_program_says_what_to_fix() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), "prog");
        let a = BuildArgs {
            dir: None,
            targets: vec!["aarch64-apple-darwin".into()],
        };
        let failing = FakeCargo::failing("cargo build failed (exit status: 101)");
        let (ctx, _) = crate::testutil::context(dir.path(), &failing, false);
        let e = build(&ctx, &a).unwrap_err();
        assert!(
            e.message.contains("cargo build failed")
                && e.hint.contains("rustup target add aarch64-apple-darwin"),
            "{e:?}"
        );
        let host_failing = FakeCargo::failing("cargo build failed (exit status: 101)");
        let (ctx, _) = crate::testutil::context(dir.path(), &host_failing, false);
        let e = build(
            &ctx,
            &BuildArgs {
                dir: None,
                targets: vec![],
            },
        )
        .unwrap_err();
        assert!(
            e.hint.contains("Fix the errors") && !e.hint.contains("rustup"),
            "{e:?}"
        );
        let lazy = FakeCargo::making_nothing();
        let (ctx, _) = crate::testutil::context(dir.path(), lazy, false);
        let e = build(
            &ctx,
            &BuildArgs {
                dir: None,
                targets: vec![],
            },
        )
        .unwrap_err();
        assert!(
            e.message.contains("did not make") && e.hint.contains("binary = \"prog\""),
            "{e:?}"
        );
        drop(p);
        let (ctx, _) = crate::testutil::context(&dir.path().join("nowhere"), lazy, false);
        assert!(
            build(
                &ctx,
                &BuildArgs {
                    dir: None,
                    targets: vec![]
                }
            )
            .is_err()
        );
    }
}
