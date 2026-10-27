import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import { cargoTargetDir } from '../src/sidecar/sidecar.mjs';

test('cargoTargetDir: the variable wins and cargo is not asked', () => {
  const never = () => {
    throw new Error('cargo metadata must not run when CARGO_TARGET_DIR is set');
  };
  assert.equal(cargoTargetDir('/w', { CARGO_TARGET_DIR: '/env/target' }, never), resolve('/env/target'));
});

test('cargoTargetDir: a build.target-dir from the cargo configuration (CI host runners) is what cargo metadata reports', () => {
  assert.equal(cargoTargetDir('/w', {}, () => JSON.stringify({ target_directory: '/kept/target-app-bundle' })), '/kept/target-app-bundle');
});

test('cargoTargetDir: falls back to <root>/target when cargo is missing or answers nonsense', () => {
  assert.equal(cargoTargetDir('/w', {}, () => 'not json'), join('/w', 'target'));
  assert.equal(cargoTargetDir('/w', {}, () => JSON.stringify({})), join('/w', 'target'));
  assert.equal(
    cargoTargetDir('/w', {}, () => {
      throw new Error('cargo: not found');
    }),
    join('/w', 'target'),
  );
});

test('cargoTargetDir: real cargo honours .cargo/ci.toml outside the checkout and a CARGO_TARGET_DIR override', (t) => {
  try {
    execFileSync('cargo', ['--version'], { stdio: 'ignore', timeout: 30000 });
  } catch {
    t.skip('cargo is not installed');
    return;
  }
  const dir = mkdtempSync(join(tmpdir(), 'target-dir-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const ws = join(dir, 'ws');
  const kept = join(dir, 'kept');
  mkdirSync(join(ws, 'src'), { recursive: true });
  mkdirSync(join(ws, '.cargo'));
  writeFileSync(join(ws, 'Cargo.toml'), '[package]\nname = "probe"\nversion = "0.0.0"\nedition = "2021"\n');
  writeFileSync(join(ws, 'src', 'main.rs'), 'fn main() {}\n');
  writeFileSync(join(ws, '.cargo', 'config.toml'), 'include = [{ path = "ci.toml", optional = true }]\n');
  const clean = { ...process.env };
  delete clean.CARGO_TARGET_DIR;
  delete clean.CARGO_BUILD_TARGET_DIR;
  const real = (p) => execFileSync('node', ['-e', `console.log(require('fs').realpathSync(${JSON.stringify(p)}))`], { encoding: 'utf8' }).trim();
  const run = (env) =>
    cargoTargetDir(ws, env, () => execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1'], { cwd: ws, env, encoding: 'utf8', timeout: 60000 }));
  mkdirSync(kept);
  writeFileSync(join(ws, '.cargo', 'ci.toml'), `[build]\ntarget-dir = '${kept.replaceAll('\\', '/')}'\n`);
  assert.equal(real(run(clean)), real(kept));
  assert.equal(run({ ...clean, CARGO_TARGET_DIR: join(dir, 'env') }), join(dir, 'env'));
});
