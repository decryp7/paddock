//! The built-in source kinds and their forms. The app renders the add-watch form from these, exactly as it does
//! for plugin kinds.

use crate::schema::{ChoiceOption, FieldKind, FieldSpec, SourceKindSpec};

/// A folder, mounted drive or share.
pub const FOLDER: &str = "folder";
/// An HTTP directory listing.
pub const WEB_DIRECTORY: &str = "web-directory";
/// RSS or Atom.
pub const FEED: &str = "feed";
/// GitHub releases.
pub const GITHUB: &str = "github";
/// Gitea or Forgejo releases.
pub const GITEA: &str = "gitea";
/// GitLab releases.
pub const GITLAB: &str = "gitlab";
/// Container image tags.
pub const CONTAINER: &str = "container";
/// A package registry (crates.io, npm, NuGet, PyPI).
pub const PACKAGE: &str = "package";
/// Any JSON API.
pub const JSON_API: &str = "json-api";
/// A web page (selector, pattern or change detection).
pub const WEB_PAGE: &str = "web-page";
/// The key of the optional access token field.
pub const TOKEN: &str = "token";

fn pattern_fields(kind: SourceKindSpec, what: &str) -> SourceKindSpec {
    kind.with_field(
        FieldSpec::new("pattern", "Name pattern", FieldKind::Text)
            .help(&format!("Which {what} to look at. * matches anything, ? one character."))
            .default_value("*")
            .placeholder("MyApp-*.zip"),
    )
    .with_field(
        FieldSpec::new(
            "syntax",
            "Pattern syntax",
            FieldKind::Choice(vec![
                ChoiceOption::new("glob", "Wildcards (* and ?)"),
                ChoiceOption::new("regex", "Regular expression"),
            ]),
        )
        .default_value("glob"),
    )
}

fn token(help: &str) -> FieldSpec {
    FieldSpec::new(TOKEN, "Access token", FieldKind::Secret).help(help)
}

fn asset_pattern() -> FieldSpec {
    FieldSpec::new("assets", "Asset pattern", FieldKind::Text)
        .help("Optional: point each release at its first file matching this (e.g. *windows*.msi). Releases without one are skipped.")
        .placeholder("*-x64-setup.exe")
}

/// Every built-in kind, in the order the add-watch wizard shows them.
pub fn builtin_kinds() -> Vec<SourceKindSpec> {
    vec![
        pattern_fields(
            SourceKindSpec::new(
                FOLDER,
                "Folder or network share",
                "A local folder, a mounted network drive or a UNC share (\\\\server\\share).",
            )
            .with_field(
                FieldSpec::new("path", "Folder", FieldKind::Folder)
                    .required()
                    .help("Credentials-free shares only: Spotter uses your own sign-in to the share.")
                    .placeholder(r"\\buildserver\drops\MyApp"),
            )
            .with_field(
                FieldSpec::new(
                    "entries",
                    "Look at",
                    FieldKind::Choice(vec![
                        ChoiceOption::new("files", "Files"),
                        ChoiceOption::new("folders", "Sub-folders"),
                        ChoiceOption::new("both", "Files and sub-folders"),
                    ]),
                )
                .default_value("files"),
            ),
            "names",
        ),
        pattern_fields(
            SourceKindSpec::new(
                WEB_DIRECTORY,
                "Web directory",
                "An HTTP(S) directory listing, such as an nginx or Apache index page.",
            )
            .with_field(
                FieldSpec::new("url", "Address", FieldKind::Url)
                    .required()
                    .placeholder("https://downloads.example.org/myapp/"),
            ),
            "links",
        ),
        SourceKindSpec::new(
            FEED,
            "RSS or Atom feed",
            "A feed whose entries are releases; the version is read from each title.",
        )
        .with_field(
            FieldSpec::new("url", "Feed address", FieldKind::Url)
                .required()
                .placeholder("https://github.com/owner/repo/releases.atom"),
        ),
        SourceKindSpec::new(GITHUB, "GitHub", "Releases of a GitHub repository (or GitHub Enterprise).")
            .with_field(
                FieldSpec::new("repo", "Repository", FieldKind::Text)
                    .required()
                    .placeholder("owner/repository"),
            )
            .with_field(
                FieldSpec::new("server", "API address", FieldKind::Url)
                    .default_value("https://api.github.com")
                    .help("Only for GitHub Enterprise: https://github.example.com/api/v3"),
            )
            .with_field(asset_pattern())
            .with_field(token(
                "Optional: for private repositories and a higher rate limit. Kept in your credential store.",
            )),
        SourceKindSpec::new(
            GITEA,
            "Gitea or Forgejo",
            "Releases of a repository on a Gitea, Forgejo or Codeberg server.",
        )
        .with_field(
            FieldSpec::new("server", "Server", FieldKind::Url)
                .required()
                .placeholder("https://codeberg.org"),
        )
        .with_field(
            FieldSpec::new("repo", "Repository", FieldKind::Text)
                .required()
                .placeholder("owner/repository"),
        )
        .with_field(asset_pattern())
        .with_field(token("Optional: for private repositories. Kept in your credential store.")),
        SourceKindSpec::new(GITLAB, "GitLab", "Releases of a GitLab project (gitlab.com or self-managed).")
            .with_field(
                FieldSpec::new("project", "Project", FieldKind::Text)
                    .required()
                    .placeholder("group/project"),
            )
            .with_field(FieldSpec::new("server", "Server", FieldKind::Url).default_value("https://gitlab.com"))
            .with_field(asset_pattern())
            .with_field(token("Optional: a personal access token with read_api, for private projects.")),
        SourceKindSpec::new(
            CONTAINER,
            "Container image",
            "Tags of an image in an OCI registry (Docker Hub, GHCR, Quay, your own).",
        )
        .with_field(
            FieldSpec::new("image", "Image", FieldKind::Text)
                .required()
                .placeholder("ghcr.io/owner/image")
                .help("Docker Hub images can be written short: nginx, owner/image."),
        )
        .with_field(FieldSpec::new("username", "User name", FieldKind::Text).help("Optional, for private images."))
        .with_field(token("Optional, for private images. Kept in your credential store.")),
        SourceKindSpec::new(
            PACKAGE,
            "Package registry",
            "Versions of a package on crates.io, npm, NuGet or PyPI.",
        )
        .with_field(
            FieldSpec::new(
                "registry",
                "Registry",
                FieldKind::Choice(vec![
                    ChoiceOption::new("crates-io", "crates.io (Rust)"),
                    ChoiceOption::new("npm", "npm (JavaScript)"),
                    ChoiceOption::new("nuget", "NuGet (.NET)"),
                    ChoiceOption::new("pypi", "PyPI (Python)"),
                ]),
            )
            .required(),
        )
        .with_field(
            FieldSpec::new("package", "Package", FieldKind::Text)
                .required()
                .placeholder("serde"),
        ),
        SourceKindSpec::new(
            WEB_PAGE,
            "Web page",
            "A page that shows the current version, or any page whose text you want to know about when it changes.",
        )
        .with_field(FieldSpec::new("url", "Address", FieldKind::Url).required().placeholder("https://example.org/download"))
        .with_field(
            FieldSpec::new(
                "mode",
                "Watch for",
                FieldKind::Choice(vec![
                    ChoiceOption::new("version", "A version number on the page"),
                    ChoiceOption::new("change", "Any change of the text"),
                ]),
            )
            .default_value("version"),
        )
        .with_field(
            FieldSpec::new("selector", "Element (CSS selector)", FieldKind::Text)
                .placeholder("#latest .version")
                .help("Where the version is, or the part of the page to watch. Empty: the whole page, or the version it states in its metadata."),
        )
        .with_field(FieldSpec::new("attribute", "Attribute", FieldKind::Text).help("Read this attribute of the element instead of its text.").placeholder("data-version"))
        .with_field(
            FieldSpec::new("pattern", "Pattern", FieldKind::Text)
                .help("Regular expression applied to the text; its first group is the version.")
                .placeholder(r"Latest:\s*v?([0-9.]+)"),
        )
        .with_field(
            FieldSpec::new("rendered", "Page needs JavaScript", FieldKind::Toggle)
                .help("Open the page in a browser first (slower, uses more memory)."),
        )
        .with_field(FieldSpec::new("wait", "Wait for element", FieldKind::Text).help("For JavaScript pages: the element that shows when the page is ready.")),
        SourceKindSpec::new(
            JSON_API,
            "JSON API",
            "Any JSON endpoint, read with simple selectors ($.items[*].version).",
        )
        .with_field(FieldSpec::new("url", "Address", FieldKind::Url).required())
        .with_field(
            FieldSpec::new("items", "Releases at", FieldKind::Text)
                .required()
                .default_value("$[*]")
                .help("Where the list of releases is: $.releases[*] for an array, $.versions.* for an object's entries."),
        )
        .with_field(
            FieldSpec::new("version", "Version", FieldKind::Text)
                .required()
                .default_value("$.version")
                .help("Relative to one release: $.tag, or @key for the entry's key."),
        )
        .with_field(FieldSpec::new("name", "Name", FieldKind::Text).placeholder("$.name"))
        .with_field(
            FieldSpec::new("location", "Location", FieldKind::Text)
                .placeholder("$.url")
                .help("A selector, or an address with {version} in it."),
        )
        .with_field(FieldSpec::new("published", "Published", FieldKind::Text).placeholder("$.date"))
        .with_field(FieldSpec::new("size", "Size", FieldKind::Text).placeholder("$.size"))
        .with_field(FieldSpec::new("sha256", "SHA-256", FieldKind::Text).placeholder("$.sha256"))
        .with_field(token("Optional: sent as a Bearer token. Kept in your credential store.")),
    ]
}

/// One built-in kind.
pub fn builtin(id: &str) -> Option<SourceKindSpec> {
    builtin_kinds().into_iter().find(|k| k.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_unique_and_well_formed() {
        let kinds = builtin_kinds();
        let mut ids: Vec<&str> = kinds.iter().map(|k| k.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), kinds.len());
        for k in &kinds {
            assert!(!k.label.is_empty() && !k.description.is_empty() && !k.id.contains('/'));
            for f in &k.fields {
                // Defaults pass their own field's check.
                assert!(f.default.is_empty() || f.check(Some(&f.default)).is_ok(), "{}.{}", k.id, f.key);
            }
        }
        assert!(builtin(GITHUB).is_some() && builtin("nope").is_none());
    }
}
