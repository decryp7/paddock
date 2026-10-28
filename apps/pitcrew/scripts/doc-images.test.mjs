// Every screenshot a Markdown file under apps/pitcrew names exists (a renamed or dropped image must not leave a broken link).
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function markdownFiles(dir) {
  return readdirSync(dir).flatMap((name) => {
    if (name === 'node_modules' || name === 'target' || name === 'dist' || name.startsWith('.')) return [];
    const path = join(dir, name);
    return statSync(path).isDirectory() ? markdownFiles(path) : name.endsWith('.md') ? [path] : [];
  });
}

test('every screenshots/*.png a Markdown file names exists', () => {
  const missing = [];
  let seen = 0;
  for (const file of markdownFiles(root)) {
    for (const m of readFileSync(file, 'utf8').matchAll(/\]\(([^)\s]*screenshots\/[^)\s]+\.png)\)/g)) {
      seen++;
      if (!existsSync(resolve(dirname(file), m[1]))) missing.push(`${file.slice(root.length + 1)}: ${m[1]}`);
    }
  }
  assert.ok(seen > 0, 'the docs name screenshots (the check reads them)');
  assert.deepEqual(missing, []);
});
