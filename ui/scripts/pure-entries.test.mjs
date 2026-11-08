// `format` and `locale` stay loadable in plain Node (Playwright specs import app source that imports them): they import
// nothing from Angular or from the shell entries, which pull the language store and need the JIT compiler.
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const lib = join(dirname(fileURLToPath(import.meta.url)), '../projects/paddock-ui');
const sources = (dir) =>
  readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? sources(join(dir, e.name)) : e.name.endsWith('.ts') && !e.name.endsWith('.spec.ts') ? [join(dir, e.name)] : [],
  );

for (const entry of ['format', 'locale']) {
  test(`${entry} imports no Angular and no shell entry`, () => {
    for (const file of sources(join(lib, entry))) {
      const bad = [...readFileSync(file, 'utf8').matchAll(/(?:from|import)\s+'([^']+)'/g)].map((m) => m[1]).filter((s) => s.startsWith('@angular') || s.includes('/shell') || s.startsWith('@ngrx'));
      assert.deepEqual(bad, [], `${file} imports ${bad.join(', ')}`);
    }
  });
}
