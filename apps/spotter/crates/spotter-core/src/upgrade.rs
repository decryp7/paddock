//! The command that installs or pulls a release, for sources whose data says what to run: container tags
//! (`docker pull`) and package registry versions (`cargo`, `npm`, `pip`, `dotnet`). The UI offers it as a copyable line;
//! Spotter never runs it. Names and versions are checked first, so a pasted line cannot carry shell syntax.

use crate::source::SourceConfig;

fn safe(text: &str, extra: &str) -> bool {
    !text.is_empty() && text.len() <= 200 && text.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

/// The install command for `version` of the package or image `source` watches, or `None` when the source has no such
/// command (folders, forges and pages: what to install depends on the platform) or its values are unusual.
pub fn upgrade_command(source: &SourceConfig, version: &str) -> Option<String> {
    let value = |k: &str| source.values.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
    match source.kind.as_str() {
        "container" => {
            let image = value("image").filter(|i| safe(i, "._/:-@"))?;
            // A tag is letters, digits, `_`, `.` and `-`; anything else (an image given with a tag already) is left alone.
            (safe(version, "._-") && !image.contains([':', '@'])).then(|| format!("docker pull {image}:{version}"))
        }
        "package" => {
            let name = value("package").filter(|n| safe(n, "._@/-"))?;
            if !safe(version, "._+-~") {
                return None;
            }
            match value("registry")? {
                "crates-io" => Some(format!("cargo install {name} --version {version} --locked")),
                "npm" => Some(format!("npm install {name}@{version}")),
                "pypi" => Some(format!("pip install {name}=={version}")),
                "nuget" => Some(format!("dotnet add package {name} --version {version}")),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(kind: &str, values: &[(&str, &str)]) -> SourceConfig {
        SourceConfig::new(kind, values)
    }

    #[test]
    fn packages_and_images_have_a_command() {
        let pkg = |registry: &str, name: &str| cfg("package", &[("registry", registry), ("package", name)]);
        assert_eq!(
            upgrade_command(&pkg("crates-io", "ripgrep"), "14.1.1").unwrap(),
            "cargo install ripgrep --version 14.1.1 --locked"
        );
        assert_eq!(
            upgrade_command(&pkg("npm", "@angular/cli"), "21.0.0").unwrap(),
            "npm install @angular/cli@21.0.0"
        );
        assert_eq!(
            upgrade_command(&pkg("pypi", "httpie"), "3.2.4").unwrap(),
            "pip install httpie==3.2.4"
        );
        assert_eq!(
            upgrade_command(&pkg("nuget", "Serilog"), "4.0.0-beta.1").unwrap(),
            "dotnet add package Serilog --version 4.0.0-beta.1"
        );
        assert_eq!(
            upgrade_command(&cfg("container", &[("image", "ghcr.io/owner/app")]), "1.2.3-rc.1").unwrap(),
            "docker pull ghcr.io/owner/app:1.2.3-rc.1"
        );
    }

    #[test]
    fn other_sources_and_unusual_values_have_none() {
        assert_eq!(upgrade_command(&cfg("github", &[("repo", "o/r")]), "1.0.0"), None);
        assert_eq!(upgrade_command(&cfg("folder", &[("path", "/srv")]), "1.0.0"), None);
        assert_eq!(
            upgrade_command(&cfg("package", &[("registry", "gems"), ("package", "x")]), "1.0.0"),
            None
        );
        assert_eq!(upgrade_command(&cfg("package", &[("registry", "npm")]), "1.0.0"), None);
        assert_eq!(upgrade_command(&cfg("package", &[("package", "x")]), "1.0.0"), None);
        // Shell syntax in a name or version is never put into a command line.
        let evil = cfg("package", &[("registry", "npm"), ("package", "x; rm -rf ~")]);
        assert_eq!(upgrade_command(&evil, "1.0.0"), None);
        let pkg = cfg("package", &[("registry", "npm"), ("package", "x")]);
        assert_eq!(upgrade_command(&pkg, "1.0.0 && echo"), None);
        assert_eq!(upgrade_command(&pkg, ""), None);
        assert_eq!(
            upgrade_command(&cfg("container", &[("image", "nginx:1.25")]), "1.26"),
            None,
            "already has a tag"
        );
        assert_eq!(upgrade_command(&cfg("container", &[("image", "nginx")]), "R1 02"), None);
        assert_eq!(upgrade_command(&cfg("container", &[]), "1"), None);
    }
}
