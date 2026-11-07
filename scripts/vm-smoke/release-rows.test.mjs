import assert from 'node:assert/strict';
import { test } from 'node:test';
import { btmEnabled, judgeKeychainOtherBuild, keychainOtherBuildStep, NOT_DRIVEN, ROW_TITLES, judgeLifecycleLog, judgeLinuxHelperLogDir, judgeMacReopen, notDriven, rowStep } from './release-rows.mjs';
import { deriveVerdict } from './verdict.mjs';

const bad = (c) => c.filter((x) => !x.ok);
test('dead state: an unreadable, denied or empty lifecycle log does not pass', () => {
  assert.ok(bad(judgeLifecycleLog({ code: 1, out: 'Permission denied' })).length);
  assert.ok(bad(judgeLifecycleLog({ code: 0, out: '' })).length);
  assert.equal(judgeLifecycleLog({ code: null, out: '' })[0].kind, 'check');
  assert.equal(bad(judgeLifecycleLog({ code: 0, out: '2026 helper started\n' })).length, 0);
});
test('dead state: a gone or empty /var/log/dyno-helper after the uninstall fails', () => {
  assert.ok(bad(judgeLinuxHelperLogDir({ code: 2, out: '' })).length);
  assert.ok(bad(judgeLinuxHelperLogDir({ code: 0, out: '' })).length);
  assert.equal(bad(judgeLinuxHelperLogDir({ code: 0, out: 'helper.log\n' })).length, 0);
});
test('ER-33: the Linux log row without its precondition (helper on, a log before the removal) is a broken check, not a pass or a product fail', () => {
  const off = judgeLinuxHelperLogDir({ code: 0, out: 'lifecycle.log\n', helperActive: false, logBefore: 'lifecycle.log' });
  assert.ok(off.some((c) => !c.ok && c.kind === 'check'));
  const empty = judgeLinuxHelperLogDir({ code: 2, out: '', helperActive: true, logBefore: '' });
  assert.ok(empty.every((c) => c.ok || c.kind === 'check'), 'a missing folder before the removal is not blamed on the product');
  assert.equal(bad(judgeLinuxHelperLogDir({ code: 0, out: 'lifecycle.log\n', helperActive: true, logBefore: 'lifecycle.log' })).length, 0);
});
test('macOS reopen: no agent fails; the notification is information; a switched-on item fails; an untested off state is a broken check', () => {
  const base = { agentBack: true, notificationSeen: false, loginItemWasOff: true, loginItemNowOff: true };
  assert.equal(bad(judgeMacReopen(base)).length, 0);
  assert.equal(judgeMacReopen(base)[1].info, true);
  assert.ok(bad(judgeMacReopen({ ...base, agentBack: false })).length);
  assert.ok(bad(judgeMacReopen({ ...base, loginItemNowOff: false })).length);
  assert.equal(judgeMacReopen({ ...base, loginItemWasOff: false })[2].kind, 'check');
  assert.equal(judgeMacReopen({ ...base, loginItemNowOff: null })[2].kind, 'check');
});
test('a row that is not driven is NOT MEASURED with its reason and keeps the run INCOMPLETE', () => {
  const s = notDriven(ROW_TITLES.secondUser, NOT_DRIVEN.secondUser);
  assert.equal(s.status, 'not-measured');
  assert.match(s.summary, /second console logon/);
  const v = deriveVerdict({ steps: [s], required: [{ id: 'second-user', match: /second user/ }] });
  assert.equal(v.verdict, 'INCOMPLETE');
  assert.equal(rowStep('x', [{ ok: true, detail: 'a' }], 'p').status, 'pass');
  for (const k of Object.keys(NOT_DRIVEN)) assert.ok(ROW_TITLES[k], k);
});

test('btmEnabled reads the Dyno entry of sfltool dumpbtm; no entry is null', () => {
  assert.equal(btmEnabled(' #1:\n   Name: Dyno\n   Identifier: net.decryptology.dyno.agent\n   Disposition: [enabled, allowed, visible, notified]\n\n #2:\n   Name: Other\n   Disposition: [disabled]'), true);
  assert.equal(btmEnabled(' #1:\n   Name: Dyno\n   Disposition: [disabled, allowed, visible, notified]'), false);
  assert.equal(btmEnabled(' #1:\n   Name: Other\n   Disposition: [enabled]'), null);
  assert.equal(btmEnabled(''), null);
});

test('keychain other-build measurement: records the delay or the missing window, is never a pass or a fail, and is not in any manifest', async () => {
  const blocked = judgeKeychainOtherBuild({ windowSeen: false, windowMs: null, sheetSeen: true, guardMs: 45_000 });
  assert.equal(blocked.status, 'not-measured');
  assert.match(blocked.summary, /no window within the 45 s guard.*sheet WAS on screen/);
  const fine = judgeKeychainOtherBuild({ windowSeen: true, windowMs: 3000, sheetSeen: false, guardMs: 45_000 });
  assert.equal(fine.status, 'not-measured', 'even a good number is not a pass until the product fix lands and an owner names the limit');
  assert.match(fine.summary, /window appeared after 3\.0 s/);
  assert.equal(judgeKeychainOtherBuild({ windowSeen: null }).checks[0].kind, 'check');
  const { readFileSync } = await import('node:fs');
  assert.ok(!/keychainOtherBuild|Measurement \(not a release row/.test(readFileSync(new URL('./manifest.mjs', import.meta.url), 'utf8')), 'not required by any run');
  assert.equal(deriveVerdict({ steps: [blocked], required: [] }).failed.length, 0);
});
test('keychain other-build driver: control first, then build B; a failed control or a throwing primitive is a broken check, not a result about B', async () => {
  let t = 0; const log = []; let bInstalled = false;
  const p = { now: () => t, sleep: async (ms) => { t += ms; }, createKeyInBuildA: async () => log.push('A-key'), reopenSameBuild: async () => { log.push('A-again'); t = 0; },
    installBuildB: async () => { log.push('B'); bInstalled = true; }, openApp: async () => { log.push('open-B'); t = 0; },
    windowVisible: async () => (bInstalled ? false : t >= 4000), sheetVisible: async () => true };
  const s = await keychainOtherBuildStep(p, 10_000);
  assert.deepEqual(log, ['A-key', 'A-again', 'B', 'open-B']);
  assert.equal(s.status, 'not-measured');
  assert.match(s.summary, /no window within the 10 s guard.*sheet WAS on screen.*positive control.*after 4\.0 s/);
  const noControl = await keychainOtherBuildStep({ ...p, windowVisible: async () => false }, 5000);
  assert.equal(noControl.checks[0].kind, 'check');
  assert.match(noControl.summary, /positive control failed/);
  assert.ok(!log.includes('x'));
  bInstalled = false; t = 0;
  const fixed = await keychainOtherBuildStep({ ...p, windowVisible: async () => true }, 5000);
  assert.match(fixed.summary, /window appeared after 0\.0 s/);
  bInstalled = false; t = 0;
  const broken = await keychainOtherBuildStep({ ...p, installBuildB: async () => { throw new Error('dmg missing'); } });
  assert.equal(broken.checks[0].kind, 'check');
  assert.match(broken.summary, /dmg missing/);
});

test('keychain row required mode: a window after the update passes, none fails, the control still comes first; the flag is off and the manifest ignores the row until then', async () => {
  const { KEYCHAIN_ROW_REQUIRED } = await import('./release-rows.mjs');
  assert.equal(KEYCHAIN_ROW_REQUIRED, false, 'flip it only when fix/dyno-mcp-keys-file has merged');
  let t = 0, bInstalled = false;
  const mk = (bWindow) => ({ now: () => t, sleep: async (ms) => { t += ms; }, createKeyInBuildA: async () => {}, reopenSameBuild: async () => { t = 0; }, installBuildB: async () => { bInstalled = true; t = 0; },
    openApp: async () => { t = 0; }, windowVisible: async () => (bInstalled ? bWindow && t >= 3000 : t >= 2000), sheetVisible: async () => !bWindow });
  const ok = await keychainOtherBuildStep(mk(true), 10_000, true);
  assert.equal(ok.status, 'pass'); assert.match(ok.title, /^Release row: an update keeps the app openable/);
  bInstalled = false;
  const bad = await keychainOtherBuildStep(mk(false), 10_000, true);
  assert.equal(bad.status, 'fail'); assert.match(bad.summary, /no window within 10 s of the update.*sheet WAS on screen/);
  bInstalled = false;
  const noControl = await keychainOtherBuildStep({ ...mk(true), windowVisible: async () => false }, 3000, true);
  assert.equal(noControl.status, 'not-measured', 'a failed positive control is never a fail of the build');
  assert.equal(noControl.cls, 'check');
});
