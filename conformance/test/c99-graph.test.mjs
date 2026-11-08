import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { parseArgs } from '../src/cli.mjs';
import { crateBinaries, crateSources, graphFindings, MACOS_TARGETS, probeWorkspace, readGraph, series, sourceScan } from '../src/util/keychain-graph.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

// A probe result: `crates` is { name: { deps: [names], features: 'a,b', version } }; the first is the workspace member. The
// tree is the text of `cargo tree --prefix depth --format "{p}|{f}"` (a repeated package is printed once with "(*)").
const ver = (c) => c?.version ?? '1.0.0';
function metadata(crates) {
  const id = (n) => `${n} ${ver(crates[n])} (registry+x)`;
  const packages = Object.keys(crates).map((name, i) => ({ id: id(name), name, version: ver(crates[name]), source: i === 0 ? null : 'registry+x', manifest_path: `/reg/${name}/Cargo.toml` }));
  return { packages, workspace_members: [id(Object.keys(crates)[0])] };
}
function tree(crates) {
  const out = [];
  const shown = new Set();
  const walk = (n, d) => {
    out.push(`${d}${n} v${ver(crates[n])}|${crates[n].features ?? 'default'}${shown.has(n) ? ' (*)' : ''}`);
    if (shown.has(n)) return;
    shown.add(n);
    for (const c of crates[n].deps ?? []) walk(c, d + 1);
  };
  walk(Object.keys(crates)[0], 0);
  return out.join('\n');
}
// The in-check control (ER-33) needs security-framework in some probed graph and its real call in the source: a fixture that
// has none gets the reviewed TLS chain (app -> rustls-platform-verifier 0.7 -> security-framework 3.x) and `ctrl` serves the call.
const CONTROL = 'unsafe { SecKeychainSetUserInteractionAllowed(0) }';
const ctrl = (fn = () => [{ file: 'src/lib.rs', text: 'fn ok() {}' }]) => (m) => (m.includes('/security-framework/') ? [{ file: 'src/lib.rs', text: CONTROL }] : m.includes('/rustls-platform-verifier/') ? [{ file: 'src/lib.rs', text: '' }] : fn(m));
function withControl(crates) {
  if (crates['security-framework']) return crates;
  const first = Object.keys(crates)[0];
  return { ...crates, [first]: { ...crates[first], deps: [...(crates[first].deps ?? []), 'rustls-platform-verifier'] }, 'rustls-platform-verifier': { version: '0.7.1', deps: ['security-framework'] }, 'security-framework': { version: '3.7.0' } };
}
const probe = (input) => {
  const crates = withControl(input);
  return { meta: metadata(crates), runs: MACOS_TARGETS.map((target) => ({ target, tree: tree(crates) })) };
};
const found = (crates, scanOptions = { readSources: () => [{ file: 'src/lib.rs', text: 'fn ok() {}' }], listBinaries: () => [] }) => {
  const g = readGraph(metadata(crates), tree(crates));
  return { graph: graphFindings(g), scan: sourceScan(g, scanOptions) };
};
const whats = (r) => r.graph.map((f) => f.what);
const keys = (r) => r.graph.map((f) => f.key);

// Positive controls (ADR 0057 §3, ER-33): each of these must be found, or the probe cannot see what it is for.
test('C99 graph: a keyring crate anywhere in the macOS graph is found, with who pulled it in', () => {
  const r = found({ app: { deps: ['mid'] }, mid: { deps: ['keyring'] }, keyring: {} });
  assert.deepEqual(whats(r), ['keyring@1.0.0 is in the macOS build (pulled in by mid@1.0.0); only paddock-secrets may use it, and only off macOS'.replace('mid@1.0.0', 'mid')]);
  assert.deepEqual(keys(r), ['keyring']);
  for (const k of ['keyring-core', 'apple-native-keyring-store']) assert.deepEqual(keys(found({ app: { deps: [k] }, [k]: {} })), [k]);
});

test('C99 graph: a Keychain binding crate is allowed only below a parent reviewed for it, at the reviewed series', () => {
  const sf = { 'security-framework': { deps: ['security-framework-sys'], version: '3.7.0' }, 'security-framework-sys': { version: '2.17.0' } };
  assert.deepEqual(keys(found({ app: { deps: ['sneaky'] }, sneaky: { deps: ['security-framework'] }, ...sf })), ['security-framework<-sneaky']);
  assert.deepEqual(keys(found({ app: { deps: ['rustls-platform-verifier'] }, 'rustls-platform-verifier': { deps: ['security-framework'], version: '0.7.1' }, ...sf })), [], 'TLS trust at 0.7 is reviewed');
  assert.deepEqual(keys(found({ app: { deps: ['rustls-platform-verifier'] }, 'rustls-platform-verifier': { deps: ['security-framework'], version: '0.8.0' }, ...sf })), ['security-framework<-rustls-platform-verifier'], 'another series is not');
  assert.match(whats(found({ app: { deps: ['rustls-platform-verifier'] }, 'rustls-platform-verifier': { deps: ['security-framework'], version: '0.8.0' }, ...sf }))[0], /only the 0\.7 series of rustls-platform-verifier was reviewed/);
  // The row is per binding: objc2-service-management is reviewed for objc2-security, not for security-framework.
  assert.deepEqual(keys(found({ app: { deps: ['objc2-service-management'] }, 'objc2-service-management': { deps: ['security-framework'], version: '0.3.2' }, ...sf })), ['security-framework<-objc2-service-management']);
  assert.deepEqual(keys(found({ app: { deps: ['objc2-service-management'] }, 'objc2-service-management': { deps: ['objc2-security'], version: '0.3.2' }, 'objc2-security': { version: '0.3.2', features: 'Authorization,objc2' } })), []);
  assert.deepEqual(keys(found({ app: { deps: ['rustls-platform-verifier'] }, 'rustls-platform-verifier': { deps: ['objc2-security'], version: '0.7.1' }, 'objc2-security': { version: '0.3.2' } })), ['objc2-security<-rustls-platform-verifier']);
  assert.equal(series('0.7.1'), '0.7');
  assert.equal(series('3.7.0'), '3');
});

test('C99 graph: objc2-security with a Keychain feature on is found, with Authorization alone it is not', () => {
  const base = (features) => ({ app: { deps: ['objc2-service-management'] }, 'objc2-service-management': { deps: ['objc2-security'], version: '0.3.2' }, 'objc2-security': { version: '0.3.2', features } });
  assert.deepEqual(keys(found(base('Authorization,bitflags,objc2'))), []);
  assert.deepEqual(keys(found(base('Authorization,SecItem,SecKeychain'))), ['objc2-security:features']);
  assert.match(whats(found(base('SecKeychainItem')))[0], /Keychain features SecKeychainItem on/);
  assert.deepEqual(keys(found(base('SecAccess,SecKey,SecBase'))), [], 'only the ADR 0057 list switches a Keychain item on');
  for (const f of ['SecItem', 'SecKeychain', 'SecKeychainItem', 'SecKeychainSearch', 'SecImportExport', 'SecIdentitySearch']) assert.deepEqual(keys(found(base(f))), ['objc2-security:features'], f);
});

test('C99 graph: the dependency source scan finds the Keychain symbols in a dependency, and in a binding crate only when asked', () => {
  const src = { sneaky: [{ file: 'src/store.rs', text: 'unsafe { SecItemAdd(q, std::ptr::null_mut()) };\nSecItemAdd(q, p);\nlet _ = SecKeychainFindGenericPassword;' }] };
  const readSources = (m) => src[m.split('/')[2]] ?? [{ file: 'src/lib.rs', text: 'pub fn SecKeychainSetUserInteractionAllowed() {}' }];
  const crates = { app: { deps: ['sneaky', 'security-framework-sys'] }, sneaky: {}, 'security-framework-sys': { version: '2.17.0' } };
  const r = found(crates, { readSources, listBinaries: () => [] });
  assert.deepEqual(r.scan.findings.map((f) => f.what), ['sneaky@1.0.0 (src/store.rs) uses SecItemAdd', 'sneaky@1.0.0 (src/store.rs) uses SecKeychainFindGenericPassword']);
  const all = found(crates, { readSources, listBinaries: () => [], scanBindings: true });
  assert.ok(all.scan.findings.some((f) => /security-framework-sys@2\.17\.0 .* uses SecKeychainSetUserInteractionAllowed/.test(f.what)), 'the control sees the symbol in a binding');
  assert.equal(found({ app: { deps: ['tls'] }, tls: {} }, { readSources: () => [{ file: 'src/lib.rs', text: 'SecTrustEvaluateWithError(); // SecItem is not a call' }], listBinaries: () => [] }).scan.findings.length, 0, 'TLS trust is not a keychain item');
  const blob = (crates) => found(crates, { readSources: () => [{ file: 'src/lib.rs', text: '' }], listBinaries: () => ['libx.a'] }).scan.binaries;
  assert.deepEqual(blob({ app: { deps: ['blob'] }, blob: {} }), [{ key: 'bin:blob@1.0.0', what: 'blob@1.0.0: libx.a' }]);
  // Reviewed binaries are pinned by series: ring at its reviewed 0.17 is quiet, at another series it is listed again.
  assert.deepEqual(blob({ app: { deps: ['ring'] }, ring: { version: '0.17.14' } }), []);
  assert.equal(blob({ app: { deps: ['ring'] }, ring: { version: '0.18.0' } }).length, 1);
});

test('C99 graph: crate files of every language are read, root tests are not, a nested tests folder is, binaries are listed', (t) => {
  const root = makeApp({
    'src/a.swift': 'SecItemAdd', 'src/b.mm': 'SecItemAdd', 'src/c.cc': 'x', 'src/d.cpp': 'x', 'src/e.m': 'x', 'src/f.c': 'x', 'src/g.h': 'x',
    'src/tests/inner.rs': 'x', 'tests/outer.rs': 'x', 'examples/e.rs': 'x', 'benches/b.rs': 'x', 'README.md': 'x',
    'prebuilt/libz.a': 'x', 'vendor.framework/Versions/A/Sec': 'x',
  });
  t.after(() => rmSync(root, { recursive: true, force: true }));
  assert.deepEqual(crateSources(root), ['src/a.swift', 'src/b.mm', 'src/c.cc', 'src/d.cpp', 'src/e.m', 'src/f.c', 'src/g.h', 'src/tests/inner.rs']);
  assert.deepEqual(crateBinaries(root), ['prebuilt/libz.a', 'vendor.framework']);
});

test('C99 graph: an unusable document is broken, an unread dependency is unfetched', () => {
  assert.match(readGraph({ packages: [], workspace_members: [] }, 'x').broken, /no workspace member/);
  assert.match(readGraph(metadata({ app: {} }), 'garbage').broken, /cannot read/);
  assert.match(readGraph(metadata({ app: {} }), '0app v1.0.0|default\n1ghost v1.0.0|default').broken, /ghost@1\.0\.0 is in the cargo tree but not in the cargo metadata/);
  assert.match(readGraph(metadata({ app: {} }), '').broken, /no workspace member/);
  const dup = metadata({ app: {}, dep: {} });
  dup.packages.push({ ...dup.packages[1], id: 'dep 1.0.0 (git+y)' });
  assert.match(readGraph(dup, tree({ app: { deps: ['dep'] }, dep: {} })).broken, /two packages are named dep@1\.0\.0/);
  assert.deepEqual(found({ app: { deps: ['dep'] }, dep: {} }, { readSources: () => [], listBinaries: () => [] }).scan.unfetched, ['dep@1.0.0']);
});

const CI = '{"cargo": "."}\n';
const fixture = (extra = {}) =>
  makeApp({
    'paddock.ci.json': CI,
    'Cargo.toml': '[package]\nname = "demo-app"\nversion = "0.1.0"\n',
    'Cargo.lock': '# lock\n',
    'clippy.toml': '',
    'src/lib.rs': 'pub fn f() {}\n',
    ...extra,
  });
const make = (t, files) => {
  const root = fixture(files);
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
};
const probeOf = (crates, scan) => ({ options: { probe: () => probe(crates), readSources: ctrl(scan?.readSources), listBinaries: () => [] } });
const waiver = (path) => `[[waiver]]\ncheck = "C99"\npath = "${path}"\nreason = "moves to files in its work package"\nexpires = "2099-01-01"\n`;

test('C99 through the runner: every graph finding has its own path and a clean graph passes', async (t) => {
  const bad = await findingsOf('C99', make(t, {}), probeOf({ app: { deps: ['mid'] }, mid: { deps: ['keyring'] }, keyring: {} }, { readSources: () => [{ file: 'src/lib.rs', text: 'SecItemAdd();' }] }));
  assert.deepEqual(bad.map((f) => f.file), ['Cargo.lock#keyring', 'Cargo.lock#src:mid@1.0.0:SecItemAdd', 'Cargo.lock#src:keyring@1.0.0:SecItemAdd']);
  assert.match(bad[0].message, /keyring@1\.0\.0 is in the macOS build \(pulled in by mid\)/);
  assert.deepEqual(await findingsOf('C99', make(t, {}), probeOf({ app: { deps: ['ok'] }, ok: {} })), []);
});

test('C99 waivers: a waived lock still reports a broken check and any finding the waiver does not name', async (t) => {
  const crates = { app: { deps: ['keyring'] }, keyring: {} };
  const w = { 'paddock.waivers.toml': waiver('Cargo.lock#keyring') };
  const waived = await findingsOf('C99', make(t, w), probeOf(crates));
  assert.deepEqual(waived, [], 'the named finding is waived');
  // A new SecItem finding in the same lock is not covered by the keyring waiver.
  const added = await findingsOf('C99', make(t, w), probeOf(crates, { readSources: () => [{ file: 'src/lib.rs', text: 'SecItemAdd();' }] }));
  assert.deepEqual(added.map((f) => f.file), ['Cargo.lock#src:keyring@1.0.0:SecItemAdd']);
  // A new keyring-family crate is not covered either.
  const more = await findingsOf('C99', make(t, w), probeOf({ app: { deps: ['keyring', 'keyring-core'] }, keyring: {}, 'keyring-core': {} }));
  assert.deepEqual(more.map((f) => f.file), ['Cargo.lock#keyring-core']);
  // Even a waiver for the whole lock (a glob) never hides a broken check or a missing measurement.
  for (const path of ['Cargo.lock', 'Cargo.lock*', '*']) {
    const glob = { 'paddock.waivers.toml': waiver(path) };
    const broken = await findingsOf('C99', make(t, glob), { options: { probe: () => ({ broken: 'cargo fetch failed: exit 101' }) } });
    assert.match(broken[0].message, /check broken/, path);
    assert.equal(broken[0].severity, 'error');
    const unfetched = await findingsOf('C99', make(t, glob), probeOf({ app: { deps: ['dep'] }, dep: {} }, { readSources: () => [] }));
    assert.match(unfetched.map((f) => f.message).join('\n'), /check broken.*not downloaded/, path);
    const nolock = makeApp({ ...glob, 'paddock.ci.json': CI, 'Cargo.toml': '[package]\nname = "a"\n', 'src/lib.rs': '' });
    t.after(() => rmSync(nolock, { recursive: true, force: true }));
    assert.match((await findingsOf('C99', nolock))[0].message, /not measured/, path);
  }
});

test('C99 through the runner: a failed probe, an unfetched dependency and a missing lock are errors', async (t) => {
  const failed = await findingsOf('C99', make(t, {}), { options: { probe: () => ({ broken: 'cargo fetch failed: exit 101' }) } });
  assert.deepEqual(failed.map((f) => f.severity), ['error']);
  assert.match(failed[0].message, /check broken.*cargo fetch failed/);
  const unfetched = await findingsOf('C99', make(t, {}), probeOf({ app: { deps: ['dep'] }, dep: {} }, { readSources: () => [] }));
  assert.match(unfetched[0].message, /check broken.*not downloaded/);
  // ADR 0057 §3.4: no Cargo.lock to probe is an error ("not measured"), not a warning.
  const lockless = makeApp({ 'paddock.ci.json': CI, 'Cargo.toml': '[package]\nname = "a"\n', 'src/lib.rs': '' });
  t.after(() => rmSync(lockless, { recursive: true, force: true }));
  const r = await findingsOf('C99', lockless);
  assert.deepEqual(r.map((f) => f.severity), ['error']);
  assert.match(r[0].message, /not measured: Cargo\.lock does not exist/);
  // No paddock.ci.json names a workspace: nothing was probed.
  const unnamed = makeApp({ 'Cargo.toml': '[package]\nname = "a"\n', 'Cargo.lock': '', 'src/lib.rs': '' });
  t.after(() => rmSync(unnamed, { recursive: true, force: true }));
  assert.match((await findingsOf('C99', unnamed))[0].message, /not measured: no paddock\.ci\.json names a Cargo workspace/);
});

test('C99 through the runner: only the named workspaces and crates/ are probed, not a vendored lock', async (t) => {
  const root = makeApp({
    'apps/a/paddock.ci.json': '{"cargo": "src-tauri"}\n', 'apps/a/src-tauri/Cargo.toml': '[package]\nname = "a"\n', 'apps/a/src-tauri/Cargo.lock': '', 'apps/a/src-tauri/src/lib.rs': '',
    'crates/Cargo.toml': '[package]\nname = "c"\n', 'crates/Cargo.lock': '', 'crates/src/lib.rs': '',
    'apps/a/vendor/dep/Cargo.toml': '[package]\nname = "v"\n', 'apps/a/vendor/dep/Cargo.lock': '',
  });
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const seen = [];
  const f = await findingsOf('C99', root, { options: { probe: (d) => (seen.push(d.slice(root.length)), probe({ app: {} })), readSources: ctrl(() => []), listBinaries: () => [] } });
  assert.deepEqual(seen.sort(), ['/apps/a/src-tauri', '/crates']);
  // Neither has a clippy.toml: the demos' recorded deviation is reported as a warning, never silently dropped.
  assert.deepEqual(f.map((x) => [x.file, x.severity]), [['apps/a/src-tauri/Cargo.lock#clippy-not-measured', 'warn'], ['crates/Cargo.lock#clippy-not-measured', 'warn']]);
  assert.match(f[0].message, /Clippy on macOS: not measured/);
});

test('C99: --no-graph is a switch of the command line and says the graph was not measured', async (t) => {
  assert.equal(parseArgs(['--app', 'x', '--no-graph'])['no-graph'], true);
  const r = await findingsOf('C99', make(t, {}), { options: { graph: 'skipped' } });
  assert.deepEqual(r.map((f) => f.severity), ['warn']);
  assert.match(r[0].message, /not measured.*--no-graph/);
});

test('C99 graph: a package of the same name at another version is not a workspace member', () => {
  // The member is dep@1.0.0 (a path package); a dependency named dep at 2.0.0 is a crate to check, not a member to skip.
  const meta = { packages: [{ id: 'm', name: 'dep', version: '1.0.0', source: null, manifest_path: '/m/Cargo.toml' }, { id: 'r', name: 'dep', version: '2.0.0', source: 'registry+x', manifest_path: '/r/Cargo.toml' }], workspace_members: ['m'] };
  const g = readGraph(meta, '0dep v1.0.0|default\n1dep v2.0.0|default');
  assert.deepEqual(g.crates.map((p) => p.version), ['2.0.0']);
});

test('C99 through the runner: an unreviewed binary is an error that a waiver for its exact path covers; the scan control must see a real call', async (t) => {
  const crates = { app: { deps: ['blob', 'rustls-platform-verifier'] }, blob: {}, 'rustls-platform-verifier': { version: '0.7.1', deps: ['security-framework'] }, 'security-framework': { version: '3.7.0' } };
  const opts = (sources, bins) => ({ options: { probe: () => probe(crates), readSources: (m) => (m.includes('security-framework') ? [{ file: 'src/lib.rs', text: sources }] : [{ file: 'src/lib.rs', text: '' }]), listBinaries: (m) => (m.includes('/blob/') ? bins : []) } });
  const real = 'unsafe { SecKeychainSetUserInteractionAllowed(0) }';
  const r = await findingsOf('C99', make(t, {}), opts(real, ['libx.a']));
  assert.deepEqual(r.map((f) => [f.file, f.severity]), [['Cargo.lock#bin:blob@1.0.0', 'error']]);
  assert.deepEqual(await findingsOf('C99', make(t, { 'paddock.waivers.toml': waiver('Cargo.lock#bin:blob@1.0.0') }), opts(real, ['libx.a'])), []);
  // The control: a scan that cannot see the call in the real binding source is a broken check, whatever the waivers say.
  const blind = await findingsOf('C99', make(t, { 'paddock.waivers.toml': waiver('Cargo.lock*') }), opts('pub fn nothing() {}', []));
  assert.match(blind[0].message, /check broken.*did not find SecKeychainSetUserInteractionAllowed in security-framework@3\.7\.0/);
  assert.equal(blind[0].severity, 'error');
});

test('C99 waivers: a path with a glob is itself an error (W004), an exact path is fine', async (t) => {
  const crates = { app: { deps: ['keyring'] }, keyring: {} };
  for (const path of ['Cargo.lock*', '*', 'apps/*/Cargo.lock#keyring']) {
    const r = await findingsOf('C99', make(t, { 'paddock.waivers.toml': waiver(path) }), probeOf(crates));
    assert.ok(r.some((f) => f.check === 'W004' && /exact path/.test(f.message)), path);
    assert.ok(r.some((f) => f.file === 'Cargo.lock#keyring'), `${path} waived nothing`);
  }
});

test('C99 --scope: a release of one app is judged on that app and crates/ only, and a scope that matches nothing is broken', async (t) => {
  const app = (name, deps) => ({
    [`apps/${name}/paddock.ci.json`]: CI, [`apps/${name}/clippy.toml`]: '', [`apps/${name}/Cargo.lock`]: '#',
    [`apps/${name}/Cargo.toml`]: `[package]\nname = "${name}"\nversion = "0.1.0"\n\n[dependencies]\n${deps}`, [`apps/${name}/src/lib.rs`]: 'pub fn f() {}\n',
  });
  const root = makeApp({ ...app('good', ''), ...app('bad', 'keyring = "4"\n'), 'crates/Cargo.toml': '[package]\nname = "shared"\nversion = "0.1.0"\n', 'crates/Cargo.lock': '#', 'crates/clippy.toml': '', 'crates/src/lib.rs': '' });
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const seen = [];
  const run = (scope) => findingsOf('C99', root, { options: { scope, probe: (d) => (seen.push(d.slice(root.length)), probe({ app: {} })), readSources: ctrl(() => []), listBinaries: () => [] } });
  assert.ok((await run(undefined)).some((f) => f.file === 'apps/bad/Cargo.toml'), 'repo-wide: the bad app is found');
  seen.length = 0;
  assert.deepEqual(await run('apps/good'), [], "scoped to the good app: the bad app's manifest does not block it");
  assert.deepEqual(seen.sort(), ['/apps/good', '/crates'], 'its workspace and crates/ are probed, no other app');
  assert.ok((await run('apps/bad')).some((f) => f.file === 'apps/bad/Cargo.toml'), 'scoped to the bad app: found');
  const typo = await run('apps/goood');
  assert.match(typo[0].message, /check broken: --scope apps\/goood matches no file/);
  assert.equal(typo.length, 1);
  // crates/ is always in scope.
  const crates = makeApp({ ...app('good', ''), 'crates/Cargo.toml': '[package]\nname = "shared"\nversion = "0.1.0"\n\n[dependencies]\nkeyring = "4"\n', 'crates/src/lib.rs': '' });
  t.after(() => rmSync(crates, { recursive: true, force: true }));
  assert.ok((await findingsOf('C99', crates, { options: { scope: 'apps/good', graph: 'off' } })).some((f) => f.file === 'crates/Cargo.toml'));
  assert.equal(parseArgs(['--app', 'x', '--scope', 'apps/y']).scope, 'apps/y');
});

test('C99 --scope reads the UI the app ships (ui/projects/<app>, paddock-ui) but not another app\'s, and skips the W003 of waivers outside the scope', async (t) => {
  const bad = "run('security dump-keychain');\n";
  const files = {
    'paddock.ci.json': CI, 'clippy.toml': '', 'Cargo.lock': '#', 'Cargo.toml': '[package]\nname = "x"\nversion = "0.1.0"\n', 'src/lib.rs': '',
    'apps/mine/Cargo.toml': '[package]\nname = "mine"\nversion = "0.1.0"\n', 'apps/mine/src/lib.rs': '', 'apps/other/Cargo.toml': '[package]\nname = "other"\nversion = "0.1.0"\n', 'apps/other/src/lib.rs': '',
    'ui/projects/mine/src/a.ts': bad, 'ui/projects/paddock-ui/src/b.ts': bad, 'ui/projects/other/src/c.ts': bad,
  };
  const root = makeApp(files);
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const run = (scope, extra = {}) => findingsOf('C99', root, { options: { scope, graph: 'off', ...extra } });
  assert.deepEqual((await run('apps/mine')).map((f) => f.file).sort(), ['ui/projects/mine/src/a.ts', 'ui/projects/paddock-ui/src/b.ts']);
  assert.equal((await run(undefined)).filter((f) => f.file.startsWith('ui/')).length, 3, 'repo-wide: all three');
  // W003: a waiver outside the scope is not reported as unused; one inside the scope that waives nothing still is.
  const waived = makeApp({ ...files, 'paddock.waivers.toml': waiver('apps/other/Cargo.toml') + waiver('apps/mine/Cargo.toml') });
  t.after(() => rmSync(waived, { recursive: true, force: true }));
  const w = await findingsOf('C99', waived, { options: { scope: 'apps/mine', graph: 'off' } });
  assert.deepEqual(w.filter((f) => f.check === 'W003').map((f) => f.message), ['waiver for C99 apps/mine/Cargo.toml waives nothing']);
  const all = await findingsOf('C99', waived, { options: { graph: 'off' } });
  assert.equal(all.filter((f) => f.check === 'W003').length, 2, 'unscoped: both are reported');
});

test('C99 fails closed: with no security-framework in any probed graph the control never ran, and that is a broken check no waiver covers', async (t) => {
  const crates = { app: { deps: ['ok'] }, ok: {} };
  const raw = { meta: metadata(crates), runs: MACOS_TARGETS.map((target) => ({ target, tree: tree(crates) })) };
  const r = await findingsOf('C99', make(t, { 'paddock.waivers.toml': waiver('*') }), { options: { probe: () => raw, readSources: () => [{ file: 'src/lib.rs', text: '' }], listBinaries: () => [] } });
  assert.ok(r.length > 0 && r.every((f) => f.severity === 'error'));
  assert.match(r.map((f) => f.message).join('\n'), /check broken: no probed macOS graph contains security-framework/);
});

// The whole path with a real cargo and no network: path dependencies.
const hasCargo = spawnSync('cargo', ['--version']).status === 0;
const skipCargo = hasCargo ? false : 'cargo is not installed here: run the conformance tests where cargo is (CI image)';
const env = { ...process.env, RUSTC_WRAPPER: '' };
const pkg = (name, deps = '', extra = '') => `[package]\nname = "${name}"\nversion = "0.1.0"\nedition = "2021"\n${extra}\n[dependencies]\n${deps}`;
// The reviewed TLS chain as path stubs, so the in-check control finds a real call: rustls-platform-verifier 0.7.1 -> security-framework 3.7.0.
const stub = (name, version, deps = '') => `[package]\nname = "${name}"\nversion = "${version}"\nedition = "2021"\n\n[dependencies]\n${deps}`;
const tlsStubs = {
  'harness/rpv/Cargo.toml': stub('rustls-platform-verifier', '0.7.1', 'security-framework = { path = "../sf" }\n'),
  'harness/rpv/src/lib.rs': '',
  'harness/sf/Cargo.toml': stub('security-framework', '3.7.0'),
  'harness/sf/src/lib.rs': 'extern "C" { pub fn SecKeychainSetUserInteractionAllowed(a: u8) -> i32; }\n',
};
const tlsDep = 'rustls-platform-verifier = { path = "harness/rpv" }\n';
const lockfile = (dir) => {
  const r = spawnSync('cargo', ['generate-lockfile', '--offline'], { cwd: dir, encoding: 'utf8', env });
  assert.equal(r.status, 0, r.stderr);
};

test('C99 with a real cargo: a planted keyring crate and a planted SecItemAdd call are found', { skip: skipCargo }, async (t) => {
  const root = makeApp({
    'paddock.ci.json': CI,
    'clippy.toml': '',
    ...tlsStubs,
    'Cargo.toml': pkg('demo-app', `${tlsDep}planted = { path = "vendor/planted" }\nkeyring = { path = "vendor/keyring" }\n`),
    'src/lib.rs': 'pub fn f() {}\n',
    'vendor/planted/Cargo.toml': pkg('planted'),
    'vendor/planted/src/lib.rs': 'extern "C" { pub fn SecItemAdd(a: usize, b: usize) -> i32; }\n',
    'vendor/keyring/Cargo.toml': pkg('keyring'),
    'vendor/keyring/src/lib.rs': '',
  });
  t.after(() => rmSync(root, { recursive: true, force: true }));
  lockfile(root);
  const f = await findingsOf('C99', root, { options: { cargoArgs: ['--offline'] } });
  const msg = f.map((x) => x.message).join('\n');
  assert.match(msg, /keyring@0\.1\.0 is in the macOS build \(pulled in by demo-app\)/);
  assert.match(msg, /dependency planted@0\.1\.0 \(src\/lib\.rs\) uses SecItemAdd/);
  assert.ok(f.some((x) => x.file === 'Cargo.lock#keyring'));
  assert.ok(!msg.includes('check broken'), 'the TLS stubs give the control its real call');
});

// The paddock-secrets pattern: a shared crate with an optional `keyring` feature that the app enables off macOS only.
test('C99 with a real cargo: a keyring feature enabled off macOS is not in the macOS graph, and is in the Windows graph', { skip: skipCargo }, async (t) => {
  const root = makeApp({
    'paddock.ci.json': CI,
    'clippy.toml': '',
    ...tlsStubs,
    'Cargo.toml': `${pkg('demo-app', `${tlsDep}secrets = { path = "vendor/secrets", package = "paddock-secrets" }\n`)}\n[target.'cfg(not(target_os = "macos"))'.dependencies]\nsecrets = { path = "vendor/secrets", package = "paddock-secrets", features = ["keyring"] }\n`,
    'src/lib.rs': 'pub fn f() {}\n',
    'vendor/secrets/Cargo.toml': `${pkg('paddock-secrets', 'keyring = { path = "../keyring", optional = true }\n')}\n[features]\nkeyring = ["dep:keyring"]\n`,
    'vendor/secrets/src/lib.rs': '',
    'vendor/keyring/Cargo.toml': pkg('keyring'),
    'vendor/keyring/src/lib.rs': '',
  });
  t.after(() => rmSync(root, { recursive: true, force: true }));
  lockfile(root);
  assert.deepEqual(await findingsOf('C99', root, { options: { cargoArgs: ['--offline'] } }), [], 'the macOS graph is clean (cargo metadata alone would say otherwise)');
  // The control: the same probe on the Windows triple does show keyring, so a clean macOS result is not a blind probe.
  const win = probeWorkspace(root, { extra: ['--offline'], targets: ['x86_64-pc-windows-msvc'], env });
  assert.ok(!win.broken, win.broken);
  const g = readGraph(win.meta, win.runs[0].tree);
  assert.deepEqual(keys({ graph: graphFindings(g) }), ['keyring']);
  // And for both Apple triples the probe ran.
  const mac = probeWorkspace(root, { extra: ['--offline'], env });
  assert.deepEqual(mac.runs.map((r) => r.target), MACOS_TARGETS);
});

// A positive control on a real binding crate (needs the registry sources, fetched by the probe in CI).
test('C99 dependency scan finds SecKeychainSetUserInteractionAllowed in the real security-framework, then the binding is allowed', (t) => {
  const dir = join(import.meta.dirname, '..', '..', 'apps', 'pitwall');
  const p = hasCargo ? probeWorkspace(dir, { extra: ['--offline'], targets: [MACOS_TARGETS[0]], env }) : { broken: 'no cargo' };
  if (p.broken) return t.skip(`the Pitwall dependencies are not fetched here (${p.broken.slice(0, 80)}): run where cargo fetch has run`);
  const g = readGraph(p.meta, p.runs[0].tree);
  const scan = sourceScan(g, { scanBindings: true });
  assert.ok(scan.read > 0 && scan.findings.some((f) => /security-framework@.*SecKeychainSetUserInteractionAllowed/.test(f.what)), 'the scan sees the real call');
  assert.ok(!sourceScan(g).findings.some((f) => /security-framework/.test(f.what)), 'with the binding allowed it is not reported');
  assert.deepEqual(graphFindings(g), [], 'and Pitwall has no unreviewed parent');
});
