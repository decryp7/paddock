import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { main } from '../src/cli.mjs';
import { inMonorepoApp } from '../src/sync/main.mjs';
import { blockFiles, PADDOCK } from './helpers.mjs';

const write = (root, files) => {
  for (const [p, c] of Object.entries(files)) {
    mkdirSync(dirname(join(root, p)), { recursive: true });
    writeFileSync(join(root, p), c);
  }
};
const snapshot = (root, dir = '') =>
  readdirSync(join(root, dir), { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? snapshot(root, join(dir, e.name)) : [[join(dir, e.name), readFileSync(join(root, dir, e.name), 'utf8')]]));

// ER-14: temp trees only. An app folder in a monorepo carries no per-repository baseline (C28 is not run there), so `sync --write`
// there must touch only the shared blocks whose source changed, and leave every other file byte for byte as it was.
test('sync --write in a monorepo app folder rewrites only the outdated block, not the baseline files', async () => {
  const mono = mkdtempSync(join(tmpdir(), 'sync-blocks-'));
  try {
    for (const d of [join('.gitea'), join('tools', 'paddock-dev')]) mkdirSync(join(mono, d), { recursive: true });
    const root = join(mono, 'apps', 'demo');
    const files = blockFiles('demo');
    files['AGENTS.md'] = files['AGENTS.md'].replace(/design-principles rev=[0-9a-f]{8}/, 'design-principles rev=00000000');
    files['cliff.toml'] = '# the app keeps its own cliff.toml\n';
    files['Cargo.toml'] = '[workspace]\nmembers = []\n';
    write(root, files);
    const before = new Map(snapshot(root));
    assert.equal(inMonorepoApp(root), true);
    const logs = [];
    const code = await main(['sync', '--app', 'demo', '--root', root, '--paddock', PADDOCK, '--write'], { log: (m) => logs.push(m), err: () => {} });
    assert.equal(code, 0);
    const after = new Map(snapshot(root));
    assert.deepEqual([...after.keys()].sort(), [...before.keys()].sort(), 'no file was created');
    const changed = [...after].filter(([p, t]) => before.get(p) !== t).map(([p]) => p);
    assert.deepEqual(changed, ['AGENTS.md']);
    assert.match(after.get('AGENTS.md'), /design-principles rev=(?!00000000)[0-9a-f]{8}/);
  } finally {
    rmSync(mono, { recursive: true, force: true });
  }
});

test('--blocks-only skips the baseline anywhere; a folder outside a monorepo is not a monorepo app', async () => {
  const root = mkdtempSync(join(tmpdir(), 'sync-blocks-'));
  try {
    assert.equal(inMonorepoApp(root), false);
    write(root, { ...blockFiles('demo'), 'cliff.toml': 'own\n' });
    const code = await main(['sync', '--app', 'demo', '--root', root, '--paddock', PADDOCK, '--write', '--blocks-only'], { log: () => {}, err: () => {} });
    assert.equal(code, 0);
    assert.equal(readFileSync(join(root, 'cliff.toml'), 'utf8'), 'own\n');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
