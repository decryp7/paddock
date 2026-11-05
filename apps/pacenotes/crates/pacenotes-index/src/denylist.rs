//! The secret denylist and redaction (AGENTS.md "Secrets are redacted from tool output", audit PN1).
//!
//! The rules are **data**: `denylist.default.toml`, in a format a shared crate can own later (rule 42; ADR 0005). This module
//! parses a pack, matches paths and masks text; it holds no list of names of its own.

use std::borrow::Cow;
use std::sync::OnceLock;

use pacenotes_core::RepoPath;
use regex::Regex;
use serde::Deserialize;

/// The default pack, compiled in.
pub const DEFAULT_PACK: &str = include_str!("denylist.default.toml");

/// Why a path is withheld.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denied {
    /// The file name is a credential file by name or pattern (`.env`, `*.env`, `id_rsa*`, `secrets.*`).
    Name,
    /// An extension marks key material (`.pem`, `.p8`, `.key.bak`).
    Extension,
    /// The path is inside a folder that holds credentials or repository internals (`.ssh`, `.git`, `.config/gcloud`).
    Folder,
    /// A name or suffix the user added.
    Extra,
    /// The pack could not be read, so everything is withheld (fail closed).
    Everything,
}

/// Why a pack was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackError {
    /// The TOML is malformed or has an unknown key.
    #[error("the denylist pack is not valid: {0}")]
    Syntax(String),
    /// The format version is not 1.
    #[error("the denylist pack has format version {0}; this build reads version 1")]
    Version(u32),
    /// A redaction rule's regular expression does not compile.
    #[error("redaction rule {name:?}: {why}")]
    Regex {
        /// The rule's name.
        name: String,
        /// Why.
        why: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackFile {
    version: u32,
    names: NamesFile,
    extensions: ExtensionsFile,
    folders: FoldersFile,
    #[serde(default)]
    redact: Vec<RedactFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NamesFile {
    #[serde(default)]
    exact: Vec<String>,
    #[serde(default)]
    globs: Vec<String>,
    #[serde(default)]
    plain_globs: Vec<String>,
    #[serde(default)]
    allow: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtensionsFile {
    #[serde(default)]
    list: Vec<String>,
    #[serde(default)]
    backups: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FoldersFile {
    #[serde(default)]
    list: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RedactFile {
    name: String,
    regex: String,
    replace: String,
}

/// A parsed pack.
#[derive(Debug, Clone)]
struct Pack {
    exact: Vec<String>,
    globs: Vec<String>,
    plain_globs: Vec<String>,
    allow: Vec<String>,
    extensions: Vec<String>,
    backups: Vec<String>,
    folders: Vec<Vec<String>>,
    redact: Vec<(Regex, String)>,
    everything: bool,
}

impl Pack {
    fn parse(text: &str) -> Result<Pack, PackError> {
        let file: PackFile = toml::from_str(text).map_err(|e| PackError::Syntax(e.message().to_owned()))?;
        if file.version != 1 {
            return Err(PackError::Version(file.version));
        }
        let lower = |v: Vec<String>| v.into_iter().map(|s| s.to_lowercase()).collect::<Vec<_>>();
        let mut redact = Vec::new();
        for r in file.redact {
            let re = Regex::new(&r.regex).map_err(|e| PackError::Regex {
                name: r.name.clone(),
                why: e.to_string(),
            })?;
            redact.push((re, r.replace));
        }
        Ok(Pack {
            exact: lower(file.names.exact),
            globs: lower(file.names.globs),
            plain_globs: lower(file.names.plain_globs),
            allow: lower(file.names.allow),
            extensions: lower(file.extensions.list),
            backups: lower(file.extensions.backups),
            folders: lower(file.folders.list)
                .into_iter()
                .map(|f| f.split('/').map(str::to_owned).collect())
                .collect(),
            redact,
            everything: false,
        })
    }

    /// What a build with an unreadable default pack uses: nothing is readable, nothing is served.
    fn deny_everything() -> Pack {
        Pack {
            exact: Vec::new(),
            globs: Vec::new(),
            plain_globs: Vec::new(),
            allow: Vec::new(),
            extensions: Vec::new(),
            backups: Vec::new(),
            folders: Vec::new(),
            redact: Vec::new(),
            everything: true,
        }
    }
}

fn default_pack() -> &'static Pack {
    static PACK: OnceLock<Pack> = OnceLock::new();
    PACK.get_or_init(|| Pack::parse(DEFAULT_PACK).unwrap_or_else(|_| Pack::deny_everything()))
}

/// `*` matches any run (also empty), `?` one character; both texts are compared as given (callers lower-case them).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Which paths are never read, indexed, served or searched. The default pack always applies; a user pack and the extra names
/// are added to it, so the list can be extended and never shrunk.
#[derive(Debug, Clone)]
pub struct Denylist {
    pack: Option<Pack>,
    extra_names: Vec<String>,
    extra_suffixes: Vec<String>,
}

impl Default for Denylist {
    fn default() -> Self {
        Denylist::new()
    }
}

impl Denylist {
    /// The default pack.
    pub fn new() -> Denylist {
        Denylist {
            pack: None,
            extra_names: Vec::new(),
            extra_suffixes: Vec::new(),
        }
    }

    /// The default pack plus a user pack in the same format. The user pack **adds** names, patterns, extensions, folders and
    /// redaction rules; what the default denies stays denied (a user `allow` cannot lift it) and the default's redaction
    /// rules always run first.
    pub fn from_pack(text: &str) -> Result<Denylist, PackError> {
        Ok(Denylist {
            pack: Some(Pack::parse(text)?),
            extra_names: Vec::new(),
            extra_suffixes: Vec::new(),
        })
    }

    /// The default pack plus file names (`secrets.local.toml`) and suffixes (`.token`) the user added; both match
    /// case-insensitively and an empty name or suffix is ignored.
    pub fn with_extra(names: &[&str], suffixes: &[&str]) -> Denylist {
        Denylist {
            pack: None,
            extra_names: names.iter().filter(|n| !n.is_empty()).map(|n| n.to_lowercase()).collect(),
            extra_suffixes: suffixes.iter().filter(|n| !n.is_empty()).map(|n| n.to_lowercase()).collect(),
        }
    }

    /// Why `path` is withheld, or `None` when it may be read: the default pack first, then the user pack.
    pub fn check(&self, path: &RepoPath) -> Option<Denied> {
        let default = default_pack();
        if default.everything || self.pack.as_ref().is_some_and(|u| u.everything) {
            return Some(Denied::Everything);
        }
        self.check_with(default, path)
            .or_else(|| self.pack.as_ref().and_then(|user| self.check_with(user, path)))
            .or_else(|| self.check_extra(path))
    }

    fn check_extra(&self, path: &RepoPath) -> Option<Denied> {
        let name = path.as_str().rsplit('/').next().unwrap_or("").to_lowercase();
        (self.extra_names.contains(&name) || self.extra_suffixes.iter().any(|s| name.ends_with(s.as_str()))).then_some(Denied::Extra)
    }

    fn check_with(&self, pack: &Pack, path: &RepoPath) -> Option<Denied> {
        let lower = path.as_str().to_lowercase();
        let segments: Vec<&str> = lower.split('/').collect();
        let name = segments.last().copied().unwrap_or("");
        for folder in &pack.folders {
            if segments.windows(folder.len()).any(|w| w.iter().zip(folder).all(|(a, b)| a == b)) {
                return Some(Denied::Folder);
            }
        }
        if pack.allow.iter().any(|a| glob_match(a, name)) {
            return None;
        }
        let plain = !name.contains('.');
        if pack.exact.iter().any(|e| e == name)
            || pack.globs.iter().any(|g| glob_match(g, name))
            || (plain && pack.plain_globs.iter().any(|g| glob_match(g, name)))
        {
            return Some(Denied::Name);
        }
        if has_key_extension(name, &pack.extensions, &pack.backups) {
            return Some(Denied::Extension);
        }
        None
    }

    /// Masks what looks like a credential in `text` with this list's redaction rules.
    pub fn redact<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let mut out = Cow::Borrowed(text);
        for (re, replacement) in default_pack().redact.iter().chain(self.pack.iter().flat_map(|p| p.redact.iter())) {
            if re.is_match(&out) {
                out = Cow::Owned(re.replace_all(&out, replacement.as_str()).into_owned());
            }
        }
        out
    }
}

/// An extension of `name` (not its first dot-separated part) is in `list` and is the last one or is followed only by backup
/// suffixes. A leading dot with one more part (`.key`) is a hidden file named `key`, not an extension.
fn has_key_extension(name: &str, list: &[String], backups: &[String]) -> bool {
    let parts: Vec<&str> = name.split('.').collect();
    if parts.len() < 2 || (parts.len() == 2 && parts[0].is_empty()) {
        return false;
    }
    (1..parts.len()).any(|i| list.iter().any(|e| e == parts[i]) && parts[i + 1..].iter().all(|later| backups.iter().any(|b| b == later)))
}

/// Masks what looks like a credential in `text` with the default rules: private-key blocks (also PGP), URL credentials,
/// bearer tokens, cloud, forge and package tokens, JSON web tokens, and the value of an assignment to a name that says
/// `password`, `secret`, `token` or `key`. It errs towards masking. Every text that leaves the engine for a client (an MCP
/// reply, a log line, the UI) passes this.
pub fn redact(text: &str) -> Cow<'_, str> {
    Denylist::new().redact(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> RepoPath {
        RepoPath::parse(s).unwrap()
    }

    #[test]
    fn the_default_pack_parses_and_every_redaction_rule_compiles() {
        let pack = Pack::parse(DEFAULT_PACK).unwrap();
        assert!(!pack.everything && pack.redact.len() >= 7 && pack.exact.len() > 10);
        assert!(
            !default_pack().everything,
            "the compiled-in pack is valid, so the fail-closed fallback is not in use"
        );
    }

    #[test]
    fn a_bad_pack_is_refused_with_a_message_and_the_fallback_denies_everything() {
        assert!(matches!(
            Pack::parse("version = 1\n[names]\n[extensions]\n[folders]\nbogus = 1"),
            Err(PackError::Syntax(_))
        ));
        assert_eq!(
            Pack::parse("version = 2\n[names]\n[extensions]\n[folders]\n").unwrap_err(),
            PackError::Version(2)
        );
        let bad_regex = "version = 1\n[names]\n[extensions]\n[folders]\n[[redact]]\nname = \"x\"\nregex = \"(\"\nreplace = \"y\"\n";
        let err = Pack::parse(bad_regex).unwrap_err();
        assert!(matches!(&err, PackError::Regex { name, .. } if name == "x") && err.to_string().contains("\"x\""));
        let d = Denylist {
            pack: Some(Pack::deny_everything()),
            extra_names: vec![],
            extra_suffixes: vec![],
        };
        assert_eq!(d.check(&p("src/lib.rs")), Some(Denied::Everything));
        assert!(Denylist::from_pack("not toml [").is_err());
    }

    /// ADR 0005: a user pack extends the default and never shrinks it.
    #[test]
    fn a_user_pack_merges_over_the_default_and_cannot_lift_it() {
        let pack = "version = 1\n[names]\nexact = [\"mine.txt\"]\nallow = [\".env\", \"id_rsa\", \"*.env\", \"prod.env\"]\n[extensions]\nlist = [\"zzz\"]\n[folders]\nlist = [\"secret-dir\"]\n[[redact]]\nname = \"mine\"\nregex = \"MINE-[0-9]+\"\nreplace = \"[mine]\"\n";
        let d = Denylist::from_pack(pack).unwrap();
        assert_eq!(d.check(&p("a/Mine.txt")), Some(Denied::Name), "the user's name is added");
        assert_eq!(d.check(&p("secret-dir/x")), Some(Denied::Folder));
        assert_eq!(d.check(&p("k.zzz")), Some(Denied::Extension));
        for default_denied in [".env", "id_rsa", "prod.env", "certs/x.pem", ".ssh/known_hosts"] {
            assert!(
                d.check(&p(default_denied)).is_some(),
                "{default_denied}: the default still denies it, whatever the user allows"
            );
        }
        assert_eq!(d.check(&p("src/lib.rs")), None);
        let out = d.redact("password = 'abcdefgh' and MINE-42");
        assert!(
            out.contains("[mine]") && !out.contains("abcdefgh"),
            "the default rules run, then the user's: {out}"
        );
    }

    #[test]
    fn glob_matching_handles_stars_questions_and_edges() {
        for (pattern, text, want) in [
            ("*.env", "prod.env", true),
            ("*.env", ".env", true),
            (".env.*", ".env.local", true),
            (".env.*", ".env", false),
            ("*deploy*key*", "github-deploy-key", true),
            ("*deploy*key*", "deploy_key.pub", true),
            ("a?c", "abc", true),
            ("a?c", "ac", false),
            ("*", "", true),
            ("", "", true),
            ("a*b*c", "aXXbYYc", true),
            ("a*b*c", "aXXbYY", false),
        ] {
            assert_eq!(glob_match(pattern, text), want, "{pattern} vs {text}");
        }
    }

    /// Every credential file the review listed, plus the ones that were denied before, are denied.
    #[test]
    fn the_regression_table_of_denied_paths() {
        let d = Denylist::new();
        for path in [
            // before
            ".env",
            "app/.env",
            ".ENV",
            ".env.production",
            "a/.env.local",
            "id_rsa",
            "home/id_ed25519",
            "id_rsa_backup",
            "credentials.json",
            "service-account-prod.json",
            "terraform.tfstate.backup",
            "certs/server.pem",
            "a/b/signing.KEY",
            "store.p12",
            "prod.tfvars",
            ".git/config",
            "sub/.git",
            ".ssh/known_hosts",
            "deep/.aws/credentials",
            "node_modules/pkg/.env",
            "node_modules/a/b/id_rsa",
            // the review
            "prod.env",
            "config/app.env",
            ".env_prod",
            ".env-local",
            ".pgpass",
            ".my.cnf",
            ".kube/config",
            "kubeconfig",
            ".docker/config.json",
            ".yarnrc.yml",
            "auth.json",
            "prod.tfstate",
            "secrets.toml",
            ".streamlit/secrets.toml",
            "deploy_key",
            "github-deploy-key",
            "key.asc",
            "AuthKey_ABC.p8",
            "client.ovpn",
            ".s3cfg",
            ".boto",
            "_netrc",
            ".config/gcloud/application_default_credentials.json",
            "x.pem.bak",
            "backup.key.old",
            "secrets.json",
            "SECRETS.YAML",
            "vault.age",
        ] {
            assert!(d.check(&p(path)).is_some(), "{path} must be withheld");
        }
    }

    #[test]
    fn the_regression_table_of_allowed_paths() {
        let d = Denylist::new();
        for path in [
            ".env.example",
            ".env.sample",
            ".env.template",
            "id_rsa.pub",
            "id_rsa.PUB",
            "src/lib.rs",
            "key.rs",
            "keys/readme.md",
            "environment.ts",
            "pem.txt",
            ".key",
            "Cargo.toml",
            "monkey.key.rs",
            "my.config/x",
            "docker/compose.yml",
            "config/gcloud/readme.md",
            "secrets_manager.rs",
            "deploy.sh",
            "docs/deploy-keys.md",
            "deploy-keys.txt",
            "notes/deploy.key.md",
            "a.pem.rs",
            "kubectl.md",
            "notes.age.md",
        ] {
            assert_eq!(d.check(&p(path)), None, "{path} may be read");
        }
    }

    #[test]
    fn denial_reasons_are_reported() {
        let d = Denylist::new();
        assert_eq!(d.check(&p("prod.env")), Some(Denied::Name));
        assert_eq!(d.check(&p("x.pem.bak")), Some(Denied::Extension));
        assert_eq!(d.check(&p("a/.config/gcloud/x")), Some(Denied::Folder));
        assert_eq!(d.check(&p("a/.config/other/x")), None);
    }

    #[test]
    fn the_list_can_be_extended_not_shrunk() {
        let d = Denylist::with_extra(&["Secrets.Local.toml", ""], &[".token", ""]);
        assert_eq!(
            d.check(&p("a/secrets.local.toml")),
            Some(Denied::Name),
            "already matched by secrets.*"
        );
        assert_eq!(d.check(&p("api.TOKEN")), Some(Denied::Extra));
        assert_eq!(d.check(&p("odd.local.toml")), None);
        let named = Denylist::with_extra(&["odd.local.toml"], &[]);
        assert_eq!(named.check(&p("x/odd.local.toml")), Some(Denied::Extra));
        assert_eq!(d.check(&p(".env")), Some(Denied::Name), "the defaults still apply");
        assert_eq!(d.check(&p("fine.toml")), None);
    }

    /// Every case the review listed, plus the ones that were masked before.
    #[test]
    fn the_regression_table_of_redactions() {
        let key = "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\nabc\n-----END RSA PRIVATE KEY-----";
        let pgp = "-----BEGIN PGP PRIVATE KEY BLOCK-----\n\nlQdGBF\nabc\n-----END PGP PRIVATE KEY BLOCK-----";
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdefghijk";
        for (input, want) in [
            (format!("before {key} after"), "before [redacted private key] after".to_owned()),
            (
                "-----BEGIN PRIVATE KEY-----\ncut off".to_owned(),
                "[redacted private key]".to_owned(),
            ),
            (format!("x {pgp} y"), "x [redacted private key] y".to_owned()),
            ("key AKIAABCDEFGHIJKLMNOP end".to_owned(), "key [redacted key] end".to_owned()),
            (format!("t=ghp_{}", "a".repeat(36)), "t=[redacted token]".to_owned()),
            (
                "DATABASE_URL=postgres://app:hunter2pw@db.example.invalid:5432/app".to_owned(),
                "DATABASE_URL=postgres://[redacted]@db.example.invalid:5432/app".to_owned(),
            ),
            (
                "clone https://user:ghs_notreallyatoken@example.invalid/r.git".to_owned(),
                "clone https://[redacted]@example.invalid/r.git".to_owned(),
            ),
            (
                "Authorization: Bearer abcdef0123456789xyz".to_owned(),
                "Authorization: Bearer [redacted]".to_owned(),
            ),
            ("key sk_live_abcdefghijklmnop end".to_owned(), "key [redacted token] end".to_owned()),
            ("key sk_test_abcdefghijklmnop".to_owned(), "key [redacted token]".to_owned()),
            (
                format!("token glpat-{}", "a1b2c3d4e5f6g7h8i9j0"),
                "token [redacted token]".to_owned(),
            ),
            (format!("npm_{}", "a".repeat(36)), "[redacted token]".to_owned()),
            (format!("key=AIza{}", "B".repeat(35)), "key=[redacted token]".to_owned()),
            ("pwd = 'hunter2hunter2'".to_owned(), "pwd = [redacted]".to_owned()),
            ("API_TOKEN=PN1-DUMMY-SECRET-0f3a9c71".to_owned(), "API_TOKEN=[redacted]".to_owned()),
            ("password: \"hunter2hunter2\"".to_owned(), "password: [redacted]".to_owned()),
            ("{\"api_key\": \"abcdefgh\"}".to_owned(), "{\"api_key\": [redacted]}".to_owned()),
            (jwt.to_owned(), "[redacted token]".to_owned()),
        ] {
            assert_eq!(redact(&input), want, "{input}");
        }
        for plain in [
            "fn parse_token(input: &str) -> Token",
            "let password_len = 8;",
            "token_count = 3",
            "see the README",
            "https://example.invalid/path",
            "a bearer of news",
        ] {
            assert_eq!(redact(plain), plain, "{plain}");
        }
        assert!(matches!(redact("nothing here"), Cow::Borrowed(_)));
    }
}
