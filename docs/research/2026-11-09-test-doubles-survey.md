# Test-double survey tool (2026-11-09, ADR 0073)

The read-only tool behind the survey tables of [ADR 0073](../adr/0073-no-test-doubles-in-a-shipped-build.md) and the
timings in [benchmarks.md](../benchmarks.md#2026-11-09-test-double-survey-scan-and-shipped-feature-trees) (rule 48: the
command must be reproducible). It is a generalisation of Dyno's `no_ungated_doubles.rs` (the same `cfg` evaluation). It is
**not** the check: ADR 0073 WP-0 builds that in `./dev`. It is kept here as text, not as a crate, so no workspace,
formatter or planner picks it up.

## Run it

Copy the two files below into an empty folder outside the repository (`Cargo.toml` and `src/main.rs`), then:

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=<scratch>/target cargo build --release --offline
# one Cargo workspace at a time; the second argument is the repository root (paths are printed relative to it)
<scratch>/target/release/fake-scan apps/pitwall .   > pitwall.tsv
<scratch>/target/release/fake-scan crates .         > crates.tsv
```

Output: one tab-separated line per finding: package, `file:line`, kind, name, the item's own `cfg` text. Kinds:

- `pub-double`, `priv-double`, `double-mod`, `double-impl` and `pub-double-fn`: the declaration rules;
- `doc-test-only`: the advisory doc-comment rule (noisy, triaged by hand);
- `switch-literal`: a `--` or environment-like literal with a test word;
- `env-read`: an `env::var` or `env::var_os` argument, literal or constant;
- `flag-arm`: a `"--…"` match arm;
- `use-site`: a path that names a double;
- `feature`: a package feature with a test word.

Anything gated by `cfg(test)`, `#[test]` or a `cfg` that is false with `test` and `feature = "test-support"` off is
skipped. The shipped scope was then taken from `cargo tree -p <pkg> -e normal --prefix none --target all -f '{p} {f}'` of
each shipped package. Known blind spots: clap-derived flags, `==` comparisons with `"--…"`, and environment names passed
to a helper. ADR 0073 §2.3 asks WP-0 to cover them.

## Cargo.toml

```toml
[package]
name = "fake-scan"
version = "0.0.0"
edition = "2021"
publish = false
[dependencies]
syn = { version = "2", features = ["full", "visit", "extra-traits"] }
proc-macro2 = { version = "1", features = ["span-locations"] }
serde_json = "1"
quote = "1"
[workspace]
```

## src/main.rs

```rust
//! Survey (ADR 0073 draft): every test double, test-only module and test-only switch that a build without test targets
//! and without `test-support` still compiles. Generalised from Dyno's `no_ungated_doubles.rs` (cfg evaluation identical).
//! Usage: fake-scan <cargo workspace dir> <repo root>. Prints TSV: package, file:line, kind, name, detail.
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;
use syn::visit::Visit;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tri {
    False,
    True,
    Maybe,
}
fn not(t: Tri) -> Tri {
    match t {
        Tri::False => Tri::True,
        Tri::True => Tri::False,
        Tri::Maybe => Tri::Maybe,
    }
}
const GATE_FEATURES: &[&str] = &["test-support"];
fn eval_cfg(meta: &syn::Meta) -> Tri {
    match meta {
        syn::Meta::Path(p) if p.is_ident("test") => Tri::False,
        syn::Meta::NameValue(nv) if nv.path.is_ident("feature") => match &nv.value {
            syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) if GATE_FEATURES.contains(&s.value().as_str()) => Tri::False,
            _ => Tri::Maybe,
        },
        syn::Meta::List(l) if l.path.is_ident("any") || l.path.is_ident("all") || l.path.is_ident("not") => {
            use syn::punctuated::Punctuated;
            let Ok(args) = l.parse_args_with(Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated) else { return Tri::Maybe };
            let vals: Vec<Tri> = args.iter().map(eval_cfg).collect();
            if l.path.is_ident("not") {
                return vals.first().map_or(Tri::Maybe, |t| not(*t));
            }
            let any = l.path.is_ident("any");
            let (decisive, neutral) = if any { (Tri::True, Tri::False) } else { (Tri::False, Tri::True) };
            if vals.contains(&decisive) {
                decisive
            } else if vals.iter().all(|t| *t == neutral) {
                neutral
            } else {
                Tri::Maybe
            }
        }
        _ => Tri::Maybe,
    }
}
fn gated(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if a.path().is_ident("test") {
            return true;
        }
        if !a.path().is_ident("cfg") {
            return false;
        }
        a.parse_args::<syn::Meta>().is_ok_and(|m| eval_cfg(&m) == Tri::False)
    })
}
/// The cfg text of an attribute list, for the report (a `Maybe` gate such as `feature = "fake"`).
fn cfg_text(attrs: &[syn::Attribute]) -> String {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("cfg"))
        .map(|a| match &a.meta {
            syn::Meta::List(l) => format!("cfg({})", l.tokens),
            _ => String::new(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}
const DOUBLE_PREFIXES: &[&str] = &["Fake", "Mock", "Stub", "Scripted", "Dummy"];
fn is_double(name: &str) -> bool {
    DOUBLE_PREFIXES.iter().any(|p| name.starts_with(p))
}
const DOUBLE_MODS: &[&str] = &["fake", "fakes", "mock", "mocks", "testing", "test_support", "testkit", "stub", "stubs", "fake_engine", "test_util", "test_utils"];
fn is_double_fn(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["fake", "mock", "stub", "for_test", "for_tests", "test_"].iter().any(|p| n.starts_with(p)) || n.ends_with("_for_test") || n.ends_with("_for_tests")
}
fn suspicious_lit(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    if s.starts_with("--") && s.len() < 40 && !s.contains(' ') {
        return ["fake", "mock", "test", "e2e", "ready-when", "die-after", "stub", "scripted", "crash", "simulate"].iter().any(|w| l.contains(w));
    }
    // Environment-variable-like names.
    let envish = s.len() > 3 && s.len() < 60 && s.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') && s.contains('_');
    envish && ["FAKE", "MOCK", "TEST", "E2E", "STUB", "SCRIPTED", "SIMULATE"].iter().any(|w| s.contains(w))
}

fn doc_of(attrs: &[syn::Attribute]) -> String {
    attrs.iter().filter(|a| a.path().is_ident("doc")).filter_map(|a| match &a.meta {
        syn::Meta::NameValue(nv) => match &nv.value { syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => Some(s.value()), _ => None },
        _ => None,
    }).collect::<Vec<_>>().join(" ")
}
fn doc_says_test(d: &str) -> bool {
    let l = d.to_ascii_lowercase();
    ["for tests", "test double", "test fake", "test fixture", "for testing", "only for tests", "tests only", "test-only", "test only"].iter().any(|w| l.contains(w))
}
fn quote_expr(e: &syn::Expr) -> String {
    let t = format!("{}", quote::ToTokens::to_token_stream(e));
    t.chars().take(60).collect()
}
struct Finding {
    file: String,
    line: usize,
    kind: &'static str,
    name: String,
    detail: String,
}
struct Scan<'a> {
    root: &'a Path,
    out: Vec<Finding>,
    gated_seen: usize,
}
fn line_of(s: proc_macro2::Span) -> usize {
    s.start().line
}
fn attrs_of(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Struct(i) => &i.attrs,
        syn::Item::Enum(i) => &i.attrs,
        syn::Item::Trait(i) => &i.attrs,
        syn::Item::Type(i) => &i.attrs,
        syn::Item::Fn(i) => &i.attrs,
        syn::Item::Const(i) => &i.attrs,
        syn::Item::Static(i) => &i.attrs,
        syn::Item::Mod(i) => &i.attrs,
        syn::Item::Impl(i) => &i.attrs,
        syn::Item::Use(i) => &i.attrs,
        syn::Item::Macro(i) => &i.attrs,
        _ => &[],
    }
}
fn item_name(item: &syn::Item) -> Option<(&syn::Visibility, String, &'static str)> {
    match item {
        syn::Item::Struct(i) => Some((&i.vis, i.ident.to_string(), "struct")),
        syn::Item::Enum(i) => Some((&i.vis, i.ident.to_string(), "enum")),
        syn::Item::Trait(i) => Some((&i.vis, i.ident.to_string(), "trait")),
        syn::Item::Type(i) => Some((&i.vis, i.ident.to_string(), "type")),
        syn::Item::Fn(i) => Some((&i.vis, i.sig.ident.to_string(), "fn")),
        syn::Item::Const(i) => Some((&i.vis, i.ident.to_string(), "const")),
        syn::Item::Static(i) => Some((&i.vis, i.ident.to_string(), "static")),
        _ => None,
    }
}
fn type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        syn::Type::Reference(r) => type_name(&r.elem),
        _ => None,
    }
}
fn path_attr(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find(|a| a.path().is_ident("path")).and_then(|a| match &a.meta {
        syn::Meta::NameValue(nv) => match &nv.value {
            syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => Some(s.value()),
            _ => None,
        },
        _ => None,
    })
}
fn child_dir(file: &Path) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new("."));
    match file.file_stem().and_then(|s| s.to_str()) {
        Some("lib" | "main" | "mod") | None => dir.to_path_buf(),
        Some(stem) => dir.join(stem),
    }
}

/// Collects suspicious string literals in code that is not gated.
struct Lits<'s> {
    found: &'s mut Vec<(usize, String)>,
}
impl<'ast> Visit<'ast> for Lits<'_> {
    fn visit_item(&mut self, i: &'ast syn::Item) {
        if gated(attrs_of(i)) {
            return;
        }
        if let syn::Item::Mod(_) = i {
            return; // modules are walked by the scan itself
        }
        syn::visit::visit_item(self, i);
    }
    fn visit_impl_item_fn(&mut self, i: &'ast syn::ImplItemFn) {
        if gated(&i.attrs) {
            return;
        }
        syn::visit::visit_impl_item_fn(self, i);
    }
    fn visit_expr(&mut self, e: &'ast syn::Expr) {
        let attrs: &[syn::Attribute] = match e {
            syn::Expr::Block(b) => &b.attrs,
            syn::Expr::Call(c) => &c.attrs,
            syn::Expr::MethodCall(c) => &c.attrs,
            syn::Expr::If(c) => &c.attrs,
            _ => &[],
        };
        if gated(attrs) {
            return;
        }
        syn::visit::visit_expr(self, e);
    }
    fn visit_stmt(&mut self, s: &'ast syn::Stmt) {
        let attrs: &[syn::Attribute] = match s {
            syn::Stmt::Local(l) => &l.attrs,
            syn::Stmt::Macro(m) => &m.attrs,
            _ => &[],
        };
        if gated(attrs) {
            return;
        }
        syn::visit::visit_stmt(self, s);
    }
    fn visit_arm(&mut self, a: &'ast syn::Arm) {
        if gated(&a.attrs) {
            return;
        }
        syn::visit::visit_arm(self, a);
    }
    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        if let syn::Expr::Path(fp) = &*c.func {
            let segs: Vec<String> = fp.path.segments.iter().map(|s| s.ident.to_string()).collect();
            let last = segs.last().map(String::as_str).unwrap_or("");
            if (last == "var" || last == "var_os") && (segs.len() == 1 || segs.iter().any(|s| s == "env")) {
                if let Some(a) = c.args.first() {
                    let what = match a {
                        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => s.value(),
                        syn::Expr::Path(p) => format!("const {}", p.path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>().join("::")),
                        syn::Expr::Reference(r) => format!("expr {}", quote_expr(&r.expr)),
                        other => format!("expr {}", quote_expr(other)),
                    };
                    self.found.push((line_of(c.span()), format!("ENV {what}")));
                }
            }
        }
        syn::visit::visit_expr_call(self, c);
    }
    fn visit_pat(&mut self, pt: &'ast syn::Pat) {
        if let syn::Pat::Lit(syn::ExprLit { lit: syn::Lit::Str(st), .. }) = pt {
            let v = st.value();
            if v.starts_with("--") {
                self.found.push((line_of(st.span()), format!("FLAG {v}")));
                return;
            }
        }
        syn::visit::visit_pat(self, pt);
    }
    fn visit_lit_str(&mut self, l: &'ast syn::LitStr) {
        let v = l.value();
        if suspicious_lit(&v) {
            self.found.push((line_of(l.span()), v));
        }
    }
    fn visit_path(&mut self, p: &'ast syn::Path) {
        let segs: Vec<String> = p.segments.iter().map(|s| s.ident.to_string()).collect();
        if segs.iter().any(|s| is_double(s) || DOUBLE_MODS.contains(&s.as_str()) || s == "MemorySecrets") {
            self.found.push((line_of(p.span()), format!("USE {}", segs.join("::"))));
        }
        syn::visit::visit_path(self, p);
    }
    fn visit_use_tree(&mut self, u: &'ast syn::UseTree) {
        let mut t = Vec::new();
        fn walk(u: &syn::UseTree, pre: &mut Vec<String>, out: &mut Vec<(usize, String)>) {
            match u {
                syn::UseTree::Path(p) => { pre.push(p.ident.to_string()); walk(&p.tree, pre, out); pre.pop(); }
                syn::UseTree::Name(n) => { let mut v = pre.clone(); v.push(n.ident.to_string()); check(&v, n.ident.span(), out); }
                syn::UseTree::Rename(n) => { let mut v = pre.clone(); v.push(n.ident.to_string()); check(&v, n.ident.span(), out); }
                syn::UseTree::Glob(g) => { let mut v = pre.clone(); v.push("*".into()); check(&v, g.star_token.span, out); }
                syn::UseTree::Group(g) => for i in &g.items { walk(i, pre, out); },
            }
        }
        fn check(v: &[String], sp: proc_macro2::Span, out: &mut Vec<(usize, String)>) {
            if v.iter().any(|s| is_double(s) || DOUBLE_MODS.contains(&s.as_str()) || s == "MemorySecrets") {
                out.push((sp.start().line, format!("USE {}", v.join("::"))));
            }
        }
        walk(u, &mut t, self.found);
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        // Literals inside macros (println!, matches!, clap attrs) are tokens: scan them.
        for t in m.tokens.clone() {
            self.tok(t);
        }
    }
}
impl Lits<'_> {
    fn tok(&mut self, t: proc_macro2::TokenTree) {
        match t {
            proc_macro2::TokenTree::Group(g) => {
                for t in g.stream() {
                    self.tok(t);
                }
            }
            proc_macro2::TokenTree::Literal(l) => {
                if let Ok(s) = syn::parse_str::<syn::LitStr>(&l.to_string()) {
                    if suspicious_lit(&s.value()) {
                        self.found.push((l.span().start().line, s.value()));
                    }
                }
            }
            _ => {}
        }
    }
}

impl Scan<'_> {
    fn rel(&self, p: &Path) -> String {
        p.strip_prefix(self.root).unwrap_or(p).display().to_string()
    }
    fn push(&mut self, file: &Path, line: usize, kind: &'static str, name: String, detail: String) {
        let file = self.rel(file);
        self.out.push(Finding { file, line, kind, name, detail });
    }
    fn file(&mut self, path: &Path) {
        let Ok(src) = std::fs::read_to_string(path) else {
            self.push(path, 0, "unreadable", String::new(), String::new());
            return;
        };
        match syn::parse_file(&src) {
            Ok(f) => {
                if gated(&f.attrs) {
                    self.gated_seen += 1;
                    return;
                }
                self.items(&f.items, path, &child_dir(path))
            }
            Err(e) => self.push(path, 0, "parse-error", String::new(), e.to_string()),
        }
    }
    fn items(&mut self, items: &[syn::Item], file: &Path, dir: &Path) {
        for item in items {
            self.item(item, file, dir);
        }
    }
    fn item(&mut self, item: &syn::Item, file: &Path, dir: &Path) {
        let attrs = attrs_of(item);
        if gated(attrs) {
            self.gated_seen += 1;
            return;
        }
        let cfg = cfg_text(attrs);
        if let Some((vis, name, kind)) = item_name(item).or_else(|| match item {
            syn::Item::Mod(m) => Some((&m.vis, m.ident.to_string(), "mod")),
            _ => None,
        }) {
            let doc = doc_of(attrs);
            if !is_double(&name) && !matches!(vis, syn::Visibility::Inherited) && doc_says_test(&doc) {
                self.push(file, line_of(item.span()), "doc-test-only", format!("{kind} {name}"), cfg.clone());
            }
        }
        if let Some((vis, name, kind)) = item_name(item) {
            let public = !matches!(vis, syn::Visibility::Inherited);
            if is_double(&name) {
                self.push(file, line_of(item.span()), if public { "pub-double" } else { "priv-double" }, format!("{kind} {name}"), cfg.clone());
            } else if kind == "fn" && public && is_double_fn(&name) {
                self.push(file, line_of(item.span()), "pub-double-fn", format!("fn {name}"), cfg.clone());
            }
        }
        match item {
            syn::Item::Mod(m) => {
                let name = m.ident.to_string();
                if DOUBLE_MODS.contains(&name.as_str()) || name.starts_with("fake") || name.starts_with("mock") {
                    self.push(file, line_of(m.ident.span()), "double-mod", format!("mod {name}"), cfg.clone());
                }
                if let Some((_, inner)) = &m.content {
                    self.items(inner, file, &dir.join(&name));
                } else {
                    let file_dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
                    let cands = match path_attr(&m.attrs) {
                        Some(p) => vec![file_dir.join(p)],
                        None => vec![dir.join(format!("{name}.rs")), dir.join(&name).join("mod.rs")],
                    };
                    match cands.into_iter().find(|p| p.exists()) {
                        Some(p) => self.file(&p),
                        None => self.push(file, line_of(m.ident.span()), "missing-mod", format!("mod {name}"), cfg.clone()),
                    }
                }
                return;
            }
            syn::Item::Impl(i) => {
                let tn = type_name(&i.self_ty).unwrap_or_default();
                if i.trait_.is_some() && is_double(&tn) {
                    let tr = i.trait_.as_ref().and_then(|t| t.1.segments.last()).map(|s| s.ident.to_string()).unwrap_or_default();
                    self.push(file, line_of(i.self_ty.span()), "double-impl", format!("impl {tr} for {tn}"), cfg.clone());
                }
                if i.trait_.is_none() {
                    for ii in &i.items {
                        if let syn::ImplItem::Fn(f) = ii {
                            let n = f.sig.ident.to_string();
                            if !gated(&f.attrs) && !matches!(f.vis, syn::Visibility::Inherited) && is_double_fn(&n) {
                                self.push(file, line_of(f.sig.ident.span()), "pub-double-fn", format!("{tn}::{n}"), cfg_text(&f.attrs));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        let mut found = Vec::new();
        Lits { found: &mut found }.visit_item(item);
        for (line, v) in found {
            let kind = if v.starts_with("USE ") { "use-site" } else if v.starts_with("ENV ") { "env-read" } else if v.starts_with("FLAG ") { "flag-arm" } else { "switch-literal" };
            let v = v.trim_start_matches("USE ").trim_start_matches("ENV ").trim_start_matches("FLAG ").to_string();
            self.push(file, line, kind, v, cfg.clone());
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ws = PathBuf::from(&args[1]);
    let repo = PathBuf::from(&args[2]).canonicalize().unwrap();
    let out = std::process::Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1", "--offline"])
        .current_dir(&ws)
        .output()
        .expect("cargo metadata");
    if !out.status.success() {
        eprintln!("cargo metadata failed in {}: {}", ws.display(), String::from_utf8_lossy(&out.stderr));
        std::process::exit(2);
    }
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let mut total_gated = 0;
    for p in meta["packages"].as_array().unwrap() {
        let name = p["name"].as_str().unwrap();
        let feats: Vec<String> = p["features"].as_object().unwrap().keys().filter(|k| {
            let l = k.to_ascii_lowercase();
            ["test", "fake", "mock", "e2e", "stub"].iter().any(|w| l.contains(w))
        }).cloned().collect();
        if !feats.is_empty() {
            println!("{name}\t{}\tfeature\t{}\t", p["manifest_path"].as_str().unwrap().replace(repo.to_str().unwrap(), "").trim_start_matches('/'), feats.join(","));
        }
        let mut seen = std::collections::BTreeSet::new();
        for t in p["targets"].as_array().unwrap() {
            let kinds: Vec<&str> = t["kind"].as_array().unwrap().iter().filter_map(|k| k.as_str()).collect();
            if !kinds.iter().any(|k| ["lib", "rlib", "bin", "proc-macro", "cdylib", "staticlib"].contains(k)) {
                continue;
            }
            let src = PathBuf::from(t["src_path"].as_str().unwrap());
            if !seen.insert(src.clone()) {
                continue;
            }
            let mut s = Scan { root: &repo, out: Vec::new(), gated_seen: 0 };
            s.file(&src);
            total_gated += s.gated_seen;
            for f in s.out {
                println!("{name}\t{}:{}\t{}\t{}\t{}", f.file, f.line, f.kind, f.name, f.detail);
            }
        }
    }
    eprintln!("gated items skipped: {total_gated}");
}
```
