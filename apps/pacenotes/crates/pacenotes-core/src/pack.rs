//! [`ServerPack`]: the declarative language-server pack of tier 3 (proposal 20.4), parsed and validated at the edge.
//!
//! A pack is data. [`ServerPack::parse`] turns the TOML text into a typed value or refuses it with a message that says
//! what to fix: unknown keys, a command that is neither a bare program name nor an absolute path, a download without a
//! SHA-256 or without `https`, empty globs, bounds exceeded. Nothing here starts a process or fetches a file.

use serde::{Deserialize, Serialize};

use crate::language::Language;

/// The largest pack file accepted.
pub const MAX_PACK_BYTES: usize = 64 * 1024;
/// The most globs, detect entries, arguments and discovery files a pack may list.
pub const MAX_LIST: usize = 64;

/// Why a pack is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackError {
    /// The file is larger than [`MAX_PACK_BYTES`].
    #[error("the pack file is {0} bytes; at most {MAX_PACK_BYTES} are accepted")]
    TooLarge(usize),
    /// The TOML is malformed, a key is unknown or a value has the wrong type.
    #[error("the pack file is not valid: {0}")]
    Syntax(String),
    /// A value is invalid. `field` is the key and `fix` says what to do.
    #[error("pack field `{field}`: {problem}. {fix}")]
    Invalid {
        /// The key, for example `server.command`.
        field: &'static str,
        /// What is wrong.
        problem: String,
        /// What to do about it.
        fix: &'static str,
    },
    /// Two packs have the same id.
    #[error("two packs have the id `{0}`")]
    DuplicateId(String),
}

fn invalid(field: &'static str, problem: impl Into<String>, fix: &'static str) -> PackError {
    PackError::Invalid {
        field,
        problem: problem.into(),
        fix,
    }
}

/// The LSP requests Pacenotes uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LspFeature {
    /// `textDocument/definition`.
    Definition,
    /// `textDocument/references`.
    References,
    /// Call hierarchy.
    CallHierarchy,
    /// Type hierarchy.
    TypeHierarchy,
    /// `textDocument/documentSymbol`.
    DocumentSymbol,
    /// `workspace/symbol`.
    WorkspaceSymbol,
    /// Semantic tokens.
    SemanticTokens,
    /// Diagnostics.
    Diagnostics,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPack {
    pack: RawPackInfo,
    server: RawServer,
    #[serde(default)]
    workspace: RawWorkspace,
    features: RawFeatures,
    scip: Option<RawScip>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPackInfo {
    id: String,
    name: Option<String>,
    languages: Vec<String>,
    globs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawServer {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    detect: Vec<String>,
    install: Option<RawInstall>,
    download: Option<RawDownload>,
    #[serde(default)]
    init_options: toml::Table,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInstall {
    offer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDownload {
    url: String,
    sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkspace {
    #[serde(default)]
    discover: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFeatures {
    #[serde(rename = "use")]
    use_: Vec<LspFeature>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScip {
    command: String,
}

/// How to find an installed server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "how", rename_all = "snake_case")]
pub enum Detect {
    /// A program of this name on `PATH`.
    Path {
        /// The program name.
        program: String,
    },
    /// A rustup component of this name.
    RustupComponent {
        /// The component name.
        component: String,
    },
}

/// A pinned download, offered on the user's click only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Download {
    /// An `https` URL.
    pub url: String,
    /// The expected SHA-256 as 64 lowercase hex digits.
    pub sha256: String,
}

/// A validated tier-3 language-server pack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerPack {
    /// The pack id (a slug).
    pub id: String,
    /// A display name, when given.
    pub name: Option<String>,
    /// The languages it serves.
    pub languages: Vec<Language>,
    /// The files it applies to.
    pub globs: Vec<String>,
    /// The server program: a bare name or an absolute path.
    pub command: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Environment variables for the server.
    pub env: std::collections::BTreeMap<String, String>,
    /// How to find an installed server.
    pub detect: Vec<Detect>,
    /// A command shown to the user to install the server (never run without a click).
    pub install_offer: Option<String>,
    /// A pinned download.
    pub download: Option<Download>,
    /// LSP initialisation options, passed through (least-execution defaults live here).
    pub init_options: toml::Table,
    /// Build-config files that mark a workspace root.
    pub discover: Vec<String>,
    /// The LSP features used.
    pub features: Vec<LspFeature>,
    /// An optional SCIP indexer command for batch indexing.
    pub scip_command: Option<String>,
}

fn is_slug(s: &str) -> bool {
    let mut chars = s.chars();
    s.len() <= 48
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

const SHELL_META: &[char] = &[';', '|', '&', '$', '`', '<', '>', '\n', '\r', '\0'];

fn check_command(field: &'static str, command: &str) -> Result<(), PackError> {
    const FIX: &str = "Use the program's name (found on PATH) or its absolute path; arguments go in `args`.";
    if command.is_empty() || command.len() > 1024 {
        return Err(invalid(field, "it is empty or too long", FIX));
    }
    if command.contains(SHELL_META) {
        return Err(invalid(field, "it contains a shell metacharacter or a control character", FIX));
    }
    let bytes = command.as_bytes();
    let absolute = command.starts_with('/')
        || (bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/'));
    if absolute {
        return Ok(());
    }
    let bare = command.len() <= 64
        && command
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'));
    if bare {
        Ok(())
    } else {
        Err(invalid(
            field,
            format!("{command:?} is neither a bare program name nor an absolute path"),
            FIX,
        ))
    }
}

fn check_globs(field: &'static str, globs: &[String], required: bool) -> Result<(), PackError> {
    if required && globs.is_empty() {
        return Err(invalid(field, "the list is empty", "List at least one glob such as \"**/*.rs\"."));
    }
    if globs.len() > MAX_LIST {
        return Err(invalid(field, format!("more than {MAX_LIST} entries"), "Merge globs."));
    }
    for g in globs {
        if g.is_empty() || g.len() > 256 || g.contains('\0') || g.starts_with('/') || g.split('/').any(|s| s == "..") {
            return Err(invalid(
                field,
                format!("{g:?} is not a workspace-relative glob"),
                "Use relative globs without `..`, for example \"**/*.rs\".",
            ));
        }
    }
    Ok(())
}

fn parse_detect(entry: &str) -> Result<Detect, PackError> {
    const FIX: &str = "Use `path:<program>` or `rustup:component=<name>`.";
    let word = |s: &str| {
        !s.is_empty()
            && s.len() <= 64
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
    };
    if let Some(p) = entry.strip_prefix("path:").filter(|p| word(p)) {
        return Ok(Detect::Path { program: p.to_owned() });
    }
    if let Some(c) = entry.strip_prefix("rustup:component=").filter(|c| word(c)) {
        return Ok(Detect::RustupComponent { component: c.to_owned() });
    }
    Err(invalid("server.detect", format!("{entry:?} is not a known detection rule"), FIX))
}

impl ServerPack {
    /// Parses and validates the text of a pack file.
    pub fn parse(text: &str) -> Result<ServerPack, PackError> {
        if text.len() > MAX_PACK_BYTES {
            return Err(PackError::TooLarge(text.len()));
        }
        let raw: RawPack = toml::from_str(text).map_err(|e| PackError::Syntax(e.message().to_owned()))?;
        if !is_slug(&raw.pack.id) {
            return Err(invalid(
                "pack.id",
                format!("{:?} is not an id", raw.pack.id),
                "Use lowercase letters, digits and `-`, starting with a letter.",
            ));
        }
        if raw.pack.name.as_deref().is_some_and(|n| n.is_empty() || n.len() > 120) {
            return Err(invalid(
                "pack.name",
                "it is empty or longer than 120 characters",
                "Give a short display name or leave it out.",
            ));
        }
        if raw.pack.languages.is_empty() || raw.pack.languages.len() > MAX_LIST {
            return Err(invalid(
                "pack.languages",
                "list between 1 and 64 languages",
                "Name the languages the server serves, for example [\"rust\"].",
            ));
        }
        let languages = raw
            .pack
            .languages
            .iter()
            .map(|l| {
                Language::parse(l).map_err(|e| invalid("pack.languages", e.to_string(), "Use language ids such as \"rust\" or \"cpp\"."))
            })
            .collect::<Result<Vec<_>, _>>()?;
        check_globs("pack.globs", &raw.pack.globs, true)?;
        check_command("server.command", &raw.server.command)?;
        if raw.server.args.len() > MAX_LIST || raw.server.args.iter().any(|a| a.len() > 1024 || a.contains('\0')) {
            return Err(invalid(
                "server.args",
                "too many or too long arguments",
                "Keep at most 64 arguments of at most 1024 characters.",
            ));
        }
        if raw.server.env.len() > MAX_LIST
            || raw
                .server
                .env
                .iter()
                .any(|(k, v)| k.is_empty() || k.contains(['=', '\0']) || v.contains('\0') || v.len() > 1024)
        {
            return Err(invalid(
                "server.env",
                "an invalid name or value, or too many",
                "Use plain NAME = \"value\" pairs.",
            ));
        }
        if raw.server.detect.len() > MAX_LIST / 2 {
            return Err(invalid(
                "server.detect",
                "more than 32 entries",
                "Keep the detection rules you need.",
            ));
        }
        let detect = raw.server.detect.iter().map(|d| parse_detect(d)).collect::<Result<Vec<_>, _>>()?;
        if let Some(i) = &raw.server.install
            && (i.offer.is_empty() || i.offer.len() > 512 || i.offer.contains(['\n', '\r', '\0']))
        {
            return Err(invalid(
                "server.install.offer",
                "it is empty, too long or has a line break",
                "Give the one-line command the user may run.",
            ));
        }
        let download = raw.server.download.as_ref().map(check_download).transpose()?;
        check_globs("workspace.discover", &raw.workspace.discover, false)?;
        if raw.features.use_.is_empty() {
            return Err(invalid(
                "features.use",
                "the list is empty",
                "List the LSP features the pack uses, for example [\"definition\", \"references\"].",
            ));
        }
        if let Some(s) = &raw.scip {
            check_command("scip.command", s.command.split_whitespace().next().unwrap_or(""))?;
            if s.command.len() > 1024 || s.command.contains(SHELL_META) {
                return Err(invalid(
                    "scip.command",
                    "it is too long or contains a shell metacharacter",
                    "Give the indexer command and its arguments only.",
                ));
            }
        }
        Ok(ServerPack {
            id: raw.pack.id,
            name: raw.pack.name,
            languages,
            globs: raw.pack.globs,
            command: raw.server.command,
            args: raw.server.args,
            env: raw.server.env,
            detect,
            install_offer: raw.server.install.map(|i| i.offer),
            download,
            init_options: raw.server.init_options,
            discover: raw.workspace.discover,
            features: raw.features.use_,
            scip_command: raw.scip.map(|s| s.command),
        })
    }

    /// Refuses a set of packs in which two share an id.
    pub fn check_unique(packs: &[ServerPack]) -> Result<(), PackError> {
        let mut seen = std::collections::BTreeSet::new();
        match packs.iter().find(|p| !seen.insert(p.id.as_str())) {
            Some(p) => Err(PackError::DuplicateId(p.id.clone())),
            None => Ok(()),
        }
    }
}

fn check_download(d: &RawDownload) -> Result<Download, PackError> {
    const FIX: &str = "A download is pinned: give an `https` URL and the file's SHA-256 (64 hex digits). Unpinned downloads are refused.";
    let Some(hash) = d.sha256.as_deref() else {
        return Err(invalid("server.download.sha256", "the download is not pinned by a hash", FIX));
    };
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("server.download.sha256", "it is not 64 hexadecimal digits", FIX));
    }
    if !d.url.starts_with("https://")
        || d.url.len() > 2048
        || d.url.chars().any(|c| c.is_whitespace() || c.is_control())
        || d.url.len() == "https://".len()
    {
        return Err(invalid("server.download.url", "it is not an `https` URL", FIX));
    }
    Ok(Download {
        url: d.url.clone(),
        sha256: hash.to_ascii_lowercase(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
[pack]
id = "rust"
languages = ["rust"]
globs = ["**/*.rs"]

[server]
command = "rust-analyzer"
detect = ["path:rust-analyzer", "rustup:component=rust-analyzer"]
install = { offer = "rustup component add rust-analyzer" }
init_options = { cargo = { buildScripts = { enable = false } } }

[workspace]
discover = ["Cargo.toml"]

[features]
use = ["definition", "references", "callHierarchy", "documentSymbol"]

[scip]
command = "rust-analyzer scip ."
"#;

    fn with(edit: impl Fn(&str) -> String) -> Result<ServerPack, PackError> {
        ServerPack::parse(&edit(GOOD))
    }

    fn message(r: Result<ServerPack, PackError>) -> String {
        r.unwrap_err().to_string()
    }

    #[test]
    fn the_proposals_example_pack_is_accepted() {
        let p = ServerPack::parse(GOOD).unwrap();
        assert_eq!((p.id.as_str(), p.command.as_str()), ("rust", "rust-analyzer"));
        assert_eq!(p.features.len(), 4);
        assert_eq!(
            p.detect[1],
            Detect::RustupComponent {
                component: "rust-analyzer".into()
            }
        );
        assert!(p.init_options.contains_key("cargo"));
        assert_eq!(p.scip_command.as_deref(), Some("rust-analyzer scip ."));
    }

    /// The regression test of the plan (A1): a pack with an unpinned download is refused with a message.
    #[test]
    fn a_pack_with_an_unpinned_download_is_refused_with_a_message() {
        let unpinned = with(|g| {
            g.replace(
                "[workspace]",
                "[server.download]\nurl = \"https://example.invalid/ra.gz\"\n\n[workspace]",
            )
        });
        let msg = message(unpinned);
        assert!(
            msg.contains("server.download.sha256") && msg.contains("not pinned") && msg.contains("Unpinned downloads are refused"),
            "{msg}"
        );
        let short = with(|g| {
            g.replace(
                "[workspace]",
                "[server.download]\nurl = \"https://example.invalid/ra.gz\"\nsha256 = \"abc\"\n\n[workspace]",
            )
        });
        assert!(message(short).contains("64 hexadecimal digits"));
        let http = with(|g| {
            g.replace(
                "[workspace]",
                &format!(
                    "[server.download]\nurl = \"http://example.invalid/ra.gz\"\nsha256 = \"{}\"\n\n[workspace]",
                    "a".repeat(64)
                ),
            )
        });
        assert!(message(http).contains("`https` URL"));
    }

    #[test]
    fn a_pinned_download_is_accepted_and_the_hash_is_lowered() {
        let ok = with(|g| {
            g.replace(
                "[workspace]",
                &format!(
                    "[server.download]\nurl = \"https://example.invalid/ra.gz\"\nsha256 = \"{}\"\n\n[workspace]",
                    "AB".repeat(32)
                ),
            )
        })
        .unwrap();
        assert_eq!(ok.download.unwrap().sha256, "ab".repeat(32));
    }

    #[test]
    fn unknown_keys_are_refused() {
        assert!(message(with(|g| g.replace("[features]", "[extra]\nx = 1\n\n[features]"))).contains("unknown field"));
        assert!(message(with(|g| g.replace("globs = [", "color = \"red\"\nglobs = ["))).contains("unknown field"));
    }

    #[test]
    fn commands_are_bare_names_or_absolute_paths() {
        for ok in [
            "clangd",
            "C:\\\\Tools\\\\clangd.exe",
            "/usr/bin/clangd",
            "/opt/with space/clangd",
            "dotnet-ls_1.2",
        ] {
            assert!(
                with(|g| g.replace("\"rust-analyzer\"\ndetect", &format!("\"{ok}\"\ndetect"))).is_ok(),
                "{ok}"
            );
        }
        for bad in ["bin/clangd", "./clangd", "clangd; rm -rf /", "a|b", "$HOME/x", "", "clangd --stdio"] {
            let r = with(|g| g.replace("\"rust-analyzer\"\ndetect", &format!("\"{bad}\"\ndetect")));
            assert!(message(r).contains("server.command"), "{bad:?}");
        }
    }

    #[test]
    fn lists_are_required_and_bounded() {
        assert!(message(with(|g| g.replace("globs = [\"**/*.rs\"]", "globs = []"))).contains("pack.globs"));
        assert!(
            message(with(|g| g.replace(
                "use = [\"definition\", \"references\", \"callHierarchy\", \"documentSymbol\"]",
                "use = []"
            )))
            .contains("features.use")
        );
        assert!(message(with(|g| g.replace("\"**/*.rs\"", "\"../x\""))).contains("workspace-relative"));
        assert!(message(with(|g| g.replace("\"definition\"", "\"hover\""))).contains("unknown variant"));
        assert!(message(with(|g| g.replace("path:rust-analyzer", "magic:x"))).contains("detection rule"));
        assert!(message(with(|g| g.replace("id = \"rust\"", "id = \"Rust Pack\""))).contains("pack.id"));
        assert!(message(with(|g| g.replace("[\"rust\"]", "[\"no way\"]"))).contains("pack.languages"));
    }

    #[test]
    fn a_huge_or_malformed_file_is_refused_not_parsed() {
        assert!(matches!(
            ServerPack::parse(&" ".repeat(MAX_PACK_BYTES + 1)),
            Err(PackError::TooLarge(_))
        ));
        assert!(matches!(ServerPack::parse("[pack"), Err(PackError::Syntax(_))));
        assert!(matches!(ServerPack::parse(""), Err(PackError::Syntax(_))));
    }

    #[test]
    fn duplicate_ids_are_refused() {
        let a = ServerPack::parse(GOOD).unwrap();
        assert_eq!(
            ServerPack::check_unique(&[a.clone(), a.clone()]),
            Err(PackError::DuplicateId("rust".into()))
        );
        assert_eq!(ServerPack::check_unique(&[a]), Ok(()));
    }

    #[test]
    fn parsing_never_panics_on_hostile_input() {
        paddock_testkit::assert_never_panics(&[GOOD.as_bytes()], |b| ServerPack::parse(&String::from_utf8_lossy(b)).is_ok());
    }

    #[test]
    fn every_bound_and_field_check_has_a_message() {
        let many = |n: usize, item: &str| format!("[{}]", vec![item; n].join(","));
        assert!(
            message(with(
                |g| g.replace("globs = [\"**/*.rs\"]", &format!("globs = {}", many(MAX_LIST + 1, "\"a\"")))
            ))
            .contains("more than 64")
        );
        assert!(message(with(|g| g.replace("id = \"rust\"", "id = \"rust\"\nname = \"\""))).contains("pack.name"));
        assert!(
            message(with(|g| g.replace(
                "command = \"rust-analyzer\"",
                &format!("command = \"rust-analyzer\"\nargs = {}", many(MAX_LIST + 1, "\"-v\""))
            )))
            .contains("server.args")
        );
        assert!(
            message(with(|g| g.replace(
                "command = \"rust-analyzer\"",
                "command = \"rust-analyzer\"\nenv = { \"A=B\" = \"x\" }"
            )))
            .contains("server.env")
        );
        assert!(
            message(with(|g| g.replace(
                "detect = [\"path:rust-analyzer\", \"rustup:component=rust-analyzer\"]",
                &format!("detect = {}", many(33, "\"path:a\""))
            )))
            .contains("server.detect")
        );
        assert!(message(with(|g| g.replace("rustup component add rust-analyzer", ""))).contains("server.install.offer"));
        assert!(message(with(|g| g.replace("rust-analyzer scip .", "rust-analyzer scip ; rm x"))).contains("scip.command"));
        assert!(message(with(|g| g.replace("languages = [\"rust\"]", "languages = []"))).contains("pack.languages"));
    }

    #[test]
    fn a_pack_can_leave_out_the_optional_parts() {
        let min = "[pack]\nid = \"go\"\nlanguages = [\"go\"]\nglobs = [\"**/*.go\"]\n[server]\ncommand = \"gopls\"\n[features]\nuse = [\"definition\"]\n";
        let p = ServerPack::parse(min).unwrap();
        assert!(p.name.is_none() && p.download.is_none() && p.scip_command.is_none() && p.detect.is_empty() && p.discover.is_empty());
    }
}
