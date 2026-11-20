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
 * kept to one line so that no check flags this file itself.
 */
const SAMPLE = {
  C30: ['crates/a/tests/t.rs', '#[test]\nfn t() {\n    let s = std::time::Instant::now();\n    assert!(s.elapsed() < std::time::Duration::from_secs(1));\n}\n'],
  C135: ['apps/a/src/x.spec.ts', "installLeakDetector({ mode: 'off' });\n"],
  C140: ['apps/a/app/src/x.store.ts', '// per-item: each runner answers on its own pipe\n'],
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

test('allow list: every check that reads one has a sample here, and C30, C135 and C140 read one (C121, C124, C125, C130 and C131 are Rust-only since pair P3)', () => {
  assert.deepEqual(users.map((c) => c.id).filter((id) => !SAMPLE[id]), []);
  assert.deepEqual(users.map((c) => c.id), ['C30', 'C135', 'C140']);
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

// ---- the list only shrinks: every entry, counted `count` times, against the merge base (the reader's own growth rule, driven here through C135, the one check left on the JavaScript host that reads a list) ----
import { execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';

const allowJson = (entries) => JSON.stringify({ allow: entries });
const sh = (root, ...args) => execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', '-c', 'commit.gpgsign=false', ...args], { cwd: root, stdio: 'ignore' });
/** A repository whose merge base (`main`) has the allow list `base` and whose `topic` commit has `now`. */
function growthRepo(base, now) {
  const SHELL = "installLeakDetector({ mode: 'off' });\n";
  const root = makeApp({ 'conformance/c135-allow.json': allowJson(base), 'apps/a/src/ok.spec.ts': SHELL.repeat(3) });
  sh(root, 'init', '-q', '-b', 'main');
  sh(root, 'add', '.');
  sh(root, 'commit', '-q', '-m', 'base');
  sh(root, 'checkout', '-q', '-b', 'topic');
  writeFileSync(join(root, 'conformance/c135-allow.json'), allowJson(now));
  sh(root, 'commit', '-q', '-a', '--allow-empty', '-m', 'topic');
  return root;
}
const ok = (count) => ({ path: 'apps/a/src/ok.spec.ts', site: "mode: 'off'", reason: 'baseline: opt-outs that were there before the list', count });
const grew = async (root) => (await findingsOf('C135', root, { options: { baseRef: 'main' } })).filter((f) => /allow list (grew|.s shrink-only)/.test(f.message));

test('allow list growth: an unchanged or shrunk list says nothing, a raised count or a new entry is one unwaivable finding at the allow file', async () => {
  assert.deepEqual(await grew(growthRepo([ok(3)], [ok(3)])), []);
  assert.deepEqual(await grew(growthRepo([ok(3)], [ok(2)])), [], 'a lowered count is shrinking');
  const raised = await grew(growthRepo([ok(2)], [ok(3)]));
  assert.deepEqual(raised.map((f) => [f.file, f.line, f.unwaivable]), [['conformance/c135-allow.json', 1, true]]);
  assert.match(raised[0].message, /the allow list grew: 1 entry is \(counting each `count`\) not in conformance\/c135-allow\.json at the merge base [0-9a-f]{9}; it may only shrink/);
  const added = await grew(growthRepo([ok(1)], [ok(1), { ...ok(2), site: 'skip:' }]));
  assert.match(added[0].message, /grew: 2 entries are/);
  assert.match(added[0].fix, /user's approval/);
});

test('allow list growth: a list new at the merge base is an empty base, so its entries are growth (ER-46); a fixture folder without .git and an unmeasurable base are not', async () => {
  const first = makeApp({ 'apps/a/src/ok.spec.ts': 'x\n', 'conformance/.keep': '' });
  sh(first, 'init', '-q', '-b', 'main');
  sh(first, 'add', '.');
  sh(first, 'commit', '-q', '-m', 'main: no list');
  sh(first, 'checkout', '-q', '-b', 'topic');
  writeFileSync(join(first, 'conformance/c135-allow.json'), allowJson([ok(3)]));
  sh(first, 'add', '.');
  sh(first, 'commit', '-q', '-m', 'topic: the list appears with 3 entries');
  const intro = await grew(first);
  assert.equal(intro.length, 1, 'a new list is growth');
  assert.match(intro[0].message, /the allow list grew: 3 entries are/);
  assert.deepEqual(await grew(makeApp({ 'conformance/c135-allow.json': allowJson([ok(3)]) })), [], 'no .git: nothing to compare, nothing said');
  const none = growthRepo([ok(1)], [ok(1)]);
  const w = (await findingsOf('C135', none, { options: { baseRef: 'no-such-ref' } })).filter((f) => /allow list's shrink-only rule is not enforced/.test(f.message));
  assert.equal(w.length, 1, 'a checkout where git cannot give a base says so, never silence');
});

// ---- the target ref: a member branch of a train is measured against the train, not main (ER-21; ADR 0098 amendment of 2026-11-16) ----
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { isCi, targetRef } from '../src/util/baseline.mjs';

/** main has the list `base`; `train` (off main) has `trainList`; `member` (off train) has `memberList`. */
function trainRepo(base, trainList, memberList) {
  const root = makeApp({ 'conformance/c135-allow.json': allowJson(base), 'apps/a/src/ok.spec.ts': "installLeakDetector({ mode: 'off' });\n".repeat(4) });
  sh(root, 'init', '-q', '-b', 'main');
  sh(root, 'add', '.');
  sh(root, 'commit', '-q', '-m', 'main');
  sh(root, 'checkout', '-q', '-b', 'train');
  writeFileSync(join(root, 'conformance/c135-allow.json'), allowJson(trainList));
  sh(root, 'commit', '-q', '-a', '-m', 'train member 1');
  sh(root, 'checkout', '-q', '-b', 'member');
  writeFileSync(join(root, 'conformance/c135-allow.json'), allowJson(memberList));
  sh(root, 'commit', '-q', '-a', '--allow-empty', '-m', 'member');
  return root;
}
const growthOn = async (root, baseRef) => (await findingsOf('C135', root, { options: { baseRef } })).filter((f) => /allow list grew/.test(f.message));

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
  // A source audit over the host's own code (not declared data): outside util/baseline.mjs no code line names the variable (a read by
  // `process.env.X`, `process.env['X']`, a destructuring or `env[...]` all name it), over src, bin and the CI scripts.
  const offenders = [];
  const walk = (dir) => {
    for (const f of readdirSync(dir)) {
      const p = join(dir, f);
      if (f === 'node_modules' || f === 'test' || f === 'fixtures') continue;
      if (statSync(p).isDirectory()) walk(p);
      else if (/\.(?:mjs|js|cjs)$/.test(f) && !p.endsWith('conformance/src/util/baseline.mjs')) {
        const code = readFileSync(p, 'utf8').split('\n').filter((l) => !/^\s*(?:\/\/|\*|\/\*)/.test(l)).map((l) => l.replace(/\s\/\/.*$/, ''));
        if (code.some((l) => l.includes('PADDOCK_BASE_REF'))) offenders.push(p.slice(p.indexOf('conformance/')));
      }
    }
  };
  walk(new URL('..', import.meta.url).pathname);
  assert.deepEqual(offenders, []);
});

test('target ref: an empty PADDOCK_BASE_REF or run ref is unset', () => {
  const saved = process.env.PADDOCK_BASE_REF;
  try {
    process.env.PADDOCK_BASE_REF = '';
    assert.equal(targetRef({}), 'origin/main');
    assert.equal(targetRef({ baseRef: '' }), 'origin/main');
    process.env.PADDOCK_BASE_REF = 'origin/train/x';
    assert.equal(targetRef({ baseRef: '' }), 'origin/train/x');
  } finally {
    if (saved === undefined) delete process.env.PADDOCK_BASE_REF;
    else process.env.PADDOCK_BASE_REF = saved;
  }
});

test('allow list growth: a target ref that cannot be seen is a warning locally and a not-measured error in CI (ADR 0089)', async () => {
  const root = growthRepo([ok(1)], [ok(1)]);
  const said = async (ci) => (await findingsOf('C135', root, { options: { baseRef: 'no-such-ref', ci } })).filter((f) => /allow list's shrink-only rule/.test(f.message));
  const local = await said(false);
  assert.deepEqual(local.map((f) => [f.severity, f.unwaivable ?? false]), [['warn', false]]);
  const ci = await said(true);
  assert.deepEqual(ci.map((f) => [f.severity, f.unwaivable, f.outcome]), [['error', true, 'not-measured']]);
  assert.match(ci[0].message, /^not measured: the allow list's shrink-only rule cannot be checked/);
  assert.match(ci[0].fix, /ADR 0089/);
  // The older format at the base is not a missing target: a warning with its own fix even in CI.
  assert.equal(isCi({ CI: 'true' }), true);
  assert.equal(isCi({ CI: 'false' }), false);
  assert.equal(isCi({ GITEA_ACTIONS: 'true' }), true);
  assert.equal(isCi({}), false);
});

test('allow list growth: a base in the older format says so with its own fix', async () => {
  const root = makeApp({ 'conformance/c135-allow.json': JSON.stringify({ allow: [{ path: 'apps/a/src/ok.spec.ts', reason: 'an older entry without a site' }] }), 'apps/a/src/ok.spec.ts': 'x\n' });
  sh(root, 'init', '-q', '-b', 'main');
  sh(root, 'add', '.');
  sh(root, 'commit', '-q', '-m', 'old');
  sh(root, 'checkout', '-q', '-b', 'topic');
  writeFileSync(join(root, 'conformance/c135-allow.json'), allowJson([ok(1)]));
  sh(root, 'commit', '-q', '-a', '-m', 'new');
  const w = (await findingsOf('C135', root, { options: { baseRef: 'main' } })).filter((f) => /allow list's shrink-only rule is not enforced/.test(f.message));
  assert.equal(w.length, 1);
  assert.match(w[0].message, /cannot be parsed/);
  assert.match(w[0].fix, /older format/);
});
