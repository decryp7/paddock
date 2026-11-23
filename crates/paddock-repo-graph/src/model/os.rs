//! OS placement derived from the code (ADR 0117 §7, WP-6): an extractor that finds the places where code behaves differently per
//! operating system and emits an `os` edge from the node that holds it to a mark node, and the query [`select_os`] that turns the
//! marks into the set of operating systems a node's tests run on. The rule (user decision 2026-11-22, "a test runs on an OS only if
//! its code behaves differently there"): a node with no mark is portable and runs on Linux only; a node with a mark also runs on the
//! Windows and macOS hosts. Declared exceptions (`hostTests` in a `paddock.ci.json`) add or take away; this module knows none.
//!
//! The signals, each one mark (`os:<name>`), all of them also `os:windows` when the site names Windows:
//!
//! | mark | signal |
//! |---|---|
//! | `os:cfg` | an OS `cfg` (`cfg(windows)`, `cfg(unix)`, `target_os`, `target_family`, `cfg_attr`, and the expression `cfg!(..)`), in code and in a manifest's `[target.'cfg(..)']` table |
//! | `os:api` | a use of an OS-API crate (`windows_sys`, `libc`, `objc2`, `core_foundation`, `security_framework`, `zbus`, `nix`, `rustix`, `winreg`, `windows`, `winapi`) |
//! | `os:layer` | an entry point of the portable layer of ADR 0109 in production code (`paddock_proc::command`, `paddock_env::winpath`/`atomic`, `std::process::Command`, a file lock) |
//! | `os:build-script` | `CARGO_CFG_TARGET_OS` (or `_FAMILY`) read by a `build.rs` |
//! | `os:env-consts` | `std::env::consts::OS` or `FAMILY` |
//! | `os:node-platform` | `process.platform` or `os.platform()` in TypeScript or JavaScript |
//! | `os:dependency` | a dependency whose behaviour differs by OS (`sysinfo`, `notify`, `tauri`, and the OS-API crates) |
//!
//! Comments and string contents are masked before a Rust file is read, so a word in a comment is not a use. Masking is the host's
//! lexer, behind the [`RustLex`] port (the core stays free of a parser dependency; paddock-dev supplies rustc's own lexer).
//!
//! Granularity (ADR 0117 §7): a package for every crate and npm package; a module for the crates the caller names (paddock-dev,
//! whose tests are selected by test-name prefix). A module that calls another (`crate::m`, production code) is an `imports` edge, and
//! a mark on the importer reaches the importee: the host code calls it, so its tests run where the caller's do.

use super::extract::{BulkRead, ExtractError, Extracted, Extractor};
use super::nodes::{Edge, EdgeKind, Model, ModelNode, NodeKind};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

/// The host's Rust lexer (rustc's own, in paddock-dev): what the extractor needs to read code and not comments or strings.
pub trait RustLex {
    /// `text` with comments and the insides of literals blanked (newlines and offsets kept).
    fn mask(&self, text: &str) -> String;
    /// `text` with comments blanked and literals kept (a build script's `"CARGO_CFG_TARGET_OS"` is a literal).
    fn uncommented(&self, text: &str) -> String;
    /// `masked` (a [`RustLex::mask`] result) with every test-only item blanked, offsets and newlines kept.
    fn production(&self, masked: &str) -> String;
}

/// An operating system a test can run on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Os {
    /// The Linux runner: runs everything.
    Linux,
    /// The Windows host runner.
    Windows,
    /// The macOS host runner.
    MacOs,
}

impl Os {
    /// The runner label (`ubuntu-latest`, `windows-latest`, `macos-latest`).
    pub fn runner(self) -> &'static str {
        match self {
            Os::Linux => "ubuntu-latest",
            Os::Windows => "windows-latest",
            Os::MacOs => "macos-latest",
        }
    }
}

/// The kinds of signal; each is one mark node `os:<name>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Signal {
    /// An OS `cfg`.
    Cfg,
    /// A use of an OS-API crate.
    Api,
    /// An entry point of the portable layer.
    Layer,
    /// `CARGO_CFG_TARGET_OS` in a build script.
    BuildScript,
    /// `std::env::consts::OS`.
    EnvConsts,
    /// `process.platform`.
    NodePlatform,
    /// A dependency that behaves differently per OS.
    Dependency,
}

impl Signal {
    /// Every kind, in order.
    pub const ALL: [Signal; 7] = [
        Signal::Cfg,
        Signal::Api,
        Signal::Layer,
        Signal::BuildScript,
        Signal::EnvConsts,
        Signal::NodePlatform,
        Signal::Dependency,
    ];

    /// The word of the mark and of the report (`cfg`, `os-api`...).
    pub fn name(self) -> &'static str {
        match self {
            Signal::Cfg => "cfg",
            Signal::Api => "os-api",
            Signal::Layer => "layer",
            Signal::BuildScript => "build-script",
            Signal::EnvConsts => "env-consts",
            Signal::NodePlatform => "node-platform",
            Signal::Dependency => "dependency",
        }
    }

    /// The id of the mark node.
    pub fn mark(self) -> String {
        format!("os:{}", self.name())
    }
}

/// The mark of a site that names Windows (added beside the kind's own mark).
pub const WINDOWS_MARK: &str = "os:windows";

/// One OS-specific site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// The 1-based line.
    pub line: usize,
    /// What it is.
    pub kind: Signal,
    /// The site is Windows code: a cfg that names `windows`, a Windows API crate, `win32`.
    pub windows: bool,
}

fn re(s: &str) -> Regex {
    Regex::new(s).unwrap_or_else(|e| panic!("bad built-in pattern {s}: {e}"))
}

// The span of a cfg ends at `;`, a brace or the `]` that closes the attribute, so a word of a following item is never read as part of it.
static CFG: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?-u:\b)cfg(?:_attr)?!?[ \t\r\n]*\([^;{}\]]{0,300}?(?-u:\b)(?:windows|unix|target_os|target_family)(?-u:\b)",
    )
});
// `windows::` only as a crate (not after `::`, where it is one of our own `endpoint::windows` modules).
static API: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?-u:\b)(?:windows_sys|windows_core|winapi|libc|objc2|core_foundation|security_framework|zbus|nix|rustix|winreg)[ \t\r\n]*::|(?:^|[^:A-Za-z0-9_])windows[ \t\r\n]*::",
    )
});
static LAYER: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?-u:\b)paddock_proc[ \t\r\n]*::[ \t\r\n]*command(?-u:\b)|(?-u:\b)paddock_env[ \t\r\n]*::[ \t\r\n]*(?:winpath|atomic)(?-u:\b)|(?-u:\b)process[ \t\r\n]*::[ \t\r\n]*Command(?-u:\b)|(?-u:\b)process[ \t\r\n]*::[ \t\r\n]*\{[^}]*(?-u:\b)Command(?-u:\b)|(?-u:\b)fd_lock(?-u:\b)|(?-u:\b)fs2[ \t\r\n]*::|(?-u:\b)(?:try_lock|lock_exclusive|flock)(?-u:\b)",
    )
});
static ENV_CONSTS: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?-u:\b)consts[ \t\r\n]*::[ \t\r\n]*(?:\{[^}]*(?-u:\b))?(?:OS|FAMILY)(?-u:\b)")
});
static BUILD_CFG: LazyLock<Regex> =
    LazyLock::new(|| re(r"CARGO_CFG_(?:TARGET_OS|TARGET_FAMILY|WINDOWS|UNIX)(?-u:\b)"));
static NODE_PLATFORM: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?-u:\b)process[ \t\r\n]*[.][ \t\r\n]*platform(?-u:\b)|(?-u:\b)os[ \t\r\n]*[.][ \t\r\n]*(?:platform|type)[ \t\r\n]*\(",
    )
});
static CRATE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?-u:\b)crate[ \t\r\n]*::[ \t\r\n]*([A-Za-z_][A-Za-z0-9_]*(?:[ \t\r\n]*::[ \t\r\n]*[A-Za-z_][A-Za-z0-9_]*)*)",
    )
});
static TABLE: LazyLock<Regex> = LazyLock::new(|| re(r"^\[+[ \t]*([^\]]*?)[ \t]*\]+"));
static KEY: LazyLock<Regex> = LazyLock::new(|| re(r"^([A-Za-z0-9_-]+)[ \t]*[=.]"));

/// The dependencies whose behaviour differs by OS (review P2-4): the OS-API crates, a process-supervision and file-watching pair, and
/// the Tauri runtime. Names as in a manifest (`-`, not `_`).
const OS_DEPENDENCIES: [&str; 16] = [
    "sysinfo",
    "notify",
    "tauri",
    "windows",
    "windows-sys",
    "windows-core",
    "winapi",
    "winreg",
    "libc",
    "nix",
    "rustix",
    "objc2",
    "core-foundation",
    "security-framework",
    "zbus",
    "fd-lock",
];

/// Words one of which every Rust signal contains (a file without any has no site): `cfg`, the OS-API crate names, the portable layer's
/// entry points, `consts::` (`env::consts::OS`) and `CARGO_CFG`.
const SIGNAL_WORDS: [&str; 22] = [
    "cfg",
    "windows",
    "libc",
    "objc2",
    "core_foundation",
    "security_framework",
    "zbus",
    "nix",
    "rustix",
    "winreg",
    "winapi",
    "paddock_proc",
    "paddock_env",
    "process",
    "fd_lock",
    "fs2",
    "try_lock",
    "lock_exclusive",
    "flock",
    "consts",
    "CARGO_CFG",
    "Command",
];

fn line_of(s: &str, at: usize) -> usize {
    s[..at].bytes().filter(|b| *b == b'\n').count() + 1
}

/// Whether a file is test code: under a `tests/` folder, or named `tests.rs` or `*_tests.rs` (a module only `#[cfg(test)]` includes).
pub fn is_test_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    rel.split('/').any(|s| s == "tests") || name == "tests.rs" || name.ends_with("_tests.rs")
}

/// The OS-specific sites of one Rust source file. An OS `cfg`, a call into an OS-API crate, `env::consts::OS` and (in a build script)
/// `CARGO_CFG_TARGET_OS` count anywhere (a `cfg(windows)` test is an OS-specific test); an entry point of the portable layer (a
/// process, a lock) counts only in production code: a test that starts `node` or `git` to run a check is not code that behaves
/// differently per OS, the code it drives is judged where it is. `test_file`: [`is_test_file`]; `build_script`: the file is `build.rs`.
pub fn rust_sites(text: &str, test_file: bool, build_script: bool, lex: &dyn RustLex) -> Vec<Site> {
    // Most files hold none of the words a signal is made of: a substring test is far cheaper than lexing them (measured in
    // docs/benchmarks.md, "OS placement derived from the code"). A file that holds a word is read in full.
    if !SIGNAL_WORDS.iter().any(|w| text.contains(w)) {
        return Vec::new();
    }
    let m = lex.mask(text);
    let production = if test_file {
        String::new()
    } else {
        lex.production(&m)
    };
    let mut out = Vec::new();
    for (re, kind, scanned) in [
        (&*CFG, Signal::Cfg, &m),
        (&*API, Signal::Api, &m),
        (&*LAYER, Signal::Layer, &production),
        (&*ENV_CONSTS, Signal::EnvConsts, &m),
    ] {
        for hit in re.find_iter(scanned) {
            // A match may start on the character before the word (the `windows::` form): the site is the word.
            let word = hit
                .as_str()
                .trim_start_matches(|c: char| !c.is_alphanumeric() && c != '_');
            let at = hit.end() - word.len();
            // Strings are blanked in `m`, so `target_os = "windows"` is read from the original (offsets are kept).
            let original = text.get(hit.start()..hit.end()).unwrap_or("");
            let windows = (kind == Signal::Cfg && original.contains("windows"))
                || (kind == Signal::Api
                    && (word.starts_with("windows")
                        || word.starts_with("winreg")
                        || word.starts_with("winapi")));
            out.push(Site {
                line: line_of(scanned, at),
                kind,
                windows,
            });
        }
    }
    if build_script {
        let code = lex.uncommented(text);
        for hit in BUILD_CFG.find_iter(&code) {
            out.push(Site {
                line: line_of(&code, hit.start()),
                kind: Signal::BuildScript,
                windows: hit.as_str().contains("WINDOWS"),
            });
        }
    }
    out.sort_by_key(|s| (s.line, s.kind));
    out.dedup();
    out
}

/// The OS-specific sites of a TypeScript or JavaScript source: `process.platform` and `os.platform()` in code (comments blanked).
pub fn script_sites(text: &str) -> Vec<Site> {
    let code = crate::imports::strip_comments(text);
    NODE_PLATFORM
        .find_iter(&code)
        .map(|h| Site {
            line: line_of(&code, h.start()),
            kind: Signal::NodePlatform,
            windows: false,
        })
        .collect()
}

/// The OS-specific sites of a Cargo manifest: a `[target.'cfg(..)'...]` table and a dependency of [`OS_DEPENDENCIES`] in any
/// dependency table (`[dependencies]`, dev, build, target tables, `[dependencies.name]`).
pub fn manifest_sites(text: &str) -> Vec<Site> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some(t) = TABLE.captures(line).and_then(|c| c.get(1)) {
            let name = t.as_str().trim();
            in_deps = false;
            if name.starts_with("workspace") {
                continue;
            }
            if name.starts_with("target.")
                && (name.contains("cfg(")
                    || name.contains("windows")
                    || name.contains("apple")
                    || name.contains("linux"))
            {
                out.push(Site {
                    line: i + 1,
                    kind: Signal::Cfg,
                    windows: name.contains("windows"),
                });
            }
            let tail = name
                .rsplit_once('.')
                .map_or(name, |(_, last)| last.trim_matches(['\'', '"']));
            let table = name.rsplit_once("dependencies").map(|(_, rest)| rest);
            match table {
                Some("") => in_deps = true,
                // `[dependencies.tauri]`: the dependency is the table's last segment.
                Some(rest) if rest.starts_with('.') && OS_DEPENDENCIES.contains(&tail) => {
                    out.push(Site {
                        line: i + 1,
                        kind: Signal::Dependency,
                        windows: tail.starts_with("windows")
                            || tail == "winapi"
                            || tail == "winreg",
                    })
                }
                _ => {}
            }
            continue;
        }
        if in_deps
            && let Some(k) = KEY.captures(line).and_then(|c| c.get(1))
            && OS_DEPENDENCIES.contains(&k.as_str())
        {
            let n = k.as_str();
            out.push(Site {
                line: i + 1,
                kind: Signal::Dependency,
                windows: n.starts_with("windows") || n == "winapi" || n == "winreg",
            });
        }
    }
    out
}

/// The `crate::` paths a source file's production code names, one per use: `crate::a::b` is `a::b`, and a group `crate::{a, b::c}`
/// is `a` and `b::c`. With the line of each. `masked` is a [`RustLex::mask`] result.
pub fn crate_paths(masked: &str, lex: &dyn RustLex) -> Vec<(usize, String)> {
    let m = lex.production(masked);
    let mut out = Vec::new();
    for cap in CRATE_PATH.captures_iter(&m) {
        let Some(g) = cap.get(1) else { continue };
        out.push((
            line_of(&m, g.start()),
            g.as_str()
                .split("::")
                .map(str::trim)
                .collect::<Vec<_>>()
                .join("::"),
        ));
    }
    // `use crate::{a, b::c, d::{e, f}};`
    let mut from = 0;
    while let Some(i) = m[from..].find("crate::{") {
        let open = from + i + "crate::".len();
        let Some(body) = braced(&m[open..]) else {
            break;
        };
        for p in group_paths(body, "") {
            out.push((line_of(&m, open), p));
        }
        from = open + body.len();
    }
    out
}

/// The text inside the `{ .. }` that starts `s` (without the braces), or `None` when it never closes.
fn braced(s: &str) -> Option<&str> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&s[1..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The leaf paths of a use group body (`a, b::c, d::{e, f}` under `prefix`): `a`, `b::c`, `d::e`, `d::f`. `self` leaves are the prefix.
fn group_paths(body: &str, prefix: &str) -> Vec<String> {
    let mut parts: Vec<&str> = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (i, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    let mut out = Vec::new();
    for p in parts.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
        let joined = |tail: &str| {
            if prefix.is_empty() {
                tail.to_string()
            } else {
                format!("{prefix}::{tail}")
            }
        };
        if let Some(open) = p.find('{') {
            let head = p[..open].trim_end_matches([':', ' ']);
            let inner = braced(&p[open..]).unwrap_or("");
            out.extend(group_paths(inner, &joined(head)));
        } else {
            let leaf = p.split(" as ").next().unwrap_or(p).trim();
            out.push(if leaf == "self" {
                prefix.to_string()
            } else {
                joined(leaf)
            });
        }
    }
    out.retain(|p| !p.is_empty());
    out
}

/// Whether a manifest is no member of the workspace around it: it has a `[workspace]` of its own, or sits in a folder cargo's workspace
/// excludes by convention here (`vendor`, `docs` research harnesses, `fuzz` crates). cargo cannot select such a package with `-p`.
pub fn non_member(rel: &str, text: &str) -> bool {
    text.contains("[workspace]")
        || rel
            .split('/')
            .any(|s| matches!(s, "vendor" | "docs" | "fuzz"))
}

/// The module path of a file of a crate's `src/` (`ci_host/kept_clone.rs` is `ci_host::kept_clone`, `x/mod.rs` is `x`); `None` for
/// `lib.rs` and `main.rs`. With `tests`: a test-only file (`tests.rs`, a `tests/` folder) too, else `None` for those.
pub fn src_module(rel: &str, crate_dir: &str, tests: bool) -> Option<String> {
    let inner = rel
        .strip_prefix(crate_dir)?
        .strip_prefix("/src/")?
        .strip_suffix(".rs")?;
    if inner == "lib" || inner == "main" {
        return None;
    }
    let test_only =
        inner.rsplit('/').next() == Some("tests") || inner.split('/').any(|s| s == "tests");
    if test_only && !tests {
        return None;
    }
    Some(
        inner
            .strip_suffix("/mod")
            .unwrap_or(inner)
            .replace('/', "::"),
    )
}

/// The module a file of a crate's `tests/it/` belongs to (`worktree/mod.rs` is `worktree`, `golden/planner.rs` is `golden::planner`):
/// the test-name prefix of its tests. `None` for `main.rs`.
pub fn it_module(rel: &str, crate_dir: &str) -> Option<String> {
    let inner = rel
        .strip_prefix(crate_dir)?
        .strip_prefix("/tests/it/")?
        .strip_suffix(".rs")?;
    if inner == "main" {
        return None;
    }
    Some(
        inner
            .strip_suffix("/mod")
            .unwrap_or(inner)
            .replace('/', "::"),
    )
}

/// The id of a module's node.
pub fn module_id(crate_dir: &str, module: &str) -> String {
    format!("module:{crate_dir}::{module}")
}

/// The adapter: OS marks per package (`crate:<dir>`, `npm:<dir>`) and per module of the crates named in `module_crates`.
pub struct OsExtractor<'a> {
    lex: &'a dyn RustLex,
    module_crates: Vec<String>,
    ignore: BTreeSet<Signal>,
}

impl<'a> OsExtractor<'a> {
    /// The adapter over a lexer; `module_crates` are the crate folders judged module by module (paddock-dev).
    pub fn new(lex: &'a dyn RustLex, module_crates: &[String]) -> OsExtractor<'a> {
        OsExtractor {
            lex,
            module_crates: module_crates.to_vec(),
            ignore: BTreeSet::new(),
        }
    }

    /// The same adapter that leaves a kind of signal out (the measurement of what marking `std::process::Command` costs or saves).
    pub fn ignoring(mut self, kind: Signal) -> OsExtractor<'a> {
        self.ignore.insert(kind);
        self
    }
}

/// The deepest folder of `dirs` that holds `file`.
fn deepest<'a>(file: &str, dirs: &'a BTreeSet<String>) -> Option<&'a str> {
    let mut at = file;
    while let Some((head, _)) = at.rsplit_once('/') {
        if let Some(d) = dirs.get(head) {
            return Some(d.as_str());
        }
        at = head;
    }
    dirs.get("").map(String::as_str)
}

fn manifest_dir(path: &str) -> &str {
    path.strip_suffix("Cargo.toml")
        .or_else(|| path.strip_suffix("package.json"))
        .unwrap_or(path)
        .trim_end_matches('/')
}

impl Extractor for OsExtractor<'_> {
    fn name(&self) -> &'static str {
        "os"
    }

    fn claims(&self) -> &[&'static str] {
        &[
            "Cargo.toml",
            "package.json",
            ".rs",
            ".ts",
            ".tsx",
            ".mts",
            ".mjs",
            ".cjs",
            ".js",
        ]
    }

    fn extract(&self, files: &[String], read: &dyn BulkRead) -> Result<Extracted, ExtractError> {
        // A text that cannot be read fails the extractor (fail closed); a file that is not there is skipped.
        let texts = super::extract::texts_of("os", read, files)?;
        let text_of: BTreeMap<&str, &str> = texts
            .iter()
            .map(|(f, t)| (f.as_str(), t.as_str()))
            .collect();
        let manifest = |f: &&String| f.rsplit('/').next() == Some("Cargo.toml");
        let crates: BTreeSet<String> = files
            .iter()
            .filter(manifest)
            .filter(|f| {
                // A one-crate workspace (paddock-dev) has a `[workspace]` of its own and is still the crate its modules belong to.
                let one_crate = self.module_crates.iter().any(|c| c == manifest_dir(f));
                text_of
                    .get(f.as_str())
                    .is_some_and(|t| t.contains("[package]") && (one_crate || !non_member(f, t)))
            })
            .map(|f| manifest_dir(f).to_string())
            .collect();
        let packages: BTreeSet<String> = files
            .iter()
            .filter(|f| f.rsplit('/').next() == Some("package.json"))
            .map(|f| manifest_dir(f).to_string())
            .collect();
        let mut found: BTreeMap<String, BTreeSet<(Signal, bool)>> = BTreeMap::new();
        let mut modules: BTreeMap<String, (String, String)> = BTreeMap::new(); // id -> (crate dir, module)
        let mut imports: Vec<(String, String)> = Vec::new(); // (importer id, target path as `a::b`) per crate
        let mut importer_files: Vec<(&str, String, &str)> = Vec::new();
        for f in files {
            let Some(text) = text_of.get(f.as_str()) else {
                continue;
            };
            let name = f.rsplit('/').next().unwrap_or(f);
            let mut put = |id: String, sites: Vec<Site>| {
                for s in sites.into_iter().filter(|s| !self.ignore.contains(&s.kind)) {
                    found
                        .entry(id.clone())
                        .or_default()
                        .insert((s.kind, s.windows));
                }
            };
            if name == "Cargo.toml" {
                if crates.contains(manifest_dir(f)) {
                    put(format!("crate:{}", manifest_dir(f)), manifest_sites(text));
                }
            } else if f.ends_with(".rs") {
                let Some(dir) = deepest(f, &crates) else {
                    continue;
                };
                let sites = rust_sites(text, is_test_file(f), name == "build.rs", self.lex);
                let module = self
                    .module_crates
                    .iter()
                    .any(|c| c == dir)
                    .then(|| src_module(f, dir, true).or_else(|| it_module(f, dir)))
                    .flatten();
                match module {
                    Some(m) => {
                        let id = module_id(dir, &m);
                        modules.insert(id.clone(), (dir.to_string(), m));
                        put(id.clone(), sites);
                        if src_module(f, dir, false).is_some() {
                            importer_files.push((dir, id, text));
                        }
                    }
                    None => put(format!("crate:{dir}"), sites),
                }
            } else if let Some(dir) = deepest(f, &packages) {
                put(format!("npm:{dir}"), script_sites(text));
            }
        }
        // `imports` between modules: a production use of `crate::m`, to the longest module path that names a source file.
        let by_crate: BTreeMap<&str, BTreeSet<&str>> =
            modules.values().fold(BTreeMap::new(), |mut acc, (d, m)| {
                acc.entry(d.as_str()).or_default().insert(m.as_str());
                acc
            });
        for (dir, id, text) in importer_files
            .iter()
            .filter(|(_, _, t)| t.contains("crate::"))
        {
            let masked = self.lex.mask(text);
            let known = by_crate.get(dir);
            for (_, path) in crate_paths(&masked, self.lex) {
                let segs: Vec<&str> = path.split("::").collect();
                let target = (1..=segs.len())
                    .rev()
                    .map(|n| segs[..n].join("::"))
                    .find(|p| known.is_some_and(|k| k.contains(p.as_str())));
                if let Some(t) = target {
                    imports.push((id.clone(), module_id(dir, &t)));
                }
            }
        }
        let mut x = Extracted::default();
        for id in modules.keys() {
            x.nodes.push(ModelNode::new(id, NodeKind::Project, &[]));
        }
        for (from, mark_kinds) in &found {
            for (kind, windows) in mark_kinds {
                x.edges.push(Edge::new(from, kind.mark(), EdgeKind::Os));
                if *windows {
                    x.edges.push(Edge::new(from, WINDOWS_MARK, EdgeKind::Os));
                }
            }
        }
        for (from, to) in imports {
            if from != to {
                x.edges.push(Edge::new(from, to, EdgeKind::Imports));
            }
        }
        Ok(x)
    }

    fn spawn_budget(&self, _: &[String]) -> usize {
        0
    }
}

/// The operating systems `id`'s tests run on (ADR 0117 §5 `select_os`): Linux always, and Windows and macOS when the node holds an OS
/// mark ([`marks`]). Marks reach a node from
///
/// - itself;
/// - the test files it runs (`tests` edges) and, for a test target, the crate or module it covers (`depends` edges);
/// - every module that calls it over `imports`, transitively (the host code's callee runs where its caller does).
///
/// `ignoring` leaves some marks out (a measurement; [`Signal::mark`] names them).
pub fn select_os(model: &Model, id: &str, ignoring: &[String]) -> BTreeSet<Os> {
    let mut out = BTreeSet::from([Os::Linux]);
    if !marks(model, id, ignoring).is_empty() {
        out.insert(Os::Windows);
        out.insert(Os::MacOs);
    }
    out
}

/// The mark nodes that reach `id` by the rules of [`select_os`], with the node that holds each (`os:cfg` from `crate:..`).
pub fn marks(model: &Model, id: &str, ignoring: &[String]) -> BTreeSet<(String, String)> {
    let mut sources: BTreeSet<String> = BTreeSet::new();
    let mut todo = vec![id.to_string()];
    let is_target = model
        .node(id)
        .is_some_and(|n| n.kind == NodeKind::TestTarget);
    if is_target {
        todo.extend(
            model
                .targets(id, EdgeKind::Depends)
                .into_iter()
                .map(str::to_string),
        );
    }
    todo.extend(
        model
            .targets(id, EdgeKind::Tests)
            .into_iter()
            .map(str::to_string),
    );
    // The callers, transitively: a mark on the importer reaches the importee.
    while let Some(at) = todo.pop() {
        if !sources.insert(at.clone()) {
            continue;
        }
        todo.extend(model.importers(&at).into_iter().map(str::to_string));
    }
    let mut out = BTreeSet::new();
    for s in &sources {
        for m in model.targets(s, EdgeKind::Os) {
            if !ignoring.iter().any(|i| i == m) {
                out.insert((m.to_string(), s.clone()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
