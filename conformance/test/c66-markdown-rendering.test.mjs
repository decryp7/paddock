import assert from 'node:assert/strict';
import { test } from 'node:test';
import { lintMarkdown } from '../src/util/markdown-lint.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const lint = (text, files = []) => lintMarkdown(text, { file: 'docs/a.md', exists: (p) => files.includes(p) }).map((p) => [p.kind, p.line]);

test('a blank line that splits a table is found, two separate tables are not', () => {
  const split = '| A | B |\n|---|---|\n| 1 | 2 |\n\n| 3 | 4 |\n';
  assert.deepEqual(lint(split), [['table', 4]]);
  const two = '| A | B |\n|---|---|\n| 1 | 2 |\n\n| C | D |\n|---|---|\n| 3 | 4 |\n';
  assert.deepEqual(lint(two), []);
});

test('table cell counts, delimiter row, pipe-less lines and escaped pipes', () => {
  assert.deepEqual(lint('| A | B |\n|---|---|\n| 1 |\n'), [['table', 3]]);
  assert.deepEqual(lint('| A | B |\n|---|---|\n| `x|y` | 2 |\n'), [['table', 3]]);
  assert.deepEqual(lint('| A | B |\n|---|---|\n| `x\\|y` | 2 |\n'), []);
  assert.deepEqual(lint('| A | B |\n|---|\n| 1 | 2 |\n'), [['table', 2]]);
  assert.deepEqual(lint('| A | B |\n|---|---|\n| 1 | 2 |\nloose text\n'), [['table', 4]]);
  assert.deepEqual(lint('| A | B |\n|---|---|\n| 1 | 2 |\n- item\n'), []);
});

test('a table glued to a paragraph is found; after a heading or blank line it is fine', () => {
  assert.deepEqual(lint('Some text:\n| A | B |\n|---|---|\n| 1 | 2 |\n'), [['table', 2]]);
  assert.deepEqual(lint('## H\n| A | B |\n|---|---|\n| 1 | 2 |\n'), []);
  assert.deepEqual(lint('Text\n\n| A | B |\n|---|---|\n| 1 | 2 |\n'), []);
});

test('code fences: unclosed, shorter closing fence, content that looks like a table', () => {
  assert.deepEqual(lint('```sh\nls\n'), [['fence', 1]]);
  assert.deepEqual(lint('````md\n```\nx\n```\n'), [['fence', 1]]);
  assert.deepEqual(lint('```\n| A | B |\n|---|---|\n| 1 |\n```\n'), []);
  assert.deepEqual(lint('~~~\nx\n~~~\n'), []);
});

test('links: relative targets, folders, anchors, externals and code spans', () => {
  const md = '[a](b.md) [b](missing.md) [c](sub/) [d](https://x.invalid/y) `[e](nope.md)` ![i](img/p.png#x)\n';
  assert.deepEqual(lint(md, ['docs/b.md', 'docs/sub', 'docs/img/p.png']), [['link', 1]]);
  assert.deepEqual(lint('# Hello, World\n\n[x](#hello-world) [y](#gone)\n'), [['anchor', 3]]);
});

test('list continuation that lost its indent is found, a lazy or deeper one is not', () => {
  assert.deepEqual(lint('1. step\n\n  more text\n'), [['list', 3]]);
  assert.deepEqual(lint('1. step\n\n   more text\n'), []);
  assert.deepEqual(lint('- item\n\nnext paragraph\n'), []);
});

test('HTML: unclosed and stray tags, void tags, comments and code are ignored', () => {
  assert.deepEqual(lint('<details>\n<summary>S</summary>\nbody\n'), [['html', 1]]);
  assert.deepEqual(lint('x </div>\n'), [['html', 1]]);
  assert.deepEqual(lint('a<br>b <!-- <div> --> `<span>` <img src="x"> <Vec<T>>\n'), []);
  assert.deepEqual(lint('<details>\n<summary>S</summary>\nbody\n</details>\n'), []);
});

test('a percent sign in an anchor does not throw', () => {
  assert.deepEqual(lint('# Top\n\n[x](#100%) [y](a%zz.md)\n'), [['anchor', 3], ['link', 3]]);
});

test('heading slugs: underscores in code spans stay, emphasis goes, setext headings count', () => {
  assert.deepEqual(lint('## `paddock_log` setup\n\n[a](#paddock_log-setup)\n'), []);
  assert.deepEqual(lint('## _Fast_ and *loud*\n\n[a](#fast-and-loud)\n'), []);
  assert.deepEqual(lint('## snake_case_word\n\n[a](#snake_case_word)\n'), []);
  assert.deepEqual(lint('Title here\n==========\n\nSub head\n--------\n\n[a](#title-here) [b](#sub-head)\n'), []);
  assert.deepEqual(lint('- item\n---\n\n[a](#item)\n'), [['anchor', 4]]);
});

test('a fence indented four spaces is indented code, not a fence', () => {
  assert.deepEqual(lint('Text\n\n    ```sh\n    ls\n'), []);
  assert.deepEqual(lint('Text\n\n   ```sh\n   ls\n'), [['fence', 3]]);
});

test('HTML check ignores link destinations in angle brackets', () => {
  assert.deepEqual(lint('[x](<b c.md>)\n', ['docs/b c.md']), []);
});

test('a very long line is scanned in bounded time', () => {
  const t = Date.now();
  lint(`${'['.repeat(200000)}\n`);
  assert.ok(Date.now() - t < 2000);
});

test('C66 through the runner: the crates README split is an error, THIRD_PARTY_NOTICES.md is skipped', async () => {
  const bad = '| A | B |\n|---|---|\n| 1 | 2 |\n\n| 3 | 4 |\n';
  const root = makeApp({ 'crates/README.md': bad, 'x/THIRD_PARTY_NOTICES.md': bad, 'ok.md': '# Fine\n\n| A |\n|---|\n| 1 |\n' });
  const f = await findingsOf('C66', root);
  assert.deepEqual(f.map((x) => [x.file, x.line, x.severity]), [['crates/README.md', 4, 'error']]);
  assert.match(f[0].message, /blank line splits the table/);
});
