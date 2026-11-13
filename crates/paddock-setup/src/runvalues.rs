//! `clean-run`: which `Run` values to remove and the walk over the registry views and user hives (ADR 0053 section 6.3). A
//! value is the app's when its command names a program inside `$INSTDIR`. Reading and deleting are behind [`RunPort`].

use crate::args::RunScope;
use crate::checkdir::{components, is_within};
use crate::log::Sink;

/// One value under a `Run` key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunValue {
    /// The value name.
    pub name: String,
    /// The command line, already expanded (a `REG_EXPAND_SZ` value is expanded by the glue).
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

/// Whether the command starts a program inside `dir`. A path with a `..` component never counts (it may leave `dir`).
pub fn names_program_in(command: &str, dir: &str) -> bool {
    program_of(command)
        .is_some_and(|p| !components(&p).iter().any(|c| c == "..") && is_within(&p, dir))
}

/// HKLM: every value whose command names a program in `dir`.
pub fn machine_removals(values: &[RunValue], dir: &str) -> Vec<String> {
    values
        .iter()
        .filter(|v| names_program_in(&v.command, dir))
        .map(|v| v.name.clone())
        .collect()
}

/// A user hive: only the value named `product` (the app's own), and only when it points into `dir`.
pub fn user_removals(values: &[RunValue], dir: &str, product: &str) -> Vec<String> {
    values
        .iter()
        .filter(|v| v.name.eq_ignore_ascii_case(product) && names_program_in(&v.command, dir))
        .map(|v| v.name.clone())
        .collect()
}

/// The registry behind [`clean`].
pub trait RunPort {
    /// The values of `HKLM\...\Run` in the 64-bit and the 32-bit view, each with the view's label.
    fn machine_values(&mut self) -> Vec<(&'static str, Vec<RunValue>)>;
    /// Deletes a machine value in a view.
    #[must_use = "false means the value is still in the registry"]
    fn delete_machine(&mut self, view: &str, name: &str) -> bool;
    /// The subkeys of `HKU`.
    fn hives(&mut self) -> Vec<String>;
    /// The `Run` values of one hive.
    fn user_values(&mut self, hive: &str) -> Vec<RunValue>;
    /// Deletes a value in one hive.
    #[must_use = "false means the value is still in the registry"]
    fn delete_user(&mut self, hive: &str, name: &str) -> bool;
}

/// What `clean` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cleaned {
    /// Values removed.
    pub removed: u32,
    /// Values that should have gone but could not be deleted.
    pub failed: u32,
}

/// Removes the values (machine always, users only for `MachineAndUsers`) and logs the count without names or commands.
pub fn clean(
    port: &mut dyn RunPort,
    scope: RunScope,
    dir: &str,
    product: &str,
    log: &mut dyn Sink,
) -> Cleaned {
    let mut out = Cleaned::default();
    for (view, values) in port.machine_values() {
        for name in machine_removals(&values, dir) {
            if port.delete_machine(view, &name) {
                out.removed += 1
            } else {
                out.failed += 1
            }
        }
    }
    if scope == RunScope::MachineAndUsers {
        for hive in crate::guard::scan_accounts(&port.hives()) {
            let values = port.user_values(&hive);
            for name in user_removals(&values, dir, product) {
                if port.delete_user(&hive, &name) {
                    out.removed += 1
                } else {
                    out.failed += 1
                }
            }
        }
    }
    log.line(
        "clean-run",
        &format!(
            "removed {} value(s){}",
            out.removed,
            if out.failed > 0 {
                format!(", {} could not be removed", out.failed)
            } else {
                String::new()
            }
        ),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(name: &str, command: &str) -> RunValue {
        RunValue {
            name: name.into(),
            command: command.into(),
        }
    }
    const DIR: &str = r"C:\Program Files\Dyno";

    #[test]
    fn program_quoted_and_unquoted() {
        assert_eq!(
            program_of(r#""C:\Program Files\Dyno\dyno.exe" --hidden"#).as_deref(),
            Some(r"C:\Program Files\Dyno\dyno.exe")
        );
        assert_eq!(
            program_of(r"C:\Program Files\Dyno\dyno.exe --hidden").as_deref(),
            Some(r"C:\Program Files\Dyno\dyno.exe")
        );
        assert_eq!(
            program_of(r"C:\Dyno\DYNO.EXE").as_deref(),
            Some(r"C:\Dyno\DYNO.EXE")
        );
        assert_eq!(program_of(""), None);
        assert_eq!(program_of(r#""unterminated"#), None);
        assert_eq!(program_of("notepad"), None);
    }

    #[test]
    fn names_program_in_dir_by_component() {
        assert!(names_program_in(
            r#""C:\Program Files\Dyno\dyno.exe" --hidden"#,
            DIR
        ));
        assert!(names_program_in(r"c:\program files\dyno\sub\x.exe", DIR));
        assert!(!names_program_in(
            r#""C:\Program Files\Dyno Helper\dyno-helper.exe""#,
            DIR
        ));
        assert!(!names_program_in(
            r"C:\Users\x\AppData\Local\Dyno\dyno.exe",
            DIR
        ));
    }

    #[test]
    fn a_dotdot_path_that_leaves_the_folder_is_not_inside_it() {
        assert!(!names_program_in(
            r#""C:\Program Files\Dyno\..\Other\other.exe""#,
            DIR
        ));
        assert!(!names_program_in(
            r"C:\Program Files\Dyno\..\..\Windows\x.exe",
            DIR
        ));
        assert!(
            !names_program_in(r"C:\Program Files\Dyno\sub\..\dyno.exe", DIR),
            "refused even when it stays inside: the safe answer"
        );
    }

    #[test]
    fn machine_removes_any_value_into_the_folder() {
        let vals = [
            v("Dyno", r#""C:\Program Files\Dyno\dyno.exe""#),
            v("Other", r"C:\Tools\o.exe"),
            v("Agent", r"C:\Program Files\Dyno\dyno-agent.exe run"),
        ];
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

    #[derive(Default)]
    struct Fake {
        machine: Vec<(&'static str, Vec<RunValue>)>,
        users: Vec<(String, Vec<RunValue>)>,
        deleted: Vec<String>,
        refuse: Vec<&'static str>,
    }
    impl RunPort for Fake {
        fn machine_values(&mut self) -> Vec<(&'static str, Vec<RunValue>)> {
            self.machine.clone()
        }
        fn delete_machine(&mut self, view: &str, name: &str) -> bool {
            self.deleted.push(format!("machine/{view}/{name}"));
            !self.refuse.contains(&name)
        }
        fn hives(&mut self) -> Vec<String> {
            let mut h: Vec<String> = self.users.iter().map(|(s, _)| s.clone()).collect();
            h.push("S-1-5-21-9-9-9-1001_Classes".into());
            h.push(".DEFAULT".into());
            h
        }
        fn user_values(&mut self, hive: &str) -> Vec<RunValue> {
            self.users
                .iter()
                .find(|(s, _)| s == hive)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        }
        fn delete_user(&mut self, hive: &str, name: &str) -> bool {
            self.deleted.push(format!("{hive}/{name}"));
            true
        }
    }
    #[derive(Default)]
    struct Lines(Vec<String>);
    impl Sink for Lines {
        fn line(&mut self, phase: &str, message: &str) {
            self.0.push(format!("{phase}: {message}"));
        }
    }
    fn fake() -> Fake {
        Fake {
            machine: vec![
                (
                    "64",
                    vec![
                        v("Dyno", r#""C:\Program Files\Dyno\dyno.exe""#),
                        v("Other", r"C:\x.exe"),
                    ],
                ),
                ("32", vec![v("DynoOld", r"C:\Program Files\Dyno\old.exe")]),
            ],
            users: vec![
                (
                    "S-1-5-21-1-1-1-1001".into(),
                    vec![v("Dyno", r#""C:\Program Files\Dyno\dyno.exe""#)],
                ),
                (
                    "S-1-5-21-1-1-1-1002".into(),
                    vec![v("Dyno", r"C:\Users\b\AppData\Local\Dyno\dyno.exe")],
                ),
            ],
            ..Fake::default()
        }
    }

    #[test]
    fn machine_scope_walks_both_views_and_leaves_users_alone() {
        let mut f = fake();
        let mut log = Lines::default();
        let done = clean(&mut f, RunScope::Machine, DIR, "Dyno", &mut log);
        assert_eq!(
            done,
            Cleaned {
                removed: 2,
                failed: 0
            }
        );
        assert_eq!(f.deleted, ["machine/64/Dyno", "machine/32/DynoOld"]);
        assert_eq!(log.0, ["clean-run: removed 2 value(s)"]);
    }

    #[test]
    fn all_scope_also_walks_the_user_hives_but_not_classes_hives() {
        let mut f = fake();
        let done = clean(
            &mut f,
            RunScope::MachineAndUsers,
            DIR,
            "Dyno",
            &mut Lines::default(),
        );
        assert_eq!(done.removed, 3);
        assert_eq!(f.deleted.last().unwrap(), "S-1-5-21-1-1-1-1001/Dyno");
        assert!(
            !f.deleted
                .iter()
                .any(|d| d.contains("1002") || d.contains("_Classes"))
        );
    }

    #[test]
    fn a_value_that_cannot_be_deleted_is_counted_and_reported() {
        let mut f = fake();
        f.refuse = vec!["Dyno"];
        let mut log = Lines::default();
        let done = clean(&mut f, RunScope::Machine, DIR, "Dyno", &mut log);
        assert_eq!(
            done,
            Cleaned {
                removed: 1,
                failed: 1
            }
        );
        assert_eq!(
            log.0,
            ["clean-run: removed 1 value(s), 1 could not be removed"]
        );
    }
}
