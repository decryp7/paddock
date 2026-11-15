// Tests of the other-os member gate scripts (docs/adr/amendments/0087-member-gate-other-os-2026-11-16.md): what can be checked
// without Docker or the VM. The runs themselves are shown in the amendment's measurements.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const read = (f) => readFileSync(join(here, f), 'utf8');

for (const f of ['member-gate-linux.sh', 'member-gate-linux-inner.sh', 'member-gate-windows.sh']) {
  test(`${f} parses`, () => {
    assert.equal(spawnSync('bash', ['-n', join(here, f)]).status, 0);
  });
}

test('the Linux run builds and tests in its own writable fresh clone, never in the read-only /work mount', () => {
  // A test that writes into the repository's .git (wrapper::the_cached_tool_holds_itself..., which takes a waiter lock there) fails
  // with "Read-only file system" on linux-container.sh's read-only mounts, 5 of 5; CI has a writable clone. The inner script must
  // run every spec in the clone it made and never `cd /work`.
  const s = read('member-gate-linux-inner.sh');
  assert.match(s, /git init -q "\$src"/);
  assert.match(s, /fetch -q --depth 1 file:\/\/\/work/);
  assert.match(s, /cd "\$src\/\$a" && cargo nextest run/);
  assert.doesNotMatch(s, /cd \/work/);
});

test('every script refuses a call without a spec with exit 2 before it touches Docker or the VM', () => {
  for (const f of ['member-gate-linux.sh', 'member-gate-windows.sh']) {
    const r = spawnSync('bash', [join(here, f), 'sometree'], { encoding: 'utf8' });
    assert.equal(r.status, 2, `${f}: ${r.stderr}`);
    assert.match(r.stderr, /::error::.*no spec given/);
  }
});
