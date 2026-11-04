//! `clean-run`: which `Run` values to remove (ADR 0053 section 6.3). A value is the app's when its command names a program
//! inside `$INSTDIR`; the registry reading and deleting is the glue's.

use crate::checkdir::is_within;

/// One value under a `Run` key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunValue {
    /// The value name.
    pub name: String,
    /// The command line (already expanded).
    pub command: String,
}

/// The program a `Run` command starts: the quoted path, or the text up to the first `.exe` for an unquoted command.
pub fn program_of(command: &str) -> Option<String> {
    let c = command.trim();
    if let Some(rest) = c.strip_prefix('"') {
        let end = rest.find('"')?;
        let program = &rest[..end];
        return (!program.is_empty()).then(|| program.to_string());
    }
    let lower = c.to_ascii_lowercase();
    let at = lower.find(".exe")?;
    let end = at + 4;
    let boundary = lower[end..].chars().next().is_none_or(char::is_whitespace);
    (boundary && at > 0).then(|| c[..end].to_string())
}

/// Whether the command starts a program inside `dir`.
pub fn names_program_in(command: &str, dir: &str) -> bool {
    program_of(command).is_some_and(|p| is_within(&p, dir))
}

/// HKLM: every value whose command names a program in `dir`.
pub fn machine_removals(values: &[RunValue], dir: &str) -> Vec<String> {
    values.iter().filter(|v| names_program_in(&v.command, dir)).map(|v| v.name.clone()).collect()
}

/// A user hive: only the value named `product` (the app's own), and only when it points into `dir`.
pub fn user_removals(values: &[RunValue], dir: &str, product: &str) -> Vec<String> {
    values
        .iter()
        .filter(|v| v.name.eq_ignore_ascii_case(product) && names_program_in(&v.command, dir))
        .map(|v| v.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(name: &str, command: &str) -> RunValue {
        RunValue { name: name.into(), command: command.into() }
    }
    const DIR: &str = r"C:\Program Files\Dyno";

    #[test]
    fn program_quoted_and_unquoted() {
        assert_eq!(program_of(r#""C:\Program Files\Dyno\dyno.exe" --hidden"#).as_deref(), Some(r"C:\Program Files\Dyno\dyno.exe"));
        assert_eq!(program_of(r"C:\Program Files\Dyno\dyno.exe --hidden").as_deref(), Some(r"C:\Program Files\Dyno\dyno.exe"));
        assert_eq!(program_of(r"C:\Dyno\DYNO.EXE").as_deref(), Some(r"C:\Dyno\DYNO.EXE"));
        assert_eq!(program_of(""), None);
        assert_eq!(program_of(r#""unterminated"#), None);
        assert_eq!(program_of("notepad"), None);
    }

    #[test]
    fn names_program_in_dir_by_component() {
        assert!(names_program_in(r#""C:\Program Files\Dyno\dyno.exe" --hidden"#, DIR));
        assert!(names_program_in(r"c:\program files\dyno\sub\x.exe", DIR));
        assert!(!names_program_in(r#""C:\Program Files\Dyno Helper\dyno-helper.exe""#, DIR));
        assert!(!names_program_in(r"C:\Users\x\AppData\Local\Dyno\dyno.exe", DIR));
    }

    #[test]
    fn machine_removes_any_value_into_the_folder() {
        let vals = [v("Dyno", r#""C:\Program Files\Dyno\dyno.exe""#), v("Other", r"C:\Tools\o.exe"), v("Agent", r"C:\Program Files\Dyno\dyno-agent.exe run")];
        assert_eq!(machine_removals(&vals, DIR), vec!["Dyno", "Agent"]);
    }

    #[test]
    fn user_removes_only_the_apps_own_value_when_it_points_into_the_folder() {
        let vals = [
            v("Dyno", r#""C:\Program Files\Dyno\dyno.exe" --hidden"#),
            v("Agent", r"C:\Program Files\Dyno\dyno-agent.exe"),
            v("dyno", r#""C:\Program Files\Dyno\dyno.exe""#),
        ];
        assert_eq!(user_removals(&vals, DIR, "Dyno"), vec!["Dyno", "dyno"]);
        let per_user_copy = [v("Dyno", r"C:\Users\x\AppData\Local\Dyno\dyno.exe")];
        assert!(user_removals(&per_user_copy, DIR, "Dyno").is_empty());
    }
}
