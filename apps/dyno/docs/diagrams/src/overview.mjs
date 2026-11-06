// Hand-drawn, theme-aware overview of Dyno (one SVG, light and dark through a prefers-color-scheme style block).
// Same recipe as Paddock's diagrams/tools/overview.mjs. Usage: node docs/diagrams/src/overview.mjs  ->  diagrams/dyno-overview.svg
import { theme } from '../../../../../diagrams/tools/themes.mjs';
import { tw, wrap } from '../../../../../diagrams/tools/lib.mjs';
import { writeFileSync } from 'node:fs';
import { OUT } from './common.mjs';

const t = theme('dyno');
// Plugin colour: the kit's is too close to Dyno's magenta, so use the sky chart colour (same as common.mjs).
const mix = (hex, bg, a) => {
  const q = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
  const [c, b] = [q(hex), q(bg)];
  return '#' + c.map((v, i) => Math.round(b[i] + (v - b[i]) * a).toString(16).padStart(2, '0')).join('').toUpperCase();
};
t.plugin = { s: ['#0072B2', '#56B4E9'], f: [mix('#0072B2', '#FFFFFF', 0.1), mix('#56B4E9', '#15171B', 0.16)] };
const FONT = '-apple-system,BlinkMacSystemFont,\'Segoe UI\',system-ui,sans-serif';
const roles = { accent: 'acc', ui: 'ui', core: 'core', process: 'pro', external: 'ext', group: 'grp', plugin: 'plg' };
const vars = (i) => Object.entries(roles).map(([r, n]) => `--${n}-s:${t[r].s[i]};--${n}-f:${t[r].f[i]};`).join('') + `--ink:${t.text[i]};--mut:${t.muted[i]};--line:${t.edge[i]};--bg:${t.white[i]};`;
const out = [];
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const W = 1328;

function text(x, y, s, cls) { out.push(`<text x="${x}" y="${y}" class="${cls}">${esc(s)}</text>`); }
function box(x, y, w, h, role, name, body, planned = false, tag = planned ? 'planned' : '') {
  out.push(`<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="6" class="n ${roles[role]}${planned ? ' pl' : ''}"/>`);
  text(x + 16, y + 28, name, 'h');
  if (tag) {
    const pw = tw(tag, 11, true) + 14;
    out.push(`<rect x="${x + w - 8 - pw}" y="${y + 10}" width="${pw}" height="18" rx="9" class="tag"/>`);
    text(x + w - 8 - pw / 2, y + 23, tag, 'tg');
  }
  wrap(body, 12, w - 32).forEach((l, i) => text(x + 16, y + 50 + i * 16, l, 't'));
}
function group(x, y, w, h, label) {
  out.push(`<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="10" class="g"/>`);
  text(x + 16, y + 28, label, 'gl');
}
/** Horizontal arrow with its label above the line. */
function arrow(x1, y, x2, label, dash = false) {
  out.push(`<path d="M${x1} ${y} L${x2} ${y}" class="e${dash ? ' d' : ''}" marker-end="url(#a)"/>`);
  const w = tw(label, 12) + 4;
  text(Math.round((x1 + x2) / 2 - w / 2), y - 8, label, 'm');
}
/** Vertical arrow with its label to the right, in the gap between two boxes. */
function varrow(x, y1, y2, label) {
  out.push(`<path d="M${x} ${y1} L${x} ${y2}" class="e" marker-end="url(#a)"/>`);
  text(x + 10, Math.round((y1 + y2) / 2 + 4), label, 'm');
}

text(48, 64, 'Dyno', 'title');
text(48, 94, 'Watches one PC: a per-user agent keeps the history, a window, tray and command line show it', 'sub');
text(48, 118, 'Live and history, why slow, alarms, a disk map and benchmarks · acts only when asked · Windows, macOS and Linux', 'desc');

const AX = 48, AW = 304, BX = 472, BW = 416, CX = 1008, CW = 272, top = 160;
const BH = 88, PITCH = 120, y = (i) => 208 + i * PITCH;
const gh = (rows) => 48 + (rows - 1) * PITCH + BH + 24 - 8;
group(AX, top, AW, gh(4), 'Dyno app and command line');
group(BX, top, BW, gh(5), 'Dyno agent [one per user, started at login]');
group(CX, top, CW, gh(5), 'Around the agent');

// app
box(AX + 24, y(0), AW - 48, BH, 'ui', 'Window', 'Angular 22: live and history, processes, why slow, alarms, disk map, counters, benchmarks.');
box(AX + 24, y(1), AW - 48, BH, 'accent', 'Tauri shell', 'Commands; validates every request and speaks to the agent.');
box(AX + 24, y(2), AW - 48, BH, 'accent', 'Tray', 'A live metric and a compact menu; stays when the window closes.');
box(AX + 24, y(3), AW - 48, BH, 'accent', 'dyno command', 'status, why-slow, top, alarms, snapshot, counters. Reads only.');
varrow(AX + 48, y(0) + BH, y(1), 'commands');
varrow(AX + 48, y(1) + BH, y(2), 'status');
// agent
const bx = BX + 24, bw = BW - 48;
box(bx, y(0), bw, BH, 'accent', 'Sampler', 'Adaptive intervals: sensors, processes, grouping, diagnosis, baselines; measures its own overhead.');
box(bx, y(1), bw, BH, 'accent', 'IPC server and handlers', 'One thread per connection, bounded push queue; answers from measured or stored data.');
box(bx, y(2), bw, BH, 'accent', 'Plugin host', 'Starts sensor plugins on first use; a plugin that crashes is quarantined.');
box(bx, y(3), bw, BH, 'accent', 'Benchmarks', 'Only on a click, one kind at a time, in a child process with a timeout.');
box(bx, y(4), bw, BH, 'accent', 'History, alarms, recorder', 'SQLite rollups (min, average, max); alarms that notify; issues; ten-minute flight recorder.');
// around
const cx = CX + 24, cw = CW - 48;
box(cx, y(0), cw, BH, 'accent', 'Platform adapters', 'Windows, macOS, Linux behind the core\'s ports.');
box(cx, y(1), cw, BH, 'process', 'Operating system', 'Counters, /proc and /sys, OS logs, launchd, systemd.');
box(cx, y(2), cw, BH, 'plugin', 'Plugins', 'Vendor sensors; out of process.');
box(cx, y(3), cw, BH, 'process', 'dyno-bench', 'CPU, memory and disk tests.');
box(cx, y(4), cw, BH, 'process', 'Data folder', 'Dyno\'s own data: dyno.db and JSON; never uploaded.');
varrow(cx + 24, y(0) + BH, y(1), 'reads');
// horizontal arrows in the gutters
const mid = (i) => y(i) + BH / 2;
arrow(AX + AW - 24, y(1) + 22, bx, 'requests');
arrow(bx, y(1) + 50, AX + AW - 24, 'pushes');
// the command line is a second client of the same IPC server: up the gutter, into the server below the shell's pair
{
  const gx = 400, x0 = AX + AW - 24, ye = y(1) + 72;
  out.push(`<path d="M${x0} ${mid(3)} L${gx - 8} ${mid(3)} Q${gx} ${mid(3)} ${gx} ${mid(3) - 8} L${gx} ${ye + 8} Q${gx} ${ye} ${gx + 8} ${ye} L${bx} ${ye}" class="e" marker-end="url(#a)"/>`);
  text(gx + 8, Math.round((y(2) + BH + y(3)) / 2 + 4), 'requests', 'm');
}
arrow(bx + bw, mid(0), cx, 'samples');
arrow(bx + bw, mid(2), cx, 'calls');
arrow(bx + bw, mid(3), cx, 'spawns');
arrow(bx + bw, mid(4), cx, 'writes');

// core bar
const coreY = top + gh(5) + 40;
out.push(`<rect x="48" y="${coreY}" width="1232" height="88" rx="6" class="n core"/>`);
text(64, coreY + 28, 'dyno-core: pure Rust, no I/O, no clock', 'h');
text(64, coreY + 52, 'Grouping, diagnosis, baselines, health reasons, alarms, issues, history tiers, protocol.', 't');
text(64, coreY + 72, 'Time comes in as values. The agent and the app both use it; the platform adapters implement its ports.', 't');
// legend
let lx = 64; const ly = coreY + 136;
const legAt = out.length;
for (const [r, label, pl] of [['ui', 'Window'], ['accent', 'Dyno'], ['core', 'Pure core'], ['plugin', 'Plugin'], ['process', 'Process · OS · data']]) {
  out.push(`<rect x="${lx + 4}" y="${ly - 8}" width="28" height="16" rx="4" class="n ${roles[r]}${pl ? ' pl' : ''}"/>`);
  text(lx + 40, ly + 4, label, 'm');
  lx += 40 + tw(label, 12) + 36;
}
out.splice(legAt, 0, `<rect x="48" y="${ly - 20}" width="${lx - 48 - 20}" height="40" rx="14" class="g"/>`);
text(1280, ly + 4, 'Dyno · updated 2026-10-28', 'foot');
const H = ly + 48;

const css = `
    :root{${vars(0)}}
    @media (prefers-color-scheme: dark){:root{${vars(1)}}}
    text{font-family:${FONT};fill:var(--ink)}
    .title{font-size:24px;font-weight:600}.sub{font-size:14px;fill:var(--mut)}.desc{font-size:12px;fill:var(--mut)}
    .h{font-size:14px;font-weight:600}.t{font-size:12px}.m{font-size:12px;fill:var(--mut)}.gl{font-size:12px;font-weight:600;fill:var(--mut)}.foot{font-size:11px;fill:var(--mut);text-anchor:end}
    .tg{font-size:11px;font-weight:600;fill:var(--pro-s);text-anchor:middle}
    .g{fill:var(--grp-f);stroke:var(--grp-s);stroke-width:1}
    .n{stroke-width:1.5}
    .acc{fill:var(--acc-f);stroke:var(--acc-s)}.ui{fill:var(--ui-f);stroke:var(--ui-s)}.core{fill:var(--core-f);stroke:var(--core-s)}.pro{fill:var(--pro-f);stroke:var(--pro-s)}.plg{fill:var(--plg-f);stroke:var(--plg-s)}
    .ext{fill:var(--ext-f);stroke:var(--ext-s);stroke-dasharray:6 4}
    .pl{fill:var(--bg);stroke-dasharray:6 4}
    .tag{fill:var(--bg);stroke:var(--pro-s);stroke-width:1}
    .e{stroke:var(--line);stroke-width:1.5;fill:none}.e.d{stroke-dasharray:6 4}.ah{fill:var(--line)}`;
const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-labelledby="t d">
  <title id="t">Dyno architecture overview</title>
  <desc id="d">Dyno only watches. The Dyno app (an Angular window and a tray in a Tauri shell) and the dyno command line talk over one local IPC to a per-user agent. The agent samples, answers requests, hosts sensor plugins, runs benchmarks in a child process on a click and keeps history, alarms, issues and a flight recorder in SQLite. Around the agent are the platform adapters for Windows, macOS and Linux, the operating system, out-of-process plugins, the benchmark process and the data folder. A pure Rust core holds every decision.</desc>
  <defs><marker id="a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0 L10 5 L0 10 z" class="ah"/></marker></defs>
  <style>${css}
  </style>
${out.join('\n')}
</svg>
`;
writeFileSync(`${OUT}/dyno-overview.svg`, svg);
