import { theme } from '../../../../paddock/diagrams/tools/themes.mjs';
import { tw, wrap } from '../../../../paddock/diagrams/tools/lib.mjs';
import { writeFileSync } from 'node:fs';

const t = theme('spotter');
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

text(48, 64, 'Spotter', 'title');
text(48, 94, 'Watches for new releases anywhere and tells you the moment one appears', 'sub');
text(48, 118, 'Folders, listings, APIs, forges, feeds and pages · local only · Windows, macOS and Linux', 'desc');

const OX = 48, OW = 304, SX = 472, SW = 416, AX = 1008, AW = 272, top = 160, ph = 736;
group(OX, top, OW, ph, 'Outside Spotter');
group(SX, top, SW, ph, 'Spotter engine: watches and checks');
group(AX, top, AW, ph, 'Spotter app: see and act');

// engine
box(SX + 24, 208, SW - 48, 88, 'accent', 'Scheduler', 'Every 5 seconds the due watches run, a few at a time, each at its own interval with back-off and jitter.');
box(SX + 24, 320, SW - 48, 96, 'accent', 'Polite fetching', 'One request at a time per host, ETag and Last-Modified, size caps, capped redirects. Loopback and link-local addresses are refused.');
box(SX + 24, 440, SW - 48, 96, 'accent', 'Page rendering', 'JavaScript pages run in a supervised, off-screen Chromium with a throwaway profile. A login or captcha becomes “Needs you”, never solved.');
box(SX + 24, 560, SW - 48, 96, 'accent', 'Select, soak, decide', 'Orders versions, applies your filters and soak time and compares with what the watch remembers. Presets are TOML packs.');
box(SX + 24, 680, SW - 48, 96, 'accent', 'Notify and act', 'OS notification, digests and quiet hours, webhook and ntfy channels, then the watch\'s actions on the new release.');
box(SX + 24, 800, SW - 48, 72, 'process', 'Local data', 'SQLite history and spotter.toml in your data folder.');
// outside
box(OX + 24, 320, OW - 48, 96, 'external', 'Release places', 'Folders and shares, HTTP listings, JSON APIs, forges, feeds and web pages.');
box(OX + 24, 440, OW - 48, 96, 'external', 'Chromium', 'Edge, Chrome or Chromium; or a pinned, verified download on request.');
box(OX + 24, 680, OW - 48, 96, 'process', 'OS and channels', 'Notifications, the credential store, and the webhook, ntfy and e-mail channels you add.');
// app
box(AX + 24, 208, AW - 48, 128, 'ui', 'Window', 'Dashboard, watches, detail, editor and wizard, settings. Dark and light.');
box(AX + 24, 360, AW - 48, 112, 'ui', 'Tray popup', 'Unseen releases at a glance; check all, pause notifications.');
box(AX + 24, 496, AW - 48, 112, 'accent', 'Rust shell', 'Validates every request from the web view; keeps secrets in the OS store.');
box(AX + 24, 680, AW - 48, 96, 'accent', 'Notifier and actions', 'Shows the notification; opens, downloads or runs, with time limits.');
box(AX + 24, 792, AW - 48, 80, 'process', 'Close it', 'The tray stays; Quit is the only exit.');
// arrows (horizontal, in the gutters)
arrow(SX + 24, 368, OX + OW - 24, 368, 'fetches');
arrow(SX + 24, 488, OX + OW - 24, 488, 'renders');
arrow(SX + 24, 728, OX + OW - 24, 728, 'notifies');
arrow(SX + SW - 24, 256, AX + 24, 256, 'pushes changes');
arrow(AX + 24, 584, SX + SW - 24, 584, 'commands');
arrow(SX + SW - 24, 728, AX + 24, 728, 'hands over');

// core bar
out.push(`<rect x="48" y="936" width="1232" height="88" rx="6" class="n core"/>`);
text(64, 964, 'spotter-core: pure Rust, no I/O, no clock', 'h');
text(64, 988, 'Version schemes and ordering, filters, soak time, schedule and back-off, notification policy and digests, packs, config migration.', 't');
text(64, 1008, 'Time comes in as values. The engine, the shell and the tests all use it.', 't');
// legend
let lx = 64; const ly = 1080;
out.push(`<rect x="48" y="${ly - 20}" width="860" height="40" rx="14" class="g"/>`);
for (const [r, label] of [['external', 'Outside'], ['accent', 'Spotter'], ['ui', 'Window and tray'], ['process', 'Process · OS · data'], ['core', 'Pure core']]) {
  out.push(`<rect x="${lx + 4}" y="${ly - 8}" width="28" height="16" rx="4" class="n ${roles[r]}"/>`);
  text(lx + 40, ly + 4, label, 'm');
  lx += 40 + tw(label, 12) + 36;
}
text(1280, ly + 4, 'Spotter · updated 2026-10-23', 'foot');
text(48, 1140, 'Everything stays on your machine: no cloud, no account, no telemetry. The window reaches the engine only through Tauri commands.', 'desc');
const H = 1164;
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
  <title id="t">Spotter architecture overview</title>
  <desc id="d">Release places (folders and shares, HTTP listings, JSON APIs, forges, feeds, web pages) and a supervised Chromium sit outside Spotter. The Spotter engine schedules checks, fetches politely, renders JavaScript pages, selects and soaks versions, and notifies and acts, keeping its history in local SQLite. The Spotter app (window, tray popup, Rust shell) shows and controls it through Tauri commands. A pure Rust core holds all the rules. Everything stays on the machine.</desc>
  <defs><marker id="a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0 L10 5 L0 10 z" class="ah"/></marker></defs>
  <style>${css}
  </style>
${out.join('\n')}
</svg>
`;
writeFileSync(new URL('../spotter-overview.svg', import.meta.url), svg);
