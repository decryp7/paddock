// The notes of an app's first release come from its CHANGELOG (conformance/ci/first-release-notes.sh), in two steps of
// app-release.yml. The steps are read out of the workflow with parseSteps and run for real in a scratch repository.
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
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

// notes-from-changelog (Dyno 1.1.0): the notes are the CHANGELOG section of the tagged version for every release, so an earlier tag
// of the app does not hand over to git-cliff (the history behind Dyno 1.1.0 is landing commits git-cliff cannot tell apart).
const runFromChangelog = (step, dir) =>
  spawnSync('bash', ['-c', step.run], {
    cwd: dir,
    encoding: 'utf8',
    env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, FIRST_CHANGELOG: 'CHANGELOG.md', NOTES_FROM_CHANGELOG: 'true', TAG: 'dyno-v1.0.0', PREFIX: 'dyno-', CONFIG: '', PATHS: '' },
  });

test('with notes-from-changelog an earlier tag still gets the CHANGELOG section, not git-cliff', () => {
  const dir = repo(GOOD, { earlier: ['dyno-v0.9.0'] });
  const r = runFromChangelog(notesStep, dir);
  assert.equal(r.status, 0, r.stderr);
  assert.match(notes(dir), /- Dyno measures\.\n- It explains\./);
  assert.doesNotMatch(notes(dir), /GIT-CLIFF|older|^## /m);
});

for (const [what, text, message] of [
  ['a missing section', '# Changelog\n\n## [0.9.0] - 2026-10-01\n\n- older\n', /no '## \[1\.0\.0\]' section/],
  ['an Unreleased heading', GOOD.replace('2026-11-01', 'Unreleased'), /still says/],
  ['an empty section', '## [1.0.0] - 2026-11-01\n\n## [0.9.0] - x\n- older\n', /is empty/],
]) {
  test(`with notes-from-changelog a later release fails on ${what}, in both steps (it would have gone to git-cliff before)`, () => {
    for (const step of [notesStep, failFastStep]) {
      const r = runFromChangelog(step, repo(text, { earlier: ['dyno-v0.9.0'] }));
      assert.notEqual(r.status, 0, `${step.name} accepted ${what}`);
      assert.match(r.stdout + r.stderr, message);
    }
  });
}

test('the workflow passes notes-from-changelog to both steps, and Dyno switches it on in both of its release jobs', () => {
  assert.match(notesStep.text, /NOTES_FROM_CHANGELOG: \$\{\{ inputs\.notes-from-changelog \}\}/);
  assert.match(failFastStep.text, /NOTES_FROM_CHANGELOG: \$\{\{ inputs\.notes-from-changelog \}\}/);
  const dyno = readFileSync(join(PADDOCK, '.gitea/workflows/dyno-release.yml'), 'utf8');
  assert.equal((dyno.match(/^ {6}notes-from-changelog: true$/gm) ?? []).length, 2);
  assert.equal((dyno.match(/first-release-changelog: apps\/dyno\/CHANGELOG\.md/g) ?? []).length, 2);
});

/** A bare "origin" that holds the given tags, and a --depth 1 clone of its tag `at` (what a CI checkout holds). */
function shallowClone(originTags, at, changelog = GOOD) {
  const work = repo(changelog);
  const git = (cwd, ...a) => execFileSync('git', a, { cwd, stdio: 'pipe' });
  git(work, 'tag', '-d', 'dyno-v1.0.0');
  // The earlier tags sit on an older commit, as real ones do: a shallow clone of the newer one does not receive them.
  for (const t of originTags) git(work, 'tag', t);
  writeFileSync(join(work, 'CHANGELOG.md'), changelog);
  git(work, 'commit', '-q', '-a', '--allow-empty', '-m', 'feat: y');
  git(work, 'tag', at);
  const origin = `${work}-origin.git`;
  execFileSync('git', ['clone', '-q', '--bare', work, origin], { stdio: 'pipe' });
  const dir = `${work}-ci`;
  execFileSync('git', ['clone', '-q', '--depth', '1', '--branch', at, `file://${origin}`, dir], { stdio: 'pipe' });
  mkdirSync(join(dir, 'release'), { recursive: true });
  symlinkSync(PADDOCK, join(dir, '.paddock'));
  return dir;
}
const runIn = (step, dir, tag) =>
  spawnSync('bash', ['-c', step.run], {
    cwd: dir,
    encoding: 'utf8',
    env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, FIRST_CHANGELOG: 'CHANGELOG.md', TAG: tag, PREFIX: 'dyno-', CONFIG: '', PATHS: '' },
  });

test('a shallow CI checkout finds the earlier tags on the server, without fetching them', () => {
  const dir = shallowClone(['dyno-v1.0.0'], 'dyno-v1.0.1', '# Changelog\n');
  assert.equal(execFileSync('git', ['tag', '-l'], { cwd: dir, encoding: 'utf8' }).trim(), 'dyno-v1.0.1', 'the checkout holds only its own tag');
  for (const step of [notesStep, failFastStep]) {
    const r = runIn(step, dir, 'dyno-v1.0.1');
    assert.equal(r.status, 0, `${step.name}: ${r.stdout}${r.stderr}`);
  }
  assert.equal(notes(dir), 'FROM GIT-CLIFF\n', 'not the first release: git-cliff writes the notes');
  assert.equal(execFileSync('git', ['tag', '-l'], { cwd: dir, encoding: 'utf8' }).trim(), 'dyno-v1.0.1', 'no tag was fetched');
});

test('a shallow checkout of the first tag is the first release', () => {
  const dir = shallowClone([], 'dyno-v1.0.0');
  assert.equal(runIn(notesStep, dir, 'dyno-v1.0.0').status, 0);
  assert.match(notes(dir), /Dyno measures/);
});

test('an origin that cannot be reached fails loudly and blames the tag listing, not the CHANGELOG', () => {
  const dir = shallowClone([], 'dyno-v1.0.0');
  execFileSync('git', ['remote', 'set-url', 'origin', 'file:///nonexistent/origin.git'], { cwd: dir });
  const r = runIn(notesStep, dir, 'dyno-v1.0.0');
  assert.notEqual(r.status, 0);
  assert.match(r.stdout + r.stderr, /could not list the tags of origin/);
  assert.doesNotMatch(r.stdout + r.stderr, /CHANGELOG\.md (has no|is empty|still)/);
});

test('the workflow does not fetch tags (the full history behind every tag)', () => {
  assert.doesNotMatch(failFastStep.run, /git fetch/);
});

// The awk program must read the same under mawk (Ubuntu's default awk), gawk and BSD awk. Every awk found on this machine
// runs the script on a fixture with link references; a bracket expression that holds ']' (`[^]]`, mawk and old BSD awks
// differ on it) is rejected by a lint of the script itself.
const awks = [];
for (const name of ['awk', 'mawk', 'gawk', 'original-awk', 'nawk', 'busybox']) {
  const found = spawnSync('sh', ['-c', `command -v ${name}`], { encoding: 'utf8' }).stdout.trim();
  if (found && !awks.some((a) => a.real === found)) awks.push({ name, real: found });
}

for (const awk of awks) {
  test(`first-release-notes.sh gives the same section under ${awk.name} (${awk.real})`, () => {
    const shim = join(scratch, `awk-${awk.name}`);
    mkdirSync(shim, { recursive: true });
    const target = awk.name === 'busybox' ? `#!/bin/sh\nexec ${awk.real} awk "$@"\n` : `#!/bin/sh\nexec ${awk.real} "$@"\n`;
    writeFileSync(join(shim, 'awk'), target);
    chmodSync(join(shim, 'awk'), 0o755);
    const dir = repo(GOOD + '\n[unreleased]: https://example.invalid/u\n');
    const r = spawnSync('bash', ['-c', notesStep.run], {
      cwd: dir,
      encoding: 'utf8',
      env: { ...process.env, PATH: `${shim}:${bin}:${process.env.PATH}`, FIRST_CHANGELOG: 'CHANGELOG.md', TAG: 'dyno-v1.0.0', PREFIX: 'dyno-', CONFIG: '', PATHS: '' },
    });
    assert.equal(r.status, 0, r.stderr + r.stdout);
    assert.equal(notes(dir), '\n### Added\n\n- Dyno measures.\n- It explains.\n');
  });
}

test('the CI shell scripts have no awk bracket expression that holds a closing bracket', () => {
  const dirs = ['conformance/ci', 'scripts'];
  const bad = [];
  const walk = (rel) => {
    for (const e of readdirSync(join(PADDOCK, rel), { withFileTypes: true })) {
      const p = join(rel, e.name);
      if (e.isDirectory()) walk(p);
      else if (e.name.endsWith('.sh')) {
        readFileSync(join(PADDOCK, p), 'utf8').split('\n').forEach((line, i) => {
          if (/\[\^\]/.test(line)) bad.push(`${p}:${i + 1}`);
        });
      }
    }
  };
  dirs.forEach(walk);
  assert.deepEqual(bad, []);
});

test('the script logs its awk, file, heading line and section size on stderr, and the first lines of an empty section', () => {
  const ok = run(notesStep, repo(GOOD));
  assert.match(ok.stderr, /first-release-notes: awk='.*' changelog=CHANGELOG\.md bytes=\d+ sha256=[0-9a-f]{64} heading_line=3/);
  assert.match(ok.stderr, /section lines=\d+ bytes=\d+/);
  const bad = run(notesStep, repo('## [1.0.0] - 2026-11-01\n\n[1.0.0]: https://example.invalid/a\n## [0.9.0] - x\n'));
  assert.match(bad.stderr + bad.stdout, /is empty/);
  assert.match(bad.stderr, /first 3 lines after the heading:\n  \| \n  \| \[1\.0\.0\]: https/);
});

// ER-11: a section larger than a pipe buffer (64 KB). `printf "$section" | grep -q` under pipefail lost a SIGPIPE race on it
// and reported a non-empty section as empty (the first Dyno release); the script now matches without a pipe.
test('a section larger than 64 KB is not reported empty (no pipe-fed grep -q under pipefail)', () => {
  const bullets = Array.from({ length: 1500 }, (_, i) => `- Item ${i}: ${'x'.repeat(60)}`).join('\n');
  const big = `## [1.0.0] - 2026-11-01\n\n${bullets}\n\n## [0.9.0] - x\n- older\n`;
  assert.ok(big.length > 100 * 1024);
  for (let i = 0; i < 5; i++) {
    const dir = repo(big);
    const r = run(notesStep, dir);
    assert.equal(r.status, 0, r.stderr + r.stdout);
    assert.ok(notes(dir).length > 64 * 1024);
  }
});
