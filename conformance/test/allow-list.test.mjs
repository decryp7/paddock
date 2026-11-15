import assert from 'node:assert/strict';
import { test } from 'node:test';
import { chmodSync } from 'node:fs';
import { join } from 'node:path';
import { checks } from '../src/checks/index.mjs';
import { findingsOf, makeApp } from './helpers.mjs';
import { AllowList, parseAllowList, readAllowList } from '../src/util/allow-list.mjs';

const FILE = 'conformance/cX-allow.json';
const entry = (extra = {}) => ({ path: 'a/b.rs', site: 'needle', reason: 'a reason long enough', ...extra });
const SPEC = { id: 'CX', rule: 21, file: FILE, unseen: 'a site' };
/** A run's view of a tree of `files` ({ path: text }). */
const ctxOf = (files) => ({ read: (p) => (Object.hasOwn(files, p) ? files[p] : null), files: () => Object.keys(files) });
const list = (...entries) => new AllowList(SPEC, parseAllowList(JSON.stringify({ allow: entries }), FILE), ctxOf({ 'a/b.rs': 'needle' }));
/** The warnings' messages; `looked`: the run read `a/b.rs` (else it never looked at it). */
const messages = (l, looked = true) => {
  if (looked) l.read('a/b.rs');
  return l.warnings().map((f) => f.message);
};

test('allow list: an entry needs a path, a site and a reason; count is a whole number of at least 1', () => {
  assert.deepEqual(parseAllowList(JSON.stringify({ allow: [entry()] }), FILE), [{ ...entry(), count: 1 }]);
  assert.equal(parseAllowList(JSON.stringify({ allow: [entry({ count: 3 })] }), FILE)[0].count, 3);
  for (const bad of [{ site: '' }, { path: '' }, { reason: 'short' }]) {
    assert.throws(() => parseAllowList(JSON.stringify({ allow: [entry(bad)] }), FILE), /path, a site .* and a reason/);
  }
  for (const count of [0, -1, 1.5, '2']) assert.throws(() => parseAllowList(JSON.stringify({ allow: [entry({ count })] }), FILE), /count/);
  assert.throws(() => parseAllowList('{}', FILE), /no `allow` array/);
  assert.throws(() => parseAllowList('nope', FILE), /not JSON/);
});

test('allow list: a site is allowed only in its own file and only when the entry names it', () => {
  const l = list(entry());
  const holds = (line) => (site) => line.includes(site);
  assert.equal(l.admit('a/c.rs', holds('let x = needle;')), 'unlisted');
  assert.equal(l.admit('a/b.rs', holds('let x = other;')), 'unlisted');
  assert.equal(l.admit('a/b.rs', holds('let x = needle;')), 'allowed');
  assert.deepEqual(messages(l), []);
});

test('allow list: admit counts up to the count and calls the rest beyond; fewer than the count is stale (PHPStan count)', () => {
  const l = list(entry({ count: 2 }));
  const exact = (site) => site === 'needle';
  assert.deepEqual([l.admit('a/b.rs', exact), l.admit('a/b.rs', exact), l.admit('a/b.rs', exact)], ['allowed', 'allowed', 'beyond']);
  assert.deepEqual(messages(l), [], "a site beyond the count is the caller's finding at the site, not a warning on the entry");
  assert.deepEqual(messages(list(entry())), ['stale allow entry (a/b.rs: needle) matches no site']);
  const short = list(entry({ count: 3 }));
  short.admit('a/b.rs', exact);
  assert.deepEqual(messages(short), ['stale allow entry (a/b.rs: needle) matches only 1 of its 3 sites']);
});

test('allow list: an entry for a file the run did not look at is neither stale nor counted', () => {
  assert.deepEqual(messages(list(entry()), false), []);
});

test('allow list: a whole float count is its integer, as the Rust host reads it', () => {
  assert.equal(parseAllowList('{"allow": [{"path": "a", "site": "b", "reason": "a reason long enough", "count": 1.0}]}', 'f').at(0).count, 1);
  assert.throws(() => parseAllowList('{"allow": [{"path": "a", "site": "b", "reason": "a reason long enough", "count": 1.5}]}', 'f'), /count/);
});

// The error paths of every check that reads an allow list (ER-21 class "the two hosts diverge on an error path": C125 in 865498235,
// then C121 and C124). The reader owns them, so they are the same for each check; the Rust host's twin is
// tools/paddock-dev/tests/it/check_allow_list.rs, which also compares the two hosts.
/**
 * A file each check reads, with one site in it. A check that names an allow list and has no sample here fails the test. The sites are
 * built from parts so that C121 and C125 do not flag this file itself.
 */
const SAMPLE = {
  C121: ['docs/g.toml', `check = ["sh", "-c", "sed -n p .gitea/${'workflows'}/ci.yml"]\n`],
  C124: ['tools/paddock-dev/src/x.rs', 'pub enum V {\n    NotMeasured,\n}\n'],
  C125: ['scripts/a.sh', `x=/${'Volumes'}/d7sst7\n`],
};
const users = checks.filter((c) => c.allowList);

test('allow list: the reader reads and parses the file once, and a file it could not read is not a stale entry\'s file', () => {
  const ctx = ctxOf({ [FILE]: JSON.stringify({ allow: [entry(), entry({ path: 'a/gone.rs' })] }), 'a/b.rs': 'x' });
  const { list: l, broken } = readAllowList(ctx, SPEC);
  assert.equal(broken, undefined);
  assert.deepEqual(l.read('a/b.rs'), { text: 'x' });
  assert.equal(l.probes, 1);
  const unread = new AllowList(SPEC, [{ ...entry(), count: 1 }], ctxOf({}));
  const { finding } = unread.read('a/b.rs');
  assert.deepEqual([finding.file, finding.severity, finding.outcome, finding.unwaivable], ['a/b.rs', 'error', 'not-measured', true]);
  assert.match(finding.message, /not measured: a\/b\.rs could not be read, so a site in it cannot be seen/);
  assert.deepEqual(unread.warnings(), [], 'an entry for a file that could not be read is not stale: its sites are unknown');
  assert.deepEqual(l.warnings().map((f) => f.message), ['stale allow entry (a/b.rs: needle) matches no site', 'stale allow entry (a/gone.rs: needle) matches no site']);
});

test('allow list: every check that reads one has a sample here, and C121, C124 and C125 read one', () => {
  assert.deepEqual(users.map((c) => c.id).filter((id) => !SAMPLE[id]), []);
  for (const id of ['C121', 'C124', 'C125']) assert.ok(users.some((c) => c.id === id), id);
});

test('allow list: a broken allow file is one unwaivable check-broken finding and nothing else, for every check that reads one', async () => {
  for (const c of users) {
    const [file, text] = SAMPLE[c.id];
    for (const allow of [null, 'nope', '{}', '{"allow": [{"path": "a"}], "history": []}']) {
      const found = await findingsOf(c.id, makeApp({ [file]: text, ...(allow === null ? {} : { [c.allowList]: allow }) }));
      assert.deepEqual(found.map((f) => [f.file, f.unwaivable, f.outcome]), [[c.allowList, true, 'check-broken']], `${c.id} with ${allow}`);
    }
  }
});

test('allow list: a file the check cannot read is a not-measured error at that file, for every check that reads one', { skip: process.getuid?.() === 0 && 'root reads a 000 file' }, async () => {
  for (const c of users) {
    const [file, text] = SAMPLE[c.id];
    const root = makeApp({ [file]: text, [c.allowList]: '{"allow": [], "history": []}' });
    chmodSync(join(root, file), 0o000);
    try {
      const found = await findingsOf(c.id, root);
      assert.deepEqual(found.map((f) => [f.file, f.severity, f.outcome]), [[file, 'error', 'not-measured']], c.id);
    } finally {
      chmodSync(join(root, file), 0o644);
    }
  }
});
