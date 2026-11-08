import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ownerHasProbe, probeLines, hasOtherUsersTotal } from './win-scoping.mjs';

test('the owner control accepts the probe in the snapshot when top lists only six groups', () => {
  assert.equal(ownerHasProbe('[{"name":"System"}]', '{"processes":[{"name":"owner-secret-probe.exe"}]}'), true);
  assert.equal(ownerHasProbe('[{"name":"System"}]', '{"processes":[]}'), false);
});
test('probe lines give a window of the hit', () => assert.match(probeLines('{"a":1,"name":"Owner-Secret-Probe.exe","b":2}')[0], /Owner-Secret-Probe/));
test('an other-users total is the JSON key or the words, not any text with "other"', () => {
  assert.equal(hasOtherUsersTotal('{"otherUsers":{"processes":40}}'), true);
  assert.equal(hasOtherUsersTotal('{"other_users": {"processes":40}}'), true);
  assert.equal(hasOtherUsersTotal('{"name":"another"}'), false);
});
