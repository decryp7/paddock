//! C178: paddock-dev's top-level modules sit in layers, and a module refers only to its own layer or a lower one (rule 40, ER-21,
//! ER-22; the paddock-dev test-split ADR, decision 3 (e)). ER-21 class: 45 of the 66 modules (90% of the lines) form one cycle of
//! `crate::<module>` references, so no crate split and no per-area test unit is possible until the references that point up are gone.
//!
//! The layers are declared once, as a module-to-layer table in `tools/paddock-dev/layers.toml` ([`LAYERS`]), in the shape of the crate
//! split of docs/plans/paddock-dev-crate-split.md: a `[[layer]]` has a `name`, its `modules` and the layers it `uses` (each declared
//! earlier in the file, so the table cannot hold a cycle; a layer uses what its used layers use). A diamond is two layers that use the
//! same lower one and not each other. The check reads every production
//! `.rs` file under `tools/paddock-dev/src/`, takes the top-level module a file belongs to (`src/pre_push/exec.rs` is `pre_push`) and
//! every `crate::<module>` it names, and reports
//! - each reference from a module to a module of a layer its own layer does not use (one finding per reference, at its line),
//! - each module of `src/` the table does not list, and each table entry with no module in `src/` (the table is the one declaration,
//!   so it cannot lag the code).
//!
//! What is read: a `crate::` path anywhere in code (expressions, types, patterns, attributes), a `use crate::..` tree (groups
//! included) and a `crate::` path inside a macro's tokens. Not read: test code (a file under `tests/`, `tests.rs`, `*_tests.rs`,
//! `testutil*.rs`, and an item marked `#[cfg(test)]` or `#[test]`: a test of a lower module may drive a higher one, and the crate
//! split moves tests with their module), a reference to an item that lives in `lib.rs` itself (`crate::Foo`, not a module: it is no
//! module edge), and a reference to the module's own name. `self::` and `super::` paths stay inside the module.
//!
//! Blocking (findings are errors, mode gate; ER-41) since 2026-11-28. It was report-only from 2026-11-27, with 32 references in 19 module
//! pairs on 5e5991a31, until the back-edges were gone (the ADR's WP-4: batches B1 to B5). First green run on the real tree: `./dev check
//! run --only C178` on refactor/paddock-dev-b4-b5 at 1baf7a33c (the commit that removes the last 15 references), 2026-11-28: 0 errors,
//! 0 warnings, 295 probes, control true. It is deleted when the crate split lands, because cargo refuses a crate cycle itself. No
//! baseline file (ER-46): the report is the list. Rust host only: born in Rust.

use super::finding::Finding;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use paddock_repo_graph::model::os::is_test_file;
use proc_macro2::{TokenStream, TokenTree};
use std::collections::{BTreeMap, BTreeSet};
use syn::visit::Visit;

/// The rule field: rule 40, a recurring class is a missing design rule.
const RULE: u32 = 40;

/// The source folder of the crate.
pub const SRC: &str = "tools/paddock-dev/src/";

/// The module-to-layer table, declared once.
pub const LAYERS: &str = "tools/paddock-dev/layers.toml";

/// The parsed table: a module's layer (its index in the file), the layer names and, per layer, the layers it may refer to.
#[derive(Debug)]
pub struct Layers {
    names: Vec<String>,
    of: BTreeMap<String, usize>,
    uses: Vec<BTreeSet<usize>>,
}

impl Layers {
    /// Parses the table; an error names what is wrong (a module in two layers, an empty table, a TOML error).
    pub fn parse(text: &str) -> Result<Layers, String> {
        let table: toml::Table = text.parse().map_err(|e| format!("{LAYERS} does not parse: {e}"))?;
        let layers = table.get("layer").and_then(toml::Value::as_array).filter(|a| !a.is_empty());
        let Some(layers) = layers else {
            return Err(format!("{LAYERS} declares no [[layer]]"));
        };
        let mut of = BTreeMap::new();
        let mut names: Vec<String> = Vec::new();
        let mut uses: Vec<BTreeSet<usize>> = Vec::new();
        for (i, l) in layers.iter().enumerate() {
            let name = l
                .get("name")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| format!("{LAYERS}: layer {} has no `name`", i + 1))?;
            let modules = l
                .get("modules")
                .and_then(toml::Value::as_array)
                .ok_or_else(|| format!("{LAYERS}: layer `{name}` has no `modules` list"))?;
            let mut reach = BTreeSet::new();
            for u in l.get("uses").and_then(toml::Value::as_array).map_or(&[][..], Vec::as_slice) {
                let u = u
                    .as_str()
                    .ok_or_else(|| format!("{LAYERS}: layer `{name}` uses a layer that is not a string"))?;
                let Some(at) = names.iter().position(|n| n == u) else {
                    return Err(format!("{LAYERS}: layer `{name}` uses `{u}`, which is not declared above it"));
                };
                reach.insert(at);
                reach.extend(uses[at].iter().copied());
            }
            names.push(name.to_string());
            uses.push(reach);
            for m in modules {
                let m = m
                    .as_str()
                    .ok_or_else(|| format!("{LAYERS}: layer `{name}` lists a module that is not a string"))?;
                if let Some(first) = of.insert(m.to_string(), i) {
                    return Err(format!("{LAYERS}: module `{m}` is in layer `{}` and in `{name}`", names[first]));
                }
            }
        }
        Ok(Layers { names, of, uses })
    }

    /// The layer index of a module, if the table lists it.
    pub fn layer(&self, module: &str) -> Option<usize> {
        self.of.get(module).copied()
    }

    /// Whether a module of layer `from` may refer to a module of layer `to`: the same layer, or one it uses.
    pub fn may_use(&self, from: usize, to: usize) -> bool {
        from == to || self.uses[from].contains(&to)
    }

    /// The name of a layer.
    pub fn name(&self, layer: usize) -> &str {
        &self.names[layer]
    }

    /// Every module the table lists.
    pub fn modules(&self) -> impl Iterator<Item = &str> {
        self.of.keys().map(String::as_str)
    }
}

/// One `crate::<name>` reference: its 1-based line and the first segment after `crate`.
#[derive(Debug, PartialEq, Eq)]
pub struct Edge {
    /// The 1-based line.
    pub line: usize,
    /// The name after `crate::`.
    pub target: String,
}

/// The top-level module of a source file under [`SRC`], or `None` for a file outside it or a test file.
pub fn module_of(rel: &str) -> Option<String> {
    let rest = rel.strip_prefix(SRC)?;
    let name = rest.rsplit('/').next().unwrap_or(rest);
    if !rel.ends_with(".rs") || is_test_file(rel) || name.starts_with("testutil") {
        return None;
    }
    let first = rest.split('/').next()?;
    Some(first.strip_suffix(".rs").unwrap_or(first).to_string())
}

/// Whether an attribute list marks test code: `#[test]` or a `#[cfg(..)]` that names `test` and no `not`.
fn is_test_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if a.path().is_ident("test") {
            return true;
        }
        if !a.path().is_ident("cfg") {
            return false;
        }
        let s = quote_tokens(a);
        s.split(|c: char| !c.is_alphanumeric() && c != '_').any(|w| w == "test") && !s.contains("not")
    })
}

fn quote_tokens(a: &syn::Attribute) -> String {
    match &a.meta {
        syn::Meta::List(l) => l.tokens.to_string(),
        _ => String::new(),
    }
}

/// The attributes of the items that can carry `#[cfg(test)]`.
fn item_attrs(i: &syn::Item) -> &[syn::Attribute] {
    use syn::Item::*;
    match i {
        Mod(x) => &x.attrs,
        Fn(x) => &x.attrs,
        Impl(x) => &x.attrs,
        Use(x) => &x.attrs,
        Const(x) => &x.attrs,
        Static(x) => &x.attrs,
        Struct(x) => &x.attrs,
        Enum(x) => &x.attrs,
        Trait(x) => &x.attrs,
        Type(x) => &x.attrs,
        _ => &[],
    }
}

#[derive(Default)]
struct Collect {
    edges: Vec<Edge>,
}

impl Collect {
    fn push(&mut self, line: usize, target: &syn::Ident) {
        self.edges.push(Edge {
            line,
            target: target.to_string(),
        });
    }

    /// `crate :: name` in a token stream (a macro's arguments are not parsed by syn).
    fn tokens(&mut self, ts: TokenStream) {
        let v: Vec<TokenTree> = ts.into_iter().collect();
        for (i, t) in v.iter().enumerate() {
            match t {
                TokenTree::Group(g) => self.tokens(g.stream()),
                TokenTree::Ident(id) if id == "crate" => {
                    let colons = matches!((v.get(i + 1), v.get(i + 2)), (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b))) if a.as_char() == ':' && b.as_char() == ':');
                    if let (true, Some(TokenTree::Ident(name))) = (colons, v.get(i + 3)) {
                        self.push(id.span().start().line, name);
                    }
                }
                _ => {}
            }
        }
    }

    /// The names that follow `crate::` in a `use` tree below the `crate` segment.
    fn use_tree(&mut self, tree: &syn::UseTree, line: usize) {
        match tree {
            syn::UseTree::Path(p) => self.push(line, &p.ident),
            syn::UseTree::Name(n) => self.push(line, &n.ident),
            syn::UseTree::Rename(r) => self.push(line, &r.ident),
            syn::UseTree::Group(g) => g.items.iter().for_each(|t| self.use_tree(t, line)),
            syn::UseTree::Glob(_) => {}
        }
    }
}

impl<'ast> Visit<'ast> for Collect {
    fn visit_item(&mut self, i: &'ast syn::Item) {
        if is_test_attr(item_attrs(i)) {
            return;
        }
        syn::visit::visit_item(self, i);
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        if is_test_attr(&f.attrs) {
            return;
        }
        syn::visit::visit_impl_item_fn(self, f);
    }

    fn visit_path(&mut self, p: &'ast syn::Path) {
        let mut segs = p.segments.iter();
        if let (Some(first), Some(second)) = (segs.next(), segs.next())
            && first.ident == "crate"
            && p.leading_colon.is_none()
        {
            self.push(first.ident.span().start().line, &second.ident);
        }
        syn::visit::visit_path(self, p);
    }

    fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
        if let syn::UseTree::Path(p) = &u.tree
            && p.ident == "crate"
        {
            self.use_tree(&p.tree, p.ident.span().start().line);
        }
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        self.visit_path(&m.path);
        self.tokens(m.tokens.clone());
    }
}

/// The `crate::<name>` references of one file's production code, in source order. A file that does not parse has none.
pub fn edges(text: &str) -> Vec<Edge> {
    let Ok(file) = syn::parse_file(text) else { return Vec::new() };
    let mut c = Collect::default();
    c.visit_file(&file);
    c.edges.sort_by_key(|e| e.line);
    c.edges
}

/// The check.
pub struct C178;

impl Check for C178 {
    fn id(&self) -> &'static str {
        "C178"
    }

    fn rule(&self) -> u32 {
        RULE
    }

    fn title(&self) -> &'static str {
        "paddock-dev's modules refer only to their own layer or a layer it uses (tools/paddock-dev/layers.toml)"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::Gate
    }

    fn origin(&self) -> super::Origin {
        super::Origin::BornInRust
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    LAYERS,
                    "[[layer]]\nname = \"low\"\nmodules = [\"a\", \"gone\"]\n\n[[layer]]\nname = \"high\"\nuses = [\"low\"]\nmodules = [\"b\", \"lib\"]\n",
                ),
                // Red: a use of a higher module (line 2) and a path inside a macro (line 4); a module the table does not list.
                (
                    "tools/paddock-dev/src/a/mod.rs",
                    "pub fn f() {\n    use crate::b::X;\n}\npub fn g() {\n    m!(crate::b::W);\n}\n",
                ),
                ("tools/paddock-dev/src/c.rs", "pub fn c() {}\n"),
                // Green: a reference down, the module's own name, an item of lib.rs, test code, the root file.
                (
                    "tools/paddock-dev/src/b.rs",
                    "pub fn h() {\n    crate::a::g();\n    crate::b::h();\n    let _ = crate::ROOT_ITEM;\n}\n",
                ),
                ("tools/paddock-dev/src/a/tests.rs", "use crate::b::X;\n"),
                (
                    "tools/paddock-dev/src/a/other.rs",
                    "#[cfg(test)]\nmod t {\n    use crate::b::Y;\n}\n#[test]\nfn u() {\n    crate::b::h();\n}\n",
                ),
                ("tools/paddock-dev/src/lib.rs", "pub mod a;\npub mod b;\n"),
                ("tools/other/src/a.rs", "use crate::b::X;\n"),
            ],
            expect: vec![
                ("tools/paddock-dev/layers.toml", 1),
                ("tools/paddock-dev/src/a/mod.rs", 2),
                ("tools/paddock-dev/src/a/mod.rs", 5),
                ("tools/paddock-dev/src/c.rs", 1),
            ],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let unmet = |detail: String| CheckOutput {
            findings: Vec::new(),
            probes: 0,
            completed: false,
            precondition: Precondition { met: false, detail },
        };
        let Some(text) = ctx.read(LAYERS) else {
            return unmet(format!("{LAYERS} is missing: the layers are declared there"));
        };
        let layers = match Layers::parse(&text) {
            Ok(l) => l,
            Err(e) => return unmet(e),
        };
        let files: Vec<String> = ctx
            .files()
            .iter()
            .filter(|f| f.starts_with(SRC) && f.ends_with(".rs"))
            .cloned()
            .collect();
        // Every module of src/, test files included: a module that holds only tests is still a module.
        let mut modules: BTreeMap<String, String> = BTreeMap::new();
        for f in &files {
            if let Some(first) = f.strip_prefix(SRC).and_then(|r| r.split('/').next()) {
                modules
                    .entry(first.strip_suffix(".rs").unwrap_or(first).to_string())
                    .or_insert_with(|| f.clone());
            }
        }
        let mut findings = Vec::new();
        for (m, file) in &modules {
            if layers.layer(m).is_none() {
                findings.push(Finding::error(
                    "C178",
                    RULE,
                    file,
                    1,
                    format!("module `{m}` is in no layer of {LAYERS}"),
                    format!("Add `{m}` to the `modules` of the layer it belongs to: the lowest layer that holds everything it refers to."),
                ));
            }
        }
        let listed: BTreeSet<&str> = modules.keys().map(String::as_str).collect();
        for m in layers.modules().filter(|m| !listed.contains(m)) {
            findings.push(Finding::error(
                "C178",
                RULE,
                LAYERS,
                1,
                format!("{LAYERS} lists module `{m}`, which {SRC} does not have"),
                "Delete the entry (the table is the one declaration of the modules' layers).",
            ));
        }
        let mut probes = 0;
        for f in &files {
            let Some(from) = module_of(f) else { continue };
            let Some(text) = ctx.read(f) else { continue };
            probes += 1;
            let Some(from_layer) = layers.layer(&from) else { continue };
            for e in edges(&text) {
                // An item of lib.rs, or the module's own name, is no edge between modules.
                if e.target == from || !modules.contains_key(&e.target) {
                    continue;
                }
                let Some(to_layer) = layers.layer(&e.target) else { continue };
                if !layers.may_use(from_layer, to_layer) {
                    findings.push(Finding::error(
                        "C178",
                        RULE,
                        f,
                        e.line,
                        format!(
                            "`{from}` (layer `{}`) refers to `crate::{}`, a module of layer `{}`, which `{}` does not use",
                            layers.name(from_layer),
                            e.target,
                            layers.name(to_layer),
                            layers.name(from_layer)
                        ),
                        format!(
                            "Move what `{from}` needs from `{}` into a layer `{from}` may use (or into a trait `{from}` owns, with the implementation in `{}`), so the reference points to a used layer; move `{from}` only if it belongs in another layer.",
                            e.target, e.target
                        ),
                    ));
                }
            }
        }
        CheckOutput {
            findings,
            probes,
            completed: true,
            precondition: Precondition {
                met: probes > 0,
                detail: format!(
                    "{probes} production files of {} modules read, {} layers",
                    modules.len(),
                    layers.names.len()
                ),
            },
        }
    }
}
