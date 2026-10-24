import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findingsOf, makeApp, messages, TAURI_CONF } from './helpers.mjs';

const base = (v = '1.2.3', pkg = v) => ({
  'app/src-tauri/tauri.conf.json': TAURI_CONF(v),
  'app/package.json': JSON.stringify({ version: pkg }),
  'CHANGELOG.md': `# Changelog\n\n## [${v}] - 2026-10-01\n- x\n`,
});
const wf = (releaseNeeds) => `jobs:\n  test:\n    runs-on: x\n  bundle:\n    needs: [test]\n  release:\n    needs: ${releaseNeeds}\n    steps:\n      - run: node scripts/release/publish.mjs\n`;
const W = '.gitea/workflows/build.yml';

test('C07 good: versions agree, release needs every gate', async () => {
  assert.deepEqual(await findingsOf('C07', makeApp({ ...base(), [W]: wf('[test, bundle]') })), []);
  assert.deepEqual(await findingsOf('C07', makeApp({ ...base(), [W]: wf('\n      - bundle') })), []);
});

test('C07 bad: the CHANGELOG head and package.json disagree with tauri.conf.json', async () => {
  const files = { ...base('1.2.3', '1.2.2'), 'CHANGELOG.md': '## [1.2.4] - x\n', [W]: wf('[test, bundle]') };
  assert.match(messages(await findingsOf('C07', makeApp(files))), /the version differs: .*package\.json=1\.2\.2.*CHANGELOG\.md=1\.2\.4/);
});

test('C07 a 0.0.0 package.json placeholder is not a version', async () => {
  assert.deepEqual(await findingsOf('C07', makeApp({ ...base('1.2.3', '0.0.0'), [W]: wf('[test, bundle]') })), []);
});

test('C07 bad: the release job skips a gate; nonGates waives it by config', async () => {
  const files = { ...base(), [W]: wf('[test]') };
  assert.match(messages(await findingsOf('C07', makeApp(files))), /does not need: bundle/);
  const f = await findingsOf('C07', makeApp({ ...files, 'paddock.conformance.json': JSON.stringify({ nonGates: ['bundle'] }) }));
  assert.deepEqual(f, []);
});

function fakeGit({ skipCi = false, missing = [], tagExists = true } = {}) {
  return (args) => {
    const a = args.join(' ');
    if (a.startsWith('rev-parse')) {
      if (!tagExists) throw new Error('unknown revision');
      return 'abc1234567890\n';
    }
    if (a.startsWith('log -1')) return skipCi ? 'chore: x [skip ci]\n' : 'chore: release\n';
    if (a.startsWith('describe')) return 'v1.2.2\n';
    if (a.startsWith('for-each-ref')) return missing.length ? 'fix/store-worker\n' : '';
    if (a.startsWith('rev-list')) return missing.join('\n');
    throw new Error(`unexpected git ${a}`);
  };
}
const fetchOf = (runs, ok = true) => async () => ({ ok, status: ok ? 200 : 500, json: async () => ({ workflow_runs: runs }) });
const pre = (git, runs, extra = {}) => findingsOf('C07', makeApp({ ...base(), [W]: wf('[test, bundle]') }), { options: { preflight: 'v1.2.3', git, fetch: fetchOf(runs), api: 'https://h/api/v1', repo: 'o/r', ...extra } });

test('C07 preflight good: green run for this commit, nothing missing', async () => {
  assert.deepEqual(await pre(fakeGit(), [{ name: 'build', conclusion: 'success', head_sha: 'abc1234567890' }]), []);
});

test('C07 preflight bad: [skip ci] tag, red run, unmerged fix branch, wrong version', async () => {
  const f = await pre(fakeGit({ skipCi: true, missing: ['deadbeef0001', 'deadbeef0002'] }), [{ name: 'build', conclusion: 'failure', head_sha: 'abc1234567890' }], {});
  const m = messages(f);
  assert.match(m, /is \[skip ci\]/);
  assert.match(m, /run "build".* is failure/);
  assert.match(m, /fix\/store-worker has 2 commit/);
  const wrong = await findingsOf('C07', makeApp({ ...base(), [W]: wf('[test, bundle]') }), { options: { preflight: 'v1.2.4', git: fakeGit(), fetch: fetchOf([{ conclusion: 'success' }]), api: 'https://h/api/v1', repo: 'o/r' } });
  assert.match(messages(wrong), /says 1\.2\.3, the tag says 1\.2\.4/);
});

test('C07 preflight fails closed: no run listed, API error, missing tag', async () => {
  assert.match(messages(await pre(fakeGit(), [])), /lists no run for commit/);
  const err = await findingsOf('C07', makeApp({ ...base(), [W]: wf('[test, bundle]') }), { options: { preflight: 'v1.2.3', git: fakeGit(), fetch: fetchOf([], false), api: 'https://h/api/v1', repo: 'o/r' } });
  assert.match(messages(err), /cannot read the CI result/);
  assert.match(messages(await pre(fakeGit({ tagExists: false }), [])), /does not exist in this checkout/);
});
