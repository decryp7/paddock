import assert from 'node:assert/strict';
import { test } from 'node:test';
import C156, { scansNotHeldStill } from '../src/checks/C156-axe-scan-held-still.mjs';

const ctxOf = (files) => ({ files: () => Object.keys(files), read: (f) => files[f] ?? null });
const BARE = "import AxeBuilder from '@axe-core/playwright';\nconst r = await new AxeBuilder({ page }).withTags(['wcag2a']).analyze();\n";

test('C156 bad (positive control): a scan outside withPageStill is flagged, on its line', () => {
  assert.deepEqual(scansNotHeldStill(BARE), [{ line: 2 }]);
  assert.deepEqual(scansNotHeldStill('const b = new AxeBuilder({ page });\nawait b\n  .include(x)\n  .analyze();\n'), [{ line: 4 }]);
  // A finished withPageStill call earlier in the file does not cover a later scan.
  assert.deepEqual(scansNotHeldStill('await withPageStill(page, () => a.analyze());\nawait b.analyze();\n'), [{ line: 2 }]);
});

test('C156 good: scans inside withPageStill, comments, strings and a waiver', () => {
  assert.deepEqual(scansNotHeldStill('const r = await withPageStill(page, () => new AxeBuilder({ page }).withTags(t).analyze());\n'), []);
  assert.deepEqual(scansNotHeldStill('await withPageStill(page, () =>\n  (inc ? b.include(inc) : b)\n    .analyze(),\n);\n'), []);
  assert.deepEqual(scansNotHeldStill('// new AxeBuilder({ page }).analyze() is what withPageStill wraps\n'), []);
  assert.deepEqual(scansNotHeldStill("const doc = 'call .analyze() through withPageStill';\n"), []);
  assert.deepEqual(scansNotHeldStill('// conformance: axe-still — the test measures the scan of a moving page\nawait b.analyze();\n'), []);
});

test('C156 a waiver without a reason does not count', () => {
  assert.equal(scansNotHeldStill('// conformance: axe-still\nawait b.analyze();\n').length, 1);
});

test('C156 run: e2e specs, ui specs and the kit naming AxeBuilder are read; other files are not', () => {
  const found = C156.run(ctxOf({
    'apps/debrief/app/e2e/a11y.e2e.ts': BARE,
    'ui/e2e/gallery.spec.ts': BARE,
    'ui/projects/paddock-e2e/src/other.ts': BARE,
    'apps/debrief/app/e2e/no-axe.e2e.ts': 'await thing.analyze();\n',
    'apps/debrief/app/src/app/x.ts': BARE,
    'docs/x.md': BARE,
  }));
  assert.deepEqual(found.map((f) => f.file).sort(), ['apps/debrief/app/e2e/a11y.e2e.ts', 'ui/e2e/gallery.spec.ts', 'ui/projects/paddock-e2e/src/other.ts']);
  assert.equal(found[0].severity, 'warn');
  assert.equal(found[0].line, 2);
  assert.match(found[0].fix, /axeViolations|withPageStill/);
});
