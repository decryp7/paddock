import assert from 'node:assert/strict';
import { join } from 'node:path';
import { test } from 'node:test';
import { buildEnvNames, buildInfoProblems } from '../src/checks/C56-build-env-names.mjs';
import { runChecks } from '../src/runner.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const NAMES = ['PADDOCK_BUILD_COMMIT', 'PADDOCK_BUILD_DATE'];
const GOOD = 'let b = x\n    .build_info(option_env!("PADDOCK_BUILD_COMMIT"), option_env!("PADDOCK_BUILD_DATE"));\n';
const DYNO = 'fn a() {}\nlet b = x\n    .build_info(option_env!("DYNO_COMMIT"), option_env!("DYNO_BUILT"));\n';
const INFO_GOOD = 'AppInfo::new(ID, NAME, V, option_env!("PADDOCK_BUILD_COMMIT").unwrap_or(""))';
const INFO_BAD = 'fn a() {}\nAppInfo::new(\n  ID,\n  NAME,\n  V,\n  option_env!("SHELL_DEMO_COMMIT").unwrap_or(""),\n)';

test('the names come from the code that sets them', () => {
  assert.deepEqual(buildEnvNames(), NAMES);
});

test('buildInfoProblems: the shared names pass; others, swapped or none fail, with the line', () => {
  assert.deepEqual(buildInfoProblems(GOOD), []);
  assert.deepEqual(buildInfoProblems('b.build_info(\n option_env!("PADDOCK_BUILD_COMMIT"),\n option_env!("PADDOCK_BUILD_DATE"),\n)'), []);
  const bad = buildInfoProblems(DYNO);
  assert.match(bad[0].message, /DYNO_COMMIT and DYNO_BUILT/);
  assert.equal(bad[0].line, 3);
  assert.equal(buildInfoProblems('b.build_info(option_env!("PADDOCK_BUILD_DATE"), option_env!("PADDOCK_BUILD_COMMIT"))').length, 1);
  assert.match(buildInfoProblems('b.build_info(None, None)')[0].message, /no option_env/);
});

test('buildInfoProblems: AppInfo::new may read only the commit name', () => {
  assert.deepEqual(buildInfoProblems(INFO_GOOD), []);
  assert.equal(buildInfoProblems('AppInfo::new("a", "A", "1", "")').length, 1, 'no option_env! at all');
  assert.equal(buildInfoProblems('AppInfo::new(ID, N, V, option_env!("PADDOCK_BUILD_COMMIT").or(option_env!("PADDOCK_BUILD_COMMIT")))').length, 1, 'two reads');
  const bad = buildInfoProblems(INFO_BAD);
  assert.equal(bad.length, 1);
  assert.equal(bad[0].line, 2);
  assert.match(bad[0].message, /SHELL_DEMO_COMMIT/);
});

test('buildInfoProblems: comments, strings and test modules are not looked at', () => {
  assert.deepEqual(buildInfoProblems('// .build_info(option_env!("X"), option_env!("Y"))\n'), []);
  assert.deepEqual(buildInfoProblems('/* .build_info(option_env!("X"), option_env!("Y")) */\n'), []);
  assert.deepEqual(buildInfoProblems('let s = "// .build_info(a)";\n' + GOOD), [], 'a // inside a string opens no comment');
  assert.deepEqual(buildInfoProblems(`${GOOD}#[cfg(test)]\nmod t { fn f() { x.build_info(None, None); } }\n`), []);
});

test('buildInfoProblems: a leading #[cfg(test)] use hides nothing below it', () => {
  assert.equal(buildInfoProblems(`#[cfg(test)]\nuse x::y;\n${DYNO}`).length, 1);
  assert.equal(buildInfoProblems(`#[cfg(test)]\nuse x::{y, z};\n${DYNO}`).length, 1);
  assert.deepEqual(buildInfoProblems(`${GOOD}#[cfg(test)]\nmod t { fn f() { if a { x.build_info(None, None); } } }\n`), []);
});

test('C56 from the repo root: an app with its own names is flagged; tests, shared names and non-callers pass', async () => {
  const bad = await findingsOf(
    'C56',
    makeApp({
      'apps/a/app/src-tauri/src/lib.rs': DYNO,
      'apps/b/app/src-tauri/src/lib.rs': GOOD,
      'apps/c/app/src-tauri/src/lib.rs': 'fn run() {}\n',
      'apps/d/app/src-tauri/tests/it.rs': DYNO,
      'apps/e/src-tauri/src/lib.rs': INFO_BAD,
    }),
  );
  assert.equal(bad.length, 2, messages(bad));
  assert.deepEqual(bad.map((f) => f.file).sort(), ['apps/a/app/src-tauri/src/lib.rs', 'apps/e/src-tauri/src/lib.rs']);
});

test('C56 per app (--root apps/<app>): the old Dyno lib.rs fails, the fixed one passes', async () => {
  const run = async (lib) => (await runChecks({ root: makeApp({ 'app/src-tauri/src/lib.rs': lib }), app: 'dyno', only: ['C56'], options: { paddock: join(import.meta.dirname, '..', '..') } })).findings;
  const old = await run('.build_info(option_env!("DYNO_COMMIT"), option_env!("DYNO_BUILT"));\n');
  assert.equal(old.length, 1, messages(old));
  assert.equal(old[0].file, 'app/src-tauri/src/lib.rs');
  assert.deepEqual(await run(GOOD), []);
});
