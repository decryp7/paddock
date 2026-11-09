//! The shell side of the CI model (ADR 0060 §2.2, §2.3, §2.5): a `run:` block or a `paddock.ci.json` command list becomes
//! simple commands, each with its working folder, whether it can fail the step, and whether it runs unconditionally.
//! `conformance/ci/run-commands.sh` semantics are modelled: one command per line, a `cd` holds for the lines after it.

use std::collections::BTreeMap;

/// One simple command of a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmd {
    /// The command text, environment assignments included (`PW_PORT=1 npx playwright test`).
    pub text: String,
    /// 0-based index of the physical line in the block.
    pub line: usize,
    /// The folder it runs in, from the repository root (`""` is the root); `None` when a `cd` could not be resolved.
    pub cwd: Option<String>,
    /// Its failure does not fail the step (`|| true`, `; true`, after `set +e`).
    pub report_only: bool,
    /// It sits inside an `if`/`case` or behind a test (`[ -f x ] && cmd`).
    pub conditional: bool,
}

/// Normalises a slash path (`a/b/../c` is `a/c`); `None` when it climbs out of the root.
pub fn normalise(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

/// Joins a folder and a relative path (an absolute or variable path gives `None`).
pub fn join(cwd: &str, rel: &str) -> Option<String> {
    let rel = rel.trim_matches(|c| c == '"' || c == '\'');
    if rel.starts_with('/') || rel.contains('$') || rel.contains('`') || rel.starts_with('~') {
        return None;
    }
    normalise(&format!("{cwd}/{rel}"))
}

/// Replaces `$X` and `${X}` for names in `env` whose value is one line; other names stay as written.
pub fn expand_vars(text: &str, env: &BTreeMap<String, String>) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '$' && i + 1 < c.len() {
            let (name, end) = if c[i + 1] == '{' {
                match c[i + 2..].iter().position(|x| *x == '}') {
                    Some(p) => (c[i + 2..i + 2 + p].iter().collect::<String>(), i + 3 + p),
                    None => (String::new(), i + 1),
                }
            } else {
                let mut j = i + 1;
                while j < c.len() && (c[j].is_ascii_alphanumeric() || c[j] == '_') {
                    j += 1;
                }
                (c[i + 1..j].iter().collect::<String>(), j)
            };
            if let Some(v) = env.get(&name).filter(|v| !v.contains('\n') && !name.is_empty()) {
                out.push_str(v);
                i = end;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Strips the workspace marker: `<ws>/a/../b` is `b`, `<ws>` is the root.
pub fn strip_workspace(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(p) = rest.find("<ws>") {
        out.push_str(&rest[..p]);
        let after = &rest[p + 4..];
        let end = after
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            .unwrap_or(after.len());
        let path = after[..end].trim_start_matches('/');
        out.push_str(&normalise(path).filter(|n| !n.is_empty()).unwrap_or_else(|| ".".to_string()));
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sep {
    Start,
    And,
    Or,
    Semi,
    Pipe,
}

/// Splits a physical line into simple commands on `&&`, `||`, `;` and `|`, outside quotes and `$( )`/`( )`.
fn split_simple(line: &str) -> Vec<(Sep, String)> {
    let c: Vec<char> = line.chars().collect();
    let mut out: Vec<(Sep, String)> = Vec::new();
    let mut cur = String::new();
    let mut sep = Sep::Start;
    let (mut sq, mut dq, mut depth) = (false, false, 0i32);
    let mut i = 0;
    let flush = |cur: &mut String, sep: Sep, out: &mut Vec<(Sep, String)>| {
        let t = cur.trim().to_string();
        if !t.is_empty() {
            out.push((sep, t));
        }
        cur.clear();
    };
    while i < c.len() {
        let ch = c[i];
        if sq {
            sq = ch != '\'';
        } else if dq {
            if ch == '\\' && i + 1 < c.len() {
                cur.push(ch);
                i += 1;
                cur.push(c[i]);
                i += 1;
                continue;
            }
            dq = ch != '"';
        } else {
            match ch {
                '#' if cur.trim().is_empty() || cur.ends_with(' ') => break,
                '\'' => sq = true,
                '"' => dq = true,
                '(' => depth += 1,
                ')' => depth -= 1,
                '&' if depth <= 0 && c.get(i + 1) == Some(&'&') => {
                    flush(&mut cur, sep, &mut out);
                    sep = Sep::And;
                    i += 2;
                    continue;
                }
                '|' if depth <= 0 && c.get(i + 1) == Some(&'|') => {
                    flush(&mut cur, sep, &mut out);
                    sep = Sep::Or;
                    i += 2;
                    continue;
                }
                '|' if depth <= 0 => {
                    flush(&mut cur, sep, &mut out);
                    sep = Sep::Pipe;
                    i += 1;
                    continue;
                }
                ';' if depth <= 0 => {
                    flush(&mut cur, sep, &mut out);
                    sep = Sep::Semi;
                    i += 1;
                    continue;
                }
                _ => {}
            }
        }
        cur.push(ch);
        i += 1;
    }
    flush(&mut cur, sep, &mut out);
    out
}

fn strip_keywords(seg: &str) -> (String, bool) {
    let mut s = seg.trim().to_string();
    let mut guarded = false;
    loop {
        let before = s.clone();
        for kw in [
            "if ", "then ", "else ", "elif ", "do ", "while ", "until ", "! ", "time ", "{ ", "( ",
        ] {
            if let Some(r) = s.strip_prefix(kw) {
                if kw == "if " || kw == "elif " || kw == "while " || kw == "until " {
                    guarded = true;
                }
                s = r.trim_start().to_string();
            }
        }
        if s == before {
            break;
        }
    }
    (s.trim_end_matches([')', '}']).trim().to_string(), guarded)
}

fn is_test(seg: &str) -> bool {
    seg.starts_with('[') || seg.starts_with("test ") || seg.starts_with("command -v") || seg.starts_with("grep ")
}

/// How the shell treats failures in the script being read (ADR 0060 §2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opts {
    /// `set -o pipefail` is on from the start (`shell: bash` runs `bash -eo pipefail`, `run-commands.sh` sets it).
    pub pipefail: bool,
    /// Every line is checked on its own (`run-commands.sh` runs each in `( set -e; eval line )` and tests the status), so a
    /// failure left of `&&` still fails the step; in a plain `bash -e` script it does not, unless it is the last line.
    pub lines_checked: bool,
}

impl Opts {
    /// A command list of `paddock.ci.json` (run-commands.sh) and anything else that is checked line by line.
    pub const CHECKED: Opts = Opts {
        pipefail: true,
        lines_checked: true,
    };
}

/// Whether the handler after `||` ends in a failure (`exit 1`, a bare `exit`, `false`, `return`): then the failure is not swallowed.
fn handler_fails(handler: &str) -> bool {
    let words: Vec<&str> = handler
        .split(|c: char| c.is_whitespace() || c == ';' || c == '}' || c == '{')
        .filter(|w| !w.is_empty())
        .collect();
    words.iter().enumerate().any(|(i, w)| match *w {
        "exit" => words.get(i + 1).is_none_or(|n| *n != "0"),
        "false" | "return" => true,
        _ => false,
    })
}

/// Reads a script as a command list (see [`Opts::CHECKED`]).
pub fn commands(script: &str, start: Option<String>) -> Vec<Cmd> {
    commands_with(script, start, Opts::CHECKED)
}

/// Reads a script (a `run:` block or a command list) into simple commands.
///
/// `start` is the folder the script starts in. A `cd` holds for the commands after it, as in `run-commands.sh` and in a
/// `run:` block; a `cd` inside `( … )` does not.
pub fn commands_with(script: &str, start: Option<String>, opts: Opts) -> Vec<Cmd> {
    let mut pipefail = opts.pipefail;
    let mut cwd = start;
    let mut out = Vec::new();
    let mut set_plus_e = false;
    let mut blocks: i32 = 0;
    let mut in_func = false;
    let mut physical: Vec<(usize, String)> = Vec::new();
    // Join backslash continuations.
    let mut pending: Option<(usize, String)> = None;
    for (i, raw) in script.lines().enumerate() {
        let (idx, mut text) = pending.take().unwrap_or((i, String::new()));
        let t = if text.is_empty() { raw.trim_end() } else { raw.trim() };
        if let Some(stripped) = t.strip_suffix('\\') {
            text.push_str(stripped.trim_end());
            text.push(' ');
            pending = Some((idx, text));
        } else {
            text.push_str(t);
            physical.push((idx, text));
        }
    }
    if let Some(p) = pending {
        physical.push(p);
    }
    let last_idx = physical
        .iter()
        .rev()
        .find(|(_, l)| !l.trim().is_empty() && !l.trim().starts_with('#'))
        .map_or(0, |(i, _)| *i);
    for (idx, line) in physical {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.ends_with("() {") || trimmed.ends_with("(){") {
            in_func = true;
            continue;
        }
        if in_func {
            if trimmed == "}" {
                in_func = false;
            }
            continue;
        }
        let opens = ["if ", "case "].iter().map(|k| trimmed.matches(k).count()).sum::<usize>() as i32;
        let closes = trimmed
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| *w == "fi" || *w == "esac")
            .count() as i32;
        let guarded_line = blocks > 0 || opens > 0;
        blocks = (blocks + opens - closes).max(0);
        let subshell = trimmed.starts_with('(') && trimmed.ends_with(')');
        let saved = cwd.clone();
        let segs = split_simple(if subshell { &trimmed[1..trimmed.len() - 1] } else { trimmed });
        // A trailing `|| true`/`; true` makes every earlier command of the line report-only.
        // Any `|| handler` that does not end in a failure swallows what is left of it (`|| true`, `|| echo …`), and so does `; true`.
        let tail_true = segs.iter().enumerate().any(|(i, (sep, t))| match sep {
            Sep::Or => !handler_fails(&segs[i..].iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>().join(" ; ")),
            Sep::Semi => t == "true" || t == ":" || t == "exit 0",
            _ => false,
        });
        let is_last_line = idx == last_idx;
        let mut test_before = false;
        for (i, (sep, seg)) in segs.iter().enumerate() {
            let next = segs.get(i + 1).map(|(s, _)| *s);
            let (text, kw_guard) = strip_keywords(seg);
            if text.is_empty() || text == "true" || text == ":" || text == "fi" || text == "done" || text == "esac" {
                continue;
            }
            if let Some(dir) = text.strip_prefix("cd ").or(if text == "cd" { Some("") } else { None }) {
                cwd = cwd.as_deref().and_then(|c| join(c, dir.trim()));
                continue;
            }
            if text == "set +e" {
                set_plus_e = true;
                continue;
            }
            if text.starts_with("set ") && text.contains("pipefail") {
                pipefail = !text.starts_with("set +o");
            }
            if text.starts_with("set ") && text.contains("-e") && !text.contains("+e") {
                set_plus_e = false;
                continue;
            }
            if is_test(&text) || kw_guard {
                test_before = true;
                if is_test(&text) {
                    continue;
                }
            }
            let conditional = guarded_line || (test_before && *sep == Sep::And) || (*sep == Sep::Or && !tail_true);
            out.push(Cmd {
                text,
                line: idx,
                cwd: cwd.clone(),
                report_only: set_plus_e
                    || tail_true
                    // left of a pipe without pipefail: only the last command's status counts
                    || (next == Some(Sep::Pipe) && !pipefail)
                    // left of `&&` in a plain `bash -e` script: `set -e` ignores it unless the list is the last command
                    || (next == Some(Sep::And) && !opts.lines_checked && !is_last_line),
                conditional,
            });
        }
        if subshell {
            cwd = saved;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(c: &[Cmd]) -> Vec<(&str, Option<&str>, bool, bool)> {
        c.iter()
            .map(|c| (c.text.as_str(), c.cwd.as_deref(), c.report_only, c.conditional))
            .collect()
    }

    #[test]
    fn a_cd_holds_for_the_lines_after_it_good_and_bad() {
        // apps/dyno/paddock.ci.json `checks`
        let c = commands(
            "cd apps/dyno\nnode --test scripts/a.test.mjs\ncd ..\nnode b.mjs",
            Some(String::new()),
        );
        assert_eq!(
            texts(&c),
            vec![
                ("node --test scripts/a.test.mjs", Some("apps/dyno"), false, false),
                ("node b.mjs", Some("apps"), false, false)
            ]
        );
        // a cd the model cannot resolve makes the folder unknown, not wrong
        let c = commands("cd \"$X\"\nnode b.mjs", Some(String::new()));
        assert_eq!(c[0].cwd, None);
    }

    #[test]
    fn a_cd_inside_a_subshell_does_not_hold() {
        let c = commands("(cd x && make)\nmake", Some(String::new()));
        assert_eq!(texts(&c), vec![("make", Some("x"), false, false), ("make", Some(""), false, false)]);
    }

    #[test]
    fn report_only_shapes() {
        let c = commands(
            "node a.mjs || true\nnode b.mjs || :\nnode c.mjs; true\nset +e\nnode d.mjs\nset -e\nnode e.mjs",
            Some(String::new()),
        );
        let flags: Vec<_> = c.iter().map(|c| (c.text.as_str(), c.report_only)).collect();
        assert_eq!(
            flags,
            vec![
                ("node a.mjs", true),
                ("node b.mjs", true),
                ("node c.mjs", true),
                ("node d.mjs", true),
                ("node e.mjs", false)
            ]
        );
    }

    fn flags(script: &str, o: Opts) -> Vec<(String, bool)> {
        commands_with(script, Some(String::new()), o)
            .into_iter()
            .map(|c| (c.text, c.report_only))
            .collect()
    }

    #[test]
    fn a_handler_after_or_swallows_the_failure_unless_it_ends_in_one() {
        let o = Opts::CHECKED;
        // bad (swallowed): echo, a brace group without exit, `|| true`
        assert!(flags("node a.mjs || echo skipped", o)[0].1);
        assert!(flags("node a.mjs || { echo skipped; }", o)[0].1);
        // good (fails the step): the handler exits non-zero, or a bare exit, or `false`
        assert!(!flags("node a.mjs || { echo broken; exit 1; }", o)[0].1);
        assert!(!flags("node a.mjs || exit", o)[0].1);
        assert!(!flags("node a.mjs || false", o)[0].1);
    }

    #[test]
    fn a_failure_left_of_and_is_ignored_by_bash_e_unless_the_list_is_last() {
        let plain = Opts {
            pipefail: true,
            lines_checked: false,
        };
        // bad: not the last line
        let f = flags("node a.mjs && echo ok\nnode b.mjs", plain);
        assert_eq!(f[0], ("node a.mjs".to_string(), true));
        // good: the last line, and run-commands.sh lines (each is checked)
        assert!(!flags("node b.mjs\nnode a.mjs && echo ok", plain)[1].1);
        assert!(!flags("node a.mjs && echo ok\nnode b.mjs", Opts::CHECKED)[0].1);
    }

    #[test]
    fn the_left_of_a_pipe_counts_only_with_pipefail() {
        let no = Opts {
            pipefail: false,
            lines_checked: true,
        };
        assert!(flags("node a.mjs | tee log", no)[0].1);
        assert!(!flags("node a.mjs | tee log", Opts::CHECKED)[0].1);
        // set -o pipefail inside the script turns it on from there
        let f = flags("node a.mjs | tee x\nset -eo pipefail\nnode b.mjs | tee y", no);
        assert_eq!((f[0].1, f[1].1), (true, false));
    }

    #[test]
    fn commands_behind_a_test_or_inside_an_if_are_conditional() {
        let c = commands(
            "[ -f x ] && node a.mjs\nif [ \"$OS\" = Linux ]; then step cargo fmt --all --check; fi\nnode plain.mjs",
            Some(String::new()),
        );
        let flags: Vec<_> = c.iter().map(|c| (c.text.as_str(), c.conditional)).collect();
        assert_eq!(
            flags,
            vec![
                ("node a.mjs", true),
                ("step cargo fmt --all --check", true),
                ("node plain.mjs", false)
            ]
        );
    }

    #[test]
    fn continuations_and_function_bodies() {
        let c = commands("step() {\n  \"$@\"\n}\nnode a.mjs \\\n  --flag\n", Some(String::new()));
        assert_eq!(texts(&c), vec![("node a.mjs --flag", Some(""), false, false)]);
    }

    #[test]
    fn variables_and_workspace_paths() {
        let mut env = BTreeMap::new();
        env.insert("APP".into(), "dyno".into());
        env.insert("MULTI".into(), "a\nb".into());
        assert_eq!(expand_vars("x $APP ${APP} $MULTI $OTHER", &env), "x dyno dyno $MULTI $OTHER");
        assert_eq!(
            strip_workspace("bash \"<ws>/.gitea/actions/app-gates/../../../conformance/ci/run-commands.sh\""),
            "bash \"conformance/ci/run-commands.sh\""
        );
        assert_eq!(join("apps/dyno", "../x"), Some("apps/x".into()));
        assert_eq!(join("", "/abs"), None);
    }
}
