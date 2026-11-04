// Marketing art for every Paddock app, from one small data file per app (apps/<app>.mjs). Plain Node, no dependencies.
//   node docs/images/src/app-poster.mjs <app|all> [--png]
// Writes into the app's own checkout (sibling directory of paddock, or $PADDOCK_APPS/<app>):
// (an app with `home` in its data file, a planned app or tool with no folder under apps/ yet, writes into that folder of this repository
// instead; `stem` replaces <app> in the file names, e.g. `recce-placeholder` for placeholder art)
//   docs/images/<app>-hero.svg          1600x900, theme-aware, real screenshot in a window frame (a concept mock for planned apps,
//                                       an illustration for apps with `illustrated: true` whose screenshots are out of date)
//   docs/images/<app>-features.svg      four feature panels with zoomed screenshot crops, plus the "Works with" strip
//   docs/images/<app>-how-it-works.svg  one line of 3-5 steps
//   docs/images/<app>-social.svg        1280x640 repository card
// and with --png the 2x light/dark PNGs (the social card is exactly 1280x640: <app>-social.png is dark, -social-light.png light).
// Screenshots are read from the app's docs/screenshots, embedded as downsized JPEG data URIs (an SVG used as <img> cannot load files).
import { readFileSync, writeFileSync, mkdirSync, existsSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { versionFile } from './version-links.mjs';
import { screenshotSvg } from './chrome.mjs';
import { crops } from './shots.mjs';
import { glyph } from './glyphs.mjs';
import { STATUS } from './family.mjs';
import * as mock from './mocks.mjs';
import { MOCK_CSS } from './mocks.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const paddock = join(here, '../../..');
const appsRoot = process.env.PADDOCK_APPS || join(paddock, '..');
/** The app's own folder: `home` (relative to this repository) for a planned app that has no folder under apps/ yet, else apps/<id>. */
const appHome = (d, id) => (d.home ? join(paddock, d.home) : join(appsRoot, id));
const FONT = '-apple-system,"SF Pro Display","SF Pro Text","Segoe UI Variable Display","Segoe UI",Cantarell,"Noto Sans",system-ui,sans-serif';

// ---- data --------------------------------------------------------------------------------------------------------------
const ids = readdirSync(join(here, 'apps')).filter((f) => f.endsWith('.mjs')).map((f) => f.slice(0, -4));
const load = async (id) => (await import(`./apps/${id}.mjs`)).default;
const accents = {};
for (const id of ids) accents[id] = (await load(id)).accent;
const names = {};
for (const id of ids) names[id] = (await load(id)).name;

function iconSvg(id, x, y, size) {
  const f = join(here, 'icons', `${id}.svg`);
  if (!existsSync(f)) { // no icon yet: dashed monogram tile
    const nm = names[id] ?? id;
    return `<rect class="ph" x="${x + 1}" y="${y + 1}" width="${size - 2}" height="${size - 2}" rx="${size * 0.225}"/><text class="mono" x="${x + size / 2}" y="${y + size / 2 + size * 0.17}" text-anchor="middle" style="font-size:${size * 0.5}px">${nm[0]}</text>`;
  }
  const raw = readFileSync(f, 'utf8');
  const inner = raw.replace(/^[\s\S]*?<svg[^>]*>/, '').replace(/<\/svg>\s*$/, '').replace(/<title>[^<]*<\/title>/, '');
  return `<svg x="${x}" y="${y}" width="${size}" height="${size}" viewBox="60 60 904 904" overflow="visible">${inner}</svg>`;
}

// ---- shared pieces -----------------------------------------------------------------------------------------------------
const hex2rgb = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
function css(a) {
  const [r, g, b] = hex2rgb(a.fill);
  return `<style>
:root{--ink:#14161A;--muted:#566070;--card:#FFFFFF;--line:#DDE1E8;--soft:#F1F3F7;--bar:#ECEEF2;--link:#8A93A3;--codebg:#F7F8FA;--acc:${a.light};--acct:${a.light};--accf:${a.fill};--glow:.20;--shadow:.22;--wbg:#FFFFFF}
@media (prefers-color-scheme: dark){:root{--ink:#F2F4F7;--muted:#A6AEBC;--card:#171A20;--line:#2B303A;--soft:#1B1E24;--bar:#20242C;--link:#667085;--codebg:#12151A;--acc:${a.dark};--acct:${a.dark};--glow:.26;--shadow:.55;--wbg:#0E0F12}}
text{font-family:${FONT};font-variant-numeric:tabular-nums;fill:var(--ink)}
.lt{display:inline}.dk{display:none}
@media (prefers-color-scheme: dark){.lt{display:none}.dk{display:inline}}
.eyebrow{font-size:15px;font-weight:600;letter-spacing:.14em;fill:var(--acct)}
.h1{font-size:56px;font-weight:600;letter-spacing:-.025em}
.h2{font-size:42px;font-weight:600;letter-spacing:-.02em}
.sub{font-size:22px;fill:var(--muted)}
.small{font-size:20px;fill:var(--muted)}
.pilltxt{font-size:17px;font-weight:600;fill:var(--acct)}
.name{font-size:26px;font-weight:600;letter-spacing:-.01em}
.card{fill:var(--card);stroke:var(--line)}
.ph{fill:var(--soft);stroke:var(--link);stroke-width:1.5;stroke-dasharray:5 4}
.mono{font-weight:600;fill:var(--muted)}
.track{fill:none;stroke:var(--line);stroke-width:1.25;opacity:.8}
.gl{fill:none;stroke:var(--acct);stroke-width:1.7;stroke-linecap:round;stroke-linejoin:round}
.accfill{fill:var(--acc);fill-opacity:.13}.accline{fill:var(--acc)}
.ptitle{font-size:29px;font-weight:600;letter-spacing:-.01em}
.pbody{font-size:21px;fill:var(--muted)}
.stepn{font-size:16px;font-weight:600;letter-spacing:.1em;fill:var(--muted)}
.stept{font-size:26px;font-weight:600}
.steps{font-size:21px;fill:var(--muted)}
.conn{fill:none;stroke:var(--link);stroke-width:1.5;stroke-linecap:round;stroke-dasharray:1 7}
.chev{fill:none;stroke:var(--link);stroke-width:1.8;stroke-linecap:round;stroke-linejoin:round}
.wtit{font-size:17px;font-weight:600;letter-spacing:.14em;fill:var(--muted)}
.wname{font-size:22px;font-weight:600}.wnote{font-size:20px;fill:var(--muted)}
.wbar{fill:var(--bar)}.wedge{fill:none;stroke:var(--line)}
${MOCK_CSS}
</style>`;
}
function wrap(s, n) {
  const out = []; let line = '';
  for (const w of s.split(' ')) { if ((line + ' ' + w).trim().length > n) { out.push(line); line = w; } else line = (line + ' ' + w).trim(); }
  if (line) out.push(line); return out;
}
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;');
function pill(x, y, text, status) { // status chip with a dot, left-aligned at x
  const w = Math.round(text.length * 9.8 + 40);
  return `<rect x="${x}" y="${y - 19}" width="${w}" height="38" rx="19" class="accfill"/><circle cx="${x + 18}" cy="${y}" r="4" class="accline"/><text class="pilltxt" x="${x + 32}" y="${y + 6}">${esc(text)}</text>`;
}
function glow(w, h, cx, cy, r, id = 'g1') {
  return `<defs><radialGradient id="${id}" cx="${cx}" cy="${cy}" r="${r}" gradientUnits="userSpaceOnUse"><stop offset="0" style="stop-color:var(--acc);stop-opacity:var(--glow)"/><stop offset="1" style="stop-color:var(--acc);stop-opacity:0"/></radialGradient></defs>` +
    `<rect width="${w}" height="${h}" fill="url(#${id})"/>`;
}
const tracks = (w, h) => `<path class="track" d="M-40 ${h * 0.86} C ${w * 0.2} ${h * 0.66}, ${w * 0.4} ${h * 0.96}, ${w * 0.62} ${h * 0.74} S ${w * 0.92} ${h * 0.5}, ${w + 40} ${h * 0.62}"/><path class="track" d="M-40 ${h * 0.9} C ${w * 0.2} ${h * 0.7}, ${w * 0.4} ${h} , ${w * 0.62} ${h * 0.78} S ${w * 0.92} ${h * 0.54}, ${w + 40} ${h * 0.66}"/>`;

let uid = 0;
/** A macOS-style window at (x,y), width w, with body content (drawn at body size w x bh) and a soft shadow. */
function windowFrame(x, y, w, bh, body, { opacity = 1, shadow = true } = {}) {
  const id = `w${++uid}`, tb = Math.round(w * 0.036), H = bh + tb;
  return `<g opacity="${opacity}" transform="translate(${x} ${y})"><defs><clipPath id="${id}"><rect width="${w}" height="${H}" rx="12"/></clipPath>` +
    `<filter id="${id}s" x="-20%" y="-20%" width="140%" height="150%"><feDropShadow dx="0" dy="28" stdDeviation="30" flood-color="#000" flood-opacity="var(--shadow)"/></filter></defs>` +
    `<rect width="${w}" height="${H}" rx="12" fill="var(--wbg)"${shadow ? ` filter="url(#${id}s)"` : ''}/>` +
    `<g clip-path="url(#${id})"><rect class="wbar" width="${w}" height="${tb}"/><rect class="wedge" y="${tb - 0.5}" width="${w}" height="1" style="fill:var(--line);stroke:none"/>` +
    [['#FF5F57', 0], ['#FEBC2E', 1], ['#28C840', 2]].map(([c, i]) => `<circle cx="${tb * 0.5 + i * tb * 0.42}" cy="${tb / 2}" r="${tb * 0.14}" fill="${c}"/>`).join('') +
    `<g transform="translate(0 ${tb})">${body}</g></g><rect width="${w}" height="${H}" rx="12" fill="none" stroke="var(--line)"/></g>`;
}
/** Body for a screenshot: both themes as <use> of defs images. */
const shotBody = (idl, idd, w, h) => `<g class="lt"><use href="#${idl}" width="${w}" height="${h}"/></g><g class="dk"><use href="#${idd}" width="${w}" height="${h}"/></g>`;
const imgDef = (id, uri, w, h) => `<image id="${id}" href="${uri}" width="${w}" height="${h}" preserveAspectRatio="none"/>`;
const mockBody = (g, w, designW, designH) => `<g transform="scale(${w / designW})">${g}</g>`;

/** Drawn instead of screenshots: a planned app (concept) or `illustrated: true` (the screenshots show an older app). */
const drawn = (d) => d.planned || d.illustrated;

// ---- screenshots -------------------------------------------------------------------------------------------------------
// layout 'dir': screenshots/<theme>/<name>.png; 'light-suffix': <name>.png is dark and <name>-light.png light (Debrief); otherwise <name>-<theme>.png.
const shotFile = (d, app, name, theme) => d.shots.layout === 'dir' ? join(appsRoot, app, 'docs/screenshots', theme, `${name}.png`)
  : d.shots.layout === 'light-suffix' ? join(appsRoot, app, 'docs/screenshots', theme === 'light' ? `${name}-light.png` : `${name}.png`)
  : join(appsRoot, app, 'docs/screenshots', `${name}-${theme}.png`);
/** Collect crop tasks for an app and return {key: {light,dark}} data URIs. */
function shotsFor(d) {
  if (drawn(d)) return {};
  const jobs = [];
  const add = (key, name, rect, outW, q) => ['light', 'dark'].forEach((th) => jobs.push({ key, th, src: shotFile(d, d.id, name, th), x: rect[0], y: rect[1], w: rect[2], h: rect[3], outW, q }));
  // heroH: the hero screenshot's height in CSS px when it is shorter than the 900 px window (a frame that ends between rows).
  add('hero', d.shots.hero, [0, 0, 2880, (d.shots.heroH ?? 900) * 2], 1500, 0.8);
  add('back', d.shots.back, [0, 0, 2880, 1800], 1100, 0.72);
  (d.tour?.panels ?? []).forEach((p, i) => add(`t${i}`, p.shot, [p.crop[0] * 2, p.crop[1] * 2, p.crop[2] * 2, Math.round((p.crop[2] * 2) / 2.4)], 1100, 0.78));
  d.features.panels.forEach((p, i) => add(`p${i}`, p.shot, [p.crop[0] * 2, p.crop[1] * 2, p.crop[2] * 2, Math.round((p.crop[2] * 2) / 2.4)], 1100, 0.78));
  for (const j of jobs) if (!existsSync(j.src)) throw new Error(`missing screenshot ${j.src}`);
  const res = crops(jobs);
  const out = {};
  jobs.forEach((j, i) => { (out[j.key] ??= {})[j.th] = res[i]; });
  return out;
}

// ---- hero --------------------------------------------------------------------------------------------------------------
function heroSvg(d, S, { W = 900 * 16 / 9, H = 900 } = {}) {
  const a = d.accent;
  // main window geometry: body aspect 1.6
  const fw = 920, fbh = 575, fx = 650, fy = 214, bw = 860, bbh = 537, bx = 740, by = 112;
  const defs = drawn(d) ? '' : imgDef('hero-l', S.hero.light, fw, fbh) + imgDef('hero-d', S.hero.dark, fw, fbh) + imgDef('back-l', S.back.light, bw, bbh) + imgDef('back-d', S.back.dark, bw, bbh);
  const front = windowFrame(fx, fy, fw, fbh, drawn(d) ? mockBody(mock.hero(d.id), fw, 960, 600) + `<rect width="${fw}" height="${fbh}" fill="none"/>` : shotBody('hero-l', 'hero-d', fw, fbh));
  const back = drawn(d) ? '' : windowFrame(bx, by, bw, bbh, shotBody('back-l', 'back-d', bw, bbh), { opacity: 0.55, shadow: true });
  const lines = d.headline.map((l, i) => `<text class="h1" x="96" y="${372 + i * 66}">${esc(l)}</text>`).join('');
  const sub = d.sub.map((l, i) => `<text class="sub" x="96" y="${372 + (d.headline.length - 1) * 66 + 58 + i * 34}">${esc(l)}</text>`).join('');
  const subEnd = 372 + (d.headline.length - 1) * 66 + 26 + d.sub.length * 34;
  const tag = d.planned ? pill(96, subEnd + 46, 'Planned · coming soon') : pill(96, subEnd + 46, STATUS[d.id]?.text ?? d.status);
  const note = d.planned ? 'Concept: the screen is a mock-up, not a product screenshot.' : d.illustrated ? 'Illustration of the app, not a product screenshot.' : '';
  const mockNote = note ? `<text class="small" x="${fx}" y="${fy + fbh + 54}" style="font-size:17px">${note}</text>` : '';
  return `<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="900" viewBox="0 0 1600 900" role="img" aria-labelledby="t d">
<title id="t">${d.name}: ${esc(d.headline.join(' '))}</title><desc id="d">${esc(d.sub.join(' '))}</desc>
${css(a)}<defs>${defs}</defs>
${glow(1600, 900, 1180, 430, 760)}${glow(1600, 900, 140, 840, 520, 'g2')}${tracks(1600, 900)}
${back}${front}${mockNote}
${iconSvg(d.icon ?? d.id, 96, 196, 72)}
<text class="name" x="186 " y="242">${d.name}</text>
${lines}${sub}${tag}
<text class="small" x="96" y="${subEnd + 112}">${esc(d.platforms)}. Local only.</text>
</svg>`;
}

// ---- features ----------------------------------------------------------------------------------------------------------
function featuresSvg(d, S) {
  const a = d.accent, W = 1600, PW = 704, PH = 440, GAP = 32, X0 = 80, Y0 = 244;
  const defs = [];
  const panels = d.features.panels.map((p, i) => {
    const x = X0 + (i % 2) * (PW + GAP), y = Y0 + Math.floor(i / 2) * (PH + GAP), iw = PW - 32, ih = Math.round(iw / 2.4);
    let art;
    if (p.mock) art = `<g transform="scale(${iw / 672})">${mock.panel(d.id, p.mock)}</g>`;
    else { defs.push(imgDef(`p${i}l`, S[`p${i}`].light, iw, ih), imgDef(`p${i}d`, S[`p${i}`].dark, iw, ih)); art = shotBody(`p${i}l`, `p${i}d`, iw, ih); }
    const cid = `pc${i}`;
    const lines = wrap(p.text, 58);
    return `<g transform="translate(${x} ${y})"><rect class="card" width="${PW}" height="${PH}" rx="22"/>` +
      `<clipPath id="${cid}"><rect x="16" y="16" width="${iw}" height="${ih}" rx="12"/></clipPath><g clip-path="url(#${cid})"><g transform="translate(16 16)">${art}</g></g>` +
      `<rect x="16" y="16" width="${iw}" height="${ih}" rx="12" fill="none" stroke="var(--line)"/>` +
      `<text class="ptitle" x="24" y="${ih + 16 + 48}">${esc(p.title)}</text>` +
      lines.map((l, k) => `<text class="pbody" x="24" y="${ih + 16 + 86 + k * 30}">${esc(l)}</text>`).join('') + `</g>`;
  }).join('\n');
  const sy = Y0 + 2 * PH + GAP + 68, ww = d.worksWith, cw = (W - 160) / Math.max(ww.length, 4);
  const works = `<text class="wtit" x="80" y="${sy}">WORKS WITH</text>` + ww.map((w, i) => {
    const x = 80 + i * cw;
    return `<g transform="translate(${x} ${sy + 24})">${iconSvg(w.app, 0, 0, 52)}<text class="wname" x="66" y="22">${names[w.app]}</text><text class="wnote" x="66" y="46">${esc(w.note)}</text></g>`;
  }).join('');
  // No "Works with" strip for a member that names no partner apps yet (a planned tool).
  const H = ww.length ? sy + 24 + 52 + 48 : sy - 28;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-labelledby="t d">
<title id="t">${d.name} features</title><desc id="d">${esc(d.features.panels.map((p) => `${p.title}: ${p.text}`).join(' '))}</desc>
${css(a)}<defs>${defs.join('')}</defs>${glow(W, 900, 800, 0, 900)}
${iconSvg(d.icon ?? d.id, 80, 64, 56)}<text class="name" x="150" y="102">${d.name}</text>
${d.planned ? pill(150 + d.name.length * 15.5 + 16, 94, 'Planned', true) : ''}
<text class="h2" x="80" y="196">${esc(d.features.heading)}</text>
${panels}
${ww.length ? `<line x1="80" x2="${W - 80}" y1="${sy - 28}" y2="${sy - 28}" stroke="var(--line)"/>\n${works}` : ''}
</svg>`;
}

// ---- tour (optional: d.tour = { heading, panels: [{ shot, crop, title, text }] }) -------------------------------------------
// A 2x2 strip of numbered steps, each a cropped screenshot with a title and one line; the README's numbered steps sit below it.
function tourSvg(d, S) {
  const a = d.accent, W = 1600, PW = 704, PH = 430, GAP = 32, X0 = 80, Y0 = 172;
  const defs = [];
  const panels = d.tour.panels.map((p, i) => {
    const x = X0 + (i % 2) * (PW + GAP), y = Y0 + Math.floor(i / 2) * (PH + GAP), iw = PW - 32, ih = Math.round(iw / 2.4);
    defs.push(imgDef(`t${i}l`, S[`t${i}`].light, iw, ih), imgDef(`t${i}d`, S[`t${i}`].dark, iw, ih));
    const cid = `tc${i}`, lines = wrap(p.text, 60);
    return `<g transform="translate(${x} ${y})"><rect class="card" width="${PW}" height="${PH}" rx="22"/>` +
      `<clipPath id="${cid}"><rect x="16" y="16" width="${iw}" height="${ih}" rx="12"/></clipPath><g clip-path="url(#${cid})"><g transform="translate(16 16)">${shotBody(`t${i}l`, `t${i}d`, iw, ih)}</g></g>` +
      `<rect x="16" y="16" width="${iw}" height="${ih}" rx="12" fill="none" stroke="var(--line)"/>` +
      `<circle class="accfill" cx="42" cy="${ih + 16 + 38}" r="18"/><text class="ptitle" x="42" y="${ih + 16 + 46}" text-anchor="middle" style="font-size:20px;fill:var(--acct)">${i + 1}</text>` +
      `<text class="ptitle" x="72" y="${ih + 16 + 46}">${esc(p.title)}</text>` +
      lines.map((l, k) => `<text class="pbody" x="24" y="${ih + 16 + 88 + k * 28}">${esc(l)}</text>`).join('') + `</g>`;
  }).join('\n');
  const H = Y0 + 2 * PH + GAP + 48;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-labelledby="t d">
<title id="t">${d.name}: a two-minute tour</title><desc id="d">${esc(d.tour.panels.map((p, i) => `${i + 1}. ${p.title}: ${p.text}`).join(' '))}</desc>
${css(a)}<defs>${defs.join('')}</defs>${glow(W, 900, 800, 0, 900)}
${iconSvg(d.icon ?? d.id, 80, 52, 56)}<text class="name" x="150" y="90">${d.name}</text>
<text class="h2" x="80" y="148" style="font-size:36px">${esc(d.tour.heading)}</text>
${panels}
</svg>`;
}

// ---- how it works ------------------------------------------------------------------------------------------------------
function howSvg(d) {
  const a = d.accent, W = 1600, H = 500, st = d.how.steps, n = st.length, cw = (W - 160) / n;
  const items = st.map((s, i) => {
    const cx = 80 + cw * (i + 0.5), cy = 188;
    const conn = i < n - 1 ? `<path class="conn" d="M${cx + 58} ${cy} L${cx + cw - 66} ${cy}"/><path class="chev" d="M${cx + cw - 72} ${cy - 6} l7 6 -7 6"/>` : '';
    return `<circle class="accfill" cx="${cx}" cy="${cy}" r="44"/>${glyph(s.glyph, cx, cy, 36)}${conn}` +
      `<text class="stepn" x="${cx}" y="${cy + 84}" text-anchor="middle">STEP ${i + 1}</text><text class="stept" x="${cx}" y="${cy + 114}" text-anchor="middle">${esc(s.title)}</text>` +
      wrap(s.text, 25).map((l, k) => `<text class="steps" x="${cx}" y="${cy + 142 + k * 28}" text-anchor="middle">${esc(l)}</text>`).join('');
  }).join('\n');
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-labelledby="t d">
<title id="t">How ${d.name} works</title><desc id="d">${esc(st.map((s) => s.title).join(', then '))}. ${esc(d.how.family)}</desc>
${css(a)}${iconSvg(d.icon ?? d.id, 80, 40, 40)}<text class="name" x="132" y="68" style="font-size:28px">How ${d.name} works</text>
${items}
<line x1="80" x2="${W - 80}" y1="${H - 66}" y2="${H - 66}" stroke="var(--line)"/>
<text class="small" x="80" y="${H - 28}">${esc(d.how.family)}</text>
</svg>`;
}

// ---- social (1280x640) ---------------------------------------------------------------------------------------------------
function socialSvg(d, S) {
  // d.social = { fx, fy } moves the bleeding window so the right and bottom edges cut between columns and rows, not through text.
  const a = d.accent, fw = 880, fbh = 550, fx = d.social?.fx ?? 600, fy = d.social?.fy ?? 118;
  const defs = drawn(d) ? '' : imgDef('hero-l', S.hero.light, fw, fbh) + imgDef('hero-d', S.hero.dark, fw, fbh);
  const win = windowFrame(fx, fy, fw, fbh, drawn(d) ? mockBody(mock.hero(d.id), fw, 960, 600) : shotBody('hero-l', 'hero-d', fw, fbh));
  return `<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="640" viewBox="0 0 1280 640" role="img" aria-labelledby="t">
<title id="t">${d.name}: ${esc(d.headline.join(' '))}</title>${css(a)}<defs>${defs}</defs>
<rect width="1280" height="640" fill="var(--wbg)"/>${glow(1280, 640, 960, 340, 640)}${tracks(1280, 640)}
${win}
${iconSvg(d.icon ?? d.id, 72, 84, 76)}<text class="name" x="164" y="136" style="font-size:34px">${d.name}</text>
${d.headline.map((l, i) => `<text class="h1" x="72" y="${268 + i * 62}" style="font-size:50px">${esc(l)}</text>`).join('')}
${d.sub.map((l, i) => `<text class="sub" x="72" y="${268 + d.headline.length * 62 + 12 + i * 30}" style="font-size:20px">${esc(l)}</text>`).join('')}
${pill(72, 568, d.planned ? 'Planned · coming soon' : STATUS[d.id]?.text ?? d.status)}
</svg>`;
}

// ---- render ------------------------------------------------------------------------------------------------------------
const png = (svgPath, outPath, w, h, theme, scale) => screenshotSvg(svgPath, outPath, w, h, theme, scale);
const dims = (svg) => { const m = svg.match(/<svg[^>]* width="(\d+)" height="(\d+)"/); return [+m[1], +m[2]]; };

async function build(id, wantPng) {
  const d = await load(id);
  const outDir = join(appHome(d, id), 'docs/images');
  mkdirSync(outDir, { recursive: true });
  const S = shotsFor(d), stem = d.stem ?? id;
  const files = { hero: heroSvg(d, S), features: featuresSvg(d, S), 'how-it-works': howSvg(d), social: socialSvg(d, S), ...(d.tour ? { tour: tourSvg(d, S) } : {}) };
  for (const [k, svg] of Object.entries(files)) {
    const p = join(outDir, `${stem}-${k}.svg`);
    writeFileSync(p, svg);
    console.log(`wrote ${p} (${Math.round(svg.length / 1024)} KB)`);
    if (!wantPng) continue;
    const [w, h] = dims(svg);
    if (k === 'social') { png(p, join(outDir, `${stem}-social.png`), w, h, 'dark', 1); png(p, join(outDir, `${stem}-social-light.png`), w, h, 'light', 1); }
    else for (const th of ['light', 'dark']) png(p, join(outDir, `${stem}-${k}-${th}.png`), w, h, th, 2);
  }
  if (versionFile(join(appHome(d, id), 'README.md'))) console.log(`versioned image links in ${id}/README.md`);
}

const want = process.argv.slice(2).filter((a) => !a.startsWith('--'));
const png2 = process.argv.includes('--png');
const targets = want[0] === 'all' || !want.length ? (await Promise.all(ids.map(async (i) => [i, await load(i)]))).filter(([, d]) => !d.draft).map(([i]) => i) : want;
for (const id of targets) await build(id, png2);
