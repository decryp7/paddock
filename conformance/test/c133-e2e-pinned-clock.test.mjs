import assert from 'node:assert/strict';
import { test } from 'node:test';
import C133, { bareOptOuts, emptySetWaits, liveSteps } from '../src/checks/C133-e2e-pinned-clock.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

test('C133 rule 1: pinnedClock false and pin false need `pinned-clock: off — <why>` on the line or the line above', () => {
  assert.deepEqual(bareOptOuts('test.use({ pinnedClock: false });\n').map((s) => s.line), [1]);
  assert.deepEqual(bareOptOuts('const c = await newPinnedContext(browser, { pin: false });\n').map((s) => s.line), [1]);
  assert.deepEqual(bareOptOuts('// pinned-clock: off — counts real animation frames\ntest.use({ pinnedClock: false });\n'), []);
  assert.deepEqual(bareOptOuts('test.use({ pinnedClock: false }); // pinned-clock: off — counts real frames\n'), [], 'the same line');
  assert.deepEqual(bareOptOuts('test.use({ pinnedClock: false }); /* pinned-clock: off — counts real frames */\n'), [], 'a block comment');
  assert.deepEqual(bareOptOuts('// the sampler counts real frames, so it needs the real time\ntest.use({ pinnedClock: false });\n').map((s) => s.line), [2], 'prose is not the waiver form');
  assert.deepEqual(bareOptOuts('// pinned-clock: off — x\ntest.use({ pinnedClock: false });\n').map((s) => s.line), [2], 'a reason of under ten characters');
  assert.deepEqual(bareOptOuts('// pinnedClock: false\n"pinnedClock: false"\n'), [], 'a comment or a string is not an opt-out');
});

const spec = (body) => `import { pauseLive, withLivePaused, clickWithLivePaused } from '@decryp7/paddock-e2e';\ntest('a row menu', async ({ page }) => {\n${body}\n});\n`;

test('C133 rule 2: in a live-page spec the first interaction is inside withLivePaused or after pauseLive', () => {
  assert.deepEqual(liveSteps(spec("  await page.getByRole('row').focus();\n  await pauseLive(page);")).map((s) => s.line), [3]);
  assert.deepEqual(liveSteps(spec("  await pauseLive(page);\n  await page.keyboard.press('Shift+F10');")), []);
  assert.deepEqual(liveSteps(spec("  await withLivePaused(page, async () => {\n    await row.click();\n  });")), []);
  assert.deepEqual(liveSteps(spec("  await withLivePaused(page, async () => {\n    await expect(row).toBeVisible();\n  });\n  await row.click();")).map((s) => s.line), [6], 'after the callback has ended it is not paused');
  assert.deepEqual(liveSteps(spec('  await clickWithLivePaused(page, row);')), []);
  assert.deepEqual(liveSteps(spec('  await row.tap();')).map((s) => s.line), [3], 'a tap is an interaction');
  assert.deepEqual(liveSteps(spec('  await expect(row).toBeVisible();')), [], 'nothing to overtake');
  assert.deepEqual(liveSteps(spec('  // conformance: e2e-live — drives the updates with clock.runFor itself\n  await row.click();')), []);
});

test('C133 rule 2: a live-page spec is one that imports the helpers, not one that mentions them', () => {
  const body = "test('a', async ({ page }) => {\n  await row.click();\n});\n";
  assert.deepEqual(liveSteps(body), []);
  assert.deepEqual(liveSteps(`// pauseLive is not used here\n${body}`), [], 'a comment is not an import');
  assert.deepEqual(liveSteps(`import { pauseLive } from '@decryp7/paddock-e2e';\n${body}`).map((s) => s.line), [3]);
});

test('C133 rule 3: waitForFunction over every() with no length check', () => {
  assert.deepEqual(emptySetWaits("await page.waitForFunction(() => Array.from(document.querySelectorAll('pd-table')).every((t) => t.clientWidth));\n").map((s) => s.line), [1]);
  assert.deepEqual(emptySetWaits("await page.waitForFunction(() => {\n  const t = [...document.querySelectorAll('x')];\n  return t.length > 0 && t.every((e) => e.ok);\n});\n"), [], 'a length check');
  assert.deepEqual(emptySetWaits("await page.waitForFunction(() => document.fonts.ready);\n"), [], 'no every()');
  assert.deepEqual(emptySetWaits("// conformance: e2e-wait — the set is never empty here\nawait page.waitForFunction(() => rows.every((r) => r.ok));\n"), []);
  assert.deepEqual(emptySetWaits("// waitForFunction(() => a.every(b))\n"), []);
});

// The hermetic control: a tree in memory with each defect planted; the check sees each one and nothing in the good files.
test('C133 sees a planted opt-out without a reason, an unpaused live step and an empty-set wait in a temporary tree, and only those', async () => {
  const live = "import { pauseLive } from '@decryp7/paddock-e2e';\n";
  const root = makeApp({
    'apps/demo/app/e2e/optout.spec.ts': "import { test } from './test';\ntest.use({ pinnedClock: false });\n",
    'apps/demo/app/e2e/live.spec.ts': `${live}test('a', async ({ page }) => {\n  await row.click();\n});\n`,
    'apps/demo/app/e2e/wait.spec.ts': "import { test } from './test';\ntest('a', async ({ page }) => {\n  await page.waitForFunction(() => rows.every((r) => r.ok));\n});\n",
    'apps/demo/app/e2e/good.spec.ts': `// pinned-clock: off — measures real frame cost\ntest.use({ pinnedClock: false });\n${live}test('a', async ({ page }) => {\n  await pauseLive(page);\n  await row.click();\n  await page.waitForFunction(() => rows.length > 0 && rows.every((r) => r.ok));\n});\n`,
    'apps/demo/app/e2e/test.ts': 'export const x = { pinnedClock: false };\n',
    'apps/demo/app/src/app/core/mock-backend.ts': 'export const stamp = () => Date.now();\n',
  });
  const found = await findingsOf('C133', root);
  assert.deepEqual(
    found.map((f) => [f.file, f.line, f.severity]).sort(),
    [
      ['apps/demo/app/e2e/live.spec.ts', 3, 'warn'],
      ['apps/demo/app/e2e/optout.spec.ts', 2, 'warn'],
      ['apps/demo/app/e2e/wait.spec.ts', 3, 'warn'],
    ],
  );
  assert.deepEqual(found.map((f) => /\(rule (\d)\)/.exec(f.message)?.[1]).sort(), ['1', '2', '3']);
});

test('C133 is report-only: its findings are warnings', () => {
  assert.equal(C133.mode, 'report-only');
});
