//! Repository paths are `/`-separated strings relative to the root, on every OS.

/// The folder of a path: `a/b/c.json` is `a/b`; a bare file name gives `.` (as Node's `dirname`).
pub fn dirname(path: &str) -> String {
    match path.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => path[..i].to_string(),
        None => ".".to_string(),
    }
}

/// `rel` resolved against the folder `base` (both relative to the root) with `.` and `..` applied. `None` when the result
/// leaves the root.
pub fn join_normalized(base: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in base.split('/').chain(rel.split('/')) {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// `path` relative to `root`, both absolute and `/`-separated (backslashes are taken as separators): `a/b` for
/// `/r/a/b` under `/r`, an empty string for the root itself, `../x` for a path outside it.
pub fn relative(root: &str, path: &str) -> String {
    let split = |s: &str| {
        s.replace('\\', "/")
            .split('/')
            .filter(|p| !p.is_empty() && *p != ".")
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let (r, p) = (split(root), split(path));
    let common = r.iter().zip(&p).take_while(|(a, b)| a == b).count();
    let mut out: Vec<String> = vec!["..".to_string(); r.len() - common];
    out.extend(p[common..].iter().cloned());
    out.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirname_matches_node() {
        assert_eq!(dirname("a/b/c.json"), "a/b");
        assert_eq!(dirname("c.json"), ".");
        assert_eq!(dirname("/c.json"), "/");
    }

    #[test]
    fn join_applies_dots_and_stops_at_the_root() {
        assert_eq!(
            join_normalized("apps/x/src-tauri", "../dist").as_deref(),
            Some("apps/x/dist")
        );
        assert_eq!(
            join_normalized(".", "./ui/dist").as_deref(),
            Some("ui/dist")
        );
        assert_eq!(join_normalized("a", "../../b"), None);
        assert_eq!(join_normalized("a", "..").as_deref(), Some(""));
    }

    #[test]
    fn relative_handles_inside_equal_outside_and_backslashes() {
        assert_eq!(relative("/r", "/r/a/b"), "a/b");
        assert_eq!(relative("/r", "/r"), "");
        assert_eq!(relative("/r/x", "/r/y/z"), "../y/z");
        assert_eq!(relative("C:/r", "C:\\r\\a\\b"), "a/b");
    }
}
