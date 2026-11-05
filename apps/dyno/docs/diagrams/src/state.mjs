import { P, save, link, pair, dot } from './common.mjs';

const st = (d, id, x, y, w, h, name, desc) => d.node(id, x, y, w, h, 'accent', { name, desc });
const E = (d, a, sa, ta, b, sb, tb, via = [], o = {}) => d.edge(a, b, d.port(a, sa, ta), d.port(b, sb, tb), { via, ...o });

// ---------- service registration ----------
// ADR 0056 (revision 4): the record holds one state; a hold is derived from the attempts, never stored.
{
  const d = P({ title: 'Service registration', kind: 'UML · State machine', w: 1552, date: '2026-11-06',
    desc: 'One record state; every start, install, repair, resume and uninstall passes one transition function.' });
  const W = 232, H = 120, G = 152, X0 = 96, cx = (i) => X0 + i * (W + G), R0 = 292, R1 = R0 + H + 104;
  const gAt = cx(1) - 24, gR = cx(3) + W + 24, gTop = R0 - 64, gBot = R1 + H + 64;
  const oTop = gTop - 60, oBot = gBot + 24, oL = cx(0) - 24, oR = gR + 24;
  d.group('outer', oL, oTop, oR - oL, oBot - oTop, 'Record state');
  d.group('att', gAt, gTop, gR - gAt, gBot - gTop, 'Attempting (version, attempts)');
  dot(d, 'ini', 40, R0 + H / 2 - 8, 'start');
  st(d, 'idle', cx(0), R0, W, H, 'Idle', 'The last version that saw the service answer, if any.');
  st(d, 'old', cx(1), R0, W, H, 'Removing old', 'Written before the first removal. After a crash: still there, replace; gone, just register.');
  st(d, 'reg', cx(2), R0, W, H, 'Registering', 'Written before register. After a crash: absent, just register; there, wait.');
  st(d, 'await', cx(3), R0, W, H, 'Awaiting answer', 'The OS accepted it; the lock is free while we wait.');
  st(d, 'fail', cx(2), R1, W, H, 'Register failed', 'Other, or not as written. Retried only under 3 attempts.');
  st(d, 'held', cx(3), R1, W, H, 'Held', 'Derived, not stored: unanswered, refused or register failed 3 times.');
  st(d, 'unin', cx(0), oBot + 72, W, 88, 'Removing (uninstall)', 'Written before the first OS step.');
  const at = (id, side, t) => d.port(id, side, t);
  const ym = R0 + H / 2;
  // Start, and Idle's two ways out.
  E(d, 'ini', 'R', 0.5, 'idle', 'L', 0.5);
  d.edge('idle', 'old', [cx(0) + W, ym], [cx(1), ym], {});
  d.lbl('New version or Repair', (cx(0) + W + cx(1)) / 2, ym, 'a', { maxw: 140 });
  const yI = R0 + H + 26, xI = cx(2) - 44, xi = Math.round(cx(0) + W * 0.9), yR = Math.round(R0 + H * 0.8);
  d.edge('idle', 'reg', [xi, R0 + H], [cx(2), yR], { via: [[xi, yI], [xI, yI], [xI, yR]] });
  d.lbl('First install, switch on', Math.round((xi + cx(1) + W / 2 - 24) / 2), yI, 'b', { maxw: 200 });
  // The replace.
  d.edge('old', 'reg', [cx(1) + W, ym - 24], [cx(2), ym - 24], {});
  d.lbl('Gone or timed out', (cx(1) + W + cx(2)) / 2, ym - 24, 'a', { maxw: 140 });
  d.edge('reg', 'await', [cx(2) + W, ym], [cx(3), ym], {});
  d.lbl('OS accepted', (cx(2) + W + cx(3)) / 2, ym, 'a', { maxw: 140 });
  const x1 = Math.round(cx(2) + W * 0.3), x2 = Math.round(cx(2) + W * 0.7), my = (R0 + H + R1) / 2;
  d.edge('reg', 'fail', [x1, R0 + H], [x1, R1], {}); d.lbl('[Other, not as written]', x1, my + 18, 'l', { maxw: 200 });
  d.edge('fail', 'reg', [x2, R1], [x2, R0 + H], {}); d.lbl('Retry [attempts < 3]', x2, my + 18, 'r', { maxw: 200 });
  // No answer: another cycle under the cap, a hold at it.
  const xa = Math.round(cx(3) + W * 0.3), xo = Math.round(cx(1) + W * 0.7), yT = R0 - 26;
  d.edge('await', 'old', [xa, R0], [xo, R0], { via: [[xa, yT], [xo, yT]] });
  d.lbl('[attempts < 3] no answer', Math.round((xa + xo) / 2), yT, 'b', { maxw: 220 });
  d.edge('await', 'held', [cx(3) + W / 2, R0 + H], [cx(3) + W / 2, R1], {});
  d.lbl('[attempts = 3]', cx(3) + W / 2, my + 18, 'r', { maxw: 120 });
  // A hold ends by Repair or a new version (or by the service answering, below).
  const yB = R1 + H + 32, xh = Math.round(cx(3) + W * 0.3), xb = Math.round(cx(1) + W / 2);
  d.edge('held', 'old', [xh, R1 + H], [xb, R0 + H], { via: [[xh, yB], [xb, yB]] });
  d.lbl('New version or Repair', Math.round((xh + xb) / 2), yB, 'a', { maxw: 200 });
  // The service answers (Awaiting or Held): back to Idle, from the group's border.
  const xg = Math.round(cx(3) + W * 0.85), yA = gTop - 30, xd = Math.round(cx(0) + W / 2);
  d.edge('att', 'idle', [xg, gTop], [xd, R0], { via: [[xg, yA], [xd, yA]] });
  d.lbl('Answers', Math.round((cx(1) + cx(2)) / 2), yA, 'a', { maxw: 100 });
  // Uninstall from any state: from the border of the outer group.
  const xu = Math.round(cx(0) + W * 0.7), xr = Math.round(cx(0) + W * 0.3);
  d.edge('outer', 'unin', [xu, oBot], [xu, oBot + 72], {}); d.lbl('Uninstall, switch off', xu, oBot + 36, 'r', { maxw: 200 });
  d.edge('unin', 'idle', [xr, oBot + 72], [xr, R0 + H], {}); d.lbl('Removed', xr, oBot + 36, 'l', { maxw: 100 });
  const ny = oBot + 72 + 88 + 48;
  d.note('n', cx(0), ny, 4 * W + 3 * G, 'New version replaces on macOS; on Linux and Windows only when the definition differs, and a current one only updates the record. Declined, cancelled, needs approval or elevation, or unavailable: back to the state before. A hold ends when the service answers, the version changes, or Repair or Install is pressed (they run past 3 attempts).');
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
