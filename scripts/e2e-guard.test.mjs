import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import guard from './e2e-guard.cjs';
const { assertFreshBuilds, e2ePortFor, PORT_RANGE_START, PORT_SLOTS, reuseServer, staleBuilds, worktreePort } = guard;

const at = (file, sec) => {
  mkdirSync(join(file, '..'), { recursive: true });
  writeFileSync(file, 'x');
  utimesSync(file, sec, sec);
};
const fixture = (cssAt, distAt, scssAt = 1000, tsAt = 1000) => {
  const ui = mkdtempSync(join(tmpdir(), 'e2e-guard-'));
  const pkg = join(ui, 'projects', 'paddock-ui');
  at(join(pkg, 'scss', '_tokens.scss'), scssAt);
  at(join(pkg, 'src', 'a.ts'), tsAt);
  at(join(pkg, 'css', 'material.css'), cssAt);
  at(join(ui, 'dist', 'paddock-ui', 'index.mjs'), distAt);
  return ui;
};

test('reuse is opt-in only', () => {
  assert.equal(reuseServer({}), false);
  assert.equal(reuseServer({ PW_REUSE_SERVER: '' }), false);
  assert.equal(reuseServer({ PW_REUSE_SERVER: '0' }), false);
  assert.equal(reuseServer({ PW_REUSE_SERVER: '1' }), true);
});

test('ports derive from the root, stay in range, differ per config offset, and are fixed on CI', () => {
  const a = worktreePort(4300, '/wt/a', {});
  assert.equal(a, worktreePort(4300, '/wt/a', {}));
  assert.equal(worktreePort(4301, '/wt/a', {}), a + 1);
  assert.ok(a >= PORT_RANGE_START && a < PORT_RANGE_START + PORT_SLOTS + 100);
  const others = new Set(['/wt/b', '/wt/c', '/wt/d', '/wt/e'].map((r) => worktreePort(4300, r, {})));
  assert.ok(others.size > 1);
  assert.equal(worktreePort(4300, '/wt/a', { CI: 'true' }), 4300);
});

test('the environment variable still wins and is validated', () => {
  assert.equal(e2ePortFor('PW_PORT', 4300, { PW_PORT: '4601' }, '/wt/a'), 4601);
  assert.equal(e2ePortFor('PW_PORT', 4300, {}, '/wt/a'), worktreePort(4300, '/wt/a', {}));
  assert.throws(() => e2ePortFor('PW_PORT', 4300, { PW_PORT: 'x' }, '/wt/a'), /is not a port/);
});

test('fresh builds pass; stale css or dist fail with the rebuild hint', () => {
  const fresh = fixture(2000, 2000);
  const staleCss = fixture(500, 2000);
  const staleDist = fixture(2000, 500);
  const staleTs = fixture(2000, 1500, 1000, 1800);
  try {
    assert.deepEqual(staleBuilds(fresh, { checkDist: true }), []);
    assert.match(staleBuilds(staleCss)[0], /css\/material\.css is older/);
    assert.deepEqual(staleBuilds(staleDist), []); // dist is checked only on request
    assert.match(staleBuilds(staleDist, { checkDist: true })[0], /ui\/dist is older/);
    assert.match(staleBuilds(staleTs, { checkDist: true })[0], /ui\/dist is older/);
    assert.throws(() => assertFreshBuilds(staleCss, {}, {}), /rebuild ui: npm run build -w ui/);
    assert.doesNotThrow(() => assertFreshBuilds(staleCss, {}, { PW_SKIP_BUILD_GUARD: '1' }));
  } finally {
    for (const d of [fresh, staleCss, staleDist, staleTs]) rmSync(d, { recursive: true, force: true });
  }
});

test('a spec file newer than dist is not a stale build; a missing dist is', () => {
  const ui = fixture(2000, 2000);
  try {
    at(join(ui, 'projects', 'paddock-ui', 'src', 'a.spec.ts'), 3000);
    assert.deepEqual(staleBuilds(ui, { checkDist: true }), []);
    rmSync(join(ui, 'dist'), { recursive: true });
    assert.match(staleBuilds(ui, { checkDist: true })[0], /missing or empty/);
  } finally {
    rmSync(ui, { recursive: true, force: true });
  }
});
