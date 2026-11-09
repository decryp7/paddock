// ADR 0073 / rule 73: `test-support` is enabled only through pitwall-server's self dev-dependency, never on the shipped
// graph (`cargo tree -e normal`). The control: the dev graph does show it, so the probe can see the feature.
import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const tree = (edges, extra = []) =>
  execFileSync('cargo', ['tree', '-p', 'pitwall-server', '-e', edges, '-f', '{p} [{f}]', '--prefix', 'none', ...extra], {
    cwd: root,
    encoding: 'utf8',
  });

test('the dev graph enables test-support on pitwall-server (the probe can see it)', () => {
  const dev = tree('normal,dev');
  assert.match(dev, /^pitwall-server v[^\n]*\[[^\]]*test-support/m);
});

test('the shipped graph never enables test-support', () => {
  const shipped = tree('normal');
  assert.doesNotMatch(shipped, /test-support/);
  assert.match(shipped, /^pitwall-server v/m);
});
