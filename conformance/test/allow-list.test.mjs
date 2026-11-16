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
/** The warnings' messages after a run that read `a/b.rs`. */
const messages = (l) => {
  l.read('a/b.rs');
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
  // `a/b.rs` exists but is not among the run's files (a run over other files): nothing is said about its entry.
  const outside = new AllowList(SPEC, parseAllowList(JSON.stringify({ allow: [entry()] }), FILE), { read: () => 'needle', files: () => [] });
  assert.deepEqual(outside.warnings(), []);
});

test('allow list: an entry for a file of the run that the check does not read names a file it does not read', () => {
  const l = new AllowList(SPEC, parseAllowList(JSON.stringify({ allow: [entry(), entry({ path: 'docs/x.md' })] }), FILE), ctxOf({ 'a/b.rs': 'needle', 'docs/x.md': 'needle' }));
  l.read('a/b.rs');
  l.admit('a/b.rs', () => true);
  assert.deepEqual(l.warnings().map((f) => [f.file, f.message]), [[FILE, 'allow entry (docs/x.md: needle) names a file this check does not read']]);
});

test('allow list: an entry path is a plain repository path, never ./, ../, absolute, doubled or with backslashes', () => {
  for (const path of ['./a/b.rs', 'a/../b.rs', '/a/b.rs', 'a//b.rs', 'a\\b.rs', 'a/./b.rs', '..']) {
    assert.throws(() => parseAllowList(JSON.stringify({ allow: [entry({ path })] }), FILE), /plain repository path/, path);
  }
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
  C130: ['tools/t/tests/it/a.rs', 'let s = "#!/bin/sh\\n";\n'],
};
const users = checks.filter((c) => c.allowList);

test('allow list: the reader reads and parses the file once, and a file it could not read is not a stale entry\'s file', () => {
  const ctx = ctxOf({ [FILE]: JSON.stringify({ allow: [entry(), entry({ path: 'a/gone.rs' })] }), 'a/b.rs': 'x' });
  const { list: l, broken } = readAllowList(ctx, SPEC);
  assert.equal(broken, undefined);
  assert.deepEqual(l.read('a/b.rs'), { text: 'x' });
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
      const found = await findingsOf(c.id, makeApp({ [file]: text, ...(allow === null ? {} : { [c.allowList.file]: allow }) }));
      assert.deepEqual(found.map((f) => [f.file, f.unwaivable, f.outcome]), [[c.allowList.file, true, 'check-broken']], `${c.id} with ${allow}`);
    }
  }
});

test('allow list: a file the check cannot read is a not-measured error at that file, for every check that reads one', { skip: process.getuid?.() === 0 && 'root reads a 000 file' }, async () => {
  for (const c of users) {
    const [file, text] = SAMPLE[c.id];
    const root = makeApp({ [file]: text, [c.allowList.file]: '{"allow": [], "history": []}' });
    chmodSync(join(root, file), 0o000);
    try {
      const found = await findingsOf(c.id, root);
      assert.deepEqual(found.map((f) => [f.file, f.severity, f.outcome]), [[file, 'error', 'not-measured']], c.id);
    } finally {
      chmodSync(join(root, file), 0o644);
    }
  }
});

// ---- the list only shrinks: every entry, counted `count` times, against the merge base (C121, C124, C125 and C130 all get it) ----
import { execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';

const allowJson = (entries) => JSON.stringify({ allow: entries });
const sh = (root, ...args) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', '-c', 'commit.gpgsign=false', ...args], { cwd: root, stdio: 'ignore' });
/** A repository whose merge base (`main`) has the allow list `base` and whose `topic` commit has `now`. */
function growthRepo(base, now) {
  const SHELL = 'let s = "#!/bin/sh\\n";\n';
  const root = makeApp({ 'conformance/c130-allow.json': allowJson(base), 'tools/t/tests/it/ok.rs': SHELL.repeat(3) });
  sh(root, 'init', '-q', '-b', 'main');
  sh(root, 'add', '.');
  sh(root, 'commit', '-q', '-m', 'base');
  sh(root, 'checkout', '-q', '-b', 'topic');
  writeFileSync(join(root, 'conformance/c130-allow.json'), allowJson(now));
  sh(root, 'commit', '-q', '-a', '--allow-empty', '-m', 'topic');
  return root;
}
const ok = (count) => ({ path: 'tools/t/tests/it/ok.rs', site: '#!/bin/sh', reason: 'baseline: planted before StandIn', count });
const grew = async (root) => (await findingsOf('C130', root, { options: { baseRef: 'main' } })).filter((f) => /allow list (grew|.s shrink-only)/.test(f.message));

test('allow list growth: an unchanged or shrunk list says nothing, a raised count or a new entry is one unwaivable finding at the allow file', async () => {
  assert.deepEqual(await grew(growthRepo([ok(3)], [ok(3)])), []);
  assert.deepEqual(await grew(growthRepo([ok(3)], [ok(2)])), [], 'a lowered count is shrinking');
  const raised = await grew(growthRepo([ok(2)], [ok(3)]));
  assert.deepEqual(raised.map((f) => [f.file, f.line, f.unwaivable]), [['conformance/c130-allow.json', 1, true]]);
  assert.match(raised[0].message, /the allow list grew: 1 entry is \(counting each `count`\) not in conformance\/c130-allow\.json at the merge base [0-9a-f]{9}; it may only shrink/);
  const added = await grew(growthRepo([ok(1)], [ok(1), { ...ok(2), site: '#!/bin/bash' }]));
  assert.match(added[0].message, /grew: 2 entries are/);
  assert.match(added[0].fix, /user's approval/);
});

test('allow list growth: a list new at the merge base, a fixture folder without .git and an unmeasurable base are not growth', async () => {
  const first = makeApp({ 'conformance/c130-allow.json': allowJson([ok(3)]), 'tools/t/tests/it/ok.rs': 'x\n' });
  sh(first, 'init', '-q', '-b', 'main');
  sh(first, 'add', '.');
  sh(first, 'commit', '-q', '-m', 'only');
  assert.deepEqual((await grew(first)).map((f) => f.message.slice(0, 40)), [], 'the first introduction adds nothing');
  assert.deepEqual(await grew(makeApp({ 'conformance/c130-allow.json': allowJson([ok(3)]) })), [], 'no .git: nothing to compare, nothing said');
  const none = growthRepo([ok(1)], [ok(1)]);
  const w = (await findingsOf('C130', none, { options: { baseRef: 'no-such-ref' } })).filter((f) => /shrink-only rule is not enforced/.test(f.message));
  assert.equal(w.length, 1, 'a checkout where git cannot give a base says so, never silence');
});

// ---- the target ref: a member branch of a train is measured against the train, not main (ER-21; report-only, ER-41) ----
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { targetRef } from '../src/util/baseline.mjs';

/** main has the list `base`; `train` (off main) has `trainList`; `member` (off train) has `memberList`. */
function trainRepo(base, trainList, memberList) {
  const root = makeApp({ 'conformance/c130-allow.json': allowJson(base), 'tools/t/tests/it/ok.rs': 'let s = "#!/bin/sh\\n";\n'.repeat(4) });
  sh(root, 'init', '-q', '-b', 'main');
  sh(root, 'add', '.');
  sh(root, 'commit', '-q', '-m', 'main');
  sh(root, 'checkout', '-q', '-b', 'train');
  writeFileSync(join(root, 'conformance/c130-allow.json'), allowJson(trainList));
  sh(root, 'commit', '-q', '-a', '-m', 'train member 1');
  sh(root, 'checkout', '-q', '-b', 'member');
  writeFileSync(join(root, 'conformance/c130-allow.json'), allowJson(memberList));
  sh(root, 'commit', '-q', '-a', '--allow-empty', '-m', 'member');
  return root;
}
const growthOn = async (root, baseRef) => (await findingsOf('C130', root, { options: { baseRef } })).filter((f) => /allow list grew/.test(f.message));

test('target ref: the train\'s own entries are not growth of a member branch measured against the train, but are against main', async () => {
  const root = trainRepo([ok(1)], [ok(3)], [ok(3)]);
  assert.equal((await growthOn(root, 'main')).length, 1, 'against main the train\'s raised count shows up as the member\'s growth');
  assert.deepEqual(await growthOn(root, 'train'), [], 'against the train the member added nothing');
  const grew = trainRepo([ok(1)], [ok(3)], [ok(4)]);
  const own = await growthOn(grew, 'train');
  assert.equal(own.length, 1, 'the member\'s own raise counts');
  assert.match(own[0].message, /the allow list grew: 1 entry is/);
});

test('target ref: the run\'s own, else PADDOCK_BASE_REF, else origin/main, and nothing else reads the variable', () => {
  const saved = process.env.PADDOCK_BASE_REF;
  try {
    delete process.env.PADDOCK_BASE_REF;
    assert.equal(targetRef({}), 'origin/main');
    assert.equal(targetRef(undefined), 'origin/main');
    process.env.PADDOCK_BASE_REF = 'origin/train/x';
    assert.equal(targetRef({}), 'origin/train/x');
    assert.equal(targetRef({ baseRef: 'topic' }), 'topic');
  } finally {
    if (saved === undefined) delete process.env.PADDOCK_BASE_REF;
    else process.env.PADDOCK_BASE_REF = saved;
  }
  // A source audit over the host's own code (not declared data): the variable is read in util/baseline.mjs only.
  const readers = [];
  const walk = (dir) => {
    for (const f of readdirSync(dir)) {
      const p = join(dir, f);
      if (statSync(p).isDirectory()) walk(p);
      else if (f.endsWith('.mjs') && /process\.env\.PADDOCK_BASE_REF/.test(readFileSync(p, 'utf8'))) readers.push(p);
    }
  };
  walk(new URL('../src', import.meta.url).pathname);
  assert.deepEqual(readers.map((p) => p.slice(p.indexOf('conformance/'))), ['conformance/src/util/baseline.mjs']);
});

test('allow list growth: a base in the older format says so with its own fix', async () => {
  const root = makeApp({ 'conformance/c130-allow.json': JSON.stringify({ allow: [{ path: 'tools/t/tests/it/ok.rs', reason: 'an older entry without a site' }] }), 'tools/t/tests/it/ok.rs': 'x\n' });
  sh(root, 'init', '-q', '-b', 'main');
  sh(root, 'add', '.');
  sh(root, 'commit', '-q', '-m', 'old');
  sh(root, 'checkout', '-q', '-b', 'topic');
  writeFileSync(join(root, 'conformance/c130-allow.json'), allowJson([ok(1)]));
  sh(root, 'commit', '-q', '-a', '-m', 'new');
  const w = (await findingsOf('C130', root, { options: { baseRef: 'main' } })).filter((f) => /shrink-only rule is not enforced/.test(f.message));
  assert.equal(w.length, 1);
  assert.match(w[0].message, /cannot be parsed/);
  assert.match(w[0].fix, /older format/);
});
