import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';
import { graphFindings, readGraph, sourceScan } from '../src/util/keychain-graph.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

// A probe result: `crates` is { name: { deps: [names] } }; the first is the workspace member. The tree is the text of
// `cargo tree --prefix depth` (a repeated package is printed once with "(*)" and no children, as cargo does).
function metadata(crates) {
  const id = (n) => `${n} 1.0.0 (registry+x)`;
  const packages = Object.keys(crates).map((name, i) => ({ id: id(name), name, version: '1.0.0', source: i === 0 ? null : 'registry+x', manifest_path: `/reg/${name}/Cargo.toml` }));
  return { packages, workspace_members: [id(Object.keys(crates)[0])] };
}
function tree(crates) {
  const out = [];
  const shown = new Set();
  const walk = (n, d) => {
    out.push(`${d}${n} v1.0.0${shown.has(n) ? ' (*)' : ''}`);
    if (shown.has(n)) return;
    shown.add(n);
    for (const c of crates[n].deps ?? []) walk(c, d + 1);
  };
  walk(Object.keys(crates)[0], 0);
  return out.join('\n');
}
const probe = (crates) => ({ meta: metadata(crates), tree: tree(crates) });
const found = (crates, sources = () => [{ file: 'src/lib.rs', text: 'fn ok() {}' }]) => {
  const g = readGraph(metadata(crates), tree(crates));
  return { graph: graphFindings(g).map((f) => f.what), scan: sourceScan(g, sources) };
};

// Positive controls (ER-33): the probe must see each of these, or it cannot see what it is for.
test('C99 graph: a keyring crate anywhere in the macOS graph is found, with who pulled it in', () => {
  const r = found({ app: { deps: ['mid'] }, mid: { deps: ['keyring'] }, keyring: {} });
  assert.deepEqual(r.graph, ['keyring@1.0.0 is in the macOS build (pulled in by mid); only paddock-secrets may use it, and only off macOS']);
  for (const k of ['keyring-core', 'apple-native-keyring-store']) assert.equal(found({ app: { deps: [k] }, [k]: {} }).graph.length, 1, k);
});

test('C99 graph: a Keychain binding crate needs a reviewed dependent', () => {
  assert.equal(found({ app: { deps: ['sneaky'] }, sneaky: { deps: ['security-framework'] }, 'security-framework': { deps: ['security-framework-sys'] }, 'security-framework-sys': {} }).graph.length, 1, 'security-framework below sneaky');
  assert.match(found({ app: { deps: ['sneaky'] }, sneaky: { deps: ['objc2-security'] }, 'objc2-security': {} }).graph[0], /objc2-security@1\.0\.0 .* below sneaky/);
  assert.deepEqual(found({ app: { deps: ['rustls-platform-verifier'] }, 'rustls-platform-verifier': { deps: ['security-framework'] }, 'security-framework': { deps: ['security-framework-sys'] }, 'security-framework-sys': {} }).graph, [], 'TLS trust is reviewed');
  assert.deepEqual(found({ app: { deps: ['objc2-service-management'] }, 'objc2-service-management': { deps: ['objc2-security'] }, 'objc2-security': {} }).graph, [], 'login items are reviewed');
});

test('C99 graph: the dependency source scan finds the Keychain symbols, in a dependency but not in the bindings', () => {
  const src = { sneaky: [{ file: 'src/store.rs', text: 'unsafe { SecItemAdd(q, std::ptr::null_mut()) };\nSecItemAdd(q, p);\nlet _ = SecKeychainFindGenericPassword;' }] };
  const r = found({ app: { deps: ['sneaky', 'security-framework-sys'] }, sneaky: {}, 'security-framework-sys': {} }, (m) => src[m.split('/')[2]] ?? [{ file: 'src/lib.rs', text: 'pub fn SecItemAdd() {}' }]);
  assert.deepEqual(r.scan.findings.map((f) => f.what), ['sneaky@1.0.0 (src/store.rs) uses SecItemAdd', 'sneaky@1.0.0 (src/store.rs) uses SecKeychainFindGenericPassword']);
  assert.equal(found({ app: { deps: ['tls'] }, tls: {} }, () => [{ file: 'src/lib.rs', text: 'SecTrustEvaluateWithError(); // SecItem is not a call' }]).scan.findings.length, 0, 'TLS trust is not a keychain item');
});

test('C99 graph: an unusable document is broken, an unread dependency is unfetched', () => {
  assert.match(readGraph({ packages: [], workspace_members: [] }, 'x').broken, /no workspace member/);
  assert.match(readGraph(metadata({ app: {} }), 'garbage').broken, /cannot read/);
  assert.match(readGraph(metadata({ app: {} }), '0app v1.0.0\n1ghost v1.0.0').broken, /ghost@1\.0\.0 is in the cargo tree but not in the cargo metadata/);
  assert.match(readGraph(metadata({ app: {} }), '').broken, /no workspace member/);
  const r = found({ app: { deps: ['dep'] }, dep: {} }, () => []);
  assert.deepEqual(r.scan.unfetched, ['dep@1.0.0']);
});

const fixture = (extra = {}) =>
  makeApp({
    'paddock.ci.json': '{"cargo": "."}\n',
    'Cargo.toml': '[package]\nname = "demo-app"\nversion = "0.1.0"\n',
    'Cargo.lock': '# lock\n',
    'src/lib.rs': 'pub fn f() {}\n',
    ...extra,
  });
const probeOf = (crates, sources) => ({ options: { probe: () => probe(crates), readSources: sources ?? (() => [{ file: 'src/lib.rs', text: 'fn ok() {}' }]) } });

test('C99 through the runner: findings from the graph land on the Cargo.lock; a clean graph passes', async () => {
  const bad = await findingsOf('C99', fixture(), probeOf({ app: { deps: ['mid'] }, mid: { deps: ['keyring'] }, keyring: {} }, () => [{ file: 'src/lib.rs', text: 'SecItemAdd();' }]));
  assert.deepEqual(bad.map((f) => [f.file, f.severity]), [['Cargo.lock', 'error'], ['Cargo.lock', 'error'], ['Cargo.lock', 'error']]);
  assert.match(bad[0].message, /keyring@1\.0\.0 is in the macOS build \(pulled in by mid\)/);
  assert.match(bad.map((f) => f.message).join('\n'), /dependency mid@1\.0\.0 \(src\/lib\.rs\) uses SecItemAdd/);
  assert.deepEqual(await findingsOf('C99', fixture(), probeOf({ app: { deps: ['ok'] }, ok: {} })), []);
});

test('C99 through the runner: a failed probe, an unfetched dependency and a missing lock are never a pass', async () => {
  const failed = await findingsOf('C99', fixture(), { options: { probe: () => ({ broken: 'cargo fetch failed: exit 101' }) } });
  assert.deepEqual(failed.map((f) => f.severity), ['error']);
  assert.match(failed[0].message, /check broken.*cargo fetch failed/);
  const unfetched = await findingsOf('C99', fixture(), probeOf({ app: { deps: ['dep'] }, dep: {} }, () => []));
  assert.match(unfetched[0].message, /check broken.*not downloaded/);
  const noLock = fixture();
  const lockless = await findingsOf('C99', makeApp({ 'paddock.ci.json': '{"cargo": "."}\n', 'Cargo.toml': '[package]\nname = "a"\n', 'src/lib.rs': '' }));
  assert.deepEqual(lockless.map((f) => f.severity), ['warn']);
  assert.match(lockless[0].message, /not measured/);
  assert.ok(noLock);
  // The crates/ library workspace is not a build: its Cargo.lock is not probed.
  const lib = makeApp({ 'paddock.ci.json': '{"cargo": "."}\n', 'crates/Cargo.toml': '[package]\nname = "a"\n', 'crates/Cargo.lock': '', 'crates/src/lib.rs': '' });
  assert.match((await findingsOf('C99', lib, { options: { probe: () => assert.fail('crates/ must not be probed') } }))[0].message, /not measured/);
});

// The whole path with a real cargo and no network: path dependencies, one named keyring, one calling SecItemAdd.
const hasCargo = spawnSync('cargo', ['--version']).status === 0;
test('C99 with a real cargo: a planted keyring crate and a planted SecItemAdd call are found', { skip: hasCargo ? false : 'cargo is not installed here: run the conformance tests where cargo is (CI image)' }, async () => {
  const pkg = (name, deps = '') => `[package]\nname = "${name}"\nversion = "0.1.0"\nedition = "2021"\n\n[dependencies]\n${deps}`;
  const root = makeApp({
    'paddock.ci.json': '{"cargo": "."}\n',
    'Cargo.toml': pkg('demo-app', 'planted = { path = "vendor/planted" }\nkeyring = { path = "vendor/keyring" }\n'),
    'src/lib.rs': 'pub fn f() {}\n',
    'vendor/planted/Cargo.toml': pkg('planted'),
    'vendor/planted/src/lib.rs': 'extern "C" { pub fn SecItemAdd(a: usize, b: usize) -> i32; }\n',
    'vendor/keyring/Cargo.toml': pkg('keyring'),
    'vendor/keyring/src/lib.rs': '',
  });
  const lock = spawnSync('cargo', ['generate-lockfile', '--offline'], { cwd: root, encoding: 'utf8' });
  assert.equal(lock.status, 0, lock.stderr);
  const f = await findingsOf('C99', root, { options: { cargoArgs: ['--offline'] } });
  const msg = f.map((x) => x.message).join('\n');
  assert.match(msg, /keyring@0\.1\.0 is in the macOS build \(pulled in by demo-app\)/);
  assert.match(msg, /dependency planted@0\.1\.0 \(src\/lib\.rs\) uses SecItemAdd/);
  // Without the two planted crates the same fixture is clean.
  // A keyring that only Windows builds is not in the macOS graph (features resolve per target; cargo metadata would say otherwise).
  const clean = makeApp({
    'paddock.ci.json': '{"cargo": "."}\n',
    'Cargo.toml': `${pkg('demo-app')}\n[target.'cfg(windows)'.dependencies]\nwinonly = { path = "harness/winonly" }\n`,
    'src/lib.rs': 'pub fn f() {}\n',
    'harness/winonly/Cargo.toml': pkg('winonly', 'keyring = { path = "../../vendor/keyring" }\n'),
    'harness/winonly/src/lib.rs': '',
    'vendor/keyring/Cargo.toml': pkg('keyring'),
    'vendor/keyring/src/lib.rs': '',
  });
  assert.equal(spawnSync('cargo', ['generate-lockfile', '--offline'], { cwd: clean }).status, 0);
  assert.deepEqual(await findingsOf('C99', clean, { options: { cargoArgs: ['--offline'] } }), []);
});
