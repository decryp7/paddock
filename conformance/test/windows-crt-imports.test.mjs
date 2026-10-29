// ADR 0032: no shipped Windows binary imports the VC++ runtime. The PE files here are synthetic, built byte by byte.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { check, collect, needsRedist, peImports } from '../release/windows-crt-imports.mjs';
import { makeApp, pe } from './helpers.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const CLI = join(HERE, '../release/windows-crt-imports.mjs');
const REPO = join(HERE, '../..');

const SYSTEM = ['KERNEL32.dll', 'api-ms-win-crt-runtime-l1-1-0.dll', 'ntdll.dll'];

test('the redistributable DLLs are recognised; Windows\' own runtime (UCRT, msvcrt) is allowed', () => {
  for (const d of ['VCRUNTIME140.dll', 'vcruntime140_1.dll', 'MSVCP140.dll', 'msvcp140_atomic_wait.dll', 'msvcp140d.dll', 'MSVCR120.dll', 'concrt140.dll', 'vcomp140.dll', 'ucrtbased.dll', 'mfc140u.dll']) {
    assert.ok(needsRedist(d), d);
  }
  for (const d of ['ucrtbase.dll', 'msvcrt.dll', 'api-ms-win-crt-heap-l1-1-0.dll', 'kernel32.dll', 'vcruntime.dll.bak', 'WebView2Loader.dll']) {
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
  const files = collect([join(root, 'dist')]);
  assert.deepEqual(files.map((f) => f.slice(root.length + 1)), ['dist/agent.exe', 'dist/app.exe', 'dist/fake.exe', 'dist/lib/duckdb.dll']);
  const findings = check(files);
  assert.equal(findings.length, 3);
  assert.match(findings[0], /agent\.exe: imports VCRUNTIME140\.dll/);
  assert.match(findings[1], /fake\.exe: not a valid PE file/);
  assert.match(findings[2], /duckdb\.dll: imports MSVCP140\.dll/);
  assert.deepEqual(check([join(root, 'dist/app.exe')]), []);
  assert.match(check([join(root, 'missing.exe')])[0], /cannot be read/);
});

test('the CLI: exit 0 when clean, 1 on an import, 2 when there is nothing to check', () => {
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
    assert.match(read(p), /windows-crt-imports\.mjs/, p);
  }
});
