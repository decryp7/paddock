//! `port` (a slot's block of ports) and `pattern` (a process pattern scoped to the job's own folders).

use super::home::{ci_home, slot, slot_number};
use super::table::Kept;
use super::{Ctx, R, Sinks, Stop};
use regex::Regex;

/// `port <name>`: the first port of this slot's block for `<name>` in `ports.txt`.
pub fn cmd_port(ctx: &Ctx, args: &[String], io: &mut Sinks<'_>) -> R<()> {
    let name = args.first().ok_or_else(|| Stop::die("usage: dev ci isolation port <name>"))?;
    let block = ctx
        .ports()?
        .into_iter()
        .find(|b| &b.name == name)
        .ok_or_else(|| Stop::die(format!("no port block '{name}' in ports.txt")))?;
    let s = slot(ctx)?;
    io.value((block.base + u64::from(slot_number(&s) - 1) * block.stride).to_string());
    Ok(())
}

/// A path as an extended regular expression for `pkill -f`: every separator matches `/` and `\`, a Windows drive (`C:` or `/c`) is
/// left out, metacharacters are escaped.
pub fn path_regex(path: &str, windows: bool) -> String {
    let mut p = path;
    if windows {
        let b = p.as_bytes();
        let drive = b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
        let git_bash = b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b'/';
        if drive || git_bash {
            p = &p[2..];
        }
    }
    let mut out = String::new();
    for c in p.chars() {
        match c {
            '/' | '\\' => out.push_str("[/\\\\]"),
            ']' | '.' | '[' | '*' | '+' | '?' | '(' | ')' | '{' | '}' | '|' | '^' | '$' => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

fn name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
}

/// `{slot}` occurs once, ends the pattern, and comes right after a slot kept folder's name: the layout's text before `{name}` (after a
/// separator or at the start), a name, and a dash (for example `[/\\]test-browser-spotter-{slot}`).
pub fn slot_anchored(pat: &str, kept: &Kept) -> bool {
    let Some(before) = pat.strip_suffix("{slot}") else { return false };
    if before.contains("{slot}") {
        return false;
    }
    let Some(rest) = before.strip_suffix('-') else { return false };
    for l in kept.slot_layouts() {
        let stem = l.layout.split("{name}").next().unwrap_or(&l.layout).replace('/', "[/\\\\]");
        // `rest` = <anything><separator>? <stem><name>: the stem starts the pattern, or follows `/` or `]`.
        for (i, _) in rest.match_indices(&stem) {
            let name = &rest[i + stem.len()..];
            if name.is_empty() || !name.chars().all(name_char) {
                continue;
            }
            if i == 0 || rest[..i].ends_with('/') || rest[..i].ends_with(']') {
                return true;
            }
        }
    }
    false
}

/// Would the regex match the slot holder's command line as this job starts it (Unix and Windows spellings of its paths)?
fn holder_matches(ctx: &Ctx, re: &str) -> bool {
    let Ok(re) = Regex::new(re) else { return false };
    let temp = ctx
        .get("PADDOCK_JOB_TEMP")
        .or_else(|| ctx.get("RUNNER_TEMP"))
        .unwrap_or("/job-temp");
    let home = ci_home(ctx)
        .ok()
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "/ci-home".to_string());
    let back = |s: &str| s.replace('/', "\\");
    for t in [temp.to_string(), back(temp)] {
        for h in [home.clone(), back(&home)] {
            let line = format!(
                "{h}/tools/paddock-dev/bin/0/paddock-dev ci slot hold --max 4 --owner {t} --root {h} --poll-ms 500 --max-hold-secs 12600"
            );
            if re.is_match(&line) {
                return true;
            }
        }
    }
    false
}

/// `pattern <regex>`: a process pattern with `{tmp}`, `{build}` and `{slot}` filled in. Refused (exit 1, nothing on standard output)
/// when it names none of them, when `{slot}` is not anchored on a kept folder's name, or when it would also match this job's slot holder.
pub fn cmd_pattern(ctx: &Ctx, args: &[String], io: &mut Sinks<'_>) -> R<()> {
    let given = args.first().map(String::as_str).unwrap_or("");
    let windows = ctx.get("RUNNER_OS") == Some("Windows");
    let refuse = || Stop::silent(1);
    let mut pat = given.to_string();
    let mut scoped = false;
    if pat.contains("{tmp}") {
        let v = ctx.get("PADDOCK_JOB_TEMP").ok_or_else(refuse)?;
        pat = pat.replace("{tmp}", &path_regex(v, windows));
        scoped = true;
    }
    if pat.contains("{build}") {
        let v = ctx.get("PADDOCK_BUILD_FOLDER").ok_or_else(refuse)?;
        pat = pat.replace("{build}", &path_regex(v, windows));
        scoped = true;
    }
    if pat.contains("{slot}") {
        let s = slot(ctx).map_err(|_| refuse())?;
        if !slot_anchored(&pat, &ctx.kept()?) {
            return Err(refuse());
        }
        // A boundary after the slot: s1 must not match s12.
        pat = pat.replace("{slot}", &format!("{s}([^0-9]|$)"));
        scoped = true;
    }
    if !scoped {
        return Err(refuse());
    }
    // Never the slot's own holder: its command line names the job's temp folder (--owner) and the CI home (--root), so a pattern such
    // as "{tmp}" alone would end it and free the slot in the middle of the job.
    if holder_matches(ctx, &pat) {
        return Err(Stop::line(
            1,
            format!("job-isolation: the pattern '{given}' would also match this job's slot holder (`ci slot hold --owner <job temp>`)"),
        ));
    }
    io.value(pat);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_isolation::table::parse_kept;
    use crate::ci_isolation::testkit::{ctx, go};

    #[test]
    fn paths_become_separator_agnostic_regexes() {
        assert_eq!(path_regex("/ci/target-x-s1", false), "[/\\\\]ci[/\\\\]target-x-s1");
        assert_eq!(path_regex("C:/ci/a.b", true), "[/\\\\]ci[/\\\\]a\\.b");
        assert_eq!(path_regex("/c/ci", true), "[/\\\\]ci");
        assert_eq!(path_regex("a(b)+", false), "a\\(b\\)\\+");
    }

    #[test]
    fn the_slot_must_end_the_pattern_right_after_a_kept_folders_name() {
        let k = parse_kept("target target-{name}-{src}-{slot} slot\nbrowser test-browser-{name}-{slot} slot\nsrc src/{name}-{slot} slot\n")
            .unwrap();
        for good in [
            "[/\\\\]test-browser-spotter-{slot}",
            "test-browser-x-{slot}",
            "[/\\\\]src[/\\\\]paddock-crate-macOS-{slot}",
            "/test-browser-a.b-{slot}",
        ] {
            assert!(slot_anchored(good, &k), "{good}");
        }
        for bad in [
            "chrome-{slot}",
            "{tmp}/x-{slot}",
            "[/\\\\]test-browser-spotter-{slot}[/\\\\]",
            "[/\\\\]target-x-{slot}-{slot}",
            "xtarget-x-{slot}",
            "test-browser--{slot}x",
            "test-browser-{slot}",
        ] {
            assert!(!slot_anchored(bad, &k), "{bad}");
        }
    }

    #[test]
    fn a_pattern_that_would_match_the_holder_is_refused() {
        let c = ctx(&[
            ("PADDOCK_JOB_TEMP", "/w/abc/tmp"),
            ("HOME", "/nonexistent-home-for-test"),
            ("PADDOCK_CI_HOME", "/nonexistent"),
        ]);
        let (code, out, err) = go(&c, &["pattern", "{tmp}"]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(err.contains("would also match this job's slot holder"), "{err}");
        let (code, out, _) = go(&c, &["pattern", "{tmp}[/\\\\]dyno-it-plugins-[A-Za-z0-9]+"]);
        assert_eq!(code, 0);
        assert_eq!(out.trim(), "[/\\\\]w[/\\\\]abc[/\\\\]tmp[/\\\\]dyno-it-plugins-[A-Za-z0-9]+");
        assert_eq!(go(&c, &["pattern", "gitea-runner"]).0, 1, "names none of the job's folders");
    }
}
