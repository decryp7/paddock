import assert from 'node:assert/strict';
import { test } from 'node:test';
import C144, { bareStreamWaits } from '../src/checks/C144-bare-stream-wait.mjs';

const ctxOf = (files) => ({ files: () => Object.keys(files), read: (f) => files[f] ?? null });
const SPEC = 'apps/debrief/app/e2e/ai-chat.e2e.ts';

test('C144 bad: each form of a wait for the stream to finish', () => {
  assert.equal(bareStreamWaits("await expect(log).toHaveAttribute('aria-busy', 'false');\n").length, 1);
  assert.equal(bareStreamWaits('await expect(log).toHaveAttribute("aria-busy", "false", { timeout: 60_000 });\n').length, 1);
  assert.equal(bareStreamWaits("await page.waitForSelector('[role=log][aria-busy=\"false\"]');\n").length, 1);
  assert.equal(bareStreamWaits("await expect(page.locator('[role=log][aria-busy=false]')).toBeVisible();\n").length, 1);
  assert.equal(bareStreamWaits("await page.waitForFunction(() => !!document.querySelector('[aria-busy=\"false\"]'));\n")[0].line, 1);
  assert.equal(bareStreamWaits('x();\nawait expect(a).toHaveAttribute(`aria-busy`, `false`);\n')[0].line, 2);
  assert.equal(bareStreamWaits("await expect(a).toHaveAttribute(\n  'aria-busy',\n  'false',\n);\n").length, 1);
});

test('C144 good: the helper, comments, other attributes and a waiver', () => {
  assert.deepEqual(bareStreamWaits('await expectStreamDone(log);\n'), []);
  assert.deepEqual(bareStreamWaits("// toHaveAttribute('aria-busy', 'false') is what the helper does\n"), []);
  assert.deepEqual(bareStreamWaits('/* [aria-busy=false] is read by\n * expectStreamDone */\n'), []);
  assert.deepEqual(bareStreamWaits("await expect(a).toHaveAttribute('aria-expanded', 'false');\n"), []);
  assert.deepEqual(bareStreamWaits("await expect(x).toBeVisible(); // aria-busy=false later\n"), []);
  assert.deepEqual(bareStreamWaits("// conformance: stream-wait — not a stream, a settled panel\nawait expect(p).toHaveAttribute('aria-busy', 'false');\n"), []);
  assert.deepEqual(bareStreamWaits("await expect(p.locator('[aria-busy=false]')).toBeVisible(); // conformance: stream-wait — panel, no stream\n"), []);
});

test('C144 good: busy asserted true, skeleton counts, reads and negated-true settle waits are not stream-finished waits', () => {
  assert.deepEqual(bareStreamWaits("await expect(log, 'busy while the turn runs').toHaveAttribute('aria-busy', 'true');\n"), []);
  assert.deepEqual(bareStreamWaits("await expect(page.locator('section.pd-w[aria-busy=true]')).toHaveCount(5);\n"), []);
  assert.deepEqual(bareStreamWaits("await expect(popup.locator('[aria-busy=\"true\"]')).toHaveCount(0);\n"), []);
  assert.deepEqual(bareStreamWaits("await expect(page.getByTestId('x').locator('[aria-busy=true]')).toBeVisible();\n"), []);
  assert.deepEqual(bareStreamWaits("await page.waitForFunction(() => !document.querySelector('[aria-busy=\"true\"]'));\n"), []);
  assert.deepEqual(bareStreamWaits("const b = await log.getAttribute('aria-busy');\n"), []);
});

test('C144 a waiver without a reason does not count', () => {
  assert.equal(bareStreamWaits("// conformance: stream-wait\nawait expect(p).toHaveAttribute('aria-busy', 'false');\n").length, 1);
});

test('C144 run: specs and the kit are read; the helper, its tests and other files are not', () => {
  const bad = "await expect(log).toHaveAttribute('aria-busy', 'false');\n";
  const found = C144.run(ctxOf({
    [SPEC]: `x\n${bad}`,
    'ui/e2e/a.spec.ts': bad,
    'ui/projects/paddock-e2e/src/other.ts': bad,
    'ui/projects/paddock-e2e/src/ready.ts': bad,
    'ui/projects/paddock-e2e/src/ready.test.ts': bad,
    'ui/projects/paddock-e2e/selftest/stream-done.spec.ts': bad,
    'apps/debrief/app/src/app/chat.ts': bad,
    'docs/x.md': bad,
  }));
  assert.deepEqual(found.map((f) => f.file).sort(), [SPEC, 'ui/e2e/a.spec.ts', 'ui/projects/paddock-e2e/src/other.ts'].sort());
  assert.equal(found.find((f) => f.file === SPEC).line, 2);
  assert.equal(found[0].severity, 'warn');
  assert.match(found[0].fix, /expectStreamDone/);
});
