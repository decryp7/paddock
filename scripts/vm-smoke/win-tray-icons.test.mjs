import assert from 'node:assert/strict';
import { test } from 'node:test';
import { dynoTrayIcons, metricsFromLog, trayLogLines } from './win-tray-icons.mjs';

const log = [
  '{"@m":"Dyno 1.1.0 starting (OS Windows 11, x86_64)","@t":"2026-01-01T00:00:00Z"}',
  '{"@m":"tray icon dyno-number created (metric CPU, GUID 0000: Accepted; on Windows 11 a new icon may sit in the overflow)","@t":"2026-01-01T00:00:01Z"}',
  '{"@m":"tray icon dyno-number-2 created (metric Memory, GUID 0001: Accepted; on Windows 11)","@t":"2026-01-01T00:00:01Z"}',
  '{"@m":"tray icon dyno-number created (metric CPU, GUID 0000: Accepted; on Windows 11)","@t":"2026-01-01T00:00:03Z"}',
  '{"@m":"live update #1: cpu 3%","@t":"2026-01-01T00:00:04Z"}',
].join('\n');

// What the actor returns with Dyno's Settings window open and its icons in the overflow (the shape of the 7538 restart check).
const rows = [
  'Dyno\t500,20\tButton\tTauri Window\tdyno-app',
  'Start Dyno at login\t600,400\tButton\tTauri Window\tdyno-app',
  'CPU 4%\t900,700\tSystemTray.NormalButton\tTopLevelWindowForOverflowXamlIsland\texplorer',
  'Memory: the agent is not running\t940,700\tSystemTray.NormalButton\tShell_TrayWnd\texplorer',
  'CPU usage widget\t100,700\tButton\tShell_TrayWnd\texplorer',
  'Show Hidden Icons\t860,750\tSystemTray.ChevronButton\tShell_TrayWnd\texplorer',
  'Memory 61%\t300,300\tButton\tChrome_WidgetWin_1\tmsedge',
].join('\n');

test('metrics come from the "tray icon ... created" lines, once each', () => {
  assert.deepEqual(metricsFromLog(log), ['CPU', 'Memory']);
  assert.deepEqual(metricsFromLog(''), []);
});

test("Dyno's own window buttons are never counted as tray icons", () => {
  const icons = dynoTrayIcons(rows, metricsFromLog(log));
  assert.deepEqual(icons.map((i) => [i.name, i.where]), [['CPU 4%', 'overflow'], ['Memory: the agent is not running', 'taskbar']]);
  assert.ok(!icons.some((i) => /Dyno/.test(i.name)));
});

test('with Dyno running but no window and no icons, the count is 0 (the after-restart case)', () => {
  const none = rows.split('\n').filter((l) => !/explorer$/.test(l) || /Show Hidden|widget/.test(l)).join('\n');
  assert.equal(dynoTrayIcons(none, ['CPU', 'Memory']).length, 0);
});

test('no metric logged means no icon can be claimed', () => {
  assert.equal(dynoTrayIcons(rows, []).length, 0);
});

test('a metric name is matched as a whole word', () => {
  assert.equal(dynoTrayIcons('CPUs idle\t1,1\tB\tShell_TrayWnd\texplorer', ['CPU']).length, 0);
});

test('the tray log lines keep the start and every icon, not live updates', () => {
  const l = trayLogLines(log);
  assert.equal(l.length, 4);
  assert.ok(l.every((x) => !/live update/.test(x)));
});
