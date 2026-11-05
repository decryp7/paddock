import assert from 'node:assert/strict';
import { test } from 'node:test';
import { btmEnabled, NOT_DRIVEN, ROW_TITLES, judgeLifecycleLog, judgeLinuxHelperLogDir, judgeMacReopen, notDriven, rowStep } from './release-rows.mjs';
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
