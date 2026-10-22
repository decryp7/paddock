import { P, save, link, hub, dot } from './common.mjs';

// ---------- watch status ----------
{
  const d = P({ title: 'Watch status', kind: 'UML · State machine', w: 1360,
    desc: 'What a watch shows after each check. Any status can follow any other: the next check decides. Pause stops checks without changing the status.' });
  const NH = 72, G = 16, y0 = 160;
  const SX = 760, SW = 264, n = 7, gh = 40 + n * NH + (n - 1) * G + 24;
  const gx = SX - 24;
  const ys = (i) => y0 + 40 + i * (NH + G);
  d.group('g', gx, y0, SW + 48, gh, 'Status after a check');
  const states = [
    ['ok', 'ok', 'Ok', 'The last check worked and releases match the filters.'],
    ['nm', 'noMatch', 'NoMatch', 'It worked, but nothing matched the filters.'],
    ['un', 'unreachable', 'Unreachable', 'Share offline, network down or server error; retried with back-off.'],
    ['au', 'authNeeded', 'AuthNeeded', 'Credentials are missing or were refused.'],
    ['ny', 'needsYou', 'NeedsYou', 'A page asked for a login or a captcha; the user deals with it.'],
    ['iv', 'invalid', 'Invalid', 'The watch\'s configuration is not valid; no check ran.'],
    ['fa', 'failed', 'Failed', 'Any other failure: not found, bad response, timeout.'],
  ];
  states.forEach(([id, , name, desc], i) => d.node(id, SX, ys(i), SW, NH, 'accent', { name, desc }));
  const midY = y0 + 40 + (n * NH + (n - 1) * G) / 2;
  const CW = 184, CH = n * NH + (n - 1) * G, CX = 392;
  d.node('chk', CX, ys(0), CW, CH, 'accent', { name: 'Checking', desc: 'A scheduled check or Check now. The old status stays on screen until the result is in.' });
  d.node('pen', 96, Math.round(midY - 40), 176, 80, 'accent', { name: 'Pending', desc: 'Saved, not checked yet.' });
  dot(d, 'ini', 48, Math.round(midY - 8), 'start');
  d.edge('ini', 'pen', d.port('ini', 'R', 0.5), d.port('pen', 'L', 0.5));
  link(d, 'pen', 'R', 'chk', 'L', { label: 'First check' });
  const labels = ['Releases match', 'None match', 'Cannot reach', 'Refused', 'Challenge', 'Bad config', 'Other error'];
  states.forEach(([id], i) => hub(d, 'chk', 'R', id, 'L', { label: labels[i] }));
  // paused
  const px = gx + SW + 48 + 112, PH = 128, py = Math.round(midY - PH / 2);
  d.node('pa', px, py, 200, PH, 'process', { name: 'Paused', desc: 'No scheduled check runs. Check now still works; the status is kept.' });
  const ya = py + 40, yb = py + 96;
  const ga = d.port('g', 'R', (ya - y0) / gh), gb = d.port('g', 'R', (yb - y0) / gh);
  d.edge('g', 'pa', ga, d.port('pa', 'L', 40 / PH));
  d.edge('pa', 'g', d.port('pa', 'L', 96 / PH), gb, { dash: true, arrow: 'open' });
  d.lbl('Pause', (gx + SW + 48 + px) / 2, ya, 'a');
  d.lbl('Resume', (gx + SW + 48 + px) / 2, yb, 'b');
  // next check bus
  const busY = y0 + gh + 32;
  d.edge('g', 'chk', d.port('g', 'B', 0.5), d.port('chk', 'B', 0.5), { via: [[gx + (SW + 48) / 2, busY], [CX + CW / 2, busY]] });
  d.lbl('Next check', (CX + CW / 2 + gx + (SW + 48) / 2) / 2, busY, 'b');
  d.note('n', 48, Math.round(midY + 72), 296, 'Every failure counts towards back-off, so the next try comes later (with jitter). Invalid is set before any request, when the watch\'s configuration does not parse.');
  d.legend([{ kind: 'accent', label: 'State' }, { kind: 'process', label: 'Pause' }, { kind: 'line', label: 'Event' }, { kind: 'dashline', label: 'Return' }]);
  save('uml-state-watch', d, busY + 40);
}
