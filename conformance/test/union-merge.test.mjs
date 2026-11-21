// The append-only docs merge with the lessons driver (.gitattributes `merge=lessons`: git's union merge, then the blank line before each
// heading restored by `./dev merge lessons-driver`). The attribute is read through git itself (`git check-attr`); the real three-way
// merge is tools/paddock-dev/tests/it/lessons_merge.rs (it needs the driver, a `./dev` command).
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');

/** The append-only log: new sections are added, earlier ones are not rewritten. Only its git attribute is read here (conformance C145 reads its text;
 * the planner's DOC_INPUTS, tools/paddock-dev/src/plan.rs, lists it). */
const APPEND_ONLY = ['docs/lessons.md'];
/** Edited in place (a corrected results block): union would keep both versions silently. */
const EDITED_IN_PLACE = ['docs/benchmarks.md', 'apps/dyno/docs/performance.md', 'docs/gates.toml'];

function git(cwd, ...args) {
  const r = spawnSync('git', args, {
    cwd,
    encoding: 'utf8',
    env: { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_NOSYSTEM: '1', GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@example.invalid', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@example.invalid' },
  });
  return { status: r.status, out: (r.stdout ?? '') + (r.stderr ?? '') };
}

test('the append-only docs use the lessons merge driver', () => {
  for (const file of APPEND_ONLY) {
    const r = git(root, 'check-attr', 'merge', '--', file);
    assert.equal(r.status, 0, r.out);
    assert.match(r.out, /: merge: lessons$/m, `${file} has no merge=lessons in .gitattributes: ${r.out}`);
  }
  // Files edited in place must conflict on two edits of one line, not keep both.
  for (const file of EDITED_IN_PLACE) assert.match(git(root, 'check-attr', 'merge', '--', file).out, /: merge: unspecified$/m, file);
});
