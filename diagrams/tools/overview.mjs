// Hand-drawn overview SVG: three panels (outside, service, app) and a core bar, theme-aware through a prefers-color-scheme style
// block (one file, no <picture> needed). Usage: node diagrams/tools/overview.mjs <app> <out.svg>. This is Pitcrew's content as a
// worked example: copy the file, change the text and the box list, keep the layout rules of STYLE.md (horizontal arrows in gutters).

import { theme } from './themes.mjs';
import { tw, wrap } from './lib.mjs';
import { writeFileSync } from 'node:fs';

const t = theme(process.argv[2] || 'kit');
const FONT = '-apple-system,BlinkMacSystemFont,\'Segoe UI\',system-ui,sans-serif';
const roles = { accent: 'acc', ui: 'ui', core: 'core', process: 'pro', external: 'ext', group: 'grp' };
const vars = (i) => Object.entries(roles).map(([r, n]) => `--${n}-s:${t[r].s[i]};--${n}-f:${t[r].f[i]};`).join('') + `--ink:${t.text[i]};--mut:${t.muted[i]};--line:${t.edge[i]};`;
const out = [];
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const W = 1328;

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
function arrow(x1, y1, x2, y2, label) {
  out.push(`<path d="M${x1} ${y1} L${x2} ${y2}" class="e" marker-end="url(#a)"/>`);
  const w = tw(label, 12) + 4;
  text(Math.round((x1 + x2) / 2 - w / 2), y1 - 10, label, 'm');
}

text(48, 64, 'Pitcrew', 'title');
text(48, 94, 'A runner manager for Gitea and Forgejo: install, run, watch, update and clean CI runners', 'sub');
text(48, 118, 'Keep running, update safely, show everything · Windows, macOS and Linux', 'desc');

const OX = 48, OW = 304, SX = 472, SW = 416, AX = 1008, AW = 272, top = 160, ph = 656;
group(OX, top, OW, ph, 'Outside Pitcrew');
group(SX, top, SW, ph, 'Pitcrew service: keeps runners going');
group(AX, top, AW, ph, 'Pitcrew app: see and control');

// service
box(SX + 24, 208, SW - 48, 96, 'accent', 'Supervisor', 'One thread owns every runner: a controller, a job tracker and an update machine each, driven by one message channel.');
box(SX + 24, 328, SW - 48, 96, 'accent', 'Watchdog and restarts', 'Stall rule: 6 timed-out polls in 15 min with no job. Back-off 10 s to 5 min, crash-loop pause. A running job is never killed.');
box(SX + 24, 448, SW - 48, 96, 'accent', 'Safe updates', 'Download, verify, wait for the job, swap, restart, confirm. Roll back automatically if the new runner is not accepted.');
box(SX + 24, 568, SW - 48, 80, 'accent', 'Housekeeping, history, metrics', 'Cleanup of work and cache folders; uptime, restarts and jobs; CPU and memory of runners.');
box(SX + 24, 672, SW - 48, 72, 'process', 'Runner processes', 'gitea-runner, one per runner, children of the service.');
// outside
box(OX + 24, 208, OW - 48, 96, 'process', 'The operating system', 'Service manager, credential store, notifications and tray.');
box(OX + 24, 448, OW - 48, 112, 'external', 'Runner release hosts', 'dl.gitea.com and gitea.com: version, binary and SHA-256 checksums from two hosts. Pitcrew\'s own updates are opt-in.');
box(OX + 24, 672, OW - 48, 112, 'external', 'Gitea / Forgejo server', 'Hands out jobs; runners register and poll it. The app reads job outcomes with a read-only token.');
// app
box(AX + 24, 208, AW - 48, 120, 'ui', 'Window', 'Dashboard, runners, activity, logs, updates, settings and the register wizard, dark and light.');
box(AX + 24, 352, AW - 48, 104, 'ui', 'Tray and notifications', 'Worst state at a glance; stalls, crash loops, updates, disk.');
box(AX + 24, 480, AW - 48, 120, 'accent', 'Rust shell', 'Validates every request; keeps tokens in the OS credential store.');
box(AX + 24, 624, AW - 48, 120, 'process', 'Close it, crash it', 'The service goes on supervising; quitting the app never stops the runners.');
// arrows (horizontal, in the gutters)
arrow(OX + OW - 24, 256, SX + 24 - 0, 256, 'starts');
out.push('<!-- -->');
arrow(SX + 24, 496, OX + OW - 24, 496, 'downloads');
arrow(SX + 24, 708, OX + OW - 24, 708, 'polls for jobs');
arrow(SX + SW - 24, 256, AX + 24, 256, 'pushes events');
arrow(AX + 24, 536, SX + SW - 24, 536, 'requests');

// core bar
out.push(`<rect x="48" y="856" width="1232" height="88" rx="6" class="n core"/>`);
text(64, 884, 'pitcrew-core: pure Rust, no I/O, no clock', 'h');
text(64, 908, 'Controller, watchdog, restart policy, update machine and policy, housekeeping plan, history, metrics, config, protocol.', 't');
text(64, 928, 'Time comes in as values. The service and the app both use it.', 't');
// legend
let lx = 64; const ly = 1000;
out.push(`<rect x="48" y="${ly - 20}" width="860" height="40" rx="14" class="g"/>`);
for (const [r, label] of [['external', 'Outside'], ['accent', 'Pitcrew'], ['ui', 'Window and tray'], ['process', 'Process · OS'], ['core', 'Pure core']]) {
  out.push(`<rect x="${lx + 4}" y="${ly - 8}" width="28" height="16" rx="4" class="n ${roles[r]}"/>`);
  text(lx + 40, ly + 4, label, 'm');
  lx += 40 + tw(label, 12) + 36;
}
text(1280, ly + 4, 'Pitcrew · updated 2026-10-22', 'foot');
text(48, 1060, 'One local IPC joins app and service: a named pipe or Unix socket, length-prefixed JSON frames, pushed status events.', 'desc');
const H = 1084;

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
    .e{stroke:var(--line);stroke-width:1.5;fill:none}.ah{fill:var(--line)}`;
const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-labelledby="t d">
  <title id="t">Pitcrew architecture overview</title>
  <desc id="d">The outside world (the operating system, runner release hosts, the Gitea or Forgejo server) meets the Pitcrew service, which supervises runner processes, restarts stalled or crashed runners, updates them safely with roll-back and cleans their disks. The Pitcrew app (window, tray, Rust shell) shows and controls it over one local IPC. A pure Rust core holds all the rules.</desc>
  <defs><marker id="a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0 L10 5 L0 10 z" class="ah"/></marker></defs>
  <style>${css}
  </style>
${out.join('\n')}
</svg>
`;
writeFileSync(process.argv[3] || 'overview.svg', svg);
