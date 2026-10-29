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
#[serde(rename_all = "snake_case")]
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

fn word(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
}

impl Detect {
    fn check(&self) -> Result<(), PackError> {
        let ok = match self {
            Detect::Path { program } => word(program),
            Detect::RustupComponent { component } => word(component),
        };
        if ok {
            Ok(())
        } else {
            Err(invalid(
                "server.detect",
                format!("{self:?} is not a valid detection rule"),
                "Use `path:<program>` or `rustup:component=<name>`.",
            ))
        }
    }
}

/// A pinned download, offered on the user's click only. Reading one from the wire goes through [`Download::new`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DownloadWire")]
pub struct Download {
    url: String,
    sha256: String,
}

/// The wire form of a [`Download`].
#[derive(Serialize, Deserialize)]
pub(crate) struct DownloadWire {
    url: String,
    sha256: String,
}

impl TryFrom<DownloadWire> for Download {
    type Error = PackError;
    fn try_from(w: DownloadWire) -> Result<Download, PackError> {
        Download::new(&w.url, Some(&w.sha256))
    }
}

impl Download {
    /// A pinned download: an `https` URL and the file's SHA-256 (64 hex digits). Unpinned downloads are refused.
    pub fn new(url: &str, sha256: Option<&str>) -> Result<Download, PackError> {
        const FIX: &str =
            "A download is pinned: give an `https` URL and the file's SHA-256 (64 hex digits). Unpinned downloads are refused.";
        let Some(hash) = sha256 else {
            return Err(invalid("server.download.sha256", "the download is not pinned by a hash", FIX));
        };
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("server.download.sha256", "it is not 64 hexadecimal digits", FIX));
        }
        if !url.starts_with("https://")
            || url.len() > 2048
            || url.chars().any(|c| c.is_whitespace() || c.is_control())
            || url.len() == "https://".len()
        {
            return Err(invalid("server.download.url", "it is not an `https` URL", FIX));
        }
        Ok(Download {
            url: url.to_owned(),
            sha256: hash.to_ascii_lowercase(),
        })
    }

    /// The `https` URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The expected SHA-256, 64 lowercase hex digits.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// The wire form of a [`ServerPack`]: every field as data. Reading a `ServerPack` from any serde format reads this and then
/// runs the same validation as the TOML parser, so a pack cannot be built around the checks.
#[derive(Serialize, Deserialize)]
pub(crate) struct PackWire {
    pub(crate) id: String,
    pub(crate) name: Option<String>,
    pub(crate) languages: Vec<Language>,
    pub(crate) globs: Vec<String>,
    pub(crate) command: String,
    pub(crate) args: Vec<String>,
    pub(crate) env: std::collections::BTreeMap<String, String>,
    pub(crate) detect: Vec<Detect>,
    pub(crate) install_offer: Option<String>,
    pub(crate) download: Option<Download>,
    pub(crate) init_options: String,
    pub(crate) discover: Vec<String>,
    pub(crate) features: Vec<LspFeature>,
    pub(crate) scip_command: Option<String>,
}

impl TryFrom<PackWire> for ServerPack {
    type Error = PackError;
    fn try_from(w: PackWire) -> Result<ServerPack, PackError> {
        ServerPack::from_wire(w)
    }
}

/// A validated tier-3 language-server pack. Fields are private: a pack exists only if it passed [`ServerPack::parse`] or the
/// same validation on the wire.
///
/// `init_options` is carried as JSON text (an object, at most 16 KiB) so the type crosses postcard. The core does not
/// judge its content: the pack loader in `pacenotes-lsp` owns the check of known execution switches (rust-analyzer's
/// `cargo.buildScripts` and `procMacro`, and the like) against the repository's trust state (ADR 0002).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PackWire")]
pub struct ServerPack {
    id: String,
    name: Option<String>,
    languages: Vec<Language>,
    globs: Vec<String>,
    command: String,
    args: Vec<String>,
    env: std::collections::BTreeMap<String, String>,
    detect: Vec<Detect>,
    install_offer: Option<String>,
    download: Option<Download>,
    init_options: String,
    discover: Vec<String>,
    features: Vec<LspFeature>,
    scip_command: Option<String>,
}

fn is_slug(s: &str) -> bool {
    let mut chars = s.chars();
    s.len() <= 48
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

const SHELL_META: &[char] = &[';', '|', '&', '$', '`', '<', '>', '\n', '\r', '\0'];
/// The largest `init_options` JSON text.
pub const MAX_INIT_OPTIONS_BYTES: usize = 16 * 1024;

/// Environment variables a pack may never set: they make the server load code of the pack's choosing.
fn forbidden_env(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n == "LD_PRELOAD" || n == "LD_LIBRARY_PATH" || n.starts_with("DYLD_") || n == "NODE_OPTIONS"
}

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
        let languages = raw
            .pack
            .languages
            .iter()
            .map(|l| {
                Language::parse(l).map_err(|e| invalid("pack.languages", e.to_string(), "Use language ids such as \"rust\" or \"cpp\"."))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let detect = raw.server.detect.iter().map(|d| parse_detect(d)).collect::<Result<Vec<_>, _>>()?;
        let download = raw
            .server
            .download
            .as_ref()
            .map(|d| Download::new(&d.url, d.sha256.as_deref()))
            .transpose()?;
        let init_options = serde_json::to_string(&raw.server.init_options).map_err(|_| {
            invalid(
                "server.init_options",
                "it cannot be written as JSON",
                "Use plain tables, strings, numbers and booleans.",
            )
        })?;
        ServerPack::from_wire(PackWire {
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
            init_options,
            discover: raw.workspace.discover,
            features: raw.features.use_,
            scip_command: raw.scip.map(|s| s.command),
        })
    }

    /// The one validation, used by the TOML parser and by every serde format.
    pub(crate) fn from_wire(w: PackWire) -> Result<ServerPack, PackError> {
        if !is_slug(&w.id) {
            return Err(invalid(
                "pack.id",
                format!("{:?} is not an id", w.id),
                "Use lowercase letters, digits and `-`, starting with a letter.",
            ));
        }
        if w.name.as_deref().is_some_and(|n| n.is_empty() || n.len() > 120) {
            return Err(invalid(
                "pack.name",
                "it is empty or longer than 120 characters",
                "Give a short display name or leave it out.",
            ));
        }
        if w.languages.is_empty() || w.languages.len() > MAX_LIST {
            return Err(invalid(
                "pack.languages",
                "list between 1 and 64 languages",
                "Name the languages the server serves, for example [\"rust\"].",
            ));
        }
        check_globs("pack.globs", &w.globs, true)?;
        check_command("server.command", &w.command)?;
        if w.args.len() > MAX_LIST || w.args.iter().any(|a| a.len() > 1024 || a.contains('\0')) {
            return Err(invalid(
                "server.args",
                "too many or too long arguments",
                "Keep at most 64 arguments of at most 1024 characters.",
            ));
        }
        if w.env.len() > MAX_LIST
            || w.env
                .iter()
                .any(|(k, v)| k.is_empty() || k.contains(['=', '\0']) || v.contains('\0') || v.len() > 1024)
        {
            return Err(invalid(
                "server.env",
                "an invalid name or value, or too many",
                "Use plain NAME = \"value\" pairs.",
            ));
        }
        if let Some(name) = w.env.keys().find(|k| forbidden_env(k)) {
            return Err(invalid(
                "server.env",
                format!("{name} makes the server load code of the pack's choosing"),
                "Remove it; LD_PRELOAD, LD_LIBRARY_PATH, DYLD_* and NODE_OPTIONS are never set by a pack.",
            ));
        }
        if w.detect.len() > MAX_LIST / 2 {
            return Err(invalid(
                "server.detect",
                "more than 32 entries",
                "Keep the detection rules you need.",
            ));
        }
        w.detect.iter().try_for_each(Detect::check)?;
        if let Some(offer) = &w.install_offer
            && (offer.is_empty() || offer.len() > 512 || offer.contains(['\n', '\r', '\0']))
        {
            return Err(invalid(
                "server.install.offer",
                "it is empty, too long or has a line break",
                "Give the one-line command the user may run.",
            ));
        }
        if w.init_options.len() > MAX_INIT_OPTIONS_BYTES {
            return Err(invalid(
                "server.init_options",
                "it is larger than 16 KiB",
                "Keep only the options the server needs.",
            ));
        }
        if !matches!(
            serde_json::from_str::<serde_json::Value>(&w.init_options),
            Ok(serde_json::Value::Object(_))
        ) {
            return Err(invalid("server.init_options", "it is not a JSON object", "Use a table of options."));
        }
        check_globs("workspace.discover", &w.discover, false)?;
        if w.features.is_empty() {
            return Err(invalid(
                "features.use",
                "the list is empty",
                "List the LSP features the pack uses, for example [\"definition\", \"references\"].",
            ));
        }
        if let Some(s) = &w.scip_command {
            check_command("scip.command", s.split_whitespace().next().unwrap_or(""))?;
            if s.len() > 1024 || s.contains(SHELL_META) {
                return Err(invalid(
                    "scip.command",
                    "it is too long or contains a shell metacharacter",
                    "Give the indexer command and its arguments only.",
                ));
            }
        }
        Ok(ServerPack {
            id: w.id,
            name: w.name,
            languages: w.languages,
            globs: w.globs,
            command: w.command,
            args: w.args,
            env: w.env,
            detect: w.detect,
            install_offer: w.install_offer,
            download: w.download,
            init_options: w.init_options,
            discover: w.discover,
            features: w.features,
            scip_command: w.scip_command,
        })
    }

    /// True when `w` passes the validation (for the wire tests).
    #[cfg(test)]
    pub(crate) fn from_wire_for_test(w: PackWire) -> bool {
        ServerPack::from_wire(w).is_ok()
    }

    /// Refuses a set of packs in which two share an id.
    pub fn check_unique(packs: &[ServerPack]) -> Result<(), PackError> {
        let mut seen = std::collections::BTreeSet::new();
        match packs.iter().find(|p| !seen.insert(p.id.as_str())) {
            Some(p) => Err(PackError::DuplicateId(p.id.clone())),
            None => Ok(()),
        }
    }

    /// The pack id (a slug).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// A display name, when given.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The languages it serves.
    pub fn languages(&self) -> &[Language] {
        &self.languages
    }

    /// The files it applies to.
    pub fn globs(&self) -> &[String] {
        &self.globs
    }

    /// The server program: a bare name or an absolute path.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Its arguments.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Environment variables for the server (never a code-loading one).
    pub fn env(&self) -> &std::collections::BTreeMap<String, String> {
        &self.env
    }

    /// How to find an installed server.
    pub fn detect(&self) -> &[Detect] {
        &self.detect
    }

    /// A command shown to the user to install the server (never run without a click).
    pub fn install_offer(&self) -> Option<&str> {
        self.install_offer.as_deref()
    }

    /// A pinned download.
    pub fn download(&self) -> Option<&Download> {
        self.download.as_ref()
    }

    /// LSP initialisation options as a JSON object text; the pack loader judges its content (see the type's docs).
    pub fn init_options_json(&self) -> &str {
        &self.init_options
    }

    /// Build-config files that mark a workspace root.
    pub fn discover(&self) -> &[String] {
        &self.discover
    }

    /// The LSP features used.
    pub fn features(&self) -> &[LspFeature] {
        &self.features
    }

    /// An optional SCIP indexer command for batch indexing.
    pub fn scip_command(&self) -> Option<&str> {
        self.scip_command.as_deref()
    }
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
        assert_eq!((p.id(), p.command()), ("rust", "rust-analyzer"));
        assert_eq!(p.features().len(), 4);
        assert_eq!(
            p.detect()[1],
            Detect::RustupComponent {
                component: "rust-analyzer".into()
            }
        );
        assert!(p.init_options_json().contains("buildScripts"));
        assert_eq!(p.scip_command(), Some("rust-analyzer scip ."));
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
        assert_eq!(ok.download().unwrap().sha256(), "ab".repeat(32));
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
        assert!(
            p.name().is_none() && p.download().is_none() && p.scip_command().is_none() && p.detect().is_empty() && p.discover().is_empty()
        );
    }

    #[test]
    fn code_loading_environment_variables_are_refused() {
        for name in [
            "LD_PRELOAD",
            "ld_library_path",
            "DYLD_INSERT_LIBRARIES",
            "DYLD_FRAMEWORK_PATH",
            "NODE_OPTIONS",
        ] {
            let r = with(|g| {
                g.replace(
                    "command = \"rust-analyzer\"",
                    &format!("command = \"rust-analyzer\"\nenv = {{ {name} = \"x\" }}"),
                )
            });
            assert!(message(r).contains("load code"), "{name}");
        }
        assert!(
            with(|g| g.replace(
                "command = \"rust-analyzer\"",
                "command = \"rust-analyzer\"\nenv = { RUST_LOG = \"info\" }"
            ))
            .is_ok()
        );
    }

    #[test]
    fn init_options_are_bounded_json_objects() {
        let big = "x".repeat(MAX_INIT_OPTIONS_BYTES);
        assert!(
            message(with(
                |g| g.replace("init_options = {", &format!("init_options = {{ big = \"{big}\", "))
            ))
            .contains("16 KiB")
        );
    }

    #[test]
    fn a_pack_round_trips_and_the_wire_is_validated_like_the_file() {
        let p = ServerPack::parse(GOOD).unwrap();
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<ServerPack>(&json).unwrap(), p);
        assert_eq!(postcard::from_bytes::<ServerPack>(&postcard::to_stdvec(&p).unwrap()).unwrap(), p);
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v["command"] = "bin/x".into();
        assert!(serde_json::from_value::<ServerPack>(v).is_err());
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v["env"] = serde_json::json!({ "LD_PRELOAD": "/x.so" });
        assert!(serde_json::from_value::<ServerPack>(v).is_err());
        assert!(serde_json::from_str::<Download>(r#"{"url":"http://x","sha256":"abc"}"#).is_err());
    }

    #[test]
    fn every_getter_returns_what_was_parsed_and_detect_rules_are_rechecked() {
        let p = ServerPack::parse(GOOD).unwrap();
        assert_eq!(p.name(), None);
        assert_eq!(p.languages().len(), 1);
        assert_eq!(p.globs(), ["**/*.rs"]);
        assert!(p.args().is_empty() && p.env().is_empty());
        assert_eq!(p.install_offer(), Some("rustup component add rust-analyzer"));
        assert_eq!(p.discover(), ["Cargo.toml"]);
        let dl = with(|g| {
            g.replace(
                "[workspace]",
                &format!(
                    "[server.download]\nurl = \"https://example.invalid/ra.gz\"\nsha256 = \"{}\"\n\n[workspace]",
                    "ab".repeat(32)
                ),
            )
        })
        .unwrap();
        assert_eq!(dl.download().unwrap().url(), "https://example.invalid/ra.gz");
        let mut wire = serde_json::to_value(&p).unwrap();
        wire["detect"] = serde_json::json!([{ "path": { "program": "a b" } }]);
        assert!(
            serde_json::from_value::<ServerPack>(wire).is_err(),
            "a detect rule read from the wire is checked again"
        );
        let many = format!("[{}]", vec!["\"path:a\""; 33].join(","));
        assert!(
            message(with(|g| g.replace(
                "detect = [\"path:rust-analyzer\", \"rustup:component=rust-analyzer\"]",
                &format!("detect = {many}")
            )))
            .contains("server.detect")
        );
    }
}
