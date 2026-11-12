import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseJobs } from '../src/util/workflows.mjs';
import { runnerClasses, unorderedPairs } from '../src/checks/C113-timing-jobs.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const wf = (jobs) => `name: x\non: push\njobs:\n${jobs}`;
const pairs = (jobs) => unorderedPairs(parseJobs(wf(jobs))).map((p) => `${p.job}~${p.heavy}`);

const SMOKE = (needs, extra = '') => `  smoke:
    # paddock-timing-job: macos — start-up budget
${extra}    needs: [${needs}]
    runs-on: \${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    steps:
      - run: echo
`;
const BUNDLE = `  bundle:
    runs-on: \${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    steps:
      - run: echo
`;

test('C113 bad: a timing job beside a heavy job on the same runner class', () => {
  assert.deepEqual(pairs(SMOKE('plan') + BUNDLE), ['smoke~bundle']);
});

test('C113 good: ordered by needs, in either direction and through a chain', () => {
  assert.deepEqual(pairs(SMOKE('plan, bundle') + BUNDLE), []);
  assert.deepEqual(pairs(SMOKE('plan') + BUNDLE.replace('  bundle:\n', '  bundle:\n    needs: [smoke]\n')), []);
  assert.deepEqual(pairs(SMOKE('plan, mid') + '  mid:\n    needs: [bundle]\n    runs-on: ubuntu-latest\n' + BUNDLE), []);
});

test('C113 good: another runner class, an unmarked job, a waiver and exclusive tag sides', () => {
  assert.deepEqual(pairs(SMOKE('plan') + BUNDLE.replace('[ubuntu-latest, macos-latest]', '[ubuntu-latest, windows-latest]')), [], 'the timing gate is macOS only');
  assert.deepEqual(pairs(SMOKE('plan').replace('    # paddock-timing-job: macos — start-up budget\n', '') + BUNDLE), []);
  assert.deepEqual(pairs(SMOKE('plan', '    # paddock-timing-overlap: Gitea 28.2 skips a job whose need was skipped\n') + BUNDLE), []);
  const branchOnly = SMOKE('plan').replace('    needs:', "    if: ${{ !startsWith(github.ref, 'refs/tags/') }}\n    needs:");
  const tagOnly = BUNDLE.replace('  bundle:\n', "  bundle:\n    if: ${{ startsWith(github.ref, 'refs/tags/x-v') }}\n");
  assert.deepEqual(pairs(branchOnly + tagOnly), [], 'never in the same run');
  const orManual = tagOnly.replace("x-v') }}", "x-v') || inputs.gates }}");
  assert.deepEqual(pairs(branchOnly + orManual), ['smoke~bundle'], 'a manual run has both');
});

test('C113: a comment after the last job belongs to the next job, and a reusable-workflow call uses every class', () => {
  const calls = `  gates:\n    uses: ./.gitea/workflows/ci.yml\n`;
  assert.deepEqual(pairs(SMOKE('plan') + '\n  # paddock-timing-job: any — a comment above the next job\n' + calls), ['smoke~gates']);
  assert.deepEqual([...runnerClasses('  gates:\n    uses: x.yml\n')].sort(), ['linux', 'macos', 'windows']);
  assert.deepEqual([...runnerClasses('  j:\n    runs-on: macos-latest\n')], ['macos']);
});

test('C113 through the runner: an error on the timing job, and a declaration without a reason', async () => {
  const bad = wf(SMOKE('plan') + BUNDLE);
  const f = await findingsOf('C113', makeApp({ '.gitea/workflows/a.yml': bad }));
  assert.deepEqual(f.map((x) => [x.file, x.severity]), [['.gitea/workflows/a.yml', 'error']]);
  assert.match(f[0].message, /smoke.*bundle/);
  const noReason = wf('  j:\n    # paddock-timing-job: macos\n    runs-on: macos-latest\n');
  const g = await findingsOf('C113', makeApp({ '.gitea/workflows/b.yml': noReason }));
  assert.match(g[0].message, /needs classes and a reason/);
  assert.deepEqual(await findingsOf('C113', makeApp({ '.gitea/workflows/c.yml': wf(SMOKE('plan, bundle') + BUNDLE) })), []);
});
