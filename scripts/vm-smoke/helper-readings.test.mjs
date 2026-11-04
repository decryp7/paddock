import assert from 'node:assert/strict';
import { test } from 'node:test';
import { READINGS, readingStep, readingSteps } from './helper-readings.mjs';

const driver = (text, opens = true) => ({ openPage: async () => opens, text: async () => text });
const by = (id) => READINGS.find((r) => r.id === id);

test('a wired reading passes, an unwired one fails with the missing text', async () => {
  const good = await readingStep(by('processes-all-accounts'), 'windows', driver('Processes Account Name SYSTEM svchost Read by the administrator helper'));
  assert.equal(good.status, 'pass');
  const unwired = await readingStep(by('processes-all-accounts'), 'windows', driver('Processes Account Name SYSTEM svchost Needs the helper'));
  assert.equal(unwired.status, 'fail');
  assert.match(unwired.summary, /administrator helper/);
  assert.match(unwired.summary, /needs the helper/i);
});
test('a page that cannot be opened fails', async () => {
  const s = await readingStep(by('firewall'), 'macos', driver('', false));
  assert.equal(s.status, 'fail');
  assert.match(s.summary, /could not open firewall/);
});
test('every reading names expectations for each OS it lists', () => {
  for (const r of READINGS) for (const os of r.os) assert.ok((r.expect[os] ?? []).length > 0, `${r.id} has no expectation for ${os}`);
});
test('hooks are produced per OS', () => {
  assert.equal(readingSteps('windows', async () => driver('')).length, READINGS.filter((r) => r.os.includes('windows')).length);
  assert.ok(readingSteps('linux', async () => driver('')).length >= 4);
});
