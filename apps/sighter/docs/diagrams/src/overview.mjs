// Sighter overview hero (theme-aware SVG, ecosystem style). Writes docs/images/sighter-overview.svg.
//   node docs/diagrams/src/overview.mjs [--png]      (--png also renders light and dark 2x PNGs with headless Chrome)
// Needs the Paddock checkout next to this repo (icons, glyphs, kit font metrics).
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { tw } from '../../../../paddock/diagrams/tools/lib.mjs';
import { GLYPHS } from '../../../../paddock/docs/images/src/glyphs.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, '../../images');
const ICONS = join(here, '../../../../paddock/docs/images/src/icons');
const W = 1600;
const X0 = 48, CW = 214, GAP = 44, CT = 352, CHT = 344;
const colX = (i) => X0 + i * (CW + GAP);
const colMid = (i) => colX(i) + CW / 2;
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;');
const o = [];

function icon(id, x, y, size) {
  const raw = readFileSync(join(ICONS, `${id}.svg`), 'utf8');
  const inner = raw.replace(/^[\s\S]*?<svg[^>]*>/, '').replace(/<\/svg>\s*$/, '').replace(/<title>[^<]*<\/title>/, '');
  return `<svg x="${x}" y="${y}" width="${size}" height="${size}" viewBox="60 60 904 904" overflow="visible">${inner}</svg>`;
}
const glyph = (name, cx, cy, size, cls = 'gl') =>
  `<g class="${cls}" transform="translate(${cx - size / 2} ${cy - size / 2}) scale(${size / 24})">${GLYPHS[name]}</g>`;
const head = (x, y, dir, cls) => {
  const p = { u: `${x},${y} ${x - 6},${y + 11} ${x + 6},${y + 11}`, d: `${x},${y} ${x - 6},${y - 11} ${x + 6},${y - 11}`,
    l: `${x},${y} ${x + 11},${y - 6} ${x + 11},${y + 6}`, r: `${x},${y} ${x - 11},${y - 6} ${x - 11},${y + 6}` }[dir];
  return `<polygon class="${cls}" points="${p}"/>`;
};
const pill = (cx, cy, text, cls = 'pill', tcls = 'lbl') => {
  const w = Math.round(tw(text, 16) * 1.12 + 32);
  return `<rect class="${cls}" x="${cx - w / 2}" y="${cy - 17}" width="${w}" height="34" rx="17"/><text class="${tcls}" x="${cx}" y="${cy + 5.5}" text-anchor="middle">${esc(text)}</text>`;
};

// ---- header ----
o.push(icon('sighter', 48, 44, 92));
o.push(`<text class="eyebrow" x="164" y="76">SIGHTER</text>`);
o.push(`<text class="h1" x="164" y="126">Record it. Label it. Train it. Test it.</text>`);
o.push(`<text class="sub" x="164" y="160">A local vision locator for UI automation: your tests find any element by sight, on your machine.</text>`);
o.push(`<text class="sub2" x="164" y="188">Local only  ·  never a blind click  ·  permissive licences only</text>`);
const planned = 'Planned: nothing is built yet';
const pw = Math.round(tw(planned, 14) + 34);
o.push(`<rect class="chip-plan" x="${W - 48 - pw}" y="52" width="${pw}" height="30" rx="15"/><text class="chipt" x="${W - 48 - pw / 2}" y="72" text-anchor="middle">${planned}</text>`);

// ---- stage cards ----
const stages = [
  { n: '01', name: 'Record', g: 'record', out: 'Frames and events', lines: ['Screen and input events', 'Studio or headless CLI', 'Follows app focus', 'Accessibility optional'] },
  { n: '02', name: 'Label', g: 'tag', lines: ['Suggestions from OCR,', 'templates and detector', 'Human review queue for', 'anything unidentified'], pw: 'Pitwall vision helper' },
  { n: '03', name: 'Train', g: 'cube', out: 'Immutable model vN', lines: ['Permissive YOLO-style', 'detector per app', 'DPI, theme and size', 'variation, augmentation', 'Versioned model registry'] },
  { n: '04', name: 'Serve', g: 'server', lines: ['Locator API, loopback'], resolver: true },
  { n: '05', name: 'Test', g: 'checklist', lanes: true },
  { n: '06', name: 'Results', g: 'doc', out: 'For CI and Debrief', lines: ['JUnit, TRX and Allure', 'Debrief run bundles with', 'video and screenshots', 'Requirement traceability', 'with Pacenotes'] },
];
stages.forEach((s, i) => {
  const x = colX(i), y = CT;
  o.push(`<rect class="card" x="${x}" y="${y}" width="${CW}" height="${CHT}" rx="20"/>`);
  o.push(`<circle class="gc" cx="${x + 42}" cy="${y + 46}" r="24"/>${glyph(s.g, x + 42, y + 46, 26)}`);
  o.push(`<text class="num" x="${x + 82}" y="${y + 38}">${s.n}</text><text class="name" x="${x + 82}" y="${y + 66}">${s.name}</text>`);
  if (s.lines) s.lines.forEach((l, k) => o.push(`<text class="body" x="${x + 24}" y="${y + 118 + k * 24}">${esc(l)}</text>`));
  if (s.out) o.push(`<path class="outl" d="M${x + 24} ${y + CHT - 52} L${x + CW - 24} ${y + CHT - 52}"/><text class="outt" x="${x + 24}" y="${y + CHT - 24}">→ ${s.out}</text>`);
  if (s.pw) {
    const py = y + CHT - 62;
    o.push(`<rect class="pwchip" x="${x + 16}" y="${py}" width="${CW - 32}" height="40" rx="12"/>${icon('pitwall', x + 28, py + 8, 24)}<text class="pwt" x="${x + 62}" y="${py + 25}">${s.pw}</text>`);
  }
  if (s.resolver) {
    const layers = ['Cache', 'Template', 'OCR anchor', 'Detector', 'Pitwall grounding', 'Needs labelling'];
    layers.forEach((l, k) => {
      const ly = y + 124 + k * 34, cls = k === 4 ? 'lay pwlay' : k === 5 ? 'lay needs' : 'lay';
      o.push(`<rect class="${cls}" x="${x + 16}" y="${ly}" width="${CW - 32}" height="28" rx="9"/><text class="layt" x="${x + 30}" y="${ly + 19}">${l}</text>`);
      if (k < 5) o.push(`<text class="layn" x="${x + CW - 30}" y="${ly + 20}" text-anchor="end">${k + 1}</text>`);
    });
  }
  if (s.lanes) {
    const lh = (CHT - 112 - 12) / 2;
    const lane = (ly, title, ls) => `<rect class="lane" x="${x + 14}" y="${ly}" width="${CW - 28}" height="${lh}" rx="14"/>` +
      `<text class="lt" x="${x + 28}" y="${ly + 28}">${title}</text>` + ls.map((l, k) => `<text class="body sm" x="${x + 28}" y="${ly + 52 + k * 20}">${l}</text>`).join('');
    o.push(lane(y + 96, 'Engineers’ tests', ['C#, Java, C++, Python', 'alongside FlaUI, Appium', 'and other frameworks']));
    o.push(lane(y + 96 + lh + 12, 'Sighter’s own tests', ['Recorded flows,', 'sighter run in CI', 'One test, several apps']));
  }
  // flow arrow into the next card
  if (i < stages.length - 1) {
    const ax = x + CW + 4, bx = colX(i + 1) - 4, ay = y + 46;
    o.push(`<path class="flow" d="M${ax} ${ay} L${bx - 8} ${ay}"/>${head(bx, ay, 'r', 'ah')}`);
  }
});
// Local API surfaces both test lanes: nothing extra to draw; Serve -> Test arrow covers it.

// ---- loops over the top (nested, no crossings) ----
const arc = (x1, x2, apex, cls) => {
  const y = CT - 4, c = apex;
  return `<path class="${cls}" d="M${x1} ${y - 11} C${x1} ${c}, ${x2} ${c}, ${x2} ${y - 11}"/>${head(x2, y, 'd', cls === 'loopa' ? 'aha' : 'ah')}`;
};
o.push(arc(colMid(3), colMid(1) + 40, 258, 'loopa'));
o.push(pill((colMid(3) + colMid(1) + 40) / 2, CT - 4 - 11 - 0.75 * (CT - 15 - 262), 'Low confidence or UI change → retrain', 'pillacc', 'lbl'));
o.push(arc(colMid(4), colMid(1) - 40, 196, 'loopb'));
o.push(pill((colMid(4) + colMid(1) - 40) / 2, CT - 15 - 0.75 * (CT - 15 - 196), 'Recovery proposals → studio', 'pill', 'lbl'));

// ---- recovery capsule under Test and Results ----
const RX = colX(4), RW = colX(5) + CW - RX, RY = CT + CHT + 52, RH = 204;
o.push(`<rect class="cap" x="${RX}" y="${RY}" width="${RW}" height="${RH}" rx="20"/>`);
o.push(`<text class="eyebrow2" x="${RX + 24}" y="${RY + 32}">RECOVERY LOOP</text>`);
const chip = (x, y, w, h, lines, cls = 'rc') => `<rect class="${cls}" x="${x}" y="${y}" width="${w}" height="${h}" rx="12"/>` +
  lines.map((l, k) => `<text class="rt" x="${x + w / 2}" y="${y + h / 2 + 5 + (k - (lines.length - 1) / 2) * 19}" text-anchor="middle">${esc(l)}</text>`).join('');
const r1 = RY + 50, r2 = RY + 140, ch = 52;
const wA = 168, wB = 208;
o.push(chip(RX + 24, r1, wA, ch, ['Step is stuck']));
o.push(`<path class="flow" d="M${RX + 24 + wA + 6} ${r1 + ch / 2} L${RX + 24 + wA + 28} ${r1 + ch / 2}"/>${head(RX + 24 + wA + 36, r1 + ch / 2, 'r', 'ah')}`);
const pxx = RX + 24 + wA + 44;
o.push(`<rect class="rc rcpw" x="${pxx}" y="${r1}" width="${RW - 24 - (pxx - RX)}" height="${ch}" rx="12"/>${icon('pitwall', pxx + 12, r1 + 14, 24)}` +
  `<text class="rt" x="${pxx + 46}" y="${r1 + 22}">Pitwall diagnosis</text><text class="rt" x="${pxx + 46}" y="${r1 + 41}">and plan</text>`);
const w3 = (RW - 48 - 2 * 36) / 3;
const xs = [0, 1, 2].map((k) => RX + 24 + k * (w3 + 36));
o.push(chip(xs[2], r2, w3, ch, ['Guarded', 'recovery']));
o.push(chip(xs[1], r2, w3, ch, ['Passed with', 'recovery'], 'rc rcwarn'));
o.push(chip(xs[0], r2, w3, ch, ['Proposal for', 'the studio']));
[1, 2].forEach((k) => o.push(`<path class="flow" d="M${xs[k] - 4} ${r2 + ch / 2} L${xs[k] - 24} ${r2 + ch / 2}"/>${head(xs[k] - 34, r2 + ch / 2, 'l', 'ah')}`));
const gx = xs[2] + w3 / 2;
o.push(`<path class="flow" d="M${gx} ${r1 + ch + 4} L${gx} ${r2 - 14}"/>${head(gx, r2 - 4, 'd', 'ah')}`);
// Test card -> capsule
o.push(`<path class="flow" d="M${colMid(4)} ${CT + CHT + 4} L${colMid(4)} ${RY - 12}"/>${head(colMid(4), RY - 4, 'd', 'ah')}`);
o.push(`<text class="mut2" x="${colMid(4) + 14}" y="${CT + CHT + 34}">A step fails</text>`);

// ---- Pitwall band ----
const BY = RY + RH + 56, BH = 92, BW = W - 96;
o.push(`<rect class="hub" x="${X0}" y="${BY}" width="${BW}" height="${BH}" rx="22"/>`);
o.push(icon('pitwall', X0 + 26, BY + 16, 60));
o.push(`<text class="hubname" x="${X0 + 106}" y="${BY + 42}">Pitwall</text><text class="body mut" x="${X0 + 106}" y="${BY + 68}">The local AI hub. Optional: Sighter works without it.</text>`);
const hx = (t) => X0 + 560 + t;
['Suggests and names labels', 'Grounds an unseen element', 'Diagnoses a stuck step and plans a recovery'].forEach((t, k) => {
  const xx = [hx(0), hx(250), hx(500)][k];
  o.push(`<rect x="${xx - 14}" y="${BY + 24}" width="2" height="44" rx="1" class="rule"/><text class="hubt" x="${xx}" y="${BY + 44}">${['Label', 'Serve', 'Test'][k]}</text>`);
  o.push(`<text class="hubs" x="${xx}" y="${BY + 68}">${t}</text>`);
});
// dotted stubs from the cards (Label, Serve) and the recovery chip down to the band
const stub = (x, y0, y1, label) => `<path class="stub" d="M${x} ${y0} L${x} ${y1}"/>${head(x, y1, 'd', 'ahp')}<path class="stub" d="M${x} ${y1} L${x} ${y0}" opacity="0"/>${pill(x, (y0 + y1) / 2 - 8, label, 'pill', 'lbl')}`;
const stubTop = CT + CHT + 4;
o.push(stub(colMid(1), stubTop, BY - 3, 'Suggestions'));
o.push(stub(colMid(3), stubTop, BY - 3, 'Grounding'));
o.push(`<path class="stub" d="M${pxx + (RW - 24 - (pxx - RX)) / 2} ${RY + RH + 2} L${pxx + (RW - 24 - (pxx - RX)) / 2} ${BY - 3}"/>${head(pxx + (RW - 24 - (pxx - RX)) / 2, BY - 3, 'd', 'ahp')}`);

// ---- legend ----
const VY = BY + BH;
const LY = VY + 48;
o.push(`<path class="flow" d="M${X0} ${LY} L${X0 + 34} ${LY}"/>${head(X0 + 44, LY, 'r', 'ah')}<text class="legt" x="${X0 + 62}" y="${LY + 5}">Flow</text>`);
o.push(`<path class="loopa" d="M${X0 + 170} ${LY} L${X0 + 210} ${LY}"/>${head(X0 + 220, LY, 'r', 'aha')}<text class="legt" x="${X0 + 238}" y="${LY + 5}">Retrain loop</text>`);
o.push(`<path class="stub" d="M${X0 + 400} ${LY} L${X0 + 440} ${LY}"/>${head(X0 + 450, LY, 'r', 'ahp')}<text class="legt" x="${X0 + 468}" y="${LY + 5}">Uses Pitwall (optional)</text>`);
const Hh = LY + 34;

const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${Hh}" viewBox="0 0 ${W} ${Hh}" role="img" aria-labelledby="t d">
<title id="t">How Sighter works</title>
<desc id="d">Planned, nothing built yet. Six stages left to right: record screen and input events; label with OCR, template and detector suggestions, optional Pitwall vision and a human review queue; train a permissive YOLO-style detector with variation and a model registry; serve a local API with a layered resolver (cache, template, OCR anchor, detector, Pitwall grounding, needs labelling); test, in engineers' C#, Java or C++ tests through the locator client and in Sighter's own recorded flows; results as JUnit, TRX, Allure, Debrief run bundles and Pacenotes traceability. A retrain loop sends low-confidence results or UI changes to the review queue and on to retraining. A recovery loop sends a stuck step to Pitwall for a diagnosis and plan, runs a guarded recovery, reports passed with recovery and sends a proposal to the studio.</desc>
<style>
:root{--ink:#14161A;--muted:#566070;--card:#FFFFFF;--line:#DDE1E8;--soft:#EEF0F4;--hand:#566070;--acc:#0A8CA8;--accf:#E3F4F7;--hubf:#F0ECFB;--hubs:#6D3FD6;--warn:#B25E00;--warnf:#FBF0E1;--bgc:#F6F8FA}
@media (prefers-color-scheme: dark){:root{--ink:#F2F4F7;--muted:#A6AEBC;--card:#171A20;--line:#2B303A;--soft:#20242C;--hand:#A6AEBC;--acc:#00B8D9;--accf:#0F2E36;--hubf:#242040;--hubs:#A58BFA;--warn:#F2A33A;--warnf:#33281A;--bgc:#12151A}}
text{font-family:-apple-system,"SF Pro Display","SF Pro Text","Segoe UI Variable Display","Segoe UI",Cantarell,"Noto Sans",system-ui,sans-serif;fill:var(--ink)}
.eyebrow{font-size:15px;font-weight:600;letter-spacing:.18em;fill:var(--acc)}
.eyebrow2{font-size:13px;font-weight:600;letter-spacing:.14em;fill:var(--muted)}
.h1{font-size:42px;font-weight:600;letter-spacing:-.02em}
.sub{font-size:19px;fill:var(--muted)}.sub2{font-size:15px;font-weight:500;letter-spacing:.04em;fill:var(--muted)}.outl{stroke:var(--line);stroke-width:1}.outt{font-size:15px;font-weight:500;fill:var(--ink)}
.chip-plan{fill:none;stroke:var(--muted);stroke-dasharray:3 3}.chipt{font-size:14px;font-weight:500;fill:var(--muted)}
.card{fill:var(--card);stroke:var(--line)}
.gc{fill:var(--accf);stroke:var(--acc);stroke-width:1.2}
.gl{fill:none;stroke:var(--acc);stroke-width:1.8;stroke-linecap:round;stroke-linejoin:round}
.num{font-size:13px;font-weight:600;letter-spacing:.14em;fill:var(--acc)}
.name{font-size:25px;font-weight:600;letter-spacing:-.01em}
.body{font-size:16px;fill:var(--muted)}.sm{font-size:15px}.mut{fill:var(--muted)}
.mut2{font-size:15px;fill:var(--muted)}
.lay{fill:var(--soft);stroke:var(--line)}.layt{font-size:15px;font-weight:500}.layn{font-size:13px;fill:var(--muted)}
.pwlay{fill:var(--hubf);stroke:var(--hubs);stroke-dasharray:3 3}.needs{fill:none;stroke:var(--acc);stroke-width:1.5}
.pwchip{fill:var(--hubf);stroke:var(--hubs);stroke-dasharray:3 3}.pwt{font-size:14px;font-weight:500}
.lane{fill:var(--soft);stroke:var(--line)}.lt{font-size:16px;font-weight:600}
.flow{fill:none;stroke:var(--hand);stroke-width:2;stroke-linecap:round}.ah{fill:var(--hand)}
.loopa{fill:none;stroke:var(--acc);stroke-width:2.5;stroke-linecap:round}.aha{fill:var(--acc)}
.loopb{fill:none;stroke:var(--hand);stroke-width:2;stroke-linecap:round}
.pill{fill:var(--card);stroke:var(--line)}.pillacc{fill:var(--accf);stroke:var(--acc)}.lbl{font-size:16px;font-weight:500}
.cap{fill:var(--bgc);stroke:var(--line)}
.rc{fill:var(--card);stroke:var(--line)}.rcpw{fill:var(--hubf);stroke:var(--hubs);stroke-dasharray:3 3}.rcwarn{fill:var(--warnf);stroke:var(--warn)}
.rt{font-size:14.5px;font-weight:500}
.stub{fill:none;stroke:var(--hubs);stroke-width:2;stroke-linecap:round;stroke-dasharray:1 6}.ahp{fill:var(--hubs)}
.hub{fill:var(--hubf);stroke:var(--hubs);stroke-width:1.5}.hubname{font-size:28px;font-weight:600}.hubt{font-size:15px;font-weight:600}.hubs{font-size:15px;fill:var(--muted)}
.vt{font-size:17px;font-weight:600}.vs{font-size:16px;fill:var(--muted)}.legt{font-size:15px;fill:var(--muted)}.rule{fill:var(--line)}
</style>
${o.join('\n')}
</svg>
`;
mkdirSync(out, { recursive: true });
writeFileSync(join(out, 'sighter-overview.svg'), svg);
console.log('wrote sighter-overview.svg', W, Hh);

if (process.argv.includes('--png')) {
  const chrome = process.env.CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
  const tmp = process.env.TMPDIR || '/tmp';
  const png = join(out, 'diagrams/png');
  mkdirSync(png, { recursive: true });
  for (const [theme, bg] of [['light', '#ffffff'], ['dark', '#0d1117']]) {
    const html = join(tmp, `sighter-ov-${theme}.html`);
    writeFileSync(html, `<!doctype html><meta name="color-scheme" content="${theme}"><style>html,body{margin:0;background:${bg};color-scheme:${theme}}img{display:block}</style><img src="file://${join(out, 'sighter-overview.svg')}" width="${W}" height="${Hh}">`);
    execFileSync(chrome, ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--force-device-scale-factor=2',
      `--force-dark-mode=${theme === 'dark'}`, `--screenshot=${join(png, `sighter-overview-${theme}.png`)}`, `--window-size=${W},${Hh}`, `file://${html}`], { stdio: 'ignore' });
    console.log(`wrote sighter-overview-${theme}.png`);
  }
}
