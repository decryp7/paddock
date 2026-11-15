// The append-only docs merge with git's union driver (.gitattributes): two branches that each add a section at the end of
// docs/lessons.md merge without a conflict. The attribute is read through git itself (`git check-attr`), and the merge runs in a
// scratch repository that carries the repository's own .gitattributes.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');

/** The append-only logs: new sections are added, earlier ones are not rewritten. */
const APPEND_ONLY = ['docs/lessons.md', 'docs/benchmarks.md', 'apps/dyno/docs/performance.md', 'apps/debrief/docs/performance.md'];

function git(cwd, ...args) {
  const r = spawnSync('git', args, {
    cwd,
    encoding: 'utf8',
    env: { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_NOSYSTEM: '1', GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@example.invalid', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@example.invalid' },
  });
  return { status: r.status, out: (r.stdout ?? '') + (r.stderr ?? '') };
}

test('the append-only docs use the union merge driver', () => {
  for (const file of APPEND_ONLY) {
    const r = git(root, 'check-attr', 'merge', '--', file);
    assert.equal(r.status, 0, r.out);
    assert.match(r.out, /: merge: union$/m, `${file} has no merge=union in .gitattributes: ${r.out}`);
  }
  // A register is not append-only text: two edits of one row must conflict, not be kept both.
  assert.match(git(root, 'check-attr', 'merge', '--', 'docs/gates.toml').out, /: merge: unspecified$/m);
});

test('two branches that each add a lesson at the end merge without a conflict and keep both', () => {
  const dir = mkdtempSync(join(tmpdir(), 'union-merge-'));
  try {
    assert.equal(git(dir, 'init', '-q', '-b', 'main').status, 0);
    copyFileSync(join(root, '.gitattributes'), join(dir, '.gitattributes'));
    mkdirSync(join(dir, 'docs'));
    const lessons = join(dir, 'docs', 'lessons.md');
    writeFileSync(lessons, '# Lessons\n\n## First (2026-11-01)\n\n- One.\n');
    git(dir, 'add', '-A');
    assert.equal(git(dir, 'commit', '-qm', 'base').status, 0);
    git(dir, 'checkout', '-qb', 'a');
    writeFileSync(lessons, readFileSync(lessons, 'utf8') + '\n## From a (2026-11-15)\n\n- Lesson a.\n');
    git(dir, 'commit', '-qam', 'a');
    git(dir, 'checkout', '-q', 'main');
    git(dir, 'checkout', '-qb', 'b');
    writeFileSync(lessons, readFileSync(lessons, 'utf8') + '\n## From b (2026-11-15)\n\n- Lesson b.\n');
    git(dir, 'commit', '-qam', 'b');
    const m = git(dir, 'merge', '--no-edit', 'a');
    assert.equal(m.status, 0, `the merge conflicted: ${m.out}`);
    const merged = readFileSync(lessons, 'utf8');
    assert.match(merged, /## From a[\s\S]*- Lesson a\./);
    assert.match(merged, /## From b[\s\S]*- Lesson b\./);
    assert.doesNotMatch(merged, /^(<<<<<<<|>>>>>>>|=======)/m);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
