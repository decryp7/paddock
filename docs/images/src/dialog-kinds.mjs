// Mockups of the dialog kinds and their states in the shared DialogFrame (ADR 0026), light and dark, plus an anatomy
// sheet. Drawn in the Paddock tokens (`ui/projects/paddock-ui/scss/_tokens.scss`): every kind in graphite, the neutral
// demo accent, so no app's colour stands for the family; the tool shows Spotter's settings and so uses Spotter's accent,
// which its caption names. Each image carries a caption (kind, OS order, accent). Widths are the ADR's steps (400, 560, 720;
// the tool at its preferred 960 x 680). Button order follows ADR 0026 §2: macOS and Linux put Cancel then the primary on
// the right; Windows puts the primary then Cancel. Concepts for review, not product screens.
//   node docs/images/src/dialog-kinds.mjs <out-dir>    writes <scene>-<theme>.svg and sizes.json
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

// Token values (`--pd-*`): page is the desktop behind the window, title the OS title bar; everything inside the window is a
// token. `side` is --pd-bg (the tool's sidebar), `info` / `infoTint` --pd-info and its tint.
const T = {
  light: {
    page: '#dfe3e9', win: '#f0f2f5', field: '#ffffff', title: '#e8ebef', titleText: '#4f5764', hair: 'rgba(15,20,30,0.10)',
    strong: 'rgba(15,20,30,0.22)', control: '#6b7380', text: '#14171c', muted: '#4f5764', subtle: '#5f6775', dangerFill: '#c42b2b',
    danger: '#c42b2b', dangerTint: '#fce6e6', info: '#1f5fa8', infoTint: '#e4eef9', shadow: 'rgba(15,20,30,0.22)', side: '#f4f5f7',
    accents: {
      graphite: { text: '#4f5764', fill: '#4f5764', on: '#ffffff', control: '#4f5764', onControl: '#ffffff' },
      spotter: { text: '#0b7a6e', fill: '#0e8577', on: '#ffffff', control: '#0e8577', onControl: '#ffffff' },
    },
  },
  dark: {
    page: '#0b0c0f', win: '#1b1e23', field: '#15171b', title: '#22262c', titleText: '#a0a7b2', hair: 'rgba(255,255,255,0.08)',
    strong: 'rgba(255,255,255,0.16)', control: '#838b98', text: '#eceef1', muted: '#a0a7b2', subtle: '#8b93a0', dangerFill: '#c42b2b',
    danger: '#ff6b6b', dangerTint: 'rgba(255,107,107,0.14)', info: '#7db2e8', infoTint: 'rgba(125,178,232,0.14)', shadow: 'rgba(0,0,0,0.6)',
    side: '#0e0f12',
    accents: {
      graphite: { text: '#a6a8ad', fill: '#a6a8ad', on: '#16181c', control: '#a6a8ad', onControl: '#0e0f12' },
      spotter: { text: '#2cc4b0', fill: '#0e8577', on: '#ffffff', control: '#2cc4b0', onControl: '#0e0f12' },
    },
  },
};

const FONT = `-apple-system, 'SF Pro Text', 'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif`;
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
/** Approximate text width of the system UI font (measured against SF Pro in Chrome: about 0.47 em per glyph at 400, 0.52 at 500-600). */
const tw = (s, size = 13, w = 400) => s.length * size * (w >= 500 ? 0.52 : 0.47);
const text = (x, y, s, { size = 13, w = 400, fill, anchor = 'start' } = {}) =>
  `<text x="${x}" y="${y}" font-family="${FONT}" font-size="${size}" font-weight="${w}" fill="${fill}" text-anchor="${anchor}">${esc(s)}</text>`;
const rect = (x, y, w, h, fill, { rx = 0, stroke, sw = 1, extra = '' } = {}) =>
  `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${rx}" fill="${fill}"${stroke ? ` stroke="${stroke}" stroke-width="${sw}"` : ''}${extra}/>`;
/** Greedy word wrap to `max` px. */
function wrap(s, max, size = 13) {
  const out = [];
  let line = '';
  for (const word of s.split(' ')) {
    const next = line ? `${line} ${word}` : word;
    if (tw(next, size) > max && line) {
      out.push(line);
      line = word;
    } else line = next;
  }
  if (line) out.push(line);
  return out;
}
const para = (x, y, s, max, fill, lh = 20) => wrap(s, max).map((l, i) => text(x, y + i * lh, l, { fill })).join('');
const lines = (s, max) => wrap(s, max).length;
const btnWidth = (label, busy = false) => Math.round(tw(label, 13, 500) + 28 + (busy ? 22 : 0));

/** A footer button. `role`: primary | secondary | cancel | destructive; `focus` draws the ring; `disabled`; `busy` a spinner. */
function button(c, ac, x, y, a) {
  const { label, focus = false, disabled = false, busy = false } = a;
  const role = a.plain ? 'secondary' : a.role;
  const w = btnWidth(label, busy), h = 32;
  const filled = role === 'primary' || role === 'destructive';
  const fill = role === 'primary' ? ac.fill : role === 'destructive' ? c.dangerFill : c.field;
  const ink = role === 'primary' ? ac.on : role === 'destructive' ? '#ffffff' : c.text;
  let s = '';
  if (focus) s += rect(x - 4, y - 4, w + 8, h + 8, 'none', { rx: 9, stroke: ac.text, sw: 2 });
  s += `<g${disabled ? ' opacity="0.5"' : ''}>`;
  s += rect(x + 0.5, y + 0.5, w - 1, h - 1, fill, { rx: 6, stroke: filled ? null : c.strong });
  if (busy) {
    const cx = x + 18, cy = y + 16;
    s += `<circle cx="${cx}" cy="${cy}" r="6" fill="none" stroke="${ink}" stroke-opacity="0.35" stroke-width="2"/>`;
    s += `<path d="M${cx} ${cy - 6} a6 6 0 0 1 6 6" fill="none" stroke="${ink}" stroke-width="2" stroke-linecap="round"/>`;
    s += text(x + 22 + (w - 22) / 2, y + 20.5, label, { w: 500, fill: ink, anchor: 'middle' });
  } else s += text(x + w / 2, y + 20.5, label, { w: 500, fill: ink, anchor: 'middle' });
  return { svg: `${s}</g>`, w };
}

/**
 * Lays out declared actions as `layoutActions` does (ADR 0026 §2, D1). macOS and Linux: [lead] … [secondary] [cancel]
 * [primary|destructive]. Windows: [lead] … [primary|destructive] [secondary] [cancel]. A destructive action beside a
 * primary is a plain outlined button: a lead action on macOS and Linux, between the primary and Cancel on Windows.
 */
function footer(c, ac, x, y, width, actions, os = 'mac') {
  // Beside a primary, a destructive action is a plain outlined button (one filled button per dialog). On macOS and
  // Linux it is a lead action; on Windows it sits between the primary and Cancel (Save, Don't save, Cancel).
  const hasPrimary = actions.some((a) => a.role === 'primary');
  actions = actions.map((a) => (a.role === 'destructive' && hasPrimary ? { ...a, plain: true, lead: os !== 'win' } : a));
  const lead = actions.filter((a) => a.lead);
  const rank = os === 'win'
    ? { primary: 0, destructive: hasPrimary ? 1 : 0, secondary: 1, cancel: 2 }
    : { secondary: 0, cancel: 1, primary: 2, destructive: 2 };
  const trail = actions.filter((a) => !a.lead).sort((p, q) => rank[p.role] - rank[q.role]);
  let s = '', lx = x;
  for (const a of lead) {
    const b = button(c, ac, lx, y, a);
    s += b.svg;
    lx += b.w + 8;
  }
  let rx = x + width - (trail.reduce((n, a) => n + btnWidth(a.label, a.busy), 0) + 8 * (trail.length - 1));
  for (const a of trail) {
    const b = button(c, ac, rx, y, a);
    s += b.svg;
    rx += b.w + 8;
  }
  return s;
}

let clipSeq = 0;
/**
 * A dialog window (no title-bar buttons, ADR 0009 §1): content `w` x `h` on a padded backdrop. `os`: 'mac' draws a
 * centred title in a 28 px bar and 10 px corners; 'win' a left-aligned title (no icon) in a 32 px bar and 8 px corners.
 * `caption` is a line under the window on the backdrop (kind, OS, accent). Ids are sequential per file, so the output is
 * deterministic.
 */
function windowFrame(c, title, w, h, body, { pad = 40, left = pad, right = pad, top = pad, bottom, extra = '', os = 'mac', caption = null } = {}) {
  bottom ??= caption ? 64 : pad;
  const TB = os === 'win' ? 32 : 28, R = os === 'win' ? 8 : 10;
  const W = w + left + right, H = h + TB + top + bottom;
  if (caption && tw(caption, 12) > W - 2 * left) throw new Error(`caption wider than its image: ${caption}`);
  const id = `w${++clipSeq}`;
  const titleSvg = os === 'win'
    ? text(left + 12, top + 20.5, title, { size: 12, fill: c.titleText })
    : text(left + w / 2, top + 18.5, title, { size: 13, w: 600, fill: c.titleText, anchor: 'middle' });
  return {
    W, H,
    svg: `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">
<defs><filter id="${id}-shadow" x="-20%" y="-20%" width="140%" height="150%"><feDropShadow dx="0" dy="12" stdDeviation="16" flood-color="${c.shadow}"/></filter>
<clipPath id="${id}-clip"><rect x="${left}" y="${top}" width="${w}" height="${h + TB}" rx="${R}"/></clipPath></defs>
${rect(0, 0, W, H, c.page)}
<g filter="url(#${id}-shadow)">${rect(left, top, w, h + TB, c.win, { rx: R })}</g>
<g clip-path="url(#${id}-clip)">
${rect(left, top, w, TB, c.title)}${rect(left, top + TB - 0.5, w, 0.5, c.hair)}
${titleSvg}
<g transform="translate(${left},${top + TB})">${body}</g>
</g>
${rect(left + 0.5, top + 0.5, w - 1, h + TB - 1, 'none', { rx: R, stroke: c.strong })}
${caption ? text(left, H - 20, caption, { size: 12, fill: c.muted }) : ''}
${extra}</svg>`,
  };
}

const field = (c, x, y, w, value, { focus = false, ac, disabled = false } = {}) =>
  (focus ? rect(x - 3, y - 3, w + 6, 36, 'none', { rx: 8, stroke: ac.text, sw: 2 }) : '') +
  `<g${disabled ? ' opacity="0.6"' : ''}>` +
  rect(x + 0.5, y + 0.5, w - 1, 29, c.field, { rx: 6, stroke: focus ? ac.text : c.control }) +
  text(x + 10, y + 19.5, value, { fill: c.text }) + '</g>';

const checkbox = (c, ac, x, y, label, checked = false, disabled = false) =>
  `<g${disabled ? ' opacity="0.6"' : ''}>` +
  rect(x + 0.5, y + 0.5, 15, 15, checked ? ac.control : c.field, { rx: 4, stroke: checked ? ac.control : c.control }) +
  (checked ? `<path d="M${x + 4} ${y + 8.5} l3 3 l5.5 -6.5" fill="none" stroke="${ac.onControl}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>` : '') +
  text(x + 24, y + 12.5, label, { fill: c.text }) + '</g>';

const selectBox = (c, x, y, w, value, disabled = false) =>
  `<g${disabled ? ' opacity="0.6"' : ''}>` + rect(x + 0.5, y + 0.5, w - 1, 29, c.field, { rx: 6, stroke: c.control }) + text(x + 10, y + 19.5, value, { fill: c.text }) +
  `<path d="M${x + w - 22} ${y + 13} l4 4 l4 -4" fill="none" stroke="${c.muted}" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></g>`;

/** The 32 px glyph of a decision: `danger` (!) or `info` (i), the status colour on its tint (never an app accent). */
function glyph(c, x, y, kind) {
  const fg = kind === 'danger' ? c.danger : c.info;
  const disc = `<circle cx="${x + 16}" cy="${y + 16}" r="16" fill="${kind === 'danger' ? c.dangerTint : c.infoTint}"/>`;
  const mark = kind === 'danger'
    ? `<path d="M${x + 16} ${y + 8.5} v9" stroke="${fg}" stroke-width="2.2" stroke-linecap="round"/><circle cx="${x + 16}" cy="${y + 22.5}" r="1.4" fill="${fg}"/>`
    : `<circle cx="${x + 16}" cy="${y + 9.5}" r="1.4" fill="${fg}"/><path d="M${x + 16} ${y + 14} v9" stroke="${fg}" stroke-width="2.2" stroke-linecap="round"/>`;
  return disc + mark;
}

// ---------- scenes ----------

const P = 20;
/** Footer band of the viewer and the tool: a hairline, then 16 px, 32 px buttons, 16 px. */
const FH = 64;

/** A decision: glyph, heading (the question), message, optional extra body, then the footer. */
function decisionScene(c, { heading, message, glyphKind = 'danger', actions, os = 'mac', w = 400, extraBody = null, accent = 'graphite' }) {
  const ac = c.accents[accent];
  const X = P + 48, maxText = w - X - P;
  let b = glyph(c, P, P, glyphKind);
  b += text(X, P + 13, heading, { size: 15, w: 600, fill: c.text });
  b += para(X, P + 37, message, maxText, c.muted);
  let y = P + 37 + (lines(message, maxText) - 1) * 20 + 8;
  if (extraBody) {
    b += extraBody.svg(X, y + 10, maxText);
    y += 10 + extraBody.h;
  }
  const fy = y + 20;
  b += footer(c, ac, P, fy, w - 2 * P, actions, os);
  return { b, w, h: fy + 32 + P, fy };
}

const revoke = (os) => ({
  heading: 'Revoke “debrief-laptop”?',
  message: 'Debrief on that laptop loses access to your models. You can create a new key at any time.',
  actions: [{ label: 'Cancel', role: 'cancel', focus: true }, { label: 'Revoke key', role: 'destructive' }], os,
});
const unsaved = (os) => ({
  glyphKind: 'info', heading: 'Save changes to “orders.debrief”?',
  message: 'Your changes are lost if you don’t save them.',
  actions: [{ label: 'Don’t save', role: 'destructive', lead: true }, { label: 'Cancel', role: 'cancel' }, { label: 'Save', role: 'primary', focus: true }], os,
});
const ORDER = { mac: 'macOS and Linux order', win: 'Windows order' };
const NEUTRAL = 'neutral demo accent';
const scene = (c, title, spec, caption) => {
  const d = decisionScene(c, spec);
  return windowFrame(c, title, d.w, d.h, d.b, { os: spec.os, caption });
};

const decision = (c) => scene(c, 'Revoke key', revoke('mac'), `Decision, narrow 400 · ${ORDER.mac} · ${NEUTRAL}`);
const decisionWindows = (c) => scene(c, 'Revoke key', revoke('win'), `Decision, narrow 400 · ${ORDER.win} · ${NEUTRAL}`);
const saveDontSave = (c) => scene(c, 'Unsaved changes', unsaved('mac'), `Save prompt, narrow 400 · ${ORDER.mac} · ${NEUTRAL}`);
const saveDontSaveWindows = (c) => scene(c, 'Unsaved changes', unsaved('win'), `Save prompt, narrow 400 · ${ORDER.win} · ${NEUTRAL}`);

/** A required choice: no Cancel; Escape and the OS close do nothing; Copy is a lead action that does not close. */
function requiredChoice(c) {
  const secret = {
    h: 30 + 28,
    svg: (x, y, max) => rect(x + 0.5, y + 0.5, max - 1, 29, c.field, { rx: 6, stroke: c.control }) +
      text(x + 10, y + 19.5, 'pw_live_7Hq2…e91F', { fill: c.text }) +
      text(x, y + 50, 'Paste it into Debrief’s settings now.', { size: 12, fill: c.subtle }),
  };
  return scene(c, 'New key', {
    glyphKind: 'info', heading: 'Copy the key for “debrief-laptop”',
    message: 'This is the only time Pitwall shows it.', extraBody: secret,
    actions: [{ label: 'Copy key', role: 'secondary', lead: true }, { label: 'I’ve saved it', role: 'primary', focus: true }],
  }, `Required choice, narrow 400 · no Cancel · ${NEUTRAL}`);
}

/** The form; `state`: idle | busy | error (a rejected async action, kept open with the message and Try again). */
function formScene(c, state = 'idle') {
  const ac = c.accents.graphite, w = 560;
  const busy = state === 'busy';
  let b = '', y = P;
  if (state === 'error') {
    const msg = 'Couldn’t create the app: Pitwall’s service didn’t answer. Your entries are kept.';
    const max = w - 2 * P - 36 - btnWidth('Try again') - 20;
    const hh = 22 + lines(msg, max) * 20;
    b += rect(P, y, w - 2 * P, hh, c.dangerTint, { rx: 6 });
    b += `<circle cx="${P + 18}" cy="${y + 21}" r="8" fill="none" stroke="${c.danger}" stroke-width="1.6"/><path d="M${P + 18} ${y + 17} v4.5" stroke="${c.danger}" stroke-width="1.6" stroke-linecap="round"/><circle cx="${P + 18}" cy="${y + 25}" r="1" fill="${c.danger}"/>`;
    b += para(P + 36, y + 26, msg, max, c.text);
    b += button(c, ac, w - P - 10 - btnWidth('Try again'), y + (hh - 32) / 2, { label: 'Try again', role: 'secondary', focus: true }).svg;
    y += hh + 16;
  }
  b += text(P, y + 13, 'Name', { size: 12, w: 600, fill: c.text });
  y += 22;
  b += field(c, P, y, w - 2 * P, 'Debrief on the laptop', { focus: state === 'idle', ac, disabled: busy });
  y += 36;
  b += text(P, y + 12, 'Shown in the request log and on the key.', { size: 12, fill: c.subtle });
  y += 32;
  b += text(P, y + 13, 'Models it may use', { size: 12, w: 600, fill: c.text });
  y += 22;
  b += selectBox(c, P, y, w - 2 * P, 'All installed models', busy);
  y += 48;
  b += checkbox(c, ac, P, y, 'Allow embeddings', true, busy);
  y += 26;
  b += checkbox(c, ac, P, y, 'Allow requests from other computers on this network', false, busy);
  y += 40;
  b += footer(c, ac, P, y, w - 2 * P, [
    { label: 'Cancel', role: 'cancel', disabled: busy },
    busy ? { label: 'Creating…', role: 'primary', busy: true } : { label: 'Create app', role: 'primary' },
  ]);
  const what = { idle: 'Form, medium 560', busy: 'Form, busy: the pressed action spins, all else inert', error: 'Form, rejected: input kept, Try again focused' }[state];
  return windowFrame(c, 'New app', w, y + 32 + P, b, { caption: `${what} · ${NEUTRAL}` });
}
const form = (c) => formScene(c, 'idle');
const formBusy = (c) => formScene(c, 'busy');
const formError = (c) => formScene(c, 'error');

/** Viewer: modeless, opens at its preferred 720 x 560 and grows with its content to the cap; the footer never scrolls. */
function viewer(c) {
  const ac = c.accents.graphite, w = 720, h = 560;
  const bodyH = h - FH, max = w - 2 * P;
  let b = text(P, P + 12, 'ERROR · orders-api · 14:02:31.418', { size: 12, w: 600, fill: c.subtle });
  const blocks = [
    ['p', 'The request to /orders/7731 failed because the payment service timed out after 30 s. The same timeout appears 41 times in the last hour, always after the connection pool reported 64 of 64 connections in use.'],
    ['p', 'Most likely cause: the pool is too small for the 14:00 batch import, which holds connections for the length of each file. The first timeout follows the batch start by 2.1 s on every occasion in this log.'],
    ['h', 'What to try next'],
    ['li', 'Raise the pool size, or give the batch import a pool of its own.'],
    ['li', 'Compare with the 13:00 run, which finished without timeouts.'],
    ['li', 'Check whether the payment service logged the same requests.'],
    ['li', 'Look for client retries during the batch window.'],
    ['h', 'Evidence'],
    ['li', '14:00:02.114  batch-import started 312 files (orders-api, pid 4410).'],
    ['li', '14:00:04.230  pool: 64 of 64 connections in use, 17 waiting.'],
    ['li', '14:00:34.231  payment-client: timeout after 30 000 ms (/orders/7712).'],
    ['li', '14:02:31.418  payment-client: timeout after 30 000 ms (/orders/7731).'],
    ['n', 'Based on 1 284 events in orders-api.log between 13:00 and 14:05.'],
  ];
  let y = P + 44;
  for (const [k, s] of blocks) {
    if (k === 'h') {
      y += 6;
      b += text(P, y, s, { w: 600, fill: c.text });
      y += 26;
    } else if (k === 'li') {
      b += `<circle cx="${P + 4}" cy="${y - 4}" r="2" fill="${c.muted}"/>` + para(P + 16, y, s, max - 16, c.text);
      y += lines(s, max - 16) * 20 + 2;
    } else if (k === 'n') {
      y += 14;
      b += text(P, y, s, { size: 12, fill: c.subtle });
      y += 20;
    } else {
      b += para(P, y, s, max, c.text);
      y += lines(s, max) * 20 + 12;
    }
  }
  if (y > bodyH - P) throw new Error(`viewer content (${y}) overflows its preferred height (${bodyH - P}): trim the content`);
  b += rect(0, bodyH, w, 1, c.hair);
  b += footer(c, ac, P, bodyH + 16, w - 2 * P, [
    { label: 'Copy explanation', role: 'secondary', lead: true },
    { label: 'Show cited rows', role: 'secondary', lead: true },
    { label: 'Close', role: 'cancel', focus: true },
  ]);
  return windowFrame(c, 'Explain event', w, h, b, {
    caption: `Viewer, wide 720 at its preferred 560 high · grows with its content to the cap, then the body scrolls · ${NEUTRAL}`,
  });
}

/** A switch as `.pd-switch` draws it: 32 x 18, on = the accent control fill with the on-control handle. */
const toggle = (c, ac, x, y, on) =>
  rect(x + 0.5, y + 0.5, 31, 17, on ? ac.control : c.field, { rx: 8.5, stroke: on ? ac.control : c.control }) +
  `<circle cx="${x + (on ? 23 : 9)}" cy="${y + 9}" r="6" fill="${on ? ac.onControl : c.muted}"/>`;

/** Tool: modeless, user-sized, its own layout, drawn at its preferred 960 x 680; the footer keeps a status line and Close. */
function tool(c) {
  const ac = c.accents.spotter, w = 960, h = 680, SW = 216;
  let b = rect(0, 0, SW, h - FH, c.side) + rect(SW - 0.5, 0, 0.5, h - FH, c.hair);
  ['General', 'Appearance', 'Notifications', 'Storage', 'Network', 'Updates', 'Diagnostics'].forEach((s, i) => {
    const y = 12 + i * 32;
    if (i === 3) {
      // Selected navigation item (design guide §2): 14 % accent tint, 600 label, a 2 px accent bar.
      b += rect(8, y, SW - 16, 28, ac.text, { rx: 6, extra: ' fill-opacity="0.14"' }) + rect(8, y + 6, 2, 16, ac.text, { rx: 1 });
    }
    b += text(22, y + 18.5, s, { fill: i === 3 ? c.text : c.muted, w: i === 3 ? 600 : 400 });
  });
  const X = SW + 32, R = w - 32, RW = R - X;
  b += text(X, 40, 'Storage', { size: 18, w: 600, fill: c.text });
  b += text(X, 64, 'Where Spotter keeps its data, and how much it may use.', { fill: c.muted });
  const rows = [
    ['Data folder', (y) => text(R - btnWidth('Change…') - 12, y + 26, '~/Library/Application Support/Spotter', { fill: c.muted, anchor: 'end' }) +
      button(c, ac, R - btnWidth('Change…'), y + 6, { label: 'Change…', role: 'secondary' }).svg],
    ['Used', (y) => text(R, y + 26, '2.4 GB of 10 GB', { fill: c.muted, anchor: 'end' })],
    ['Keep history for', (y) => selectBox(c, R - 180, y + 7, 180, '30 days')],
    ['Keep snapshots', (y) => selectBox(c, R - 180, y + 7, 180, 'The last 20')],
    ['Clean up old logs when the disk is low', (y) => toggle(c, ac, R - 32, y + 13, true)],
  ];
  rows.forEach(([k, v], i) => {
    const y = 92 + i * 44;
    b += rect(X, y + 43.5, RW, 0.5, c.hair) + text(X, y + 26, k, { fill: c.text }) + v(y);
  });
  const by = 92 + rows.length * 44 + 28;
  b += text(X, by, 'Space used', { size: 12, w: 600, fill: c.text });
  b += rect(X, by + 12, RW, 8, c.hair, { rx: 4 }) + rect(X, by + 12, RW * 0.24, 8, ac.text, { rx: 4 });
  b += text(X, by + 40, 'Logs 1.6 GB · Snapshots 0.7 GB · Cache 0.1 GB', { size: 12, fill: c.subtle });
  b += button(c, ac, X, by + 60, { label: 'Clear cache', role: 'secondary' }).svg;
  // The footer spans the window, its Close lined up with the content column's right edge (32 px in).
  b += rect(0, h - FH, w, 1, c.hair) + text(P, h - FH + 36, 'Changes are saved as you make them.', { size: 12, fill: c.subtle });
  b += footer(c, ac, P, h - FH + 16, R - P, [{ label: 'Close', role: 'cancel' }]);
  return windowFrame(c, 'Spotter settings', w, h, b, { caption: 'Tool at its preferred 960 × 680, user-sized · Spotter’s settings in Spotter’s accent' });
}

/** The show-deadline fallback (ADR 0026 §4): a form not ready after about 1 s shows Loading with a Close. */
function loading(c) {
  const ac = c.accents.graphite, w = 560, h = 240;
  const cx = w / 2, cy = 84;
  let b = `<circle cx="${cx}" cy="${cy}" r="11" fill="none" stroke="${c.subtle}" stroke-opacity="0.35" stroke-width="2.4"/>`;
  b += `<path d="M${cx} ${cy - 11} a11 11 0 0 1 11 11" fill="none" stroke="${c.muted}" stroke-width="2.4" stroke-linecap="round"/>`;
  b += text(cx, cy + 36, 'Loading…', { fill: c.text, anchor: 'middle' });
  b += text(cx, cy + 56, 'This window is still starting. You can close it and open it again.', { size: 12, fill: c.subtle, anchor: 'middle' });
  b += footer(c, ac, P, h - 32 - P, w - 2 * P, [{ label: 'Close', role: 'cancel', focus: true }]);
  return windowFrame(c, 'New app', w, h, b, { caption: `Form, medium 560, not ready after about 1 s: Loading and Close · ${NEUTRAL}` });
}

/** The frame's anatomy: a decision with its measurements, annotated in neutral ink (text and muted text). */
function anatomy(c) {
  const { b, w, h, fy } = decisionScene(c, revoke('mac'));
  // Side margins from the label widths, so the backdrop has the same 32 px beyond the longest label on both sides.
  const leftLabels = [['Body', 600], ['scrolls only past the cap', 400], ['Footer', 600], ['declared actions, never scrolls', 400]];
  const rightLabels = [['20 px padding all round', 600], ['heading 15/600, text 13, 12 px gap', 400], ['Buttons 32 px, 8 px apart, order native per OS', 600],
    ['Enter = default (here Cancel; never a destructive action)', 400], ['Escape, Cmd/Ctrl+W = Cancel', 400]];
  const widest = (ls) => Math.max(...ls.map(([s, wt]) => tw(s, 12, wt)));
  const M = 32, L = Math.round(76 + widest(leftLabels) + M), R = Math.round(64 + widest(rightLabels) + M), TOP = 64, BOT = 84;
  const ox = L, oy = TOP + 28, A = c.muted;
  const ln = (x1, y1, x2, y2) => `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${A}" stroke-width="1"/>`;
  const head = (x, y, s, anchor = 'start') => text(x, y, s, { size: 12, w: 600, fill: c.text, anchor });
  const lab = (x, y, s, anchor = 'start') => text(x, y, s, { size: 12, fill: c.muted, anchor });
  let a = '';
  a += rect(ox + 0.5, oy + 0.5, w - 1, fy - 9, 'none', { stroke: A, extra: ' stroke-dasharray="4 3"' });
  a += rect(ox + 0.5, oy + fy - 7.5, w - 1, h - fy + 7, 'none', { stroke: A, extra: ' stroke-dasharray="4 3"' });
  a += ln(ox - 70, oy + 40, ox, oy + 40) + head(ox - 76, oy + 36, 'Body', 'end') + lab(ox - 76, oy + 52, 'scrolls only past the cap', 'end');
  a += ln(ox - 70, oy + fy + 16, ox, oy + fy + 16) + head(ox - 76, oy + fy + 12, 'Footer', 'end') + lab(ox - 76, oy + fy + 28, 'declared actions, never scrolls', 'end');
  a += lab(ox + w / 2, TOP - 14, 'OS title bar: no close, minimise or zoom (ADR 0009); title from the contract or setTitle()', 'middle');
  const rx = ox + w + 64;
  a += head(rx, oy + 16, '20 px padding all round') + lab(rx, oy + 32, 'heading 15/600, text 13, 12 px gap');
  a += head(rx, oy + fy + 2, 'Buttons 32 px, 8 px apart, order native per OS') + lab(rx, oy + fy + 18, 'Enter = default (here Cancel; never a destructive action)')
    + lab(rx, oy + fy + 34, 'Escape, Cmd/Ctrl+W = Cancel');
  const hx = ox + w + 24;
  a += ln(hx, oy, hx, oy + h) + ln(hx - 4, oy, hx + 4, oy) + ln(hx - 4, oy + h, hx + 4, oy + h);
  a += `<text x="${hx + 14}" y="${oy + h / 2}" font-family="${FONT}" font-size="12" font-weight="400" fill="${c.muted}" text-anchor="middle" transform="rotate(-90 ${hx + 14} ${oy + h / 2})">height: measured</text>`;
  const wy = oy + h + 20;
  a += ln(ox, wy, ox + w, wy) + ln(ox, wy - 4, ox, wy + 4) + ln(ox + w, wy - 4, ox + w, wy + 4);
  a += lab(ox + w / 2, wy + 20, 'Width: narrow 400, medium 560, wide 720 (ADR 0026 §1). OS minimum = measured min content:', 'middle')
    + lab(ox + w / 2, wy + 36, 'header + footer + two body lines high, the footer on one line wide', 'middle');
  return windowFrame(c, 'Revoke key', w, h, b, { left: L, right: R, top: TOP, bottom: BOT, extra: a });
}

const SCENES = { anatomy, decision, decisionWindows, saveDontSave, saveDontSaveWindows, requiredChoice, form, formBusy, formError, viewer, tool, loading };

const out = process.argv[2];
if (!out) throw new Error('usage: node dialog-kinds.mjs <out-dir>');
mkdirSync(out, { recursive: true });
const sizes = {};
for (const theme of ['light', 'dark']) {
  for (const [name, fn] of Object.entries(SCENES)) {
    clipSeq = 0;
    const m = fn(T[theme]);
    const file = `${name.replace(/[A-Z]/g, (ch) => `-${ch.toLowerCase()}`)}-${theme}`;
    writeFileSync(join(out, `${file}.svg`), m.svg);
    sizes[file] = [m.W, m.H];
  }
}
writeFileSync(join(out, 'sizes.json'), `${JSON.stringify(sizes, null, 2)}\n`);
console.log(Object.keys(sizes).length, 'scenes');
