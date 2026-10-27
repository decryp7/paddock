import { P, save, link, hub, dot } from './common.mjs';

// ---------- runner lifecycle ----------
{
  const d = P({ title: 'Runner lifecycle', kind: 'UML · State machine', w: 1520,
    desc: 'One runner, left to right: wait, start, run, stop. Every way back to Starting runs along the bottom as one "Launch" bus.' });
  const W = 184, H = 88, X0 = 96, G = 104, cx = (i) => X0 + i * (W + G);
  const R0 = 208, R1 = 336, R2 = 464, R3 = 592, yTop = 168, busY = 712;
  const st = (id, i, y, name, desc, h = H, role = 'accent') => d.node(id, cx(i), y, W, h, role, { name, desc });
  st('wts', 0, R1, 'WaitingToStart', 'Back-off: 10 s doubling to 5 min.');
  st('sta', 1, R1, 'Starting', 'entry: Launch; the launcher spawns the process.');
  st('run', 2, R1, 'Running', 'Process alive; stall detector watches.');
  st('dra', 2, R2, 'Draining', 'A request waits; finish the job, take no more.');
  st('stp', 3, R1, 'Stopping', 'Graceful stop; after 120 s with no job the tree is killed.', H * 3 + 80);
  st('stopped', 4, R1, 'Stopped', 'The user stopped it; Start launches it again.');
  st('paused', 4, R2, 'Paused', 'Drained for maintenance; Resume launches it.');
  st('held', 4, R3, 'Held', 'An update has taken over; releasing it launches.');
  st('wfl', 0, R2, 'WaitingForLogon', 'Interactive runner on Windows; nobody is logged on.');
  st('cl', 1, R0, 'CrashLoop', '5 exits in 10 min pause restarts for 30 min.');
  dot(d, 'dec', cx(2) + W / 2 - 16, R0 + 28, 'choice');
  dot(d, 'ini', 48, R1 + H / 2 - 8, 'start');
  const P_ = (id, s, t) => d.port(id, s, t);
  const E = (a, sa, ta, b, sb, tb, via = [], o = {}) => d.edge(a, b, P_(a, sa, ta), P_(b, sb, tb), { via, ...o });
  E('ini', 'R', 0.5, 'wts', 'L', 0.5);
  link(d, 'wts', 'R', 'sta', 'L', { label: 'now ≥ at' });
  link(d, 'sta', 'R', 'run', 'L', { label: 'Launched' });
  link(d, 'run', 'R', 'stp', 'L', { label: 'Stop now', t: [0.5, 0.5 * 88 / (H * 3 + 80)] });
  link(d, 'dra', 'R', 'stp', 'L', { label: 'Job ended', t: [0.5, (R2 + H / 2 - R1) / (H * 3 + 80)] });
  // Running <-> Draining
  E('run', 'B', 0.3, 'dra', 'T', 0.3); d.lbl('Request, job runs', cx(2) + W * 0.3, R1 + H + 20, 'l');
  E('dra', 'T', 0.7, 'run', 'B', 0.7); d.lbl('Resume', cx(2) + W * 0.7, R1 + H + 20, 'r');
  // exits -> restart decision
  E('run', 'T', 0.5, 'dec', 'B', 0.5); d.lbl('Exits', cx(2) + W / 2, R0 + 60 + 20 + 0, 'r');
  E('dec', 'L', 0.5, 'cl', 'R', 0.5); d.lbl('[5 exits in 10 min]', (cx(1) + W + cx(2) + W / 2 - 16) / 2, R0 + 44, 'a');
  E('dec', 'T', 0.5, 'wts', 'T', 0.5, [[cx(2) + W / 2, yTop], [cx(0) + W / 2, yTop]]); d.lbl('[fewer exits] RestartAt', (cx(0) + cx(2) + W) / 2, yTop, 'a');
  E('stp', 'T', 0.5, 'dec', 'R', 0.5, [[cx(3) + W / 2, R0 + 44]]); d.lbl('StallRestart', (cx(2) + W / 2 + 16 + cx(3) + W / 2) / 2, R0 + 44, 'a');
  link(d, 'cl', 'B', 'sta', 'T', { label: 'Cool-down over' });
  // Starting <-> WaitingForLogon
  E('sta', 'B', 0.12, 'wfl', 'R', 0.3, [[cx(1) + W * 0.12, R2 + H * 0.3]]); d.lbl('No user logged on', (cx(0) + W + cx(1) + W * 0.12) / 2, R2 + H * 0.3, 'a');
  E('wfl', 'R', 0.7, 'sta', 'B', 0.34, [[cx(1) + W * 0.34, R2 + H * 0.7]]); d.lbl('User logs on', (cx(0) + W + cx(1) + W * 0.34) / 2, R2 + H * 0.7, 'b');
  // Stopping -> end states
  hub(d, 'stp', 'R', 'stopped', 'L', { label: 'Stop' });
  hub(d, 'stp', 'R', 'paused', 'L', { label: 'Pause' });
  hub(d, 'stp', 'R', 'held', 'L', { label: 'Hold' });
  // the Launch bus
  const busX = cx(4) + W + 40, sx = cx(1) + W * 0.8;
  for (const [id, y] of [['stopped', R1], ['paused', R2], ['held', R3]]) E(id, 'R', 0.5, 'sta', 'B', 0.8, [[busX, y + H / 2], [busX, busY], [sx, busY]]);
  E('stp', 'B', 0.5, 'sta', 'B', 0.8, [[cx(3) + W / 2, busY], [sx, busY]]);
  d.lbl('Launch', (sx + cx(3) + W / 2) / 2, busY, 'b');
  d.note('n', cx(0), R3, W, 'Controller::new starts in WaitingToStart (wanted), Stopped or Paused, from the saved state. Start clears a crash loop and a pause. A launch that fails counts as an exit.');
  d.legend([{ kind: 'accent', label: 'State' }, { kind: 'line', label: 'Event / action' }]);
  save('uml-state-runner', d, busY + 24);
}

// ---------- update machine ----------
{
  const d = P({ title: 'Update machine', kind: 'UML · State machine', w: 1600,
    desc: 'Download, verify, wait for the job, swap, restart, confirm; roll back if the new runner is not accepted. Every run ends in one outcome.' });
  const W = 128, G = 96, H = 128, X0 = 48, cx = (i) => X0 + i * (W + G), R1 = 176, R2 = R1 + H + 56, barY = R2 + 88 + 56;
  const names = [['idl', 'Idle', 'No update in progress.'], ['dl', 'Downloading', 'entry: Download { version }'], ['ver', 'Verifying', 'entry: Verify (checksums from two hosts, --version)'],
    ['dra', 'Draining', 'entry: WatchForIdle; the runner keeps working'], ['stp', 'Stopping', 'entry: StopGracefully (runner idle)'],
    ['swp', 'Swapping', 'entry: Swap (binary → .previous, download → binary)'], ['con', 'Confirming', 'entry: StartRunner; wait for the server']];
  names.forEach(([id, n, t], i) => d.node(id, cx(i), R1, W, H, 'accent', { name: n, desc: t }));
  d.node('rb', cx(6), R2, W, 88, 'accent', { name: 'RollingBack', desc: 'entry: StopRunner, RestorePrevious' });
  d.node('bar', X0, barY, cx(6) + W - X0, 56, 'process', { name: 'Finish { outcome }', desc: 'Updated · RolledBack · Postponed · Cancelled · Failed, then back to Idle.' });
  const ids = names.map((n) => n[0]);
  [['idl', 'dl', 'Begin'], ['dl', 'ver', 'Downloaded'], ['ver', 'dra', 'Verified'], ['dra', 'stp', 'RunnerIdle'], ['stp', 'swp', 'Stopped'], ['swp', 'con', 'Swapped']].forEach(([a, b, l]) => link(d, a, 'R', b, 'L', { label: l }));
  d.edge('dra', 'swp', d.port('dra', 'T', 0.5), d.port('swp', 'T', 0.5), { via: [[cx(3) + W / 2, R1 - 24], [cx(5) + W / 2, R1 - 24]] });
  d.lbl('Not running', cx(4) + W / 2, R1 - 24, 'a');
  link(d, 'con', 'B', 'rb', 'T', { label: 'Not accepted' });
  const outs = [['dl', 'Failed, Cancelled'], ['ver', 'Failed, Cancelled'], ['dra', 'Postponed, Cancelled'], ['stp', 'Postponed'], ['swp', 'Failed']];
  for (const [id, l] of outs) { link(d, id, 'B', 'bar', 'T', { label: l, at: 0.5 }); }
  d.edge('rb', 'bar', d.port('rb', 'B', 0.5), [cx(6) + W / 2, barY], {});
  d.lbl('RolledBack, Failed', cx(6) + W / 2, R2 + 88 + 28, 'l');
  const busX = cx(6) + W + 40;
  d.edge('con', 'bar', d.port('con', 'R', 0.8), d.port('bar', 'R', 0.5), { via: [[busX, Math.round(R1 + H * 0.8)], [busX, barY + 28]] });
  d.lbl('Accepted', busX, R2 + 20, 'r');
  d.edge('bar', 'idl', [X0 + W / 2, barY], d.port('idl', 'B', 0.5));
  d.lbl('Back to Idle', X0 + W / 2, R1 + H + 100, 'r');
  d.note('n', cx(0), barY + 56 + 24, 5 * W + 4 * G, 'Postponed and Cancelled leave the installed version untouched. Failed with "needs attention" (no previous binary, or the roll-back itself failed) leaves the run for the owner.');
  d.legend([{ kind: 'accent', label: 'Phase' }, { kind: 'process', label: 'Outcome' }, { kind: 'line', label: 'Event' }]);
  save('uml-state-update', d, d.rect.n[1] + d.rect.n[3]);
}
