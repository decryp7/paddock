// The merge driver of docs/lessons.md (scripts/lessons-merge.sh, `merge=lessons`): a real three-way merge of two branches that each
// append a section. Git's union merge drops the blank line before the second `## ` heading (9ec43be3c, b9bf5fc4b, 75ce0d2c8, 5c241d93f);
// the control runs the same merge with the plain union attribute and shows the drop, so the fixture is known to reproduce it (ER-33).
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { PADDOCK, makeApp } from './helpers.mjs';

const SCRIPT = join(PADDOCK, 'scripts', 'lessons-merge.sh');
const BASE = '# Lessons\n\nIntro.\n\n## Old\n- x\n';

function git(cwd, ...args) {
  const r = spawnSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.invalid', '-c', 'commit.gpgsign=false', ...args], { cwd, encoding: 'utf8' });
  return r;
}

/** A repository with `docs/lessons.md`, branches `a` and `b` each appending its own section, and `main` merged with both. */
function merged(attribute, driver) {
  const root = makeApp({ 'docs/lessons.md': BASE, '.gitattributes': `/docs/lessons.md merge=${attribute}\n` });
  mkdirSync(join(root, 'scripts'), { recursive: true });
  copyFileSync(SCRIPT, join(root, 'scripts', 'lessons-merge.sh'));
  for (const args of [['init', '-q', '-b', 'main'], ['add', '-A'], ['commit', '-q', '-m', 'base']]) assert.equal(git(root, ...args).status, 0);
  if (driver) git(root, 'config', 'merge.lessons.driver', 'bash scripts/lessons-merge.sh %O %A %B');
  for (const [name, text] of [['a', 'A'], ['b', 'B']]) {
    git(root, 'checkout', '-q', '-b', name, 'main');
    writeFileSync(join(root, 'docs/lessons.md'), `${BASE}\n## New ${text}\n- ${text.toLowerCase()}\n`);
    git(root, 'commit', '-q', '-am', name);
  }
  git(root, 'checkout', '-q', 'main');
  assert.equal(git(root, 'merge', '-q', '--no-edit', 'a').status, 0);
  const second = git(root, 'merge', '-q', '--no-edit', 'b');
  return { second, text: readFileSync(join(root, 'docs/lessons.md'), 'utf8') };
}

const glued = (text) => text.split('\n').some((l, i, ls) => l.startsWith('## ') && i > 0 && ls[i - 1] !== '');

test('control: git\'s plain union merge of two appended sections glues the second heading (the drop reproduces)', () => {
  const { second, text } = merged('union', false);
  assert.equal(second.status, 0, second.stderr);
  assert.ok(text.includes('## New A') && text.includes('## New B'), 'both sections are kept');
  assert.ok(glued(text), `the union dropped the blank line:\n${text}`);
});

test('the lessons driver keeps both sections and the blank line before each heading', () => {
  const { second, text } = merged('lessons', true);
  assert.equal(second.status, 0, second.stderr);
  assert.ok(!glued(text), `a heading is glued:\n${text}`);
  assert.equal(text, `${BASE}\n## New A\n- a\n\n## New B\n- b\n`);
});

test('the driver leaves headings in fenced blocks alone and a file with no conflict as it was', () => {
  const root = makeApp({ 'base': '# T\n\n## A\n- x\n', 'ours': '# T\n\n## A\n- x\n\n```\ntext\n## not a heading\n```\n', 'theirs': '# T\n\n## A\n- x\n' });
  const r = spawnSync('bash', [SCRIPT, join(root, 'base'), join(root, 'ours'), join(root, 'theirs')], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(readFileSync(join(root, 'ours'), 'utf8'), '# T\n\n## A\n- x\n\n```\ntext\n## not a heading\n```\n');
});

test('a failing git merge-file is an error annotation and a failure, never a silent pass', () => {
  const root = makeApp({ 'base': 'x\n', 'theirs': 'x\n' });
  const r = spawnSync('bash', [SCRIPT, join(root, 'base'), join(root, 'missing'), join(root, 'theirs')], { encoding: 'utf8' });
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /::error::lessons-merge/);
});
