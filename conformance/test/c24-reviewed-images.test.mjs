import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { test } from 'node:test';
import { parseReviewed } from '../src/checks/C24-reviewed-images.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const sha = (b) => createHash('sha256').update(b).digest('hex');
const A = Buffer.from('image a');
const B = Buffer.from('image b');
const row = (name, buf, who = 'Ada', date = '2026-10-25') => `| ${name} | ${sha(buf)} | ${who} | ${date} |`;
const reviewed = (...rows) => `# Reviewed\n\n| Image | SHA-256 | Reviewer | Date |\n|---|---|---|---|\n${rows.join('\n')}\n`;
const good = () => ({
  'docs/screenshots/dark/a.png': A,
  'docs/screenshots/light/b.png': B,
  'docs/screenshots/hashes.json': JSON.stringify({ 'dark/a.png': sha(A), 'light/b.png': sha(B) }),
  'docs/screenshots/REVIEWED.md': reviewed(row('dark/a.png', A), row('light/b.png', B)),
});
const run = (files) => findingsOf('C24', makeApp(files));

test('C24 good: hashes and reviews match; no images is fine', async () => {
  assert.deepEqual(await run(good()), []);
  assert.deepEqual(await run({ 'README.md': 'x' }), []);
});

test('C24 bad: a changed image fails the hash and the review', async () => {
  const f = await run({ ...good(), 'docs/screenshots/dark/a.png': Buffer.from('regenerated') });
  assert.match(messages(f), /differs from its hashes\.json entry/);
  assert.match(messages(f), /dark\/a\.png changed since it was reviewed/);
});

test('C24 bad: missing manifest entry, missing review, missing files', async () => {
  const noEntry = good();
  noEntry['docs/screenshots/hashes.json'] = JSON.stringify({ 'dark/a.png': sha(A) });
  assert.match(messages(await run(noEntry)), /light\/b\.png.*|has no entry in hashes\.json/);
  const noReview = good();
  noReview['docs/screenshots/REVIEWED.md'] = reviewed(row('dark/a.png', A));
  const f = await run(noReview);
  assert.equal(f.length, 1);
  assert.equal(f[0].file, 'docs/screenshots/light/b.png');
  const files = good();
  delete files['docs/screenshots/REVIEWED.md'];
  assert.match(messages(await run(files)), /no review record/);
  const noHashes = good();
  delete noHashes['docs/screenshots/hashes.json'];
  assert.match(messages(await run(noHashes)), /no recorded hashes/);
});

test('C24 bad: a row without reviewer or date, a stale row, a stale manifest entry, a damaged manifest', async () => {
  const f1 = await run({ ...good(), 'docs/screenshots/REVIEWED.md': reviewed(row('dark/a.png', A, '', '2026-10-25'), row('light/b.png', B, 'Ada', 'yesterday')) });
  assert.equal(f1.filter((f) => /needs a reviewer and an ISO date/.test(f.message)).length, 2);
  const f2 = await run({ ...good(), 'docs/screenshots/REVIEWED.md': reviewed(row('dark/a.png', A), row('light/b.png', B), row('gone.png', A)) });
  assert.match(messages(f2), /reviews gone\.png, which does not exist/);
  const f3 = await run({ ...good(), 'docs/screenshots/hashes.json': JSON.stringify({ 'dark/a.png': sha(A), 'light/b.png': sha(B), 'old.png': 'x' }) });
  assert.match(messages(f3), /lists old\.png, which does not exist/);
  assert.match(messages(await run({ ...good(), 'docs/screenshots/hashes.json': '{ nope' })), /cannot be read/);
  assert.match(messages(await run({ ...good(), 'docs/screenshots/hashes.json': '[]' })), /cannot be read/);
});

test('C24 diagrams under docs/images need only the review row', async () => {
  const svg = Buffer.from('<svg/>');
  assert.deepEqual(await run({ 'docs/images/d.svg': svg, 'docs/images/REVIEWED.md': reviewed(row('d.svg', svg)) }), []);
  assert.match(messages(await run({ 'docs/images/d.svg': svg })), /no review record/);
});

test('parseReviewed reads rows, ignores headers and malformed rows, lower-cases hashes, last row wins', () => {
  const rows = parseReviewed(`| Image | SHA-256 |\n|---|---|\n| a.png | ${'A'.repeat(64)} | Ada | 2026-10-25 |\n| a.png | ${'b'.repeat(64)} | Bo | 2026-10-26 |\n| x | short | y | z |\n`);
  assert.equal(rows.size, 1);
  assert.equal(rows.get('a.png').reviewer, 'Bo');
  assert.equal(parseReviewed(null).size, 0);
  assert.equal(parseReviewed(`| \`a.png\` | ${'a'.repeat(64)} | Ada | 2026-10-25 |`).get('a.png').hash, 'a'.repeat(64));
});

test('C24 checks the folders named in screenshotDirs (an app whose images live under ui/)', async () => {
  const files = { 'ui/docs/screenshots/dark/a.png': A, 'ui/docs/screenshots/hashes.json': JSON.stringify({ 'dark/a.png': sha(A) }), 'ui/docs/screenshots/REVIEWED.md': reviewed(), 'paddock.conformance.json': JSON.stringify({ screenshotDirs: ['ui/docs/screenshots'] }) };
  assert.match(messages(await run(files)), /dark\/a\.png|has no review in ui\/docs\/screenshots\/REVIEWED\.md/);
  assert.deepEqual(await run({ ...files, 'ui/docs/screenshots/REVIEWED.md': reviewed(row('dark/a.png', A)) }), []);
  // Without the setting the folder is not looked at.
  assert.deepEqual(await run({ ...files, 'paddock.conformance.json': '{}' }), []);
});
