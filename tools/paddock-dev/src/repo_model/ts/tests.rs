//! The oxc import extractor with its positive controls (ADR 0117 §8) and the determinism test (§4, review P1-7).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

fn set(v: &[&str]) -> BTreeSet<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn resolver(files: &[&str], texts: &[(&str, &str)], packages: &[(&str, &str)]) -> Resolver {
    Resolver::new(
        &set(files),
        texts.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect(),
        packages.iter().map(|(n, d)| (n.to_string(), d.to_string())).collect(),
    )
}

#[test]
fn every_import_form_is_a_specifier_and_comments_and_strings_are_not() {
    let text = r#"
import { a } from './a';
import type { T } from "./types";
import './side-effect';
export * from './b.js';
export { c } from '../c';
import fs = require('./legacy');
const d = await import('./d');
const e = require(`./e`);
type X = import('./x-type').X;
const dyn = await import(name);
const fs = await import(/* @vite-ignore */ ['node', 'fs'].join(':'));
const joined = await import('./' + 'joined');
// import y from './commented';
/* export * from './block' */
const s = "import z from './in-a-string'";
import pkg from '@scope/pkg/sub';
"#;
    let s = specifiers("x/f.ts", text).unwrap();
    assert_eq!(
        s.literal,
        [
            "./a",
            "./types",
            "./side-effect",
            "./b.js",
            "../c",
            "./legacy",
            "./d",
            "./e",
            "./x-type",
            "node:fs",
            "./joined",
            "@scope/pkg/sub"
        ]
    );
    assert_eq!(s.dynamic, 1);
}

#[test]
fn a_file_the_parser_gives_up_on_is_an_error_not_an_empty_list() {
    assert!(specifiers("x/f.ts", "import { from './a'; }}}} (((").is_err());
    // Control: the same file fixed parses.
    assert!(specifiers("x/f.ts", "import { a } from './a';").is_ok());
}

#[test]
fn specifiers_resolve_over_tracked_files_extensions_indexes_and_the_js_alias() {
    let r = resolver(
        &[
            "app/e2e/helper.ts",
            "app/e2e/lib/index.ts",
            "app/src/x.ts",
            "app/e2e/a.spec.ts",
            "conformance/src/run.mjs",
        ],
        &[],
        &[],
    );
    let from = "app/e2e/a.spec.ts";
    assert_eq!(r.resolve(from, "./helper"), Some(Target::File("app/e2e/helper.ts".into())));
    assert_eq!(r.resolve(from, "./helper.js"), Some(Target::File("app/e2e/helper.ts".into())));
    assert_eq!(r.resolve(from, "./lib"), Some(Target::File("app/e2e/lib/index.ts".into())));
    assert_eq!(r.resolve(from, "../src/x"), Some(Target::File("app/src/x.ts".into())));
    assert_eq!(
        r.resolve("conformance/bin/cli.mjs", "../src/run.mjs"),
        Some(Target::File("conformance/src/run.mjs".into()))
    );
    // Unresolved: a relative specifier naming no tracked file.
    assert_eq!(r.resolve(from, "./missing"), None);
    assert_eq!(r.resolve(from, "node:fs"), Some(Target::Builtin));
    assert_eq!(r.resolve(from, "path"), Some(Target::Builtin));
}

#[test]
fn a_workspace_package_resolves_by_name_without_node_modules_and_an_outside_one_is_external() {
    let r = resolver(&["ui/package.json", "app/e2e/a.spec.ts"], &[], &[("@decryp7/paddock-ui", "ui")]);
    let from = "app/e2e/a.spec.ts";
    assert_eq!(r.resolve(from, "@decryp7/paddock-ui"), Some(Target::Package("ui".into())));
    assert_eq!(r.resolve(from, "@decryp7/paddock-ui/testing"), Some(Target::Package("ui".into())));
    assert_eq!(
        r.resolve(from, "@playwright/test"),
        Some(Target::External("@playwright/test".into()))
    );
    assert_eq!(r.resolve(from, "rxjs/operators"), Some(Target::External("rxjs".into())));
    // Control: without the package in the npm graph the same name is external.
    let bare = resolver(&["ui/package.json", "app/e2e/a.spec.ts"], &[], &[]);
    assert_eq!(
        bare.resolve(from, "@decryp7/paddock-ui"),
        Some(Target::External("@decryp7/paddock-ui".into()))
    );
}

#[test]
fn tsconfig_paths_resolve_through_the_tracked_config() {
    let tsconfig = r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@app/*": ["src/app/*"] } } }"#;
    let files = ["app/tsconfig.json", "app/src/app/shell.ts", "app/src/main.ts"];
    let r = resolver(&files, &[("app/tsconfig.json", tsconfig)], &[]);
    assert_eq!(
        r.resolve("app/src/main.ts", "@app/shell"),
        Some(Target::File("app/src/app/shell.ts".into()))
    );
    // Control: without the config the alias is an external package name.
    let none = resolver(&files, &[], &[]);
    assert_eq!(
        none.resolve("app/src/main.ts", "@app/shell"),
        Some(Target::External("@app/shell".into()))
    );
}

/// Writes `files` under a temporary folder and returns it.
fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (p, t) in files {
        let path = d.path().join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, t).unwrap();
    }
    d
}

#[test]
fn the_graph_is_the_same_with_and_without_node_modules_on_disk() {
    // Determinism (review P1-7): CI's plan job has no node_modules, a developer's checkout has them. The resolver sees only the
    // tracked files, so both give the same graph.
    let tracked: &[(&str, &str)] = &[
        ("package.json", r#"{ "name": "root", "workspaces": ["ui", "app"] }"#),
        ("ui/package.json", r#"{ "name": "@decryp7/paddock-ui" }"#),
        (
            "app/e2e/a.spec.ts",
            "import { x } from '@decryp7/paddock-ui';\nimport pad from 'left-pad';\nimport { h } from './h';\n",
        ),
        ("app/e2e/h.ts", "export const h = 1;\n"),
    ];
    let untracked: &[(&str, &str)] = &[
        (
            "node_modules/left-pad/package.json",
            r#"{ "name": "left-pad", "main": "index.js" }"#,
        ),
        ("node_modules/left-pad/index.js", "module.exports = 1;\n"),
        (
            "node_modules/@decryp7/paddock-ui/package.json",
            r#"{ "name": "@decryp7/paddock-ui", "main": "x.js" }"#,
        ),
        ("node_modules/@decryp7/paddock-ui/x.js", "module.exports = 1;\n"),
    ];
    let graph_of = |dir: &Path| {
        let files: BTreeSet<String> = tracked.iter().map(|(p, _)| p.to_string()).collect();
        let texts: BTreeMap<String, String> = tracked
            .iter()
            .filter(|(p, _)| resolver_reads(p))
            .map(|(p, t)| (p.to_string(), t.to_string()))
            .collect();
        let r = Resolver::new(
            &files,
            texts,
            BTreeMap::from([("@decryp7/paddock-ui".to_string(), "ui".to_string())]),
        );
        let read = |paths: &[String]| paths.iter().map(|p| std::fs::read_to_string(dir.join(p)).ok()).collect();
        import_graph(&["app/e2e/a.spec.ts".to_string()], &r, &read, &|_| true)
    };
    let without = tree(tracked);
    let with: Vec<(&str, &str)> = tracked.iter().chain(untracked).copied().collect();
    let with = tree(&with);
    let a = graph_of(without.path());
    let b = graph_of(with.path());
    assert_eq!(a, b);
    assert_eq!(
        a.edges["app/e2e/a.spec.ts"],
        BTreeSet::from([
            Target::File("app/e2e/h.ts".into()),
            Target::Package("ui".into()),
            Target::External("left-pad".into())
        ])
    );
    // Positive control: oxc's resolver on the real disk does see node_modules, so the test above can tell the difference.
    let os = oxc_resolver::Resolver::new(ResolveOptions::default());
    let disk = os.resolve(with.path().join("app/e2e"), "left-pad").map(|r| r.path().to_path_buf());
    assert!(disk.is_ok_and(|p| p.ends_with("node_modules/left-pad/index.js")));
    assert!(os.resolve(without.path().join("app/e2e"), "left-pad").is_err());
}

#[test]
fn the_walk_follows_imports_transitively_records_unresolved_and_the_control_drops_the_edge() {
    let texts: BTreeMap<&str, &str> = BTreeMap::from([
        ("e2e/a.spec.ts", "import { h } from './fixtures';\nimport { g } from './gone';\n"),
        ("e2e/fixtures.ts", "export * from './deep/h';"),
        ("e2e/deep/h.ts", "export const h = 1;"),
        ("e2e/broken.ts", "export const = ;;; {{{"),
        ("e2e/b.spec.ts", "import './broken';"),
    ]);
    let files: BTreeSet<String> = texts.keys().map(|s| s.to_string()).collect();
    let r = Resolver::new(&files, BTreeMap::new(), BTreeMap::new());
    let read = |paths: &[String]| paths.iter().map(|p| texts.get(p.as_str()).map(|t| t.to_string())).collect();
    let g = import_graph(&["e2e/a.spec.ts".to_string(), "e2e/b.spec.ts".to_string()], &r, &read, &|_| true);
    assert!(g.edges["e2e/a.spec.ts"].contains(&Target::File("e2e/fixtures.ts".into())));
    assert!(g.edges["e2e/fixtures.ts"].contains(&Target::File("e2e/deep/h.ts".into())));
    assert_eq!(g.unresolved["e2e/a.spec.ts"], ["./gone"]);
    assert!(g.errors.contains_key("e2e/broken.ts"));
    assert_eq!(g.reads, 3);
    // `follow` stops the walk: a file it refuses is an edge target but is not read.
    let shallow = import_graph(&["e2e/a.spec.ts".to_string()], &r, &read, &|p| !p.contains("fixtures"));
    assert!(!shallow.edges.contains_key("e2e/fixtures.ts"));
    // Positive control: without the import line the edge is gone.
    let texts2: BTreeMap<&str, &str> = BTreeMap::from([("e2e/a.spec.ts", "const x = 1;"), ("e2e/fixtures.ts", "")]);
    let read2 = |paths: &[String]| paths.iter().map(|p| texts2.get(p.as_str()).map(|t| t.to_string())).collect();
    let g2 = import_graph(&["e2e/a.spec.ts".to_string()], &r, &read2, &|_| true);
    assert!(g2.edges["e2e/a.spec.ts"].is_empty());
}

#[test]
fn package_names_keep_the_scope() {
    assert_eq!(package_name("@scope/name/sub/x"), "@scope/name");
    assert_eq!(package_name("name/sub"), "name");
    assert_eq!(package_name("@scope"), "@scope");
}
