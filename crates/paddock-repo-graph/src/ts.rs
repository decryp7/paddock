//! TypeScript and JavaScript imports through oxc (ADR 0117 §4 `imports`, §10; the `ts` feature): `oxc_parser` reads every import
//! form of a file (static `import`/`export ... from`, `import x = require(..)`, type imports, `import('..')` and `require('..')`),
//! and `oxc_resolver` resolves the specifiers over the **tracked files only**: its file system is the tracked set and the texts
//! the caller read in one bulk call, never the disk, so a checkout with `node_modules` (a developer's) and one without (CI's plan
//! job) give the same graph (review P1-7).
//!
//! - A bare specifier that names a workspace package (`@decryp7/paddock-ui`, `@decryp7/paddock-ui/testing`) is a package-level
//!   edge to that package's folder, from the npm graph (the caller passes name to folder): no `node_modules` link is needed.
//! - Any other bare specifier is an external package: a package-level edge by name (its version lives in `package-lock.json`,
//!   which the `npm-lock` content extractor narrows). A Node built-in (`node:fs`, `path`) is no edge.
//! - A relative or absolute specifier that resolves to no tracked file is **unresolved**: the caller cannot prove what the
//!   importer depends on and runs the importer's project in full (§4). So is a dynamic `import()` or `require()` whose argument is
//!   not a string literal.
//! - A file the parser cannot read is an error the caller turns into "full" for its project (§2, review round 2 P2-6).
//!
//! The text scanner [`crate::imports`] stays the fallback a build without this feature uses (§10).

use crate::paths::{dirname, join_normalized};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, CallExpression, Expression, ImportExpression, TSImportType, TSModuleReference,
};
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_resolver::{
    FileMetadata, FileSystem, ResolveError, ResolveOptions, ResolverGeneric, TsconfigDiscovery,
};
use oxc_span::SourceType;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// The extensions a specifier without one may name, in the order TypeScript's bundler resolution tries them.
const EXTENSIONS: [&str; 9] = [
    ".ts", ".tsx", ".mts", ".cts", ".d.ts", ".js", ".mjs", ".cjs", ".json",
];

/// The files whose text the resolver itself reads (package manifests and TypeScript configs): [`Resolver::wants`] names them so the
/// caller reads them in its one bulk call.
pub fn resolver_reads(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == "package.json"
        || (name.starts_with("tsconfig") && name.ends_with(".json"))
        || name == "jsconfig.json"
}

/// What one specifier names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    /// A tracked file (repository path).
    File(String),
    /// A workspace package (its folder).
    Package(String),
    /// A package outside the workspace (its name).
    External(String),
    /// A Node built-in module: no edge.
    Builtin,
}

/// The import specifiers of one source file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Specifiers {
    /// Every string-literal specifier, in source order, without duplicates.
    pub literal: Vec<String>,
    /// Dynamic `import()` and `require()` calls whose argument is not a string literal (cannot be resolved).
    pub dynamic: usize,
}

struct Collect<'s> {
    out: &'s mut Specifiers,
}

impl Collect<'_> {
    /// A literal specifier; `None` (a string oxc cannot give as UTF-8) counts as one that cannot be resolved.
    fn push(&mut self, s: Option<&str>) {
        match s {
            Some(s) if !self.out.literal.iter().any(|x| x == s) => {
                self.out.literal.push(s.to_string())
            }
            Some(_) => {}
            None => self.out.dynamic += 1,
        }
    }

    fn expr(&mut self, e: &Expression) {
        match fold(e) {
            Some(s) => self.push(Some(&s)),
            None => self.out.dynamic += 1,
        }
    }
}

/// The value of a specifier built from literals only: a string, a template without expressions, `'a' + 'b'`, and
/// `['node', 'fs'].join(':')` (the spelling specs use to hide a Node built-in from the bundler). Anything else is computed.
fn fold(e: &Expression) -> Option<String> {
    match e {
        Expression::StringLiteral(s) => s.value.as_str().map(str::to_string),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => t
            .quasis[0]
            .value
            .cooked
            .as_ref()
            .and_then(|c| c.as_str())
            .map(str::to_string),
        Expression::ParenthesizedExpression(p) => fold(&p.expression),
        Expression::BinaryExpression(b) if b.operator == oxc_ast::ast::BinaryOperator::Addition => {
            Some(format!("{}{}", fold(&b.left)?, fold(&b.right)?))
        }
        Expression::CallExpression(c) => {
            let Expression::StaticMemberExpression(m) = &c.callee else {
                return None;
            };
            let Expression::ArrayExpression(a) = &m.object else {
                return None;
            };
            if m.property.name != "join" || c.arguments.len() > 1 {
                return None;
            }
            let sep = match c.arguments.first() {
                None => ",".to_string(),
                Some(Argument::StringLiteral(s)) => s.value.as_str()?.to_string(),
                Some(_) => return None,
            };
            let parts: Option<Vec<String>> = a
                .elements
                .iter()
                .map(|el| match el {
                    oxc_ast::ast::ArrayExpressionElement::StringLiteral(s) => {
                        s.value.as_str().map(str::to_string)
                    }
                    _ => None,
                })
                .collect();
            Some(parts?.join(&sep))
        }
        _ => None,
    }
}

impl<'a> Visit<'a> for Collect<'_> {
    fn visit_import_declaration(&mut self, it: &oxc_ast::ast::ImportDeclaration<'a>) {
        self.push(it.source.value.as_str());
    }

    fn visit_export_all_declaration(&mut self, it: &oxc_ast::ast::ExportAllDeclaration<'a>) {
        self.push(it.source.value.as_str());
    }

    fn visit_export_from_declaration(&mut self, it: &oxc_ast::ast::ExportFromDeclaration<'a>) {
        self.push(it.source.value.as_str());
    }

    fn visit_import_expression(&mut self, it: &ImportExpression<'a>) {
        self.expr(&it.source);
        oxc_ast_visit::walk::walk_import_expression(self, it);
    }

    fn visit_ts_import_type(&mut self, it: &TSImportType<'a>) {
        self.push(it.source.value.as_str());
    }

    fn visit_ts_import_equals_declaration(
        &mut self,
        it: &oxc_ast::ast::TSImportEqualsDeclaration<'a>,
    ) {
        if let TSModuleReference::ExternalModuleReference(r) = &it.module_reference {
            self.push(r.expression.value.as_str());
        }
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Expression::Identifier(id) = &it.callee
            && id.name == "require"
            && it.arguments.len() == 1
        {
            match &it.arguments[0] {
                Argument::StringLiteral(s) => self.push(s.value.as_str()),
                Argument::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => {
                    match &t.quasis[0].value.cooked {
                        Some(c) => self.push(c.as_str()),
                        None => self.out.dynamic += 1,
                    }
                }
                _ => self.out.dynamic += 1,
            }
        }
        oxc_ast_visit::walk::walk_call_expression(self, it);
    }
}

/// The specifiers of a source text. `Err`: the parser gave up (a fatal error), so the file's imports are not known.
pub fn specifiers(path: &str, text: &str) -> Result<Specifiers, String> {
    let source_type = SourceType::from_path(path).map_err(|e| e.to_string())?;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, text, source_type).parse();
    if parsed.fatal_error {
        let first = parsed
            .diagnostics
            .iter()
            .next()
            .map(|d| d.to_string())
            .unwrap_or_default();
        return Err(format!("oxc cannot parse it: {first}"));
    }
    let mut out = Specifiers::default();
    Collect { out: &mut out }.visit_program(&parsed.program);
    Ok(out)
}

/// Whether a path is a source the parser reads.
pub fn is_source(path: &str) -> bool {
    [".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"]
        .iter()
        .any(|e| path.ends_with(e))
}

/// The resolver's file system: the tracked files under a virtual root, and the texts of the package manifests and TypeScript
/// configs the caller read. Nothing on disk is ever consulted.
#[derive(Clone, Default)]
struct Tracked {
    root: PathBuf,
    files: Arc<BTreeSet<String>>,
    dirs: Arc<BTreeSet<String>>,
    texts: Arc<BTreeMap<String, String>>,
}

impl Tracked {
    fn rel(&self, path: &Path) -> Option<String> {
        let rest = path.strip_prefix(&self.root).ok()?;
        let mut parts: Vec<String> = Vec::new();
        for c in rest.components() {
            match c {
                Component::Normal(s) => parts.push(s.to_str()?.to_string()),
                Component::CurDir => {}
                Component::ParentDir => {
                    parts.pop()?;
                }
                _ => return None,
            }
        }
        Some(parts.join("/"))
    }

    fn missing(path: &Path) -> io::Error {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} is not tracked", path.display()),
        )
    }
}

impl FileSystem for Tracked {
    fn new() -> Self {
        Tracked::default()
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.read_to_string(path).map(String::into_bytes)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.rel(path)
            .and_then(|r| self.texts.get(&r).cloned())
            .ok_or_else(|| Tracked::missing(path))
    }

    fn metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        let r = self.rel(path).ok_or_else(|| Tracked::missing(path))?;
        if self.files.contains(&r) {
            Ok(FileMetadata::new(true, false, false))
        } else if r.is_empty() || self.dirs.contains(&r) {
            Ok(FileMetadata::new(false, true, false))
        } else {
            Err(Tracked::missing(path))
        }
    }

    fn symlink_metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        self.metadata(path)
    }

    fn read_link(&self, path: &Path) -> Result<PathBuf, ResolveError> {
        Err(ResolveError::from(Tracked::missing(path)))
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        let r = self.rel(path).ok_or_else(|| Tracked::missing(path))?;
        Ok(self.root.join(r))
    }
}

/// The virtual root the resolver sees (an absolute path on every OS; never a real folder).
fn virtual_root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("C:\\paddock-tracked")
    } else {
        PathBuf::from("/paddock-tracked")
    }
}

/// The Node built-ins (`node:` or a bare name that Node ships).
fn builtin(spec: &str) -> bool {
    const NODE: [&str; 40] = [
        "assert",
        "async_hooks",
        "buffer",
        "child_process",
        "cluster",
        "console",
        "constants",
        "crypto",
        "dgram",
        "dns",
        "domain",
        "events",
        "fs",
        "fs/promises",
        "http",
        "http2",
        "https",
        "inspector",
        "module",
        "net",
        "os",
        "path",
        "path/posix",
        "perf_hooks",
        "process",
        "punycode",
        "querystring",
        "readline",
        "repl",
        "stream",
        "stream/promises",
        "string_decoder",
        "test",
        "timers",
        "tls",
        "tty",
        "url",
        "util",
        "v8",
        "zlib",
    ];
    spec.starts_with("node:")
        || NODE.contains(&spec)
        || NODE
            .iter()
            .any(|n| spec.strip_prefix(n).is_some_and(|r| r.starts_with('/')))
}

/// The package name of a bare specifier (`@scope/name/sub` is `@scope/name`, `name/sub` is `name`).
pub fn package_name(spec: &str) -> &str {
    let mut parts = spec.splitn(3, '/');
    let first = parts.next().unwrap_or(spec);
    if first.starts_with('@') {
        match parts.next() {
            Some(second) => &spec[..first.len() + 1 + second.len()],
            None => first,
        }
    } else {
        first
    }
}

/// Resolves specifiers over the tracked files (see the module docs).
pub struct Resolver {
    tracked: Tracked,
    with_tsconfig: ResolverGeneric<Tracked>,
    plain: ResolverGeneric<Tracked>,
    packages: BTreeMap<String, String>,
}

impl Resolver {
    /// A resolver over `files` (every tracked file), `texts` (at least every file [`resolver_reads`] names) and `packages`
    /// (workspace package name to its folder).
    pub fn new(
        files: &BTreeSet<String>,
        texts: BTreeMap<String, String>,
        packages: BTreeMap<String, String>,
    ) -> Resolver {
        let mut dirs = BTreeSet::new();
        for f in files {
            let mut d = dirname(f);
            while d != "." && d != "/" && dirs.insert(d.clone()) {
                d = dirname(&d);
            }
        }
        let tracked = Tracked {
            root: virtual_root(),
            files: Arc::new(files.clone()),
            dirs: Arc::new(dirs),
            texts: Arc::new(texts),
        };
        let options = |tsconfig: Option<TsconfigDiscovery>| ResolveOptions {
            extensions: EXTENSIONS.iter().map(|e| e.to_string()).collect(),
            // TypeScript's ESM style: `./x.js` names `./x.ts`.
            extension_alias: vec![
                (
                    ".js".into(),
                    vec![".ts".into(), ".tsx".into(), ".js".into()],
                ),
                (".mjs".into(), vec![".mts".into(), ".mjs".into()]),
                (".cjs".into(), vec![".cts".into(), ".cjs".into()]),
            ],
            condition_names: vec![
                "import".into(),
                "require".into(),
                "node".into(),
                "types".into(),
                "default".into(),
            ],
            main_fields: vec!["module".into(), "main".into(), "types".into()],
            builtin_modules: true,
            symlinks: false,
            tsconfig,
            ..ResolveOptions::default()
        };
        Resolver {
            with_tsconfig: ResolverGeneric::new_with_file_system(
                tracked.clone(),
                options(Some(TsconfigDiscovery::Auto)),
            ),
            plain: ResolverGeneric::new_with_file_system(tracked.clone(), options(None)),
            tracked,
            packages,
        }
    }

    /// What `spec`, imported by the tracked file `from`, names; `None` when a relative or absolute specifier names no tracked file
    /// (unresolved).
    pub fn resolve(&self, from: &str, spec: &str) -> Option<Target> {
        if builtin(spec) {
            return Some(Target::Builtin);
        }
        let relative = spec.starts_with("./")
            || spec.starts_with("../")
            || spec == "."
            || spec == ".."
            || spec.starts_with('/');
        if !relative && let Some(dir) = self.packages.get(package_name(spec)) {
            return Some(Target::Package(dir.clone()));
        }
        let file = self.tracked.root.join(from);
        // A TypeScript config that cannot be read (an `extends` of a package in node_modules) must not hide a plain relative
        // import: the plain resolver answers then.
        let hit = self
            .with_tsconfig
            .resolve_file(&file, spec)
            .or_else(|_| self.plain.resolve_file(&file, spec))
            .ok()
            .and_then(|r| self.tracked.rel(r.path()));
        match hit {
            Some(p) if self.tracked.files.contains(&p) => Some(Target::File(p)),
            // A tsconfig path into a workspace package's folder (`ui/dist` mapped to the package).
            Some(p) => self
                .packages
                .values()
                .filter(|d| {
                    p.strip_prefix(d.as_str())
                        .is_some_and(|r| r.starts_with('/'))
                })
                .max_by_key(|d| d.len())
                .map(|d| Target::Package(d.clone()))
                .or_else(|| (!relative).then(|| Target::External(package_name(spec).to_string()))),
            None if relative => {
                // `./x` to a folder holding `index.*`, or a plain extension the resolver did not try.
                let base = join_normalized(&dirname(from), spec)?;
                EXTENSIONS
                    .iter()
                    .map(|e| format!("{base}{e}"))
                    .chain(EXTENSIONS.iter().map(|e| format!("{base}/index{e}")))
                    .find(|t| self.tracked.files.contains(t))
                    .map(Target::File)
            }
            None => Some(Target::External(package_name(spec).to_string())),
        }
    }
}

/// The import graph of a set of files: each file to what it imports, and what could not be read or resolved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportGraph {
    /// Each followed file to its resolved targets.
    pub edges: BTreeMap<String, BTreeSet<Target>>,
    /// Each file with specifiers that named no tracked file (or dynamic imports that are not literals), with those specifiers.
    pub unresolved: BTreeMap<String, Vec<String>>,
    /// Files the parser could not read, with why.
    pub errors: BTreeMap<String, String>,
    /// How many bulk reads the walk made.
    pub reads: usize,
}

/// Follows imports from `seeds` through every resolved file for which `follow` says yes, reading texts layer by layer through
/// `read` (one bulk call per layer; the caller's reader keeps one process for all of them, ER-20). Deterministic: sorted maps, and
/// the walk depends only on the tracked set and the texts.
pub fn import_graph(
    seeds: &[String],
    resolver: &Resolver,
    read: &dyn Fn(&[String]) -> Vec<Option<String>>,
    follow: &dyn Fn(&str) -> bool,
) -> ImportGraph {
    let mut g = ImportGraph::default();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut layer: Vec<String> = seeds
        .iter()
        .filter(|s| is_source(s))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    while !layer.is_empty() {
        seen.extend(layer.iter().cloned());
        g.reads += 1;
        let texts = read(&layer);
        let mut next: BTreeSet<String> = BTreeSet::new();
        for (f, text) in layer.iter().zip(texts) {
            let Some(text) = text else {
                g.errors.insert(f.clone(), "not readable as text".into());
                continue;
            };
            let specs = match specifiers(f, &text) {
                Ok(s) => s,
                Err(e) => {
                    g.errors.insert(f.clone(), e);
                    continue;
                }
            };
            let mut to = BTreeSet::new();
            let mut bad: Vec<String> = Vec::new();
            if specs.dynamic > 0 {
                bad.push(format!(
                    "{} dynamic import(s) with a computed specifier",
                    specs.dynamic
                ));
            }
            for s in &specs.literal {
                match resolver.resolve(f, s) {
                    Some(Target::Builtin) => {}
                    Some(t) => {
                        if let Target::File(p) = &t
                            && !seen.contains(p)
                            && is_source(p)
                            && follow(p)
                        {
                            next.insert(p.clone());
                        }
                        to.insert(t);
                    }
                    None => bad.push(s.clone()),
                }
            }
            if !bad.is_empty() {
                g.unresolved.insert(f.clone(), bad);
            }
            g.edges.insert(f.clone(), to);
        }
        layer = next.into_iter().collect();
    }
    g
}

#[cfg(test)]
mod tests;
