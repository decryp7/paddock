import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { dialogTargets } from './a11y.ts';
import { dialogWindowUrl } from './shell.ts';

function contracts(ids: string[]): { dir: string; path: string } {
  const dir = mkdtempSync(join(tmpdir(), 'paddock-e2e-a11y-'));
  const path = join(dir, 'dialog-contracts.json');
  writeFileSync(path, JSON.stringify({ version: 1, base: [], dialogs: Object.fromEntries(ids.map((id) => [id, { kind: 'viewer', modality: 'modeless', requiredChoice: false }])) }));
  return { dir, path };
}

test('every dialog of the contract file is a target, in id order, with its window URL and input', () => {
  const { dir, path } = contracts(['sd.rename', 'pd.about', 'sd.confirm']);
  try {
    const targets = dialogTargets(path, { inputs: { 'sd.rename': { name: 'x y' } } });
    assert.deepEqual(targets.map((t) => t.name), ['pd.about', 'sd.confirm', 'sd.rename']);
    assert.equal(targets[0]?.url, dialogWindowUrl('pd.about', {}, { title: 'pd.about' }));
    assert.match(targets[2]?.url ?? '', /dialogInput=%7B%22name%22%3A%22x%20y%22%7D$/);
    assert.match(targets[0]?.url ?? '', /^\/#\/dialog\?id=pd\.about&host=main/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a skipped dialog is left out by name, and a contract file without dialogs is an error, not an empty pass', () => {
  const { dir, path } = contracts(['pd.about', 'sd.confirm']);
  try {
    assert.deepEqual(dialogTargets(path, { skip: { 'sd.confirm': 'needs a pending reset' } }).map((t) => t.name), ['pd.about']);
    writeFileSync(path, JSON.stringify({ version: 1 }));
    assert.throws(() => dialogTargets(path), /dialogs/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
