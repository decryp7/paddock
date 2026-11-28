//! C175: a `PADDOCK_*` variable is read through `paddock-ci-config`'s settings, never by a string literal in the tools (ADR 0123
//! section 5, decision 82; rule 42, ER-22, ER-42). ER-21 class: the same variable was named, parsed and defaulted by hand at many sites
//! (the S3 endpoint at three, each with its own default and scheme handling: run 9135 added `http://` twice).
//!
//! Read: the production code of every `.rs` file under `tools/paddock-dev/src` (a file under a `tests` folder, `tests.rs`, `*_tests.rs`
//! or `testutil*.rs`, and the `#[cfg(test)]` items of any other file, are test code and not read), comments and the insides of
//! literals masked ([`super::lex::rust::mask`]). A finding is a **complete string literal** `"PADDOCK_..."` in that code: a name typed
//! outside the crate, a read (`env::var`, `.get`, `option_env!`) or a write (`.env(..)`) alike. A name the declaration table does not
//! know is said so: it is declared first. A read goes through `paddock_ci_config::settings` (`Settings::get`, `number`, `is_on`,
//! `secret` over the `paddock_env::Vars` the adapter holds, with the `declared::PADDOCK_*` constant), which also carries the default
//! and the secret flag.
//!
//! The starting list is the report itself (ER-46: lists only shrink, no baseline file: a baseline needs the user's approval): the
//! count at the first run is recorded in docs/gates.toml (`[[report_only]]` C175).
//!
//! **Report-only (warnings, mode report-only; ER-41) until its first green run on the real tree**, recorded here and in docs/gates.toml,
//! review by 2026-12-10. Rust host only: born in Rust.

use super::finding::Finding;
use super::jsre::rx;
use super::lex::rust::{mask, production_lines};
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use paddock_ci_config::settings::declaration;
use paddock_repo_graph::model::os::is_test_file;
use regex::Regex;
use std::sync::LazyLock;

/// The rule field: rule 42, share first.
const RULE: u32 = 42;

/// Where the tools that read the repository variables live.
const ROOT: &str = "tools/paddock-dev/src/";

static LITERAL: LazyLock<Regex> = LazyLock::new(|| rx(r#""(PADDOCK_[A-Z0-9_]*[A-Z0-9])""#));

/// One name typed as a literal: its 1-based line and the name.
#[derive(Debug, PartialEq, Eq)]
pub struct Site {
    /// The 1-based line.
    pub line: usize,
    /// The variable's name.
    pub name: String,
}

/// The names typed as literals in the production code of one file's text. `test_file`: the whole file is test code.
pub fn scan(text: &str, test_file: bool) -> Vec<Site> {
    if test_file {
        return Vec::new();
    }
    let masked = mask(text);
    let production = production_lines(text);
    let mut sites = Vec::new();
    for (i, ((raw, m), prod)) in text.split('\n').zip(masked.split('\n')).zip(&production).enumerate() {
        if prod.trim().is_empty() || !raw.contains("\"PADDOCK_") {
            continue;
        }
        let code: Vec<char> = m.chars().collect();
        for c in LITERAL.captures_iter(raw) {
            let at = raw[..c.get(0).map_or(0, |g| g.start())].chars().count();
            // A literal inside a comment is blanked by the mask: only a quote the mask kept is code.
            if code.get(at) == Some(&'"') {
                sites.push(Site {
                    line: i + 1,
                    name: c[1].to_string(),
                });
            }
        }
    }
    sites
}

/// The files read.
fn read(rel: &str) -> bool {
    rel.starts_with(ROOT) && rel.ends_with(".rs")
}

/// Whether the whole file is test code.
fn test_file(rel: &str) -> bool {
    is_test_file(rel) || rel.rsplit('/').next().is_some_and(|n| n.starts_with("testutil"))
}

/// The check.
pub struct C175;

impl Check for C175 {
    fn id(&self) -> &'static str {
        "C175"
    }

    fn rule(&self) -> u32 {
        RULE
    }

    fn title(&self) -> &'static str {
        "a PADDOCK_ variable is read through paddock-ci-config's settings, not by a string literal in the tools"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::ReportOnly
    }

    fn origin(&self) -> super::Origin {
        super::Origin::BornInRust
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                // Red: a direct read, and a name nobody declared.
                (
                    "tools/paddock-dev/src/direct.rs",
                    "pub fn home() -> Option<String> {\n    std::env::var(\"PADDOCK_CI_HOME\").ok()\n}\n",
                ),
                (
                    "tools/paddock-dev/src/undeclared.rs",
                    "pub fn x(env: &Env) -> Option<String> {\n    env.get(\"PADDOCK_NOT_DECLARED_ANYWHERE\")\n}\n",
                ),
                // Green: the same read through the settings, a comment, a test file, a cfg(test) item, another folder.
                (
                    "tools/paddock-dev/src/through_settings.rs",
                    "pub fn home(vars: &dyn Vars) -> Option<String> {\n    Settings::new(vars).get(PADDOCK_CI_HOME)\n}\n",
                ),
                (
                    "tools/paddock-dev/src/commented.rs",
                    "// reads \"PADDOCK_CI_HOME\" through the settings\npub fn f() {}\n",
                ),
                (
                    "tools/paddock-dev/src/tests.rs",
                    "fn t() {\n    let _ = std::env::var(\"PADDOCK_CI_HOME\");\n}\n",
                ),
                (
                    "tools/paddock-dev/src/with_tests.rs",
                    "pub fn f() {}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        let _ = std::env::var(\"PADDOCK_CI_HOME\");\n    }\n}\n",
                ),
                (
                    "crates/other/src/lib.rs",
                    "pub fn f() -> Option<String> {\n    std::env::var(\"PADDOCK_CI_HOME\").ok()\n}\n",
                ),
            ],
            expect: vec![("tools/paddock-dev/src/direct.rs", 2), ("tools/paddock-dev/src/undeclared.rs", 2)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut findings = Vec::new();
        let mut probes = 0;
        for f in ctx.files().iter().filter(|f| read(f)) {
            let Some(text) = ctx.read(f) else { continue };
            probes += 1;
            for s in scan(&text, test_file(f)) {
                let (message, fix) = if declaration(&s.name).is_some() {
                    (
                        format!("`{}` is named by a string literal: read it through the settings", s.name),
                        format!(
                            "Read it with `paddock_ci_config::settings::Settings` and `declared::{}` over the `paddock_env::Vars` the adapter holds: the declaration carries its parser, default and secret flag (ADR 0123 section 5).",
                            s.name
                        ),
                    )
                } else {
                    (
                        format!("`{}` is read or written here and is not declared", s.name),
                        "Declare it in crates/paddock-ci-config/src/settings/declared.rs (name, type, default, who sets it), then read it through the settings (ADR 0123 section 5).".to_string(),
                    )
                };
                findings.push(Finding::warn("C175", RULE, f, s.line, message, &fix));
            }
        }
        CheckOutput {
            findings,
            probes,
            completed: true,
            precondition: Precondition {
                met: probes > 0 || ctx.is_in_memory(),
                detail: format!("{probes} Rust files read under {ROOT}"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str, test_file: bool) -> Vec<(usize, String)> {
        scan(text, test_file).into_iter().map(|s| (s.line, s.name)).collect()
    }

    #[test]
    fn a_complete_literal_in_production_code_is_a_site() {
        let t = "fn a() {\n    let _ = std::env::var(\"PADDOCK_CI_HOME\");\n    c.env(\"PADDOCK_OUT\", v);\n}\n";
        assert_eq!(lines(t, false), vec![(2, "PADDOCK_CI_HOME".into()), (3, "PADDOCK_OUT".into())]);
    }

    #[test]
    fn two_names_on_one_line_are_two_sites() {
        let t = "const A: [&str; 2] = [\"PADDOCK_A\", \"PADDOCK_B\"];\n";
        assert_eq!(lines(t, false).len(), 2);
    }

    #[test]
    fn a_comment_a_message_a_prefix_and_a_non_ascii_prefix_are_no_site() {
        let t = "// \"PADDOCK_X\"\n/// reads \"PADDOCK_X\"\nfn a() {\n    let m = \"PADDOCK_X is not set\";\n    let p = \"PADDOCK_CI_\";\n    let é = \"→\"; let n = \"PADDOCK_Y\";\n}\n";
        assert_eq!(lines(t, false), vec![(6, "PADDOCK_Y".into())], "only the whole literal after a non-ASCII char is found at its place");
    }

    #[test]
    fn test_code_is_not_read() {
        let t = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() { let _ = \"PADDOCK_X\"; }\n}\n";
        assert!(lines(t, false).is_empty());
        assert!(lines("fn t() { let _ = \"PADDOCK_X\"; }\n", true).is_empty());
    }

    #[test]
    fn only_the_tools_folder_is_read_and_its_test_files_are_test_code() {
        assert!(read("tools/paddock-dev/src/places.rs") && !read("crates/a/src/lib.rs") && !read("tools/paddock-dev/tests/it/a.rs"));
        assert!(test_file("tools/paddock-dev/src/disk/tests.rs") && test_file("tools/paddock-dev/src/ci_runs/testutil.rs"));
        assert!(!test_file("tools/paddock-dev/src/places.rs"));
    }
}
