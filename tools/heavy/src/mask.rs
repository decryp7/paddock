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
    let (mut tool, mut login) = ("", false);
    for a in args {
        match base(a) {
            "mysql" | "mysqldump" | "mariadb" if tool.is_empty() => tool = "mysql",
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
        let mut a = hide_url_credentials(raw);
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
                if a.ends_with(':') {
                    pending = true;
                }
                a = "***".into();
            }
        }
        out.push(a);
    }
    oneline(&out.join(" ")).chars().take(MAX_CMD).collect()
}
