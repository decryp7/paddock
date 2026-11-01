import assert from 'node:assert/strict';
import { test } from 'node:test';
import { updaterInstallerProblems } from '../src/checks/C59-updater-installer.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const PITCREW = 'fn updater() -> UpdatesBuilder {\n    UpdatesBuilder::new(APP_NAME)\n        .public_key(option_env!("TAURI_UPDATER_PUBKEY"))\n}\n';
const DYNO = `${PITCREW}fn more(b: UpdatesBuilder, app: &AppHandle) -> UpdatesBuilder {\n    b.installer(Arc::new(paddock_updater::tauri::PluginInstaller::new(app.clone(), key)))\n}\n`;
const DEMO = 'fn u() -> UpdatesBuilder { UpdatesBuilder::new(APP_NAME).no_install() }\n';

test('updaterInstallerProblems: a builder with a key and no installer is the Pitcrew 1.1 trap, with its line', () => {
  const p = updaterInstallerProblems(PITCREW);
  assert.equal(p.length, 1);
  assert.equal(p[0].line, 2);
  assert.match(p[0].message, /no installer/);
});

test('updaterInstallerProblems: an installer (also on another line or statement) or no_install() passes', () => {
  assert.deepEqual(updaterInstallerProblems(DYNO), []);
  assert.deepEqual(updaterInstallerProblems(DEMO), []);
  assert.deepEqual(updaterInstallerProblems('UpdatesBuilder::new(N)\n    .installer(x)'), []);
  assert.deepEqual(updaterInstallerProblems('UpdatesBuilder::new(N)\n    . no_install ()'), []);
  assert.deepEqual(updaterInstallerProblems('fn f() { other_thing(); }'), [], 'no updater, nothing to say');
});

test('updaterInstallerProblems: comments, strings and test modules say nothing either way', () => {
  assert.deepEqual(updaterInstallerProblems('// UpdatesBuilder::new(N)\n'), []);
  assert.deepEqual(updaterInstallerProblems('/* UpdatesBuilder::new(N) */\n'), []);
  assert.deepEqual(updaterInstallerProblems('let s = "UpdatesBuilder::new(N)";\n'), []);
  assert.equal(updaterInstallerProblems(`${PITCREW}// .installer(x)\nlet s = ".no_install(";\n`).length, 1, 'a comment or string is no installer');
  assert.deepEqual(updaterInstallerProblems(`#[cfg(test)]\nmod t { fn f() { UpdatesBuilder::new(N); } }\n`), []);
  assert.equal(updaterInstallerProblems(`${PITCREW}#[cfg(test)]\nmod t { fn f() { x.no_install(); } }\n`).length, 1, 'a test installer does not count');
});

test('C59 bad: an app that wires the updater without an installer; good once fixed; tests and others skipped', async () => {
  const bad = await findingsOf(
    'C59',
    makeApp({
      'apps/a/app/src-tauri/src/lib.rs': PITCREW,
      'apps/b/app/src-tauri/src/lib.rs': DYNO,
      'apps/c/app/src-tauri/src/lib.rs': DEMO,
      'apps/a/app/src-tauri/tests/start.rs': PITCREW,
      'apps/d/app/src/other.rs': PITCREW,
    }),
  );
  assert.equal(bad.length, 1, messages(bad));
  assert.ok(JSON.stringify(bad[0]).includes('apps/a/app/src-tauri/src/lib.rs'), messages(bad));
  const good = await findingsOf('C59', makeApp({ 'apps/a/app/src-tauri/src/lib.rs': DYNO }));
  assert.deepEqual(good, []);
});
