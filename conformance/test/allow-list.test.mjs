import assert from 'node:assert/strict';
import { test } from 'node:test';
import { AllowList, parseAllowList } from '../src/util/allow-list.mjs';

const FILE = 'conformance/cX-allow.json';
const entry = (extra = {}) => ({ path: 'a/b.rs', site: 'needle', reason: 'a reason long enough', ...extra });
const list = (...entries) => new AllowList(FILE, parseAllowList(JSON.stringify({ allow: entries }), FILE));
const messages = (l, looked = () => true) => l.warnings('CX', 21, looked).map((f) => f.message);

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

test('allow list: a site is allowed only in its own file and only when the line holds the site text', () => {
  const l = list(entry());
  assert.equal(l.allows('a/b.rs', 'let x = needle;'), true);
  assert.equal(l.allows('a/c.rs', 'let x = needle;'), false);
  assert.equal(l.allows('a/b.rs', 'let x = other;'), false);
  assert.deepEqual(messages(l), []);
});

test('allow list: no match is stale, a match beyond the count is ambiguous, fewer than the count is stale (PHPStan count)', () => {
  assert.deepEqual(messages(list(entry())), ['stale allow entry (a/b.rs: needle) matches no site']);
  const two = list(entry());
  two.allows('a/b.rs', 'needle 1');
  two.allows('a/b.rs', 'needle 2');
  assert.deepEqual(messages(two), ['ambiguous allow entry (a/b.rs: needle) matches 2 sites']);
  const counted = list(entry({ count: 2 }));
  counted.allows('a/b.rs', 'needle 1');
  counted.allows('a/b.rs', 'needle 2');
  assert.deepEqual(messages(counted), []);
  const short = list(entry({ count: 3 }));
  short.allows('a/b.rs', 'needle 1');
  assert.deepEqual(messages(short), ['stale allow entry (a/b.rs: needle) matches only 1 of its 3 sites']);
});

test('allow list: an entry for a file the run did not look at is neither stale nor counted', () => {
  assert.deepEqual(messages(list(entry()), () => false), []);
  // Ambiguity is a fact of the sites seen, whatever else was read.
  const l = list(entry());
  l.allows('a/b.rs', 'needle');
  l.allows('a/b.rs', 'needle');
  assert.equal(messages(l, () => false).length, 1);
});

test('allow list: admit counts up to the count and calls the rest beyond, never ambiguous', () => {
  const l = list(entry({ count: 2 }));
  const exact = (site) => site === 'needle';
  assert.deepEqual([l.admit('a/b.rs', exact), l.admit('a/b.rs', exact), l.admit('a/b.rs', exact)], ['allowed', 'allowed', 'beyond']);
  assert.equal(l.admit('a/c.rs', exact), 'unlisted');
  assert.equal(l.admit('a/b.rs', () => false), 'unlisted');
  assert.deepEqual(messages(l), [], "a site beyond the count is the caller's finding, not an ambiguous entry");
});

test('allow list: a whole float count is its integer, as the Rust host reads it', () => {
  assert.equal(parseAllowList('{"allow": [{"path": "a", "site": "b", "reason": "a reason long enough", "count": 1.0}]}', 'f').at(0).count, 1);
  assert.throws(() => parseAllowList('{"allow": [{"path": "a", "site": "b", "reason": "a reason long enough", "count": 1.5}]}', 'f'), /count/);
});
