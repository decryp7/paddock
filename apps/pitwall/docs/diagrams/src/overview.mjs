// Hand-drawn overview SVG: three panels (outside, service, app) and a core bar, theme-aware through a prefers-color-scheme style
// block (one file, no <picture> needed). Usage: node diagrams/tools/overview.mjs <app> <out.svg>. This is Pitcrew's content as a
// worked example: copy the file, change the text and the box list, keep the layout rules of STYLE.md (horizontal arrows in gutters).

import { theme } from '../../../../../diagrams/tools/themes.mjs';
import { tw, wrap } from '../../../../../diagrams/tools/lib.mjs';
import { writeFileSync } from 'node:fs';

const t = theme('pitwall');
const FONT = '-apple-system,BlinkMacSystemFont,\'Segoe UI\',system-ui,sans-serif';
const roles = { accent: 'acc', ui: 'ui', core: 'core', process: 'pro', external: 'ext', group: 'grp' };
const vars = (i) => Object.entries(roles).map(([r, n]) => `--${n}-s:${t[r].s[i]};--${n}-f:${t[r].f[i]};`).join('') + `--bg:${t.white[i]};--ink:${t.text[i]};--mut:${t.muted[i]};--line:${t.edge[i]};`;
const out = [];
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const W = 1328;
const esc2 = esc;
function planned(x, y, w, h, role, name, body) {
  out.push(`<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="6" class="n pl ${roles[role]}s"/>`);
  text(x + 16, y + 28, name, 'h');
  wrap(body, 12, w - 32).forEach((l, i) => text(x + 16, y + 50 + i * 16, l, 't'));
  const tw2 = tw('planned', 11, true) + 14;
  out.push(`<rect x="${x + w - 8 - tw2}" y="${y + 8}" width="${tw2}" height="18" rx="9" class="tg"/>`);
  out.push(`<text x="${x + w - 8 - tw2 / 2}" y="${y + 21}" class="tgt">planned</text>`);
}


function text(x, y, s, cls) { out.push(`<text x="${x}" y="${y}" class="${cls}">${esc(s)}</text>`); }
function box(x, y, w, h, role, name, body) {
  const n = roles[role];
  out.push(`<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="6" class="n ${n}"/>`);
  text(x + 16, y + 28, name, 'h');
  wrap(body, 12, w - 32).forEach((l, i) => text(x + 16, y + 50 + i * 16, l, 't'));
}
function group(x, y, w, h, label) {
  out.push(`<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="10" class="g"/>`);
  text(x + 16, y + 28, label, 'gl');
}
function arrow(x1, y1, x2, y2, label, _a, dashed) {
  out.push(`<path d="M${x1} ${y1} L${x2} ${y2}" class="e${dashed ? ' ed' : ''}" marker-end="url(#a)"/>`);
  const w = tw(label, 12) + 4;
  text(Math.round((x1 + x2) / 2 - w / 2), y1 - 10, label, 'm');
}

text(48, 64, 'Pitwall', 'title');
text(48, 94, 'A local-only LLM server and manager: run language models on your own computer for the suite and any AI tool', 'sub');
text(48, 118, 'Serve, manage memory, keep keys and usage, show everything · Windows, macOS and Linux · nothing leaves 127.0.0.1', 'desc');

const OX = 48, OW = 304, SX = 472, SW = 416, AX = 1008, AW = 272, top = 160, ph = 704;
group(OX, top, OW, ph, 'Outside Pitwall');
group(SX, top, SW, ph, 'Pitwall server: serves and manages models');
group(AX, top, AW, ph, 'Pitwall app and CLI: see and control');

// service
box(SX + 24, 208, SW - 48, 128, 'accent', 'One gate', 'OpenAI, Anthropic and Ollama APIs plus /x/v1 on 127.0.0.1:11435. Every call passes Host, Origin and key checks; the window uses the admin key.');
box(SX + 24, 352, SW - 48, 104, 'accent', 'Queue and serving', 'Aliases and presets pick the model; priorities and caps queue it; caches answer repeats; it loads on first use and is released when idle.');
box(SX + 24, 472, SW - 48, 96, 'accent', 'Model library', 'Search, fit check, resumable verified downloads with their origin, and opt-in update notices that only ever check.');
box(SX + 24, 584, SW - 48, 80, 'accent', 'Engines', 'llama.cpp builds: self-test, pin, roll back; missing features are refused with a reason.');
box(SX + 24, 680, SW - 48, 80, 'accent', 'Keys, limits and usage', 'Per-app keys and limits; tokens, requests and attributed memory over time.');
box(SX + 24, 776, SW - 48, 72, 'process', 'Engine processes', 'llama-server, one child per loaded model.');
// outside
box(OX + 24, 208, OW - 48, 80, 'process', 'Apps and AI tools', 'Suite apps, editors, SDK scripts.');
planned(OX + 24, 304, OW - 48, 88, 'external', 'Other computers', 'Shared use over TLS with a pinned certificate.');
box(OX + 24, 432, OW - 48, 112, 'external', 'Download hosts', 'Hugging Face for models, GitHub releases for engine builds. Only what you ask for; the opt-in update check is read-only.');
box(OX + 24, 576, OW - 48, 88, 'process', 'OS service manager', 'Starts the server at login or boot and restarts it.');
// app
box(AX + 24, 208, AW - 48, 88, 'ui', 'Window', 'Dashboard, models, engines, apps, usage, settings.');
box(AX + 24, 312, AW - 48, 96, 'accent', 'Rust shell and tray', 'Validates every request; keeps the admin key in the OS store.');
box(AX + 24, 424, AW - 48, 88, 'accent', 'pitwall CLI', 'status, load, pull, keys, presets, chat, bench.');
box(AX + 24, 528, AW - 48, 88, 'process', 'Close it, crash it', 'The server goes on serving; quitting never stops it.');
// arrows (horizontal, in the gutters)
arrow(OX + OW - 24, 240, SX + 24, 240, 'ask', 1);
arrow(OX + OW - 24, 320, SX + 24, 320, 'ask', 1, true);
arrow(SX + 24, 490, OX + OW - 24, 490, 'downloads');
arrow(SX + 24, 530, OX + OW - 24, 530, 'check only, opt-in', 1, true);
arrow(OX + OW - 24, 624, SX + 24, 624, 'restarts', 1);
arrow(SX + SW - 24, 250, AX + 24, 250, 'live feed');
arrow(AX + 24, 330, SX + SW - 24, 330, 'manages');
arrow(AX + 24, 440, SX + SW - 24, 440, 'asks');

// core bar
out.push(`<rect x="48" y="904" width="1232" height="88" rx="6" class="n core"/>`);
text(64, 932, 'pitwall-core: pure Rust, no I/O, no clock', 'h');
text(64, 956, 'API translation, scheduler, memory policy, fit check, model choice, presets, slots, cache rules, keys, limits, usage, update rules, engine pins, privacy allow-list, settings.', 't');
text(64, 976, 'Time and readings come in as values. The server and the app both use it.', 't');
// legend
let lx = 64; const ly = 1048;
out.push(`<rect x="48" y="${ly - 20}" width="1000" height="40" rx="14" class="g"/>`);
for (const [r, label] of [['external', 'Outside'], ['accent', 'Pitwall'], ['ui', 'Window'], ['process', 'Process · OS'], ['core', 'Pure core']]) {
  out.push(`<rect x="${lx + 4}" y="${ly - 8}" width="28" height="16" rx="4" class="n ${roles[r]}"/>`);
  text(lx + 40, ly + 4, label, 'm');
  lx += 40 + tw(label, 12) + 36;
}
out.push(`<rect x="${lx + 4}" y="${ly - 8}" width="28" height="16" rx="4" class="n pl exts"/>`);
text(lx + 40, ly + 4, 'Planned (tagged)', 'm');
text(1280, ly + 4, 'Pitwall · updated 2026-10-31', 'foot');
text(48, 1112, 'The window and CLI reach the server only on 127.0.0.1: management calls with the admin key, standard APIs with an app key.', 'desc');
const H = 1136;

const css = `
    :root{${vars(0)}}
    @media (prefers-color-scheme: dark){:root{${vars(1)}}}
    text{font-family:${FONT};fill:var(--ink)}
    .title{font-size:24px;font-weight:600}.sub{font-size:14px;fill:var(--mut)}.desc{font-size:12px;fill:var(--mut)}
    .h{font-size:14px;font-weight:600}.t{font-size:12px}.m{font-size:12px;fill:var(--mut)}.gl{font-size:12px;font-weight:600;fill:var(--mut)}.foot{font-size:11px;fill:var(--mut);text-anchor:end}
    .g{fill:var(--grp-f);stroke:var(--grp-s);stroke-width:1}
    .n{stroke-width:1.5}
    .acc{fill:var(--acc-f);stroke:var(--acc-s)}.ui{fill:var(--ui-f);stroke:var(--ui-s)}.core{fill:var(--core-f);stroke:var(--core-s)}.pro{fill:var(--pro-f);stroke:var(--pro-s)}
    .ext{fill:var(--ext-f);stroke:var(--ext-s);stroke-dasharray:6 4}
    .pl{fill:var(--bg);stroke-dasharray:6 4}.exts{stroke:var(--ext-s)}.accs{stroke:var(--acc-s)}
    .tg{fill:var(--bg);stroke:var(--ext-s);stroke-width:1}.tgt{font-size:11px;font-weight:600;fill:var(--ext-s);text-anchor:middle}
    .e{stroke:var(--line);stroke-width:1.5;fill:none}.ed{stroke-dasharray:6 4}.ah{fill:var(--line)}`;
const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-labelledby="t d">
  <title id="t">Pitwall architecture overview</title>
  <desc id="d">Apps and AI tools call one local Pitwall server on 127.0.0.1 through the OpenAI, Anthropic and Ollama APIs. The server queues requests, loads a model into a llama.cpp engine process when it fits, releases it when idle, keeps per-app keys, limits and usage, and manages engines and the model library. The window, tray and command line manage it. Only downloads the user asked for leave the machine, from Hugging Face and GitHub releases, plus an opt-in, read-only update query to Hugging Face for models already downloaded. A pure Rust core holds all the rules. Sharing on the LAN is planned.</desc>
  <defs><marker id="a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0 L10 5 L0 10 z" class="ah"/></marker></defs>
  <style>${css}
  </style>
${out.join('\n')}
</svg>
`;
writeFileSync(new URL('../pitwall-overview.svg', import.meta.url).pathname, svg);
