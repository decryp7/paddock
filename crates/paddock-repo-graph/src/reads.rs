//! The Rust `reads` extractor (ADR 0117 §4; the `rust-syn` feature): the data files and folders a Rust source reads at build or
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
//! Macro bodies (`assert!`, `vec!`, a test macro) are parsed as expressions where they can be, so a read inside them counts. A
//! source that `syn` cannot parse falls back to the caller's text scan ([`Parsed::Fallback`]), measured beside this one.

use proc_macro2::TokenStream;
use syn::punctuated::Punctuated;
use syn::visit::Visit;
use syn::{Expr, ExprLit, ExprMacro, ExprMethodCall, ItemMod, Lit, Macro, Meta, Token};

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
    m.path
        .segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default()
}

fn lit_str(e: &Expr) -> Option<String> {
    match e {
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => Some(s.value()),
        Expr::Group(g) => lit_str(&g.expr),
        Expr::Paren(p) => lit_str(&p.expr),
        _ => None,
    }
}

fn args(tokens: &TokenStream) -> Option<Vec<Expr>> {
    syn::parse::Parser::parse2(
        Punctuated::<Expr, Token![,]>::parse_terminated,
        tokens.clone(),
    )
    .ok()
    .map(|p| p.into_iter().collect())
}

/// `env!("CARGO_MANIFEST_DIR")`, also wrapped: `Path::new(env!(..))`, `PathBuf::from(env!(..))`, `&env!(..)`.
fn is_manifest(e: &Expr) -> bool {
    match e {
        Expr::Macro(ExprMacro { mac, .. }) if last_ident(mac) == "env" => args(&mac.tokens)
            .is_some_and(|a| {
                a.first()
                    .and_then(lit_str)
                    .is_some_and(|s| s == "CARGO_MANIFEST_DIR")
            }),
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

/// `X.join("a")` (and `.to_path_buf()`, `.as_path()`: the same path).
fn call_pieces(mc: &ExprMethodCall) -> Option<Vec<Piece>> {
    let ExprMethodCall {
        receiver,
        method,
        args: a,
        ..
    } = mc;
    if method == "join" && a.len() == 1 {
        let mut p = pieces(receiver)?;
        p.push(Piece::Lit(format!("/{}", lit_str(a.first()?)?)));
        Some(p)
    } else if a.is_empty() {
        pieces(receiver)
    } else {
        None
    }
}

fn macro_pieces(mac: &Macro) -> Option<Vec<Piece>> {
    let a = args(&mac.tokens)?;
    match last_ident(mac).as_str() {
        "concat" => {
            let mut out = Vec::new();
            for e in &a {
                if is_manifest(e) {
                    out.push(Piece::Manifest);
                } else {
                    out.push(Piece::Lit(lit_str(e)?));
                }
            }
            out.iter()
                .any(|p| matches!(p, Piece::Manifest))
                .then_some(out)
        }
        "format" => {
            let fmt = lit_str(a.first()?)?;
            let rest = fmt.strip_prefix("{}")?;
            (a.len() == 2 && is_manifest(&a[1]))
                .then(|| vec![Piece::Manifest, Piece::Lit(rest.to_string())])
        }
        _ => None,
    }
}

impl Finder<'_> {
    fn add(&mut self, base: &str, rel: &str, how: How) {
        if rel.is_empty() || rel.contains('{') {
            return;
        }
        if let Some(p) = crate::join_normalized(base, rel.trim_start_matches('/'))
            && !p.is_empty()
        {
            let r = Read { path: p, how };
            if !self.out.contains(&r) {
                self.out.push(r);
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
        let Some(a) = args(&mac.tokens) else { return };
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
                if let Some(p) = macro_pieces(mac) {
                    self.rooted(&p, How::ManifestDir);
                }
                // Any other macro's arguments are expressions too (`assert!`, `vec!`): a read inside one counts.
                a.iter().for_each(|e| self.visit_expr(e));
            }
        }
    }
}

impl<'ast> Visit<'ast> for Finder<'_> {
    fn visit_macro(&mut self, m: &'ast Macro) {
        self.mac(m);
    }

    fn visit_expr_method_call(&mut self, e: &'ast ExprMethodCall) {
        if e.method == "join"
            && let Some(p) = call_pieces(e)
        {
            self.rooted(&p, How::ManifestDir);
            return;
        }
        syn::visit::visit_expr_method_call(self, e);
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

/// The reads of one Rust source (`file`, repository path) of the crate in `crate_dir`.
pub fn rust_reads(file: &str, text: &str, crate_dir: &str) -> Parsed {
    let syntax = match syn::parse_file(text) {
        Ok(f) => f,
        Err(e) => return Parsed::Fallback(e.to_string()),
    };
    let dir = crate::dirname(file);
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
