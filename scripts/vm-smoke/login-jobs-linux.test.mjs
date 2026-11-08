import assert from 'node:assert/strict';
import { test } from 'node:test';
import { jobsPanelText, linuxLoginJobsDriver } from './login-jobs-drivers.mjs';

const BAR = '              landmark  Status bar\n                push button  1 running: Running benchmarks · Processor 5%\n                push button  2 unread\n';
const OPEN = `${BAR.split('push button  1')[0]}                push button  1 running: Running benchmarks · Processor 5%\n                landmark  Jobs\n                  heading  Jobs\n                  panel  Jobs\n                    heading  The jobs could not be read\n                    list item  Running benchmarks Benchmark | Processor 1 of 60\n                    push button  Cancel\nDone Start Dyno at login: done\n                push button  2 unread\n              landmark  Notifications\n`;

test('the jobs panel text is only what is inside the open Jobs panel, never the status bar button that also says "Running benchmarks"', () => {
  assert.equal(jobsPanelText(BAR), null, 'a closed panel is null');
  const t = jobsPanelText(OPEN);
  assert.match(t, /Running benchmarks/);
  assert.match(t, /Cancel/);
  assert.match(t, /Done Start Dyno at login: done/, 'the unindented continuation of a multi-line name is kept');
  assert.doesNotMatch(t, /Processor 5%/, 'the status bar button is outside the panel');
  assert.doesNotMatch(t, /unread/);
});
test('opening the Jobs list twice does not close it again (the status button toggles the panel)', async () => {
  let open = false, presses = 0;
  const atspi = (args) => {
    if (args.startsWith('tree')) return { code: 0, stdout: open ? OPEN : BAR };
    if (/^press dyno "\^Close\$"/.test(args)) return { code: 1, stdout: 'not found' };
    if (/running/.test(args) || /Jobs/.test(args)) { presses++; open = !open; return { code: 0, stdout: 'pressed' }; }
    return { code: 1, stdout: '' };
  };
  const d = linuxLoginJobsDriver(atspi, () => 'on');
  assert.equal(await d.openJobs(), true);
  assert.equal(await d.openJobs(), true);
  assert.equal(open, true, 'still open after the second call');
  assert.equal(presses, 1);
});

test('a switch that cannot be read because Settings was closed is read again after Settings is opened the way a person does', async () => {
  let settingsOpen = false; const calls = [];
  const atspi = (args) => {
    calls.push(args);
    if (/File/.test(args)) return { code: 0, stdout: 'pressed' };
    if (/Settings…/.test(args)) { settingsOpen = true; return { code: 0, stdout: 'pressed' }; }
    if (/\^Dyno\$/.test(args)) return { code: settingsOpen ? 0 : 1, stdout: '' };
    if (args.startsWith('state')) return settingsOpen ? { code: 0, stdout: 'states: checked\nchecked: yes' } : { code: 1, stdout: 'not found' };
    return { code: 1, stdout: '' };
  };
  const d = linuxLoginJobsDriver(atspi, (t) => (/checked: yes/.test(t) ? 'on' : 'off'), { settle: async () => {} });
  assert.equal(await d.readSwitch(), 'on');
  assert.ok(calls.some((c) => /Settings…/.test(c)), 'Settings was opened');
});
test('a switch that is readable is read in place, Settings is not reopened', async () => {
  const calls = [];
  const atspi = (args) => { calls.push(args); return args.startsWith('state') ? { code: 0, stdout: 'checked: no' } : { code: 0, stdout: '' }; };
  const d = linuxLoginJobsDriver(atspi, () => 'off', { settle: async () => {} });
  assert.equal(await d.readSwitch(), 'off');
  assert.equal(calls.length, 1);
});
