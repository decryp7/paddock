import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { after, test } from 'node:test';
import { README_BEGIN, README_END, README_PATH, REGENERATE, adrFiles, entryOf, readmeText } from '../src/adr-index.mjs';

const ROOT = resolve(import.meta.dirname, '..', '..');
const BIN = join(ROOT, 'conformance/bin/paddock-adr-index.mjs');
const scratch = mkdtempSync(join(tmpdir(), 'adr-index-test-'));
after(() => rmSync(scratch, { recursive: true, force: true }));

const adr = (n, title, status = 'Accepted 2026-11-01', date = '2026-11-01') => `# ${n}. ${title}\n\n- Status: ${status}\n- Date: ${date}\n\nBody.\n`;
const run = (root, ...args) => spawnSync('node', [BIN, '--root', root, ...args], { encoding: 'utf8', env: { ...process.env, CI: '' } });
/** git in a scratch repo only: the caller's GIT_* variables (GIT_DIR, GIT_WORK_TREE, GIT_INDEX_FILE of a hook run) are dropped so it can never reach the real repository, and identity comes from -c, never from `git config` (ER-45, ER-47: pass the environment except what would redirect the child). */
const git = (cwd, ...args) => {
  assert.ok(cwd.startsWith(scratch), `git runs only inside the scratch folder, not in ${cwd}`);
  const env = Object.fromEntries(Object.entries(process.env).filter(([k]) => !k.startsWith('GIT_')));
  return spawnSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', '-c', 'commit.gpgsign=false', ...args], {
    cwd,
    encoding: 'utf8',
    env: { ...env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_SYSTEM: '/dev/null' },
  });
};
/** A scratch repo root with docs/adr (a README with markers and the given ADR files). */
function makeRoot(name, files) {
  const root = join(scratch, name);
  mkdirSync(join(root, 'docs/adr'), { recursive: true });
  writeFileSync(join(root, README_PATH), `# ADRs\n\n${README_BEGIN}\n${README_END}\n`);
  for (const [f, t] of Object.entries(files)) writeFileSync(join(root, 'docs/adr', f), t);
  return root;
}

test('the committed ADR index is what the ADR files give', () => {
  const r = run(ROOT, '--check');
  assert.equal(r.status, 0, `${r.stderr}\nrun \`${REGENERATE}\``);
});

test('positive control: a hand-edited index fails --check, names the regenerate command, and a rewrite repairs it', () => {
  const root = makeRoot('hand', { '0001-a.md': adr(1, 'First'), '0002-b.md': adr(2, 'Second') });
  assert.equal(run(root).status, 0);
  assert.equal(run(root, '--check').status, 0);
  const p = join(root, README_PATH);
  writeFileSync(p, readFileSync(p, 'utf8').replace('| First |', '| First (edited by hand) |'));
  const bad = run(root, '--check');
  assert.equal(bad.status, 1);
  assert.match(bad.stderr, /is stale: run `node conformance\/bin\/paddock-adr-index\.mjs`/);
  assert.equal(run(root).status, 0);
  assert.equal(run(root, '--check').status, 0);
});

test('a new ADR file with no other edit is indexed after the regenerate; a missing one leaves the index stale', () => {
  const root = makeRoot('new', { '0001-a.md': adr(1, 'First') });
  assert.equal(run(root).status, 0);
  writeFileSync(join(root, 'docs/adr/0002-b.md'), adr(2, 'Second | with a pipe', 'Proposed (2026-11-02)', ''));
  assert.equal(run(root, '--check').status, 1);
  run(root);
  assert.match(readFileSync(join(root, README_PATH), 'utf8'), /^\| \[0002\]\(0002-b\.md\) \| Second \\\| with a pipe \| Proposed \| 2026-11-02 \|$/m);
});

test('entryOf: status keyword and date from the status line when there is no Date line; heading and name must agree', () => {
  const e = entryOf('0007-x.md', '# 7. X\n\nStatus: accepted by the user on 2026-11-13 (the redesign)\n');
  assert.deepEqual([e.number, e.title, e.status, e.date], ['0007', 'X', 'Accepted', '2026-11-13']);
  assert.equal(entryOf('0103-y.md', '# 0103. Y\n').status, 'No status line');
  assert.throws(() => entryOf('0008-x.md', '# 7. X\n'), /heading says ADR 7, the file name 0008/);
  assert.throws(() => entryOf('0008-x.md', 'No heading\n'), /first line must be/);
  assert.throws(() => readmeText('no markers', []), /needs the lines/);
  assert.deepEqual(adrFiles(['README.md', '0002-b.md', 'trims-2026.md', '0001-a.md', 'amendments']), ['0001-a.md', '0002-b.md']);
});

test('two branches that each add an ADR file merge without a conflict, and the regenerated index lists both', () => {
  const root = makeRoot('merge', { '0001-a.md': adr(1, 'First') });
  cpSync(join(ROOT, '.gitattributes'), join(root, '.gitattributes'));
  writeFileSync(join(root, 'CHANGELOG.md'), '# Changelog\n\n## Unreleased\n\n- base entry\n');
  assert.equal(run(root).status, 0);
  git(root, 'init', '-q', '-b', 'main');
  git(root, 'add', '-A');
  assert.equal(git(root, 'commit', '-q', '-m', 'base').status, 0);
  const branch = (name, n) => {
    git(root, 'checkout', '-q', '-b', name, 'main');
    writeFileSync(join(root, `docs/adr/000${n}-${name}.md`), adr(n, `ADR from ${name}`));
    assert.equal(run(root).status, 0); // the author regenerates the index, so both branches edit the table
    const cl = join(root, 'CHANGELOG.md');
    writeFileSync(cl, readFileSync(cl, 'utf8').replace('## Unreleased\n\n', `## Unreleased\n\n- entry of ${name}\n`));
    git(root, 'add', '-A');
    assert.equal(git(root, 'commit', '-q', '-m', name).status, 0);
  };
  branch('one', 2);
  branch('two', 3);
  git(root, 'checkout', '-q', 'one');
  const merged = git(root, 'merge', '--no-edit', 'two');
  assert.equal(merged.status, 0, merged.stdout + merged.stderr);
  const cl = readFileSync(join(root, 'CHANGELOG.md'), 'utf8');
  assert.match(cl, /- entry of one/);
  assert.match(cl, /- entry of two/);
  assert.doesNotMatch(cl, /<<<<<<<|>>>>>>>/);
  // union kept both sides' rows with no conflict; the check then holds the merge result to the generator, and one command repairs it
  assert.doesNotMatch(readFileSync(join(root, README_PATH), 'utf8'), /<<<<<<<|>>>>>>>/);
  run(root);
  assert.equal(run(root, '--check').status, 0);
  const idx = readFileSync(join(root, README_PATH), 'utf8');
  assert.match(idx, /ADR from one/);
  assert.match(idx, /ADR from two/);
});

test('positive control for the changelog rule: without the union attribute the same merge conflicts', () => {
  const root = makeRoot('noattr', { '0001-a.md': adr(1, 'First') });
  writeFileSync(join(root, 'CHANGELOG.md'), '# Changelog\n\n## Unreleased\n\n- base entry\n');
  git(root, 'init', '-q', '-b', 'main');
  git(root, 'add', '-A');
  git(root, 'commit', '-q', '-m', 'base');
  for (const name of ['one', 'two']) {
    git(root, 'checkout', '-q', '-b', name, 'main');
    const cl = join(root, 'CHANGELOG.md');
    writeFileSync(cl, readFileSync(cl, 'utf8').replace('## Unreleased\n\n', `## Unreleased\n\n- entry of ${name}\n`));
    git(root, 'commit', '-q', '-am', name);
  }
  git(root, 'checkout', '-q', 'one');
  assert.notEqual(git(root, 'merge', '--no-edit', 'two').status, 0);
});

test('.gitattributes lists the changelogs as union', () => {
  const have = readFileSync(join(ROOT, '.gitattributes'), 'utf8');
  assert.match(have, /^\/CHANGELOG\.md merge=union$/m);
  assert.match(have, /^\/apps\/\*\/CHANGELOG\.md merge=union$/m);
  assert.ok(have.split('\n').includes('/docs/adr/README.md merge=union'), "the ADR index must be union (its generator's --check holds the merge result)");
});
