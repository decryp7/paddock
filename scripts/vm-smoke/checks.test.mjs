import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { Check, runAll } from './checks.mjs';
import { parseArgs } from './run.mjs';

const config = JSON.parse(readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'apps', 'dyno', 'vm-smoke.json'), 'utf8'));
const B = '/Applications/Dyno.app';

/** A small fake guest: a process table, an agent.json, and the few commands the check issues. `bug` picks a seeded fault. */
function fakeGuest({ bug = null } = {}) {
  let clock = 0, pid = 100, procs = [], agentJson = null, loginOn = false;
  const spawn = (name, ppid, args = '') => { const p = { pid: ++pid, ppid, born: clock, command: `${B}/Contents/MacOS/${name}${args}` }; procs.push(p); return p; };
  const startApp = () => {
    if (procs.some((p) => p.command.includes('dyno-app'))) return; // single instance
    const app = spawn('dyno-app', 1);
    const agent = spawn('dyno-agent', loginOn ? 1 : app.pid, ' run');
    agentJson = JSON.stringify({ pid: agent.pid });
    if (bug === 'second-agent') setTimeout_(10, () => spawn('dyno-agent', app.pid, ' run'));
  };
  const timers = [];
  const setTimeout_ = (after, fn) => timers.push({ at: clock + after, fn });
  const fmt = (s) => `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
  const calls = [];
  const exec = async (cmd) => {
    calls.push(cmd);
    const ok = (stdout = '') => ({ code: 0, stdout, stderr: '' });
    if (cmd.startsWith('printf %s "$HOME"')) return ok('/Users/admin');
    if (cmd.startsWith('ps -axww')) return ok(procs.map((p) => `${p.pid} ${p.ppid} ${fmt(clock - p.born)} ${p.command}`).join('\n'));
    if (cmd.includes('agent.json') && cmd.startsWith('cat')) return agentJson ? ok(agentJson) : { code: 1, stdout: '', stderr: '' };
    if (cmd.startsWith('rm -f') && cmd.includes('agent.json')) { agentJson = null; return ok(); }
    if (cmd.startsWith('open -a')) { if (bug !== 'no-start') startApp(); return ok(); }
    if (cmd.startsWith('osascript')) {
      const app = procs.find((p) => p.command.includes('dyno-app'));
      // Quit ends the app; the agent ends with it, unless the seeded fault leaves it behind, re-parented to launchd.
      procs = procs.filter((p) => !p.command.includes('dyno-app') && (bug === 'orphan-agent' || !p.command.includes('dyno-agent')));
      if (bug === 'orphan-agent') { for (const a of procs) a.ppid = 1; } else agentJson = null;
      return app ? ok() : { code: 1, stdout: '', stderr: 'not running' };
    }
    const kill = /^ps -p (\d+) -o command= \| grep .* && kill \d+$/.exec(cmd);
    if (kill) { procs = procs.filter((p) => p.pid !== Number(kill[1])); return ok(); }
    if (cmd.includes('dyno-agent') && cmd.includes('status') && !cmd.includes('service')) return procs.some((p) => p.command.includes('dyno-agent')) ? ok('running') : { code: 1, stdout: '', stderr: '' };
    if (cmd.includes("'service' 'install'")) { loginOn = true; const a = procs.find((p) => p.command.includes('dyno-agent')); if (a) a.ppid = 1; return ok('installed and started'); }
    if (cmd.includes("'service' 'uninstall'")) { loginOn = false; const app = procs.find((p) => p.command.includes('dyno-app')); const a = procs.find((p) => p.command.includes('dyno-agent')); if (a && app) a.ppid = app.pid; return ok('removed'); }
    if (cmd.startsWith('launchctl print')) return bug === 'no-launchd' ? { code: 113, stdout: 'Could not find service', stderr: '' } : ok('\tstate = running\n\tpid = 7\n');
    if (cmd.startsWith('defaults read')) return ok('1.0.0\n');
    if (cmd.startsWith('sudo sh -c')) return ok('4242\n');
    if (cmd.startsWith('tail')) return ok('log line\n');
    return ok(); // hdiutil, codesign, sudo purge, sed, ps -p ... nc
  };
  return { calls, guest: { exec, now: () => clock * 1000, sleep: async (ms) => { clock += ms / 1000; for (const t of timers.filter((x) => x.at <= clock)) { timers.splice(timers.indexOf(t), 1); t.fn(); } } } };
}

const run = async (bug) => {
  const f = fakeGuest({ bug });
  const c = new Check(f.guest, { ...config, settleSec: 12, readyTimeoutSec: 30 });
  const r = await runAll(c, { stagedDmg: '/x/installer.dmg', manual: [{ title: 'Gatekeeper (manual)', summary: 'click' }] });
  return { ...r, calls: f.calls };
};
const byTitle = (r, re) => r.steps.find((s) => re.test(s.title));

test('a healthy app passes every step and lists the manual ones apart', async () => {
  const r = await run(null);
  const bad = r.steps.filter((s) => s.status === 'fail');
  assert.deepEqual(bad.map((s) => `${s.title}: ${s.summary}`), []);
  assert.equal(byTitle(r, /Cold start/).status, 'pass');
  assert.equal(byTitle(r, /Launch at login/).status, 'pass');
  assert.equal(byTitle(r, /Gatekeeper/).status, 'manual');
  assert.equal(r.version, '1.0.0');
});

test('a second agent that arrives ten seconds after the first fails the cold start with both pids', async () => {
  const r = await run('second-agent');
  const s = byTitle(r, /Cold start/);
  assert.equal(s.status, 'fail');
  assert.match(s.checks.find((c) => !c.ok).detail, /dyno-agent/);
});

test('an agent left running after Quit is reported as an orphan of launchd, and the check ends it by identity', async () => {
  const r = await run('orphan-agent');
  const q = byTitle(r, /Quit with a window open/);
  assert.equal(q.status, 'fail');
  assert.ok(q.checks.some((c) => /an orphan of launchd/.test(c.detail)), JSON.stringify(q.checks));
  assert.ok(r.calls.some((c) => /^ps -p \d+ -o command= \| grep -qxF -- .* && kill \d+$/.test(c)), 'identity check and kill in one guest command');
});

test('an app that never starts fails on the guard instead of passing', async () => {
  const r = await run('no-start');
  assert.equal(byTitle(r, /Cold start/).status, 'fail');
  assert.match(byTitle(r, /Cold start/).checks[0].detail, /not ready within the 30 s guard/);
});

test('Launch at login whose launchd job never loads is reported as not proven, not as a pass', async () => {
  const r = await run('no-launchd');
  const s = byTitle(r, /Launch at login/);
  assert.equal(s.status, 'skip');
  assert.match(s.summary, /NOT proven/);
  assert.equal(r.steps.some((x) => x.status === 'fail'), false);
});

test('arguments: one app, exactly one installer source, macOS only', () => {
  assert.equal(parseArgs(['--app', 'dyno', '--run', '7193']).run, '7193');
  assert.throws(() => parseArgs(['--run', '1']), /--app/);
  assert.throws(() => parseArgs(['--app', 'dyno']), /exactly one/);
  assert.throws(() => parseArgs(['--app', 'dyno', '--run', '1', '--installer', 'x']), /exactly one/);
  assert.throws(() => parseArgs(['--app', '../x', '--run', '1']), /not an app id/);
  assert.throws(() => parseArgs(['--app', 'dyno', '--run', '1', '--guest', 'windows']), /only macos/);
});
