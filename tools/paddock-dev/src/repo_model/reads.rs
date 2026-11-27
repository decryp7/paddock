//! The Rust `reads` extractor (ADR 0117 §4; `syn`, already paddock-dev's dependency, so the shared crates' workspace hack and the
//! apps' lockfiles do not change): the data files and folders a Rust source reads at build or
//! test time, from its syntax tree, not its text.
//!
//! - `include_str!("x")`, `include_bytes!("x")` and `include!("x")` (a Rust source spliced in, review P1-2): relative to the source's folder; `include_str!(concat!(env!("CARGO_MANIFEST_DIR"),
//!   "/x"))` relative to the crate.
//! - `#[path = "x"]` on a module: relative to the source's folder.
//! - `env!("CARGO_MANIFEST_DIR")` joined with literals: `Path::new(env!(..)).join("tests").join("x")`, `concat!(env!(..), "/x")`,
//!   `format!("{}/x", env!(..))`: relative to the crate, a file or a **folder** (a folder read reads every file under it, review
//!   round 3 P1-A).
//! - `println!("cargo:rerun-if-changed=x")` in a build script: relative to the crate.
//!
//! - Anything else that reads the crate folder or walks the tree is a [`How::Blind`] read, never silence: `CARGO_MANIFEST_DIR` or
//!   `CARGO_MANIFEST_PATH` outside a fully resolved literal chain (in a variable, a helper, behind `.parent()`, a non-literal
//!   `join`, `env::var`), `.ancestors()`, `current_dir`, a `"ls-files"` argument, and a chain that normalises to the repository
//!   root or out of it. The crate is then unresolved (ADR 0116 §2).
//!
//! Macro bodies (`assert!`, `vec!`, a test macro) are parsed as expressions where they can be, so a read inside them counts. A
//! source that `syn` cannot parse falls back to the caller's text scan ([`Parsed::Fallback`]), measured beside this one.

use proc_macro2::TokenStream;
use proc_macro2::TokenTree;
use syn::punctuated::Punctuated;
use syn::visit::Visit;
use syn::{Expr, ExprLit, ExprMacro, ExprMethodCall, ItemMod, Lit, LitStr, Macro, Meta, PathSegment, Token};

/// How a path is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum How {
    /// `include_str!`/`include_bytes!`/`include!`: the file must exist (a compile error otherwise).
    Include,
    /// A module's `#[path]`: the file must exist.
    ModPath,
    /// A `CARGO_MANIFEST_DIR` join, `concat!` or `format!`.
    ManifestDir,
    /// A build script's `rerun-if-changed`.
    Rerun,
    /// A read the extractor cannot resolve (a manifest folder held in a variable or a helper, a join with a non-literal, `parent`,
    /// `ancestors`, `current_dir`, `git ls-files`, a path that normalises to the repository root or leaves it): [`Read::path`] says
    /// which. The crate is unresolved: the planner runs it in full and the test cache never reuses it (ADR 0116 §2).
    Blind,
}

impl How {
    /// Whether the path must exist at build time (`include*!`, `#[path]`): a missing one is an unresolved reference, never just
    /// no edge. A folder joined at run time or a `rerun-if-changed` path may be an output and may be absent.
    pub fn required(self) -> bool {
        matches!(self, How::Include | How::ModPath)
    }
}

/// One read: a repository path (`/`-separated, normalized; the caller checks it names a tracked file or folder).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Read {
    /// The path read.
    pub path: String,
    /// How.
    pub how: How,
}

/// The answer for one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// The reads `syn` found.
    Reads(Vec<Read>),
    /// `syn` could not parse the source (the caller uses its text scan), with why.
    Fallback(String),
}

/// A piece of a path expression: the crate folder or a literal.
#[derive(Debug, Clone)]
enum Piece {
    Manifest,
    Lit(String),
}

struct Finder<'a> {
    dir: &'a str,
    crate_dir: &'a str,
    out: Vec<Read>,
}

fn last_ident(m: &Macro) -> String {
    m.path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default()
}

fn lit_str(e: &Expr) -> Option<String> {
    match e {
        Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) => Some(s.value()),
        Expr::Group(g) => lit_str(&g.expr),
        Expr::Paren(p) => lit_str(&p.expr),
        _ => None,
    }
}

fn args(tokens: &TokenStream) -> Option<Vec<Expr>> {
    syn::parse::Parser::parse2(Punctuated::<Expr, Token![,]>::parse_terminated, tokens.clone())
        .ok()
        .map(|p| p.into_iter().collect())
}

/// `env!("CARGO_MANIFEST_DIR")`, also wrapped: `Path::new(env!(..))`, `PathBuf::from(env!(..))`, `&env!(..)`.
fn is_manifest(e: &Expr) -> bool {
    match e {
        Expr::Macro(ExprMacro { mac, .. }) if last_ident(mac) == "env" => {
            args(&mac.tokens).is_some_and(|a| a.first().and_then(lit_str).is_some_and(|s| s == "CARGO_MANIFEST_DIR"))
        }
        Expr::Group(g) => is_manifest(&g.expr),
        Expr::Paren(p) => is_manifest(&p.expr),
        Expr::Reference(r) => is_manifest(&r.expr),
        Expr::Call(c) => c.args.len() == 1 && c.args.first().is_some_and(is_manifest),
        _ => false,
    }
}

/// The pieces of a path expression rooted at the crate folder: `concat!(env!(..), "/x")`, `X.join("a").join("b")`,
/// `format!("{}/x", env!(..))`.
fn pieces(e: &Expr) -> Option<Vec<Piece>> {
    if is_manifest(e) {
        return Some(vec![Piece::Manifest]);
    }
    match e {
        Expr::MethodCall(mc) => call_pieces(mc),
        Expr::Reference(r) => pieces(&r.expr),
        Expr::Paren(p) => pieces(&p.expr),
        Expr::Group(g) => pieces(&g.expr),
        Expr::Call(c) if c.args.len() == 1 => pieces(c.args.first()?),
        Expr::Macro(ExprMacro { mac, .. }) => macro_pieces(mac),
        _ => None,
    }
}

/// The zero-argument methods that keep the path: the only ones a resolved chain passes through.
const IDENTITY: &[&str] = &["to_path_buf", "as_path", "to_owned", "clone", "as_ref"];

/// Methods that make a path point somewhere other than the chain they are called on.
const MOVES: &[&str] = &["parent", "canonicalize", "with_file_name", "pop", "push"];

/// `X.join("a")` (and `.to_path_buf()`, `.as_path()`: the same path).
fn call_pieces(mc: &ExprMethodCall) -> Option<Vec<Piece>> {
    let ExprMethodCall {
        receiver, method, args: a, ..
    } = mc;
    if method == "join" && a.len() == 1 {
        let mut p = pieces(receiver)?;
        p.push(Piece::Lit(format!("/{}", lit_str(a.first()?)?)));
        Some(p)
    } else if a.is_empty() && IDENTITY.iter().any(|m| method == m) {
        pieces(receiver)
    } else {
        // Any other method (`parent`, `file_name`, `canonicalize`, ...) changes the path: the chain is not a resolved literal one.
        None
    }
}

fn macro_pieces(mac: &Macro) -> Option<Vec<Piece>> {
    let name = last_ident(mac);
    // Only `concat!` and `format!` build a path: any other macro's tokens are not parsed here.
    if name != "concat" && name != "format" {
        return None;
    }
    args_pieces(&name, &args(&mac.tokens)?)
}

/// [`macro_pieces`] over arguments already parsed (a macro's tokens are parsed once).
fn args_pieces(name: &str, a: &[Expr]) -> Option<Vec<Piece>> {
    match name {
        "concat" => {
            let mut out = Vec::new();
            for e in a {
                if is_manifest(e) {
                    out.push(Piece::Manifest);
                } else {
                    out.push(Piece::Lit(lit_str(e)?));
                }
            }
            out.iter().any(|p| matches!(p, Piece::Manifest)).then_some(out)
        }
        "format" => {
            let fmt = lit_str(a.first()?)?;
            let rest = fmt.strip_prefix("{}")?;
            (a.len() == 2 && is_manifest(&a[1])).then(|| vec![Piece::Manifest, Piece::Lit(rest.to_string())])
        }
        _ => None,
    }
}

impl Finder<'_> {
    fn push(&mut self, r: Read) {
        if !self.out.contains(&r) {
            self.out.push(r);
        }
    }

    /// A read the extractor cannot resolve: the crate is unresolved (never silence).
    fn blind(&mut self, why: &str) {
        self.push(Read {
            path: why.to_string(),
            how: How::Blind,
        });
    }

    fn add(&mut self, base: &str, rel: &str, how: How) {
        if rel.is_empty() {
            return;
        }
        if rel.contains('{') {
            return self.blind(&format!("a path with a format placeholder `{rel}`"));
        }
        // A chain that normalises to the repository root (an empty path) or out of it is a read of everything: unresolved, never dropped.
        match paddock_repo_graph::join_normalized(base, rel.trim_start_matches('/')) {
            Some(p) if !p.is_empty() => self.push(Read { path: p, how }),
            _ => self.blind(&format!("`{base}` joined with `{rel}` reaches the repository root or leaves it")),
        }
    }

    /// The tokens of a macro that did not parse as expressions: a trigger anywhere in them is a blind read.
    fn scan_tokens(&mut self, tokens: TokenStream) {
        for t in tokens {
            match t {
                TokenTree::Group(g) => self.scan_tokens(g.stream()),
                TokenTree::Ident(i) if i == "ancestors" || i == "current_dir" => self.blind(&format!("`{i}` inside a macro")),
                TokenTree::Literal(l) => {
                    let text = l.to_string();
                    let text = text.trim_matches('"');
                    if let Some(why) = literal_trigger(text) {
                        self.blind(&format!("{why} inside a macro"));
                    }
                }
                _ => {}
            }
        }
    }

    fn rooted(&mut self, p: &[Piece], how: How) {
        if !matches!(p.first(), Some(Piece::Manifest)) {
            return;
        }
        let rel: String = p[1..]
            .iter()
            .map(|x| match x {
                Piece::Lit(s) => s.as_str(),
                Piece::Manifest => "",
            })
            .collect();
        let crate_dir = self.crate_dir.to_string();
        self.add(&crate_dir, &rel, how);
    }

    fn mac(&mut self, mac: &Macro) {
        let name = last_ident(mac);
        let Some(a) = args(&mac.tokens) else {
            return self.scan_tokens(mac.tokens.clone());
        };
        match name.as_str() {
            "include_str" | "include_bytes" | "include" => {
                let Some(first) = a.first() else { return };
                if let Some(s) = lit_str(first) {
                    let dir = self.dir.to_string();
                    self.add(&dir, &s, How::Include);
                } else if let Some(p) = pieces(first) {
                    self.rooted(&p, How::Include);
                }
            }
            "println" | "print" => {
                if let Some(s) = a.first().and_then(lit_str) {
                    for key in ["cargo:rerun-if-changed=", "cargo::rerun-if-changed="] {
                        if let Some(rel) = s.strip_prefix(key) {
                            let crate_dir = self.crate_dir.to_string();
                            self.add(&crate_dir, rel, How::Rerun);
                        }
                    }
                }
                a.iter().for_each(|e| self.visit_expr(e));
            }
            _ => {
                if let Some(p) = args_pieces(&name, &a) {
                    // A resolved `concat!`/`format!` chain: its manifest folder is consumed here.
                    self.rooted(&p, How::ManifestDir);
                } else {
                    // Any other macro's arguments are expressions too (`assert!`, `vec!`): a read inside one counts.
                    a.iter().for_each(|e| self.visit_expr(e));
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for Finder<'_> {
    fn visit_macro(&mut self, m: &'ast Macro) {
        self.mac(m);
    }

    fn visit_expr_method_call(&mut self, e: &'ast ExprMethodCall) {
        if e.method == "join" {
            if let Some(p) = call_pieces(e) {
                self.rooted(&p, How::ManifestDir);
                return;
            }
            // A literal chain from the crate folder continued with something that is not a literal: where it ends is unknown.
            if e.args.len() == 1 && pieces(&e.receiver).is_some() {
                self.blind("a join of the crate folder with a value that is not a literal");
                return;
            }
        }
        if e.method == "ancestors" {
            self.blind("`.ancestors()` walks up from a path");
        }
        // A method that moves a path (up a level, to its real location) on a literal chain: the result is not the chain.
        if MOVES.iter().any(|m| e.method == m) && pieces(&e.receiver).is_some() {
            self.blind(&format!("`.{}()` moves a path built from the crate folder", e.method));
            return;
        }
        syn::visit::visit_expr_method_call(self, e);
    }

    /// Reached only for a literal no resolved chain consumed.
    fn visit_lit_str(&mut self, l: &'ast LitStr) {
        if let Some(why) = literal_trigger(&l.value()) {
            self.blind(why);
        }
    }

    fn visit_path_segment(&mut self, s: &'ast PathSegment) {
        if s.ident == "current_dir" {
            self.blind("`current_dir` depends on where the test is started");
        }
        syn::visit::visit_path_segment(self, s);
    }

    fn visit_item_mod(&mut self, m: &'ast ItemMod) {
        for attr in &m.attrs {
            if let Meta::NameValue(nv) = &attr.meta
                && nv.path.is_ident("path")
                && let Some(s) = lit_str(&nv.value)
            {
                let dir = self.dir.to_string();
                self.add(&dir, &s, How::ModPath);
            }
        }
        syn::visit::visit_item_mod(self, m);
    }
}

/// The literals that name a root the extractor cannot follow.
fn literal_trigger(text: &str) -> Option<&'static str> {
    match text {
        "CARGO_MANIFEST_DIR" => Some("CARGO_MANIFEST_DIR outside a fully resolved literal join chain"),
        "CARGO_MANIFEST_PATH" => Some("CARGO_MANIFEST_PATH outside a fully resolved literal join chain"),
        "ls-files" => Some("`git ls-files` lists the whole tree"),
        _ => None,
    }
}

/// The reads of one Rust source (`file`, repository path) of the crate in `crate_dir`.
pub fn rust_reads(file: &str, text: &str, crate_dir: &str) -> Parsed {
    let syntax = match syn::parse_file(text) {
        Ok(f) => f,
        Err(e) => return Parsed::Fallback(e.to_string()),
    };
    let dir = paddock_repo_graph::dirname(file);
    let dir = if dir == "." { String::new() } else { dir };
    let mut f = Finder {
        dir: &dir,
        crate_dir,
        out: Vec::new(),
    };
    f.visit_file(&syntax);
    let mut out = f.out;
    out.sort();
    Parsed::Reads(out)
}

#[cfg(test)]
mod tests;
