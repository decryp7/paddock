// Every local image a Markdown file under apps/pitcrew shows exists: `![alt](path)` and `<img src="path">`, PNG or SVG (the
// screenshots and the diagrams). A renamed or dropped image must not leave a broken link. Remote images are not checked.
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

/** The local image paths a Markdown text names. */
export function imageRefs(text) {
  const refs = [...text.matchAll(/!\[[^\]]*\]\(\s*<?([^)\s>]+)/g)].map((m) => m[1]);
  refs.push(...[...text.matchAll(/<img\b[^>]*?\bsrc\s*=\s*["']([^"']+)["']/gi)].map((m) => m[1]));
  return refs.filter((r) => !/^([a-z][a-z0-9+.-]*:|\/\/|#|data:)/i.test(r));
}

test('image references are read from Markdown and from img tags', () => {
  const text = '![a](docs/a.png) ![b](<diagrams/b.svg> "t") <img alt="c" src="docs/c.png" width="10"> ![d](https://x.example/d.png)';
  assert.deepEqual(imageRefs(text), ['docs/a.png', 'diagrams/b.svg', 'docs/c.png']);
});

test('every local image a Markdown file shows exists', () => {
  const missing = [];
  let seen = 0;
  for (const file of markdownFiles(root)) {
    for (const ref of imageRefs(readFileSync(file, 'utf8'))) {
      seen++;
      const path = ref.split(/[?#]/)[0];
      if (!existsSync(resolve(dirname(file), decodeURIComponent(path)))) missing.push(`${file.slice(root.length + 1)}: ${ref}`);
    }
  }
  assert.ok(seen > 0, 'the docs show images (the check reads them)');
  assert.deepEqual(missing, []);
});
