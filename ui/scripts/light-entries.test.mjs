// `shell/contributions` (and `shell/generated`, which it uses) stay light: an app that only declares contributions must not pull
// the `@decryp7/paddock-ui` root or Angular Material into its first paint (Debrief M1: +19.8 kB of paddock-ui and +41.9 kB of Material).
// A static import graph over the entry's sources, followed through the other paddock-ui entries it imports.
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const lib = join(dirname(fileURLToPath(import.meta.url)), '../projects/paddock-ui');
const PREFIX = '@decryp7/paddock-ui';
const specifiers = (file) =>
  [...readFileSync(file, 'utf8').replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '').matchAll(/(?:from|import)\s*\(?\s*'([^']+)'/g)].map((m) => m[1]);

/** Every non-spec source reachable from an entry's `public-api.ts`: relative files and other paddock-ui entries. Returns `{ files, external }`. */
export function reach(entry) {
  const files = new Set();
  const external = new Set();
  const visit = (file) => {
    if (files.has(file)) return;
    files.add(file);
    for (const spec of specifiers(file)) {
      if (spec.startsWith('.')) {
        const base = resolve(dirname(file), spec);
        const next = [`${base}.ts`, join(base, 'index.ts')].find(existsSync);
        if (next === undefined) throw new Error(`${file}: cannot resolve ${spec}`);
        visit(next);
      } else if (spec === PREFIX) {
        external.add(spec);
      } else if (spec.startsWith(`${PREFIX}/`)) {
        const sub = spec.slice(PREFIX.length + 1);
        const api = join(lib, sub, 'public-api.ts');
        if (!existsSync(api)) throw new Error(`${file}: no entry ${spec}`);
        visit(api);
      } else {
        external.add(spec);
      }
    }
  };
  visit(join(lib, entry, 'public-api.ts'));
  return { files: [...files], external: [...external] };
}

for (const entry of ['shell/contributions', 'shell/generated']) {
  test(`${entry} reaches neither the paddock-ui root nor Angular Material`, () => {
    const { external } = reach(entry);
    const bad = external.filter((s) => s === PREFIX || s.startsWith('@angular/material') || s.startsWith('@angular/cdk'));
    assert.deepEqual(bad, [], `${entry} imports ${bad.join(', ')}`);
  });
}

test('the probe sees the root: the full shell entry reaches it', () => {
  const { external } = reach('shell');
  assert.ok(external.includes(PREFIX), 'the import-graph probe cannot see the root import of the shell entry (check broken)');
});

test('shell/contributions reaches no other shell part', () => {
  const { files } = reach('shell/contributions');
  const other = files.filter((f) => /\/shell\/(src|frame|pages|menu)\//.test(f));
  assert.deepEqual(other, []);
});
