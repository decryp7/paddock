//! The path validators: the only place a path from the webview is parsed, resolved and confined. Pure functions of the file
//! system, no state.

use paddock_commands::CommandError;
use std::path::{Component, Path, PathBuf};

/// The result of every validator: the canonical path or why not, in words for the user.
pub type Result<T> = std::result::Result<T, CommandError>;

/// The longest path accepted from the webview, in bytes.
pub const MAX_PATH_LEN: usize = 4096;

/// The longest file name accepted from the webview, in bytes.
pub const MAX_NAME_LEN: usize = 255;

/// A path from the webview: not empty, not too long, no NUL, absolute.
pub fn parse(raw: &str) -> Result<&Path> {
    if raw.trim().is_empty() {
        return Err(CommandError::invalid("choose a file").field("path"));
    }
    if raw.len() > MAX_PATH_LEN || raw.contains('\0') {
        return Err(CommandError::invalid("this is not a valid path").field("path"));
    }
    let p = Path::new(raw);
    if !p.is_absolute() {
        return Err(CommandError::invalid(format!(
            "the path must be absolute (choose the file in the dialog): {raw}"
        ))
        .field("path"));
    }
    Ok(p)
}

/// An existing file or folder, canonicalised (links and `..` resolved).
pub fn canonical(path: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(path).map(plain).map_err(|e| {
        let message = format!("cannot open {}: {e}", path.display());
        if e.kind() == std::io::ErrorKind::NotFound {
            CommandError::not_found(message)
        } else {
            CommandError::io(message)
        }
    })
}

/// A canonical Windows path without its verbatim prefix when it has an ordinary spelling (`C:` drives and `UNC` shares), so
/// messages and results show the path the user knows. Elsewhere: unchanged.
pub fn plain(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    let Some(rest) = text.strip_prefix(r"\\?\") else {
        return path;
    };
    if let Some(share) = rest.strip_prefix(r"UNC\") {
        return PathBuf::from(format!(r"\\{share}"));
    }
    let b = rest.as_bytes();
    if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\' {
        return PathBuf::from(rest);
    }
    path
}

/// Where a file will be written: its folder canonicalised (it must exist) joined with its name, which must not name a
/// folder, `.` or `..`.
pub fn output_target(path: &Path) -> Result<PathBuf> {
    let name = match path.components().next_back() {
        Some(Component::Normal(n)) => n.to_owned(),
        _ => return Err(CommandError::invalid("choose a file name")),
    };
    let parent = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .ok_or_else(|| CommandError::invalid("choose a folder"))?;
    let dir = std::fs::canonicalize(parent).map_err(|_| {
        CommandError::not_found(format!("the folder {} does not exist", parent.display()))
    })?;
    let target = plain(dir).join(name);
    if target.is_dir() {
        return Err(CommandError::invalid(format!(
            "{} is a folder",
            target.display()
        )));
    }
    Ok(target)
}

/// Checks the extension of a file (a folder always passes). `extensions` are lower case without the dot; empty means any.
pub fn has_extension(path: &Path, extensions: &[&str]) -> Result<()> {
    if extensions.is_empty() || path.is_dir() {
        return Ok(());
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if extensions.contains(&ext.as_str()) {
        Ok(())
    } else {
        Err(CommandError::invalid(format!(
            "the file must end in .{}",
            extensions.join(" or .")
        )))
    }
}

/// A file to write without a grant (a path the app picks itself, or one typed into a setting): absolute, one of `extensions`,
/// in an existing folder (canonicalised), not a folder.
pub fn output_file(raw: &str, extensions: &[&str]) -> Result<PathBuf> {
    let target = output_target(parse(raw)?)?;
    has_extension(&target, extensions)?;
    Ok(target)
}

/// `name` (from the webview) as a file directly in `root`: one plain name (no separators, `.`, `..` or NUL), not too long.
pub fn child(root: &Path, name: &str) -> Result<PathBuf> {
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(n)), None)
            if name.len() <= MAX_NAME_LEN && !name.contains(['\0', '/', '\\']) =>
        {
            Ok(root.join(n))
        }
        _ => Err(CommandError::invalid(format!(
            "{} is not a file name",
            name.chars().take(80).collect::<String>()
        ))),
    }
}

/// `item` as it may be removed or changed: its parent must resolve (canonically) inside `root`, which is itself canonical (from [`canonical`]),
/// and its name must be a plain file or folder name. The returned path is built from the resolved parent, so a link in the
/// parent chain cannot point the operation elsewhere.
pub fn confined(root: &Path, item: &Path) -> std::result::Result<PathBuf, String> {
    let outside = || format!("{} is outside the folder the app manages", item.display());
    if item
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(outside());
    }
    let (Some(name), Some(parent)) = (item.file_name(), item.parent()) else {
        return Err(outside());
    };
    let parent = canonical(parent).map_err(|_| outside())?;
    if !parent.starts_with(root) {
        return Err(outside());
    }
    Ok(parent.join(name))
}

/// The file's name for a message ("the file" when it has none).
pub fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "the file".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_commands::ErrorCode;

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn parse_refuses_what_is_not_an_absolute_path() {
        let t = tempfile::tempdir().unwrap();
        let ok = s(t.path());
        assert_eq!(parse(&ok).unwrap(), t.path());
        for bad in ["", "   ", "relative/x", "./x", "a\0b"] {
            let e = parse(bad).unwrap_err();
            assert_eq!(e.code, ErrorCode::Invalid, "{bad:?}");
            assert_eq!(e.field.as_deref(), Some("path"));
        }
        assert!(
            parse("relative/x")
                .unwrap_err()
                .message
                .contains("absolute")
        );
        let long = format!("{ok}/{}", "a".repeat(MAX_PATH_LEN));
        assert_eq!(parse(&long).unwrap_err().code, ErrorCode::Invalid);
    }

    #[test]
    fn canonical_resolves_links_and_says_what_is_missing() {
        let t = tempfile::tempdir().unwrap();
        let real = canonical(t.path()).unwrap();
        std::fs::create_dir(t.path().join("d")).unwrap();
        let spelled = t.path().join("d").join("..").join("d");
        assert_eq!(canonical(&spelled).unwrap(), real.join("d"));
        let missing = canonical(&t.path().join("nope")).unwrap_err();
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert!(missing.message.contains("nope"));
        // A file used as a folder is an I/O error, not "not found".
        std::fs::write(t.path().join("f"), "x").unwrap();
        let io = canonical(&t.path().join("f").join("x")).unwrap_err();
        assert!(matches!(io.code, ErrorCode::Io | ErrorCode::NotFound));
    }

    #[test]
    fn plain_drops_the_verbatim_prefix_only_for_ordinary_spellings() {
        let p = |t: &str| plain(PathBuf::from(t));
        assert_eq!(p(r"\\?\C:\logs\a.log"), PathBuf::from(r"C:\logs\a.log"));
        assert_eq!(
            p(r"\\?\UNC\srv\share\a.log"),
            PathBuf::from(r"\\srv\share\a.log")
        );
        assert_eq!(
            p(r"\\?\Volume{x}\a.log"),
            PathBuf::from(r"\\?\Volume{x}\a.log")
        );
        assert_eq!(p(r"\\?\C:"), PathBuf::from(r"\\?\C:"));
        assert_eq!(p("/var/log/a.log"), PathBuf::from("/var/log/a.log"));
    }

    #[cfg(unix)]
    #[test]
    fn plain_leaves_names_that_are_not_text_alone() {
        use std::os::unix::ffi::OsStringExt;
        let odd = PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', 0xff, b'a']));
        assert_eq!(plain(odd.clone()), odd);
    }

    #[test]
    fn output_targets_need_an_existing_folder_and_a_plain_name() {
        let t = tempfile::tempdir().unwrap();
        let dir = canonical(t.path()).unwrap();
        assert_eq!(
            output_target(&t.path().join("new.zip")).unwrap(),
            dir.join("new.zip")
        );
        assert_eq!(
            output_target(&t.path().join("..")).unwrap_err().code,
            ErrorCode::Invalid
        );
        assert_eq!(
            output_target(Path::new("/")).unwrap_err().code,
            ErrorCode::Invalid
        );
        assert_eq!(
            output_target(Path::new("file")).unwrap_err().message,
            "choose a folder"
        );
        let e = output_target(&t.path().join("gone").join("x.zip")).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound);
        std::fs::create_dir(t.path().join("d")).unwrap();
        assert!(
            output_target(&t.path().join("d"))
                .unwrap_err()
                .message
                .contains("is a folder")
        );
    }

    #[test]
    fn extensions_are_checked_case_insensitively_and_folders_pass() {
        let t = tempfile::tempdir().unwrap();
        assert!(has_extension(Path::new("/x/a.ZIP"), &["zip"]).is_ok());
        assert!(has_extension(Path::new("/x/a"), &[]).is_ok());
        assert!(
            has_extension(t.path(), &["zip"]).is_ok(),
            "a folder has no extension"
        );
        let e = has_extension(Path::new("/x/a.txt"), &["zip", "7z"]).unwrap_err();
        assert!(e.message.contains(".zip or .7z"), "{}", e.message);
        assert!(has_extension(Path::new("/x/noext"), &["zip"]).is_err());
    }

    #[test]
    fn output_file_combines_the_checks() {
        let t = tempfile::tempdir().unwrap();
        let ok = output_file(&s(&t.path().join("a.json")), &["json"]).unwrap();
        assert!(ok.ends_with("a.json"));
        assert!(output_file(&s(&t.path().join("a.txt")), &["json"]).is_err());
        assert!(output_file("relative.json", &["json"]).is_err());
    }

    #[test]
    fn child_is_one_plain_name() {
        let root = Path::new("/root");
        assert_eq!(child(root, "a.log").unwrap(), root.join("a.log"));
        for bad in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "../x",
            "a\0",
            &"n".repeat(300),
        ] {
            assert!(child(root, bad).is_err(), "{bad:?}");
        }
        let long_name = "é".repeat(200);
        assert!(
            child(root, &long_name).is_err(),
            "bytes count, not characters"
        );
    }

    #[test]
    fn confined_keeps_removals_inside_the_root() {
        let t = tempfile::tempdir().unwrap();
        let root = canonical(t.path()).unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/a"), "a").unwrap();
        assert_eq!(
            confined(&root, &root.join("sub/a")).unwrap(),
            root.join("sub/a")
        );
        assert_eq!(
            confined(&root, &root.join("new")).unwrap(),
            root.join("new")
        );
        let outside = tempfile::tempdir().unwrap();
        let e = confined(&root, &outside.path().join("x")).unwrap_err();
        assert!(e.contains("outside the folder"), "{e}");
        assert!(
            confined(&root, &root.join("sub/../sub/a")).is_err(),
            "`..` is refused outright"
        );
        assert!(
            confined(&root, Path::new("/")).is_err(),
            "no parent, no name"
        );
        assert!(
            confined(&root, &root.join("missing/dir/x")).is_err(),
            "an unresolvable parent"
        );
    }

    #[cfg(unix)]
    #[test]
    fn confined_follows_links_in_the_parent_chain() {
        let t = tempfile::tempdir().unwrap();
        let root = canonical(t.path()).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("victim"), "v").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("link")).unwrap();
        let e = confined(&root, &root.join("link/victim")).unwrap_err();
        assert!(e.contains("outside the folder"), "{e}");
    }

    #[test]
    fn file_name_falls_back() {
        assert_eq!(file_name(Path::new("/a/b.txt")), "b.txt");
        assert_eq!(file_name(Path::new("/")), "the file");
    }
}
