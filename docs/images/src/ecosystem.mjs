// Generates docs/images/paddock-ecosystem.svg (theme-aware, transparent) and, with --png, the 2x light/dark PNGs
// via headless Chrome. Plain Node, no dependencies:  node docs/images/src/ecosystem.mjs [--png]
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, '..');
const W = 1600, H = 1000;

const apps = [
  { id: 'pitcrew', name: 'Pitcrew', accent: '#D2420F', lines: ['Keeps your CI runners alive,', 'updated and clean.'] },
  { id: 'debrief', name: 'Debrief', accent: '#2F6FE0', lines: ['An IDE for analysing logs', 'and diagnostic files.'] },
  { id: 'pitwall', name: 'Pitwall', accent: '#6D3FD6', lines: ['The private, local LLM', 'server your apps talk to.'] },
  { id: 'spotter', name: 'Spotter', accent: '#0E8577', lines: ['Watches for new releases', 'anywhere, the moment one', 'appears.'] },
  { id: 'dyno', name: 'Dyno', accent: '#C2287C', lines: ['Measures, explains', 'and tunes your PC.'] },
];

const CW = 264, GAP = 32, X0 = (W - (5 * CW + 4 * GAP)) / 2, CY = 372, CH = 224;
const cx = (i) => X0 + i * (CW + GAP) + CW / 2;
const idx = Object.fromEntries(apps.map((a, i) => [a.id, i]));

function icon(id, x, y, size) {
  const raw = readFileSync(join(here, 'icons', `${id}.svg`), 'utf8');
  const inner = raw.replace(/^[\s\S]*?<svg[^>]*>/, '').replace(/<\/svg>\s*$/, '').replace(/<title>[^<]*<\/title>/, '');
  return `<svg x="${x}" y="${y}" width="${size}" height="${size}" viewBox="60 60 904 904" overflow="visible">${inner}</svg>`;
}

// Pill label centred at (x, y).
function pill(x, y, text, w) {
  return `<g><rect class="pill" x="${x - w / 2}" y="${y - 16}" width="${w}" height="32" rx="16"/>` +
    `<text class="lbl" x="${x}" y="${y + 5}" text-anchor="middle">${text}</text></g>`;
}

// Link between two cards: arc over (up) or under (down) the row, with a label pill at the apex and an arrow head at `to`.
function link(from, to, dir, depth, label, w, arrow = true) {
  const a = cx(idx[from]) + (idx[to] > idx[from] ? 10 : -10), b = cx(idx[to]) + (idx[to] > idx[from] ? -10 : 10);
  const y0 = dir === 'up' ? CY : CY + CH, s = dir === 'up' ? -1 : 1;
  const ya = y0 + s * 2, top = y0 + s * depth, r = 28;
  const d = `M${a} ${ya} L${a} ${top - s * r} Q${a} ${top} ${a + Math.sign(b - a) * r} ${top} L${b - Math.sign(b - a) * r} ${top} Q${b} ${top} ${b} ${top - s * r} L${b} ${ya + s * 8}`;
  const head = arrow ? `<path class="arrow" d="M${b - 5} ${ya + s * 14} L${b} ${ya + s * 6} L${b + 5} ${ya + s * 14}" />` : '';
  return `<path class="link" d="${d}"/>${head}${pill((a + b) / 2, top, label, w)}`;
}

const cards = apps.map((a, i) => {
  const x = X0 + i * (CW + GAP);
  const text = a.lines.map((l, k) => `<text class="promise" x="${x + 24}" y="${CY + 160 + k * 22}">${l}</text>`).join('');
  return `<g>
  <rect class="card" x="${x}" y="${CY}" width="${CW}" height="${CH}" rx="16"/>
  ${icon(a.id, x + 24, CY + 24, 72)}
  <rect x="${x + 24}" y="${CY + 108}" width="20" height="3" rx="1.5" fill="${a.accent}"/>
  <text class="name" x="${x + 24}" y="${CY + 136}" ${''}>${a.name}</text>
  ${text}
</g>`;
}).join('\n');

const links = [
  link('pitcrew', 'debrief', 'up', 56, 'Open job log in Debrief', 188),
  link('pitwall', 'debrief', 'up', 56, 'Local AI: Explain, research', 196),
  link('pitwall', 'spotter', 'up', 56, 'Local AI: &quot;What&#8217;s new&quot;', 196),
  link('dyno', 'spotter', 'up', 56, 'Watch for updates', 148),
  link('dyno', 'debrief', 'down', 52, 'Open in Debrief', 132),
  link('dyno', 'pitcrew', 'down', 92, 'Machine health in Pitcrew', 196),
].join('\n');

// Paddock foundation
const PX = X0, PW = 5 * CW + 4 * GAP, PY = 732, PH = 120;
const provides = ['UI kit', 'Design system', 'Diagram kit', 'CI and release practices'];
const chips = (() => {
  const widths = [88, 136, 128, 212], gap = 16, total = widths.reduce((s, w) => s + w, 0) + gap * 3;
  let x = PX + PW - 32 - total;
  return provides.map((t, i) => { const w = widths[i]; const s = `<rect class="chip" x="${x}" y="${PY + 44}" width="${w}" height="32" rx="16"/><text class="lbl" x="${x + w / 2}" y="${PY + 65}" text-anchor="middle">${t}</text>`; x += w + gap; return s; }).join('');
})();

const values = [
  ['Local-only, private by design', 'No cloud. No telemetry.'],
  ['Windows, macOS and Linux', 'One family, native on each.'],
  ['Rust and Angular', 'Fast engines, native feel.'],
];
const VY = 900;
const valueEls = values.map(([t, s], i) => {
  const x = X0 + i * ((PW) / 3) + 0;
  return `<rect x="${x}" y="${VY - 4}" width="2" height="44" rx="1" class="rule"/><text class="vt" x="${x + 16}" y="${VY + 14}">${t}</text><text class="vs" x="${x + 16}" y="${VY + 36}">${s}</text>`;
}).join('');

const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-labelledby="t d">
<title id="t">The Paddock family</title>
<desc id="d">Pitcrew, Debrief, Pitwall, Spotter and Dyno, five local-first desktop apps, connected by a few hand-offs and built on the shared Paddock foundation.</desc>
<style>
:root{--ink:#14161A;--muted:#566070;--card:#FFFFFF;--line:#DDE1E8;--soft:#EEF0F4;--link:#8A93A3;--found:#F1F3F7}
@media (prefers-color-scheme: dark){:root{--ink:#F2F4F7;--muted:#A6AEBC;--card:#171A20;--line:#2B303A;--soft:#20242C;--link:#667085;--found:#12151A}}
text{font-family:-apple-system,"SF Pro Display","SF Pro Text","Segoe UI Variable Display","Segoe UI",Cantarell,"Noto Sans",system-ui,sans-serif;font-variant-numeric:tabular-nums;fill:var(--ink)}
.eyebrow{font-size:14px;font-weight:600;letter-spacing:.16em;fill:var(--muted)}
.h1{font-size:46px;font-weight:600;letter-spacing:-.02em}
.sub{font-size:18px;fill:var(--muted)}
.card{fill:var(--card);stroke:var(--line);stroke-width:1}
.name{font-size:22px;font-weight:600;letter-spacing:-.01em}
.promise{font-size:15px;fill:var(--muted)}
.link{fill:none;stroke:var(--link);stroke-width:1.5;stroke-linecap:round;stroke-linejoin:round;stroke-dasharray:1 6}
.arrow{fill:none;stroke:var(--link);stroke-width:1.5;stroke-linecap:round;stroke-linejoin:round}
.pill{fill:var(--card);stroke:var(--line)}
.lbl{font-size:13px;font-weight:500;fill:var(--ink)}
.found{fill:var(--found);stroke:var(--line)}
.chip{fill:var(--card);stroke:var(--line)}
.ft{font-size:22px;font-weight:600}
.fs{font-size:15px;fill:var(--muted)}
.vt{font-size:15px;font-weight:600}
.vs{font-size:14px;fill:var(--muted)}
.rule{fill:var(--line)}
.track{fill:none;stroke:var(--line);stroke-width:1.25;opacity:.7}
</style>
<!-- abstract track curve, very quiet -->
<path class="track" d="M-40 300 C 300 120, 520 260, 800 150 S 1300 40, 1660 210"/>
<path class="track" d="M-40 330 C 300 150, 520 290, 800 180 S 1300 70, 1660 240"/>

<text class="eyebrow" x="${W / 2}" y="88" text-anchor="middle">PADDOCK</text>
<text class="h1" x="${W / 2}" y="152" text-anchor="middle">Five local-first tools for people who keep systems running</text>
<text class="sub" x="${W / 2}" y="192" text-anchor="middle">Built on one foundation, each good alone, better together.</text>

${links}
${cards}

<g>
<rect class="found" x="${PX}" y="${PY}" width="${PW}" height="${PH}" rx="20"/>
<text class="ft" x="${PX + 32}" y="${PY + 52}">Paddock</text>
<text class="fs" x="${PX + 32}" y="${PY + 80}">The shared foundation under every app.</text>
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
