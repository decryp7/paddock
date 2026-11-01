import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { after, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { COPY_WORDS, lintWiki, linksOf, main, parsePage, readRulesSource, slug, windows } from './wiki-lint.mjs';

const roots = [];
after(() => roots.forEach((r) => rmSync(r, { recursive: true, force: true })));

const RULES = 'The coordinator watches load and free disk continuously and keeps the machine busy but not overloaded: start more agents.\n';
const page = (verified = '2026-10-20', sources = '[a.md]', body = '## Current state\n- A claim with a source [x](two.md).\n') =>
  `---\ntitle: T\nverified: ${verified}\nsources: ${sources}\n---\nSummary.\n\n${body}`;

/** A wiki in a temp folder: two linked pages, an index and a log; `over` replaces or adds files (null removes). */
function wiki(over = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'wiki-lint-'));
  roots.push(dir);
  const files = {
    'one.md': page('2026-10-20', '[a.md]', '## Current state\n- One [two](two.md).\n'),
    'two.md': page('2026-10-20', '[a.md]', '## Current state\n- Two [one](one.md).\n'),
    'index.md': '# Index\n- [One](one.md)\n- [Two](two.md)\n',
    'log.md': '# Log\n## [2026-10-31] ingest | one | seed\n',
    'README.md': '# Wiki\n',
    ...over,
  };
  for (const [name, text] of Object.entries(files)) if (text !== null) writeFileSync(join(dir, name), text);
  const rules = join(dir, 'rules.txt');
  writeFileSync(rules, RULES);
  return { dir, rules };
}

const lint = (over, extra = {}) => {
  const { dir, rules } = wiki(over);
  return lintWiki({ wikiDir: dir, rules: [{ name: 'rules.txt', text: readFileSync(rules, 'utf8') }], today: '2026-10-31', ...extra });
};
const has = (list, text) => list.some((m) => m.includes(text));

test('a sound wiki has no errors and no warnings', () => {
  assert.deepEqual(lint({}), { errors: [], warnings: [] });
});

test('a broken link and a missing heading anchor are errors', () => {
  const r = lint({ 'one.md': page('2026-10-20', '[a.md]', '- Gone [x](missing.md) and [y](two.md#nope), ok [z](two.md#current-state).\n') });
  assert.ok(has(r.errors, 'missing.md does not exist'));
  assert.ok(has(r.errors, 'two.md has no heading #nope'));
});

test('links inside code are not links; absolute URLs and pure anchors are not checked', () => {
  assert.deepEqual(linksOf('See `[x](gone.md)` and [a](https://e.org/x) and [b](#top) and [c](d.md#h).\n```\n[f](g.md)\n```'), [
    { target: 'd.md', anchor: 'h', line: 1 },
  ]);
});

test('a page missing from the index is an error', () => {
  const r = lint({ 'index.md': '# Index\n- [One](one.md)\n' });
  assert.ok(has(r.errors, 'two.md: not listed in index.md'));
});

test('an orphan (indexed, but no other page links to it) is an error', () => {
  const r = lint({ 'two.md': page('2026-10-20', '[a.md]', '- Two alone [x](one.md).\n'), 'one.md': page('2026-10-20', '[a.md]', '- One alone [x](two.md).\n'), 'three.md': page('2026-10-20', '[a.md]', '- Three [x](one.md).\n'), 'index.md': '- [One](one.md)\n- [Two](two.md)\n- [Three](three.md)\n' });
  assert.ok(has(r.errors, 'three.md: orphan'));
  assert.ok(!has(r.errors, 'one.md: orphan'));
});

test('front matter needs a real verified date and a non-empty sources list', () => {
  assert.ok(has(lint({ 'one.md': '---\nsources: [a]\n---\n- x [y](two.md)\n' }).errors, 'verified: YYYY-MM-DD'));
  assert.ok(has(lint({ 'one.md': page('2026-13-45') }).errors, 'verified: YYYY-MM-DD'));
  assert.ok(has(lint({ 'one.md': page('2026-10-20', '[]') }).errors, 'non-empty `sources:`'));
  assert.ok(has(lint({ 'one.md': '- x [y](two.md)\n' }).errors, 'verified: YYYY-MM-DD'));
});

test('sources may be a block list', () => {
  const text = '---\ntitle: T\nverified: 2026-10-20\nsources:\n  - a.md\n  - b.md\n---\n- c [two](two.md)\n';
  assert.deepEqual(parsePage(text).meta.sources, ['a.md', 'b.md']);
  assert.deepEqual(lint({ 'one.md': text }).errors, []);
});

test('a page older than the limit is flagged as a warning, not an error; a future date is an error', () => {
  const old = lint({ 'one.md': page('2026-09-01') });
  assert.deepEqual(old.errors, []);
  assert.ok(has(old.warnings, 'stale: verified 2026-09-01, 60 days ago'));
  assert.equal(lint({ 'one.md': page('2026-09-30') }).warnings.length, 1); // 31 days is over the limit of 30
  assert.equal(lint({ 'one.md': page('2026-10-01') }).warnings.length, 0); // exactly 30 days is inside
  assert.ok(has(lint({ 'one.md': page('2026-11-30') }).errors, 'in the future'));
});

test('a bullet without a source is an error; a continuation line may carry the source; src: counts', () => {
  assert.ok(has(lint({ 'one.md': page('2026-10-20', '[a]', '- Bare claim.\n- Linked [two](two.md).\n') }).errors, 'one.md:8: bullet names no source'));
  assert.deepEqual(lint({ 'one.md': page('2026-10-20', '[a]', '- Wrapped claim\n  continues here [two](two.md).\n- By id, src: `run 7158`.\n') }).errors, []);
  assert.deepEqual(lint({ 'one.md': page('2026-10-20', '[a]', '```\n- not a claim\n```\n- Claim [two](two.md).\n') }).errors, []);
});

test('the log lines must keep their shape', () => {
  assert.ok(has(lint({ 'log.md': '## 2026-10-31 something\n' }).errors, 'log line must be'));
  assert.ok(has(lint({ 'log.md': '## [2026-10-31] edit | one | x\n' }).errors, 'log line must be'));
});

test('copying a run of rule words fails; a paraphrase passes; the run length is the rule', () => {
  const copy = 'The coordinator watches load and free disk continuously and keeps the machine busy.';
  const r = lint({ 'one.md': page('2026-10-20', '[a]', `- ${copy} [two](two.md)\n`) });
  assert.ok(has(r.errors, 'one.md: copies rule text'));
  assert.deepEqual(lint({ 'one.md': page('2026-10-20', '[a]', '- Load is watched, disk too; more agents start when it is low [two](two.md)\n') }).errors, []);
  // a copy wrapped over two lines is still found
  const wrapped = lint({ 'one.md': page('2026-10-20', '[a]', '- The coordinator watches load and free disk\n  continuously and keeps the machine busy [two](two.md)\n') });
  assert.ok(has(wrapped.errors, 'copies rule text'));
  assert.equal(COPY_WORDS, 8);
  assert.equal(windows('a b c d e f g h i').size, 2);
  assert.equal(windows('a b c d e f g').size, 0);
});

test('index.md and log.md are required', () => {
  assert.ok(has(lint({ 'index.md': null }).errors, 'missing index.md'));
  assert.ok(has(lint({ 'log.md': null }).errors, 'missing log.md'));
});

test('slugs follow the forge: lower case, no punctuation, hyphens', () => {
  assert.equal(slug('CI economy (user, 2026-10-29)'), 'ci-economy-user-2026-10-29');
  assert.equal(slug('A `pid` is not a process'), 'a-pid-is-not-a-process');
});

test('the CLI returns 1 on errors and on warnings only with --strict', () => {
  const { dir, rules } = wiki({ 'one.md': page('2026-09-01') });
  const quiet = { out: () => {}, err: () => {} };
  const args = ['--wiki', dir, '--rules', rules, '--er', join(dir, 'none.md'), '--today', '2026-10-31'];
  assert.equal(main(args, quiet), 0);
  assert.equal(main([...args, '--strict'], quiet), 1);
  assert.equal(main([...args, '--max-age', '90', '--strict'], quiet), 0);
  const bad = wiki({ 'index.md': '# Index\n' });
  assert.equal(main(['--wiki', bad.dir, '--rules', bad.rules, '--er', join(dir, 'none.md'), '--today', '2026-10-31'], quiet), 1);
  assert.equal(main([...args, '--rules', join(dir, 'unreadable.txt')], quiet), 1);
});

test('a copy of any one of several rules sources fails, and the message names that source', () => {
  const { dir } = wiki({ 'one.md': page('2026-10-20', '[a]', '- Branch rule says never wait on a fixed number of event loop turns ever [two](two.md)\n') });
  const other = { name: 'origin/docs/branch:docs/rules.md', text: 'Specs: never wait on a fixed number of event loop turns ever again.' };
  const r = lintWiki({ wikiDir: dir, rules: [{ name: 'rules.txt', text: RULES }, other], today: '2026-10-31' });
  assert.ok(has(r.errors, 'shared with origin/docs/branch:docs/rules.md'));
  assert.ok(!has(r.errors, 'shared with rules.txt'));
});

test('a steward-log citation by clock time is an error; by line number it passes', () => {
  assert.ok(has(lint({ 'one.md': page('2026-10-20', '[a]', '- Claim, src: `merge-steward.md` entry 14:33 [two](two.md)\n') }).errors, 'clock time'));
  assert.deepEqual(lint({ 'one.md': page('2026-10-20', '[a]', '- Claim, src: `merge-steward.md:560` [two](two.md)\n') }).errors, []);
  assert.deepEqual(lint({ 'one.md': page('2026-10-20', '[a]', '- The runner starts at 14:33 [two](two.md)\n') }).errors, []);
});

test('a rules source is a file, or a git ref:path (read from a temp repository), or unreadable', () => {
  const { rules, dir } = wiki({});
  assert.equal(readRulesSource(rules).text, RULES);
  assert.equal(readRulesSource('no/such/file.md'), null);
  // A throwaway repository, so the test does not depend on the checkout it runs in (a `git archive` copy has no .git).
  const repo = mkdtempSync(join(tmpdir(), 'wiki-lint-git-'));
  roots.push(repo);
  const git = (...args) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', '-c', 'commit.gpgsign=false', ...args], { cwd: repo, stdio: 'ignore' });
  git('init', '-q');
  writeFileSync(join(repo, 'rules.md'), RULES);
  git('add', 'rules.md');
  git('commit', '-q', '-m', 'rules');
  assert.equal(readRulesSource('HEAD:rules.md', repo).text, RULES);
  assert.equal(readRulesSource('no-such-ref-xyz:rules.md', repo), null);
  assert.equal(readRulesSource('HEAD:nothing.md', repo), null);
  assert.equal(readRulesSource('HEAD:rules.md', dir), null); // not a git work tree: skipped, not an error
});

test('the real docs/wiki passes against docs/rules.md and the engineering rules (stale pages only warn)', () => {
  const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..');
  const rules = [join(repo, 'docs/rules.md'), resolve(repo, '../engineering-rules/RULES.md')].flatMap((f) => {
    try { return [{ name: f, text: readFileSync(f, 'utf8') }]; } catch { return []; }
  });
  const r = lintWiki({ wikiDir: join(repo, 'docs/wiki'), rules, today: new Date().toISOString().slice(0, 10) });
  assert.deepEqual(r.errors, []);
});
