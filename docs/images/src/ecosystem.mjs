// Generates docs/images/paddock-ecosystem.svg (theme-aware, transparent) and, with --png, the 2x light/dark PNGs
// via headless Chrome. Plain Node, no dependencies:  node docs/images/src/ecosystem.mjs [--png]
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { CODEBASE_TOOL } from './family.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, '..');
const W = 1600, H = 1150;

// Row 1: understand and test. Row 3: keep things running. Pitwall is the hub between them.
const row1 = [
  { id: 'codebase', name: CODEBASE_TOOL, tag: 'In design', placeholder: true, lines: ['Everything code-related:', 'index, review, diagrams,', 'requirements and tests.'], stub: 'MCP' },
  { id: 'debrief', name: 'Debrief', accent: '#2F6FE0', lines: ['An IDE for analysing logs', 'and diagnostic files.'] },
  { id: 'sighter', name: 'Sighter', accent: '#00B8D9', tag: 'In design', lines: ['Finds any UI element by', 'sight. Vision models for', 'your UI tests.'] },
];
const row3 = [
  { id: 'spotter', name: 'Spotter', accent: '#0E8577', lines: ['Watches for new releases', 'anywhere, the moment', 'one appears.'] },
  { id: 'dyno', name: 'Dyno', accent: '#C2287C', lines: ['Measures, explains and', 'tunes your PC.'] },
  { id: 'pitcrew', name: 'Pitcrew', accent: '#D2420F', lines: ['Keeps CI runners alive,', 'updated and clean, for', 'every app here.'] },
];
// Short hand-offs between neighbours; the full set is in the integrations diagram.
const gapLinks = [[row1, 0, 'Stack traces', 96], [row1, 1, 'Run bundles', 100], [row3, 0, 'Updates', 76], [row3, 1, 'Machine health', 116]];

const CW = 392, GAP = 132, X0 = (W - (3 * CW + 2 * GAP)) / 2, CH = 152;
const R1 = 252, HUB = 492, HUBH = 152, R3 = HUB + HUBH + 80;
const cardX = (i) => X0 + i * (CW + GAP);

function icon(id, x, y, size) {
  const raw = readFileSync(join(here, 'icons', `${id}.svg`), 'utf8');
  const inner = raw.replace(/^[\s\S]*?<svg[^>]*>/, '').replace(/<\/svg>\s*$/, '').replace(/<title>[^<]*<\/title>/, '');
  return `<svg x="${x}" y="${y}" width="${size}" height="${size}" viewBox="60 60 904 904" overflow="visible">${inner}</svg>`;
}
// Neutral placeholder tile (dashed, monogram) until the tool has its own icon.
function placeholder(name, x, y, size) {
  return `<rect class="ph" x="${x + 1}" y="${y + 1}" width="${size - 2}" height="${size - 2}" rx="${size * 0.225}"/>` +
    `<text class="mono" x="${x + size / 2}" y="${y + size / 2 + 13}" text-anchor="middle">${name[0]}</text>`;
}
function pill(x, y, text, w) {
  return `<g><rect class="pill" x="${x - w / 2}" y="${y - 15}" width="${w}" height="30" rx="15"/>` +
    `<text class="lbl" x="${x}" y="${y + 5}" text-anchor="middle">${text}</text></g>`;
}

function card(a, i, y) {
  const x = cardX(i);
  const ico = a.placeholder ? placeholder(a.name, x + 24, y + 24, 72) : icon(a.id, x + 24, y + 24, 72);
  const bar = `<rect x="${x + 112}" y="${y + 26}" width="20" height="3" rx="1.5" fill="${a.accent ?? 'var(--link)'}"/>`;
  const tag = a.tag ? `<rect class="tag" x="${x + CW - 24 - 76}" y="${y + 18}" width="76" height="24" rx="12"/><text class="tagt" x="${x + CW - 24 - 38}" y="${y + 34}" text-anchor="middle">${a.tag}</text>` : '';
  const text = a.lines.map((l, k) => `<text class="promise" x="${x + 112}" y="${y + 92 + k * 21}">${l}</text>`).join('');
  return `<g><rect class="card" x="${x}" y="${y}" width="${CW}" height="${CH}" rx="16"/>${ico}${bar}${tag}` +
    `<text class="name" x="${x + 112}" y="${y + 60}">${a.name}</text>${text}</g>`;
}
const cards = [...row1.map((a, i) => card(a, i, R1)), ...row3.map((a, i) => card(a, i, R3))].join('\n');

const gaps = gapLinks.map(([row, i, label, w]) => {
  const y = (row === row1 ? R1 : R3) + CH / 2, a = cardX(i) + CW + 6, b = cardX(i + 1) - 6;
  return `<path class="link" d="M${a} ${y} L${b} ${y}"/>${pill((a + b) / 2, y, label, w)}`;
}).join('\n');

// Stubs: every app reaches the hub; only the codebase tool's is labelled because it is a different protocol.
const stubs = [...row1.map((a, i) => [a, i, R1 + CH, HUB]), ...row3.map((a, i) => [a, i, HUB + HUBH, R3])].map(([a, i, y0, y1]) => {
  const x = cardX(i) + CW / 2;
  return `<path class="link" d="M${x} ${y0 + 4} L${x} ${y1 - 4}"/><circle class="dot" cx="${x}" cy="${y1 - 4 === y1 - 4 && y0 < y1 ? (y0 < HUB ? y1 - 4 : y0 + 4) : y1}" r="3.5"/>` +
    (a.stub ? pill(x, (y0 + y1) / 2, a.stub, 56) : '');
}).join('\n');

// Pitwall hub band
const HX = X0, HW = 3 * CW + 2 * GAP;
const cols = [
  ['One local API', ['OpenAI, Anthropic and Ollama', 'compatible, plus /x/v1 extras.']],
  ['Tools over MCP', ['Client and router for the', `${CODEBASE_TOOL} server and others.`]],
  ['Yours alone', ['Keys per app, priorities, caching.', 'No cloud, no telemetry.']],
];
const hubCols = cols.map(([h, ls], i) => {
  const x = HX + 560 + i * 296;
  return `<rect x="${x - 16}" y="${HUB + 38}" width="1.5" height="76" class="rule"/><text class="vt" x="${x}" y="${HUB + 58}">${h}</text>` +
    ls.map((l, k) => `<text class="vs" x="${x}" y="${HUB + 84 + k * 22}">${l}</text>`).join('');
}).join('');
const hub = `<g><rect class="hub" x="${HX}" y="${HUB}" width="${HW}" height="${HUBH}" rx="24"/>
${icon('pitwall', HX + 36, HUB + 36, 80)}
<rect x="${HX + 140}" y="${HUB + 38}" width="20" height="3" rx="1.5" fill="#6D3FD6"/>
<text class="name hubname" x="${HX + 140}" y="${HUB + 76}">Pitwall</text>
<text class="promise" x="${HX + 140}" y="${HUB + 102}">The local AI hub for the family.</text>
<text class="promise" x="${HX + 140}" y="${HUB + 124}">Every app can use it. None needs it.</text>
${hubCols}</g>`;

// Paddock foundation
const PX = X0, PW = HW, PY = R3 + CH + 56, PH = 100;
const provides = ['UI kit', 'Design system', 'Diagram kit', 'Icon set', 'Release practices', 'Lessons'];
const chips = (() => {
  const widths = provides.map((t) => Math.round(t.length * 7.4 + 36)), gap = 14, total = widths.reduce((s, w) => s + w, 0) + gap * (provides.length - 1);
  let x = PX + PW - 32 - total;
  return provides.map((t, i) => { const w = widths[i]; const s = `<rect class="chip" x="${x}" y="${PY + 34}" width="${w}" height="32" rx="16"/><text class="lbl" x="${x + w / 2}" y="${PY + 55}" text-anchor="middle">${t}</text>`; x += w + gap; return s; }).join('');
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

const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-labelledby="t d">
<title id="t">The Paddock family</title>
<desc id="d">Seven local-first desktop apps. ${CODEBASE_TOOL} (in design), Debrief and Sighter (in design) on top; Spotter, Dyno and Pitcrew below; Pitwall, the local AI hub, between them. All built on the shared Paddock foundation.</desc>
<style>
:root{--ink:#14161A;--muted:#566070;--card:#FFFFFF;--line:#DDE1E8;--soft:#EEF0F4;--link:#8A93A3;--found:#F1F3F7;--hubf:#F0ECFB;--hubs:#6D3FD6}
@media (prefers-color-scheme: dark){:root{--ink:#F2F4F7;--muted:#A6AEBC;--card:#171A20;--line:#2B303A;--soft:#20242C;--link:#667085;--found:#12151A;--hubf:#242040;--hubs:#A58BFA}}
text{font-family:-apple-system,"SF Pro Display","SF Pro Text","Segoe UI Variable Display","Segoe UI",Cantarell,"Noto Sans",system-ui,sans-serif;font-variant-numeric:tabular-nums;fill:var(--ink)}
.eyebrow{font-size:14px;font-weight:600;letter-spacing:.16em;fill:var(--muted)}
.h1{font-size:44px;font-weight:600;letter-spacing:-.02em}
.sub{font-size:18px;fill:var(--muted)}
.tag{fill:none;stroke:var(--link);stroke-dasharray:3 3}
.tagt{font-size:12px;font-weight:500;fill:var(--muted)}
.card{fill:var(--card);stroke:var(--line);stroke-width:1}
.ph{fill:var(--soft);stroke:var(--link);stroke-width:1.5;stroke-dasharray:5 4}
.mono{font-size:38px;font-weight:600;fill:var(--muted)}
.name{font-size:22px;font-weight:600;letter-spacing:-.01em}
.hubname{font-size:28px}
.promise{font-size:15px;fill:var(--muted)}
.link{fill:none;stroke:var(--link);stroke-width:1.5;stroke-linecap:round;stroke-dasharray:1 6}
.dot{fill:var(--hubs)}
.pill{fill:var(--card);stroke:var(--line)}
.lbl{font-size:13px;font-weight:500;fill:var(--ink)}
.hub{fill:var(--hubf);stroke:var(--hubs);stroke-width:1.5}
.found{fill:var(--found);stroke:var(--line)}
.chip{fill:var(--card);stroke:var(--line)}
.ft{font-size:22px;font-weight:600}
.fs{font-size:15px;fill:var(--muted)}
.vt{font-size:15px;font-weight:600}
.vs{font-size:14px;fill:var(--muted)}
.rule{fill:var(--line)}
.track{fill:none;stroke:var(--line);stroke-width:1.25;opacity:.7}
</style>
<path class="track" d="M-40 230 C 300 80, 520 200, 800 110 S 1300 20, 1660 160"/>
<path class="track" d="M-40 260 C 300 110, 520 230, 800 140 S 1300 50, 1660 190"/>

<text class="eyebrow" x="${W / 2}" y="84" text-anchor="middle">PADDOCK</text>
<text class="h1" x="${W / 2}" y="146" text-anchor="middle">Seven local-first tools for people who keep systems running</text>
<text class="sub" x="${W / 2}" y="186" text-anchor="middle">One private AI hub, one shared foundation. Each app is good alone and better together.</text>

${gaps}
${stubs}
${hub}
${cards}

<g>
<rect class="found" x="${PX}" y="${PY}" width="${PW}" height="${PH}" rx="20"/>
<text class="ft" x="${PX + 32}" y="${PY + 46}">Paddock</text>
<text class="fs" x="${PX + 32}" y="${PY + 74}">The shared foundation under every app.</text>
${chips}
</g>

${valueEls}
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
