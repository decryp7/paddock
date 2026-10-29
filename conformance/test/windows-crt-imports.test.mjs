// ADR 0032: no shipped Windows binary imports the VC++ runtime. The PE files here are synthetic, built byte by byte.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, readdirSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { cargoBinaryNames, check, collect, mergePatch, needsRedist, parseArgs, peImports, ruleFindings, shippedFiles } from '../release/windows-crt-imports.mjs';
import { makeApp, pe } from './helpers.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const CLI = join(HERE, '../release/windows-crt-imports.mjs');
const REPO = join(HERE, '../..');

const SYSTEM = ['KERNEL32.dll', 'api-ms-win-crt-runtime-l1-1-0.dll', 'ntdll.dll'];

test('the redistributable DLLs are recognised; Windows\' own runtime (UCRT, msvcrt) is allowed', () => {
  for (const d of [
    'VCRUNTIME140.dll', 'vcruntime140_1.dll', 'vcruntime140_threads.dll', 'MSVCP140.dll', 'msvcp140_atomic_wait.dll', 'msvcp140_2.dll',
    'MSVCR120.dll', 'concrt140.dll', 'vccorlib140.dll', 'vcomp140.dll', 'vcamp140.dll', 'mfc140u.dll', 'mfcm140u.dll', 'MFCM140.dll',
    // Debug runtimes, with and without a suffix after the d.
    'msvcp140d.dll', 'msvcp140d_atomic_wait.dll', 'vcruntime140d.dll', 'vcruntime140_1d.dll', 'mfcm140ud.dll', 'ucrtbased.dll',
  ]) {
    assert.ok(needsRedist(d), d);
  }
  for (const d of ['ucrtbase.dll', 'msvcrt.dll', 'api-ms-win-crt-heap-l1-1-0.dll', 'kernel32.dll', 'vcruntime.dll.bak', 'WebView2Loader.dll', 'mfplat.dll']) {
    assert.ok(!needsRedist(d), d);
  }
});

test('peImports reads the import and delay-import tables of PE32+ and PE32', () => {
  for (const plus of [true, false]) {
    const r = peImports(pe({ imports: [...SYSTEM, 'VCRUNTIME140.dll'], delay: ['MSVCP140.dll'], plus }));
    assert.deepEqual(r, { imports: [...SYSTEM, 'VCRUNTIME140.dll'], delayImports: ['MSVCP140.dll'] });
  }
  assert.deepEqual(peImports(pe()), { imports: [], delayImports: [] });
});

test('peImports refuses VA-based delay-load descriptors with a message saying so', () => {
  const r = peImports(pe({ imports: SYSTEM, delay: ['MSVCP140.dll'], delayAttributes: 0 }));
  assert.match(r.error, /delay-load descriptor uses virtual addresses.*dumpbin \/imports/);
  const root = makeApp({ 'old.dll': pe({ delay: ['KERNEL32.dll'], delayAttributes: 0 }) });
  assert.match(check([join(root, 'old.dll')])[0], /old\.dll: not a valid PE file \(a delay-load descriptor uses virtual addresses/);
});

test('peImports never throws on broken input and says what is wrong', () => {
  assert.match(peImports(Buffer.alloc(0)).error, /MZ/);
  assert.match(peImports(Buffer.from('MZ')).error, /PE signature/);
  const good = pe({ imports: ['KERNEL32.dll'] });
  for (let cut = 0; cut < good.length; cut += 7) {
    const r = peImports(good.subarray(0, cut));
    assert.ok('error' in r || Array.isArray(r.imports), `cut at ${cut}`);
  }
  // A descriptor list with no terminator and a name pointing outside every section.
  const endless = pe({ imports: ['KERNEL32.dll'] });
  endless.fill(0xff, 0x200, 0x200 + 20);
  assert.ok('error' in peImports(endless));
  // Random bytes behind a valid header.
  let seed = 7;
  for (let i = 0; i < 200; i++) {
    const junk = Buffer.from(good);
    for (let j = 0; j < 40; j++) {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      junk[0x40 + (seed % (junk.length - 0x40))] = seed & 0xff;
    }
    assert.doesNotThrow(() => peImports(junk));
  }
});

test('check: a sidecar importing VCRUNTIME140 fails, a clean one passes, a non-PE file is reported', () => {
  const root = makeApp({
    'dist/app.exe': pe({ imports: SYSTEM }),
    'dist/agent.exe': pe({ imports: [...SYSTEM, 'VCRUNTIME140.dll'] }),
    'dist/lib/duckdb.dll': pe({ imports: ['KERNEL32.dll'], delay: ['MSVCP140.dll'] }),
    'dist/readme.txt': 'not a binary',
    'dist/fake.exe': 'not a binary either',
  });
  const { files, problems } = collect([join(root, 'dist')]);
  assert.deepEqual(problems, []);
  assert.deepEqual(files.map((f) => f.slice(root.length + 1)), ['dist/agent.exe', 'dist/app.exe', 'dist/fake.exe', 'dist/lib/duckdb.dll']);
  const findings = check(files);
  assert.equal(findings.length, 3);
  assert.match(findings[0], /agent\.exe: imports VCRUNTIME140\.dll/);
  assert.match(findings[1], /fake\.exe: not a valid PE file/);
  assert.match(findings[2], /duckdb\.dll: imports MSVCP140\.dll/);
  assert.deepEqual(check([join(root, 'dist/app.exe')]), []);
  assert.match(check([join(root, 'missing.exe')])[0], /cannot be read/);
});

test('collect never throws: an unreadable path or folder is a problem, a linked folder is not followed', { skip: process.platform === 'win32' && 'needs POSIX permissions and symlinks' }, () => {
  const root = makeApp({ 'a/x.exe': pe({ imports: SYSTEM }), 'outside/y.exe': pe({ imports: ['VCRUNTIME140.dll'] }), 'locked/z.exe': pe() });
  symlinkSync(join(root, 'outside'), join(root, 'a/linked-folder'));
  symlinkSync(join(root, 'a'), join(root, 'a/loop'));
  symlinkSync(join(root, 'outside/y.exe'), join(root, 'a/linked.exe'));
  symlinkSync(join(root, 'nowhere.exe'), join(root, 'a/broken.exe'));
  chmodSync(join(root, 'locked'), 0o000);
  try {
    let r;
    assert.doesNotThrow(() => { r = collect([join(root, 'a'), join(root, 'locked'), join(root, 'missing')]); });
    assert.deepEqual(r.files.map((f) => f.slice(root.length + 1)), ['a/linked.exe', 'a/x.exe'], 'linked folders are not entered; a linked file is read');
    assert.equal(r.problems.length, process.getuid?.() === 0 ? 2 : 3, r.problems.join('\n'));
    assert.ok(r.problems.some((p) => /broken\.exe: a broken symbolic link/.test(p)));
    assert.ok(r.problems.some((p) => /missing: cannot be read \(ENOENT\)/.test(p)));
  } finally {
    chmodSync(join(root, 'locked'), 0o755);
  }
});

/** An app folder as `tauri build` leaves it: src-tauri with its configuration, a build folder with the binaries. */
function builtApp({ conf = {}, windowsConf, extraConf, cargo = '[package]\nname = "demo-app"\nversion = "0.1.0"\n', bins = {}, resources = {} } = {}) {
  const files = {
    'src-tauri/tauri.conf.json': JSON.stringify({ productName: 'Demo', ...conf }),
    'src-tauri/Cargo.toml': cargo,
    ...Object.fromEntries(Object.entries(bins).map(([n, b]) => [`target/dist/${n}`, b])),
    ...Object.fromEntries(Object.entries(resources).map(([n, b]) => [`src-tauri/${n}`, b])),
  };
  if (windowsConf) files['src-tauri/tauri.windows.conf.json'] = JSON.stringify(windowsConf);
  if (extraConf) files['src-tauri/extra.conf.json'] = JSON.stringify(extraConf);
  const root = makeApp(files);
  return { root, srcTauri: join(root, 'src-tauri'), binDir: join(root, 'target/dist') };
}

test('shippedFiles: the app, its sidecars and its .exe/.dll resources, named by the configuration; stale files are never read', () => {
  const clean = pe({ imports: SYSTEM });
  const dynamic = pe({ imports: ['VCRUNTIME140.dll'] });
  const app = builtApp({
    conf: { mainBinaryName: 'demo', bundle: { externalBin: ['binaries/demo-agent'], resources: { 'binaries/lib/': 'lib/', 'notes.md': 'notes.md' } } },
    bins: { 'demo.exe': clean, 'demo-agent.exe': clean, 'old-sidecar.exe': dynamic, 'demo-app.exe': dynamic },
    resources: { 'binaries/lib/duckdb.dll': clean, 'binaries/lib/readme.txt': '', 'notes.md': '' },
  });
  const r = shippedFiles(app);
  assert.deepEqual(r.problems, []);
  assert.deepEqual(r.files.map((f) => f.slice(app.root.length + 1)), ['target/dist/demo.exe', 'target/dist/demo-agent.exe', 'src-tauri/binaries/lib/duckdb.dll']);
  assert.deepEqual(check(r.files), [], 'old-sidecar.exe and demo-app.exe are stale: not shipped, not checked');
});

test('shippedFiles: the main binary falls back to default-run, then the package name; a missing one is an error', () => {
  const clean = pe({ imports: SYSTEM });
  const byRun = builtApp({ cargo: '[package]\nname = "pkg"\ndefault-run = "runner"\n\n[[bin]]\nname = "runner"\n', bins: { 'runner.exe': clean } });
  assert.deepEqual(shippedFiles(byRun).files.map((f) => f.slice(byRun.root.length + 1)), ['target/dist/runner.exe']);
  const byPkg = builtApp({ bins: { 'demo-app.exe': clean } });
  assert.deepEqual(shippedFiles(byPkg).problems, []);
  const none = builtApp({ conf: { bundle: { externalBin: ['binaries/agent'] } }, bins: { 'stale.exe': clean } });
  const r = shippedFiles(none);
  assert.deepEqual(r.files, []);
  assert.match(r.problems[0], /the app's binary is missing: none of demo-app\.exe in/);
  assert.match(r.problems[1], /the sidecar binaries\/agent is missing: no agent\.exe in/);
});

test('shippedFiles merges tauri.windows.conf.json and --config files over tauri.conf.json, like Tauri', () => {
  const clean = pe({ imports: SYSTEM });
  const app = builtApp({
    conf: { bundle: { externalBin: ['binaries/a'] } },
    windowsConf: { bundle: { resources: ['res/*.dll'] } },
    extraConf: { bundle: { externalBin: ['binaries/b'] } },
    bins: { 'demo-app.exe': clean, 'a.exe': clean, 'b.exe': pe({ imports: ['msvcp140.dll'] }) },
    resources: { 'res/one.dll': clean, 'res/two.txt': '' },
  });
  const r = shippedFiles({ ...app, configs: [join(app.srcTauri, 'extra.conf.json')] });
  assert.deepEqual(r.problems, []);
  assert.deepEqual(r.files.map((f) => f.slice(app.root.length + 1)), ['target/dist/demo-app.exe', 'target/dist/b.exe', 'src-tauri/res/one.dll'], 'the --config externalBin replaces the array, as a merge patch does');
  assert.match(check(r.files)[0], /b\.exe: imports msvcp140\.dll/);
  assert.deepEqual(mergePatch({ a: { b: 1, c: 2 }, d: [1] }, { a: { c: null, e: 3 }, d: [2] }), { a: { b: 1, e: 3 }, d: [2] });
  assert.deepEqual(cargoBinaryNames('[package]\nname = "x"\n\n[dependencies]\nname = "y"\n'), ['x']);
  const broken = builtApp({ bins: { 'demo-app.exe': clean } });
  writeFileSync(join(broken.srcTauri, 'tauri.conf.json'), '{ not json');
  assert.doesNotThrow(() => shippedFiles(broken));
  assert.match(shippedFiles(broken).problems[0], /tauri\.conf\.json: not valid JSON/);
});

test('the CLI: exit 0 when clean, 1 on a finding, 2 on a usage error or nothing to check', () => {
  const root = makeApp({ 'ok/a.exe': pe({ imports: SYSTEM }), 'bad/b.exe': pe({ imports: ['MSVCP140.dll'] }), 'empty/x.txt': '' });
  const run = (...a) => spawnSync(process.execPath, [CLI, ...a], { encoding: 'utf8' });
  const ok = run(join(root, 'ok'));
  assert.equal(ok.status, 0, ok.stdout + ok.stderr);
  assert.match(ok.stdout, /1 Windows binaries checked/);
  const bad = run(join(root, 'ok'), join(root, 'bad'));
  assert.equal(bad.status, 1);
  assert.match(bad.stdout, /::error::.*b\.exe: imports MSVCP140\.dll/);
  assert.equal(run(join(root, 'empty')).status, 2);
  assert.equal(run().status, 2);
  assert.equal(run('--src-tauri', root).status, 2, '--src-tauri needs --bin-dir');
  assert.equal(run('--bogus').status, 2);
  const app = builtApp({ bins: { 'demo-app.exe': pe({ imports: SYSTEM }) } });
  const shipped = run('--src-tauri', app.srcTauri, '--bin-dir', app.binDir);
  assert.equal(shipped.status, 0, shipped.stdout + shipped.stderr);
  const missing = run('--src-tauri', app.srcTauri, '--bin-dir', join(app.root, 'nowhere'));
  assert.equal(missing.status, 1);
  assert.match(missing.stdout, /::error::the app's binary is missing/);
});

test('the root cargo configuration links the C runtime statically for every Windows MSVC target', () => {
  const config = readFileSync(join(REPO, '.cargo/config.toml'), 'utf8');
  const table = /^\[target\.'cfg\(all\(windows, target_env = "msvc"\)\)'\]\n((?:[^[].*\n?)*)/m.exec(config);
  assert.ok(table, 'a [target.\'cfg(all(windows, target_env = "msvc"))\'] table');
  assert.match(table[1], /^rustflags = \["-C", "target-feature=\+crt-static"\]$/m);
});

// Tauri's own staticVCRuntime (default true) puts static_vcruntime's link arguments on the app binary: they exclude
// libucrt.lib, the static UCRT that +crt-static links, and the app fails to link (seen with cargo-xwin, 2026-10-30).
test('every app turns Tauri\'s partial static VC runtime off, so +crt-static alone decides', () => {
  const apps = readdirSync(join(REPO, 'apps'));
  const confs = apps.flatMap((a) => ['app/src-tauri', 'src-tauri'].map((d) => join('apps', a, d, 'tauri.conf.json'))).filter((p) => existsSync(join(REPO, p)));
  assert.ok(confs.length >= 7, `found ${confs.length} apps`);
  for (const p of confs) {
    const conf = JSON.parse(readFileSync(join(REPO, p), 'utf8'));
    assert.equal(conf.build?.windows?.staticVCRuntime, false, `${p}: build.windows.staticVCRuntime must be false (ADR 0032)`);
  }
});

test('every build of Windows installers runs the check', () => {
  const read = (p) => readFileSync(join(REPO, p), 'utf8');
  for (const p of ['conformance/ci/tauri-bundle.sh', 'scripts/demo/bundle.sh', '.gitea/workflows/debrief.yml']) {
    assert.match(read(p), /windows-crt-imports\.mjs"? --src-tauri /, `${p} checks the shipped list, not a glob`);
  }
});

// Sighter's ONNX Runtime (its ADR 0001 decision 4): DirectML delay-loaded in the DirectML build, absent in the CPU-only one,
// and no network API in either.
test('the import rules: delay-load, no import and no network, case-insensitively', () => {
  const dml = { delayLoad: ['DirectML.dll', 'd3d12.dll'], noNetwork: true };
  assert.deepEqual(ruleFindings({ imports: SYSTEM, delayImports: ['DIRECTML.DLL', 'd3d12.dll'] }, dml), []);
  assert.deepEqual(ruleFindings({ imports: [...SYSTEM, 'DirectML.dll'], delayImports: [] }, dml), [
    'imports directml.dll directly (it must be delay-loaded)',
    'does not delay-load d3d12.dll',
  ]);
  assert.deepEqual(ruleFindings({ imports: SYSTEM, delayImports: ['WS2_32.dll', 'directml.dll', 'd3d12.dll'] }, dml), [
    'imports ws2_32.dll (a network API)',
  ]);
  const cpu = { noImport: ['DirectML.dll'], noNetwork: true };
  assert.deepEqual(ruleFindings({ imports: SYSTEM, delayImports: [] }, cpu), []);
  assert.deepEqual(ruleFindings({ imports: SYSTEM, delayImports: ['DirectML.dll'] }, cpu), ['imports directml.dll (it must not)']);
  assert.deepEqual(ruleFindings({ imports: ['WinHTTP.dll', 'wininet.dll'], delayImports: [] }, cpu), ['imports winhttp.dll, wininet.dll (a network API)']);
  assert.deepEqual(ruleFindings({ imports: ['WinHTTP.dll'], delayImports: [] }), [], 'no rules, no findings');
  // --not-direct: absent or delay-loaded is fine, an ordinary import is not.
  const nd = { notDirect: ['dxcore.dll'] };
  assert.deepEqual(ruleFindings({ imports: SYSTEM, delayImports: [] }, nd), []);
  assert.deepEqual(ruleFindings({ imports: SYSTEM, delayImports: ['DXCORE.dll'] }, nd), []);
  assert.deepEqual(ruleFindings({ imports: [...SYSTEM, 'dxcore.dll'], delayImports: [] }, nd), ['imports dxcore.dll directly (it must be delay-loaded)']);
});

test('the import rules on synthetic PE files and through the CLI', () => {
  const root = makeApp({
    'gpu.exe': pe({ imports: SYSTEM, delay: ['DirectML.dll'] }),
    'eager.exe': pe({ imports: [...SYSTEM, 'DirectML.dll'] }),
    'cpu.exe': pe({ imports: SYSTEM }),
    'net.exe': pe({ imports: [...SYSTEM, 'WINHTTP.dll'] }),
  });
  const f = (n) => join(root, n);
  assert.deepEqual(check([f('gpu.exe')], { delayLoad: ['DirectML.dll'] }), []);
  assert.deepEqual(check([f('eager.exe')], { delayLoad: ['DirectML.dll'] }), [`${f('eager.exe')}: imports directml.dll directly (it must be delay-loaded)`]);
  assert.deepEqual(check([f('cpu.exe')], { noImport: ['DirectML.dll'], noNetwork: true }), []);
  assert.deepEqual(check([f('gpu.exe')], { noImport: ['DirectML.dll'] }), [`${f('gpu.exe')}: imports directml.dll (it must not)`]);
  assert.deepEqual(check([f('net.exe')], { noNetwork: true }), [`${f('net.exe')}: imports winhttp.dll (a network API)`]);
  const run = (...args) => spawnSync(process.execPath, [CLI, ...args], { encoding: 'utf8' });
  const ok = run('--delay-load', 'DirectML.dll', '--no-network', f('gpu.exe'));
  assert.equal(ok.status, 0, ok.stdout + ok.stderr);
  assert.match(ok.stdout, /each delay-loads DirectML\.dll, imports no network API/);
  assert.equal(run('--no-import', 'DirectML.dll', f('gpu.exe')).status, 1);
  assert.equal(run('--delay-load').status, 2);
  assert.deepEqual(parseArgs(['--no-import', 'a.dll', '--not-direct', 'b.dll', '--no-network', 'x.exe']).rules, { delayLoad: [], notDirect: ['b.dll'], noImport: ['a.dll'], noNetwork: true });
});
