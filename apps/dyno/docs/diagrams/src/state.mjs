import { P, save, link, pair, dot } from './common.mjs';

const st = (d, id, x, y, w, h, name, desc) => d.node(id, x, y, w, h, 'accent', { name, desc });
const E = (d, a, sa, ta, b, sb, tb, via = [], o = {}) => d.edge(a, b, d.port(a, sa, ta), d.port(b, sb, tb), { via, ...o });

// ---------- service registration ----------
// ADR 0056: the record holds one state; a hold is derived from the attempts, never stored.
{
  const d = P({ title: 'Service registration', kind: 'UML · State machine', w: 1344, date: '2026-11-05',
    desc: 'The record holds one state: Idle, Attempting (phase: removing the old one, registering, awaiting an answer, register failed) or Removing. After 3 unattended registers of one version the start holds; the user\'s Repair or Install goes on.' });
  const W = 208, H = 104, G = 112, X0 = 96, cx = (i) => X0 + i * (W + G), R0 = 216, R1 = R0 + H + 104;
  dot(d, 'ini', 48, R0 + H / 2 - 8, 'start');
  st(d, 'idle', cx(0), R0, W, H, 'Idle', 'The last version that saw the service answer is recorded.');
  st(d, 'old', cx(1), R0, W, H, 'Removing old', 'Recorded before the first removal; a crash here replaces again.');
  st(d, 'reg', cx(2), R0, W, H, 'Registering', 'Recorded before register; a crash here only finishes it.');
  st(d, 'await', cx(3), R0, W, H, 'Awaiting answer', 'The OS accepted it; the lock is free while we wait.');
  st(d, 'fail', cx(2), R1, W, H, 'Register failed', 'The call failed; retried only under 3 attempts.');
  st(d, 'held', cx(3), R1, W, H, 'Held', 'Derived: unanswered, refused or register failed 3 times.');
  st(d, 'unin', cx(0), R1, W, H, 'Removing', 'An uninstall began; the switch went off.');
  E(d, 'ini', 'R', 0.5, 'idle', 'L', 0.5);
  link(d, 'idle', 'R', 'old', 'L', { label: 'New version' });
  link(d, 'old', 'R', 'reg', 'L', { label: 'Gone' });
  link(d, 'reg', 'R', 'await', 'L', { label: 'OS accepted' });
  const xr = Math.round(cx(2) + W * 0.3), xf = Math.round(cx(2) + W * 0.7);
  d.edge('reg', 'fail', [xr, R0 + H], [xr, R1], {}); d.lbl('Register fails', xr, (R0 + H + R1) / 2, 'l');
  d.edge('fail', 'reg', [xf, R1], [xf, R0 + H], {}); d.lbl('Retry', xf, (R0 + H + R1) / 2, 'r');
  link(d, 'await', 'B', 'held', 'T', { label: 'No answer, 3 tries' });
  link(d, 'fail', 'R', 'held', 'L', { label: 'Fails 3 times' });
  E(d, 'await', 'T', 0.5, 'idle', 'T', 0.5, [[Math.round(cx(3) + W / 2), R0 - 40], [Math.round(cx(0) + W / 2), R0 - 40]]);
  d.lbl('Answers', Math.round((cx(0) + cx(3) + W) / 2), R0 - 48, 'a', { maxw: 120 });
  E(d, 'held', 'B', 0.5, 'old', 'B', 0.5, [[Math.round(cx(3) + W / 2), R1 + H + 40], [Math.round(cx(1) + W / 2), R1 + H + 40]]);
  d.lbl('Repair or Install', Math.round((cx(1) + cx(3) + W) / 2), R1 + H + 36, 'a', { maxw: 160 });
  const xi = Math.round(cx(0) + W * 0.3), xu = Math.round(cx(0) + W * 0.7);
  d.edge('idle', 'unin', [xi, R0 + H], [xi, R1], {}); d.lbl('Switched off', xi, (R0 + H + R1) / 2, 'l');
  d.edge('unin', 'idle', [xu, R1], [xu, R0 + H], {}); d.lbl('Removed', xu, (R0 + H + R1) / 2, 'r');
  const ny = R1 + H + 96;
  d.note('n', cx(0), ny, 4 * W + 3 * G, 'Every start, install, repair, resume and uninstall passes one transition function and writes the record before each OS step. A hold ends when the service answers, the version changes, or the user presses Repair. macOS and Linux share this; Windows Dyno has no agent service.');
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
  d.note('n', cx(0), ny, 3 * W + 2 * G, 'Every backend request has a timeout and an inline error with Retry; initialisers start work in the background and return, so a reopened window is never blank for good. The agent keeps sampling and raising alarms after the app exits.');
  d.legend([{ kind: 'accent', label: 'State' }, { kind: 'line', label: 'Event' }]);
  save('uml-state-window', d, ny + d.rect.n[3]);
}
