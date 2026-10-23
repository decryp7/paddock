// Generates docs/images/paddock-ecosystem.svg (theme-aware, transparent) and, with --png, the 2x light/dark PNGs
// via headless Chrome. Plain Node, no dependencies:  node docs/images/src/ecosystem.mjs [--png]
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { versionPaddockDocs } from './version-links.mjs';
import { CODEBASE_TOOL, STATUS } from './family.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, '..');
const W = 1600, H = 1500;


// Layout: Pacenotes, Debrief and Sighter form a triangle on top; Pitcrew, Dyno and Spotter below; Pitwall is the band between.
const row1 = [
  { id: 'pacenotes', name: CODEBASE_TOOL, accent: '#831826', lines: ['Understands code in any', 'language: link graph, code', 'review, local search.'] },
  { id: 'debrief', name: 'Debrief', accent: '#2F6FE0', lines: ['An IDE for analysing logs', 'and diagnostic files.'] },
  { id: 'sighter', name: 'Sighter', accent: '#00B8D9', lines: ['Finds any UI element by', 'sight. MCP server and', 'agent for any desktop.'] },
];
const row3 = [
  { id: 'pitcrew', name: 'Pitcrew', accent: '#D2420F', ci: 'Runs CI for all apps', lines: ['Keeps CI runners alive,', 'updated and clean.'] },
  { id: 'dyno', name: 'Dyno', accent: '#C2287C', lines: ['Measures, explains and', 'tunes your PC.'] },
  { id: 'spotter', name: 'Spotter', accent: '#0E8577', lines: ['Watches for new releases', 'anywhere, the moment', 'one appears.'] },
];
const CW = 340, GAP = 215, X0 = 75, CH = 170;
const R1 = 340, HUB = 650, HUBH = 160, R3 = HUB + HUBH + 140;
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
  const words = text.split(' '), lines = text.length > 13 && words.length > 1 ? [words.slice(0, Math.ceil(words.length / 2)).join(' '), words.slice(Math.ceil(words.length / 2)).join(' ')] : [text];
  const w = Math.round(Math.max(...lines.map((l) => l.length)) * 10.6 + 28), h = lines.length === 1 ? 34 : 58;
  const t = lines.map((l, k) => `<text class="lbl" x="${x}" y="${y + 8 + (k - (lines.length - 1) / 2) * 26}" text-anchor="middle">${l}</text>`).join('');
  return `<g><rect class="pill" x="${x - w / 2}" y="${y - h / 2}" width="${w}" height="${h}" rx="${h === 36 ? 18 : 22}"/>${t}</g>`;
}
const head = (x, y, dir, cls) => {
  const p = { u: `${x},${y} ${x - 6},${y + 11} ${x + 6},${y + 11}`, d: `${x},${y} ${x - 6},${y - 11} ${x + 6},${y - 11}`,
    l: `${x},${y} ${x + 11},${y - 6} ${x + 11},${y + 6}`, r: `${x},${y} ${x - 11},${y - 6} ${x - 11},${y + 6}` }[dir];
  return `<polygon class="${cls}" points="${p}"/>`;
};
const rr = 22;
// Hand-off line (solid, or dashed when "later") along orthogonal points, with an arrow head at the end (and optionally the start).
function hand(pts, { later = false, both = false } = {}) {
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
  let s = `<path class="hand${later ? ' later' : ''}" d="${d}"/>${head(pts[n - 1][0], pts[n - 1][1], dirOf(pts[n - 2], pts[n - 1]), 'ah')}`;
  if (both) s += head(pts[0][0], pts[0][1], dirOf(pts[1], pts[0]), 'ah');
  return s;
}
// Pitwall stub: dotted purple line from a card to the hub, arrow towards the hub (both ways when `both`).
function stub(x, y0, y1, label, { both = false } = {}) {
  const down = y1 > y0, mid = (y0 + y1) / 2;
  let s = `<path class="stub" d="M${x} ${y0} L${x} ${y1}"/>${head(x, y1, down ? 'd' : 'u', 'ahp')}`;
  if (both) s += head(x, y0, down ? 'u' : 'd', 'ahp');
  return s + pill(x, mid, label);
}

function card(a, i, y) {
  const x = cardX(i);
  const ico = icon(a.id, x + 22, y + 24, 64);
  const bar = `<rect x="${x + 104}" y="${y + 28}" width="20" height="3" rx="1.5" fill="${a.accent ?? 'var(--link)'}"/>`;
  const st = STATUS[a.id], cw = Math.round(st.text.length * 7.8 + 30);
  const tag = `<rect class="chip-${st.tone}" x="${x + CW - 20 - cw}" y="${y + 18}" width="${cw}" height="26" rx="13"/><text class="chipt-${st.tone}" x="${x + CW - 20 - cw / 2}" y="${y + 36}" text-anchor="middle">${st.text}</text>`;
  const badge = a.ci ? `<circle cx="${x + 110}" cy="${y + 147}" r="4" fill="${a.accent}"/><text class="cinote" x="${x + 122}" y="${y + 153}">${a.ci}</text>` : '';
  const text = a.lines.map((l, k) => `<text class="promise" x="${x + 104}" y="${y + 96 + k * 23}">${l}</text>`).join('');
  return `<g><rect class="card" x="${x}" y="${y}" width="${CW}" height="${CH}" rx="16"/>${ico}${bar}${tag}${badge}` +
    `<text class="name" x="${x + 104}" y="${y + 64}">${a.name}</text>${text}</g>`;
}
const cards = [...row1.map((a, i) => card(a, i, R1)), ...row3.map((a, i) => card(a, i, R3))].join('\n');

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
// Dyno -> Pitcrew, Dyno -> Spotter
links.push(hand([[cardX(1) - 2, m3], [gapL(0) + 2, m3]]) + pill((gapL(0) + cardX(1)) / 2, m3, 'Machine health'));
links.push(hand([[gapL(1) + 2, m3], [cardX(2) - 2, m3]]) + pill((gapL(1) + cardX(2)) / 2, m3, 'Installed software'));
// Pacenotes -> Sighter (test plans) and back (results): nested arcs over the top row
links.push(hand([[335, R1 - 2], [335, R1 - 40], [1265, R1 - 40], [1265, R1 - 2]]) + pill(800, R1 - 40, 'Test plans, MCP host'));
links.push(hand([[1385, R1 - 2], [1385, R1 - 100], [275, R1 - 100], [275, R1 - 2]]) + pill(800, R1 - 100, 'Results'));
// Pitcrew -> Debrief, Dyno -> Debrief, Spotter -> Sighter (later): lines through the lane around the hub band
const CH1 = Math.round((gapL(0) + cardX(1)) / 2), DX = 900, SX = 1450;
links.push(hand([[380, R3 - 2], [380, R3 - 36], [CH1, R3 - 36], [CH1, laneTop], [660, laneTop], [660, B1 + 2]]) + pill(CH1, R3 - 70, 'Job logs'));
links.push(hand([[DX, R3 - 2], [DX, B1 + 2]]) + pill(DX, B1 + 70, 'Issues'));
links.push(hand([[SX, R3 - 2], [SX, B1 + 2]], { later: true }) + pill(SX, B1 + 70, 'Smoke tests'));

// ---- Pitwall stubs: what each app uses the hub for ----
const T = B1 + 4, Bt = HUB - 4, H2 = HUB + HUBH + 4, T3 = R3 - 4;
const stubs = [
  stub(cardX(0) + CW / 2, T, Bt, 'Q&amp;A, embeddings', { both: true }),
  stub(720, T, Bt, 'Explain, research'),
  stub(1265, T, Bt, 'Vision, planner', { both: true }),
  stub(720, T3, H2, 'Diagnosis'),
  stub(1265, T3, H2, 'What’s new'),
].join('\n');

// ---- Pitwall hub band (avoids the x of the hand-off lines that pass through it) ----
const HX = X0, HW = 3 * CW + 2 * GAP;
const cols = [
  [580, 'One local API', ['OpenAI, Anthropic and Ollama', 'compatible, plus /x/v1.']],
  [930, 'Tools over MCP', ['MCP client and tool', 'router.']],
  [1172, 'Yours alone', ['Keys, priorities, caching.', 'No cloud, no telemetry.']],
];
const hubCols = cols.map(([x, h, ls]) => `<text class="vt" x="${x}" y="${HUB + 62}">${h}</text>` +
  ls.map((l, k) => `<text class="vs" x="${x}" y="${HUB + 90 + k * 23}">${l}</text>`).join('')).join('');
const hst = STATUS.pitwall, hcw = Math.round(hst.text.length * 7.8 + 30);
const hubTag = `<rect class="chip-${hst.tone}" x="${1420 - hcw}" y="${HUB + 20}" width="${hcw}" height="26" rx="13"/><text class="chipt-${hst.tone}" x="${1420 - hcw / 2}" y="${HUB + 38}" text-anchor="middle">${hst.text}</text>`;
const hub = `<g><rect class="hub" x="${HX}" y="${HUB}" width="${HW}" height="${HUBH}" rx="24"/>${hubTag}
${icon('pitwall', HX + 28, HUB + 40, 80)}
<rect x="${HX + 130}" y="${HUB + 44}" width="20" height="3" rx="1.5" fill="#6D3FD6"/>
<text class="name hubname" x="${HX + 130}" y="${HUB + 80}">Pitwall</text>
<text class="promise" x="${HX + 130}" y="${HUB + 106}">The local AI hub, on llama.cpp.</text>
<text class="promise" x="${HX + 130}" y="${HUB + 129}">Optional for every app.</text>
${hubCols}</g>`;

// ---- Paddock foundation, values, legend ----
const PX = X0, PW = HW, PY = R3 + CH + 60, PH = 100;
const provides = ['UI kit', 'Design system', 'Diagram kit', 'Icon set', 'Release practices', 'Lessons'];
const chips = (() => {
  const widths = provides.map((t) => Math.round(t.length * 7.4 + 36)), gap = 14, total = widths.reduce((s, w) => s + w, 0) + gap * (provides.length - 1);
  let x = PX + PW - 32 - total;
  return provides.map((t, i) => { const w = widths[i]; const s = `<rect class="chip" x="${x}" y="${PY + 34}" width="${w}" height="32" rx="16"/><text class="chiplbl" x="${x + w / 2}" y="${PY + 55}" text-anchor="middle">${t}</text>`; x += w + gap; return s; }).join('');
})();
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
const LY = VY + 90;
const legend = `<g>
<path class="hand" d="M${X0} ${LY} L${X0 + 44} ${LY}"/>${head(X0 + 46, LY, 'r', 'ah')}<text class="legt" x="${X0 + 62}" y="${LY + 6}">Hand-off, arrow shows the direction of data</text>
<path class="hand later" d="M${X0 + 480} ${LY} L${X0 + 524} ${LY}"/>${head(X0 + 526, LY, 'r', 'ah')}<text class="legt" x="${X0 + 542}" y="${LY + 6}">Dashed: later</text>
<path class="stub" d="M${X0 + 740} ${LY} L${X0 + 784} ${LY}"/>${head(X0 + 786, LY, 'r', 'ahp')}<text class="legt" x="${X0 + 802}" y="${LY + 6}">Uses Pitwall for local AI (optional)</text>
</g>`;

const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-labelledby="t d">
<title id="t">The Paddock family</title>
<desc id="d">Seven local-first desktop apps. ${CODEBASE_TOOL}, Debrief and Sighter above, Spotter, Dyno and Pitcrew below, Pitwall the optional local AI hub between them. Hand-offs: ${CODEBASE_TOOL} and Debrief exchange stack traces and log checks, ${CODEBASE_TOOL} sends Sighter test plans and gets results, Sighter sends Debrief run bundles, Debrief and Pacenotes drive Sighter as MCP hosts (Debrief reproduces a bug from a video), Sighter uses Pitwall as its vision model and agent planner, Pitcrew sends job logs, Dyno sends issues, installed software and machine health, Pitcrew runs CI for all, and Spotter will feed Sighter smoke tests later.</desc>
<style>
:root{--ink:#14161A;--muted:#566070;--card:#FFFFFF;--line:#DDE1E8;--soft:#EEF0F4;--link:#8A93A3;--hand:#566070;--found:#F1F3F7;--hubf:#F0ECFB;--hubs:#6D3FD6;--okf:#E6F2EF;--oks:#007A5E}
@media (prefers-color-scheme: dark){:root{--ink:#F2F4F7;--muted:#A6AEBC;--card:#171A20;--line:#2B303A;--soft:#20242C;--link:#667085;--hand:#A6AEBC;--found:#12151A;--hubf:#242040;--hubs:#A58BFA;--okf:#193330;--oks:#2EC4A0}}
text{font-family:-apple-system,"SF Pro Display","SF Pro Text","Segoe UI Variable Display","Segoe UI",Cantarell,"Noto Sans",system-ui,sans-serif;font-variant-numeric:tabular-nums;fill:var(--ink)}
.eyebrow{font-size:15px;font-weight:600;letter-spacing:.16em;fill:var(--muted)}
.h1{font-size:44px;font-weight:600;letter-spacing:-.02em}
.sub{font-size:19px;fill:var(--muted)}
.chip-design{fill:none;stroke:var(--link);stroke-dasharray:3 3}
.chip-dev{fill:var(--soft);stroke:var(--line)}
.chip-soon{fill:var(--hubf);stroke:var(--hubs)}
.chip-ok{fill:var(--okf);stroke:var(--oks)}
.chipt-design,.chipt-dev{font-size:14px;font-weight:500;fill:var(--muted)}
.chipt-soon{font-size:14px;font-weight:600;fill:var(--hubs)}
.chipt-ok{font-size:14px;font-weight:600;fill:var(--oks)}
.cinote{font-size:15px;font-weight:500;fill:var(--muted)}
.card{fill:var(--card);stroke:var(--line);stroke-width:1}
.name{font-size:25px;font-weight:600;letter-spacing:-.01em}
.hubname{font-size:30px}
.promise{font-size:17px;fill:var(--muted)}
.hand{fill:none;stroke:var(--hand);stroke-width:2;stroke-linecap:round;stroke-linejoin:round}
.later{stroke-dasharray:7 6}
.ah{fill:var(--hand)}
.stub{fill:none;stroke:var(--hubs);stroke-width:2;stroke-linecap:round;stroke-dasharray:1 6}
.ahp{fill:var(--hubs)}
.pill{fill:var(--card);stroke:var(--line)}
.lbl{font-size:21.5px;font-weight:500;fill:var(--ink)}
.hub{fill:var(--hubf);stroke:var(--hubs);stroke-width:1.5}
.found{fill:var(--found);stroke:var(--line)}
.chip{fill:var(--card);stroke:var(--line)}
.chiplbl{font-size:15px;font-weight:500}
.ft{font-size:24px;font-weight:600}
.fs{font-size:17px;fill:var(--muted)}
.vt{font-size:17px;font-weight:600}
.vs{font-size:16px;fill:var(--muted)}
.legt{font-size:16px;fill:var(--muted)}
.rule{fill:var(--line)}
.track{fill:none;stroke:var(--line);stroke-width:1.25;opacity:.7}
</style>
<path class="track" d="M-40 230 C 300 80, 520 200, 800 110 S 1300 20, 1660 160"/>

<text class="eyebrow" x="${W / 2}" y="76" text-anchor="middle">PADDOCK</text>
<text class="h1" x="${W / 2}" y="136" text-anchor="middle">Seven local-first tools for people who keep systems running</text>
<text class="sub" x="${W / 2}" y="176" text-anchor="middle">One private AI hub, one shared foundation. Each app is good alone and better together.</text>

${hub}
${stubs}
${cards}
${links.join('\n')}

<g>
<rect class="found" x="${PX}" y="${PY}" width="${PW}" height="${PH}" rx="20"/>
<text class="ft" x="${PX + 32}" y="${PY + 46}">Paddock</text>
<text class="fs" x="${PX + 32}" y="${PY + 74}">The shared foundation under every app.</text>
${chips}
</g>

${valueEls}
${legend}
</svg>
`;
mkdirSync(out, { recursive: true });
writeFileSync(join(out, 'paddock-ecosystem.svg'), svg);
console.log('wrote paddock-ecosystem.svg');

if (process.argv.includes('--png')) {
  const chrome = process.env.CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
  const tmp = process.env.TMPDIR || '/tmp';
  for (const [theme, bg] of [['light', '#ffffff'], ['dark', '#0d1117']]) {
    const html = join(tmp, `paddock-eco-${theme}.html`);
    writeFileSync(html, `<!doctype html><meta name="color-scheme" content="${theme}"><style>html,body{margin:0;background:${bg};color-scheme:${theme}}img{display:block}</style><img src="file://${join(out, 'paddock-ecosystem.svg')}" width="${W}" height="${H}">`);
    execFileSync(chrome, ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--force-device-scale-factor=2',
      `--force-dark-mode=${theme === 'dark'}`, `--screenshot=${join(out, `paddock-ecosystem-${theme}.png`)}`, `--window-size=${W},${H}`, `file://${html}`], { stdio: 'ignore' });
    console.log(`wrote paddock-ecosystem-${theme}.png`);
  }
}
versionPaddockDocs(join(out, '../..'));
