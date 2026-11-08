#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Every reading the helper advertises has a production sender (ADR 0052 section 1.1, C83 for Dyno until `./dev check` takes
//! it over; Dyno plan 2026-11-04 section 7.2).
//!
//! The scan is by name, not a call graph: it parses the non-test code of `dyno-agent/src` and `dyno-platform/src` with `syn`,
//! finds the functions that construct `HelperRequest::<Kind>`, and asks whether any non-test code calls (or mentions) such a
//! function from outside its own impl block, or, for a trait impl, whether the composition boxes the implementing type
//! (`Box::new(X..)` or `Arc::new(X..)` outside X's impl). Closures, macros and trait objects are not followed; the helper's
//! run-time `served:` counts (ADR 0052 section 1.5) are the backstop for those.
//!
//! The kinds that are neither advertised nor wired must be declared in `apps/dyno/wiring.toml` with a reason and an end.
//! The tests over the checker itself (fixtures that must fail) run always; the two tests over the real tree are `#[ignore]`d
//! until WP-INT has wired every reading and reconciled the user guide and the changelog.

use dyno_core::helper::HelperHost;
use dyno_core::helper_policy::RequestKind;
use dyno_core::helper_wire::{readings, Advertised};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use syn::visit::Visit;

// ---------------------------------------------------------------- the scan

/// One non-test function, by name.
#[derive(Debug, Clone, Default)]
struct FnInfo {
    name: String,
    /// The file it is in.
    file: String,
    /// The type of the enclosing impl block, when there is one.
    owner: Option<String>,
    /// The trait of the enclosing impl block, when it is a trait impl.
    trait_impl: Option<String>,
    /// `HelperRequest` kinds this function constructs.
    builds: BTreeSet<String>,
    /// Last path segments of everything called or mentioned as a path or method.
    mentions: BTreeSet<String>,
    /// Types named inside `Box::new(..)`, `Arc::new(..)` and `Rc::new(..)` arguments.
    boxed: BTreeSet<String>,
}

fn is_test_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if a.path().is_ident("test") {
            return true;
        }
        if !a.path().is_ident("cfg") {
            return false;
        }
        let text = quote_tokens(a);
        text.contains("test") && !text.contains("not (test)") && !text.contains("not(test)")
    })
}

fn quote_tokens(a: &syn::Attribute) -> String {
    match &a.meta {
        syn::Meta::List(l) => l.tokens.to_string(),
        _ => String::new(),
    }
}

#[derive(Default)]
struct Scan {
    fns: Vec<FnInfo>,
    file: String,
    owner: Option<String>,
    trait_impl: Option<String>,
}

fn type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        syn::Type::Reference(r) => type_name(&r.elem),
        _ => None,
    }
}

/// Collects what one function body constructs, calls and boxes.
struct Body<'a>(&'a mut FnInfo);

fn path_idents(p: &syn::Path) -> Vec<String> {
    p.segments.iter().map(|s| s.ident.to_string()).collect()
}

impl Body<'_> {
    fn path(&mut self, p: &syn::Path) {
        let ids = path_idents(p);
        if ids.len() >= 2 && ids[ids.len() - 2] == "HelperRequest" {
            self.0.builds.insert(ids[ids.len() - 1].clone());
        }
        if let Some(last) = ids.last() {
            self.0.mentions.insert(last.clone());
        }
    }
}

struct TypeNames(BTreeSet<String>);
impl<'ast> Visit<'ast> for TypeNames {
    fn visit_path(&mut self, p: &'ast syn::Path) {
        for s in &p.segments {
            self.0.insert(s.ident.to_string());
        }
        syn::visit::visit_path(self, p);
    }
}

impl<'ast> Visit<'ast> for Body<'_> {
    fn visit_expr_path(&mut self, e: &'ast syn::ExprPath) {
        self.path(&e.path);
        syn::visit::visit_expr_path(self, e);
    }
    fn visit_expr_struct(&mut self, e: &'ast syn::ExprStruct) {
        self.path(&e.path);
        syn::visit::visit_expr_struct(self, e);
    }
    fn visit_expr_method_call(&mut self, e: &'ast syn::ExprMethodCall) {
        self.0.mentions.insert(e.method.to_string());
        syn::visit::visit_expr_method_call(self, e);
    }
    fn visit_expr_call(&mut self, e: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = &*e.func {
            let ids = path_idents(&p.path);
            if ids.len() == 2 && matches!(ids[0].as_str(), "Box" | "Arc" | "Rc" | "HelperFirst") && ids[1] == "new" {
                let mut names = TypeNames(BTreeSet::new());
                for a in &e.args {
                    names.visit_expr(a);
                }
                self.0.boxed.extend(names.0);
            }
        }
        syn::visit::visit_expr_call(self, e);
    }
}

impl Scan {
    fn function(&mut self, name: &syn::Ident, attrs: &[syn::Attribute], block: &syn::Block) {
        if is_test_attr(attrs) {
            return;
        }
        let mut info = FnInfo {
            name: name.to_string(),
            file: self.file.clone(),
            owner: self.owner.clone(),
            trait_impl: self.trait_impl.clone(),
            ..FnInfo::default()
        };
        Body(&mut info).visit_block(block);
        self.fns.push(info);
    }
}

impl<'ast> Visit<'ast> for Scan {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if !is_test_attr(&m.attrs) {
            syn::visit::visit_item_mod(self, m);
        }
    }
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.function(&f.sig.ident, &f.attrs, &f.block);
    }
    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        if is_test_attr(&i.attrs) {
            return;
        }
        let (owner, tr) = (self.owner.take(), self.trait_impl.take());
        self.owner = type_name(&i.self_ty);
        self.trait_impl = i
            .trait_
            .as_ref()
            .and_then(|(_, p, _)| p.segments.last().map(|s| s.ident.to_string()));
        syn::visit::visit_item_impl(self, i);
        self.owner = owner;
        self.trait_impl = tr;
    }
    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        self.function(&f.sig.ident, &f.attrs, &f.block);
    }
    fn visit_trait_item_fn(&mut self, f: &'ast syn::TraitItemFn) {
        if let Some(b) = &f.default {
            self.function(&f.sig.ident, &f.attrs, b);
        }
    }
}

/// Parses `(path, source)` pairs; an unparsable file is a problem, never skipped.
fn scan(sources: &[(String, String)]) -> Result<Vec<FnInfo>, String> {
    let mut all = Vec::new();
    for (path, text) in sources {
        let file = syn::parse_file(text).map_err(|e| format!("{path}: does not parse ({e}); the wiring scan cannot skip a file"))?;
        let mut s = Scan {
            file: path.clone(),
            ..Scan::default()
        };
        s.visit_file(&file);
        all.extend(s.fns);
    }
    Ok(all)
}

/// Files whose functions are the composition: the program starts here, so what they (transitively) name is reachable.
const ROOT_FILES: [&str; 3] = ["agent.rs", "main.rs", "lib.rs"];

/// Trait impls the composition reaches without a `Box::new` the scan can see. Each names the type and the trait (ADR 0052
/// section 1.1: the exemption names the type, never the trait alone): `tool_download::install` hands the link to the downloader as a
/// `&dyn ToolSink`, and `SharedHelper::new` holds a `LinkWakes` that the wake centre calls as a `WakeScheduler`.
const COMPOSED_NOT_BOXED: [(&str, &str); 2] = [("HelperLink", "ToolSink"), ("LinkWakes", "WakeScheduler")];

/// The typed `HelperCalls` methods are called by name from the readings only (`HelperReading::via_helper`): a method of the
/// same name elsewhere (`shared.processes`, `x.traffic()`) is not a reading using the helper. Callee trait, then the trait of
/// the only callers that count.
const ONLY_CALLED_FROM: [(&str, &str); 1] = [("HelperCalls", "HelperReading")];

/// For each `HelperRequest` kind name, whether a function that builds it is reachable from the composition by name: a root
/// function mentions it (or a function that mentions it, and so on), or, for a trait impl, the composition boxes the type.
fn wired_kinds(fns: &[FnInfo]) -> BTreeMap<String, bool> {
    let mut reachable: Vec<bool> = fns.iter().map(|f| ROOT_FILES.iter().any(|r| f.file.ends_with(r))).collect();
    loop {
        let mentioned: BTreeSet<&str> = fns
            .iter()
            .zip(&reachable)
            .filter(|(_, r)| **r)
            .flat_map(|(f, _)| f.mentions.iter().map(String::as_str))
            .collect();
        let boxed: BTreeSet<&str> = fns
            .iter()
            .zip(&reachable)
            .filter(|(_, r)| **r)
            .flat_map(|(f, _)| f.boxed.iter().map(String::as_str))
            .collect();
        let from_readings: BTreeSet<&str> = fns
            .iter()
            .zip(&reachable)
            .filter(|(f, r)| **r && f.trait_impl.as_deref() == Some("HelperReading"))
            .flat_map(|(f, _)| f.mentions.iter().map(String::as_str))
            .collect();
        let mut changed = false;
        for (i, f) in fns.iter().enumerate() {
            if reachable[i] {
                continue;
            }
            let ok = match (&f.trait_impl, &f.owner) {
                // A trait impl is reached when the composition builds the type as a trait object AND something reachable calls
                // the method by name: boxing `Bounded` does not make its `processes` a sender until a reading calls `processes`.
                (Some(tr), Some(x)) => {
                    let called = if ONLY_CALLED_FROM.iter().any(|(callee, _)| callee == tr) {
                        &from_readings
                    } else {
                        &mentioned
                    };
                    (boxed.contains(x.as_str()) || COMPOSED_NOT_BOXED.iter().any(|(t, c)| *t == x && *c == tr))
                        && called.contains(f.name.as_str())
                }
                _ => mentioned.contains(f.name.as_str()),
            };
            if ok {
                reachable[i] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut out: BTreeMap<String, bool> = BTreeMap::new();
    for (f, r) in fns.iter().zip(&reachable) {
        for k in &f.builds {
            *out.entry(k.clone()).or_insert(false) |= *r;
        }
    }
    out
}

// ---------------------------------------------------------------- wiring.toml

#[derive(Debug, Clone, PartialEq, Eq)]
struct Unwired {
    wire: String,
    kind: String,
    why: String,
    until: String,
}

/// Parses `wiring.toml` (ADR 0052 section 1.1); a malformed entry is a problem.
fn parse_wiring(text: &str) -> Result<Vec<Unwired>, String> {
    let v: toml::Table = text.parse().map_err(|e| format!("wiring.toml does not parse: {e}"))?;
    let mut out = Vec::new();
    for e in v.get("unwired").and_then(|u| u.as_array()).map_or(&[][..], |a| a.as_slice()) {
        let field = |k: &str| {
            e.get(k)
                .and_then(|x| x.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
                .ok_or_else(|| format!("wiring.toml: an [[unwired]] entry has no `{k}`"))
        };
        out.push(Unwired {
            wire: field("wire")?,
            kind: field("kind")?,
            why: field("why")?,
            until: field("until")?,
        });
    }
    Ok(out)
}

/// Days since 1970-01-01 of a civil date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `until` is `dyno-vX.Y.Z` or `YYYY-MM-DD`; a date before `today` (days since 1970) is expired.
fn until_problem(until: &str, today: i64) -> Option<String> {
    if let Some(v) = until.strip_prefix("dyno-v") {
        let ok = v.split('.').count() == 3 && v.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        return (!ok).then(|| format!("`until` \"{until}\" is not a release tag like dyno-v1.2.0 or a date"));
    }
    let parts: Vec<&str> = until.split('-').collect();
    let nums: Option<Vec<i64>> = parts.iter().map(|p| p.parse().ok()).collect();
    match (parts.len(), nums) {
        (3, Some(n)) if (1..=12).contains(&n[1]) && (1..=31).contains(&n[2]) => {
            (days_from_civil(n[0], n[1], n[2]) < today).then(|| format!("the waiver ended on {until}"))
        }
        _ => Some(format!("`until` \"{until}\" is not a release tag like dyno-v1.2.0 or a date")),
    }
}

// ---------------------------------------------------------------- the check

const WIRE: &str = "agent-helper";

/// All problems of the tree, each with the next step. Empty means the check passes.
fn check(fns: &[FnInfo], advertised: &BTreeSet<String>, unwired: &[Unwired], today: i64) -> Vec<String> {
    let wired = wired_kinds(fns);
    let mut problems = Vec::new();
    for kind in RequestKind::ALL {
        let name = format!("{kind:?}");
        let is_wired = wired.get(&name).copied().unwrap_or(false);
        let declared = unwired.iter().find(|u| u.wire == WIRE && u.kind == name);
        match (is_wired, advertised.contains(&name), declared) {
            (true, _, None) | (false, false, Some(_)) => {}
            (true, _, Some(_)) => problems.push(format!(
                "{name}: wired, but apps/dyno/wiring.toml still declares it unwired; delete the [[unwired]] entry"
            )),
            (false, true, d) => problems.push(format!(
                "{name}: advertised by readings() but no non-test code sends HelperRequest::{name} to a caller{}; wire it (WP in plan section 2) \
                 and do not declare an advertised reading unwired",
                if d.is_some() { " (and wiring.toml declares it unwired, which is not allowed for an advertised kind)" } else { "" }
            )),
            (false, false, None) => problems.push(format!(
                "{name}: not advertised and not sent by non-test code; wire it or add an [[unwired]] entry (wire = \"{WIRE}\", kind, why, until) \
                 to apps/dyno/wiring.toml"
            )),
        }
    }
    for u in unwired {
        if u.wire != WIRE {
            problems.push(format!(
                "wiring.toml: unknown wire \"{}\" (only \"{WIRE}\" exists for Dyno)",
                u.wire
            ));
        }
        if !RequestKind::ALL.iter().any(|k| format!("{k:?}") == u.kind) {
            problems.push(format!("wiring.toml: \"{}\" is not a HelperRequest kind", u.kind));
        }
        if let Some(p) = until_problem(&u.until, today) {
            problems.push(format!(
                "wiring.toml: {}: {p}; wire it or move the date with a Wiring-date trailer",
                u.kind
            ));
        }
    }
    problems
}

fn advertised_kinds() -> BTreeSet<String> {
    [HelperHost::Windows, HelperHost::Macos, HelperHost::Linux]
        .iter()
        .flat_map(|h| readings(*h).iter().flat_map(|a| a.kinds.iter().map(|k| format!("{k:?}"))))
        .collect()
}

// ---------------------------------------------------------------- the real tree

fn dyno_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if p.is_dir() {
            if name != "tests" {
                rust_files(&p, out);
            }
        } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
            out.push(p);
        }
    }
}

fn real_sources() -> Vec<(String, String)> {
    let root = dyno_root();
    let mut files = Vec::new();
    for c in ["dyno-agent", "dyno-platform"] {
        rust_files(&root.join("crates").join(c).join("src"), &mut files);
    }
    files
        .iter()
        .map(|p| (p.display().to_string(), std::fs::read_to_string(p).unwrap()))
        .collect()
}

fn today() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    (secs / 86_400) as i64
}

#[test]
#[ignore = "red until every advertised reading is wired (WP-FW, WP-PROC, WP-CNT, WP-LNX, WP-MEM, WP-BTM, WP-TRF, WP-EXT); WP-INT un-ignores it"]
fn every_advertised_reading_has_a_production_sender() {
    let fns = scan(&real_sources()).unwrap();
    let wiring = std::fs::read_to_string(dyno_root().join("wiring.toml")).expect("apps/dyno/wiring.toml");
    let problems = check(&fns, &advertised_kinds(), &parse_wiring(&wiring).unwrap(), today());
    assert!(
        problems.is_empty(),
        "{} helper request kind(s) are advertised or declared wrongly (name-level scan, not a call graph):\n- {}",
        problems.len(),
        problems.join("\n- ")
    );
}

// ---------------------------------------------------------------- doc claims

/// The lines `- <text>` between `<!-- helper-readings:start -->` and `<!-- helper-readings:end -->`.
fn claimed(doc: &str) -> Result<BTreeSet<String>, String> {
    let (_, rest) = doc
        .split_once("<!-- helper-readings:start -->")
        .ok_or("no `<!-- helper-readings:start -->` marker")?;
    let (block, _) = rest
        .split_once("<!-- helper-readings:end -->")
        .ok_or("no `<!-- helper-readings:end -->` marker")?;
    Ok(block
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- "))
        .map(|l| l.trim().to_string())
        .collect())
}

/// Problems between a document's claimed readings and the advertised ones.
fn claim_problems(name: &str, doc: &str, all: &[Advertised]) -> Vec<String> {
    let claimed = match claimed(doc) {
        Ok(c) => c,
        Err(e) => {
            return vec![format!(
                "{name}: {e}; list the advertised readings between the markers, one `- text` per line"
            )]
        }
    };
    let wanted: BTreeSet<String> = all.iter().map(|a| a.text.to_string()).collect();
    let mut out: Vec<String> = wanted
        .difference(&claimed)
        .map(|t| format!("{name}: does not mention \"{t}\""))
        .collect();
    out.extend(
        claimed
            .difference(&wanted)
            .map(|t| format!("{name}: claims \"{t}\", which readings() does not advertise")),
    );
    out
}

fn all_advertised() -> Vec<Advertised> {
    [HelperHost::Windows, HelperHost::Macos, HelperHost::Linux]
        .iter()
        .flat_map(|h| readings(*h).iter().copied())
        .collect()
}

#[test]
#[ignore = "red until WP-INT reconciles docs/user-guide.md and CHANGELOG.md with readings() (they are WP-INT's files)"]
fn the_user_guide_and_the_changelog_claim_what_readings_advertises() {
    let root = dyno_root();
    let mut problems = Vec::new();
    for file in ["docs/user-guide.md", "CHANGELOG.md"] {
        let doc = std::fs::read_to_string(root.join(file)).unwrap();
        problems.extend(claim_problems(file, &doc, &all_advertised()));
    }
    assert!(problems.is_empty(), "the docs differ from readings():\n- {}", problems.join("\n- "));
}

// ---------------------------------------------------------------- the checker proves it can fail

fn src(text: &str) -> Vec<FnInfo> {
    scan(&[("lib_like.rs".to_string(), text.to_string())]).unwrap()
}

/// `defs` in an ordinary file, `compose` in `agent.rs`, the composition root.
fn composed(defs: &str, compose: &str) -> Vec<FnInfo> {
    scan(&[
        ("fixture.rs".to_string(), defs.to_string()),
        ("agent.rs".to_string(), compose.to_string()),
    ])
    .unwrap()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

const LINK: &str = "struct L; impl L { fn firewall(&self) { let _ = HelperRequest::Firewall; } fn other(&self) { self.firewall(); } }";

#[test]
fn a_sender_nothing_reaches_is_not_wired() {
    let fns = composed(LINK, "fn compose() {}");
    assert_eq!(
        wired_kinds(&fns).get("Firewall"),
        Some(&false),
        "called only from its own impl, which nobody calls"
    );
}

#[test]
fn a_sender_the_composition_reaches_is_wired() {
    let fns = composed(LINK, "fn compose(l: &L) { l.firewall(); }");
    assert_eq!(wired_kinds(&fns).get("Firewall"), Some(&true));
}

#[test]
fn reaching_goes_through_a_chain_of_calls() {
    let fns = composed(
        "struct L; impl L { fn a(&self) { let _ = HelperRequest::Traffic; } } struct M; impl M { fn b(l: &L) { l.a(); } } fn c(l: &L) { M::b(l); }",
        "fn compose(l: &L) { c(l); }",
    );
    assert_eq!(wired_kinds(&fns).get("Traffic"), Some(&true));
}

#[test]
fn a_wrapper_that_no_root_reaches_does_not_lend_reachability() {
    // The shape of the seam: `Bounded::traffic` calls the link, but nothing composes a reading that calls `Bounded`.
    let fns = composed(
        "struct L; impl L { fn traffic_in(&self) { let _ = HelperRequest::Traffic; } } struct B; impl B { fn traffic(&self, l: &L) { l.traffic_in(); } }",
        "fn compose() {}",
    );
    assert_eq!(wired_kinds(&fns).get("Traffic"), Some(&false));
}

#[test]
fn a_sender_used_only_in_test_code_is_not_wired() {
    let fns = composed(
        "struct L; impl L { fn firewall(&self) { let _ = HelperRequest::Firewall; } }
         #[cfg(test)] mod tests { fn t(l: &super::L) { l.firewall(); } }",
        "fn compose() {}",
    );
    assert_eq!(wired_kinds(&fns).get("Firewall"), Some(&false));
    let fns = src("#[cfg(test)] fn only_test() { let _ = HelperRequest::Smart; }");
    assert!(
        !wired_kinds(&fns).contains_key("Smart"),
        "a test-only construction is not even a sender"
    );
}

#[test]
fn a_trait_impl_counts_only_when_the_composition_boxes_the_type() {
    let impl_ = "struct L; trait R { fn read(&self); } impl R for L { fn read(&self) { let _ = HelperRequest::Firewall; } } struct W; impl R for W { fn read(&self) {} } fn user(b: &dyn R) { b.read(); }";
    let not_built = composed(impl_, "fn compose() { let b: Box<dyn R> = Box::new(W); user(&*b); }");
    assert_eq!(
        wired_kinds(&not_built).get("Firewall"),
        Some(&false),
        "a second impl never lends the first its reachability"
    );
    let built = composed(impl_, "fn compose() { let b: Box<dyn R> = Box::new(L); user(&*b); }");
    assert_eq!(wired_kinds(&built).get("Firewall"), Some(&true));
    let in_first = composed(impl_, "fn compose() { let r = HelperFirst::new(L, port()); user(&r); }");
    assert_eq!(
        wired_kinds(&in_first).get("Firewall"),
        Some(&true),
        "a reading handed to the seam is composed"
    );
}

#[test]
fn a_boxed_type_does_not_make_every_trait_method_a_sender() {
    let defs = "struct B; trait Calls { fn processes(&self); fn memory(&self); }
        impl Calls for B { fn processes(&self) { let _ = HelperRequest::Processes; } fn memory(&self) { let _ = HelperRequest::MemoryModules; } }
        fn read_memory(c: &dyn Calls) { c.memory(); }";
    let fns = composed(defs, "fn compose() { let c: Box<dyn Calls> = Box::new(B); read_memory(&*c); }");
    let wired = wired_kinds(&fns);
    assert_eq!((wired.get("MemoryModules"), wired.get("Processes")), (Some(&true), Some(&false)));
}

#[test]
fn the_typed_helper_calls_count_only_when_a_reading_calls_them() {
    let defs = "struct B; trait HelperCalls { fn traffic(&self); } trait HelperReading { fn via_helper(&self, c: &dyn HelperCalls); }
        impl HelperCalls for B { fn traffic(&self) { let _ = HelperRequest::Traffic; } }
        struct R; impl HelperReading for R { fn via_helper(&self, c: &dyn HelperCalls) { c.traffic(); } }
        fn unrelated(x: &B) { x.traffic(); }";
    let by_other = composed(defs, "fn compose() { let c: Box<dyn HelperCalls> = Box::new(B); let _ = &c; }");
    assert_eq!(
        wired_kinds(&by_other).get("Traffic"),
        Some(&false),
        "a method of the same name elsewhere is not a reading"
    );
    let by_reading = composed(
        defs,
        "fn compose() { let c: Box<dyn HelperCalls> = Box::new(B); let r = HelperFirst::new(R, ()); r.via_helper(&*c); }",
    );
    assert_eq!(wired_kinds(&by_reading).get("Traffic"), Some(&true));
}

#[test]
fn an_unparsable_file_is_an_error_not_a_skip() {
    assert!(scan(&[("broken.rs".into(), "fn (".into())]).unwrap_err().contains("broken.rs"));
}

fn unwired(kind: &str, until: &str) -> Unwired {
    Unwired {
        wire: WIRE.into(),
        kind: kind.into(),
        why: "later".into(),
        until: until.into(),
    }
}

#[test]
fn the_check_reports_each_way_a_kind_can_be_wrong() {
    let none: Vec<FnInfo> = vec![];
    let adv = set(&["Firewall"]);
    let today = days_from_civil(2026, 11, 4);
    let all_declared: Vec<Unwired> = RequestKind::ALL
        .iter()
        .filter(|k| **k != RequestKind::Firewall)
        .map(|k| unwired(&format!("{k:?}"), "dyno-v1.2.0"))
        .collect();
    let p = check(&none, &adv, &all_declared, today);
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(p[0].starts_with("Firewall: advertised"), "{p:?}");
    // An advertised kind may not hide in wiring.toml.
    let mut with_fw = all_declared.clone();
    with_fw.push(unwired("Firewall", "dyno-v1.2.0"));
    assert!(check(&none, &adv, &with_fw, today)[0].contains("not allowed"));
    // An unadvertised, unsent kind needs an entry.
    let p = check(&none, &set(&[]), &[], today);
    assert_eq!(p.len(), RequestKind::ALL.len());
    assert!(p[0].contains("[[unwired]]"));
    // A stale entry (the kind is wired now) is a problem.
    let fns = composed(
        "struct L; impl L { fn f(&self) { let _ = HelperRequest::LogTail(1); } }",
        "fn compose(l: &L) { l.f(); }",
    );
    let p = check(&fns, &set(&[]), &[unwired("LogTail", "dyno-v1.2.0")], today);
    assert!(p.iter().any(|m| m.starts_with("LogTail: wired, but")), "{p:?}");
}
#[test]
fn waivers_are_bounded() {
    let today = days_from_civil(2026, 11, 4);
    assert_eq!(until_problem("dyno-v1.2.0", today), None);
    assert_eq!(until_problem("2026-12-01", today), None);
    assert!(until_problem("2026-11-03", today).unwrap().contains("ended"));
    assert!(until_problem("someday", today).is_some());
    assert!(until_problem("dyno-v1.2", today).is_some());
    assert!(parse_wiring("[[unwired]]\nwire = \"agent-helper\"\nkind = \"LogTail\"\n")
        .unwrap_err()
        .contains("`why`"));
}

#[test]
fn wiring_toml_of_the_tree_parses_and_names_real_kinds() {
    let text = std::fs::read_to_string(dyno_root().join("wiring.toml")).expect("apps/dyno/wiring.toml");
    let entries = parse_wiring(&text).unwrap();
    assert!(!entries.is_empty());
    for e in &entries {
        assert!(RequestKind::ALL.iter().any(|k| format!("{k:?}") == e.kind), "{e:?}");
        assert!(
            !advertised_kinds().contains(&e.kind),
            "{}: an advertised kind may not be declared unwired",
            e.kind
        );
    }
}

#[test]
fn the_claims_check_names_what_differs() {
    let list = all_advertised();
    let mut doc = String::from("x\n<!-- helper-readings:start -->\n");
    let texts: BTreeSet<&str> = list.iter().map(|a| a.text).collect();
    for t in &texts {
        doc.push_str(&format!("- {t}\n"));
    }
    doc.push_str("<!-- helper-readings:end -->\n");
    assert!(claim_problems("doc", &doc, &list).is_empty());
    let missing = doc.replace("- Memory modules\n", "");
    assert_eq!(
        claim_problems("doc", &missing, &list),
        vec!["doc: does not mention \"Memory modules\"".to_string()]
    );
    let extra = doc.replace("- Memory modules\n", "- Memory modules\n- Fan curves\n");
    assert!(claim_problems("doc", &extra, &list)[0].contains("claims \"Fan curves\""));
    assert!(claim_problems("doc", "no markers", &list)[0].contains("marker"));
}
