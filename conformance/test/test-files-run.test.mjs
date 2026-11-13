// A test file that no CI step or package script runs proves nothing (recurring class: guard tests nothing ran, three times).
// Every `*.test.mjs` under scripts/, conformance/ and tools/ must be named by a `node --test` command in a workflow, an action
// or a package.json script, or be listed in UNRUN with the reason. Matching is by path suffix (a command's `cd` or
// `working-directory` is not followed), so a glob in another folder with the same tail can cover a file it does not run; the
// check finds the file nobody names, not every misplaced glob.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';

/** Test files nothing runs on purpose: path -> the reason. Empty is the goal. */
export const UNRUN = {};

/** The path patterns that `node --test` commands (and `npm run` scripts that contain them) name in `text`. */
export function nodeTestPatterns(text) {
  const out = [];
  for (const m of text.matchAll(/node\b[^\n|&;"]*?--test\b([^\n|&;"]*(?:\\\n[^\n|&;"]*)*)/g)) {
    for (const tok of m[1].replace(/\\\n/g, ' ').split(/\s+/)) {
      if (tok && !tok.startsWith('-') && /\.m?js$|\*|\/$/.test(tok)) out.push(tok.replace(/^(\.\.?\/)+/, ''));
    }
  }
  return out;
}

/** Is `file` named by one of `patterns` (a path, or a glob whose `*` stays inside one folder)? */
export function covered(file, patterns) {
  return patterns.some((p) => {
    const re = new RegExp(`(^|/)${p.replace(/[.+?^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '[^/]*')}$`);
    return re.test(file);
  });
}

test('the reader finds the paths a node --test command names', () => {
  const p = nodeTestPatterns('run: node --test scripts/a.test.mjs scripts/*.test.mjs\n  x: node --disable-warning=X --test ../scripts/*.test.mjs scripts/b.test.mjs && echo hi');
  assert.deepEqual(p, ['scripts/a.test.mjs', 'scripts/*.test.mjs', 'scripts/*.test.mjs', 'scripts/b.test.mjs']);
  assert.equal(covered('scripts/x.test.mjs', ['scripts/*.test.mjs']), true);
  assert.equal(covered('scripts/vm-smoke/x.test.mjs', ['scripts/*.test.mjs']), false, '* stays in one folder');
  assert.equal(covered('conformance/test/x.test.mjs', ['test/*.test.mjs']), true);
});

test('every test file under scripts, conformance and tools is run by a CI step or a package script', () => {
  const git = (...a) => execFileSync('git', ['-C', PADDOCK, ...a], { encoding: 'utf8', maxBuffer: 1 << 26 }).split('\n').filter(Boolean);
  const files = git('ls-files', '--', 'scripts/**/*.test.mjs', 'scripts/*.test.mjs', 'conformance/**/*.test.mjs', 'tools/**/*.test.mjs');
  const runners = git('ls-files', '--', '.gitea/workflows/*.yml', '.gitea/actions/*/action.yml', 'package.json', '*/package.json', '*/paddock.ci.json');
  const patterns = runners.flatMap((f) => nodeTestPatterns(readFileSync(join(PADDOCK, f), 'utf8')));
  assert.ok(files.length > 50 && patterns.length > 5, `found ${files.length} test files and ${patterns.length} patterns`);
  const unrun = files.filter((f) => !covered(f, patterns) && !(f in UNRUN));
  assert.deepEqual(unrun, [], 'no CI step or package script runs these tests: add them to a node --test line, or to UNRUN with a reason');
  for (const [f, why] of Object.entries(UNRUN)) {
    assert.ok(why.length >= 15, `${f}: UNRUN needs a reason`);
    assert.ok(files.includes(f), `${f} is listed in UNRUN but is not a tracked test file`);
    assert.ok(!covered(f, patterns), `${f} is now run; remove it from UNRUN`);
  }
});
