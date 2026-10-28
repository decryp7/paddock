import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { dialogTargets } from './a11y.ts';
import { dialogContracts, dialogWindowUrl, parseDialogContractsModule } from './shell.ts';

/** A `dialog-contracts.ts` in the shape `paddock_dialogs::ts::contracts_module` writes. */
function moduleText(ids: string[]): string {
  const entry = (id: string) =>
    `  '${id}': {\n    id: '${id}',\n    kind: 'viewer',\n    modality: 'modeless',\n    width: 720,\n    widthStep: 'wide',\n    height: 560,\n    instanced: false,\n    requiredChoice: false,\n    commands: ['about_info', 'updates_*'],\n  },\n`;
  return `// Generated.\n\nexport const DIALOG_CONTRACTS = {\n${ids.map(entry).join('')}} as const;\n\nexport type DialogId = keyof typeof DIALOG_CONTRACTS;\n`;
}

function contracts(ids: string[]): { dir: string; path: string } {
  const dir = mkdtempSync(join(tmpdir(), 'paddock-e2e-a11y-'));
  const path = join(dir, 'dialog-contracts.ts');
  writeFileSync(path, moduleText(ids));
  return { dir, path };
}

test('a generated contracts module reads as its entries, and several merge', () => {
  const read = parseDialogContractsModule(moduleText(['pd.about']));
  assert.deepEqual(read['pd.about'], {
    id: 'pd.about',
    kind: 'viewer',
    modality: 'modeless',
    width: 720,
    widthStep: 'wide',
    height: 560,
    instanced: false,
    requiredChoice: false,
    commands: ['about_info', 'updates_*'],
  });
  assert.deepEqual(parseDialogContractsModule('export const DIALOG_CONTRACTS = {\n} as const;\n'), {});
  assert.throws(() => parseDialogContractsModule('export const X = 1;'), /not a generated dialog-contracts/);
  assert.throws(() => parseDialogContractsModule('export const DIALOG_CONTRACTS = { a: b } as const;'), /could not be read/);
  const one = contracts(['pd.about']);
  const two = contracts(['sd.rename']);
  try {
    assert.deepEqual(Object.keys(dialogContracts(one.path, two.path)), ['pd.about', 'sd.rename']);
  } finally {
    rmSync(one.dir, { recursive: true, force: true });
    rmSync(two.dir, { recursive: true, force: true });
  }
});

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
    writeFileSync(path, moduleText([]));
    assert.throws(() => dialogTargets([path]), /lists no dialog/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
