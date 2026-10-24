import assert from 'node:assert/strict';
import { test } from 'node:test';
import { crateDirOf, externalBins } from '../src/checks/C34-sidecar-self-test.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const conf = (...bins) => JSON.stringify({ productName: 'Demo', version: '1.0.0', bundle: { externalBin: bins.map((b) => `binaries/${b}`) } });
const crate = (name, main) => ({ [`crates/${name}/Cargo.toml`]: `[package]\nname = "${name}"\n`, [`crates/${name}/src/main.rs`]: main });
const config = (...names) => JSON.stringify({ app: 'demo', sidecars: names.map((name) => ({ name, args: ['--self-test'] })) });
const app = (files) => makeApp({ 'app/src-tauri/tauri.conf.json': conf('svc'), 'paddock.conformance.json': config('svc'), ...files });

test('externalBins reads the names and survives junk', () => {
  assert.deepEqual(externalBins(conf('a', 'b')), ['a', 'b']);
  assert.deepEqual(externalBins('{ nope'), []);
  assert.deepEqual(externalBins('{}'), []);
  assert.deepEqual(externalBins(JSON.stringify({ bundle: { externalBin: [1, 'x/y'] } })), ['y']);
});

test('C34 good: declared, and the crate handles the flag (either way)', async () => {
  assert.deepEqual(await findingsOf('C34', app(crate('svc', 'fn main() { if let Some(c) = paddock_proc::self_test::handle("svc", "1", &[]) { std::process::exit(c) } }'))), []);
  assert.deepEqual(await findingsOf('C34', app(crate('svc', 'fn main() { if args.first() == Some("--self-test") {} }'))), []);
});

test('C34 bad: not declared for CI, and not implemented', async () => {
  const notDeclared = await findingsOf('C34', app({ ...crate('svc', 'fn main() { self_test() }'), 'paddock.conformance.json': config() }));
  assert.match(messages(notDeclared), /not in "sidecars"/);
  const notImplemented = await findingsOf('C34', app(crate('svc', 'fn main() { run() }')));
  assert.equal(notImplemented.length, 1);
  assert.match(notImplemented[0].message, /svc does not implement --self-test/);
  assert.equal(notImplemented[0].file, 'crates/svc/Cargo.toml');
});

test('C34: a binary named by [[bin]] is found; a crate that cannot be found is a warning; no sidecars is fine', async () => {
  const bin = { 'crates/worker/Cargo.toml': '[package]\nname = "worker-crate"\n[[bin]]\nname = "svc"\npath = "src/main.rs"\n', 'crates/worker/src/main.rs': 'fn main() { self_test::handle() }' };
  assert.deepEqual(await findingsOf('C34', app(bin)), []);
  const missing = await findingsOf('C34', app({}));
  assert.equal(missing.length, 1);
  assert.equal(missing[0].severity, 'warn');
  assert.deepEqual(await findingsOf('C34', makeApp({ 'app/src-tauri/tauri.conf.json': JSON.stringify({ version: '1.0.0' }) })), []);
});

test('crateDirOf finds the package or the bin, and the root crate', () => {
  const ctx = { files: () => ['Cargo.toml', 'x/Cargo.toml'], read: (f) => (f === 'Cargo.toml' ? '[package]\nname = "root"\n' : '[package]\nname = "a"\n[[bin]]\nname = "b"\n') };
  assert.equal(crateDirOf(ctx, 'root'), '');
  assert.equal(crateDirOf(ctx, 'b'), 'x');
  assert.equal(crateDirOf(ctx, 'zzz'), null);
});
