import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const CONTRACTS = (ids) => JSON.stringify({ version: 1, base: [], dialogs: Object.fromEntries(ids.map((i) => [i, { kind: 'viewer', modality: 'modeless', requiredChoice: false }])) });
const AXE_ALL = "import { axeEveryDialog } from '@decryp7/paddock-e2e';\ntest('dialogs', async ({ page }) => { expect(await axeEveryDialog(page, 'dialog-contracts.json')).toEqual({}); });\n";
const KEYS = "import { expectTabOrder } from '@decryp7/paddock-e2e';\ntest('keys', async ({ page }) => { await expectTabOrder(page); });\n";
const ACCENT = "import { accentProblems, cssRules } from '@decryp7/paddock-ui/shell';\nit('accent', () => expect(accentProblems(cssRules(css), 'demo')).toEqual([]));\n";

test('C33 good: axe over every dialog, a keyboard flow and an accent test leave nothing to report', async () => {
  const root = makeApp({
    'app/e2e/a11y.spec.ts': AXE_ALL + KEYS,
    'app/src/theme.spec.ts': ACCENT,
    'app/src-tauri/dialog-contracts.json': CONTRACTS(['pd.about', 'db.rename']),
  });
  assert.deepEqual(await findingsOf('C33', root), []);
});

test('C33 bad: no spec runs axe at all', async () => {
  const f = await findingsOf('C33', makeApp({ 'app/e2e/home.spec.ts': "test('home', async ({ page }) => { await page.goto('/'); });\n" }));
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'error');
  assert.match(messages(f), /no e2e spec runs axe/);
});

test('C33 bad: a dialog nobody checks is named; naming it in a spec that runs axe covers it', async () => {
  const files = {
    'app/e2e/home.spec.ts': "import { axeViolations } from '@decryp7/paddock-e2e';\ntest('home', async ({ page }) => { expect(await axeViolations(page)).toEqual([]); });\n" + KEYS,
    'app/src/theme.spec.ts': ACCENT,
    'app/src-tauri/dialog-contracts.json': CONTRACTS(['pd.about', 'db.rename']),
  };
  const f = await findingsOf('C33', makeApp(files));
  assert.deepEqual(f.map((x) => x.severity), ['error', 'error']);
  assert.match(messages(f), /dialog db\.rename is not checked/);
  const covered = await findingsOf('C33', makeApp({ ...files, 'app/e2e/dialogs.spec.ts': "import { axeViolations } from '@decryp7/paddock-e2e';\n// 'pd.about' 'db.rename'\ntest('d', async () => { await axeViolations(page); });\n" }));
  assert.deepEqual(covered, []);
});

test('C33: a skipped dialog needs a reason and must still exist; a broken contract file is an error', async () => {
  const base = { 'app/e2e/a11y.spec.ts': "import { axeViolations } from '@decryp7/paddock-e2e';\nawait axeViolations(page);\n" + KEYS, 'app/src/theme.spec.ts': ACCENT };
  const ok = makeApp({ ...base, 'app/src-tauri/dialog-contracts.json': CONTRACTS(['db.reset']), 'paddock.conformance.json': JSON.stringify({ a11y: { skip: { 'db.reset': 'opens only after a reset' } } }) });
  assert.deepEqual(await findingsOf('C33', ok), []);
  const bad = makeApp({ ...base, 'app/src-tauri/dialog-contracts.json': CONTRACTS(['db.reset']), 'paddock.conformance.json': JSON.stringify({ a11y: { skip: { 'db.reset': ' ', 'db.gone': 'old' } } }) });
  assert.match(messages(await findingsOf('C33', bad)), /db\.gone, which is not a dialog any more[\s\S]*db\.reset gives no reason|db\.reset gives no reason[\s\S]*db\.gone/);
  const broken = await findingsOf('C33', makeApp({ ...base, 'app/src-tauri/dialog-contracts.json': '{ nope' }));
  assert.match(messages(broken), /not readable/);
});

test('C33 warns, not fails, when there is no keyboard flow or accent test', async () => {
  const f = await findingsOf('C33', makeApp({ 'app/e2e/a11y.spec.ts': AXE_ALL, 'app/src-tauri/dialog-contracts.json': CONTRACTS(['pd.about']) }));
  assert.deepEqual(f.map((x) => x.severity), ['warn', 'warn']);
  assert.match(messages(f), /keyboard-only flow[\s\S]*accentProblems/);
});

test('C33 reads the generated dialog-contracts.ts of a dialogs! table', async () => {
  const TS = (ids) => `export const DIALOG_CONTRACTS = {\n${ids.map((id) => `  '${id}': {\n    id: '${id}',\n  },\n`).join('')}} as const;\n`;
  const AXE_ONE = "import { axeViolations } from '@decryp7/paddock-e2e';\ntest('a', async ({ page }) => { await page.goto('/#/dialog?id=pd.about'); expect(await axeViolations(page)).toEqual([]); });\n";
  const f = await findingsOf('C33', makeApp({ 'app/e2e/a11y.spec.ts': AXE_ONE, 'app/src/app/generated/dialog-contracts.ts': TS(['pd.about', 'db.rename']) }));
  assert.ok(f.some((x) => x.message.includes('db.rename is not checked')), JSON.stringify(f));
  assert.ok(!f.some((x) => x.message.includes('pd.about is not checked')), JSON.stringify(f));
  const broken = await findingsOf('C33', makeApp({ 'app/e2e/a11y.spec.ts': AXE_ONE, 'app/src/app/generated/dialog-contracts.ts': 'export const X = 1;' }));
  assert.ok(broken.some((x) => x.message.includes('not readable')), JSON.stringify(broken));
});
