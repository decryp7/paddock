import assert from 'node:assert/strict';
import { test } from 'node:test';
import { appWindows, btmItem, judgeLoginItems, judgeLoginStart, parseBtmDump, parseLoginItems, parseQuarantine, parseTree, parseWindows } from './gui-judge.mjs';

// Shapes captured from the GUI test image (macOS 26.6.2, 2026-10-30/31); names and ids invented where they were not Paddock's.
const IMAGE_ROWS = ['Cirrus Labs, Inc.', 'launchctl'];
const loginItemsTree = ({ open = [], dynoOn = 1, legacy = false } = {}) => [
  '      AXHeading  Open at Login',
  '      AXStaticText  These items will open automatically when you log in.',
  '      AXGroup  ',
  '        AXScrollArea  ',
  '          AXOutline  ',
  '            AXColumn  6B08891F',
  ...open.flatMap((n) => ['            AXRow  ', '              AXCell  ', `                AXStaticText  ${n}`, '              AXCell  ', '                AXStaticText  Application']),
  '            AXGroup  ',
  '              AXButton  Item',
  '              AXButton  Kind',
  '        AXButton  Add',
  '      AXHeading  App Background Activity',
  '      AXStaticText  Apps run in the background after you close them.',
  '      AXGroup  ',
  '        AXStaticText  Cirrus Labs, Inc.',
  '        AXStaticText  2 items: 1 item affects all users | background-detail-for-Cirrus Labs, Inc.-app-login-items',
  '        AXCheckBox  background-switch-Cirrus Labs, Inc. | value=1',
  '        AXStaticText  Dyno',
  '        AXStaticText  1 item | background-detail-for-Dyno-app-login-items',
  `        AXCheckBox  background-switch-Dyno | value=${dynoOn}`,
  ...(legacy ? ['        AXStaticText  dyno-app', '        AXStaticText  Item from unidentified developer. | background-detail-for-dyno-app-app-login-items',
    '        AXCheckBox  Show in Finder | background-switch-dyno-app | value=1', '        AXCheckBox  background-switch-dyno-app | value=1'] : []),
  '        AXStaticText  launchctl',
  '        AXStaticText  Item from unidentified developer. | background-detail-for-launchctl-app-login-items',
  '        AXCheckBox  Show in Finder | background-switch-launchctl | value=1',
  '        AXCheckBox  background-switch-launchctl | value=1',
  '      AXHeading  Extensions',
  '        AXStaticText  Books',
].join('\n');

test('parseQuarantine reads the flags and the approved bit, and refuses other text', () => {
  assert.deepEqual(parseQuarantine('0183;6ae4c31a;Safari;CBD8A3DC-BEA3-4907-A2CC-7117337A7767\n'), { flags: 0x183, agent: 'Safari', approved: false, raw: '0183;6ae4c31a;Safari;CBD8A3DC-BEA3-4907-A2CC-7117337A7767' });
  assert.equal(parseQuarantine('01c3;6ae4c31a;Safari;x').approved, true);
  assert.equal(parseQuarantine('xattr: /Applications/Dyno.app: No such xattr'), null);
  assert.equal(parseQuarantine(''), null);
});

test('parseWindows and appWindows keep the app\'s real windows, not menu-bar items', () => {
  const w = parseWindows('Dyno\t1066\t0\tDyno\t0,30,1024x680\nControl Center\t461\t25\tClock\t880,0,144x30\nDyno\t1066\t0\t\t900,0,30x22\nbad line');
  assert.equal(w.length, 3);
  assert.deepEqual(appWindows(w, 'Dyno').map((x) => [x.title, x.w, x.h]), [['Dyno', 1024, 680]]);
});

test('parseTree reads depth, role and label', () => {
  assert.deepEqual(parseTree('AXApplication  Dyno\n  AXWindow  Dyno | x\n    AXButton  \n'), [
    { depth: 0, role: 'AXApplication', label: 'Dyno' }, { depth: 1, role: 'AXWindow', label: 'Dyno | x' }, { depth: 2, role: 'AXButton', label: '' }]);
});

test('parseLoginItems reads Open at Login rows and background rows with their switches', () => {
  const r = parseLoginItems(loginItemsTree({ open: ['Dyno'], legacy: true }));
  assert.deepEqual(r.openAtLogin, ['Dyno']);
  assert.deepEqual(r.background.map((b) => [b.name, b.on]), [['Cirrus Labs, Inc.', true], ['Dyno', true], ['dyno-app', true], ['launchctl', true]]);
  assert.equal(r.background.find((b) => b.name === 'dyno-app').detail, 'Item from unidentified developer.');
});

test('judgeLoginItems passes exactly the two Dyno rows and fails a legacy row, a missing or switched-off row', () => {
  const want = { openAtLogin: ['Dyno'], background: ['Dyno'] };
  assert.ok(judgeLoginItems(parseLoginItems(loginItemsTree({ open: ['Dyno'] })), want, IMAGE_ROWS).ok);
  const legacy = judgeLoginItems(parseLoginItems(loginItemsTree({ open: ['Dyno'], legacy: true })), want, IMAGE_ROWS);
  assert.equal(legacy.ok, false);
  assert.match(legacy.checks.find((c) => !c.ok).detail, /unexpected background row.*dyno-app.*unidentified/);
  assert.equal(judgeLoginItems(parseLoginItems(loginItemsTree({ open: [] })), want, IMAGE_ROWS).ok, false);
  assert.equal(judgeLoginItems(parseLoginItems(loginItemsTree({ open: ['Dyno'], dynoOn: 0 })), want, IMAGE_ROWS).ok, false);
  assert.equal(judgeLoginItems(parseLoginItems(loginItemsTree({ open: ['Dyno', 'Dyno'] })), want, IMAGE_ROWS).ok, false);
});

test('parseBtmDump and btmItem find the agent by its launchd label with its disposition', () => {
  const dump = [
    '========================', ' Records for UID 501 : 1AA2', '========================', '', ' ServiceManagement Records:', '',
    ' #1:', '                 UUID: 1', '                 Name: Dyno', '       Developer Name: (null)', '                 Type: app (0x2)',
    '          Disposition: [enabled, allowed, notified] (0xb)', '           Identifier: 2.net.decryptology.dyno', '                  URL: file:///Applications/Dyno.app/',
    ' #2:', '                 UUID: 2', '                 Name: dyno-agent', '                 Type: agent (0x8)',
    '          Disposition: [enabled, disallowed, notified] (0x9)', '           Identifier: 8.net.decryptology.dyno.agent',
    '                  URL: Contents/Library/LaunchAgents/net.decryptology.dyno.agent.plist', '      Executable Path: Contents/MacOS/dyno-agent',
    '    Parent Identifier: 2.net.decryptology.dyno',
  ].join('\n');
  const items = parseBtmDump(dump);
  assert.equal(items.length, 2);
  const a = btmItem(items, 'net.decryptology.dyno.agent');
  assert.deepEqual([a.name, a.enabled, a.allowed, a.notified, a.parent], ['dyno-agent', true, false, true, '2.net.decryptology.dyno']);
  assert.equal(btmItem(items, 'net.decryptology.dyno').type, 'app (0x2)');
  assert.equal(btmItem(items, 'no.such.label'), null);
});

test('judgeLoginStart wants the hidden log line, no window line and no window after the login', () => {
  const cfg = { hidden: 'started hidden \\(flag or login item\\)', shown: 'started with a window', windowOwner: 'Dyno' };
  const none = parseWindows('Dock\t459\t20\tDock\t0,0,1024x768');
  assert.ok(judgeLoginStart('22:36:35 start\n22:36:36 started hidden (flag or login item)\n', none, cfg).ok);
  const shown = judgeLoginStart('22:36:36 started with a window\n', parseWindows('Dyno\t434\t0\tDyno\t0,30,1024x680'), cfg);
  assert.deepEqual(shown.checks.map((c) => c.ok), [false, false, false]);
  assert.equal(judgeLoginStart('', none, cfg).ok, false, 'no decision logged is not a pass');
});
