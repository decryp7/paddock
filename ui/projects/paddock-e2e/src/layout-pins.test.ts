import assert from 'node:assert/strict';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { unpinnedLayoutSpecs } from './layout-pins.ts';

const measuring = "const found = await textCutOffWithoutTooltip(page);\nexpect(found).toEqual([]);";

test('a spec that measures text without pinning the longest value is listed', () => {
  assert.deepEqual(unpinnedLayoutSpecs([{ path: 'a.spec.ts', source: measuring }]), ['a.spec.ts']);
  assert.deepEqual(unpinnedLayoutSpecs([{ path: 'b.spec.ts', source: 'expect(el.scrollWidth).toBeLessThanOrEqual(w)' }]), ['b.spec.ts']);
});

test('a pinned spec, or one that measures no text, is not listed', () => {
  assert.deepEqual(unpinnedLayoutSpecs([{ path: 'a.spec.ts', source: `await pinLongestValues(page);\n${measuring}` }]), []);
  assert.deepEqual(unpinnedLayoutSpecs([{ path: 'a.spec.ts', source: `// layout-pinned: Memory 999.9 of 999 GB\n${measuring}` }]), []);
  assert.deepEqual(unpinnedLayoutSpecs([{ path: 'c.spec.ts', source: "await expect(page).toHaveTitle('x')" }]), []);
});

/** The repository's own e2e specs, report-only: the list is printed as a warning and never fails the run. */
test('report-only: the repository specs that measure text without a pinned longest value', () => {
  const root = join(import.meta.dirname, '..', '..', '..', '..');
  const specs: { path: string; source: string }[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      if (name === 'node_modules' || name === 'dist' || name === 'target' || name.startsWith('.')) continue;
      const p = join(dir, name);
      if (statSync(p).isDirectory()) walk(p);
      else if (p.includes('e2e') && name.endsWith('.spec.ts')) specs.push({ path: p.slice(root.length + 1), source: readFileSync(p, 'utf8') });
    }
  };
  for (const top of ['apps', 'ui']) walk(join(root, top));
  assert.ok(specs.length > 0, 'the e2e specs were found');
  const unpinned = unpinnedLayoutSpecs(specs);
  if (unpinned.length) console.log(`::warning::layout specs measuring text without a pinned longest value (report-only): ${unpinned.join(', ')}`);
});
