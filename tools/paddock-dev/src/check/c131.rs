//! C131: a golden file holds nothing that belongs to the machine that captured it (ER-21 class "a golden captured on one machine fails
//! on another": WP-2 of train 14 had six, among them the Mac's uid 501 baked into a guardian golden). The fix is in the test: normalise
//! the value in the test's `norm()` so the golden says `<uid>` and every machine writes the same.
//!
//! Read: the text files of the golden folders (a `golden` or `goldens` folder in the path, or a file named `*.golden`). Flagged, one
//! finding per line and kind: ids (`uid`, `gid`, `euid`, `egid`, `user_id`, `group_id` followed by a number), home paths
//! (`/Users/<name>`, `/home/<name>`, `C:\Users\<name>`), temp paths (`/tmp/<x>`, `/private/tmp/<x>`, `/var/folders/`, `\Temp\`) and
//! the machine that runs the check, read from the environment and never written down: user name and home folder, host name, RAM in
//! bytes, KiB or MiB, and the CPU count next to `cpu`, `core` or `thread`. `PADDOCK_GOLDEN_FACTS` (JSON: user, home, host, memBytes,
//! cpus) replaces them (tests; judging a tree against another machine). Allowed: per-site entries of `conformance/c131-allow.json`, read
//! by the shared [`super::allow_list`]. **Report-only (warnings) until its first green run on the real tree (ER-41).**
//!
//! The JavaScript check `conformance/src/checks/C131-golden-machine.mjs` is the original; `tests/it/check_c131.rs` compares the two
//! finding for finding. Patterns never use a look-around (the `regex` crate has none): the text around a site is matched as a character.

use super::allow_list::{self, Admit};
use super::finding::Finding;
use super::jsre::rx;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use crate::ci_tool::ToolCall;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

/// The allow file both hosts read.
pub const ALLOW: &str = "conformance/c131-allow.json";
/// The check's own files: they name the patterns.
pub const IMPLEMENTATION: [&str; 5] = [
    "conformance/src/checks/C131-golden-machine.mjs",
    "conformance/test/c131-golden-machine.test.mjs",
    "tools/paddock-dev/src/check/c131.rs",
    "tools/paddock-dev/tests/it/check_c131.rs",
    ALLOW,
];
/// The longest the host-name and memory probes may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

static GOLDEN_PATH: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|/)goldens?/|\.golden$"));
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

/// What a machine is, as the check sees it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    /// The account name.
    pub user: String,
    /// The home folder.
    pub home: String,
    /// The host name.
    pub host: String,
    /// The RAM in bytes (0 when unknown).
    pub mem_bytes: f64,
    /// The CPU count (0 when unknown).
    pub cpus: f64,
}

/// `PADDOCK_GOLDEN_FACTS` when set, else this machine from the environment and, for the host name and the RAM, one bounded probe each.
pub fn machine_facts(env: &HashMap<String, String>) -> Facts {
    if let Some(json) = env.get("PADDOCK_GOLDEN_FACTS").filter(|v| !v.is_empty()) {
        let v: Value = serde_json::from_str(json).unwrap_or(Value::Null);
        let text = |k: &str| v[k].as_str().unwrap_or("").to_string();
        return Facts {
            user: text("user"),
            home: text("home"),
            host: text("host"),
            mem_bytes: v["memBytes"].as_f64().unwrap_or(0.0),
            cpus: v["cpus"].as_f64().unwrap_or(0.0),
        };
    }
    let first = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| env.get(*k).filter(|v| !v.is_empty()).cloned())
            .unwrap_or_default()
    };
    let probe = |program: &str, args: &[&str]| {
        let ran = ToolCall::new(program)
            .args(args.iter().copied())
            .timeout(PROBE_TIMEOUT)
            .run(env, &mut std::io::sink())
            .ok()?;
        ran.success.then(|| ran.stdout.trim().to_string())
    };
    let mem_bytes = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|info| {
            let kb = info
                .lines()
                .find_map(|l| l.strip_prefix("MemTotal:"))?
                .split_whitespace()
                .next()?
                .parse::<f64>()
                .ok()?;
            Some(kb * 1024.0)
        })
        .or_else(|| probe("sysctl", &["-n", "hw.memsize"])?.parse::<f64>().ok())
        .unwrap_or(0.0);
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .or_else(|| probe("hostname", &[]))
        .unwrap_or_else(|| first(&["COMPUTERNAME", "HOSTNAME"]));
    Facts {
        user: first(&["USER", "USERNAME", "LOGNAME"]),
        home: first(&["HOME", "USERPROFILE"]),
        host,
        mem_bytes,
        cpus: std::thread::available_parallelism().map_or(0.0, |n| n.get() as f64),
    }
}

fn word(name: &str) -> Regex {
    rx(&format!("(?i)(?:^|[^A-Za-z0-9_.-])({})(?:$|[^A-Za-z0-9_-])", regex::escape(name)))
}

/// The patterns of the machine `facts` describe: `(kind, pattern)`. A fact too short or too common to tell from other text makes none.
pub fn machine_patterns(facts: &Facts) -> Vec<(&'static str, Regex)> {
    let mut out = Vec::new();
    if facts.user.chars().count() >= 3 && !facts.user.eq_ignore_ascii_case("root") {
        out.push(("user", word(&facts.user)));
    }
    if facts.home.chars().count() > 3 && facts.home != "/" && facts.home != "/root" {
        out.push((
            "home-of-this-machine",
            rx(&format!("({})(?:$|[^A-Za-z0-9_])", regex::escape(&facts.home))),
        ));
    }
    let host = {
        let h = facts.host.as_str();
        if h.len() > 6 && h[h.len() - 6..].eq_ignore_ascii_case(".local") {
            &h[..h.len() - 6]
        } else {
            h
        }
    };
    let mut names = vec![host];
    if let Some(short) = host.split('.').next()
        && short != host
    {
        names.push(short);
    }
    for h in names {
        if h.chars().count() >= 4 && !h.eq_ignore_ascii_case("localhost") {
            out.push(("host", word(h)));
        }
    }
    let mem = facts.mem_bytes;
    if mem.is_finite() && mem >= 268_435_456.0 {
        let mut seen: Vec<String> = Vec::new();
        for n in [mem, (mem / 1024.0).floor(), (mem / 1_048_576.0).floor()] {
            let n = format!("{n:.0}");
            if !seen.contains(&n) {
                out.push(("ram", rx(&format!("(?:^|[^0-9.])({n})(?:$|[^0-9])"))));
                seen.push(n);
            }
        }
    }
    let cpus = facts.cpus;
    if cpus.fract() == 0.0 && cpus >= 2.0 {
        out.push((
            "cpus",
            rx(&format!(
                "(?i)(?:^|[^A-Za-z0-9_])((?:cpus?|cores?|threads?|nproc)[^A-Za-z0-9\\n]{{0,4}}{cpus:.0})(?:$|[^0-9.])"
            )),
        ));
    }
    out
}

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
pub fn golden_sites(text: &str, facts: &Facts) -> Vec<Site> {
    let machine = machine_patterns(facts);
    let mut out = Vec::new();
    for (i, raw) in text.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        for (kind, re) in PATTERNS.iter().chain(machine.iter()) {
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
        Mode::ReportOnly
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
        let env: HashMap<String, String> = std::env::vars().collect();
        let facts = machine_facts(&env);
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
            for s in golden_sites(&text, &facts) {
                let (kind, site) = (s.kind, &s.site);
                let finding = match allow.admit(f, |entry| s.text.contains(entry)) {
                    Admit::Allowed => continue,
                    Admit::Beyond => Finding::warn(
                        "C131",
                        40,
                        f,
                        s.line,
                        format!(
                            "golden holds a machine-specific {kind} `{site}` beyond the count of its allow entry: it fails on a machine that differs"
                        ),
                        FIX,
                    ),
                    Admit::Unlisted => Finding::warn(
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

    fn facts() -> Facts {
        Facts {
            user: "mia".into(),
            home: "/Users/mia".into(),
            host: "m9-mac-mini.local".into(),
            mem_bytes: 17_179_869_184.0,
            cpus: 10.0,
        }
    }

    fn kinds(text: &str) -> Vec<String> {
        golden_sites(text, &facts())
            .iter()
            .map(|s| format!("{}:{}", s.line, s.kind))
            .collect()
    }

    #[test]
    fn the_ids_paths_and_machine_values_are_found() {
        assert_eq!(kinds("{\n  \"uid\": 501,\n  \"gid\": 20\n}\n"), ["2:id", "3:id"]);
        assert_eq!(kinds("cwd=/Users/someone/work\nx=/home/runner/x\n"), ["1:home", "2:home"]);
        assert_eq!(kinds("/var/folders/zz/abc/T/x\n/tmp/work-1\n"), ["1:temp", "2:temp"]);
        assert_eq!(
            kinds("ram 17179869184\nram_kb 16777216\n\"cpus\": 10\n"),
            ["1:ram", "2:ram", "3:cpus"]
        );
        assert_eq!(kinds("host m9-mac-mini.local\nbuilt by mia"), ["1:host", "2:user"]);
    }

    #[test]
    fn placeholders_near_misses_and_common_facts_are_not() {
        assert!(kinds("/Users/<name>/x /home/$USER and uid=<uid> a guid 501, the uid_max setting").is_empty());
        assert!(kinds("mia2 miami amia and 171798691841 and cpus 100").is_empty());
        let none = Facts {
            user: "root".into(),
            home: "/root".into(),
            host: "localhost".into(),
            mem_bytes: 1.0,
            cpus: 1.0,
        };
        assert!(machine_patterns(&none).is_empty());
    }

    #[test]
    fn the_facts_come_from_the_variable_or_the_environment() {
        let env: HashMap<String, String> = [(
            "PADDOCK_GOLDEN_FACTS".to_string(),
            r#"{"user":"mia","home":"/Users/mia","host":"m9-mac-mini.local","memBytes":17179869184,"cpus":10}"#.to_string(),
        )]
        .into();
        assert_eq!(machine_facts(&env), facts());
        let own: HashMap<String, String> = [("USER".to_string(), "zed".to_string()), ("HOME".to_string(), "/h/zed".to_string())].into();
        assert_eq!(machine_facts(&own).user, "zed");
    }

    #[test]
    fn golden_folders_and_files_only() {
        for f in [
            "tools/x/tests/golden/a.json",
            "golden/a.txt",
            "crates/c/tests/goldens/b.plist",
            "a/b.golden",
        ] {
            assert!(is_golden(f), "{f}");
        }
        for f in ["tools/x/tests/golden/a.png", "src/golden.rs", "docs/goldenrod/a.md", ALLOW] {
            assert!(!is_golden(f), "{f}");
        }
    }
}
