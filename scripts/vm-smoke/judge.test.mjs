import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { describeTree, distinctCounts, etimeSeconds, instancesOf, judgeSnapshot, judgeStateFile, judgeWindow, orphansAfterQuit, parseLaunchctlPrint, parsePs, peakCounts, renderResult, supervisedSurvivors } from './judge.mjs';

const BUNDLE = '/Applications/Dyno.app';
const config = {
  processes: [
    { name: 'dyno-app', role: 'app', max: 1 },
    { name: 'dyno-agent', role: 'helper', max: 1, argsPattern: '^ run( |$)', parents: ['app', 'launchd'] },
  ],
};
const ps = (rows) => parsePs(rows.map(([pid, ppid, etime, cmd]) => `${String(pid).padStart(5)} ${String(ppid).padStart(5)} ${etime.padStart(11)} ${cmd}`).join('\n'));
const APP = `${BUNDLE}/Contents/MacOS/dyno-app`;
const AGENT = `${BUNDLE}/Contents/MacOS/dyno-agent`;

test('parsePs reads pid, parent, age and command; etime has three shapes', () => {
  const p = parsePs(`  1360  1355       01:05 ${AGENT} run\n    1     0 2-03:04:05 /sbin/launchd\nnot a line\n`);
  assert.deepEqual(p.map((x) => [x.pid, x.ppid, x.ageSec]), [[1360, 1355, 65], [1, 0, 183845]]);
  assert.equal(etimeSeconds('01:02:03'), 3723);
  assert.ok(Number.isNaN(etimeSeconds('soon')));
});

test('instancesOf matches the installed bundle executable and its args, not a CLI call, a copy elsewhere or a look-alike', () => {
  const p = ps([
    [10, 1, '00:30', APP],
    [11, 10, '00:29', `${AGENT} run`],
    [12, 10, '00:05', `${AGENT} service status`],
    [13, 1, '00:05', `/Users/x/Desktop/Dyno.app/Contents/MacOS/dyno-agent run`],
    [14, 1, '00:05', `${BUNDLE}/Contents/MacOS/dyno-agent-helper run`],
    [15, 1, '00:05', `vim ${AGENT}`],
  ]);
  assert.deepEqual(instancesOf(p, config.processes[1], BUNDLE).map((x) => x.pid), [11]);
  assert.deepEqual(instancesOf(p, config.processes[0], BUNDLE).map((x) => x.pid), [10]);
});

test('judgeSnapshot passes one app with one agent whose parent is the app', () => {
  const r = judgeSnapshot(ps([[10, 1, '00:30', APP], [11, 10, '00:29', `${AGENT} run`]]), config, BUNDLE, { running: true });
  assert.equal(r.ok, true, JSON.stringify(r.checks));
});

test('judgeSnapshot fails a second agent (finding A of the Dyno round 2) and names both pids', () => {
  const r = judgeSnapshot(ps([[10, 1, '01:30', APP], [1360, 10, '01:29', `${AGENT} run`], [1368, 10, '01:19', `${AGENT} run`]]), config, BUNDLE, { running: true });
  assert.equal(r.ok, false);
  const c = r.checks.find((x) => x.id === 'count:dyno-agent');
  assert.match(c.detail, /2 running/);
  assert.match(c.detail, /1360/);
  assert.match(c.detail, /1368/);
});

test('an agent whose parent is launchd passes only with Launch at login on; otherwise it is an orphan', () => {
  const p = ps([[10, 1, '00:30', APP], [11, 1, '00:29', `${AGENT} run`]]);
  assert.equal(judgeSnapshot(p, config, BUNDLE, { running: true, loginService: true }).ok, true);
  const off = judgeSnapshot(p, config, BUNDLE, { running: true, loginService: false });
  assert.equal(off.ok, false);
  assert.match(off.checks.find((c) => c.id.startsWith('parent:')).detail, /not allowed/);
});

test('a helper missing from a running app fails (min defaults to 1)', () => {
  const r = judgeSnapshot(ps([[10, 1, '00:30', APP]]), config, BUNDLE, { running: true });
  assert.equal(r.ok, false);
  assert.match(r.checks.find((c) => c.id === 'count:dyno-agent').detail, /0 running/);
});

test('after Quit nothing of the app may remain, and a leftover with parent 1 is called an orphan', () => {
  const left = ps([[1360, 1, '03:00', `${AGENT} run`], [99, 1, '10:00', '/usr/sbin/cfprefsd']]);
  assert.equal(judgeSnapshot(left, config, BUNDLE, { running: false }).ok, false);
  assert.deepEqual(orphansAfterQuit(left, config, BUNDLE), [{ name: 'dyno-agent', pid: 1360, ppid: 1, etime: '03:00', orphan: true }]);
  assert.equal(judgeSnapshot(ps([[99, 1, '10:00', '/usr/sbin/cfprefsd']]), config, BUNDLE, { running: false }).ok, true);
});

test('the observation window catches a respawn that arrives late', () => {
  const one = ps([[10, 1, '00:30', APP], [11, 10, '00:29', `${AGENT} run`]]);
  const two = ps([[10, 1, '00:40', APP], [11, 10, '00:39', `${AGENT} run`], [12, 10, '00:01', `${AGENT} run`]]);
  assert.deepEqual(peakCounts([one, one, two], config, BUNDLE), { 'dyno-app': 1, 'dyno-agent': 2 });
  assert.equal(judgeWindow([one, one, one], config, BUNDLE, { running: true }).ok, true);
  const bad = judgeWindow([one, one, two], config, BUNDLE, { running: true });
  assert.equal(bad.ok, false);
  assert.match(bad.checks.find((c) => !c.ok).detail, /peak 2 over 3 snapshots/);
});

test('distinctCounts counts processes by pid and the start time ps printed, over a whole step', () => {
  const row = (pid, lstart) => parsePs(`${pid} 10 00:05 ${lstart} ${AGENT} run`);
  const t1 = 'Fri Oct 30 14:25:12 2026';
  const t2 = 'Fri Oct 30 14:25:20 2026';
  const a = { at: 1, procs: row(11, t1) };
  const b = { at: 2, procs: [...row(11, t1), ...row(12, t2)] };
  const c = { at: 3, procs: row(12, t2) };
  assert.equal(distinctCounts([a, b, c], config, BUNDLE)['dyno-agent'], 2, 'pid 11 seen twice is one process, pid 12 another');
  assert.equal(distinctCounts([a, { at: 9, procs: row(11, t1) }], config, BUNDLE)['dyno-agent'], 1);
  // The same pid with another start time is another process (pid reuse), whatever the host clock says.
  assert.equal(distinctCounts([a, { at: 9, procs: row(11, t2) }], config, BUNDLE)['dyno-agent'], 2);
  assert.equal(parsePs(`11 10 00:05 Sat Nov  1 03:04:05 2026 ${AGENT} run`)[0].lstart, 'Sat Nov  1 03:04:05 2026', 'a one-digit day is padded');
});

test('describeTree prints pid, age and parent for the evidence', () => {
  const lines = describeTree(ps([[10, 1, '00:30', APP], [11, 10, '00:29', `${AGENT} run`]]), config, BUNDLE);
  assert.deepEqual(lines, ['dyno-app pid 10 age 00:30 parent 1 (launchd)', 'dyno-agent pid 11 age 00:29 parent 10 (dyno-app)']);
  assert.deepEqual(describeTree([], config, BUNDLE), ["(none of the app's processes)"]);
});

test('parseLaunchctlPrint reads state, pid and program; a missing job gives nulls', () => {
  const text = 'gui/501/net.decryptology.dyno.agent = {\n\tactive count = 1\n\tprogram = /Applications/Dyno.app/Contents/MacOS/dyno-agent\n\tstate = running\n\tpid = 1360\n}\n';
  assert.deepEqual(parseLaunchctlPrint(text), { state: 'running', pid: 1360, program: '/Applications/Dyno.app/Contents/MacOS/dyno-agent' });
  assert.deepEqual(parseLaunchctlPrint('Could not find service'), { state: null, pid: null, program: null });
});

test('the discovery file must name a live agent; one that names a dead pid is stale; absent is absent', () => {
  const spec = { helper: 'dyno-agent', jsonPidField: 'pid' };
  const p = ps([[11, 10, '00:29', `${AGENT} run`]]);
  assert.deepEqual(judgeStateFile('{"pid": 11}', p, spec, BUNDLE), { present: true, pid: 11, live: true, stale: false });
  assert.deepEqual(judgeStateFile('{"pid": 1360}', p, spec, BUNDLE), { present: true, pid: 1360, live: false, stale: true });
  assert.deepEqual(judgeStateFile('not json', p, spec, BUNDLE), { present: true, pid: null, live: false, stale: true });
  assert.deepEqual(judgeStateFile(null, p, spec, BUNDLE), { present: false, pid: null, live: false, stale: false });
});

test('renderResult: the verdict is FAIL when any step failed, manual steps are listed apart, pipes are escaped', () => {
  const md = renderResult({
    app: 'Dyno', version: '1.0.0', installer: 'a.dmg', guest: 'tart', started: 't', commit: 'abc',
    steps: [
      { title: 'Cold start', status: 'pass', summary: 'one app, one agent', checks: [{ ok: true, detail: 'x' }], evidence: ['dyno-app pid 1'] },
      { title: 'Quit', status: 'fail', summary: 'orphan a|b', checks: [{ ok: false, detail: 'agent pid 3 left' }] },
      { title: 'Gatekeeper', status: 'manual', summary: 'right-click Open' },
    ],
  });
  assert.match(md, /Result: \*\*FAIL\*\*/);
  assert.match(md, /\| 2 \| Quit \| FAIL \| orphan a\\\|b \|/);
  assert.match(md, /\| 3 \| Gatekeeper \| MANUAL \|/);
  assert.match(md, /- FAIL: agent pid 3 left/);
  assert.match(md, /manual steps are listed below/);
});

test('every app config lists an app process, only known keys, and helpers whose parents are app or launchd', () => {
  const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'apps');
  const seen = [];
  for (const app of readdirSync(root)) {
    let text;
    try {
      text = readFileSync(join(root, app, 'vm-smoke.json'), 'utf8');
    } catch {
      continue;
    }
    const c = JSON.parse(text);
    seen.push(app);
    assert.equal(c.id, app);
    assert.match(c.bundle, /\.app$/);
    assert.equal(c.processes.filter((p) => p.role === 'app').length, 1, `${app}: exactly one app process`);
    for (const p of c.processes) {
      assert.ok(['app', 'helper'].includes(p.role), `${app}: role of ${p.name}`);
      for (const parent of p.parents ?? []) assert.ok(['app', 'launchd'].includes(parent), `${app}: parent ${parent}`);
    }
    assert.ok(Number.isInteger(c.readyTimeoutSec) && c.readyTimeoutSec > 0, `${app}: readyTimeoutSec`);
    assert.ok(Number.isInteger(c.settleSec) && c.settleSec >= 20 && typeof c.settleWhy === 'string', `${app}: the observation window and why`);
    if (app === 'dyno') assert.match(c.settleWhy, /crash-loop backoff capped at 15 s/, 'Dyno: one full respawn cycle');
    if (c.loginService) assert.match(c.loginService.label, /^net\.decryptology\./);
  }
  assert.ok(seen.includes('dyno'), `configs found: ${seen}`);
});

const LOGIN = 'net.decryptology.dyno.agent';
const loginConfig = { processes: [config.processes[0], { ...config.processes[1], parents: ['app'], loginJob: LOGIN }] };
const withJob = (procs, pid) => Object.assign(procs, { jobs: { [LOGIN]: { state: 'running', pid, program: null } } });

test('the SMAppService agent (bare "dyno-agent run") is found by the launchd job pid, not by name alone', () => {
  const rows = [[10, 1, '00:30', APP], [819, 1, '00:29', 'dyno-agent run']];
  assert.equal(instancesOf(withJob(ps(rows), 819), loginConfig.processes[1], BUNDLE).length, 1);
  assert.equal(instancesOf(withJob(ps(rows), 555), loginConfig.processes[1], BUNDLE).length, 0);
  assert.equal(instancesOf(ps(rows), loginConfig.processes[1], BUNDLE).length, 0);
  const ok = judgeSnapshot(withJob(ps(rows), 819), loginConfig, BUNDLE, { running: true, loginService: true });
  assert.equal(ok.ok, true);
  assert.match(ok.checks.find((c) => c.id.startsWith('parent:')).detail, new RegExp(`job ${LOGIN}`));
  assert.equal(judgeSnapshot(withJob(ps(rows), 819), loginConfig, BUNDLE, { running: true, loginService: false }).ok, false);
});

test('after Quit with the login service on only the supervised agent may remain', () => {
  const left = withJob(ps([[819, 1, '03:00', 'dyno-agent run'], [820, 1, '03:00', `${BUNDLE}/Contents/MacOS/dyno-app`]]), 819);
  assert.deepEqual(orphansAfterQuit(left, loginConfig, BUNDLE, { loginService: true }).map((o) => o.name), ['dyno-app']);
  assert.equal(orphansAfterQuit(left, loginConfig, BUNDLE, { loginService: false }).length, 2);
  assert.deepEqual(supervisedSurvivors(left, loginConfig, BUNDLE), [`dyno-agent pid 819 (launchd job ${LOGIN})`]);
  const gone = withJob(ps([[99, 1, '10:00', '/usr/sbin/cfprefsd']]), null);
  assert.equal(orphansAfterQuit(gone, loginConfig, BUNDLE, { loginService: true }).length, 0);
  assert.deepEqual(supervisedSurvivors(gone, loginConfig, BUNDLE), []);
});

test('a reading the VM base cannot take is NOT MEASURED, not pass or fail', async () => {
  const { readingStep, READINGS } = await import('./helper-readings.mjs');
  const r = READINGS.find((x) => x.id === 'lifespan-smart');
  const s = await readingStep(r, 'linux', { openPage: async () => true, text: async () => 'Lifespan /usr/sbin/smartctl is not installed' });
  assert.equal(s.status, 'not-measured');
  assert.equal(s.summary, 'not measured: smartmontools missing in base');
  assert.equal((await readingStep(r, 'linux', { openPage: async () => true, text: async () => 'Lifespan SSD Disk life Have 14 of 14 days' })).status, 'pass');
  assert.match(renderResult({ app: 'Dyno', steps: [s] }), /NOT MEASURED/);
});

test('the state file names the launchd-run helper: judged with the full process spec (loginJob), it is live; without the job it is stale', () => {
  const rows = [[438, 1, '01:00', `${BUNDLE}/Contents/MacOS/dyno-app`], [500, 1, '01:00', 'dyno-agent run']];
  const procs = withJob(ps(rows), 500);
  const stateSpec = loginConfig.processes[1] && { helper: 'dyno-agent', jsonPidField: 'pid' };
  assert.equal(judgeStateFile('{"pid":500}', procs, stateSpec, BUNDLE, loginConfig.processes[1]).live, true);
  assert.equal(judgeStateFile('{"pid":500}', procs, stateSpec, BUNDLE).live, false, 'the bare name alone is not identity');
  assert.equal(judgeStateFile('{"pid":501}', procs, stateSpec, BUNDLE, loginConfig.processes[1]).stale, true);
  assert.equal(judgeStateFile('{"pid":500}', ps(rows), stateSpec, BUNDLE, loginConfig.processes[1]).live, false, 'launchd does not report the job with this pid');
});
