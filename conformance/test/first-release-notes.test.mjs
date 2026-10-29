// The notes of an app's first release come from its CHANGELOG (conformance/ci/first-release-notes.sh), in two steps of
// app-release.yml. The steps are read out of the workflow with parseSteps and run for real in a scratch repository.
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { after, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { parseSteps } from '../src/util/workflows.mjs';

const PADDOCK = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const steps = parseSteps(readFileSync(join(PADDOCK, '.gitea/workflows/app-release.yml'), 'utf8'));
const notesStep = steps.find((s) => s.name === 'Changelog of this release');
const failFastStep = steps.find((s) => s.name === "The first release's CHANGELOG section is ready");

const scratch = mkdtempSync(join(tmpdir(), 'first-notes-'));
after(() => rmSync(scratch, { recursive: true, force: true }));

// A stand-in git-cliff: `git cliff` runs the `git-cliff` found on PATH and the notes then say it ran.
const bin = join(scratch, 'bin');
mkdirSync(bin);
writeFileSync(join(bin, 'git-cliff'), '#!/bin/sh\nwhile [ $# -gt 0 ]; do if [ "$1" = -o ]; then shift; printf "FROM GIT-CLIFF\\n" > "$1"; fi; shift; done\n');
chmodSync(join(bin, 'git-cliff'), 0o755);

let n = 0;
/** A scratch repository with the CHANGELOG text, the given earlier tags, and the tag being released checked out. */
function repo(changelog, { earlier = [] } = {}) {
  const dir = join(scratch, `r${n++}`);
  mkdirSync(join(dir, 'release'), { recursive: true });
  symlinkSync(PADDOCK, join(dir, '.paddock'));
  const git = (...a) => execFileSync('git', a, { cwd: dir, stdio: 'pipe' });
  git('init', '-q');
  git('config', 'user.email', 't@example.invalid');
  git('config', 'user.name', 't');
  writeFileSync(join(dir, 'CHANGELOG.md'), changelog);
  git('add', 'CHANGELOG.md');
  git('commit', '-q', '-m', 'feat: x');
  for (const t of earlier) git('tag', t);
  git('tag', 'dyno-v1.0.0');
  return dir;
}
const run = (step, dir) =>
  spawnSync('bash', ['-c', step.run], {
    cwd: dir,
    encoding: 'utf8',
    env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, FIRST_CHANGELOG: 'CHANGELOG.md', TAG: 'dyno-v1.0.0', PREFIX: 'dyno-', CONFIG: '', PATHS: '' },
  });
const notes = (dir) => readFileSync(join(dir, 'release/CHANGELOG-latest.md'), 'utf8');

const GOOD = '# Changelog\n\n## [1.0.0] - 2026-11-01\n\n### Added\n\n- Dyno measures.\n- It explains.\n\n[1.0.0]: https://example.invalid/dyno-v1.0.0\n\n## [0.9.0] - 2026-10-01\n\n- older\n';

test('both steps exist in the workflow and run the shared script', () => {
  assert.ok(notesStep && failFastStep);
  assert.match(notesStep.run, /first-release-notes\.sh/);
  assert.match(failFastStep.run, /first-release-notes\.sh/);
  assert.match(failFastStep.text, /first-release-changelog != ''/);
});

test('the first release gets its CHANGELOG section: no heading, no link references, nothing of the next section', () => {
  const dir = repo(GOOD);
  const r = run(notesStep, dir);
  assert.equal(r.status, 0, r.stderr + r.stdout);
  const text = notes(dir);
  assert.match(text, /- Dyno measures\.\n- It explains\./);
  assert.doesNotMatch(text, /^## |\[1\.0\.0\]:|older|GIT-CLIFF/m);
});

test('with an earlier tag of the app, git-cliff writes the notes', () => {
  const dir = repo(GOOD, { earlier: ['dyno-v0.9.0'] });
  const r = run(notesStep, dir);
  assert.equal(r.status, 0, r.stderr);
  assert.equal(notes(dir), 'FROM GIT-CLIFF\n');
});

test('a tag of another app is not an earlier tag', () => {
  const dir = repo(GOOD, { earlier: ['spotter-v1.0.0'] });
  assert.equal(run(notesStep, dir).status, 0);
  assert.match(notes(dir), /Dyno measures/);
});

test('without a configured CHANGELOG the notes come from git-cliff', () => {
  const dir = repo(GOOD);
  const r = spawnSync('bash', ['-c', notesStep.run], {
    cwd: dir,
    encoding: 'utf8',
    env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, FIRST_CHANGELOG: '', TAG: 'dyno-v1.0.0', PREFIX: 'dyno-', CONFIG: '', PATHS: '' },
  });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(notes(dir), 'FROM GIT-CLIFF\n');
});

for (const [what, text, message] of [
  ['a missing section', '# Changelog\n\n## [0.9.0] - 2026-10-01\n\n- older\n', /no '## \[1\.0\.0\]' section/],
  ['an Unreleased heading', GOOD.replace('2026-11-01', 'Unreleased'), /still says/],
  ['an empty section', '## [1.0.0] - 2026-11-01\n\n## [0.9.0] - x\n- older\n', /is empty/],
  ['a whitespace-only section', '## [1.0.0] - 2026-11-01\n  \n\t\n   \n## [0.9.0] - x\n- older\n', /is empty/],
  ['a section of link references only', '## [1.0.0] - 2026-11-01\n\n[1.0.0]: https://example.invalid/a\n[0.9.0]: https://example.invalid/b\n', /is empty/],
]) {
  test(`the first release fails on ${what}, in the notes step and in the fail-fast step`, () => {
    for (const step of [notesStep, failFastStep]) {
      const r = run(step, repo(text));
      assert.notEqual(r.status, 0, `${step.name} accepted ${what}`);
      assert.match(r.stdout + r.stderr, message);
    }
  });
}

test('the fail-fast step does nothing for a later release, even when its section is missing', () => {
  const r = run(failFastStep, repo('# Changelog\n', { earlier: ['dyno-v1.0.0-rc.1'.replace('-rc.1', '').replace('1.0.0', '0.9.0')] }));
  assert.equal(r.status, 0, r.stdout + r.stderr);
});
