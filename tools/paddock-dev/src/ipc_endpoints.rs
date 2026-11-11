//! C102, the endpoint register check (ADR 0072 §5): every place in non-test code that listens or accepts is in
//! `docs/ipc-endpoints.toml`, either as an `[[endpoint]]` that names its ending test or as an `[[out_of_scope]]` entry with a
//! reason and an owner; the ratchet `docs/ipc-endpoint-ratchet.toml` (what fails today, per instance, with the tag it blocks)
//! is well formed and refers to registered endpoints.
//!
//! The scan is a `syn` pass, not a grep: it parses every Rust file outside `tests/`, `benches/` and `examples/`, skips
//! `#[cfg(test)]` items, `#[test]` functions and the files of `#[cfg(test)] mod x;`, resolves each file's `use` paths (so an
//! aliased import is still seen) and finds every listen primitive in [`PRIMITIVES`]. Its own positive control is a fixture
//! tree with one production-style site per primitive and the skipped forms (ER-33): a test asserts that the scan reports every
//! fixture site by its call, so a call the parser stops seeing turns that test red.
//!
//! The tag refusal: [`blocked_by`] answers which ratchet entries block a tag (`dyno-v1.2.0` against `dyno-v*`). The release
//! path calls `./dev check ipc-endpoints --tag <tag>` on the tag path of `dyno-release.yml` and `pitcrew-release.yml` (ADR 0072,
//! "M0 plan" item 2 and D8). C58 (ADR 0034's copies check) does not exist on this branch, so the call is declared in those
//! workflows directly; it moves into C58's tag check when that lands.
//!
//! The scan walks the files git knows (`git ls-files`, tracked and not ignored), not the file system, and fails closed: a
//! macro whose tokens do not parse as expressions but name a listen identifier is reported as a site of its own kind.

use paddock_cli::{CliError, Report};
use proc_macro2::Span;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::visit::Visit;

/// The ids of the listen primitives the scan looks for, in the order of the ADR.
pub const PRIMITIVES: [&str; 21] = [
    "std::net::TcpListener::bind",
    "std::os::unix::net::UnixListener::bind",
    "tokio::net::TcpListener::bind",
    "tokio::net::UnixListener::bind",
    "tokio::net::TcpListener::from_std",
    "tokio::net::UnixListener::from_std",
    "tokio::net::windows::named_pipe::ServerOptions::create",
    "tokio::net::windows::named_pipe::ServerOptions::create_with_security_attributes_raw",
    "socket2::Socket::listen",
    "CreateNamedPipeW",
    "paddock_ipc::endpoint::Listener::bind*",
    "paddock_ipc::Host::listen*",
    "paddock_ipc::link::LinkServer::start",
    "paddock_mcp::serve_loopback",
    "paddock_ipc::Accepted::serve_without_handshake",
    "std::os::unix::net::UnixListener::bind_addr",
    "std::net::TcpListener::from",
    "std::os::unix::net::UnixListener::from",
    "tokio::net::TcpSocket::listen",
    "tokio::net::UnixSocket::listen",
    "macro-naming-a-listen-identifier",
];

/// The id `name` in [`PRIMITIVES`] (a name that is not there is a bug of this file, found by every test at once).
fn prim(name: &str) -> &'static str {
    PRIMITIVES.iter().copied().find(|p| *p == name).unwrap_or("unknown-primitive")
}

/// Identifiers that make a macro suspect when its tokens cannot be read as expressions.
const LISTEN_IDENTS: [&str; 17] = [
    "bind",
    "bind_addr",
    "bind_in",
    "bind_fixed",
    "bind_system",
    "from_std",
    "listen",
    "listen_in",
    "listen_at",
    "serve_loopback",
    "serve_without_handshake",
    "CreateNamedPipeW",
    "create_with_security_attributes_raw",
    "LinkServer",
    "TcpSocket",
    "UnixSocket",
    "named_pipe",
];

/// The scenario names of the ratchet file (kept in step with `paddock_ipc::testing::endings` by a test that reads its source).
pub const SCENARIOS: [&str; 13] = [
    "wrong-token",
    "wrong-version",
    "wrong-account",
    "busy",
    "silent-client",
    "malformed-first-frame",
    "client-keeps-writing",
    "owner-close-with-calls-in-flight",
    "owner-close-while-peer-not-reading",
    "stall",
    "listener-stop-with-client-in-backlog",
    "stop-with-open-connection",
    "mixed-version-peer-ignores-fail0",
];
/// The fault kinds of the ratchet file (see [`SCENARIOS`]).
pub const FAULTS: [&str; 11] = [
    "no-reason",
    "wrong-reason",
    "reason-not-last",
    "reset",
    "typed-transport",
    "cancelled-after-owner-close",
    "unanswered",
    "hang",
    "no-end-line",
    "dropped-without-verdict",
    "drain-not-eof",
];
/// Faults that are a logging gap (WP-7), not an instance of the ADR's table: they block no tag.
const LOGGING_FAULTS: [&str; 3] = ["no-end-line", "dropped-without-verdict", "drain-not-eof"];

/// Where the register and the ratchet live, relative to the repository root.
pub const REGISTER: &str = "docs/ipc-endpoints.toml";
/// See [`REGISTER`].
pub const RATCHET: &str = "docs/ipc-endpoint-ratchet.toml";

/// One listen primitive found in non-test code.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Site {
    /// The file, relative to the scanned root, with `/` separators.
    pub file: String,
    /// The line of the call.
    pub line: usize,
    /// The primitive's id from [`PRIMITIVES`].
    pub call: String,
}

#[derive(Default)]
struct Uses {
    /// alias -> the full path it stands for.
    map: HashMap<String, Vec<String>>,
    globs: Vec<Vec<String>>,
    /// Every identifier that appears in any `use` of the file.
    mentions: HashSet<String>,
}

fn flatten(tree: &syn::UseTree, prefix: &mut Vec<String>, uses: &mut Uses) {
    match tree {
        syn::UseTree::Path(p) => {
            prefix.push(p.ident.to_string());
            uses.mentions.insert(p.ident.to_string());
            flatten(&p.tree, prefix, uses);
            prefix.pop();
        }
        syn::UseTree::Name(n) => {
            let name = n.ident.to_string();
            uses.mentions.insert(name.clone());
            if name == "self" {
                if let Some(last) = prefix.last().cloned() {
                    uses.map.insert(last, prefix.clone());
                }
            } else {
                let mut full = prefix.clone();
                full.push(name.clone());
                uses.map.insert(name, full);
            }
        }
        syn::UseTree::Rename(r) => {
            uses.mentions.insert(r.ident.to_string());
            let mut full = prefix.clone();
            if r.ident != "self" {
                full.push(r.ident.to_string());
            }
            uses.map.insert(r.rename.to_string(), full);
        }
        syn::UseTree::Glob(_) => uses.globs.push(prefix.clone()),
        syn::UseTree::Group(g) => {
            for item in &g.items {
                flatten(item, prefix, uses);
            }
        }
    }
}

/// Whether `meta` (the inside of a `cfg(...)`) can only be true in a test build.
fn cfg_test_only(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(p) => p.is_ident("test"),
        syn::Meta::List(l) => {
            let name = l.path.get_ident().map(ToString::to_string).unwrap_or_default();
            let inner = l
                .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
                .ok();
            match (name.as_str(), inner) {
                ("all", Some(list)) => list.iter().any(cfg_test_only),
                ("any", Some(list)) => !list.is_empty() && list.iter().all(cfg_test_only),
                _ => false,
            }
        }
        syn::Meta::NameValue(_) => false,
    }
}

/// Whether an item with these attributes is test code: `#[test]`, `#[tokio::test]`, `#[cfg(test)]`, `#[cfg(all(test, ..))]`.
fn test_attrs(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        let last = a.path().segments.last().map(|s| s.ident.to_string());
        if last.as_deref() == Some("test") {
            return true;
        }
        if a.path().is_ident("cfg")
            && let syn::Meta::List(l) = &a.meta
        {
            return l.parse_args::<syn::Meta>().is_ok_and(|m| cfg_test_only(&m));
        }
        false
    })
}

struct Walker<'a> {
    uses: &'a mut Uses,
    collect_uses: bool,
    sites: Vec<(usize, &'static str)>,
    /// Names of `#[cfg(test)] mod x;` declarations (their files are test code).
    test_mods: Vec<String>,
}

impl Walker<'_> {
    fn resolve(&self, path: &syn::Path) -> Vec<String> {
        let segs: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        let Some(first) = segs.first() else { return segs };
        if let Some(full) = self.uses.map.get(first) {
            let mut out = full.clone();
            out.extend(segs[1..].iter().cloned());
            return out;
        }
        if first.chars().next().is_some_and(char::is_uppercase) {
            // A type brought in by a glob import: try each glob's prefix, the first that makes a primitive wins.
            for g in &self.uses.globs {
                let mut out = g.clone();
                out.extend(segs.iter().cloned());
                if primitive_of_path(&out).is_some() {
                    return out;
                }
            }
        }
        segs
    }

    fn add(&mut self, span: Span, call: &'static str) {
        self.sites.push((span.start().line, call));
    }
}

/// The primitive a resolved call path is, if any.
fn primitive_of_path(full: &[String]) -> Option<&'static str> {
    let n = full.len();
    let last = full.last()?.as_str();
    let ty = if n >= 2 { full[n - 2].as_str() } else { "" };
    let root = full.first()?.as_str();
    let own = matches!(root, "paddock_ipc" | "crate" | "self" | "super");
    Some(match (ty, last) {
        ("TcpListener", "bind") if root == "std" => prim("std::net::TcpListener::bind"),
        ("UnixListener", "bind") if root == "std" => prim("std::os::unix::net::UnixListener::bind"),
        ("UnixListener", "bind_addr") if root == "std" => prim("std::os::unix::net::UnixListener::bind_addr"),
        ("TcpListener", "from") if root == "std" => prim("std::net::TcpListener::from"),
        ("UnixListener", "from") if root == "std" => prim("std::os::unix::net::UnixListener::from"),
        ("TcpListener", "bind") if root == "tokio" => prim("tokio::net::TcpListener::bind"),
        ("UnixListener", "bind") if root == "tokio" => prim("tokio::net::UnixListener::bind"),
        ("TcpListener", "from_std") if root == "tokio" => prim("tokio::net::TcpListener::from_std"),
        ("UnixListener", "from_std") if root == "tokio" => prim("tokio::net::UnixListener::from_std"),
        ("Socket", "listen") if root == "socket2" => prim("socket2::Socket::listen"),
        ("TcpSocket", "listen") if root == "tokio" => prim("tokio::net::TcpSocket::listen"),
        ("UnixSocket", "listen") if root == "tokio" => prim("tokio::net::UnixSocket::listen"),
        (_, "CreateNamedPipeW") => prim("CreateNamedPipeW"),
        ("Listener", "bind" | "bind_in" | "bind_fixed" | "bind_system") if own => prim("paddock_ipc::endpoint::Listener::bind*"),
        ("Host", "listen" | "listen_in" | "listen_at") if own => prim("paddock_ipc::Host::listen*"),
        ("LinkServer", "start") if own => prim("paddock_ipc::link::LinkServer::start"),
        // Qualified, or called by its bare name inside its own crate.
        (_, "serve_loopback") if n == 1 || matches!(root, "paddock_mcp" | "crate" | "self" | "super") => {
            prim("paddock_mcp::serve_loopback")
        }
        _ => return None,
    })
}

impl<'ast> Visit<'ast> for Walker<'_> {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        if test_attrs(&i.attrs) {
            if i.content.is_none() {
                self.test_mods.push(i.ident.to_string());
            }
            return;
        }
        syn::visit::visit_item_mod(self, i);
    }

    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        if !test_attrs(&i.attrs) {
            syn::visit::visit_item_fn(self, i);
        }
    }

    fn visit_impl_item_fn(&mut self, i: &'ast syn::ImplItemFn) {
        if !test_attrs(&i.attrs) {
            syn::visit::visit_impl_item_fn(self, i);
        }
    }

    fn visit_trait_item_fn(&mut self, i: &'ast syn::TraitItemFn) {
        if !test_attrs(&i.attrs) {
            syn::visit::visit_trait_item_fn(self, i);
        }
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        if !test_attrs(&i.attrs) {
            syn::visit::visit_item_impl(self, i);
        }
    }

    fn visit_item_const(&mut self, i: &'ast syn::ItemConst) {
        if !test_attrs(&i.attrs) {
            syn::visit::visit_item_const(self, i);
        }
    }

    fn visit_item_static(&mut self, i: &'ast syn::ItemStatic) {
        if !test_attrs(&i.attrs) {
            syn::visit::visit_item_static(self, i);
        }
    }

    fn visit_local(&mut self, i: &'ast syn::Local) {
        if !test_attrs(&i.attrs) {
            syn::visit::visit_local(self, i);
        }
    }

    fn visit_item_use(&mut self, i: &'ast syn::ItemUse) {
        if self.collect_uses && !test_attrs(&i.attrs) {
            flatten(&i.tree, &mut Vec::new(), self.uses);
        }
    }

    fn visit_path(&mut self, p: &'ast syn::Path) {
        if self.collect_uses {
            for seg in &p.segments {
                self.uses.mentions.insert(seg.ident.to_string());
            }
        }
        syn::visit::visit_path(self, p);
    }

    /// A path in expression position is a site whether it is called (`TcpListener::bind(a)`) or passed as a value
    /// (`.map(TcpListener::bind)`).
    fn visit_expr_path(&mut self, p: &'ast syn::ExprPath) {
        if !self.collect_uses
            && let Some(call) = primitive_of_path(&self.resolve(&p.path))
            && let Some(seg) = p.path.segments.last()
        {
            self.add(seg.ident.span(), call);
        }
        syn::visit::visit_expr_path(self, p);
    }

    /// A macro's arguments are read as comma-separated expressions where they parse (`vec![..]`, `join!(..)`); where they do
    /// not (`select!` arms, `vec![x; n]`), a listen identifier among the tokens is a site of its own kind: fail closed.
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        use syn::parse::Parser;
        if let Some(seg) = m.path.segments.last() {
            self.uses.mentions.insert(seg.ident.to_string());
        }
        let parsed = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated.parse2(m.tokens.clone());
        match parsed {
            Ok(exprs) => {
                for e in &exprs {
                    self.visit_expr(e);
                }
            }
            Err(_) if !self.collect_uses => {
                if let Some(span) = first_listen_ident(m.tokens.clone()) {
                    self.add(span, prim("macro-naming-a-listen-identifier"));
                }
            }
            Err(_) => {}
        }
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        if !self.collect_uses {
            let name = m.method.to_string();
            let span = m.method.span();
            match name.as_str() {
                "serve_without_handshake" => self.add(span, prim("paddock_ipc::Accepted::serve_without_handshake")),
                "listen" if m.args.len() == 1 && self.uses.mentions.contains("socket2") => self.add(span, prim("socket2::Socket::listen")),
                "listen" if m.args.len() == 1 && self.uses.mentions.contains("TcpSocket") => {
                    self.add(span, prim("tokio::net::TcpSocket::listen"))
                }
                "listen" if m.args.len() == 1 && self.uses.mentions.contains("UnixSocket") => {
                    self.add(span, prim("tokio::net::UnixSocket::listen"))
                }
                "create" if m.args.len() == 1 && self.uses.mentions.contains("named_pipe") => {
                    self.add(span, prim("tokio::net::windows::named_pipe::ServerOptions::create"));
                }
                "create_with_security_attributes_raw" if self.uses.mentions.contains("named_pipe") => {
                    self.add(
                        span,
                        prim("tokio::net::windows::named_pipe::ServerOptions::create_with_security_attributes_raw"),
                    );
                }
                _ => {}
            }
        }
        syn::visit::visit_expr_method_call(self, m);
    }
}

/// The span of the first token in `tokens` (searching groups) that is a listen identifier.
fn first_listen_ident(tokens: proc_macro2::TokenStream) -> Option<Span> {
    for t in tokens {
        match t {
            proc_macro2::TokenTree::Ident(i) if LISTEN_IDENTS.iter().any(|n| i == n) => return Some(i.span()),
            proc_macro2::TokenTree::Group(g) => {
                if let Some(s) = first_listen_ident(g.stream()) {
                    return Some(s);
                }
            }
            _ => {}
        }
    }
    None
}

/// A file's sites, `(line, primitive)`, and the module names its `#[cfg(test)] mod x;` lines declare.
type FileScan = (Vec<(usize, &'static str)>, Vec<String>);

/// What one file holds: its sites and the files its `#[cfg(test)] mod x;` lines declare.
fn scan_source(source: &str) -> Result<FileScan, String> {
    let file = syn::parse_file(source).map_err(|e| e.to_string())?;
    let mut uses = Uses::default();
    let mut first = Walker {
        uses: &mut uses,
        collect_uses: true,
        sites: Vec::new(),
        test_mods: Vec::new(),
    };
    first.visit_file(&file);
    let mut second = Walker {
        uses: &mut uses,
        collect_uses: false,
        sites: Vec::new(),
        test_mods: Vec::new(),
    };
    second.visit_file(&file);
    let mut sites = second.sites;
    sites.sort_unstable();
    sites.dedup();
    Ok((sites, second.test_mods))
}

/// Folders that hold no product code (build output, tests, benchmarks, examples, tool state).
fn skipped_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | "target" | "node_modules" | "dist" | ".claude" | ".serena" | "tests" | "benches" | "examples" | "vendor"
    )
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        let ty = e.file_type()?;
        let path = e.path();
        if ty.is_dir() {
            if !skipped_dir(&name) {
                walk(&path, out)?;
            }
        } else if ty.is_file() && name.ends_with(".rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// The Rust files to scan: those git knows under `root` (tracked, and untracked but not ignored, so a file about to be
/// committed is checked too), without the folders of [`skipped_dir`]; where `root` is not a git checkout (a fixture tree, an
/// archive of `HEAD`) the files on disk.
fn source_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    if !root.join(".git").exists() {
        let mut files = Vec::new();
        walk(root, &mut files).map_err(|e| format!("{}: {e}", root.display()))?;
        return Ok(files);
    }
    let listed = crate::input_hash::Exec::run(
        &crate::input_hash::Real,
        "git",
        &["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "*.rs"],
        root,
        None,
    );
    // Where git cannot answer (a worktree mounted into a container without its git folder, git not installed) the file system
    // walk is used: it sees every file git would and more, so it can only add sites, never hide one.
    // Names are NUL-separated (`-z`), so a path with spaces, quotes or non-ASCII characters is listed verbatim. A name that is not
    // valid UTF-8 cannot be read back from the lossy text: that too takes the file system walk, never a silent drop.
    let Ok(listed) = listed.and_then(|l| {
        if l.contains('\u{fffd}') {
            Err(paddock_cli::CliError::failure("git listed a path that is not valid UTF-8"))
        } else {
            Ok(l)
        }
    }) else {
        let mut files = Vec::new();
        walk(root, &mut files).map_err(|e| format!("{}: {e}", root.display()))?;
        return Ok(files);
    };
    let mut files: Vec<PathBuf> = listed
        .split('\0')
        .filter(|l| !l.is_empty())
        .filter(|l| {
            !Path::new(l)
                .parent()
                .is_some_and(|d| d.components().any(|c| skipped_dir(&c.as_os_str().to_string_lossy())))
        })
        .map(|l| root.join(l))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    files.dedup();
    Ok(files)
}

/// The listen primitives in the non-test Rust files under `root`, sorted. Files that do not parse are returned in `Err` with
/// the reason: a file the parser cannot read could hide a site, so the check fails on it.
///
/// # Errors
/// The folder cannot be read, or a file does not parse (every such file is named).
pub fn scan(root: &Path) -> Result<Vec<Site>, String> {
    scan_with(root, &|| {})
}

/// [`scan`] calling `tick` after every source file it has read and parsed, so that a run over a slow disk can show it is
/// working (ADR 0071).
pub fn scan_with(root: &Path, tick: &dyn Fn()) -> Result<Vec<Site>, String> {
    let files = source_files(root)?;
    let mut per_file: BTreeMap<PathBuf, FileScan> = BTreeMap::new();
    let mut unparsed = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
        tick();
        match scan_source(&text) {
            Ok(found) => {
                per_file.insert(f.clone(), found);
            }
            Err(e) => unparsed.push(format!("{}: {e}", rel(root, f))),
        }
    }
    if !unparsed.is_empty() {
        return Err(format!(
            "these files do not parse, so a listen site in them could go unseen:\n  {}",
            unparsed.join("\n  ")
        ));
    }
    // Files of `#[cfg(test)] mod x;` are test code, and so is everything below them.
    let mut skip_files: HashSet<PathBuf> = HashSet::new();
    let mut skip_dirs: Vec<PathBuf> = Vec::new();
    for (file, (_, mods)) in &per_file {
        let Some(parent) = file.parent() else { continue };
        let stem = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let base = if matches!(stem.as_str(), "mod" | "lib" | "main") {
            parent.to_path_buf()
        } else {
            parent.join(&stem)
        };
        for m in mods {
            skip_files.insert(base.join(format!("{m}.rs")));
            skip_files.insert(base.join(m).join("mod.rs"));
            skip_dirs.push(base.join(m));
        }
    }
    let mut sites = Vec::new();
    for (file, (found, _)) in &per_file {
        if skip_files.contains(file) || skip_dirs.iter().any(|d| file.starts_with(d)) {
            continue;
        }
        let name = rel(root, file);
        sites.extend(found.iter().map(|(line, call)| Site {
            file: name.clone(),
            line: *line,
            call: (*call).to_string(),
        }));
    }
    sites.sort();
    Ok(sites)
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// One `[[endpoint]]` of the register.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The id the ending test and the ratchet use.
    pub id: String,
    /// The file with the listen sites.
    pub file: String,
    /// The primitives found in that file.
    pub calls: Vec<String>,
    /// How many sites the file has.
    pub sites: usize,
    /// `path::function` of the ending test.
    pub ending_test: String,
}

/// One `[[out_of_scope]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutOfScope {
    /// A file, or a folder as `dir/**`.
    pub path: String,
    /// Why the site is not an endpoint of the family.
    pub reason: String,
    /// Who answers for it.
    pub owner: String,
    /// The primitives found in the matching files (all of them together).
    pub calls: Vec<String>,
    /// How many sites the matching files have in all.
    pub sites: usize,
}

/// One `[[not_applicable]]` entry: a scenario that does not apply to an endpoint, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotApplicable {
    /// The endpoint id.
    pub endpoint: String,
    /// The scenario name.
    pub scenario: String,
    /// Why it does not apply.
    pub reason: String,
    /// The operating systems it holds on (empty: all).
    pub os: String,
}

/// The register file.
#[derive(Debug, Clone, Default)]
pub struct Register {
    /// The endpoints.
    pub endpoints: Vec<Endpoint>,
    /// The out-of-scope entries.
    pub out_of_scope: Vec<OutOfScope>,
    /// The scenarios that do not apply, per endpoint.
    pub not_applicable: Vec<NotApplicable>,
}

fn text_of(t: &toml::Table, key: &str) -> String {
    t.get(key).and_then(toml::Value::as_str).unwrap_or_default().to_string()
}

fn strings_of(t: &toml::Table, key: &str) -> Vec<String> {
    t.get(key)
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn tables<'a>(doc: &'a toml::Table, key: &str) -> Vec<&'a toml::Table> {
    doc.get(key)
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_table).collect())
        .unwrap_or_default()
}

/// Parses the register file.
///
/// # Errors
/// The text is not TOML.
pub fn parse_register(text: &str) -> Result<Register, String> {
    let doc: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    let endpoints = tables(&doc, "endpoint")
        .into_iter()
        .map(|t| Endpoint {
            id: text_of(t, "id"),
            file: text_of(t, "file"),
            calls: t
                .get("calls")
                .and_then(toml::Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            sites: t
                .get("sites")
                .and_then(toml::Value::as_integer)
                .map_or(0, |n| usize::try_from(n).unwrap_or(0)),
            ending_test: text_of(t, "ending_test"),
        })
        .collect();
    let out_of_scope = tables(&doc, "out_of_scope")
        .into_iter()
        .map(|t| OutOfScope {
            path: text_of(t, "path"),
            reason: text_of(t, "reason"),
            owner: text_of(t, "owner"),
            calls: strings_of(t, "calls"),
            sites: t
                .get("sites")
                .and_then(toml::Value::as_integer)
                .map_or(0, |n| usize::try_from(n).unwrap_or(0)),
        })
        .collect();
    let not_applicable = tables(&doc, "not_applicable")
        .into_iter()
        .map(|t| NotApplicable {
            endpoint: text_of(t, "endpoint"),
            scenario: text_of(t, "scenario"),
            reason: text_of(t, "reason"),
            os: text_of(t, "os"),
        })
        .collect();
    Ok(Register {
        endpoints,
        out_of_scope,
        not_applicable,
    })
}

/// One ratchet entry (`docs/ipc-endpoint-ratchet.toml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RatchetEntry {
    /// The instance of the ADR's table.
    pub instance: String,
    /// The endpoint id.
    pub endpoint: String,
    /// The scenario name or `*`.
    pub scenario: String,
    /// The fault kind.
    pub fault: String,
    /// The tag pattern this entry blocks, or `none` for a logging gap.
    pub blocks_tag: String,
    /// The operating systems it applies to (empty: all).
    pub os: String,
}

/// Parses the ratchet file.
///
/// # Errors
/// The text is not TOML.
pub fn parse_ratchet(text: &str) -> Result<Vec<RatchetEntry>, String> {
    let doc: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    Ok(tables(&doc, "ratchet")
        .into_iter()
        .map(|t| RatchetEntry {
            instance: text_of(t, "instance"),
            endpoint: text_of(t, "endpoint"),
            scenario: text_of(t, "scenario"),
            fault: text_of(t, "fault"),
            blocks_tag: text_of(t, "blocks_tag"),
            os: text_of(t, "os"),
        })
        .collect())
}

/// Whether `tag` matches the glob `pattern` (`*` matches any run of characters; nothing else is special).
pub fn tag_matches(pattern: &str, tag: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == tag;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !tag.starts_with(first) || !tag.ends_with(last) || tag.len() < first.len() + last.len() {
        return false;
    }
    let mut at = first.len();
    let end = tag.len() - last.len();
    for mid in &parts[1..parts.len() - 1] {
        match tag[at..end].find(mid) {
            Some(i) => at += i + mid.len(),
            None => return false,
        }
    }
    true
}

/// The ratchet entries that block `tag` (the tag-refusal hook of D8): an entry whose `blocks_tag` pattern matches.
pub fn blocked_by<'a>(ratchet: &'a [RatchetEntry], tag: &str) -> Vec<&'a RatchetEntry> {
    ratchet
        .iter()
        .filter(|e| e.blocks_tag != "none" && tag_matches(&e.blocks_tag, tag))
        .collect()
}

fn covers(pattern: &str, file: &str) -> bool {
    match pattern.strip_suffix("/**") {
        Some(dir) => file.starts_with(&format!("{dir}/")),
        None => pattern == file,
    }
}

/// Compares the sites with the register and the ratchet: every finding, each with its next step.
pub fn findings(root: &Path, sites: &[Site], register: &Register, ratchet: &[RatchetEntry]) -> Vec<String> {
    let mut out = Vec::new();
    let mut by_file: BTreeMap<&str, Vec<&Site>> = BTreeMap::new();
    for s in sites {
        by_file.entry(s.file.as_str()).or_default().push(s);
    }
    let mut ids = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut tests_of: BTreeMap<String, String> = BTreeMap::new();
    for e in &register.endpoints {
        if e.id.is_empty() || e.file.is_empty() || e.ending_test.is_empty() {
            out.push(format!(
                "{REGISTER}: an [[endpoint]] needs id, file, calls, sites and ending_test (got id `{}`).",
                e.id
            ));
            continue;
        }
        ids.insert(e.id.clone());
        if !seen.insert((e.id.clone(), e.file.clone())) {
            out.push(format!("{REGISTER}: endpoint `{}` lists {} twice.", e.id, e.file));
        }
        if let Some(other) = tests_of.insert(e.id.clone(), e.ending_test.clone())
            && other != e.ending_test
        {
            out.push(format!(
                "{REGISTER}: endpoint `{}` names two ending tests ({other} and {}); one id, one test.",
                e.id, e.ending_test
            ));
        }
        match by_file.get(e.file.as_str()) {
            None => out.push(format!(
                "{REGISTER}: endpoint `{}` names {}, which has no listen site in non-test code. Remove the entry or correct the file.",
                e.id, e.file
            )),
            Some(found) => {
                let calls: BTreeSet<&str> = found.iter().map(|s| s.call.as_str()).collect();
                let listed: BTreeSet<&str> = e.calls.iter().map(String::as_str).collect();
                if calls != listed || found.len() != e.sites {
                    out.push(format!(
                        "{REGISTER}: endpoint `{}` ({}) lists calls {:?} and {} site(s); the file has {:?} and {}. A new listen call in a registered file needs its own review: update the entry and its ending test.",
                        e.id, e.file, listed, e.sites, calls, found.len()
                    ));
                }
            }
        }
        let (test_file, test_fn) = e.ending_test.split_once("::").unwrap_or((e.ending_test.as_str(), ""));
        match std::fs::read_to_string(root.join(test_file)) {
            Err(_) => out.push(format!(
                "{REGISTER}: the ending test of `{}` is in {test_file}, which does not exist.",
                e.id
            )),
            Ok(text) => {
                let has_fn = !test_fn.is_empty() && text.contains(&format!("fn {test_fn}("));
                if !has_fn {
                    out.push(format!(
                        "{REGISTER}: {test_file} has no `fn {test_fn}` (the ending test of `{}`).",
                        e.id
                    ));
                }
                if !text.contains(&format!("\"{}\"", e.id)) {
                    out.push(format!(
                        "{REGISTER}: {test_file} never names the endpoint id \"{}\", so it cannot be running that endpoint's scenarios.",
                        e.id
                    ));
                }
            }
        }
    }
    for o in &register.out_of_scope {
        if o.path.is_empty() || o.reason.trim().is_empty() || o.owner.trim().is_empty() {
            out.push(format!(
                "{REGISTER}: an [[out_of_scope]] entry needs path, reason and owner (got path `{}`).",
                o.path
            ));
        }
    }
    for (file, found) in &by_file {
        let registered = register.endpoints.iter().any(|e| e.file == *file);
        let waived = register.out_of_scope.iter().any(|o| covers(&o.path, file));
        if !registered && !waived {
            let first = found[0];
            out.push(format!(
                "{file}:{}: `{}` listens here and the file is in neither section of {REGISTER}. Register it as an [[endpoint]] with an ending test (docs/ipc-endpoints.toml explains the fields), or as [[out_of_scope]] with a reason.",
                first.line, first.call
            ));
        }
    }
    for o in &register.out_of_scope {
        if o.path.is_empty() {
            continue;
        }
        let matching: Vec<&Site> = by_file
            .iter()
            .filter(|(f, _)| covers(&o.path, f))
            .flat_map(|(_, v)| v.iter().copied())
            .collect();
        if matching.is_empty() {
            out.push(format!(
                "{REGISTER}: out_of_scope `{}` matches no listen site any more. Remove the entry.",
                o.path
            ));
            continue;
        }
        let calls: BTreeSet<&str> = matching.iter().map(|s| s.call.as_str()).collect();
        let listed: BTreeSet<&str> = o.calls.iter().map(String::as_str).collect();
        if calls != listed || matching.len() != o.sites {
            out.push(format!(
                "{REGISTER}: out_of_scope `{}` lists calls {:?} and {} site(s); the files have {:?} and {}. Look at the new site, then update the entry (a changed site in a waived file is still a listener).",
                o.path, listed, o.sites, calls, matching.len()
            ));
        }
    }
    for n in &register.not_applicable {
        let at = format!("{REGISTER}: not_applicable {} / {}", n.endpoint, n.scenario);
        if !ids.contains(&n.endpoint) {
            out.push(format!("{at}: the endpoint is not registered."));
        }
        if !SCENARIOS.contains(&n.scenario.as_str()) {
            out.push(format!("{at}: unknown scenario."));
        }
        if n.reason.trim().is_empty() {
            out.push(format!("{at}: a scenario that does not apply needs its reason."));
        }
        let oses = |text: &str| -> Vec<String> { text.split(',').map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect() };
        let overlap = |a: &str, b: &str| {
            let (a, b) = (oses(a), oses(b));
            a.is_empty() || b.is_empty() || a.iter().any(|x| b.contains(x))
        };
        if ratchet
            .iter()
            .any(|r| r.endpoint == n.endpoint && r.scenario == n.scenario && overlap(&r.os, &n.os))
        {
            out.push(format!("{at}: the ratchet lists a fault for a scenario that does not apply."));
        }
    }
    for r in ratchet {
        let at = format!("{RATCHET}: entry {} / {} / {}", r.instance, r.endpoint, r.scenario);
        if !(r.instance.starts_with('I') && r.instance[1..].parse::<u32>().is_ok()) && r.instance != "WP-7" {
            out.push(format!(
                "{at}: `instance` must be one of the ADR's I1 to I27 (or WP-7 for a logging gap)."
            ));
        }
        if !ids.contains(&r.endpoint) {
            out.push(format!("{at}: the endpoint is not in {REGISTER}."));
        }
        if r.scenario != "*" && !SCENARIOS.contains(&r.scenario.as_str()) {
            out.push(format!("{at}: unknown scenario."));
        }
        if !FAULTS.contains(&r.fault.as_str()) {
            out.push(format!("{at}: unknown fault `{}`.", r.fault));
        }
        let logging = LOGGING_FAULTS.contains(&r.fault.as_str());
        if r.blocks_tag.is_empty() || (r.blocks_tag == "none" && !logging) {
            out.push(format!(
                "{at}: `blocks_tag` names the tag pattern this entry blocks (`dyno-v*`, `pitcrew-v*`); only a logging gap may say `none`."
            ));
        }
        for o in r.os.split(',').map(str::trim).filter(|o| !o.is_empty()) {
            if !["linux", "macos", "windows"].contains(&o) {
                out.push(format!("{at}: unknown os `{o}`."));
            }
        }
    }
    out
}

/// What [`check`] found: the sites, the register, the ratchet and the findings.
pub type Checked = (Vec<Site>, Register, Vec<RatchetEntry>, Vec<String>);

/// The whole check on the repository at `root`.
///
/// # Errors
/// The register or the ratchet cannot be read, or a file does not parse.
pub fn check(root: &Path) -> Result<Checked, String> {
    check_with(root, &|| {})
}

/// [`check`] reporting progress to `tick` (see [`scan_with`]).
pub fn check_with(root: &Path, tick: &dyn Fn()) -> Result<Checked, String> {
    let read = |name: &str| std::fs::read_to_string(root.join(name)).map_err(|e| format!("{name}: {e}"));
    let register = parse_register(&read(REGISTER)?).map_err(|e| format!("{REGISTER}: {e}"))?;
    let ratchet = parse_ratchet(&read(RATCHET)?).map_err(|e| format!("{RATCHET}: {e}"))?;
    let sites = scan_with(root, tick)?;
    let found = findings(root, &sites, &register, &ratchet);
    Ok((sites, register, ratchet, found))
}

/// `./dev check ipc-endpoints [--tag <tag>]`.
pub fn command(root: Option<PathBuf>, tag: Option<String>) -> Result<Report, CliError> {
    let root = match root {
        Some(r) => r,
        None => {
            let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
            let out = crate::input_hash::Exec::run(&crate::input_hash::Real, "git", &["rev-parse", "--show-toplevel"], &cwd, None)?;
            PathBuf::from(out.trim())
        }
    };
    let (sites, register, ratchet, found) = check(&root).map_err(|e| {
        CliError::failure(format!("::error::C102 {e}")).with_hint("Fix the file named above and run `./dev check ipc-endpoints` again.")
    })?;
    if !found.is_empty() {
        let lines: Vec<String> = found.iter().map(|f| format!("::error::C102 {f}")).collect();
        return Err(CliError::failure(lines.join("\n"))
            .with_hint("Each line names its next step; run `./dev check ipc-endpoints` again when done."));
    }
    if let Some(tag) = tag {
        let blocking = blocked_by(&ratchet, &tag);
        if !blocking.is_empty() {
            let mut lines: Vec<String> = blocking
                .iter()
                .map(|e| {
                    format!(
                        "::error::C102 the tag {tag} is refused: ratchet entry {} ({} / {} / {}) blocks {}.",
                        e.instance, e.endpoint, e.scenario, e.fault, e.blocks_tag
                    )
                })
                .collect();
            lines.push("Fix those endings (docs/adr/0072) and remove the entries from docs/ipc-endpoint-ratchet.toml, or release from a line they do not block.".into());
            return Err(CliError::refused(lines.join("\n")));
        }
    }
    let text = format!(
        "C102: {} listen site(s) in non-test code, {} endpoint(s), {} out-of-scope entr{}, {} ratchet entr{} ({} block a tag).",
        sites.len(),
        register.endpoints.len(),
        register.out_of_scope.len(),
        if register.out_of_scope.len() == 1 { "y" } else { "ies" },
        ratchet.len(),
        if ratchet.len() == 1 { "y" } else { "ies" },
        ratchet.iter().filter(|e| e.blocks_tag != "none").count()
    );
    Ok(Report::new(
        json!({"sites": sites.len(), "endpoints": register.endpoints.len(), "outOfScope": register.out_of_scope.len(), "ratchet": ratchet.len()}),
        text,
    ))
}
