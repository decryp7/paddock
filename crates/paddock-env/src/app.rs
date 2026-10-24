use crate::error::{EnvError, Result};

/// Who the folders belong to: a short lowercase id (Linux folders, the `<ID>_HOME` variable) and the display name
/// (macOS and Windows folders, as users see them in Finder and Explorer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppId {
    id: String,
    name: String,
}

impl AppId {
    /// `id`: 1-32 characters, a lowercase letter first, then lowercase letters, digits and `-`. `name`: 1-64 characters
    /// that are valid in a folder name on every OS (no `/ \ : * ? " < > |`, no control characters, not ending in a dot
    /// or space).
    pub fn new(id: &str, name: &str) -> Result<Self> {
        let id_ok = (1..=32).contains(&id.len())
            && id.starts_with(|c: char| c.is_ascii_lowercase())
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !id_ok {
            return Err(EnvError::InvalidApp(format!(
                "id {id:?} must be 1-32 lowercase letters, digits or '-', starting with a letter"
            )));
        }
        let name_ok = (1..=64).contains(&name.chars().count())
            && name != "."
            && name != ".."
            && !name.ends_with(['.', ' '])
            && !name.starts_with(' ')
            && !name
                .chars()
                .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c));
        if !name_ok {
            return Err(EnvError::InvalidApp(format!(
                "display name {name:?} must be 1-64 characters without / \\ : * ? \" < > | and not end in a dot or space"
            )));
        }
        Ok(Self {
            id: id.to_owned(),
            name: name.to_owned(),
        })
    }

    /// The lowercase id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The display name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The override variable: `<ID>_HOME` (`my-app` gives `MY_APP_HOME`).
    pub fn home_var(&self) -> String {
        format!("{}_HOME", self.id.to_ascii_uppercase().replace('-', "_"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_apps() {
        for (id, name) in [
            ("pitwall", "Pitwall"),
            ("shell-demo", "Paddock Shell Demo"),
            ("a", "A b"),
        ] {
            let a = AppId::new(id, name).unwrap();
            assert_eq!((a.id(), a.name()), (id, name));
        }
    }

    #[test]
    fn rejects_unusable_names() {
        for id in ["", "Pitwall", "1app", "a_b", "a/b", &"a".repeat(33)] {
            assert!(
                matches!(AppId::new(id, "X"), Err(EnvError::InvalidApp(_))),
                "{id}"
            );
        }
        for name in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "a:b",
            "a*",
            "a?",
            "a\"",
            "a<",
            "a|",
            "x.",
            "x ",
            " x",
            "a\nb",
            &"n".repeat(65),
        ] {
            assert!(
                matches!(AppId::new("app", name), Err(EnvError::InvalidApp(_))),
                "{name:?}"
            );
        }
    }

    #[test]
    fn the_home_variable_is_upper_snake() {
        assert_eq!(
            AppId::new("shell-demo", "S").unwrap().home_var(),
            "SHELL_DEMO_HOME"
        );
        assert_eq!(
            AppId::new("debrief", "Debrief").unwrap().home_var(),
            "DEBRIEF_HOME"
        );
    }
}
