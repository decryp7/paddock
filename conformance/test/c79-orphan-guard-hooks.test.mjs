import assert from 'node:assert/strict';
import { test } from 'node:test';
import C79, { ADOPTING, binPaths, callsFirst, devOnly, guardedMains, lockGraph, mainState, reaches } from '../src/checks/C79-orphan-guard-hooks.mjs';

const LOCK = `version = 4

[[package]]
name = "app"
version = "0.1.0"
dependencies = [
 "mid",
 "serde 1.0.0",
]

[[package]]
name = "mid"
version = "0.1.0"
dependencies = [
 "paddock-proc",
]

[[package]]
name = "paddock-proc"
version = "0.1.0"

[[package]]
name = "plain"
version = "0.1.0"
`;

const tree = (files) => ({
  files: () => Object.keys(files),
  read: (p) => files[p] ?? null,
});

const HOOK = 'fn main() {\n    paddock_proc::guardian::run_if_requested();\n}\n';
const BARE = 'fn main() {}\n';

test('C79 reads a Cargo.lock graph and follows it to paddock-proc', () => {
  const g = lockGraph(LOCK);
  assert.deepEqual(g.app, ['mid', 'serde']);
  assert.ok(reaches(g, 'app') && reaches(g, 'mid'));
  assert.ok(!reaches(g, 'plain') && !reaches(g, 'serde'));
});

test('C79 finds the mains that reach paddock-proc, hooked or not', () => {
  const files = {
    'w/Cargo.lock': LOCK,
    'w/app/Cargo.toml': '[package]\nname = "app"\n',
    'w/app/src/main.rs': HOOK,
    'w/app/src/bin/tool.rs': BARE,
    'w/app/src/bin/two/main.rs': BARE,
    'w/app/tests/it/main.rs': BARE,
    'w/plain/Cargo.toml': '[package]\nname = "plain"\n',
    'w/plain/src/main.rs': BARE,
  };
  const t = tree(files);
  assert.deepEqual(guardedMains(t.files(), t.read), [
    { file: 'w/app/src/main.rs', hooked: true, wrapped: false },
    { file: 'w/app/src/bin/tool.rs', hooked: false, wrapped: false },
    { file: 'w/app/src/bin/two/main.rs', hooked: false, wrapped: false },
  ]);
});

test('C79: a missing hook is an error, an adopting one a warning, and the list only shrinks', () => {
  const files = {
    'Cargo.lock': LOCK,
    'Cargo.toml': '[package]\nname = "app"\n',
    'src/main.rs': BARE,
    'src/bin/listed.rs': BARE,
    'src/bin/done.rs': HOOK,
  };
  const adopting = new Map([
    ['src/bin/listed.rs', 'adopts later'],
    ['src/bin/done.rs', 'adopts later'],
  ]);
  const f = C79.run(tree(files), adopting);
  const by = Object.fromEntries(f.map((x) => [x.file, x.severity]));
  assert.deepEqual(by, { 'src/main.rs': 'error', 'src/bin/listed.rs': 'warn', 'src/bin/done.rs': 'error' });
});

test('C79 ADOPTING names only Rust files with a reason', () => {
  for (const [file, why] of ADOPTING) {
    assert.match(file, /\.rs$/);
    assert.ok(why.length > 5);
  }
});

test('C79 matches a listed main from an app folder (paths relative to the checked root)', () => {
  const files = { 'Cargo.lock': LOCK, 'Cargo.toml': '[package]\nname = "app"\n', 'src/main.rs': BARE };
  const f = C79.run(tree(files), new Map([['apps/x/src/main.rs', 'adopts later']]));
  assert.deepEqual(f.map((x) => x.severity), ['warn']);
});

test('C79 wants the call as the first statement of main, comments and strings aside', () => {
  assert.ok(callsFirst(HOOK));
  assert.ok(callsFirst('fn main() -> anyhow::Result<()> {\n    // first\n    /* the guard */ paddock_proc::guardian::run_if_requested();\n    Ok(())\n}\n'));
  assert.ok(!callsFirst('fn main() {\n    // paddock_proc::guardian::run_if_requested();\n}\n'), 'a commented-out call');
  assert.ok(!callsFirst('fn main() {\n    let s = "paddock_proc::guardian::run_if_requested();";\n}\n'), 'a string');
  assert.ok(!callsFirst('fn main() {\n    start_app();\n    paddock_proc::guardian::run_if_requested();\n}\n'), 'not first');
  assert.ok(!callsFirst('fn helper() {\n    paddock_proc::guardian::run_if_requested();\n}\n'), 'not in main');
});

test('C79 also checks the mains a manifest names with [[bin]] path', () => {
  assert.deepEqual(binPaths('[package]\nname = "app"\n\n[[bin]]\nname = "x"\npath = "tools/x.rs"\n\n[[bin]]\nname = "y"\n\n[dependencies]\npath = "no"\n'), ['tools/x.rs']);
  const files = {
    'Cargo.lock': LOCK,
    'Cargo.toml': '[package]\nname = "app"\n\n[[bin]]\nname = "x"\npath = "tools/x.rs"\n',
    'tools/x.rs': BARE,
    'fuzz/Cargo.toml': '[package]\nname = "app"\n\n[[bin]]\nname = "f"\npath = "fuzz_targets/f.rs"\n',
    'fuzz/fuzz_targets/f.rs': '#![no_main]\nlibfuzzer_sys::fuzz_target!(|d: &[u8]| {});\n',
  };
  const t = tree(files);
  assert.deepEqual(guardedMains(t.files(), t.read), [{ file: 'tools/x.rs', hooked: false, wrapped: false }]);
});

const TOKIO = '#[tokio::main]\nasync fn main() -> anyhow::Result<()> {\n    paddock_proc::guardian::run_if_requested();\n    Ok(())\n}\n';

test('C79 refuses an async or attribute-wrapped main with its own message', () => {
  assert.equal(mainState(TOKIO), 'wrapped');
  assert.equal(mainState('#[async_std::main]\nasync fn main() {}\n'), 'wrapped');
  assert.equal(mainState('#[actix_web::main]\npub async fn main() {}\n'), 'wrapped');
  assert.equal(mainState('async fn main() {}\n'), 'wrapped');
  assert.equal(mainState('// #[tokio::main] once\nfn main() {\n    paddock_proc::guardian::run_if_requested();\n}\n'), 'hooked');
  const files = { 'Cargo.lock': LOCK, 'Cargo.toml': '[package]\nname = "app"\n', 'src/main.rs': TOKIO };
  const f = C79.run(tree(files), new Map());
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'error');
  assert.match(f[0].message, /async or wrapped by an attribute/);
});

// A binary links its normal dependencies, never its dev-dependencies: paddock-bench reached paddock-proc only through its
// dev-dependency on paddock-testkit (whose `start` feature adds paddock-proc), and its main cannot start supervised children.
test('C79: a package that reaches paddock-proc only through a dev-dependency has no main to guard', () => {
  const lock = `${LOCK}
[[package]]
name = "bench"
version = "0.1.0"
dependencies = [
 "kit",
 "plain",
]

[[package]]
name = "kit"
version = "0.1.0"
dependencies = [
 "paddock-proc",
]
`;
  const devManifest = '[package]\nname = "bench"\n\n[dependencies]\nplain = "0.1"\n\n[dev-dependencies]\nkit = { workspace = true }\n';
  assert.deepEqual([...devOnly(devManifest)], ['kit']);
  assert.deepEqual(guardedMains(['Cargo.lock', 'b/Cargo.toml', 'b/src/main.rs'], (p) => ({ 'Cargo.lock': lock, 'b/Cargo.toml': devManifest, 'b/src/main.rs': BARE })[p] ?? null), []);
  // The same crate as a normal dependency (also when it is a dev-dependency too, or a target-specific one) still counts.
  for (const normal of ['[dependencies]\nkit = "0.1"\n\n[dev-dependencies]\nkit = { workspace = true }\n', "[target.'cfg(unix)'.dependencies]\nkit.workspace = true\n"]) {
    const m = `[package]\nname = "bench"\n\n${normal}`;
    assert.deepEqual(devOnly(m).size, 0, normal);
    assert.equal(guardedMains(['Cargo.lock', 'b/Cargo.toml', 'b/src/main.rs'], (p) => ({ 'Cargo.lock': lock, 'b/Cargo.toml': m, 'b/src/main.rs': BARE })[p] ?? null).length, 1, normal);
  }
  // A target-specific dev-dependency is dev-only as well.
  assert.deepEqual([...devOnly("[package]\nname = \"bench\"\n[target.'cfg(unix)'.dev-dependencies]\nkit = \"0.1\"\n")], ['kit']);
});

// The lock lists a workspace member's dev-dependencies with its normal ones, so a member the binary does depend on can reach
// paddock-proc through its own dev-dependency: that path is not linked either.
test('C79: a dependency that reaches paddock-proc only through its own dev-dependency does not make a main reach it', () => {
  const lock = `${LOCK}
[[package]]
name = "bench"
version = "0.1.0"
dependencies = [
 "lib",
]

[[package]]
name = "lib"
version = "0.1.0"
dependencies = [
 "kit",
]

[[package]]
name = "kit"
version = "0.1.0"
dependencies = [
 "paddock-proc",
]
`;
  const files = {
    'Cargo.lock': lock,
    'bench/Cargo.toml': '[package]\nname = "bench"\n[dependencies]\nlib = { path = "../lib" }\n',
    'bench/src/main.rs': BARE,
    'lib/Cargo.toml': '[package]\nname = "lib"\n[dev-dependencies]\nkit = { path = "../kit" }\n',
  };
  assert.deepEqual(guardedMains(Object.keys(files), (p) => files[p] ?? null), []);
  files['lib/Cargo.toml'] = '[package]\nname = "lib"\n[dependencies]\nkit = { path = "../kit" }\n';
  assert.equal(guardedMains(Object.keys(files), (p) => files[p] ?? null).length, 1, 'a normal dependency still counts');
});
