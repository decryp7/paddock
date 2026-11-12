import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const SH = join(dirname(fileURLToPath(import.meta.url)), 'disk-record.sh');
const fake = (body) => {
  const d = mkdtempSync(join(tmpdir(), 'disk-record-'));
  const dev = join(d, 'dev');
  writeFileSync(dev, `#!/bin/sh\n[ "$1" = disk ] || exit 1\n${body}\n`);
  chmodSync(dev, 0o755);
  return { dev, log: join(d, 'log') };
};
const run = (dev, args) => spawnSync('bash', [SH, ...args], { env: { ...process.env, PADDOCK_DEV: dev }, cwd: '/', encoding: 'utf8' });

test('disk-record calls `disk record <dir> --kind <k> --link <wt>` when ./dev disk exists', () => {
  const g = fake('');
  writeFileSync(g.dev, `#!/bin/sh\n[ "$1" = disk ] || exit 1\n[ "$2" = --help ] && exit 0\necho "$*" > '${g.log}'\n`);
  assert.equal(run(g.dev, ['/b/t', 'build', '/wt']).status, 0);
  assert.equal(readFileSync(g.log, 'utf8').trim(), 'disk record /b/t --kind build --link /wt');
});

test('disk-record never fails its caller: no ./dev disk, a failing record, too few arguments', () => {
  assert.equal(run('/nonexistent/dev', ['/b/t', 'build']).status, 0);
  const failing = fake('[ "$2" = --help ] && exit 0\nexit 1');
  assert.equal(run(failing.dev, ['/b/t', 'build']).status, 0);
  const none = fake('exit 1');
  assert.equal(run(none.dev, ['/b/t', 'build']).status, 0);
  assert.equal(run(none.dev, ['/b/t']).status, 0);
});

test('disk-record uses the repository root\'s dev (from the script\'s place), never the current folder\'s', () => {
  const root = mkdtempSync(join(tmpdir(), 'disk-record-root-'));
  const other = mkdtempSync(join(tmpdir(), 'disk-record-cwd-'));
  mkdirSync(join(root, 'scripts'));
  copyFileSync(SH, join(root, 'scripts', 'disk-record.sh'));
  const mk = (dir, tag) => {
    writeFileSync(join(dir, 'dev'), `#!/bin/sh\n[ "$2" = --help ] && exit 0\necho ${tag} >> "${join(root, 'log')}"\n`);
    chmodSync(join(dir, 'dev'), 0o755);
  };
  mk(root, 'root');
  mk(other, 'cwd');
  const env = { ...process.env };
  delete env.PADDOCK_DEV;
  spawnSync('bash', [join(root, 'scripts', 'disk-record.sh'), '/b/t', 'build'], { env, cwd: other, encoding: 'utf8' });
  assert.equal(readFileSync(join(root, 'log'), 'utf8').trim(), 'root');
});
