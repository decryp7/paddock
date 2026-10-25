import { P, save, link, pair, dot } from './common.mjs';

const st = (d, id, x, y, w, h, name, desc) => d.node(id, x, y, w, h, 'accent', { name, desc });
const E = (d, a, sa, ta, b, sb, tb, via = [], o = {}) => d.edge(a, b, d.port(a, sa, ta), d.port(b, sb, tb), { via, ...o });

// ---------- rule engine ----------
{
  const d = P({ title: 'Rule engine, per rule and application', kind: 'UML · State machine', w: 1488,
    desc: 'One small state machine per (rule, app), fed a tick at a time. Steps add up and are undone in reverse order; protected processes and the foreground app are never touched.' });
  const W = 216, H = 104, G = 144, X0 = 96, cx = (i) => X0 + i * (W + G), R0 = 232, R1 = R0 + H + 80, busY = R1 + H + 72;
  dot(d, 'ini', 48, R0 + H / 2 - 8, 'start');
  st(d, 'idle', cx(0), R0, W, H, 'Idle', 'The rule does not apply, or the app is below the threshold.');
  st(d, 'watch', cx(1), R0, W, H, 'Watching', 'Above the threshold since t.');
  st(d, 'act', cx(2), R0, W, H, 'Acting, step n', 'Steps 1…n applied to every member.');
  st(d, 'rel', cx(3), R0, W, H, 'Releasing', 'Below the threshold since t.');
  st(d, 'snooze', cx(1), R1, W, H, 'Snoozed', 'After a manual undo; the rule waits for the app to settle.');
  E(d, 'ini', 'R', 0.5, 'idle', 'L', 0.5);
  pair(d, 'idle', 'watch', 'Above threshold', 'Drops below');
  link(d, 'watch', 'R', 'act', 'L', { label: 'Held for_secs' });
  pair(d, 'act', 'rel', 'Below threshold', 'Above again');
  // next step: a loop over the top of Acting
  const ax = cx(2), top = R0 - 40;
  d.edge('act', 'act', [Math.round(ax + W * 0.3), R0], [Math.round(ax + W * 0.7), R0], { via: [[Math.round(ax + W * 0.3), top], [Math.round(ax + W * 0.7), top]] });
  d.lbl('Next step after after_secs', ax + W / 2, top, 'a', { maxw: 240 });
  // user undoes: Acting -> Snoozed
  const sy = R1 + H / 2;
  E(d, 'act', 'B', 0.2, 'snooze', 'R', 0.5, [[Math.round(ax + W * 0.2), sy]]);
  d.lbl('User undoes', ax + W * 0.2 + 8, R0 + H + 28, 'r');
  // Snoozed -> Idle
  E(d, 'snooze', 'L', 0.5, 'idle', 'B', 0.7, [[Math.round(cx(0) + W * 0.7), sy]]);
  d.lbl('Below threshold', cx(0) + W * 0.7 + 8, sy - 4, 'ar', { maxw: 120 });
  // one merged bus: undo all in reverse order
  const bx1 = Math.round(cx(0) + W * 0.3);
  E(d, 'act', 'B', 0.8, 'idle', 'B', 0.3, [[Math.round(ax + W * 0.8), busY], [bx1, busY]]);
  E(d, 'rel', 'B', 0.5, 'idle', 'B', 0.3, [[Math.round(cx(3) + W * 0.5), busY], [bx1, busY]]);
  d.lbl('Undo all (reverse order)', (bx1 + ax + W * 0.8) / 2, busY, 'b', { maxw: 260 });
  const ny = busY + 56;
  d.note('n', cx(0), ny, 4 * W + 3 * G, 'Undo all also runs when the rule is disabled or paused, another profile is chosen, the context turns false or the app comes to the front. A process that joins while Acting gets the steps already taken. Steps are LowerPriority, EfficiencyMode, CpuCap(percent) and Notify; Notify has no inverse.');
  d.legend([{ kind: 'accent', label: 'State' }, { kind: 'line', label: 'Event' }]);
  save('uml-state-rule', d, ny + d.rect.n[3]);
}

// ---------- service registration ----------
{
  const d = P({ title: 'Service registration', kind: 'UML · State machine', w: 1344,
    desc: 'Status as dyno_agent::service reports it: not installed, or installed with two flags (running, points at this executable). ensure repairs a stale registration but never installs one unasked.' });
  const W = 232, H = 104, G = 144, X0 = 96, cx = (i) => X0 + i * (W + G), R0 = 216, R1 = R0 + H + 88;
  dot(d, 'ini', 48, R0 + H / 2 - 8, 'start');
  st(d, 'none', cx(0), R0, W, H, 'Not installed', 'No plist, unit or Run value.');
  st(d, 'run', cx(1), R0, W, H, 'Registered, running', 'Points at the current executable.');
  st(d, 'stale', cx(2), R0, W, H, 'Registered, stale', 'Names another executable, as after an update.');
  st(d, 'stop', cx(1), R1, W, H, 'Registered, not running', 'Points at the current executable.');
  E(d, 'ini', 'R', 0.5, 'none', 'L', 0.5);
  pair(d, 'none', 'run', 'Install', 'Uninstall');
  pair(d, 'run', 'stale', 'App update', 'Ensure');
  const x1 = Math.round(cx(1) + W * 0.3), x2 = Math.round(cx(1) + W * 0.7);
  d.edge('run', 'stop', [x1, R0 + H], [x1, R1], {}); d.lbl('Crash or stop', x1, (R0 + H + R1) / 2, 'l');
  d.edge('stop', 'run', [x2, R1], [x2, R0 + H], {}); d.lbl('OS restarts it', x2, (R0 + H + R1) / 2, 'r');
  E(d, 'stop', 'L', 0.5, 'none', 'B', 0.5, [[Math.round(cx(0) + W / 2), R1 + H / 2]]);
  d.lbl('Uninstall', cx(0) + W / 2 + 8, R1 + H / 2 - 4, 'ar', { maxw: 120 });
  const ny = R1 + H + 56;
  d.note('n', cx(0), ny, 3 * W + 2 * G, 'macOS: launchctl bootout then bootstrap on every install. Linux: systemctl --user enable --now. Windows: an HKCU Run value; no manager restarts a crashed agent, the app or the next logon does.');
  d.legend([{ kind: 'accent', label: 'State' }, { kind: 'line', label: 'Event' }]);
  save('uml-state-service', d, ny + d.rect.n[3]);
}

// ---------- window lifecycle ----------
{
  const d = P({ title: 'Window lifecycle', kind: 'UML · State machine', w: 1344,
    desc: 'Closing the window destroys it, because a hidden WebKit view can come back blank; the tray and the agent stay. Opening builds a fresh window. Quitting the app never stops the agent.' });
  const W = 256, H = 104, G = 144, X0 = 96, cx = (i) => X0 + i * (W + G), R0 = 216, R1 = R0 + H + 96;
  dot(d, 'ini', 48, R0 + H / 2 - 8, 'start');
  st(d, 'start', cx(0), R0, W, H, 'Starting', 'A splash in index.html; no IPC awaited before the first paint.');
  st(d, 'open', cx(1), R0, W, H, 'Window open', 'Angular UI live, subscribed to pushes.');
  st(d, 'tray', cx(2), R0, W, H, 'Closed to tray', 'No window, no webview; tray and agent running.');
  st(d, 'quit', cx(1), R1, W, H, 'Quitting', 'Unsubscribe, exit the app.');
  dot(d, 'fin', Math.round(cx(1) + W / 2 - 12), R1 + H + 56, 'end');
  E(d, 'ini', 'R', 0.5, 'start', 'L', 0.5);
  link(d, 'start', 'R', 'open', 'L', { label: 'First paint' });
  pair(d, 'open', 'tray', 'Close, tray present', 'Open: rebuild');
  link(d, 'open', 'B', 'quit', 'T', { label: 'Quit, or close with no tray' });
  E(d, 'tray', 'B', 0.5, 'quit', 'R', 0.5, [[Math.round(cx(2) + W / 2), R1 + H / 2]]);
  d.lbl('Quit', cx(2) + W / 2 + 8, R1 + H / 2 - 4, 'ar', { maxw: 80 });
  E(d, 'quit', 'B', 0.5, 'fin', 'T', 0.5);
  const ny = R1 + H + 56 + 24 + 56;
  d.note('n', cx(0), ny, 3 * W + 2 * G, 'Every backend request has a timeout and an inline error with Retry; initialisers start work in the background and return, so a reopened window is never blank for good. The agent keeps sampling and keeps rules applied after the app exits.');
  d.legend([{ kind: 'accent', label: 'State' }, { kind: 'line', label: 'Event' }]);
  save('uml-state-window', d, ny + d.rect.n[3]);
}
