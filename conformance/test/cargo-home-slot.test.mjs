import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const script = fileURLToPath(new URL('../ci/cargo-home-slot.sh', import.meta.url));
const hasFlock = process.platform === 'linux' && spawnSync('flock', ['--version']).status === 0;
const run = (env) => spawnSync('bash', [script], { encoding: 'utf8', env: { PATH: process.env.PATH, ...env } });
const setup = () => {
  const root = mkdtempSync(join(tmpdir(), 'cargo-slot-'));
  const mount = join(root, 'sccache');
  const image = join(root, 'image-cargo');
  mkdirSync(mount);
  mkdirSync(join(image, 'bin'), { recursive: true });
  writeFileSync(join(image, 'config.toml'), '[registries]\n');
  return { root, mount, image, envFile: join(root, 'env'), pathFile: join(root, 'path') };
};
const envOf = (s) => ({ PADDOCK_CI_MOUNT: s.mount, PADDOCK_IMAGE_CARGO_HOME: s.image, GITHUB_ENV: s.envFile, GITHUB_PATH: s.pathFile, PADDOCK_CARGO_SLOTS: '2' });

test('without the mount the job keeps the image cargo home and a notice says so', () => {
  const r = run({ PADDOCK_CI_MOUNT: join(tmpdir(), 'no-such-mount-xyz') });
  assert.equal(r.status, 0);
  if (process.platform === 'linux') assert.match(r.stdout, /::notice::cargo-home-slot: no writable/);
});

test('a job takes slot 1, the next concurrent job slot 2, the third finds none and degrades', { skip: !hasFlock && 'needs Linux flock' }, () => {
  const s = setup();
  const a = run(envOf(s));
  assert.equal(a.status, 0, a.stderr);
  assert.match(a.stdout, /slot 1, CARGO_HOME=.*cargo-home\/1/);
  assert.match(readFileSync(s.envFile, 'utf8'), /CARGO_HOME=.*cargo-home\/1/);
  assert.equal(existsSync(join(s.mount, 'cargo-home', '1', 'config.toml')), true, 'the image configuration is copied');
  assert.match(readFileSync(s.pathFile, 'utf8'), /image-cargo\/bin/);
  const b = run(envOf(s));
  assert.match(b.stdout, /slot 2/);
  const c = run(envOf(s));
  assert.match(c.stdout, /::warning::cargo-home-slot: all 2 cargo-home slots/);
  assert.equal((readFileSync(s.envFile, 'utf8').match(/^CARGO_HOME=/gm) ?? []).length, 2, 'the third job set no CARGO_HOME');
  // Stop the holders this test started: the script records each one's pid (CARGO_HOME_SLOT_HOLDER).
  for (const m of readFileSync(s.envFile, 'utf8').matchAll(/CARGO_HOME_SLOT_HOLDER=(\d+)/g)) process.kill(Number(m[1]), 'SIGTERM');
});
