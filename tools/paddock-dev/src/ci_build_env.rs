//! `dev ci build-env`: `PADDOCK_BUILD_COMMIT` and `PADDOCK_BUILD_DATE` for the steps after it, so the About page can show them
//! (`option_env!`), the port of `conformance/ci/build-env.mjs` (docs/plans/ci-in-rust.md, WP-3). The commit is the first 12 hex digits
//! of `GITHUB_SHA` (else `git rev-parse HEAD`), lower-cased, or `unknown`; the date is today's UTC date. The two lines go to
//! `$GITHUB_ENV` (appended) and to standard output; it never fails the job.
//!
//! `tests/it/build_env.rs` runs the script and this command on the same inputs. The script stays for tag, release and rule 54 runs
//! (`conformance/ci/kit.sh build-env`) and for conformance check C56, which reads the names from it, until the conformance engine
//! and the release path are ported (WP-10, WP-11).

use crate::ci_tool::ToolCall;
use clap::Args;
use std::collections::HashMap;
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `dev ci build-env`.
#[derive(Args, Debug)]
pub struct BuildEnvArgs {}

/// A SHA as git and GitHub print it: 7 to 64 hex digits.
fn is_sha(s: &str) -> bool {
    (7..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// `(PADDOCK_BUILD_COMMIT, PADDOCK_BUILD_DATE)` from the environment; `head` and `now` are injectable.
pub fn build_env(env: &HashMap<String, String>, head: &dyn Fn() -> Option<String>, now: SystemTime) -> [(&'static str, String); 2] {
    let from_env = env.get("GITHUB_SHA").filter(|s| is_sha(s)).cloned();
    let sha = from_env.or_else(head);
    let commit = match sha {
        Some(s) if is_sha(&s) => s.to_ascii_lowercase().chars().take(12).collect(),
        _ => "unknown".to_string(),
    };
    [("PADDOCK_BUILD_COMMIT", commit), ("PADDOCK_BUILD_DATE", utc_date(now))]
}

/// `YYYY-MM-DD` of `t` in UTC (days since the epoch to a civil date, Howard Hinnant's algorithm).
pub fn utc_date(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn git_head(env: &HashMap<String, String>) -> Option<String> {
    let mut sink = std::io::sink();
    let ran = ToolCall::new("git")
        .args(["rev-parse", "HEAD"])
        .timeout(Duration::from_secs(5))
        .run(env, &mut sink)
        .ok()?;
    ran.success.then(|| ran.stdout.trim().to_string())
}

/// `dev ci build-env`: the two assignments to `$GITHUB_ENV` and standard output. Exit 0 always (the script's contract).
pub fn run(_args: &BuildEnvArgs, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let env: HashMap<String, String> = std::env::vars().collect();
    let pairs = build_env(&env, &|| git_head(&env), SystemTime::now());
    let text: String = pairs.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    if let Some(file) = env.get("GITHUB_ENV").filter(|f| !f.is_empty()) {
        let appended = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(file)
            .and_then(|mut f| f.write_all(text.as_bytes()));
        if let Err(e) = appended {
            let _ = writeln!(
                err,
                "::warning::build-env: could not append to $GITHUB_ENV ({e}); the About page will show no build commit. Next step: check the runner's GITHUB_ENV file."
            );
        }
    }
    let _ = write!(out, "{text}");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(date_secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(date_secs)
    }

    #[test]
    fn the_utc_date_is_civil() {
        assert_eq!(utc_date(at(0)), "1970-01-01");
        assert_eq!(utc_date(at(1_792_886_399)), "2026-10-24");
        assert_eq!(utc_date(at(1_709_164_800)), "2024-02-29", "a leap day");
        assert_eq!(utc_date(at(951_782_400)), "2000-02-29");
        assert_eq!(utc_date(at(4_102_444_799)), "2099-12-31");
    }

    #[test]
    fn the_commit_comes_from_the_sha_then_git_then_unknown() {
        let now = at(1_792_886_399);
        let env = |sha: &str| HashMap::from([("GITHUB_SHA".to_string(), sha.to_string())]);
        let none: &dyn Fn() -> Option<String> = &|| None;
        let [(_, c), (_, d)] = build_env(&env("ABCDEF0123456789abcdef0123456789abcdef01"), none, now);
        assert_eq!((c.as_str(), d.as_str()), ("abcdef012345", "2026-10-24"));
        assert_eq!(
            build_env(&HashMap::new(), &|| Some("1234567890abcdef".into()), now)[0].1,
            "1234567890ab"
        );
        assert_eq!(build_env(&env("not a sha"), none, now)[0].1, "unknown");
        assert_eq!(build_env(&HashMap::new(), &|| Some("garbage!".into()), now)[0].1, "unknown");
    }
}
