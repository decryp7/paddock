// Entries that must stay light: an app that imports only them must not pull the `@decryp7/paddock-ui` root, Angular Material or a heavy
// sibling into its first paint. Each row declares one entry and what it must not reach, by a static import graph over its sources,
// followed through the other paddock-ui entries it imports.
//   - `shell/contributions` (+ `shell/generated`): Debrief M1 paid +19.8 kB of paddock-ui and +41.9 kB of Material for the registry alone.
//   - `locale` (loads in plain Node: no Angular, no ngrx, no shell) and `shell/i18n` (no root, Material or page).
//   - `shell/service` and `shell/storage` carry components of their own, so only their `/page` entry (the Settings page, split out of the
//     entry) is forbidden to them: the page entry may use the root and Material, the entry that others import may not reach it.
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const lib = join(dirname(fileURLToPath(import.meta.url)), '../projects/paddock-ui');
const PREFIX = '@decryp7/paddock-ui';
const isMaterial = (s) => /^@angular\/(material|cdk)(\/|$)/.test(s);
const ROOT = (s) => s === PREFIX;

/** The declared light entries: `forbid` are predicates over an import specifier. */
export const LIGHT_ENTRIES = {
  'shell/contributions': { forbid: [ROOT, isMaterial, (s) => s === `${PREFIX}/shell`] },
  'shell/generated': { forbid: [ROOT, isMaterial, (s) => s === `${PREFIX}/shell`] },
  locale: { forbid: [ROOT, isMaterial, (s) => s.startsWith('@angular/') || s.startsWith('@ngrx') || s.includes('/shell')] },
  'shell/i18n': { forbid: [ROOT, isMaterial, (s) => s === `${PREFIX}/shell/i18n/page`] },
  'shell/service': { forbid: [(s) => s === `${PREFIX}/shell/service/page`] },
  'shell/storage': { forbid: [(s) => s === `${PREFIX}/shell/storage/page`] },
};

/** Import and export specifiers of a source: ', " and ` quotes, static, dynamic `import()` and `export ... from`; comments are ignored. */
export function specifiers(text) {
  const code = text.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '');
  return [...code.matchAll(/(?:\bfrom|\bimport)\s*\(?\s*(['"`])([^'"`\n]+)\1/g)].map((m) => m[2]);
}

/** Every source reachable from an entry's `public-api.ts`: relative files and other paddock-ui entries. Returns `{ files, external }`. */
export function reach(entry) {
  const files = new Set();
  const external = new Set();
  const visit = (file) => {
    if (files.has(file)) return;
    files.add(file);
    for (const spec of specifiers(readFileSync(file, 'utf8'))) {
      if (spec.startsWith('.')) {
        const base = resolve(dirname(file), spec);
        const next = [`${base}.ts`, join(base, 'index.ts')].find(existsSync);
        if (next === undefined) throw new Error(`${file}: cannot resolve ${spec}`);
        visit(next);
      } else if (spec.startsWith(`${PREFIX}/`)) {
        external.add(spec);
        const api = join(lib, spec.slice(PREFIX.length + 1), 'public-api.ts');
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

for (const [entry, { forbid }] of Object.entries(LIGHT_ENTRIES)) {
  test(`${entry} stays light`, () => {
    const bad = reach(entry).external.filter((s) => forbid.some((f) => f(s)));
    assert.deepEqual(bad, [], `${entry} reaches ${bad.join(', ')}`);
  });
}

test('the probe sees the root: the full shell entry reaches it', () => {
  assert.ok(reach('shell').external.includes(PREFIX), 'the import-graph probe cannot see the root import of the shell entry (check broken)');
});

test('shell/contributions reaches no other shell part', () => {
  const other = reach('shell/contributions').files.filter((f) => /\/shell\/(src|frame|pages|menu)\//.test(f));
  assert.deepEqual(other, []);
});

test('specifiers reads every quote style, dynamic imports and re-exports, and skips comments', () => {
  const fixture = [
    "import { a } from '@decryp7/paddock-ui';",
    'import { b } from "@angular/material/menu";',
    'const c = await import(`@decryp7/paddock-ui/shell`);',
    "export * from '@decryp7/paddock-ui/shell/contributions';",
    "// import { d } from 'not-this';",
    "/* import e from 'nor-this'; */",
  ].join('\n');
  assert.deepEqual(specifiers(fixture), ['@decryp7/paddock-ui', '@angular/material/menu', '@decryp7/paddock-ui/shell', '@decryp7/paddock-ui/shell/contributions']);
  assert.ok(specifiers(fixture).some(ROOT) && specifiers(fixture).some((s) => isMaterial(s)));
});
