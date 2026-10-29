//! The secret denylist and redaction (AGENTS.md "Secrets are redacted from tool output", audit PN1).

use std::borrow::Cow;
use std::sync::OnceLock;

use pacenotes_core::RepoPath;
use regex::Regex;

/// Why a path is withheld.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denied {
    /// The file name is a credential file by name (`.env`, `id_rsa`, ...).
    Name,
    /// The extension marks key material (`.pem`, `.key`, ...).
    Extension,
    /// The path is inside a folder that holds credentials or repository internals (`.ssh`, `.git`, ...).
    Folder,
    /// A pattern the user added.
    Extra,
}

const NAMES: &[&str] = &[
    ".env",
    ".envrc",
    ".netrc",
    ".npmrc",
    ".pypirc",
    ".htpasswd",
    ".git-credentials",
    ".dockercfg",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    "credentials",
    "credentials.json",
    "secrets.json",
    "secrets.yml",
    "secrets.yaml",
];
const NAME_PREFIXES: &[&str] = &["service-account", "terraform.tfstate"];
const ENV_TEMPLATES: &[&str] = &[".env.example", ".env.sample", ".env.template", ".env.dist"];
const EXTENSIONS: &[&str] = &["pem", "key", "p12", "pfx", "jks", "keystore", "kdbx", "ppk", "gpg", "tfvars"];
const FOLDERS: &[&str] = &[".git", ".ssh", ".aws", ".gnupg", ".azure"];

/// Which paths are never read, indexed, served or searched. The default list can be extended, never shrunk.
#[derive(Debug, Clone, Default)]
pub struct Denylist {
    extra_names: Vec<String>,
    extra_suffixes: Vec<String>,
}

impl Denylist {
    /// The default list.
    pub fn new() -> Denylist {
        Denylist::default()
    }

    /// The default list plus file names (`secrets.local.toml`) and suffixes (`.token`) the user added; both match
    /// case-insensitively and a name or suffix of 0 characters is ignored.
    pub fn with_extra(names: &[&str], suffixes: &[&str]) -> Denylist {
        Denylist {
            extra_names: names.iter().filter(|n| !n.is_empty()).map(|n| n.to_lowercase()).collect(),
            extra_suffixes: suffixes.iter().filter(|n| !n.is_empty()).map(|n| n.to_lowercase()).collect(),
        }
    }

    /// Why `path` is withheld, or `None` when it may be read.
    pub fn check(&self, path: &RepoPath) -> Option<Denied> {
        let lower = path.as_str().to_lowercase();
        let mut parts: Vec<&str> = lower.split('/').collect();
        let name = parts.pop().unwrap_or("");
        if parts.iter().any(|p| FOLDERS.contains(p)) {
            return Some(Denied::Folder);
        }
        if FOLDERS.contains(&name) {
            return Some(Denied::Folder);
        }
        if name.ends_with(".pub") {
            return None;
        }
        if NAMES.contains(&name) || NAME_PREFIXES.iter().any(|p| name.starts_with(p)) {
            return Some(Denied::Name);
        }
        if name.starts_with(".env.") && !ENV_TEMPLATES.contains(&name) {
            return Some(Denied::Name);
        }
        if name.starts_with("id_rsa") || name.starts_with("id_ed25519") || name.starts_with("id_ecdsa") || name.starts_with("id_dsa") {
            return Some(Denied::Name);
        }
        if name
            .rsplit_once('.')
            .is_some_and(|(stem, ext)| !stem.is_empty() && EXTENSIONS.contains(&ext))
        {
            return Some(Denied::Extension);
        }
        if self.extra_names.iter().any(|n| n == name) || self.extra_suffixes.iter().any(|s| name.ends_with(s.as_str())) {
            return Some(Denied::Extra);
        }
        None
    }
}

fn patterns() -> &'static [(Regex, &'static str)] {
    static PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let build = |p: &str, r: &'static str| Regex::new(p).ok().map(|re| (re, r));
        [
            build(r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?(-----END [A-Z ]*PRIVATE KEY-----|\z)", "[redacted private key]"),
            build(r"\bAKIA[0-9A-Z]{16}\b", "[redacted key]"),
            build(r"\bgh[pousr]_[A-Za-z0-9]{30,}\b", "[redacted token]"),
            build(r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b", "[redacted token]"),
            build(r"\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b", "[redacted token]"),
            build(r"(?i)\b((?:[a-z0-9_.-]*(?:password|passwd|secret|token|api[_-]?key|access[_-]?key))[\x22']?\s*[:=]\s*)(?:[\x22'][^\x22'\n]{4,}[\x22']|[^\s\x22',;]{6,})", "${1}[redacted]"),
        ]
        .into_iter()
        .flatten()
        .collect()
    })
}

/// Masks what looks like a credential in `text`: private-key blocks, cloud and forge tokens, JSON web tokens and the value of
/// an assignment to a name containing `password`, `secret`, `token` or `api key`. It errs towards masking. Every text that
/// leaves the engine for a client (an MCP reply, a log line, the UI) passes this.
pub fn redact(text: &str) -> Cow<'_, str> {
    let mut out = Cow::Borrowed(text);
    for (re, replacement) in patterns() {
        if re.is_match(&out) {
            out = Cow::Owned(re.replace_all(&out, *replacement).into_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> RepoPath {
        RepoPath::parse(s).unwrap()
    }

    #[test]
    fn credential_files_are_denied_by_name_extension_and_folder() {
        let d = Denylist::new();
        for (path, why) in [
            (".env", Denied::Name),
            ("app/.env", Denied::Name),
            (".ENV", Denied::Name),
            (".env.production", Denied::Name),
            ("a/.env.local", Denied::Name),
            ("id_rsa", Denied::Name),
            ("home/id_ed25519", Denied::Name),
            ("id_rsa_backup", Denied::Name),
            ("credentials.json", Denied::Name),
            ("service-account-prod.json", Denied::Name),
            ("terraform.tfstate.backup", Denied::Name),
            ("certs/server.pem", Denied::Extension),
            ("a/b/signing.KEY", Denied::Extension),
            ("store.p12", Denied::Extension),
            ("prod.tfvars", Denied::Extension),
            (".git/config", Denied::Folder),
            ("sub/.git", Denied::Folder),
            (".ssh/known_hosts", Denied::Folder),
            ("deep/.aws/credentials", Denied::Folder),
        ] {
            assert_eq!(d.check(&p(path)), Some(why), "{path}");
        }
    }

    #[test]
    fn templates_public_keys_and_ordinary_files_are_allowed() {
        let d = Denylist::new();
        for path in [
            ".env.example",
            ".env.sample",
            ".env.template",
            "id_rsa.pub",
            "src/lib.rs",
            "key.rs",
            "keys/readme.md",
            "environment.ts",
            "pem.txt",
            ".key",
            "Cargo.toml",
        ] {
            assert_eq!(d.check(&p(path)), None, "{path}");
        }
    }

    #[test]
    fn the_list_can_be_extended_not_shrunk() {
        let d = Denylist::with_extra(&["Secrets.Local.toml", ""], &[".token", ""]);
        assert_eq!(d.check(&p("a/secrets.local.toml")), Some(Denied::Extra));
        assert_eq!(d.check(&p("api.TOKEN")), Some(Denied::Extra));
        assert_eq!(d.check(&p(".env")), Some(Denied::Name), "the defaults still apply");
        assert_eq!(d.check(&p("fine.toml")), None);
    }

    #[test]
    fn every_built_in_pattern_compiles() {
        assert_eq!(patterns().len(), 6, "a pattern that fails to compile would silently mask nothing");
    }

    #[test]
    fn credential_looking_text_is_masked_and_ordinary_text_is_not() {
        let key = "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\nabc\n-----END RSA PRIVATE KEY-----";
        assert_eq!(redact(&format!("before {key} after")), "before [redacted private key] after");
        assert_eq!(redact("-----BEGIN PRIVATE KEY-----\ncut off"), "[redacted private key]");
        assert_eq!(redact("key AKIAABCDEFGHIJKLMNOP end"), "key [redacted key] end");
        assert_eq!(redact(&format!("t=ghp_{}", "a".repeat(36))), "t=[redacted token]");
        assert_eq!(redact("API_TOKEN=PN1-DUMMY-SECRET-0f3a9c71"), "API_TOKEN=[redacted]");
        assert_eq!(redact("password: \"hunter2hunter2\""), "password: [redacted]");
        assert_eq!(redact("{\"api_key\": \"abcdefgh\"}"), "{\"api_key\": [redacted]}");
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdefghijk";
        assert_eq!(redact(jwt), "[redacted token]");
        for plain in [
            "fn parse_token(input: &str) -> Token",
            "let password_len = 8;",
            "token_count = 3",
            "see the README",
        ] {
            assert_eq!(redact(plain), plain, "{plain}");
        }
        assert!(matches!(redact("nothing here"), Cow::Borrowed(_)));
    }
}
