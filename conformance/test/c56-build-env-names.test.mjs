import assert from 'node:assert/strict';
import { test } from 'node:test';
import { actionNames, buildInfoProblems } from '../src/checks/C56-build-env-names.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const YML = 'name: build-env\ndescription: "Sets PADDOCK_BUILD_COMMIT and PADDOCK_BUILD_DATE for the following steps."\n';
const NAMES = ['PADDOCK_BUILD_COMMIT', 'PADDOCK_BUILD_DATE'];
const GOOD = 'let b = x\n    .build_info(option_env!("PADDOCK_BUILD_COMMIT"), option_env!("PADDOCK_BUILD_DATE"));\n';
const DYNO = 'let b = x\n    .build_info(option_env!("DYNO_COMMIT"), option_env!("DYNO_BUILT"));\n';

test('actionNames reads the names from the action', () => {
  assert.deepEqual(actionNames(YML), NAMES);
});

test('buildInfoProblems: the shared names pass; others, swapped or none fail', () => {
  assert.deepEqual(buildInfoProblems(GOOD, NAMES), []);
  assert.deepEqual(buildInfoProblems('b.build_info(\n option_env!("PADDOCK_BUILD_COMMIT"),\n option_env!("PADDOCK_BUILD_DATE"),\n)', NAMES), []);
  assert.match(buildInfoProblems(DYNO, NAMES)[0], /DYNO_COMMIT and DYNO_BUILT/);
  assert.equal(buildInfoProblems('b.build_info(option_env!("PADDOCK_BUILD_DATE"), option_env!("PADDOCK_BUILD_COMMIT"))', NAMES).length, 1);
  assert.match(buildInfoProblems('b.build_info(None, None)', NAMES)[0], /no option_env/);
  assert.deepEqual(buildInfoProblems('// .build_info(option_env!("X"), option_env!("Y"))\n', NAMES), [], 'a comment is no call');
});

test('C56: an app with its own names is flagged, the shared names and non-callers pass', async () => {
  const bad = await findingsOf(
    'C56',
    makeApp({
      '.gitea/actions/build-env/action.yml': YML,
      'apps/a/app/src-tauri/src/lib.rs': DYNO,
      'apps/b/app/src-tauri/src/lib.rs': GOOD,
      'apps/c/app/src-tauri/src/lib.rs': 'fn run() {}\n',
    }),
  );
  assert.equal(bad.length, 1, messages(bad));
  assert.equal(bad[0].file, 'apps/a/app/src-tauri/src/lib.rs');
  assert.deepEqual(await findingsOf('C56', makeApp({ 'apps/c/src/lib.rs': 'fn run() {}\n' })), []);
  assert.equal((await findingsOf('C56', makeApp({ 'apps/a/src/lib.rs': GOOD }))).length, 1, 'no action to read the names from');
});
