//! The OS signals and `select_os` with their positive controls (ER-33): each signal, put in a source, gives its `os` edge, and the same
//! source without it gives none; a word in a comment or a string is no signal.

use super::*;
use crate::model::extract::{MemoryRead, build};

/// A tiny lexer for the tests: `//` and `/* */` comments, `"` strings, and `#[cfg(test)]` items to the matching brace or `;`.
struct Lex;

impl RustLex for Lex {
    fn mask(&self, text: &str) -> String {
        blank(text, false)
    }

    fn uncommented(&self, text: &str) -> String {
        blank(text, true)
    }

    fn production(&self, masked: &str) -> String {
        let mut out: Vec<char> = masked.chars().collect();
        let chars = out.clone();
        let mut i = 0;
        while let Some(at) = masked[i..].find("#[cfg(test)]") {
            let start = i + at;
            let mut j = start;
            let mut depth = 0usize;
            while j < chars.len() {
                match chars[j] {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            j += 1;
                            break;
                        }
                    }
                    ';' if depth == 0 => {
                        j += 1;
                        break;
                    }
                    _ => {}
                }
                j += 1;
            }
            for c in &mut out[start..j.min(chars.len())] {
                if *c != '\n' {
                    *c = ' ';
                }
            }
            i = j.min(masked.len());
        }
        out.into_iter().collect()
    }
}

/// Blanks comments, and (unless `keep_strings`) the insides of strings.
fn blank(text: &str, keep_strings: bool) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                out.push(' ');
                i += 1;
            }
        } else if c[i] == '/' && c.get(i + 1) == Some(&'*') {
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                out.push(if c[i] == '\n' { '\n' } else { ' ' });
                i += 1;
            }
            out.push_str("  ");
            i += 2;
        } else if c[i] == '"' {
            out.push('"');
            i += 1;
            while i < c.len() && c[i] != '"' {
                out.push(if keep_strings || c[i] == '\n' {
                    c[i]
                } else {
                    ' '
                });
                i += 1;
            }
            out.push('"');
            i += 1;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

const MANIFEST: &str = "[package]\nname = \"x\"\n";

fn repo(files: &[(&str, &str)]) -> crate::model::extract::Built {
    let mut m = MemoryRead::new();
    for (p, t) in files {
        m = m.with(p, t);
    }
    let adapters: Vec<Box<dyn Extractor>> =
        vec![Box::new(OsExtractor::new(&Lex, &["tools/dev".to_string()]))];
    build(&m.paths(), &m, &adapters)
}

fn marks_of(b: &crate::model::extract::Built, from: &str) -> BTreeSet<String> {
    b.model
        .targets(from, EdgeKind::Os)
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// Each signal: the file that holds it, and a copy without it. The copy keeps the file, so the control shows the signal and nothing
/// else made the edge.
type Files = Vec<(&'static str, &'static str)>;

/// One case: its name, the signal, the files with it, the files without it, and the node that holds the mark.
type Case = (&'static str, Signal, Files, Files, &'static str);

fn cases() -> Vec<Case> {
    let lib = "crates/x/src/lib.rs";
    let crate_manifest = ("crates/x/Cargo.toml", MANIFEST);
    vec![
        (
            "cfg attribute",
            Signal::Cfg,
            vec![crate_manifest, (lib, "#[cfg(windows)]\nfn w() {}\n")],
            vec![crate_manifest, (lib, "fn w() {}\n")],
            "crate:crates/x",
        ),
        (
            "cfg! expression",
            Signal::Cfg,
            vec![crate_manifest, (lib, "fn w() -> bool { cfg!(windows) }\n")],
            vec![crate_manifest, (lib, "fn w() -> bool { true }\n")],
            "crate:crates/x",
        ),
        (
            "cfg target_os",
            Signal::Cfg,
            vec![
                crate_manifest,
                (lib, "#[cfg(target_os = \"macos\")]\nfn m() {}\n"),
            ],
            vec![
                crate_manifest,
                (lib, "#[cfg(feature = \"a\")]\nfn m() {}\n"),
            ],
            "crate:crates/x",
        ),
        (
            "cfg_attr",
            Signal::Cfg,
            vec![
                crate_manifest,
                (lib, "#[cfg_attr(unix, derive(Debug))]\nstruct S;\n"),
            ],
            vec![crate_manifest, (lib, "#[derive(Debug)]\nstruct S;\n")],
            "crate:crates/x",
        ),
        (
            "os api crate",
            Signal::Api,
            vec![crate_manifest, (lib, "fn p() -> u32 { libc::getpid() }\n")],
            vec![crate_manifest, (lib, "fn p() -> u32 { 1 }\n")],
            "crate:crates/x",
        ),
        (
            "windows crate",
            Signal::Api,
            vec![
                crate_manifest,
                (lib, "use windows::Win32::Foundation::HANDLE;\n"),
            ],
            vec![crate_manifest, (lib, "use endpoint::windows::Thing;\n")],
            "crate:crates/x",
        ),
        (
            "portable layer process",
            Signal::Layer,
            vec![
                crate_manifest,
                (lib, "fn run() { std::process::Command::new(\"git\"); }\n"),
            ],
            vec![crate_manifest, (lib, "fn run() { run_git(\"git\"); }\n")],
            "crate:crates/x",
        ),
        (
            "portable layer file lock",
            Signal::Layer,
            vec![crate_manifest, (lib, "fn l(f: &File) { f.try_lock(); }\n")],
            vec![crate_manifest, (lib, "fn l(f: &File) { f.read(); }\n")],
            "crate:crates/x",
        ),
        (
            "std env consts OS",
            Signal::EnvConsts,
            vec![
                crate_manifest,
                (lib, "fn o() -> &'static str { std::env::consts::OS }\n"),
            ],
            vec![crate_manifest, (lib, "fn o() -> &'static str { \"x\" }\n")],
            "crate:crates/x",
        ),
        (
            "build script target os",
            Signal::BuildScript,
            vec![
                crate_manifest,
                (
                    "crates/x/build.rs",
                    "fn main() { let os = std::env::var(\"CARGO_CFG_TARGET_OS\"); }\n",
                ),
            ],
            vec![
                crate_manifest,
                (
                    "crates/x/build.rs",
                    "fn main() { let os = std::env::var(\"OUT_DIR\"); }\n",
                ),
            ],
            "crate:crates/x",
        ),
        (
            "node platform",
            Signal::NodePlatform,
            vec![
                ("ui/package.json", "{}"),
                (
                    "ui/src/a.ts",
                    "export const win = process.platform === 'win32';\n",
                ),
            ],
            vec![
                ("ui/package.json", "{}"),
                ("ui/src/a.ts", "export const win = false;\n"),
            ],
            "npm:ui",
        ),
        (
            "node os.platform()",
            Signal::NodePlatform,
            vec![
                ("ui/package.json", "{}"),
                (
                    "ui/scripts/a.mjs",
                    "import os from 'node:os';\nconst p = os.platform();\n",
                ),
            ],
            vec![
                ("ui/package.json", "{}"),
                (
                    "ui/scripts/a.mjs",
                    "import os from 'node:os';\nconst p = os.EOL;\n",
                ),
            ],
            "npm:ui",
        ),
        (
            "dependency sysinfo",
            Signal::Dependency,
            vec![(
                "crates/x/Cargo.toml",
                "[package]\nname = \"x\"\n[dependencies]\nsysinfo = \"0.40\"\n",
            )],
            vec![(
                "crates/x/Cargo.toml",
                "[package]\nname = \"x\"\n[dependencies]\nserde = \"1\"\n",
            )],
            "crate:crates/x",
        ),
        (
            "dependency notify (dev)",
            Signal::Dependency,
            vec![(
                "crates/x/Cargo.toml",
                "[package]\nname = \"x\"\n[dev-dependencies]\nnotify = { version = \"8\" }\n",
            )],
            vec![(
                "crates/x/Cargo.toml",
                "[package]\nname = \"x\"\n[dev-dependencies]\ntempfile = \"3\"\n",
            )],
            "crate:crates/x",
        ),
        (
            "dependency tauri table",
            Signal::Dependency,
            vec![(
                "crates/x/Cargo.toml",
                "[package]\nname = \"x\"\n[dependencies.tauri]\nversion = \"2\"\n",
            )],
            vec![(
                "crates/x/Cargo.toml",
                "[package]\nname = \"x\"\n[dependencies.serde]\nversion = \"1\"\n",
            )],
            "crate:crates/x",
        ),
        (
            "target cfg table",
            Signal::Cfg,
            vec![(
                "crates/x/Cargo.toml",
                "[package]\nname = \"x\"\n[target.'cfg(windows)'.dependencies]\nfoo = \"1\"\n",
            )],
            vec![(
                "crates/x/Cargo.toml",
                "[package]\nname = \"x\"\n[dependencies]\nfoo = \"1\"\n",
            )],
            "crate:crates/x",
        ),
    ]
}

#[test]
fn every_signal_gives_its_mark_and_the_control_without_it_gives_none() {
    let mut seen: BTreeSet<Signal> = BTreeSet::new();
    for (name, kind, with, without, owner) in cases() {
        let b = repo(&with);
        assert!(b.errors.is_empty(), "{name}: {:?}", b.errors);
        assert!(
            marks_of(&b, owner).contains(&kind.mark()),
            "{name}: {:?}",
            marks_of(&b, owner)
        );
        let c = repo(&without);
        assert!(
            marks_of(&c, owner).is_empty(),
            "{name} control: {:?}",
            marks_of(&c, owner)
        );
        seen.insert(kind);
    }
    assert_eq!(
        seen.len(),
        Signal::ALL.len(),
        "a signal without a case: {seen:?}"
    );
}

#[test]
fn a_word_in_a_comment_or_a_string_is_no_signal() {
    let lib = "crates/x/src/lib.rs";
    let text = "// cfg(windows) libc::x std::process::Command CARGO_CFG_TARGET_OS\n/* cfg(unix) */\nfn f() { let s = \"libc::getpid() cfg(windows) std::env::consts::OS\"; }\n";
    let b = repo(&[
        ("crates/x/Cargo.toml", MANIFEST),
        (lib, text),
        (
            "crates/x/build.rs",
            "// CARGO_CFG_TARGET_OS\nfn main() {}\n",
        ),
    ]);
    assert!(
        marks_of(&b, "crate:crates/x").is_empty(),
        "{:?}",
        marks_of(&b, "crate:crates/x")
    );
    let ts = repo(&[
        ("ui/package.json", "{}"),
        (
            "ui/a.ts",
            "// process.platform\n/* os.platform() */\nconst a = 1;\n",
        ),
    ]);
    assert!(marks_of(&ts, "npm:ui").is_empty());
    let manifest = repo(&[(
        "crates/x/Cargo.toml",
        "[package]\nname = \"x\"\n# sysinfo = \"1\"\n[dependencies]\n# tauri = \"2\"\nserde = \"1\"\n",
    )]);
    assert!(marks_of(&manifest, "crate:crates/x").is_empty());
}

#[test]
fn a_process_in_a_test_or_a_test_item_is_no_layer_signal_but_a_cfg_in_a_test_is_a_cfg_signal() {
    let m = |t: &str, file: &str| repo(&[("crates/x/Cargo.toml", MANIFEST), (file, t)]);
    let cmd = "std::process::Command::new(\"node\");";
    let tests = m(&format!("fn t() {{ {cmd} }}\n"), "crates/x/tests/it/a.rs");
    assert!(marks_of(&tests, "crate:crates/x").is_empty());
    let item = m(
        &format!("fn p() {{}}\n#[cfg(test)]\nmod t {{ fn t() {{ {cmd} }} }}\n"),
        "crates/x/src/lib.rs",
    );
    assert!(
        marks_of(&item, "crate:crates/x").is_empty(),
        "{:?}",
        marks_of(&item, "crate:crates/x")
    );
    let prod = m(&format!("fn p() {{ {cmd} }}\n"), "crates/x/src/lib.rs");
    assert!(marks_of(&prod, "crate:crates/x").contains("os:layer"));
    let cfg = m("#[cfg(windows)]\nfn t() {}\n", "crates/x/tests/it/a.rs");
    assert!(marks_of(&cfg, "crate:crates/x").contains("os:cfg"));
}

#[test]
fn a_site_that_names_windows_also_gives_the_windows_mark() {
    let b = repo(&[
        ("crates/x/Cargo.toml", MANIFEST),
        ("crates/x/src/lib.rs", "#[cfg(windows)]\nfn w() {}\n"),
    ]);
    assert!(marks_of(&b, "crate:crates/x").contains(WINDOWS_MARK));
    let u = repo(&[
        ("crates/x/Cargo.toml", MANIFEST),
        ("crates/x/src/lib.rs", "#[cfg(unix)]\nfn w() {}\n"),
    ]);
    assert!(!marks_of(&u, "crate:crates/x").contains(WINDOWS_MARK));
    let api = repo(&[
        ("crates/x/Cargo.toml", MANIFEST),
        ("crates/x/src/lib.rs", "use windows_sys::Win32::X;\n"),
    ]);
    assert!(marks_of(&api, "crate:crates/x").contains(WINDOWS_MARK));
}

#[test]
fn a_manifest_that_is_no_member_has_no_node_or_mark() {
    let b = repo(&[
        ("crates/vendor/t/Cargo.toml", "[package]\nname = \"t\"\n"),
        ("crates/vendor/t/src/lib.rs", "#[cfg(windows)]\nfn w() {}\n"),
        (
            "crates/x/Cargo.toml",
            "[workspace]\nmembers = []\n[package]\nname = \"x\"\n",
        ),
    ]);
    assert_eq!(
        b.model.edges().count(),
        0,
        "{:?}",
        b.model.edges().collect::<Vec<_>>()
    );
}

/// A model with a crate, its test target and a test file, built by hand (the Cargo adapter is not in play here).
fn with_tests(marked_lib: bool, marked_test: bool) -> Model {
    let b = repo(&[
        ("crates/x/Cargo.toml", MANIFEST),
        (
            "crates/x/src/lib.rs",
            if marked_lib {
                "#[cfg(windows)]\nfn w() {}\n"
            } else {
                "fn w() {}\n"
            },
        ),
        (
            "crates/x/tests/it/a.rs",
            if marked_test {
                "#[cfg(unix)]\n#[test]\nfn t() {}\n"
            } else {
                "#[test]\nfn t() {}\n"
            },
        ),
    ]);
    let mut m = b.model;
    m.add_node(ModelNode::new(
        "crate:crates/x",
        NodeKind::Project,
        &["crates/x/"],
    ));
    m.add_node(ModelNode::new("nextest:x::it", NodeKind::TestTarget, &[]));
    m.add_edge(Edge::new(
        "nextest:x::it",
        "crate:crates/x",
        EdgeKind::Depends,
    ));
    m
}

#[test]
fn select_os_is_linux_for_a_portable_node_and_adds_the_hosts_for_a_marked_one() {
    let all = BTreeSet::from([Os::Linux, Os::Windows, Os::MacOs]);
    let one = BTreeSet::from([Os::Linux]);
    assert_eq!(
        select_os(&with_tests(true, false), "crate:crates/x", &[]),
        all
    );
    assert_eq!(
        select_os(&with_tests(true, false), "nextest:x::it", &[]),
        all,
        "a target takes its crate's marks"
    );
    // Positive control: the same crate without the signal is portable, and so is its target.
    assert_eq!(
        select_os(&with_tests(false, false), "crate:crates/x", &[]),
        one
    );
    assert_eq!(
        select_os(&with_tests(false, false), "nextest:x::it", &[]),
        one
    );
    assert_eq!(
        select_os(&with_tests(false, false), "no:such", &[]),
        one,
        "an unknown node is portable, never an error"
    );
}

#[test]
fn the_runner_labels_are_the_ones_ci_uses() {
    assert_eq!(
        [Os::Linux, Os::Windows, Os::MacOs].map(Os::runner),
        ["ubuntu-latest", "windows-latest", "macos-latest"]
    );
}

fn dev(files: &[(&str, &str)]) -> crate::model::extract::Built {
    let mut all = vec![("tools/dev/Cargo.toml", MANIFEST)];
    all.extend_from_slice(files);
    repo(&all)
}

#[test]
fn a_module_crate_is_judged_per_module_and_a_mark_reaches_what_the_marked_module_calls() {
    let caller = "use crate::callee::go;\nfn c() { go(); }\n#[cfg(windows)]\nfn w() {}\n";
    let b = dev(&[
        ("tools/dev/src/caller.rs", caller),
        ("tools/dev/src/callee.rs", "pub fn go() {}\n"),
        ("tools/dev/src/other.rs", "pub fn o() {}\n"),
    ]);
    let id = |m: &str| module_id("tools/dev", m);
    assert!(marks_of(&b, &id("caller")).contains("os:cfg"));
    assert!(
        marks_of(&b, "crate:tools/dev").is_empty(),
        "module code is the module's, not the crate's"
    );
    let hosts = BTreeSet::from([Os::Linux, Os::Windows, Os::MacOs]);
    assert_eq!(select_os(&b.model, &id("caller"), &[]), hosts);
    assert_eq!(
        select_os(&b.model, &id("callee"), &[]),
        hosts,
        "the host code calls it"
    );
    assert_eq!(
        select_os(&b.model, &id("other"), &[]),
        BTreeSet::from([Os::Linux])
    );
    // Positive control: without the `use crate::callee` line the callee is portable again.
    let c = dev(&[
        (
            "tools/dev/src/caller.rs",
            "fn c() {}\n#[cfg(windows)]\nfn w() {}\n",
        ),
        ("tools/dev/src/callee.rs", "pub fn go() {}\n"),
    ]);
    assert_eq!(
        select_os(&c.model, &id("callee"), &[]),
        BTreeSet::from([Os::Linux])
    );
    // A caller that is a test item does not make its callee host code.
    let t = dev(&[
        (
            "tools/dev/src/caller.rs",
            "fn c() {}\n#[cfg(test)]\nmod t { use crate::callee::go; }\n#[cfg(windows)]\nfn w() {}\n",
        ),
        ("tools/dev/src/callee.rs", "pub fn go() {}\n"),
    ]);
    assert_eq!(
        select_os(&t.model, &id("callee"), &[]),
        BTreeSet::from([Os::Linux])
    );
}

#[test]
fn a_test_file_of_a_module_crate_marks_the_module_it_names() {
    let b = dev(&[
        ("tools/dev/src/worktree/mod.rs", "pub fn a() {}\n"),
        (
            "tools/dev/tests/it/worktree/mod.rs",
            "#[cfg(windows)]\n#[test]\nfn t() {}\n",
        ),
        (
            "tools/dev/tests/it/golden/planner.rs",
            "#[test]\nfn t() {}\n",
        ),
    ]);
    assert!(marks_of(&b, &module_id("tools/dev", "worktree")).contains("os:cfg"));
    assert!(marks_of(&b, &module_id("tools/dev", "golden::planner")).is_empty());
}

#[test]
fn ignoring_a_signal_shows_what_marking_it_costs() {
    // `std::process::Command` is the widest signal: a crate that only starts a program is marked by it alone.
    let files = [
        ("crates/x/Cargo.toml", MANIFEST),
        (
            "crates/x/src/lib.rs",
            "fn r() { std::process::Command::new(\"git\"); }\n",
        ),
    ];
    let mut m = MemoryRead::new();
    for (p, t) in files {
        m = m.with(p, t);
    }
    let with: Vec<Box<dyn Extractor>> = vec![Box::new(OsExtractor::new(&Lex, &[]))];
    let without: Vec<Box<dyn Extractor>> = vec![Box::new(
        OsExtractor::new(&Lex, &[]).ignoring(Signal::Layer),
    )];
    let a = build(&m.paths(), &m, &with);
    let b = build(&m.paths(), &m, &without);
    assert_eq!(select_os(&a.model, "crate:crates/x", &[]).len(), 3);
    assert_eq!(select_os(&b.model, "crate:crates/x", &[]).len(), 1);
    // The query can leave a mark out too.
    assert_eq!(
        select_os(&a.model, "crate:crates/x", &[Signal::Layer.mark()]).len(),
        1
    );
}

#[test]
fn the_adapter_reads_in_one_bulk_call_and_starts_no_process() {
    let m = MemoryRead::new()
        .with("crates/x/Cargo.toml", MANIFEST)
        .with("crates/x/src/lib.rs", "fn a() {}\n")
        .with("ui/package.json", "{}")
        .with("ui/a.ts", "const a = 1;\n");
    let adapters: Vec<Box<dyn Extractor>> = vec![Box::new(OsExtractor::new(&Lex, &[]))];
    let b = build(&m.paths(), &m, &adapters);
    assert_eq!(m.calls(), 1);
    assert_eq!(b.spawns, 0);
}

#[test]
fn crate_paths_read_plain_and_grouped_uses_in_production_code() {
    let text = "use crate::a::b;\nuse crate::{c, d::e, f::{g, h as i, self}};\nfn x() { crate::j::k(); }\n#[cfg(test)]\nmod t { use crate::z::y; }\n";
    let masked = Lex.mask(text);
    let got: Vec<String> = crate_paths(&masked, &Lex)
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    for want in ["a::b", "c", "d::e", "f::g", "f::h", "f", "j::k"] {
        assert!(got.iter().any(|p| p == want), "{want} in {got:?}");
    }
    assert!(
        !got.iter().any(|p| p.starts_with('z')),
        "a test item is not production: {got:?}"
    );
}

#[test]
fn the_registry_adds_the_os_adapter_only_with_a_lexer() {
    use crate::model::extract::{Ports, extractors};
    let none = Ports {
        cargo: None,
        root: "/r",
        rust_lex: None,
        module_crates: &[],
    };
    assert!(extractors(&none).iter().all(|a| a.name() != "os"));
    let some = Ports {
        rust_lex: Some(&Lex),
        ..none
    };
    assert!(extractors(&some).iter().any(|a| a.name() == "os"));
}
