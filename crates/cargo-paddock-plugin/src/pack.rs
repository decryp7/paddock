//! `pack`: the deterministic `.paddockplugin` (manifest, `bin/<target>/<program>`, `SHA256SUMS`, optional signature).

use crate::Context;
use crate::args::PackArgs;
use crate::errors::package;
use crate::project::Project;
use paddock_cli::{CliError, Report};
use paddock_plugin_package::archive::{Build, build};
use serde_json::json;
use std::path::{Path, PathBuf};

/// What to pack, decided before anything is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The targets, in manifest order, and the program for each.
    pub programs: Vec<(String, PathBuf)>,
    /// The package file.
    pub out: PathBuf,
}

/// Decides the targets, their programs and the package file.
///
/// # Errors
///
/// When a requested target is not in the manifest, a program is missing (the hint says how to make it), or `--program` names
/// a target that is not packed.
pub fn plan(project: &Project, a: &PackArgs, cwd: &Path) -> Result<Plan, CliError> {
    let listed = &project.manifest.targets;
    let mut selected: Vec<String> = Vec::new();
    for t in &a.targets {
        if !listed.contains(t) {
            return Err(
                CliError::usage(format!("manifest.toml does not list the target {t}.")).with_hint(
                    format!(
                        "It lists: {}. Add the target to `targets` there first.",
                        listed.join(", ")
                    ),
                ),
            );
        }
        if !selected.contains(t) {
            selected.push(t.clone());
        }
    }
    if selected.is_empty() {
        selected.clone_from(listed);
    }
    // In manifest order, so the package does not depend on the order of the flags.
    selected.sort_by_key(|t| listed.iter().position(|l| l == t));
    if let Some((t, _)) = a.programs.iter().find(|(t, _)| !selected.contains(t)) {
        return Err(
            CliError::usage(format!("--program names {t}, which is not being packed."))
                .with_hint(format!("Packed targets: {}.", selected.join(", "))),
        );
    }
    let mut programs = Vec::new();
    for t in selected {
        let given = a.programs.iter().rev().find(|(pt, _)| *pt == t);
        let path = match given {
            Some((_, p)) => {
                let p = cwd.join(p);
                if !p.is_file() {
                    return Err(CliError::failure(format!("the program for {t} ({}) does not exist.", p.display()))
                        .with_hint("Check the path of --program."));
                }
                p
            }
            None => project.find_built(&t).ok_or_else(|| {
                CliError::failure(format!("the program for {t} has not been built."))
                    .with_hint(format!("Run `cargo paddock-plugin build --target {t}`, or pass --program {t}=<file>."))
            })?,
        };
        programs.push((t, path));
    }
    let out = match &a.out {
        Some(o) => cwd.join(o),
        None => project.dir.join("dist").join(format!(
            "{}-{}.paddockplugin",
            project.manifest.id, project.manifest.version
        )),
    };
    Ok(Plan { programs, out })
}

/// `pack`.
///
/// # Errors
///
/// See [`plan`]; also when the key cannot be read or the package cannot be written.
pub fn pack(ctx: &Context, a: &PackArgs) -> Result<Report, CliError> {
    let project = Project::load(&ctx.dir(a.dir.as_deref()))?;
    let plan = plan(&project, a, &ctx.cwd)?;
    let key = a
        .key
        .as_ref()
        .map(|k| crate::keys::load_key(&ctx.cwd.join(k)))
        .transpose()?;
    let parent = plan.out.parent().unwrap_or(&ctx.cwd);
    std::fs::create_dir_all(parent).map_err(|e| {
        CliError::failure(format!("{} cannot be made ({e}).", parent.display()))
            .with_hint("Choose another --out.")
    })?;
    (ctx.note)(&format!("Packing {}...", plan.out.display()));
    let mut manifest = project.manifest.clone();
    manifest.targets = plan.programs.iter().map(|(t, _)| t.clone()).collect();
    let digest = build(
        Build {
            manifest,
            programs: plan
                .programs
                .iter()
                .map(|(t, p)| (t.clone(), p.as_path()))
                .collect(),
            sign: key.as_ref(),
        },
        &plan.out,
    )
    .map_err(|e| package(&e))?;
    let targets: Vec<&str> = plan.programs.iter().map(|(t, _)| t.as_str()).collect();
    let signed = key.as_ref().map(|k| k.public().id());
    let text = format!(
        "packed {} ({}, {})\ndigest {digest}",
        plan.out.display(),
        targets.join(", "),
        signed
            .as_ref()
            .map_or("unsigned".to_string(), |k| format!("signed with key {k}")),
    );
    Ok(Report::new(
        json!({"package": plan.out.to_string_lossy(), "digest": digest, "targets": targets, "signed": signed.is_some(), "key": signed}),
        text,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::project;
    use paddock_plugin_package::manifest::host_target;
    use paddock_plugin_package::{Package, SecretKey, Signature};

    fn args() -> PackArgs {
        PackArgs {
            dir: None,
            targets: vec![],
            programs: vec![],
            key: None,
            out: None,
        }
    }

    fn built(p: &Project) -> PathBuf {
        let path = p.built(host_target(), false);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"program bytes").unwrap();
        path
    }

    #[test]
    fn the_plan_takes_the_manifests_targets_and_says_what_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), "prog");
        let e = plan(&p, &args(), dir.path()).unwrap_err();
        assert!(
            e.message.contains("has not been built") && e.hint.contains("build --target"),
            "{e:?}"
        );
        let path = built(&p);
        let plan1 = plan(&p, &args(), dir.path()).unwrap();
        assert_eq!(
            plan1.programs,
            vec![(host_target().to_string(), path.clone())]
        );
        assert!(
            plan1
                .out
                .ends_with(Path::new("dist").join("net.example.fixture-0.1.0.paddockplugin"))
        );
        // Explicit programs win; relative ones are read from the working folder.
        let other = dir.path().join("other-prog");
        std::fs::write(&other, b"y").unwrap();
        let mut a = args();
        a.programs = vec![(host_target().into(), PathBuf::from("other-prog"))];
        a.targets = vec![host_target().into(), host_target().into()];
        a.out = Some("x.paddockplugin".into());
        let plan2 = plan(&p, &a, dir.path()).unwrap();
        assert_eq!(plan2.programs[0].1, other);
        assert_eq!(plan2.out, dir.path().join("x.paddockplugin"));
        a.programs = vec![(host_target().into(), PathBuf::from("missing"))];
        assert!(
            plan(&p, &a, dir.path())
                .unwrap_err()
                .message
                .contains("does not exist")
        );
        a.programs = vec![("a-b-c".into(), PathBuf::from("other-prog"))];
        assert!(
            plan(&p, &a, dir.path())
                .unwrap_err()
                .message
                .contains("not being packed")
        );
        a.programs = vec![];
        a.targets = vec!["a-b-c".into()];
        let e = plan(&p, &a, dir.path()).unwrap_err();
        assert_eq!(e.exit, paddock_cli::Exit::Usage);
        assert!(e.hint.contains(host_target()));
    }

    #[test]
    fn the_package_is_deterministic_and_signed_on_request() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), "prog");
        built(&p);
        let (ctx, _) = crate::testutil::context(
            dir.path(),
            crate::testutil::FakeCargo::making_nothing(),
            false,
        );
        let first = pack(&ctx, &args()).unwrap();
        let out = PathBuf::from(first.data["package"].as_str().unwrap());
        let bytes = std::fs::read(&out).unwrap();
        let again = pack(&ctx, &args()).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), bytes);
        assert_eq!(first.data["digest"], again.data["digest"]);
        assert_eq!(first.data["signed"], false);
        let pkg = Package::open(&out).unwrap();
        assert_eq!(pkg.signature, Signature::Unsigned);
        assert!(pkg.program(host_target()).is_some());
        // Signed.
        let key = SecretKey::generate_to(&dir.path().join("k.key")).unwrap();
        let mut a = args();
        a.key = Some("k.key".into());
        a.out = Some("signed.paddockplugin".into());
        let signed = pack(&ctx, &a).unwrap();
        assert_eq!(signed.data["key"], key.public().id());
        assert!(signed.text.contains("signed with key"));
        assert_eq!(
            Package::open(&dir.path().join("signed.paddockplugin"))
                .unwrap()
                .signature,
            Signature::Signed { key: key.public() }
        );
    }

    #[test]
    fn a_bad_key_or_an_unwritable_place_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), "prog");
        built(&p);
        let (ctx, _) = crate::testutil::context(
            dir.path(),
            crate::testutil::FakeCargo::making_nothing(),
            false,
        );
        let mut a = args();
        a.key = Some("no-such.key".into());
        assert!(pack(&ctx, &a).is_err());
        std::fs::write(dir.path().join("k.key"), "garbage").unwrap();
        a.key = Some("k.key".into());
        assert!(pack(&ctx, &a).unwrap_err().hint.contains("keygen"));
        // The output's folder is a file.
        std::fs::write(dir.path().join("afile"), "x").unwrap();
        a.key = None;
        a.out = Some("afile/out.paddockplugin".into());
        assert!(
            pack(&ctx, &a)
                .unwrap_err()
                .message
                .contains("cannot be made")
        );
    }
}
