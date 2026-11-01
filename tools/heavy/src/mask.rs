//! Secrets never reach a record or a log: the command line is masked word by word (not by substring) before it is written.
//! `-u`/`-p` mean a credential only for known tools (`cargo -p crate` and `docker run -p 80:80` are left alone).

use crate::timefmt::oneline;

const SECRET_WORDS: &[&str] = &[
    "token",
    "password",
    "passwd",
    "pass",
    "pwd",
    "secret",
    "key",
    "apikey",
    "pw",
    "passphrase",
    "credential",
    "credentials",
    "authorization",
    "auth",
    "bearer",
    "pat",
];

/// The longest masked command line kept (characters).
pub const MAX_CMD: usize = 500;

/// Whether `s` holds a whole secret word (split on non-alphanumerics and camelCase).
pub fn is_secret_word(s: &str) -> bool {
    let mut spaced = String::new();
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if let Some(p) = prev
            && (p.is_ascii_lowercase() || p.is_ascii_digit())
            && c.is_ascii_uppercase()
        {
            spaced.push(' ');
        }
        spaced.push(if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { ' ' });
        prev = Some(c);
    }
    spaced.split_whitespace().any(|w| SECRET_WORDS.contains(&w))
}

fn base(a: &str) -> &str {
    let b = a.rsplit(['/', '\\']).next().unwrap_or(a);
    b.strip_suffix(".exe").unwrap_or(b)
}

fn hide_url_credentials(a: &str) -> String {
    if let Some((scheme, rest)) = a.split_once("://") {
        let auth_end = rest.find('/').unwrap_or(rest.len());
        let auth = &rest[..auth_end];
        if let Some(at) = auth.rfind('@') {
            return format!("{scheme}://***@{}", &rest[at + 1..]);
        }
    }
    a.to_string()
}

/// The command line with secrets replaced by `***`, on one line, at most [`MAX_CMD`] characters.
pub fn mask_cmd(args: &[String]) -> String {
    oneline(&mask_args(args).join(" ")).chars().take(MAX_CMD).collect()
}

/// The masked arguments, one for one.
pub fn mask_args(args: &[String]) -> Vec<String> {
    let (mut tool, mut login) = ("", false);
    for a in args {
        match base(a) {
            "mysql" | "mysqldump" | "mariadb" | "sshpass" if tool.is_empty() => tool = "mysql",
            "docker" if tool.is_empty() => tool = "docker",
            "curl" if tool.is_empty() => tool = "curl",
            "login" => login = true,
            _ => {}
        }
    }
    let pw = tool == "mysql" || (tool == "docker" && login);
    let curl = tool == "curl";
    let mut out: Vec<String> = Vec::with_capacity(args.len());
    let mut pending = false;
    for raw in args {
        let mut a = redact_shapes(&hide_url_credentials(raw));
        if pending && !a.starts_with('-') {
            a = "***".into();
            pending = false;
        } else {
            pending = false;
            if matches!(a.as_str(), "-u" | "--user" | "--proxy-user") {
                pending = curl;
            } else if a.starts_with("-u") && a.len() > 2 {
                if curl {
                    a = "-u***".into();
                }
            } else if a == "-p" {
                pending = pw;
            } else if a.starts_with("-p") && a.len() > 2 {
                if pw {
                    a = "-p***".into();
                }
            } else if let Some((k, v)) = a.split_once('=') {
                if is_secret_word(k) || ((v.contains(':') || v.contains(' ')) && is_secret_word(v)) {
                    a = format!("{k}=***");
                }
            } else if a.starts_with('-') {
                pending = is_secret_word(&a);
            } else if (a.contains(':') || a.contains(' ')) && is_secret_word(&a) {
                // `X-Api-Key:` and `//reg/:_authToken` are keys: the next word is the value; `Authorization: Bearer x` is whole.
                if !a.contains(' ') {
                    pending = true;
                }
                a = "***".into();
            }
        }
        out.push(a);
    }
    out
}

/// Whether `run` (a stretch of `[A-Za-z0-9_-]`) is a well-known token shape: GitHub (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`,
/// `github_pat_`), GitLab (`glpat-`), Slack (`xox?-`), AWS access key ids (`AKIA`, `ASIA`) and `sk-`/`sk_live_`/`sk_test_` keys.
fn token_shape(run: &str) -> bool {
    let long = |prefix: &str, min: usize| run.starts_with(prefix) && run.len() >= min;
    let b = run.as_bytes();
    ["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "glpat-", "sk-", "sk_live_", "sk_test_"]
        .iter()
        .any(|p| long(p, p.len() + 16))
        || long("github_pat_", 30)
        || (run.starts_with("xox") && b.get(3).is_some_and(|c| b"abprs".contains(c)) && b.get(4) == Some(&b'-') && run.len() >= 16)
        || ((run.starts_with("AKIA") || run.starts_with("ASIA"))
            && run.len() == 20
            && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
}

/// `s` with every well-known token shape replaced by `***`, wherever it stands in the text.
pub fn redact_shapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        out.push_str(if token_shape(run) { "***" } else { run });
        run.clear();
    };
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// Masks free text (a command's stderr tail) line by line, word by word, with the same rules as a command line.
pub fn mask_text(text: &str) -> String {
    text.lines()
        .map(|l| {
            let words: Vec<String> = l.split_whitespace().map(String::from).collect();
            let mut masked = mask_args(&words);
            // `Authorization: Bearer xyz` split into words: the scheme word follows a hidden key, the secret follows it.
            for i in 2..words.len() {
                let scheme = words[i - 1].to_ascii_lowercase();
                if masked[i - 1] == "***" && words[i - 2].ends_with(':') && matches!(scheme.as_str(), "bearer" | "basic" | "token") {
                    masked[i] = "***".into();
                }
            }
            masked.join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
