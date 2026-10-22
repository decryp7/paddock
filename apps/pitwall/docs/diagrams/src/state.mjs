import { P, save, link, dot } from './common.mjs';

let Y = 168;
const W = 216, G = 112, H = 104;
const cx = (i) => 96 + i * (W + G);
function frame(title, desc, w) { return P({ title, kind: 'UML · State machine diagram', desc, w }); }
const E = (d, a, sa, ta, b, sb, tb, via = [], o = {}) => d.edge(a, b, d.port(a, sa, ta), d.port(b, sb, tb), { via, ...o });
const states = (d, list) => list.forEach(([id, c, r, name, desc, h = H, w = W, role = 'accent']) => d.node(id, cx(c), Y + r * (H + 96), w, h, role, { name, desc }));

// ---------- download ----------
{
  const d = frame('States of a download', 'Model files and engine builds share one download record. The bytes arrive, the SHA-256 matches, then the file is proven to be what it claims; a failure always says what to do.', 1344);
  states(d, [
    ['q', 0, 0, 'Queued', 'Accepted; the URL is parsed against the allow-list.', 128],
    ['dl', 1, 0, 'Downloading', 'Resumes from the .part file; SHA-256 checked at the end.', 128],
    ['vf', 2, 0, 'Verifying', 'Models: the file must parse as GGUF. Engine builds: unpack, then --version and --list-devices.', 128],
    ['dn', 3, 0, 'Done', 'The model is in the library, or the build is active.', 128],
    ['cn', 1, 1, 'Cancelled', 'The partial file is kept for a later resume.'],
    ['fl', 2, 1, 'Failed', 'Offline, gated, disk full, SHA-256 mismatch (file deleted), not a GGUF or failed self-test (build discarded, previous stays).', 128, W + G + W],
  ]);
  dot(d, 'ini', 48, Y + 56, 'start');
  E(d, 'ini', 'R', 0.5, 'q', 'L', 0.5);
  link(d, 'q', 'R', 'dl', 'L', { label: 'Starts' });
  link(d, 'dl', 'R', 'vf', 'L', { label: 'Bytes, SHA ok' });
  link(d, 'vf', 'R', 'dn', 'L', { label: 'Checks pass' });
  link(d, 'dl', 'B', 'cn', 'T', { label: 'Cancel' });
  const ym = Y + H + 48, xd = cx(1) + W * 0.9, xf = cx(2) + 48;
  E(d, 'dl', 'B', 0.9, 'fl', 'T', 48 / (2 * W + G), [[Math.round(xd), ym], [Math.round(xf), ym]]);
  d.lbl('Network, HTTP, checksum', xd + 8, ym, 'br', { maxw: 160 });
  link(d, 'vf', 'B', 'fl', 'T', { label: 'Not a GGUF', t: [0.5, (W / 2) / (2 * W + G)] });
  d.legend([{ kind: 'accent', label: 'State' }, { kind: 'line', label: 'Event' }]);
  save('uml-state-download', d, d.rect.fl[1] + d.rect.fl[3]);
}

// ---------- hot model ----------
{
  Y = 248;
  const d = frame('States of a model in memory', 'Loaded on its first request, released when idle, under pressure or on request. A model that is answering is never released; a pinned one only under critical pressure (ADR 0006).', 1376);
  states(d, [
    ['nl', 0, 0, 'Not loaded', 'No process, no memory held. The file stays in the library; a dead engine lands here.'],
    ['ld', 1, 0, 'Loading', 'plan_load, spawn the engine, wait for /health, read /props.'],
    ['rd', 2, 0, 'Ready', 'In memory and idle; the idle countdown runs. Context is cleared half way.'],
    ['bs', 3, 0, 'Busy', 'Requests are running; never released.'],
    ['pn', 2, 1, 'Ready, pinned', '“Keep loaded”: the idle rule skips it.'],
  ]);
  const top = Y, bot = Y + H;
  dot(d, 'ini', cx(0) - 40, top - 56, 'start');
  E(d, 'ini', 'B', 0.5, 'nl', 'T', 0.1, [[cx(0) - 32, top - 24], [Math.round(cx(0) + W * 0.1), top - 24]]);
  link(d, 'nl', 'R', 'ld', 'L', { label: 'First request' });
  link(d, 'ld', 'R', 'rd', 'L', { label: 'Healthy' });
  link(d, 'rd', 'R', 'bs', 'L', { label: 'Request starts' });
  E(d, 'bs', 'T', 0.5, 'rd', 'T', 0.85, [[Math.round(cx(3) + W / 2), top - 28], [Math.round(cx(2) + W * 0.85), top - 28]]);
  d.lbl('Last request ends', (cx(2) + W * 0.85 + cx(3) + W / 2) / 2, top - 28, 'a', { maxw: 180 });
  E(d, 'rd', 'T', 0.3, 'nl', 'T', 0.7, [[Math.round(cx(2) + W * 0.3), top - 56], [Math.round(cx(0) + W * 0.7), top - 56]]);
  d.lbl('Idle, pressure, unload', (cx(0) + W * 0.7 + cx(2) + W * 0.3) / 2, top - 56, 'a', { maxw: 200 });
  const yb = Y + H + 48;
  E(d, 'ld', 'B', 0.5, 'nl', 'B', 0.7, [[Math.round(cx(1) + W / 2), yb], [Math.round(cx(0) + W * 0.7), yb]]);
  d.lbl('Start failed', (cx(1) + W / 2 + cx(0) + W * 0.7) / 2, yb, 'b', { maxw: 160 });
  link(d, 'rd', 'B', 'pn', 'T', { label: 'Pin', t: [0.3, 0.3] });
  link(d, 'pn', 'T', 'rd', 'B', { label: 'Unpin', t: [0.7, 0.7] });
  const yp = Y + (H + 96) + H / 2;
  E(d, 'pn', 'L', 0.5, 'nl', 'B', 0.3, [[Math.round(cx(0) + W * 0.3), yp]]);
  d.lbl('Critical pressure, unload', cx(1) - 24, yp, 'a', { maxw: 160 });
  d.note('n', cx(3), Y + (H + 96), W, 'Busy and Loading models are skipped by every release rule. Slots are chosen at load time; their memory is part of the fit check.', 128);
  d.legend([{ kind: 'accent', label: 'State' }, { kind: 'line', label: 'Event' }]);
  save('uml-state-hot-model', d, d.rect.pn[1] + d.rect.pn[3]);
}

// ---------- pairing ----------
{
  Y = 168;
  const d = frame('States of a pairing request', 'An app is not trusted until the user approves its six-digit code in Pitwall. The key is handed over once; an unanswered request expires after five minutes (ADR 0007, 0009).', 1120);
  states(d, [
    ['wt', 0, 0, 'Waiting', 'POST /x/v1/pair accepted; at most 8 pending; the code is shown in Pitwall. Polls answer pending.', 128],
    ['ap', 1, 0, 'Approved', 'The user approved with permissions; a key was created and its hash stored.', 128],
    ['dv', 2, 0, 'Delivered', 'The poll received the key once; later polls get key: null.', 128],
    ['dn', 0, 1, 'Denied', 'The user denied it. The poll answers denied.', 104],
    ['ex', 1, 1, 'Expired', 'Five minutes passed without an answer. The poll answers denied.', 104],
  ]);
  dot(d, 'ini', 48, Y + 64 - 8, 'start');
  E(d, 'ini', 'R', 0.5, 'wt', 'L', 0.5);
  link(d, 'wt', 'R', 'ap', 'L', { label: 'Approve' });
  link(d, 'ap', 'R', 'dv', 'L', { label: 'Key fetched' });
  link(d, 'wt', 'B', 'dn', 'T', { label: 'Deny', t: [0.3, 0.3] });
  const ym = Y + 128 + 40;
  E(d, 'wt', 'B', 0.85, 'ex', 'T', 0.2, [[Math.round(cx(0) + W * 0.85), ym], [Math.round(cx(1) + W * 0.2), ym]]);
  d.lbl('5 minutes pass', cx(1) + W * 0.2 + 8, ym, 'br', { maxw: 160 });
  d.note('n', cx(2), Y + 128 + 96, W, 'The app keeps the key in the OS credential store. A known app name adds a key and replaces its permissions; the owner can revoke any key.', 128);
  d.legend([{ kind: 'accent', label: 'State' }, { kind: 'line', label: 'Event' }]);
  save('uml-state-pairing', d, Math.max(d.rect.n[1] + d.rect.n[3], d.rect.ex[1] + d.rect.ex[3]));
}
