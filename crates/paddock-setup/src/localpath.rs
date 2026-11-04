//! A path the elevated process may open: parsed once, at the edge, into a type that cannot name a pipe, a device, a share or
//! a namespace path (ADR 0053 section 4.2, "no argument from the caller is trusted"). Anything else is a [`PathError`]
//! before any `CreateFileW` runs. Opening a `\\.\pipe\x` or `\\server\share\x` path as an administrator hands the
//! caller's choice of server a chance to answer; rejecting the text first removes that class.

/// The longest path accepted (Windows' own limit without the long-path prefix is 260; this is generous).
pub const MAX_PATH_TEXT: usize = 1024;

/// A drive-letter path `X:\dir\file`: absolute, backslashes only, no `..`, no empty, dotted or space-ended components, no
/// colon after the drive, no reserved device names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPath(String);

/// Why a path is not a [`LocalPath`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    /// Empty, too long, or containing a control character.
    #[error("the path is empty, too long or has control characters")]
    Shape,
    /// `\\server\share`, `\\?\`, `\\.\`, `\??\`: not a plain drive-letter path.
    #[error("the path is a network, device or namespace path")]
    Namespace,
    /// Not absolute with a drive letter.
    #[error("the path is not absolute with a drive letter")]
    NotAbsolute,
    /// A `:` after the drive (an alternate data stream), or a wildcard or other forbidden character.
    #[error("the path has a stream, wildcard or forbidden character")]
    Forbidden,
    /// `..`, an empty component, or a component ending in a dot or space.
    #[error("the path has a `..`, an empty component or a component ending in a dot or space")]
    Component,
    /// A reserved device name (`CON`, `NUL`, `COM1`, ...) or `GLOBALROOT`.
    #[error("the path names a device")]
    Device,
}

const DEVICES: [&str; 6] = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"];

fn is_device(component: &str) -> bool {
    // A device name with any extension is still the device (`NUL.txt`).
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end()
        .to_ascii_uppercase();
    if DEVICES.contains(&stem.as_str())
        || stem == "GLOBALROOT"
        || stem == "$EXTEND"
        || stem == "CLOCK$"
    {
        return true;
    }
    let numbered = |prefix: &str| {
        stem.strip_prefix(prefix)
            .is_some_and(|n| n.len() == 1 && n.chars().all(|c| ('1'..='9').contains(&c)))
    };
    numbered("COM") || numbered("LPT")
}

impl LocalPath {
    /// Parses `text`.
    pub fn parse(text: &str) -> Result<LocalPath, PathError> {
        if text.is_empty() || text.len() > MAX_PATH_TEXT || text.chars().any(char::is_control) {
            return Err(PathError::Shape);
        }
        if text.starts_with(r"\\")
            || text
                .get(..4)
                .is_some_and(|h| h.eq_ignore_ascii_case(r"\??\"))
        {
            return Err(PathError::Namespace);
        }
        if text.contains(['/', '*', '?', '"', '<', '>', '|']) {
            return Err(PathError::Forbidden);
        }
        let mut chars = text.chars();
        let rest = match (chars.next(), chars.next(), chars.next()) {
            (Some(d), Some(':'), Some('\\')) if d.is_ascii_alphabetic() => &text[3..],
            _ => return Err(PathError::NotAbsolute),
        };
        if rest.contains(':') {
            return Err(PathError::Forbidden);
        }
        for component in rest.split('\\') {
            if component.is_empty()
                || component == "."
                || component == ".."
                || component.ends_with(['.', ' '])
            {
                return Err(PathError::Component);
            }
            if is_device(component) {
                return Err(PathError::Device);
            }
        }
        Ok(LocalPath(text.to_string()))
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_drive_paths_are_accepted() {
        for p in [
            r"C:\Users\x\AppData\Local\Temp\Dyno_1.2.0_x64-setup.exe",
            r"d:\a",
            r"C:\PROGRA~1\Dyno\a.exe",
            r"C:\a b\c.d.exe",
        ] {
            assert_eq!(LocalPath::parse(p).unwrap().as_str(), p, "{p}");
        }
    }

    #[test]
    fn namespaces_shares_and_pipes_are_refused_before_any_open() {
        for p in [
            r"\\server\share\x.exe",
            r"\\?\C:\x.exe",
            r"\\?\UNC\srv\share\x",
            r"\\.\pipe\evil",
            r"\\.\GLOBALROOT\Device\x",
            r"\??\C:\x",
            r"//server/share/x",
            r"\GLOBAL??\C:\x",
        ] {
            assert!(LocalPath::parse(p).is_err(), "{p}");
        }
        assert_eq!(
            LocalPath::parse(r"\\server\share\x.exe"),
            Err(PathError::Namespace)
        );
        assert_eq!(
            LocalPath::parse(r"\\.\pipe\evil"),
            Err(PathError::Namespace)
        );
        assert_eq!(LocalPath::parse(r"\\?\C:\x.exe"), Err(PathError::Namespace));
        assert_eq!(LocalPath::parse(r"\??\C:\x"), Err(PathError::Namespace));
    }

    #[test]
    fn relative_rooted_and_drive_relative_paths_are_refused() {
        for p in [
            "x.exe",
            r"..\x.exe",
            r"\x.exe",
            r"C:x.exe",
            "C:",
            r"1:\x",
            "/x",
            "C:/x/y",
        ] {
            assert!(LocalPath::parse(p).is_err(), "{p}");
        }
        assert_eq!(LocalPath::parse("x.exe"), Err(PathError::NotAbsolute));
        assert_eq!(LocalPath::parse("C:/x/y"), Err(PathError::Forbidden));
    }

    #[test]
    fn streams_wildcards_and_traversal_are_refused() {
        assert_eq!(
            LocalPath::parse(r"C:\a\b.exe:evil"),
            Err(PathError::Forbidden)
        );
        assert_eq!(
            LocalPath::parse(r"C:\a\b.exe::$DATA"),
            Err(PathError::Forbidden)
        );
        assert_eq!(LocalPath::parse(r"C:\a\*.exe"), Err(PathError::Forbidden));
        assert_eq!(LocalPath::parse(r"C:\a\b?.exe"), Err(PathError::Forbidden));
        assert_eq!(
            LocalPath::parse(r"C:\a\..\b.exe"),
            Err(PathError::Component)
        );
        assert_eq!(LocalPath::parse(r"C:\a\\b.exe"), Err(PathError::Component));
        assert_eq!(LocalPath::parse(r"C:\a\b.exe."), Err(PathError::Component));
        assert_eq!(LocalPath::parse(r"C:\a \b.exe"), Err(PathError::Component));
        assert_eq!(LocalPath::parse(r"C:\a\."), Err(PathError::Component));
    }

    #[test]
    fn device_names_are_refused() {
        for p in [
            r"C:\NUL",
            r"C:\a\con.txt",
            r"C:\COM1",
            r"C:\lpt9.log",
            r"C:\GLOBALROOT\Device\x",
            r"C:\a\AUX",
        ] {
            assert_eq!(LocalPath::parse(p), Err(PathError::Device), "{p}");
        }
        assert!(LocalPath::parse(r"C:\COM10").is_ok());
        assert!(LocalPath::parse(r"C:\console\a").is_ok());
    }

    #[test]
    fn shape_limits() {
        assert_eq!(LocalPath::parse(""), Err(PathError::Shape));
        assert_eq!(LocalPath::parse("C:\\a\nb"), Err(PathError::Shape));
        assert_eq!(
            LocalPath::parse(&format!("C:\\{}", "a".repeat(MAX_PATH_TEXT))),
            Err(PathError::Shape)
        );
        assert_eq!(LocalPath::parse("C:\\a\u{0}b"), Err(PathError::Shape));
    }
}
