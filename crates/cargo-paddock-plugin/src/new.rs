//! `new`: writes a plugin project from [`crate::template`].

use crate::Context;
use crate::args::NewArgs;
use crate::template::{Api, Template};
use paddock_cli::{CliError, Report};
use serde_json::json;

/// `new <name>`: makes `<dir>/<name>` and writes the project into it.
///
/// # Errors
///
/// When the name, id or publisher is not valid, the folder already exists (nothing is overwritten), or a file cannot be
/// written (the half-written folder is removed).
pub fn new(ctx: &Context, a: &NewArgs) -> Result<Report, CliError> {
    let mut template = Template::new(&a.name);
    if let Some(id) = &a.id {
        template.id.clone_from(id);
    }
    if let Some(publisher) = &a.publisher {
        template.publisher.clone_from(publisher);
    }
    if let Some(path) = &a.api_path {
        template.api = Api::Path(ctx.cwd.join(path));
    }
    let files = template.files()?;
    let root = ctx.dir(a.parent.as_deref()).join(&a.name);
    if root.exists() {
        return Err(
            CliError::refused(format!("{} already exists.", root.display()))
                .with_hint("Choose another name or --dir; nothing was written."),
        );
    }
    let write = || -> std::io::Result<()> {
        for (path, text) in &files {
            let target = root.join(path);
            std::fs::create_dir_all(target.parent().unwrap_or(&root))?;
            std::fs::write(&target, text)?;
        }
        Ok(())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&root);
        return Err(
            CliError::failure(format!("{} could not be written ({e}).", root.display()))
                .with_hint("Check that you may write there, or choose another --dir."),
        );
    }
    let names: Vec<String> = files
        .iter()
        .map(|(p, _)| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let text = format!(
        "made the plugin project {}\n  {}\nNext: cd {} && cargo paddock-plugin test",
        root.display(),
        names.join("\n  "),
        a.name
    );
    Ok(Report::new(
        json!({"project": root.to_string_lossy(), "id": template.id, "files": names}),
        text,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{FakeCargo, context};

    fn args(name: &str) -> NewArgs {
        NewArgs {
            name: name.into(),
            parent: None,
            id: None,
            publisher: None,
            api_path: None,
        }
    }

    #[test]
    fn a_project_is_written_and_nothing_existing_is_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = context(dir.path(), FakeCargo::making_nothing(), false);
        let mut a = args("hello");
        a.id = Some("com.example.hello".into());
        a.publisher = Some("Me".into());
        a.api_path = Some("../crates".into());
        let r = new(&ctx, &a).unwrap();
        let root = dir.path().join("hello");
        assert_eq!(r.data["project"], root.to_string_lossy().as_ref());
        for f in [
            "Cargo.toml",
            "manifest.toml",
            "src/lib.rs",
            "src/main.rs",
            "tests/conformance.rs",
            ".gitignore",
        ] {
            assert!(root.join(f).is_file(), "{f}");
        }
        assert!(root.join(".cargo/config.toml").is_file());
        let project = crate::project::Project::load(&root).unwrap();
        assert_eq!(project.manifest.id.as_str(), "com.example.hello");
        assert_eq!(project.manifest.publisher, "Me");
        assert!(
            std::fs::read_to_string(root.join("Cargo.toml"))
                .unwrap()
                .contains("crates")
        );
        // A second time: refused, untouched.
        std::fs::write(root.join("manifest.toml"), "mine").unwrap();
        let e = new(&ctx, &a).unwrap_err();
        assert_eq!(e.exit, paddock_cli::Exit::Refused);
        assert_eq!(
            std::fs::read_to_string(root.join("manifest.toml")).unwrap(),
            "mine"
        );
    }

    #[test]
    fn a_bad_name_writes_nothing_and_an_unwritable_place_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = context(dir.path(), FakeCargo::making_nothing(), false);
        assert!(new(&ctx, &args("Bad Name")).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        // The parent is a file: the project folder cannot be made.
        std::fs::write(dir.path().join("afile"), "x").unwrap();
        let mut a = args("hello");
        a.parent = Some("afile".into());
        let e = new(&ctx, &a).unwrap_err();
        assert!(
            e.message.contains("could not be written") && e.hint.contains("--dir"),
            "{e:?}"
        );
        assert!(!dir.path().join("afile/hello").exists());
    }
}
