import assert from 'node:assert/strict';
import { test } from 'node:test';
import { judgeCard, judgeLoginSwitch, judgeTrayNote, parseSwitchState } from './linux-judges.mjs';

const bad = (c) => c.filter((x) => !x.ok);
test('the switch state is read from the AT-SPI state output', () => {
  assert.deepEqual(['states: checked\nchecked: yes', 'checked: no', 'not found', ''].map(parseSwitchState), ['on', 'off', 'unknown', 'unknown']);
});
test('dead state: the default-ON switch is never flipped OFF by a blind press', () => {
  assert.ok(bad(judgeLoginSwitch({ before: 'on', after: 'off', pressed: true })).length, 'a press on an ON switch turned it off');
  assert.equal(bad(judgeLoginSwitch({ before: 'on', after: 'on', pressed: false })).length, 0);
  assert.equal(bad(judgeLoginSwitch({ before: 'off', after: 'on', pressed: true })).length, 0);
  assert.ok(bad(judgeLoginSwitch({ before: 'off', after: 'off', pressed: true })).length);
  assert.equal(judgeLoginSwitch({ before: 'unknown', after: 'unknown', pressed: false })[0].kind, 'check');
});
test('dead state: an empty card read does not pass (the old OR passed it)', () => {
  assert.equal(judgeCard('').ok, false);
  assert.equal(judgeCard('').kind, 'check');
  assert.equal(judgeCard('Turn on administrator readings').ok, false);
  assert.equal(judgeCard('Turn on administrator readings').kind, undefined);
  assert.equal(judgeCard('Administrator readings On\nTurn off administrator readings').ok, true);
  assert.equal(judgeCard('something else').kind, 'check');
});
test('dead state: with the host present, a Settings text without the Menu bar card proves nothing about the note', () => {
  assert.equal(judgeTrayNote('Dyno Settings General Theme', false)[0].kind, 'check');
  assert.equal(judgeTrayNote('', false)[0].ok, false);
  assert.equal(judgeTrayNote('Menu bar Show icons for CPU Memory', false)[0].ok, true);
  assert.equal(judgeTrayNote("Menu bar Your desktop isn't showing tray icons", false)[0].ok, false);
  assert.equal(judgeTrayNote("Menu bar Your desktop isn't showing tray icons", true)[0].ok, true);
  assert.equal(judgeTrayNote('Menu bar Show icons', true)[0].ok, false);
  assert.equal(judgeTrayNote('Dyno Settings General', true)[0].kind, 'check');
});
