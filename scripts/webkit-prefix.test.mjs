// WKWebView (macOS) and Safari only know `-webkit-user-select`; the unprefixed property is a separate, ignored one there, so a rule that
// says only `user-select: text` cannot opt back in under the shell's `-webkit-user-select: none`. Every rule that sets one sets both.
import assert from 'node:assert/strict';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const SKIP = new Set(['node_modules', 'dist', '.angular', 'css', 'docs']);

function* sources(dir) {
  for (const name of readdirSync(dir)) {
    if (SKIP.has(name)) continue;
    const path = join(dir, name);
    if (statSync(path).isDirectory()) yield* sources(path);
    else if (/\.(ts|scss|css)$/.test(name) && !/\.(test|spec)\.ts$/.test(name)) yield path;
  }
}

/** The rules (`{ ... }` bodies) of a source that declare `user-select` without the matching `-webkit-user-select`. */
export function unprefixedRules(text) {
  const bad = [];
  for (const m of text.matchAll(/\{([^{}]*)\}/g)) {
    const body = m[1];
    for (const d of body.matchAll(/(?<![\w-])user-select\s*:\s*([\w-]+)/g)) {
      if (!new RegExp(`-webkit-user-select\\s*:\\s*${d[1]}`).test(body)) bad.push(d[0]);
    }
  }
  return bad;
}

test('the checker finds an unprefixed rule and accepts a pair', () => {
  assert.deepEqual(unprefixedRules('a { user-select: text; }'), ['user-select: text']);
  assert.deepEqual(unprefixedRules('a { -webkit-user-select: none; user-select: none; }'), []);
  assert.deepEqual(unprefixedRules('a { -webkit-user-select: none; user-select: text; }'), ['user-select: text']);
});

test('every rule that sets user-select sets -webkit-user-select too', () => {
  const found = [];
  for (const file of sources(join(root, 'ui', 'projects'))) {
    for (const rule of unprefixedRules(readFileSync(file, 'utf8'))) found.push(`${relative(root, file)}: ${rule}`);
  }
  assert.deepEqual(found, []);
});
