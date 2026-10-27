// Writes preview.html: a self-contained review sheet with inline SVGs (each icon's ids carry its app, size and state).
import { writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { APPS, UMBRELLA, NAMES, PALETTE, INK } from './design.mjs';
import { TRAY_STATES as STATUSES, master, small as isoSmall, tray as isoTray } from './iso-family.mjs';
import { check, contrast } from './cvd.mjs';

const ROOT = resolve(import.meta.dirname, '..');
const sized = (svg, px) => svg.replace(/<svg ([^>]*?)width="\d+" height="\d+"/, `<svg $1width="${px}" height="${px}"`);
const mac = (a, px) => sized(master(a), px);
const win = (a, px) => sized(master(a, { platform: 'windows' }), px);
const small = (a, s, px = s) => sized(isoSmall(a, s), px);
const tray = (a, st, theme, px = 16) => sized(isoTray(a, st, theme), px);

const CONCEPT = {
  debrief: "An engineer's laptop with a telemetry trace on the screen: the debrief after the race.",
  pitcrew: 'An impact wrench with its socket and wheel nut: the crew that keeps runners turning.',
  spotter: "A spotter's binoculars with an amber lens glint: it watches everywhere and calls the moment a release appears.",
  pitwall: 'A team radio with its status LED: the private, local model every app radios to.',
  dyno: 'A tachometer pod with the redline and the shift lights: health, load and tuning at a glance.',
  sighter: 'A camera on a tripod: it finds any UI element by sight.',
  pacenotes: 'A tablet with the route and a stylus: notes on the shape of a codebase.',
};

const cvd = check(Object.fromEntries(APPS.map(a => [a, PALETTE[a].base])));

const sizeRow = (fn, sizes) =>
  APPS.map(a => `<div class="cell"><div class="sizes">${sizes.map(s => `<figure>${fn(a, s)}<figcaption>${s}</figcaption></figure>`).join('')}</div><div class="label">${NAMES[a]}</div></div>`).join('');

const dock = theme => `
  <div class="desk mac ${theme}">
    <div class="menubar">
      <span class="apple"></span><b>Debrief</b><span>File</span><span>Edit</span><span>View</span><span>Window</span>
      <span class="spacer"></span>
      ${APPS.map(a => `<span class="tpl" title="${NAMES[a]}">${tray(a, STATUSES[a][0], 'template', 18)}</span>`).join('')}
      <span class="tpl" title="Pitcrew warning">${tray('pitcrew', 'warning', 'template', 18)}</span>
      <span class="tpl" title="Spotter new release">${tray('spotter', 'new', 'template', 18)}</span>
      <span class="clock">Thu 22 Oct 09:41</span>
    </div>
    <div class="dock">${APPS.map(a => `<div class="dockitem">${mac(a, 64)}<i></i></div>`).join('')}</div>
  </div>`;

const taskbar = theme => `
  <div class="desk win ${theme}">
    <div class="taskbar">
      <div class="tb-apps"><span class="start" title="Start (neutral placeholder)"><i></i></span>${APPS.map((a, i) => `<span class="tbitem${i === 0 ? ' active' : ''}">${small(a, 24)}</span>`).join('')}</div>
      <div class="tb-tray">
        ${tray('pitcrew', 'busy', theme)}${tray('spotter', 'new', theme)}${tray('pitwall', 'serving', theme)}${tray('dyno', 'normal', theme)}${tray('debrief', 'default', theme)}
        <span class="clock">09:41<br>22/10/2026</span>
      </div>
    </div>
  </div>`;

const trayGallery = () =>
  APPS.map(
    a => `<div class="trayrow"><div class="label">${NAMES[a]}</div>${STATUSES[a]
      .map(
        st => `<figure class="trayfig">
          <div class="chips">
            <span class="chip mac-l">${tray(a, st, 'template', 18)}</span>
            <span class="chip mac-d">${tray(a, st, 'template', 18)}</span>
            <span class="chip win-l">${tray(a, st, 'light')}</span>
            <span class="chip win-d">${tray(a, st, 'dark')}</span>
          </div>
          <figcaption>${st === 'default' ? 'tray' : st}</figcaption></figure>`,
      )
      .join('')}</div>`,
  ).join('');

const swatches = APPS.map(a => {
  const p = PALETTE[a];
  return `<div class="swatch"><div class="chipbig" style="background:linear-gradient(${p.top},${p.base} 55%,${p.bottom})"><span style="color:${INK[a]}">${NAMES[a]}</span></div>
    <code>${p.base}</code><small>top ${p.top} · bottom ${p.bottom}<br>ink ${INK[a]} · ${contrast(p.base, INK[a]).toFixed(1)}:1</small></div>`;
}).join('');

const cvdRow = (name, filter) =>
  `<div class="cvdrow"><span class="label">${name}</span><div style="filter:${filter}">${APPS.map(a => mac(a, 72)).join('')}</div></div>`;

const html = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Suite Icons Preview</title>
<style>
  :root { --bg:#F4F4F6; --fg:#16171A; --muted:#6B6E76; --card:#FFFFFF; --line:#E3E4E8; --dark:#141518; --dark-card:#1D1F23; }
  * { box-sizing:border-box; }
  body { margin:0; background:var(--bg); color:var(--fg); font:14px/1.5 -apple-system,"Segoe UI Variable","Segoe UI",Inter,system-ui,sans-serif; }
  header { padding:48px 48px 24px; }
  h1 { font-size:34px; letter-spacing:-0.02em; margin:0 0 6px; }
  h2 { font-size:20px; margin:0 0 4px; letter-spacing:-0.01em; }
  p.lead { color:var(--muted); max-width:820px; margin:0; }
  section { padding:28px 48px; }
  .sub { color:var(--muted); margin:0 0 18px; max-width:860px; }
  .split { display:grid; grid-template-columns:1fr 1fr; gap:16px; }
  .stack { display:grid; grid-template-columns:1fr; gap:16px; }
  .panel { border-radius:18px; padding:28px; background:var(--card); border:1px solid var(--line); overflow:hidden; }
  .panel.dark { background:var(--dark); border-color:#24262B; color:#E9EAED; }
  .hero { display:flex; gap:28px; overflow-x:auto; padding-bottom:12px; }
  .hero figure { margin:0; flex:none; text-align:center; }
  .hero figcaption, .label { font-weight:600; font-size:13px; }
  .grid5 { display:grid; grid-template-columns:repeat(5,1fr); gap:20px; }
  .cell { text-align:center; }
  .sizes { display:flex; align-items:flex-end; justify-content:center; gap:14px; min-height:70px; }
  .sizes figure { margin:0; display:flex; flex-direction:column; align-items:center; gap:4px; }
  .sizes figcaption { font-size:10px; color:var(--muted); }
  .panel.dark .sizes figcaption { color:#8B8E96; }
  .concept { font-size:12.5px; color:var(--muted); margin-top:8px; }
  .panel.dark .concept { color:#9A9DA5; }
  /* macOS mock */
  .desk { position:relative; height:300px; border-radius:16px; overflow:hidden; }
  .mac.light { background:radial-gradient(120% 90% at 20% 10%,#BFD6FF 0,#E9D8F5 45%,#F6E3D3 100%); }
  .mac.dark { background:radial-gradient(120% 90% at 20% 10%,#1F2C4D 0,#2A1C3D 50%,#120F18 100%); }
  .menubar { display:flex; align-items:center; gap:16px; height:30px; padding:0 14px; font-size:13px; backdrop-filter:blur(20px); }
  .mac.light .menubar { background:rgba(255,255,255,.55); color:#111; }
  .mac.dark .menubar { background:rgba(30,30,34,.55); color:#F2F2F2; }
  .mac.dark .tpl svg { filter:invert(1); }
  .menubar .spacer { flex:1; }
  .menubar .tpl { display:flex; opacity:.9; }
  .apple { width:12px; height:14px; border-radius:6px 6px 5px 5px; background:currentColor; opacity:.85; }
  .dock { position:absolute; left:50%; bottom:14px; transform:translateX(-50%); display:flex; gap:10px; padding:9px 12px 13px; border-radius:24px; backdrop-filter:blur(24px); }
  .mac.light .dock { background:rgba(255,255,255,.38); border:1px solid rgba(255,255,255,.55); }
  .mac.dark .dock { background:rgba(40,40,46,.45); border:1px solid rgba(255,255,255,.12); }
  .dockitem { position:relative; }
  .dockitem svg { display:block; }
  .dockitem i { position:absolute; left:50%; bottom:-8px; width:4px; height:4px; border-radius:50%; margin-left:-2px; background:currentColor; opacity:.6; }
  .mac.light .dockitem i { color:#222; } .mac.dark .dockitem i { color:#fff; }
  /* Windows mock */
  .win.light { background:linear-gradient(160deg,#C9DBF4,#E8EEF8 60%,#D6E3F6); }
  .win.dark { background:linear-gradient(160deg,#0B1B33,#172238 60%,#0E1420); }
  .taskbar { position:absolute; left:0; right:0; bottom:0; height:48px; display:flex; align-items:center; justify-content:center; backdrop-filter:blur(30px); }
  .win.light .taskbar { background:rgba(243,243,243,.85); border-top:1px solid rgba(0,0,0,.06); color:#1B1B1B; }
  .win.dark .taskbar { background:rgba(32,32,32,.85); border-top:1px solid rgba(255,255,255,.06); color:#fff; }
  .tb-apps { display:flex; gap:4px; }
  .tbitem, .start { width:40px; height:40px; border-radius:4px; display:grid; place-items:center; position:relative; }
  .tbitem.active { background:rgba(127,127,127,.16); }
  .tbitem.active::after { content:""; position:absolute; bottom:2px; width:16px; height:3px; border-radius:2px; background:#0067C0; }
  .win.dark .tbitem.active::after { background:#4CC2FF; }
  .start i { width:18px; height:18px; border-radius:50%; border:2px solid currentColor; opacity:.55; }
  .tb-tray { position:absolute; right:10px; display:flex; align-items:center; gap:10px; font-size:11.5px; }
  .tb-tray .clock { text-align:right; line-height:1.3; margin-left:6px; }
  /* tray gallery */
  .trayrow { display:flex; align-items:center; gap:18px; padding:10px 0; border-bottom:1px solid var(--line); }
  .trayrow .label { width:80px; }
  .trayfig { margin:0; text-align:center; }
  .chips { display:flex; gap:4px; }
  .chip { width:34px; height:34px; border-radius:8px; display:grid; place-items:center; }
  .chip.mac-l { background:#ECECEC; } .chip.mac-d { background:#2A2A2E; } .chip.mac-d svg { filter:invert(1); }
  .chip.win-l { background:#F3F3F3; border:1px solid #E2E2E2; } .chip.win-d { background:#202020; }
  .trayfig figcaption { font-size:11px; color:var(--muted); margin-top:4px; }
  .legend { font-size:12px; color:var(--muted); margin-top:10px; }
  .swatches { display:grid; grid-template-columns:repeat(5,1fr); gap:16px; }
  .swatch code { display:block; margin-top:8px; font-weight:600; }
  .swatch small { color:var(--muted); line-height:1.4; display:block; }
  .chipbig { height:84px; border-radius:16px; display:flex; align-items:flex-end; padding:10px 12px; font-weight:700; }
  .cvdrow { display:flex; align-items:center; gap:16px; margin:6px 0; }
  .cvdrow .label { width:120px; } .cvdrow div { display:flex; gap:12px; }
  .alt .note { color:#9A9DA5; font-size:13px; max-width:900px; margin-top:16px; }
  @media (max-width:900px) { header, section { padding-left:16px; padding-right:16px; } .split, .grid5, .swatches { grid-template-columns:1fr; } }
</style></head>
<body>
<svg width="0" height="0" style="position:absolute" aria-hidden="true">
  <filter id="cvd-protan"><feColorMatrix type="matrix" values="0.152286 1.052583 -0.204868 0 0  0.114503 0.786281 0.099216 0 0  -0.003882 -0.048116 1.051998 0 0  0 0 0 1 0"/></filter>
  <filter id="cvd-deutan"><feColorMatrix type="matrix" values="0.367322 0.860646 -0.227968 0 0  0.280085 0.672501 0.047413 0 0  -0.011820 0.042940 0.968881 0 0  0 0 0 1 0"/></filter>
  <filter id="cvd-tritan"><feColorMatrix type="matrix" values="1.255528 -0.076749 -0.178779 0 0  -0.078411 0.930809 0.147602 0 0  0.004733 0.691367 0.303900 0 0  0 0 0 1 0"/></filter>
</svg>
<header>
  <h1>App icon family</h1>
  <p class="lead">The seven apps and Paddock as isometric motorsport objects (STYLE.md): one saturated hue per app on the shared enamel tile, one rendered object per app in true 30 degree isometric under one key light from the top left, one accent from the app's own hue. Small sizes are hand-drawn: 2:1 pixel isometric at 32 and 24, a flat symbol at 16 and in the tray. The flat set this replaced is in <code>archive/2026-10-flat/</code>.</p>
</header>

<section>
  <h2>1024 px masters (macOS grid)</h2>
  <p class="sub">An 824 px continuous-corner body centred on the 1024 canvas, with the standard drop shadow. Scroll sideways to see all seven at full size.</p>
  <div class="hero">${APPS.map(a => `<figure>${mac(a, 1024)}<figcaption>${NAMES[a]}</figcaption></figure>`).join('')}</div>
</section>

<section>
  <h2>The family at 256 px</h2>
  <div class="stack">
    <div class="panel"><div class="grid5">${APPS.map(a => `<div class="cell">${mac(a, 200)}<div class="label">${NAMES[a]}</div><div class="concept">${CONCEPT[a]}</div></div>`).join('')}</div></div>
    <div class="panel dark"><div class="grid5">${APPS.map(a => `<div class="cell">${mac(a, 200)}<div class="label">${NAMES[a]}</div></div>`).join('')}</div></div>
  </div>
  <div class="stack" style="margin-top:16px">
    <div class="panel"><div class="grid5">${APPS.map(a => `<div class="cell">${mac(a, 256)}</div>`).join('')}</div></div>
    <div class="panel dark"><div class="grid5">${APPS.map(a => `<div class="cell">${win(a, 256)}</div>`).join('')}</div></div>
  </div>
  <p class="sub" style="margin-top:10px">Top: macOS masters at 200 on light and dark. Bottom: macOS masters at 256 on light, Windows masters at 256 on dark (nearly full-bleed rounded square with circular corners and no outer shadow).</p>
</section>

<section>
  <h2>Small sizes</h2>
  <p class="sub">macOS masters scaled down to 64, 32 and 16 px, next to the hand-tuned frames (48: the simple frame on the small tile; 32 and 24: pixel isometric; 16: the flat symbol), which snap to the pixel grid and drop detail. These frames go into icon.ico.</p>
  <div class="stack">
    <div class="panel"><div class="grid5">${sizeRow((a, s) => mac(a, s), [64, 32, 16])}</div><div class="grid5" style="margin-top:22px">${sizeRow((a, s) => small(a, s), [48, 32, 24, 16])}</div></div>
    <div class="panel dark"><div class="grid5">${sizeRow((a, s) => mac(a, s), [64, 32, 16])}</div><div class="grid5" style="margin-top:22px">${sizeRow((a, s) => small(a, s), [48, 32, 24, 16])}</div></div>
  </div>
</section>

<section>
  <h2>In context: macOS</h2>
  <p class="sub">The Dock at 64 pt and the menu-bar template images at 18 pt. macOS tints template images, which the mock imitates by inverting them on the dark menu bar.</p>
  <div class="split">${dock('light')}${dock('dark')}</div>
</section>

<section>
  <h2>In context: Windows 11</h2>
  <p class="sub">The taskbar at 100% scale uses the hand-tuned 24 px frame. The notification area shows Pitcrew (busy), Spotter (new release), Pitwall (serving), Dyno and Debrief.</p>
  <div class="split">${taskbar('light')}${taskbar('dark')}</div>
</section>

<section>
  <h2>Tray and menu-bar icons</h2>
  <p class="sub">Each status has four tiles: the macOS template on a light and a dark menu bar, then the Windows icon on a light and a dark taskbar. Badges are whole pixels: Debrief, Pitwall, Sighter and Pitcrew cut a gap into the glyph in the bottom-right corner, Spotter and Dyno set a 3 x 3 square in a corner the glyph leaves free; Pitwall idle dims the whole glyph. On Windows they are coloured, and each status also has its own shape (dot, ring, triangle, cross), so colour is never the only cue.</p>
  <div class="panel">${trayGallery()}</div>
</section>

<section>
  <h2>Palette and colour-vision check</h2>
  <p class="sub">One hue per app, chosen by searching OKLCH space for the set with the largest minimum OKLab distance under simulated protanopia, deuteranopia and tritanopia (Machado 2009, full severity). Minimum ΔE (OKLab ×100): normal ${cvd.normal.minDeltaE}, protan ${cvd.protan.minDeltaE}, deutan ${cvd.deutan.minDeltaE}, tritan ${cvd.tritan.minDeltaE}. The glyphs also differ in silhouette, so hue is never the only cue.</p>
  <div class="swatches">${swatches}</div>
  <div class="panel" style="margin-top:16px">
    ${cvdRow('Normal', 'none')}${cvdRow('Protanopia', 'url(#cvd-protan)')}${cvdRow('Deuteranopia', 'url(#cvd-deutan)')}${cvdRow('Tritanopia', 'url(#cvd-tritan)')}
  </div>
</section>

<section>
  <h2>The Paddock mark (umbrella)</h2>
  <p class="sub">Not an app: the family and its shared foundation. A pit building (four lit bays, a canopy with a pinstripe in the seven app hues, the hospitality floor, the chequered pit wall) on the asphalt-graphite tile with no hue of its own. Shown beside Debrief for scale, then the hand-tuned frames (the 16 px canopy is the seven-hue band) and the tray template (the slatted garage door).</p>
  <div class="split">
    ${['panel', 'panel dark'].map(c => `<div class="${c}"><div class="sizes">${UMBRELLA.map(a => `<figure>${mac(a, 200)}<figcaption>${NAMES[a]}</figcaption></figure>`).join('')}<figure>${mac('debrief', 200)}<figcaption>Debrief</figcaption></figure></div>
      <div class="sizes" style="margin-top:18px">${UMBRELLA.map(a => [64, 32, 16].map(s => `<figure>${mac(a, s)}<figcaption>${s}</figcaption></figure>`).join('') + [48, 32, 24, 16].map(s => `<figure>${small(a, s)}<figcaption>${s}</figcaption></figure>`).join('')).join('')}</div></div>`).join('')}
  </div>
  <div class="panel" style="margin-top:16px">${UMBRELLA.map(a => `<div class="trayrow"><div class="label">${NAMES[a]}</div><figure class="trayfig"><div class="chips"><span class="chip mac-l">${tray(a, 'default', 'template', 18)}</span><span class="chip mac-d">${tray(a, 'default', 'template', 18)}</span><span class="chip win-l">${tray(a, 'default', 'light')}</span><span class="chip win-d">${tray(a, 'default', 'dark')}</span></div><figcaption>tray</figcaption></figure></div>`).join('')}</div>
</section>
</body></html>
`;
writeFileSync(join(ROOT, 'preview.html'), html);
console.log('preview.html written', Buffer.byteLength(html));
