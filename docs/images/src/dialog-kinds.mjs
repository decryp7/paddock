// Mockups of the four dialog kinds in the shared DialogFrame (ADR 0025), light and dark, plus an anatomy sheet. Drawn in
// the Paddock tokens (`ui/projects/paddock-ui/scss/_tokens.scss`), Pitwall accent for the decision and form, Debrief for the
// viewer, graphite for the tool. Concepts for review, not product screens.
//   node docs/images/src/dialog-kinds.mjs <out-dir>    writes <kind>-<theme>.svg (and anatomy-<theme>.svg)
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const T = {
  light: {
    page: '#dfe3e9', win: '#f0f2f5', field: '#ffffff', title: '#e8ebef', titleText: '#4f5764', hair: 'rgba(15,20,30,0.10)',
    strong: 'rgba(15,20,30,0.22)', control: '#6b7380', text: '#14171c', muted: '#4f5764', subtle: '#5f6775', dangerFill: '#c42b2b',
    danger: '#c42b2b', dangerTint: '#fce6e6', shadow: 'rgba(15,20,30,0.22)', side: '#e6e9ee', sel: 'rgba(79,87,100,0.14)',
    note: '#5f6775', anno: '#c2287c',
    accents: { pitwall: { text: '#6d3fd6', fill: '#7546e0', on: '#fff' }, debrief: { text: '#1d5fd1', fill: '#2f6fe0', on: '#fff' }, graphite: { text: '#4f5764', fill: '#4f5764', on: '#fff' } },
  },
  dark: {
    page: '#0b0c0f', win: '#1b1e23', field: '#15171b', title: '#22262c', titleText: '#a0a7b2', hair: 'rgba(255,255,255,0.08)',
    strong: 'rgba(255,255,255,0.16)', control: '#838b98', text: '#eceef1', muted: '#a0a7b2', subtle: '#8b93a0', dangerFill: '#c42b2b',
    danger: '#ff6b6b', dangerTint: 'rgba(255,107,107,0.14)', shadow: 'rgba(0,0,0,0.6)', side: '#15171b', sel: 'rgba(166,168,173,0.16)',
    note: '#8b93a0', anno: '#f063a8',
    accents: { pitwall: { text: '#a58bfa', fill: '#7546e0', on: '#fff' }, debrief: { text: '#5b9bff', fill: '#2f6fe0', on: '#fff' }, graphite: { text: '#a6a8ad', fill: '#a6a8ad', on: '#16181c' } },
  },
};

const FONT = `-apple-system, 'SF Pro Text', 'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif`;
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
/** Approximate text width at 13 px system UI (about 0.5 em per glyph); weight 600 is a little wider. */
const tw = (s, size = 13, w = 400) => s.length * size * (w >= 600 ? 0.56 : 0.52);
const text = (x, y, s, { size = 13, w = 400, fill, anchor = 'start', family = FONT } = {}) =>
  `<text x="${x}" y="${y}" font-family="${family}" font-size="${size}" font-weight="${w}" fill="${fill}" text-anchor="${anchor}">${esc(s)}</text>`;
const rect = (x, y, w, h, fill, { rx = 0, stroke, sw = 1, extra = '' } = {}) =>
  `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${rx}" fill="${fill}"${stroke ? ` stroke="${stroke}" stroke-width="${sw}"` : ''}${extra}/>`;

/** A button of the frame's footer. `role`: primary | secondary | cancel | destructive; `focus`: draws the focus ring. */
function button(c, ac, x, y, label, role, { focus = false, h = 32 } = {}) {
  const w = Math.round(tw(label, 13, 500) + 28);
  const fill = role === 'primary' ? ac.fill : role === 'destructive' ? c.dangerFill : c.field;
  const ink = role === 'primary' ? ac.on : role === 'destructive' ? '#ffffff' : c.text;
  const stroke = role === 'primary' || role === 'destructive' ? null : c.strong;
  let s = '';
  if (focus) s += rect(x - 4, y - 4, w + 8, h + 8, 'none', { rx: 9, stroke: ac.text, sw: 2 });
  s += rect(x + 0.5, y + 0.5, w - 1, h - 1, fill, { rx: 6, stroke });
  s += text(x + w / 2, y + h / 2 + 4.5, label, { w: 500, fill: ink, anchor: 'middle' });
  return { svg: s, w };
}

/** Lays out declared actions the way DialogFrame does: leading group at the start, then cancel, then the affirmative. */
function footer(c, ac, x, y, width, actions) {
  let s = '';
  const lead = actions.filter((a) => a.lead);
  const trail = actions.filter((a) => !a.lead);
  let lx = x;
  for (const a of lead) {
    const b = button(c, ac, lx, y, a.label, a.role, a);
    s += b.svg;
    lx += b.w + 8;
  }
  const widths = trail.map((a) => Math.round(tw(a.label, 13, 500) + 28));
  let rx = x + width - widths.reduce((p, q) => p + q, 0) - 8 * (trail.length - 1);
  trail.forEach((a) => {
    const b = button(c, ac, rx, y, a.label, a.role, a);
    s += b.svg;
    rx += b.w + 8;
  });
  return s;
}

/** An OS dialog window: title bar without buttons (ADR 0009 §1), content `w` x `h`, on a padded backdrop. */
function windowFrame(c, title, w, h, body, { pad = 40, left = pad, right = pad, top = pad, bottom = pad, extra = '' } = {}) {
  const W = w + left + right, H = h + 28 + top + bottom;
  const id = `clip${Math.random().toString(36).slice(2, 8)}`;
  return {
    W, H,
    svg: `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">
<defs><filter id="sh" x="-20%" y="-20%" width="140%" height="150%"><feDropShadow dx="0" dy="12" stdDeviation="16" flood-color="${c.shadow}"/></filter>
<clipPath id="${id}"><rect x="${left}" y="${top}" width="${w}" height="${h + 28}" rx="10"/></clipPath></defs>
${rect(0, 0, W, H, c.page)}
<g filter="url(#sh)">${rect(left, top, w, h + 28, c.win, { rx: 10 })}</g>
<g clip-path="url(#${id})">
${rect(left, top, w, 28, c.title)}${rect(left, top + 27.5, w, 0.5, c.hair)}
${text(left + w / 2, top + 18.5, title, { size: 13, w: 600, fill: c.titleText, anchor: 'middle' })}
<g transform="translate(${left},${top + 28})">${body}</g>
</g>
${rect(left + 0.5, top + 0.5, w - 1, h + 27, 'none', { rx: 10, stroke: c.strong })}
${extra}</svg>`,
  };
}

const field = (c, x, y, w, value, { placeholder = false, focus = false, ac, invalid = false } = {}) =>
  (focus ? rect(x - 3, y - 3, w + 6, 36, 'none', { rx: 8, stroke: ac.text, sw: 2 }) : '') +
  rect(x + 0.5, y + 0.5, w - 1, 29, c.field, { rx: 6, stroke: invalid ? c.danger : focus ? ac.text : c.control }) +
  text(x + 10, y + 19.5, value, { fill: placeholder ? c.subtle : c.text });

const checkbox = (c, ac, x, y, label, checked = false) =>
  rect(x + 0.5, y + 0.5, 15, 15, checked ? ac.fill : c.field, { rx: 4, stroke: checked ? ac.fill : c.control }) +
  (checked ? `<path d="M${x + 4} ${y + 8.5} l3 3 l5.5 -6.5" fill="none" stroke="${ac.on}" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>` : '') +
  text(x + 24, y + 12.5, label, { fill: c.text });

// ---------- the four kinds ----------

/** Decision: modal, fitted, the question as the heading, one sentence, destructive never the default. */
function decisionBody(c) {
  const ac = c.accents.pitwall, w = 400, P = 20;
  let b = '';
  // Alert icon: a 32 px danger glyph in its tint.
  b += `<circle cx="${P + 16}" cy="${P + 16}" r="16" fill="${c.dangerTint}"/><path d="M${P + 16} ${P + 8.5} v9" stroke="${c.danger}" stroke-width="2.2" stroke-linecap="round"/><circle cx="${P + 16}" cy="${P + 22.5}" r="1.4" fill="${c.danger}"/>`;
  const X = P + 48;
  b += text(X, P + 13, 'Revoke “debrief-laptop”?', { size: 15, w: 600, fill: c.text });
  b += text(X, P + 37, 'Debrief on that laptop loses access to your', { fill: c.muted });
  b += text(X, P + 55, 'models. You can create a new key at any time.', { fill: c.muted });
  const fy = P + 55 + 28;
  b += footer(c, ac, P, fy, w - 2 * P, [
    { label: 'Cancel', role: 'cancel', focus: true },
    { label: 'Revoke key', role: 'destructive' },
  ]);
  return { b, w, h: fy + 32 + P, fy };
}

function decision(c) {
  const { b, w, h } = decisionBody(c);
  return windowFrame(c, 'Pitwall', w, h, b);
}

/** Form: modal here, fitted (shrinks and grows), fields in the body, pinned footer. */
function form(c) {
  const ac = c.accents.pitwall, w = 560, P = 20;
  let b = '', y = P;
  b += text(P, y + 13, 'Name', { size: 12, w: 600, fill: c.text });
  y += 22;
  b += field(c, P, y, w - 2 * P, 'Debrief on the laptop', { focus: true, ac });
  y += 30 + 6;
  b += text(P, y + 12, 'Shown in the request log and on the key.', { size: 12, fill: c.subtle });
  y += 32;
  b += text(P, y + 13, 'Models it may use', { size: 12, w: 600, fill: c.text });
  y += 22;
  b += rect(P + 0.5, y + 0.5, w - 2 * P - 1, 29, c.field, { rx: 6, stroke: c.control });
  b += text(P + 10, y + 19.5, 'All installed models', { fill: c.text });
  b += `<path d="M${w - P - 22} ${y + 13} l4 4 l4 -4" fill="none" stroke="${c.muted}" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>`;
  y += 30 + 18;
  b += checkbox(c, ac, P, y, 'Allow embeddings', true);
  y += 26;
  b += checkbox(c, ac, P, y, 'Allow requests from other computers on this network');
  y += 16 + 24;
  // Footer row: a hairline only when the body scrolls; here it does not, so none.
  b += footer(c, ac, P, y, w - 2 * P, [
    { label: 'Cancel', role: 'cancel' },
    { label: 'Create app', role: 'primary' },
  ]);
  const h = y + 32 + P;
  return windowFrame(c, 'New app', w, h, b);
}

/** Viewer: modeless, opens at its preferred size, grows to the cap, body scrolls, Close only (plus a non-closing Copy). */
function viewer(c) {
  const ac = c.accents.debrief, w = 720, h = 420, P = 20, FH = 64;
  let b = '';
  const bodyH = h - FH;
  b += text(P, P + 12, 'ERROR · orders-api · 14:02:31.418', { size: 11, w: 600, fill: c.subtle });
  const para = [
    'The request to /orders/7731 failed because the payment service timed out after',
    '30 s. The same timeout appears 41 times in the last hour, always after the',
    'connection pool reported 64 of 64 connections in use.',
    '',
    'Most likely cause: the pool is too small for the 14:00 batch import, which holds',
    'connections for the length of each file. The first timeout follows the batch',
    'start by 2.1 s on every occasion in this log.',
    '',
    'What to try next',
    '•  Raise the pool size or give the batch import a pool of its own.',
    '•  Compare with the 13:00 run, which finished without timeouts.',
    '•  Check whether the payment service logged the same requests.',
  ];
  let y = P + 40;
  for (const line of para) {
    if (line === 'What to try next') b += text(P, y, line, { w: 600, fill: c.text });
    else if (line) b += text(P, y, line, { fill: line.startsWith('•') ? c.text : c.text });
    y += 21;
  }
  // The body scrolls: scrollbar thumb, and the footer's hairline appears.
  b += rect(w - 9, P, 4, bodyH - P - 8, c.hair, { rx: 2 });
  b += rect(w - 9, P, 4, 120, c.subtle, { rx: 2, extra: ' opacity="0.55"' });
  // Fade under the last visible line, then the footer row.
  b += rect(0, bodyH, w, FH, c.win);
  b += rect(0, bodyH, w, 1, c.hair);
  b += `<g transform="translate(${P},${bodyH + 16})">`;
  b += `<path d="M0 0" />`;
  b += '</g>';
  b += footer(c, ac, P, bodyH + 16, w - 2 * P, [
    { label: 'Copy', role: 'secondary', lead: true },
    { label: 'Show cited rows', role: 'secondary', lead: true },
    { label: 'Close', role: 'cancel', focus: true },
  ]);
  return windowFrame(c, 'Explain event', w, h, b);
}

/** Tool: modeless, user-sized, remembers its size, its own layout; the footer keeps a status line and Close. */
function tool(c) {
  const ac = c.accents.graphite, w = 760, h = 470, P = 20, FH = 60, SW = 188;
  let b = '';
  b += rect(0, 0, SW, h - FH, c.side) + rect(SW - 0.5, 0, 0.5, h - FH, c.hair);
  const items = ['General', 'Appearance', 'Notifications', 'Storage', 'Network', 'Updates', 'Diagnostics'];
  items.forEach((s, i) => {
    const y = 14 + i * 32;
    if (i === 3) b += rect(10, y, SW - 20, 28, c.sel, { rx: 6 });
    b += text(22, y + 18.5, s, { fill: i === 3 ? c.text : c.muted, w: i === 3 ? 600 : 400 });
  });
  const X = SW + 28;
  b += text(X, 34, 'Storage', { size: 18, w: 600, fill: c.text });
  b += text(X, 56, 'Where the app keeps its data, and how much it may use.', { fill: c.muted });
  const rows = [['Data folder', '~/Library/Application Support/Spotter'], ['Used', '2.4 GB of 10 GB'], ['Keep history for', '30 days']];
  rows.forEach(([k, v], i) => {
    const y = 84 + i * 44;
    b += rect(X, y + 43.5, w - X - 24, 0.5, c.hair);
    b += text(X, y + 26, k, { fill: c.text });
    if (i === 2) {
      b += rect(w - 24 - 160 + 0.5, y + 7.5, 159, 29, c.field, { rx: 6, stroke: c.control });
      b += text(w - 24 - 150, y + 26.5, v, { fill: c.text });
      b += `<path d="M${w - 24 - 22} ${y + 20} l4 4 l4 -4" fill="none" stroke="${c.muted}" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>`;
    } else b += text(w - 24, y + 26, v, { fill: c.muted, anchor: 'end' });
  });
  // usage bar
  const by = 84 + 3 * 44 + 22, bw = w - X - 24;
  b += rect(X, by, bw, 8, c.hair, { rx: 4 }) + rect(X, by, bw * 0.24, 8, ac.text, { rx: 4 });
  b += text(X, by + 28, 'Logs 1.6 GB · Snapshots 0.7 GB · Cache 0.1 GB', { size: 12, fill: c.subtle });
  b += footer(c, ac, X, by + 46, 0, []);
  const sbw = Math.round(tw('Clear cache', 13, 500) + 28);
  b += button(c, ac, X, by + 46, 'Clear cache', 'secondary').svg;
  void sbw;
  // footer
  b += rect(0, h - FH, w, 0.5, c.hair);
  b += text(P, h - FH + 34, 'Changes are saved as you make them.', { size: 12, fill: c.subtle });
  b += footer(c, ac, P, h - FH + 14, w - 2 * P, [{ label: 'Close', role: 'cancel' }]);
  return windowFrame(c, 'Settings', w, h, b);
}

/** The frame's anatomy: a decision with its measurements (for the ADR and the implementer). */
function anatomy(c) {
  const { b, w, h, fy } = decisionBody(c);
  const L = 270, R = 280, TOP = 64, BOT = 84;
  const ox = L, oy = TOP + 28, A = c.anno;
  const ln = (x1, y1, x2, y2, dash = '') => `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${A}" stroke-width="1"${dash ? ` stroke-dasharray="${dash}"` : ''}/>`;
  const lab = (x, y, s, anchor = 'start', w = 500) => text(x, y, s, { size: 12, w, fill: A, anchor });
  let a = '';
  // Regions: body (dashed) and footer (dashed).
  a += rect(ox + 0.5, oy + 0.5, w - 1, fy - 9, 'none', { stroke: A, extra: ' stroke-dasharray="4 3"' });
  a += rect(ox + 0.5, oy + fy - 7.5, w - 1, h - fy + 7, 'none', { stroke: A, extra: ' stroke-dasharray="4 3"' });
  // Left: region names, with leaders.
  a += ln(ox - 70, oy + 40, ox, oy + 40) + lab(ox - 76, oy + 36, 'Body', 'end', 600) + lab(ox - 76, oy + 52, 'scrolls only past the cap', 'end');
  a += ln(ox - 70, oy + fy + 16, ox, oy + fy + 16) + lab(ox - 76, oy + fy + 12, 'Footer', 'end', 600) + lab(ox - 76, oy + fy + 28, 'declared actions, never scrolls', 'end');
  // Title bar note above.
  a += lab(ox + w / 2, TOP - 14, 'OS title bar: no close, minimise or zoom (ADR 0009); title from the contract or setTitle()', 'middle');
  // Right: measurements and keys, with leaders.
  const rx = ox + w + 64;
  a += lab(rx, oy + 16, '20 px padding all round', 'start', 600) + lab(rx, oy + 32, 'heading 15/600, text 13, 12 px gap');
  a += lab(rx, oy + fy + 2, 'Buttons 32 px, 8 px apart', 'start', 600)
    + lab(rx, oy + fy + 18, 'Enter = default: Cancel here,') + lab(rx, oy + fy + 34, 'a destructive action never is')
    + lab(rx, oy + fy + 50, 'Escape, Cmd/Ctrl+W = Cancel');
  // Height bracket (right of the window, inside the gap).
  const hx = ox + w + 24;
  a += ln(hx, oy, hx, oy + h) + ln(hx - 4, oy, hx + 4, oy) + ln(hx - 4, oy + h, hx + 4, oy + h);
  a += `<text x="${hx + 14}" y="${oy + h / 2}" font-family="${FONT}" font-size="12" font-weight="500" fill="${A}" text-anchor="middle" transform="rotate(-90 ${hx + 14} ${oy + h / 2})">height: measured</text>`;
  // Width bracket below.
  const wy = oy + h + 20;
  a += ln(ox, wy, ox + w, wy) + ln(ox, wy - 4, ox, wy + 4) + ln(ox + w, wy - 4, ox + w, wy + 4);
  a += lab(ox + w / 2, wy + 20, 'Width declared (narrow 400, medium 560, wide 720). OS minimum = measured min content:', 'middle')
    + lab(ox + w / 2, wy + 36, 'header + footer + two body lines high, the footer on one line wide', 'middle');
  return { ...windowFrame(c, 'Pitwall', w, h, b, { left: L, right: R, top: TOP, bottom: BOT, extra: a }) };
}

const out = process.argv[2];
if (!out) throw new Error('usage: node dialog-kinds.mjs <out-dir>');
mkdirSync(out, { recursive: true });
const sizes = {};
for (const theme of ['light', 'dark']) {
  for (const [name, fn] of Object.entries({ decision, form, viewer, tool, anatomy })) {
    const m = fn(T[theme]);
    writeFileSync(join(out, `${name}-${theme}.svg`), m.svg);
    sizes[`${name}-${theme}`] = [m.W, m.H];
  }
}
writeFileSync(join(out, 'sizes.json'), JSON.stringify(sizes, null, 2));
console.log(JSON.stringify(sizes));
