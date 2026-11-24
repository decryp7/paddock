//! C131: a golden file holds nothing that belongs to the machine that captured it (ER-21 class "a golden captured on one machine fails
//! on another": WP-2 of train 14 had six, among them the Mac's uid 501 baked into a guardian golden). The fix is in the test: normalise
//! the value in the test's `norm()` so the golden says `<uid>` and every machine writes the same.
//!
//! Read: the text files under the golden folders (a `golden` or `goldens` folder in the path, or `*.golden` and `*.snap` files); the
//! host walks the working folder today (git's tracked list is the approved train-14 item). Flagged, one finding per line and kind, by
//! fixed patterns only, so the result is the same on every machine: ids (`uid`, `gid`, `euid`, `egid`, `user_id`, `group_id` followed
//! by a number), home paths (`/Users/<name>`, `/home/<name>`, `C:\Users\<name>`) and temp paths (`/tmp/<x>`, `/private/tmp/<x>`,
//! `/var/folders/`, `\Temp\`). Allowed: per-site entries of `conformance/c131-allow.json`, read by the shared [`super::allow_list`].
//! **Errors, blocking (mode gate) since its first green run on the real tree, ci.yml run 8869 (cdd4c3d10, 2026-11-24; ER-41); the shared allow-list reader's merge-base growth guard is an error for it (`Spec::growth_blocks`).**
//!
//! The JavaScript check (`conformance/src/checks/C131-*.mjs`) was the original; it was retired with pair P3 (ADR 0022 amendment 2026-11-19) and the Rust host is the only one. The answers it gave are recorded in `tests/fixtures/check/recorded` and `units` and ``tests/it/check_c131.rs`` holds the check to them. Patterns never use a look-around (the `regex` crate has none): the text around a site is matched as a character.

use super::allow_list::{self, Admit};
use super::finding::Finding;
use super::jsre::rx;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use regex::Regex;
use std::sync::LazyLock;

/// The allow file both hosts read.
pub const ALLOW: &str = "conformance/c131-allow.json";
/// The check's own files: they name the patterns.
pub const IMPLEMENTATION: [&str; 3] = [
    "tools/paddock-dev/src/check/c131.rs",
    "tools/paddock-dev/tests/it/check_c131.rs",
    ALLOW,
];

static GOLDEN_PATH: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|/)goldens?/|\.(?:golden|snap)$"));
static BINARY: LazyLock<Regex> = LazyLock::new(|| {
    rx(
        r"(?i)\.(?:png|jpe?g|gif|ico|icns|webp|bmp|tiff?|pdf|zip|gz|tgz|xz|zst|7z|whl|wasm|dmg|pkg|msi|exe|dll|so|dylib|a|lib|rlib|o|obj|pdb|ttf|otf|woff2?|mp3|mp4|mov|wav|webm|parquet|duckdb|sqlite|db|evtx|dmp|bin|onnx|safetensors)$",
    )
});
const BROKEN_FIX: &str =
    "Restore conformance/c131-allow.json: {\"allow\": [{\"path\": \"...\", \"site\": \"...\", \"reason\": \"...\", \"count\": 1}]}.";
const FIX: &str = "Normalise it in the test's norm() (replace the value with a placeholder before the comparison) and regenerate the golden; a value that is fine here (invented data) gets an entry in conformance/c131-allow.json with a reason.";

/// The fixed line patterns: `(kind, pattern)`, group 1 (or 2) the site.
static PATTERNS: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    vec![
        (
            "id",
            rx(r"(?i)(?:^|[^A-Za-z0-9_])((?:e?uid|e?gid|user_id|group_id)[^A-Za-z0-9_\n][^A-Za-z0-9\n]{0,5}[0-9]+)"),
        ),
        (
            "home",
            rx(r"(?:^|[^A-Za-z0-9_.-])((?:/Users/|/home/|[A-Za-z]:\\{1,2}Users\\{1,2})[A-Za-z0-9][A-Za-z0-9._-]*)"),
        ),
        (
            "temp",
            rx(r"(?:^|[^A-Za-z0-9_.-])((?:/private)?/(?:tmp/[A-Za-z0-9._-]|var/folders/))|(\\Temp\\)"),
        ),
    ]
});

/// One site: its 1-based line, kind, the flagged text and the whole line.
#[derive(Debug, PartialEq, Eq)]
pub struct Site {
    /// The 1-based line.
    pub line: usize,
    /// `id`, `home`, `temp`, `user`, `host`, `ram`, `cpus` or `home-of-this-machine`.
    pub kind: &'static str,
    /// The flagged text.
    pub site: String,
    /// The whole line, for the allow entries.
    pub text: String,
}

/// Every machine-specific literal in `text`, one per line and kind.
pub fn golden_sites(text: &str) -> Vec<Site> {
    let mut out = Vec::new();
    for (i, raw) in text.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        for (kind, re) in PATTERNS.iter() {
            if let Some(m) = re.captures(line) {
                let site = m.get(1).or_else(|| m.get(2)).map_or("", |g| g.as_str());
                out.push(Site {
                    line: i + 1,
                    kind,
                    site: site.to_string(),
                    text: line.to_string(),
                });
            }
        }
    }
    out
}

/// Whether `path` is a file the check reads.
pub fn is_golden(path: &str) -> bool {
    GOLDEN_PATH.is_match(path) && !BINARY.is_match(path) && !IMPLEMENTATION.contains(&path)
}

static SPEC: allow_list::Spec = allow_list::Spec {
    id: "C131",
    rule: 40,
    file: ALLOW,
    unseen: "a machine-specific value",
    restore: BROKEN_FIX,
    growth_blocks: true,
};

/// The check.
pub struct C131;

impl Check for C131 {
    fn id(&self) -> &'static str {
        "C131"
    }

    fn rule(&self) -> u32 {
        40
    }

    fn title(&self) -> &'static str {
        "A golden file holds no value of the machine that captured it"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::Gate
    }

    fn allow_list(&self) -> Option<&'static allow_list::Spec> {
        Some(&SPEC)
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    ALLOW,
                    "{\"allow\":[{\"path\":\"x/tests/golden/ok.json\",\"site\":\"/Users/ana\",\"reason\":\"invented test user\"}]}",
                ),
                ("x/tests/golden/guardian.json", "{\n  \"uid\": 501\n}\n"),
                ("x/tests/golden/ok.json", "{\"home\": \"/Users/ana/x\"}\n"),
                ("x/tests/golden/norm.json", "{\"uid\": \"<uid>\"}\n"),
                ("x/tests/fixtures/other.json", "{\"uid\": 501}\n"),
            ],
            expect: vec![("x/tests/golden/guardian.json", 2)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let (mut allow, ()) = match allow_list::read(ctx, &SPEC, allow_list::entries_only(ALLOW)) {
            Ok(read) => read,
            Err(broken) => return *broken,
        };
        let mut findings = Vec::new();
        for f in ctx.files().iter() {
            if !is_golden(f) {
                continue;
            }
            let text = match allow.read(ctx, f) {
                Ok(text) => text,
                Err(unread) => {
                    findings.push(*unread);
                    continue;
                }
            };
            if text.contains('\0') {
                allow.skip(f);
                continue;
            }
            for s in golden_sites(&text) {
                let (kind, site) = (s.kind, &s.site);
                let finding = match allow.admit(f, |entry| s.text.contains(entry)) {
                    Admit::Allowed => continue,
                    Admit::Beyond => Finding::error(
                        "C131",
                        40,
                        f,
                        s.line,
                        format!(
                            "golden holds a machine-specific {kind} `{site}` beyond the count of its allow entry: it fails on a machine that differs"
                        ),
                        FIX,
                    ),
                    Admit::Unlisted => Finding::error(
                        "C131",
                        40,
                        f,
                        s.line,
                        format!("golden holds a machine-specific {kind} `{site}`: it fails on a machine that differs"),
                        FIX,
                    ),
                };
                findings.push(finding);
            }
        }
        allow.output(ctx, findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<String> {
        golden_sites(text).iter().map(|s| format!("{}:{}", s.line, s.kind)).collect()
    }

    #[test]
    fn the_ids_and_paths_are_found() {
        assert_eq!(kinds("{\n  \"uid\": 501,\n  \"gid\": 20\n}\n"), ["2:id", "3:id"]);
        assert_eq!(kinds("cwd=/Users/someone/work\nx=/home/runner/x\n"), ["1:home", "2:home"]);
        assert_eq!(kinds("/var/folders/zz/abc/T/x\n/tmp/work-1\n"), ["1:temp", "2:temp"]);
        assert_eq!(kinds("é😀 uid=1\r\nclean\r\n/home/bï\r\n"), ["1:id", "3:home"]);
    }

    #[test]
    fn placeholders_near_misses_and_this_machine_are_not() {
        assert!(kinds("/Users/<name>/x /home/$USER and uid=<uid> a guid 501, the uid_max setting").is_empty());
        assert!(kinds("user mia on host m9-mac-mini with 17179869184 bytes and cpus: 10").is_empty());
    }

    #[test]
    fn golden_folders_and_files_only() {
        for f in ["tools/x/tests/golden/a.json", "golden/a.txt", "crates/c/tests/goldens/b.plist", "a/b.golden", "a/b.snap"] {
            assert!(is_golden(f), "{f}");
        }
        for f in ["tools/x/tests/golden/a.png", "src/golden.rs", "docs/goldenrod/a.md", ALLOW] {
            assert!(!is_golden(f), "{f}");
        }
    }
}
