// Generates docs/images/paddock-ecosystem.svg (theme-aware, transparent) and, with --png, the 2x light/dark PNGs
// via the pinned headless Chrome for Testing (chrome.mjs). Plain Node, no dependencies:  node docs/images/src/ecosystem.mjs [--png]
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { versionPaddockDocs } from './version-links.mjs';
import { screenshotSvg } from './chrome.mjs';
import { CODEBASE_TOOL, STATUS } from './family.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, '..');
const W = 1600; // H follows from the layout (see the legend)


// Layout: Pacenotes, Debrief and Sighter on top; Recce, Dyno and Spotter below; Pitwall is the band between. Pitcrew, which needs no
// Pitwall, sits alone under Dyno.
const row1 = [
  { id: 'pacenotes', name: CODEBASE_TOOL, accent: '#657C1F', lines: ['Understands code in any', 'language: link graph, code', 'review, local search.'] },
  { id: 'debrief', name: 'Debrief', accent: '#2F6FE0', lines: ['An IDE for analysing logs', 'and diagnostic files.'] },
  { id: 'sighter', name: 'Sighter', accent: '#975EA2', lines: ['Finds any UI element by', 'sight. MCP server and', 'agent for any desktop.'] },
];
// Recce is planned (placeholder icon and page only): its card carries the Planned chip like Sighter's and Pacenotes'.
const row3 = [
  { id: 'recce-placeholder', key: 'recce', name: 'Recce', accent: '#2E9E75', lines: ['One search box for the', 'whole PC, answers with', 'sources. Retrieval engine.'] },
  { id: 'dyno', name: 'Dyno', accent: '#C2287C', lines: ['Watches your PC:', 'read-only monitor.'] },
  { id: 'spotter', name: 'Spotter', accent: '#0E8577', lines: ['Watches for new releases', 'anywhere, the moment', 'one appears.'] },
];
const pitcrew = { id: 'pitcrew', name: 'Pitcrew', accent: '#D2420F', ci: 'Runs CI for all apps', lines: ['Keeps CI runners alive,', 'updated and clean.'] };
const CW = 340, GAP = 215, X0 = 75, CH = 170;
const R1 = 340, HUB = 650, HUBH = 160, R3 = HUB + HUBH + 140, R4 = R3 + CH + 100;
const cardX = (i) => X0 + i * (CW + GAP);
const cMid = (r) => r + CH / 2;
const gapL = (i) => cardX(i) + CW, gapR = (i) => cardX(i + 1); // edges of the gap right of column i

function icon(id, x, y, size) {
  const raw = readFileSync(join(here, 'icons', `${id === 'codebase' ? 'pacenotes' : id}.svg`), 'utf8');
  const inner = raw.replace(/^[\s\S]*?<svg[^>]*>/, '').replace(/<\/svg>\s*$/, '').replace(/<title>[^<]*<\/title>/, '');
  return `<svg x="${x}" y="${y}" width="${size}" height="${size}" viewBox="60 60 904 904" overflow="visible">${inner}</svg>`;
}
// Label pill centred at (x, y); long labels wrap onto two lines. Returns SVG.
function pill(x, y, text) {
  const len = (s) => s.replace(/&\w+;/g, '&').length; // measure entities as one character
  const words = text.split(' '), lines = len(text) > 20 && words.length > 1 ? [words.slice(0, Math.ceil(words.length / 2)).join(' '), words.slice(Math.ceil(words.length / 2)).join(' ')] : [text];
  const w = Math.round(Math.max(...lines.map(len)) * 8.4 + 26), h = lines.length === 1 ? 30 : 50;
  const t = lines.map((l, k) => `<text class="lbl" x="${x}" y="${y + 6 + (k - (lines.length - 1) / 2) * 21}" text-anchor="middle">${l}</text>`).join('');
  return `<g><rect class="pill" x="${x - w / 2}" y="${y - h / 2}" width="${w}" height="${h}" rx="${h / 2}"/>${t}</g>`;
}
const head = (x, y, dir, cls) => {
  const p = { u: `${x},${y} ${x - 6},${y + 11} ${x + 6},${y + 11}`, d: `${x},${y} ${x - 6},${y - 11} ${x + 6},${y - 11}`,
    l: `${x},${y} ${x + 11},${y - 6} ${x + 11},${y + 6}`, r: `${x},${y} ${x - 11},${y - 6} ${x - 11},${y + 6}` }[dir];
  return `<polygon class="${cls}" points="${p}"/>`;
};
const rr = 22;
// Hand-off line (solid, or dashed when "later") along orthogonal points, with an arrow head at the end (and optionally the start).
function hand(pts, { later = false, both = false, headless = false } = {}) {
  let d = `M${pts[0][0]} ${pts[0][1]}`;
  for (let i = 1; i < pts.length; i++) {
    const [px, py] = pts[i - 1], [x, y] = pts[i];
    if (i < pts.length - 1) {
      const [nx, ny] = pts[i + 1], l1 = Math.hypot(x - px, y - py), l2 = Math.hypot(nx - x, ny - y), r = Math.min(rr, l1 / 2, l2 / 2);
      const ax = x - Math.sign(x - px) * r, ay = y - Math.sign(y - py) * r, bx = x + Math.sign(nx - x) * r, by = y + Math.sign(ny - y) * r;
      d += ` L${ax} ${ay} Q${x} ${y} ${bx} ${by}`;
    } else d += ` L${x} ${y}`;
  }
  const dirOf = ([ax, ay], [bx, by]) => (Math.abs(bx - ax) > Math.abs(by - ay) ? (bx > ax ? 'r' : 'l') : by > ay ? 'd' : 'u');
  const n = pts.length;
  let s = `<path class="hand${later ? ' later' : ''}" d="${d}"/>${headless ? '' : head(pts[n - 1][0], pts[n - 1][1], dirOf(pts[n - 2], pts[n - 1]), 'ah')}`;
  if (both) s += head(pts[0][0], pts[0][1], dirOf(pts[1], pts[0]), 'ah');
  return s;
}
// Pitwall stub: dotted purple line from a card to the hub, arrow towards the hub (both ways when `both`). `gap`: the y of a
// hand-off line that crosses the stub; the stub breaks there (a bridge), so the crossing reads at a glance.
function stub(x, y0, y1, label, { both = false, at, gap } = {}) {
  const down = y1 > y0, mid = at ?? (y0 + y1) / 2;
  const d = gap == null ? `M${x} ${y0} L${x} ${y1}` : `M${x} ${y0} L${x} ${gap - 9} M${x} ${gap + 9} L${x} ${y1}`;
  let s = `<path class="stub" d="${d}"/>${head(x, y1, down ? 'd' : 'u', 'ahp')}`;
  if (both) s += head(x, y0, down ? 'u' : 'd', 'ahp');
  return s + pill(x, mid, label);
}

// Status chip (16 px text), right edge at xr, top at y. Status tones are neutral or status colours, never an app accent.
function statusChip(st, xr, y) {
  const cw = Math.round(st.text.length * 8.9 + 30);
  return `<rect class="chip-${st.tone}" x="${xr - cw}" y="${y}" width="${cw}" height="28" rx="14"/><text class="chipt-${st.tone}" x="${xr - cw / 2}" y="${y + 19.5}" text-anchor="middle">${st.text}</text>`;
}
function card(a, i, y) {
  const x = cardX(i);
  const ico = icon(a.id, x + 22, y + 24, 64);
  const bar = `<rect x="${x + 104}" y="${y + 28}" width="20" height="3" rx="1.5" fill="${a.accent ?? 'var(--link)'}"/>`;
  const st = STATUS[a.key ?? a.id];
  // A long status does not fit beside the name: it moves to the bottom right of the card.
  const tag = statusChip(st, x + CW - 18, st.text.length > 14 ? y + CH - 40 : y + 14);
  const badge = a.ci ? `<text class="cinote" x="${x + 104}" y="${y + 153}">${a.ci}</text>` : '';
  const text = a.lines.map((l, k) => `<text class="promise" x="${x + 104}" y="${y + 96 + k * 23}">${l}</text>`).join('');
  return `<g><rect class="card" x="${x}" y="${y}" width="${CW}" height="${CH}" rx="16"/>${ico}${bar}${tag}${badge}` +
    `<text class="name" x="${x + 104}" y="${y + 64}">${a.name}</text>${text}</g>`;
}
const cards = [...row1.map((a, i) => card(a, i, R1)), ...row3.map((a, i) => card(a, i, R3)), card(pitcrew, 1, R4)].join('\n');

// ---- hand-offs between apps (solid arrows; dashed = later) ----
const m1 = cMid(R1), m3 = cMid(R3), B1 = R1 + CH, laneTop = B1 + 30;
const links = [];
// Pacenotes <-> Debrief (two lines in the gap)
links.push(hand([[gapR(0) - 2, m1 - 30], [gapL(0) + 2, m1 - 30]]) + pill((gapL(0) + gapR(0)) / 2, m1 - 30, 'Trace → code'));
links.push(hand([[gapL(0) + 2, m1 + 30], [gapR(0) - 2, m1 + 30]]) + pill((gapL(0) + gapR(0)) / 2, m1 + 30, 'Log checks'));
// Sighter -> Debrief
links.push(hand([[cardX(2) - 2, m1 - 34], [gapL(1) + 2, m1 - 34]]) + pill((gapL(1) + cardX(2)) / 2, m1 - 34, 'Run bundles'));
// Debrief -> Sighter (MCP host: reproduce a bug from a video)
links.push(hand([[gapL(1) + 2, m1 + 34], [cardX(2) - 2, m1 + 34]]) + pill((gapL(1) + cardX(2)) / 2, m1 + 34, 'Video repro (MCP)'));
// Dyno -> Pitcrew (straight down), Dyno -> Spotter
const B3 = R3 + CH, MIDG = (B3 + R4) / 2;
links.push(hand([[cardX(1) + CW / 2, B3 + 2], [cardX(1) + CW / 2, R4 - 2]]) + pill(cardX(1) + CW / 2, MIDG, 'Machine health'));
links.push(hand([[gapL(1) + 2, m3], [cardX(2) - 2, m3]]) + pill((gapL(1) + cardX(2)) / 2, m3, 'Installed software'));
// Pacenotes -> Sighter (test plans) and back (results): nested arcs over the top row
links.push(hand([[335, R1 - 2], [335, R1 - 40], [1265, R1 - 40], [1265, R1 - 2]]) + pill(800, R1 - 40, 'Test plans (MCP)'));
links.push(hand([[1385, R1 - 2], [1385, R1 - 100], [275, R1 - 100], [275, R1 - 2]]) + pill(800, R1 - 100, 'Results'));
// Pitcrew -> Debrief and Dyno -> Debrief (deep links) run up the left margin, outside Recce and the hub band: Dyno's line leaves
// its bottom and joins Pitcrew's in the gap above Pitcrew, and one line runs on to Debrief. Spotter -> Sighter (later) passes the
// band on its right end. Nothing crosses the band.
const LX = 40, DYX = 660, JOIN = MIDG, SX = 1462;
links.push(hand([[cardX(1) - 2, R4 + CH / 2], [LX, R4 + CH / 2], [LX, laneTop], [DYX, laneTop], [DYX, B1 + 2]]) + pill(330, R4 + CH / 2, 'Job logs'));
links.push(hand([[DYX, B3 + 2], [DYX, JOIN], [LX, JOIN]], { headless: true }) + pill(330, JOIN, 'Issues') +
  `<circle class="ah" cx="${LX}" cy="${JOIN}" r="4.5"/>`); // junction: Issues joins the Job logs line
links.push(hand([[SX, R3 - 2], [SX, B1 + 2]], { later: true }) + pill(SX, (HUB + HUBH + R3) / 2, 'Smoke tests'));

// ---- Pitwall stubs: what each app uses the hub for; MCP lines: whose tools Pitwall calls ----
const T = B1 + 4, Bt = HUB - 4, H2 = HUB + HUBH + 4, T3 = R3 - 4;
const stubs = [
  stub(245, T, Bt, 'Q&amp;A, embeddings', { at: (T + Bt) / 2 + 14, gap: laneTop }),
  stub(720, T, Bt, 'AI engine, paired'),
  stub(1265, T, Bt, 'Vision, planner'),
  stub(200, T3, H2, 'Answers'),
  stub(720, T3, H2, 'Diagnosis'),
  stub(1265, T3, H2, 'What’s new'),
].join('\n');
// Pitwall as MCP client and host (planned for Pitwall 1.1): a dash-dot purple line from the hub to each app whose MCP server it calls,
// the arrow towards the server. Its dash-dot pattern differs from the grey dashed "later" hand-offs, and the label says MCP, so
// colour is never the only cue; the legend says it is planned.
function mcp(x, y0, y1, { at, gap } = {}) {
  const mid = at ?? (y0 + y1) / 2;
  const d = gap == null ? `M${x} ${y0} L${x} ${y1}` : `M${x} ${y0} L${x} ${gap - 9} M${x} ${gap + 9} L${x} ${y1}`;
  return `<path class="mcp" d="${d}"/>${head(x, y1, y1 > y0 ? 'd' : 'u', 'ahp')}` + pill(x, mid, 'MCP');
}
const mcps = [
  mcp(365, Bt, T, { at: (T + Bt) / 2 + 14, gap: laneTop }), // Pacenotes
  mcp(880, Bt, T), // Debrief
  mcp(1395, Bt, T), // Sighter
  mcp(330, H2, T3), // Recce
  mcp(880, H2, T3), // Dyno
].join('\n');

// ---- Pitwall hub band: shorter than the card rows, so the hand-offs that pass it run beside its ends, never through it ----
const HX = 162, HW = SX - 40 - HX;
const cols = [
  [600, 'One local API', ['OpenAI, Anthropic and Ollama', 'compatible, plus /x/v1.']],
  [905, 'Chat and tools over MCP', ['Planned for 1.1: chat that', 'calls the apps’ MCP tools.']],
  [1185, 'Yours alone', ['Keys, priorities, caching.', 'No cloud, no telemetry.']],
];
const hubCols = cols.map(([x, h, ls]) => `<text class="vt" x="${x}" y="${HUB + 62}">${h}</text>` +
  ls.map((l, k) => `<text class="vs" x="${x}" y="${HUB + 90 + k * 23}">${l}</text>`).join('')).join('');
const hubTag = statusChip(STATUS.pitwall, HX + HW - 22, HUB + 16);
const hub = `<g><rect class="hub" x="${HX}" y="${HUB}" width="${HW}" height="${HUBH}" rx="24"/>${hubTag}
${icon('pitwall', HX + 28, HUB + 40, 80)}
<rect x="${HX + 130}" y="${HUB + 44}" width="20" height="3" rx="1.5" fill="#6D3FD6"/>
<text class="name hubname" x="${HX + 130}" y="${HUB + 80}">Pitwall</text>
<text class="promise" x="${HX + 130}" y="${HUB + 106}">The local AI hub, on llama.cpp.</text>
<text class="promise" x="${HX + 130}" y="${HUB + 129}">Optional for every app.</text>
${hubCols}</g>`;

// ---- Paddock foundation: header with the kits, then two layers (Workbench on top of the App shell), values, legend ----
const PX = X0, PW = 3 * CW + 2 * GAP, PY = R4 + CH + 60;
const chipRow = (labels, right, y, cls) => {
  const widths = labels.map((t) => Math.round(t.length * 7.9 + 26)), gap = 10, total = widths.reduce((s, w) => s + w, 0) + gap * (labels.length - 1);
  let x = right - total;
  return labels.map((t, i) => { const w = widths[i]; const s = `<rect class="${cls}" x="${x}" y="${y}" width="${w}" height="32" rx="16"/><text class="chiplbl" x="${x + w / 2}" y="${y + 21}" text-anchor="middle">${t}</text>`; x += w + gap; return s; }).join('');
};
// Two equal-width layers, Workbench (tinted) above the App shell (neutral); each names and shows the icons of its apps. All chip rows
// share one right edge.
// The App shell layer is taller: its parts take two chip rows.
const SX0 = PX + 20, SW = PW - 40, CHIPR = SX0 + SW - 24, LH = 100, LH2 = 128, LGAP = 30, L1 = PY + 96, L2 = L1 + LH + LGAP;
const PH = L2 + LH2 + 20 - PY;
const kits = chipRow(['UI kit, design system', 'Rust crates', 'Diagram kit', 'Icon set', 'Conformance checks', 'Release practices', 'Lessons'], CHIPR, PY + 30, 'chip');
const layer = (y, h, cls, chipCls, name, who, apps, rows) => {
  const ix = SX0 + 24 + { Workbench: 104, 'App shell': 84 }[name] + 14; // measured title widths at 20px/600
  const icons = apps.map((id, k) => icon(id, ix + k * 36, y + 13, 30)).join('');
  const top = y + (h - rows.length * 32 - (rows.length - 1) * 10) / 2;
  return `<g><rect class="${cls}" x="${SX0}" y="${y}" width="${SW}" height="${h}" rx="14"/>
<text class="lt" x="${SX0 + 24}" y="${y + 36}">${name}</text>${icons}${who.map((l, k) => `<text class="vs" x="${SX0 + 24}" y="${y + 62 + k * 20}">${l}</text>`).join('')}
${rows.map((parts, k) => chipRow(parts, CHIPR, top + k * 42, chipCls)).join('')}</g>`;
};
const layers = layer(L1, LH, 'layer wb', 'chip', 'Workbench', ['Adds IDE panes to the shell for', 'Debrief, Pacenotes, Sighter.'], ['debrief', 'pacenotes', 'sighter'],
  [['Docked panes, layout planner', 'Activity bar', 'Explorer', 'Command palette', 'AI pane']]) +
  layer(L2, LH2, 'layer', 'chip2', 'App shell', ['The base of all eight apps. Shell only:', 'Pitcrew, Pitwall, Spotter, Dyno, Recce.'], ['pitcrew', 'pitwall', 'spotter', 'dyno', 'recce-placeholder'],
    [['Startup protection, safe mode', 'Report a problem', 'Services, login items', 'Notifications, health'],
      ['Dialogs', 'Jobs and progress', 'Settings, About, Updates', 'Storage', 'Undo', 'Theme']]);
// "Builds on": a short arrow from the Workbench down into the App shell, labelled in the gap.
const BX = SX0 + 48;
const buildsOn = `<path class="hand" d="M${BX} ${L1 + LH + 2} L${BX} ${L2 - 2}"/>${head(BX, L2 - 1, 'd', 'ah')}<text class="legt" x="${BX + 14}" y="${L1 + LH + LGAP / 2 + 5}">builds on</text>`;
const values = [
  ['Local only, private by design', 'No cloud. No telemetry.'],
  ['Windows, macOS and Linux', 'One family, native on each.'],
  ['Rust, Tauri and Angular', 'Fast engines, native feel.'],
];
const VY = PY + PH + 44;
const valueEls = values.map(([t, s], i) => {
  const x = X0 + i * (PW / 3);
  return `<rect x="${x}" y="${VY - 4}" width="2" height="44" rx="1" class="rule"/><text class="vt" x="${x + 16}" y="${VY + 14}">${t}</text><text class="vs" x="${x + 16}" y="${VY + 36}">${s}</text>`;
}).join('');
const LY = VY + 90, LY2 = LY + 36, H = LY2 + 76;
const legend = `<g>
<path class="hand" d="M${X0} ${LY} L${X0 + 44} ${LY}"/>${head(X0 + 46, LY, 'r', 'ah')}<text class="legt" x="${X0 + 62}" y="${LY + 6}">Hand-off, arrow shows the direction of data</text>
<path class="hand later" d="M${X0 + 740} ${LY} L${X0 + 784} ${LY}"/>${head(X0 + 786, LY, 'r', 'ah')}<text class="legt" x="${X0 + 802}" y="${LY + 6}">Dashed: later</text>
<path class="stub" d="M${X0} ${LY2} L${X0 + 44} ${LY2}"/>${head(X0 + 46, LY2, 'r', 'ahp')}<text class="legt" x="${X0 + 62}" y="${LY2 + 6}">Uses Pitwall for local AI (optional)</text>
<path class="mcp" d="M${X0 + 740} ${LY2} L${X0 + 784} ${LY2}"/>${head(X0 + 786, LY2, 'r', 'ahp')}<text class="legt" x="${X0 + 802}" y="${LY2 + 6}">MCP, planned: Pitwall 1.1 calls the app’s MCP tools</text>
</g>`;

const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-labelledby="t d">
<title id="t">The Paddock family</title>
<desc id="d">Eight local-first desktop apps. ${CODEBASE_TOOL}, Debrief and Sighter above; Recce, Dyno and Spotter below; Pitwall, the optional local AI hub, between them; Pitcrew under Dyno. Hand-offs: ${CODEBASE_TOOL} and Debrief exchange stack traces and log checks, ${CODEBASE_TOOL} sends Sighter test plans and gets results, Sighter sends Debrief run bundles, Debrief drives Sighter over MCP to reproduce a bug from a video, Pitcrew sends Debrief job logs and Dyno sends it issues (one line up the left margin, beside Recce and Pitwall, not through them), Dyno sends Pitcrew machine health and Spotter installed software, Pitcrew runs CI for all, and Spotter will feed Sighter smoke tests later. Uses of Pitwall (dotted): ${CODEBASE_TOOL} for answers and embeddings, Debrief as its AI engine once paired (its built-in engine is the fallback), Sighter for vision and as agent planner, Recce for answers, Dyno for diagnosis, Spotter for what's-new summaries. Planned for Pitwall 1.1 (dash-dot, labelled MCP): Pitwall as MCP client and host calls the MCP servers of ${CODEBASE_TOOL}, Debrief, Sighter, Recce (the family's retrieval engine, so Pitwall builds no index of its own) and Dyno (its MCP server is opt-in). Recce (one search box for the whole PC) is planned and shown with its placeholder icon. Underneath, Paddock provides the kits (UI kit and design system, shared Rust crates, diagram kit, icon set, conformance checks, release practices, lessons) and two layers: the App shell every app uses (startup protection and safe mode, Report a problem, background services and login items, notifications and service health, dialogs, jobs and progress, settings, about and updates, the Storage page, undo, theme) and, on top of it, the Workbench for the IDE-style apps Debrief, ${CODEBASE_TOOL} and Sighter (docked panes with the layout planner, activity bar, Explorer, command palette, AI pane); Pitcrew, Pitwall, Spotter, Dyno and Recce use the shell with pages.</desc>
<style>
:root{--ink:#14161A;--muted:#566070;--card:#FFFFFF;--line:#DDE1E8;--soft:#EEF0F4;--link:#8A93A3;--hand:#566070;--found:#F1F3F7;--hubf:#F0ECFB;--hubs:#6D3FD6;--okf:#E6F2EF;--oks:#007A5E;--line2:#CDD3DD;--wbf:#E8EEF8;--wbs:#B3C2DB}
@media (prefers-color-scheme: dark){:root{--ink:#F2F4F7;--muted:#A6AEBC;--card:#171A20;--line:#2B303A;--soft:#20242C;--link:#667085;--hand:#A6AEBC;--found:#12151A;--hubf:#242040;--hubs:#A58BFA;--okf:#193330;--oks:#2EC4A0;--line2:#3A414D;--wbf:#1F2939;--wbs:#4A5D80}}
text{font-family:-apple-system,"SF Pro Display","SF Pro Text","Segoe UI Variable Display","Segoe UI",Cantarell,"Noto Sans",system-ui,sans-serif;font-variant-numeric:tabular-nums;fill:var(--ink)}
.eyebrow{font-size:16px;font-weight:600;letter-spacing:.16em;fill:var(--muted)}
.h1{font-size:44px;font-weight:600;letter-spacing:-.02em}
.sub{font-size:19px;fill:var(--muted)}
.chip-design{fill:none;stroke:var(--link);stroke-dasharray:3 3}
.chip-dev{fill:var(--soft);stroke:var(--line)}
.chip-soon{fill:var(--soft);stroke:var(--line2)}
.chip-ok{fill:var(--okf);stroke:var(--oks)}
.chipt-design,.chipt-dev{font-size:16px;font-weight:500;fill:var(--muted)}
.chipt-soon{font-size:16px;font-weight:600;fill:var(--ink)}
.chipt-ok{font-size:16px;font-weight:600;fill:var(--oks)}
.cinote{font-size:16px;font-weight:500;fill:var(--muted)}
.card{fill:var(--card);stroke:var(--line);stroke-width:1}
.name{font-size:25px;font-weight:600;letter-spacing:-.01em}
.hubname{font-size:30px}
.promise{font-size:17px;fill:var(--muted)}
.hand{fill:none;stroke:var(--hand);stroke-width:2;stroke-linecap:round;stroke-linejoin:round}
.later{stroke-dasharray:7 6}
.ah{fill:var(--hand)}
.stub{fill:none;stroke:var(--hubs);stroke-width:2;stroke-linecap:round;stroke-dasharray:1 6}
.ahp{fill:var(--hubs)}
.mcp{fill:none;stroke:var(--hubs);stroke-width:2;stroke-linecap:round;stroke-dasharray:12 5 1 5}
.pill{fill:var(--card);stroke:var(--line)}
.lbl{font-size:17px;font-weight:500;fill:var(--ink)}
.hub{fill:var(--hubf);stroke:var(--hubs);stroke-width:1.5}
.found{fill:var(--found);stroke:var(--line)}
.chip{fill:var(--card);stroke:var(--line)}
.chiplbl{font-size:16px;font-weight:500}
.chip2{fill:var(--soft);stroke:var(--line)}
.layer{fill:var(--card);stroke:var(--line2)}
.wb{fill:var(--wbf);stroke:var(--wbs)}
.lt{font-size:20px;font-weight:600}
.ft{font-size:24px;font-weight:600}
.fs{font-size:17px;fill:var(--muted)}
.vt{font-size:17px;font-weight:600}
.vs{font-size:16px;fill:var(--muted)}
.legt{font-size:16px;fill:var(--muted)}
.rule{fill:var(--line)}
.track{fill:none;stroke:var(--line);stroke-width:1.25;opacity:.7}
</style>

<text class="eyebrow" x="${W / 2}" y="76" text-anchor="middle">PADDOCK</text>
<text class="h1" x="${W / 2}" y="136" text-anchor="middle">Eight local-first apps for people who keep systems running</text>
<text class="sub" x="${W / 2}" y="176" text-anchor="middle">One private AI hub, one shared foundation. Each app is good alone and better together.</text>

${stubs}
${mcps}
${cards}
${links.join('\n')}
${hub}

<g>
<rect class="found" x="${PX}" y="${PY}" width="${PW}" height="${PH}" rx="20"/>
${icon('paddock', PX + 26, PY + 22, 60)}
<text class="ft" x="${PX + 100}" y="${PY + 46}">Paddock</text>
<text class="fs" x="${PX + 100}" y="${PY + 74}">The shared foundation under every app.</text>
${kits}
${layers}
${buildsOn}
</g>

${valueEls}
${legend}
</svg>
`;
mkdirSync(out, { recursive: true });
writeFileSync(join(out, 'paddock-ecosystem.svg'), svg);
console.log('wrote paddock-ecosystem.svg');

if (process.argv.includes('--png')) {
  for (const theme of ['light', 'dark']) {
    screenshotSvg(join(out, 'paddock-ecosystem.svg'), join(out, `paddock-ecosystem-${theme}.png`), W, H, theme);
    console.log(`wrote paddock-ecosystem-${theme}.png`);
  }
}
versionPaddockDocs(join(out, '../..'));
