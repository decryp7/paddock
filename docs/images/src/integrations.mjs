// Generates the two integration drawings with the Paddock diagram kit and draw.io (+ light/dark 2x PNGs with --png):
//   docs/images/paddock-integrations.svg  apps, Pitwall's processes, MCP servers (what talks to Pitwall and how)
//   docs/images/paddock-handoffs.svg      the hand-offs between apps (files, deep links, MCP), none of which needs Pitwall
//   node docs/images/src/integrations.mjs [--png]      (draw.io desktop: set DRAWIO to override the path)
// Both canvases stay at most 1100 px wide (hand-offs 896) so the 11 px pills and footer are still 9 px at README width (about 900 px).
import { readFileSync, writeFileSync, mkdirSync, copyFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { versionPaddockDocs } from './version-links.mjs';
import { screenshotSvg } from './chrome.mjs';
import { Diagram, link } from '../../../diagrams/tools/lib.mjs';
import { APPS } from '../../../diagrams/tools/themes.mjs';
import { CODEBASE_TOOL } from './family.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, '..');
const date = new Date().toLocaleDateString('sv-SE'); // local YYYY-MM-DD, like every other date in the docs
const W = 1088;

// Planned family apps keep the solid outline of their role and carry only the "planned" pill, so they never look like the
// dashed external systems (other MCP clients). In a narrow box the pill would cover the name, so it sits at the bottom right
// (`bottom`; the caller leaves 24 px for it).
function app(d, id, x, y, w, h, opts, planned = false, bottom = false) {
  const r = d.node(id, x, y, w, h, 'process', opts);
  if (planned) d.tag(x + w - 8, bottom ? y + h - 26 : y + 8, 'planned', 'process');
  return r;
}

// ---------------------------------------------------------------- integrations: apps, Pitwall, MCP servers
{
  const d = new Diagram({
    title: 'Paddock integrations', kind: 'Deployment · Processes and protocols', app: 'pitwall', date, w: W,
    desc: 'Pitwall is the optional local AI hub. Apps reach it through one shared client (Spotter, Dyno and Recce later); nothing leaves the machine.',
  });
  d.t.app = 'Paddock';

  // Apps on this machine (top), each on the shared paddock-pitwall client; Spotter, Dyno and Recce adopt it later (dashed).
  const AY = 144, NW = 144, STEP = 160;
  const apps = [
    ['pacenotes', CODEBASE_TOOL, 'app and service', 'Answers, explanations and embeddings.', true],
    ['debrief', 'Debrief', 'app', 'AI engine once paired; built-in engine as fallback.', false],
    ['sighter', 'Sighter', 'app and server', 'Vision, and the agent planner.', true],
    ['spotter', 'Spotter', 'app', 'What’s-new summaries.', false, true],
    ['dyno', 'Dyno', 'app and agent', 'Explains what it shows.', false, true],
    ['recce', 'Recce', 'app and agent', 'Answers with sources; reranking.', true, true],
  ];
  const NH = Math.max(...apps.map(([, name, tech, desc]) => d.fit(NW, { name, tech, desc }))) + 16;
  const LIB = { name: 'paddock-pitwall', tech: 'shared Rust crate in each app', desc: 'One client: discovery, pairing, keychain keys, retries, optional-AI state.' };
  const LY = AY + 48 + NH + 40, LH = d.fit(944, LIB), AGB = LY + LH + 24;

  // Pitwall: one service process (left); MCP servers (right). Geometry first: groups are drawn before (under) their nodes.
  const PY = AGB + 64, API_Y = PY + 48;
  const API = { name: 'Local API', tech: 'HTTP on 127.0.0.1',
    desc: 'OpenAI, Anthropic and Ollama compatible, plus /x/v1. One key per app, job priorities, prompt and response caching, usage and limits per app.' };
  const apiH = d.fit(656, API);
  const RY = API_Y + apiH + 48;
  const CRED = { name: 'OS credential store', tech: 'Keychain, Credential Manager, Secret Service', desc: 'Holds the per-app keys.' };
  const ENG = { name: 'llama.cpp', tech: 'child process, loopback', desc: 'The only engine. Runs the model on your hardware.' };
  const RT = { name: 'Tool router', tech: 'MCP client and host', desc: 'Pitwall 1.1: local models and chat call MCP tools.' };
  const credH = d.fit(200, CRED), engH = d.fit(184, ENG), rtH = d.fit(160, RT) + 24;
  const rowH = Math.max(credH, engH + 56, rtH);
  const NOTE = 'Local only: the sole outbound traffic is the model and engine downloads you ask for.';
  // The note stops short of the tool router's column, so the router's MCP line runs down beside it to the servers below.
  const NY = RY + rowH + 32, noteH = 48, NW2 = 456, PB = NY + noteH + 24;

  d.group('apps', 48, AY, 992, AGB - AY, 'Apps on this machine');
  d.group('pw', 48, PY, 704, PB - PY, 'Pitwall: one service process, loopback only');
  d.group('os', 312, RY, 216, engH + 56, 'Engine process', { os: true });
  // The process boundary is dotted here, so it never reads as the dashed "Outside Paddock" box of the legend.
  d.cells[d.cells.length - 1] = d.cells[d.cells.length - 1].replace('dashPattern=6 4;', 'dashPattern=1 4;');
  apps.forEach(([id, name, tech, desc, planned], i) => app(d, id, 72 + i * STEP, AY + 48, NW, NH, { name, tech, desc }, planned, true));
  d.node('lib', 72, LY, 944, LH, 'core', LIB);
  apps.forEach(([id, , , , , later]) => link(d, id, 'B', 'lib', 'T', { dash: !!later }));
  d.node('api', 72, API_Y, 656, apiH, 'accent', API);
  d.node('cred', 72, RY, 200, credH, 'process', CRED);
  d.node('eng', 328, RY + 40, 184, engH, 'process', ENG);
  d.node('router', 568, RY, 160, rtH, 'accent', RT);
  d.tag(568 + 160 - 8, RY + rtH - 26, 'planned', 'accent');
  d.note('local', 72, NY, NW2, NOTE, noteH);

  link(d, 'lib', 'B', 'api', 'T', { label: 'HTTP, one key per app', at: 0.4 });
  link(d, 'api', 'B', 'cred', 'T');
  d.edge('api', 'eng', [472, API_Y + apiH], [472, RY + 40]); // right of the "Engine process" group label
  link(d, 'api', 'B', 'router', 'T');

  // MCP servers: tools any MCP client may call (ADR 0007), in one row under Pitwall. Pitwall's tool router (planned for Pitwall
  // 1.1) is one such client; other MCP clients (right of Pitwall) are the rest. Recce is the family's retrieval engine, so Pitwall
  // builds no index of its own (Recce plan, 2026-11-04). Dyno's server is on its unreleased 1.1 line (opt-in in Settings > AI
  // access), so it carries a "1.1" pill instead of "planned"; the others are planned.
  const MW = 176, MGAP = 16, MY = PB + 48;
  const CL = { name: 'Other MCP clients', tech: 'editors, scripts', desc: 'Same servers, same tools.' };
  // Beside the tool router, top-aligned with it: the two kinds of MCP client side by side, each with its line down to the servers.
  const CW2 = 208, clH = d.fit(CW2, CL), CX = 808, CY = RY;
  const servers = [
    ['code', CODEBASE_TOOL, 'MCP server, read-only', 'Code graph, review, search.'],
    ['dbmcp', 'Debrief', 'MCP server, read-only', 'Logs and analysis; first.'],
    ['sigmcp', 'Sighter', 'MCP server, acts', 'This PC or one over RDP.'],
    ['dynomcp', 'Dyno', 'MCP server, opt-in', '8 reading tools, 2 opt-in actions.'],
    ['recmcp', 'Recce', 'MCP server, read-only', 'Search with citations.'],
  ];
  const SH = Math.max(...servers.map(([, name, tech, desc]) => d.fit(MW, { name, tech, desc })));
  const MB = MY + 48 + SH + 24;
  d.group('mcp', 48, MY, 992, MB - MY, 'MCP servers');
  servers.forEach(([id, name, tech, desc], k) => {
    const x = 72 + k * (MW + MGAP);
    app(d, id, x, MY + 48, MW, SH, { name, tech, desc }, id !== 'dynomcp');
    if (id === 'dynomcp') d.tag(x + MW - 8, MY + 56, '1.1', 'process');
  });
  d.node('mcpc', CX, CY, CW2, clH, 'external', CL);
  const RXM = 568 + 80; // the router's centre
  d.edge('router', 'mcp', [RXM, RY + rtH], [RXM, MY], { dash: true });
  d.lbl('MCP', RXM, (PB + MY) / 2, 'r');
  d.edge('mcpc', 'mcp', [CX + CW2 / 2, CY + clH], [CX + CW2 / 2, MY]);
  d.lbl('MCP', CX + CW2 / 2, (PB + MY) / 2, 'r');

  d.legend([{ kind: 'accent', label: 'Pitwall' }, { kind: 'process', label: 'Process' },
    { kind: 'external', label: 'Outside Paddock' }, { kind: 'tag', role: 'process', label: 'Not built yet' }, { kind: 'line', label: 'Request' },
    { kind: 'dashline', label: 'Later' }]);
  write(d, 'paddock-integrations', MB);
}

// ---------------------------------------------------------------- hand-offs between apps
// Two clusters. Top: everything that flows into Debrief (one fan-in) and what Debrief sends on. Bottom: the hand-offs among
// Dyno, Spotter, Pitcrew, Pacenotes and Sighter. Labels are the hero's wording; every one is in the contract table of
// docs/ecosystem.md. Each app box carries its accent bar (design guide), so the eye finds an app at once.
{
  const d = new Diagram({
    title: 'Paddock hand-offs', kind: 'Context · Files, deep links and MCP between apps', app: 'pitwall', date, w: 896,
    desc: 'Each app does its own job alone; these are the places where they meet. Pitwall is optional for all of them.',
  });
  d.t.app = 'Paddock';
  const BW = 160, BH = 48, GAP = 160, C = [48, 48 + BW + GAP, 48 + 2 * (BW + GAP)];
  const isPlanned = (id) => /^(code|sighter)/.test(id);
  const appOf = (id) => (id.startsWith('code') ? 'pacenotes' : id.replace(/\d+$/, ''));
  const box = (id, name, x, y, h = BH) => {
    app(d, id, x, y, BW, h, { name }, isPlanned(id));
    const [l, k] = APPS[appOf(id)].accent;
    d.raw(`<mxCell id="${d.id()}" value="" style="rounded=1;absoluteArcSize=1;arcSize=2;strokeColor=none;fillColor=light-dark(${l},${k});" vertex="1" parent="1"><mxGeometry x="${x + 14}" y="${y + 38}" width="20" height="3" as="geometry"/></mxCell>`);
  };
  const row = (y0, i) => y0 + i * (BH + 24);

  // Top: Sighter, Pitcrew, Dyno, Pacenotes -> Debrief -> Pacenotes, Sighter.
  const T0 = 144, into = [['sighter1', 'Sighter', 'Run bundles'], ['pitcrew1', 'Pitcrew', 'Job logs'], ['dyno1', 'Dyno', 'Issues'],
    ['code1', CODEBASE_TOOL, 'Log checks']];
  box('debrief', 'Debrief', C[1], T0, row(0, into.length) - 24);
  into.forEach(([id, name, label], i) => { box(id, name, C[0], row(T0, i)); link(d, id, 'R', 'debrief', 'L', { label }); });
  box('code2', CODEBASE_TOOL, C[2], row(T0, 1));
  box('sighter2', 'Sighter', C[2], row(T0, 2));
  link(d, 'debrief', 'R', 'code2', 'L', { label: 'Trace → code' });
  link(d, 'debrief', 'R', 'sighter2', 'L', { label: 'Video repro (MCP)' });

  // Bottom: Dyno -> Pitcrew, Spotter; Spotter -> Pacenotes, Sighter (smoke tests later); Pacenotes <-> Sighter.
  const B0 = row(T0, into.length) + 32, R1 = B0 + BH + 32, R2 = R1 + BH + 64;
  box('dyno2', 'Dyno', C[0], B0, R1 + BH - B0);
  box('pitcrew2', 'Pitcrew', C[1], B0);
  box('spotter1', 'Spotter', C[1], R1, R2 + BH - R1);
  box('code3', CODEBASE_TOOL, C[2], R1);
  box('sighter3', 'Sighter', C[2], R2);
  link(d, 'dyno2', 'R', 'pitcrew2', 'L', { label: 'Machine health' });
  link(d, 'dyno2', 'R', 'spotter1', 'L', { label: 'Installed software', t: [(R1 + BH / 2 - B0) / (R1 + BH - B0), 0.5 * BH / (R2 + BH - R1)] });
  link(d, 'spotter1', 'R', 'code3', 'L', { label: 'Newer version' });
  link(d, 'spotter1', 'R', 'sighter3', 'L', { label: 'Smoke tests', dash: true });
  const ax = C[2] + 52, bx = C[2] + BW - 52;
  d.edge('code3', 'sighter3', [ax, R1 + BH], [ax, R2]);
  d.lbl('Test plans (MCP)', ax, R1 + BH + 32, 'l');
  d.edge('sighter3', 'code3', [bx, R2], [bx, R1 + BH]);
  d.lbl('Results', bx, R1 + BH + 32, 'r');

  d.legend([{ kind: 'process', label: 'App' }, { kind: 'tag', role: 'process', label: 'Not built yet' },
    { kind: 'line', label: 'Hand-off' }, { kind: 'dashline', label: 'Later' }]);
  write(d, 'paddock-handoffs', R2 + BH);
}

function write(d, name, bottom) {
  const src = join(here, 'drawio');
  mkdirSync(src, { recursive: true });
  writeFileSync(join(src, `${name}.drawio`), d.finish(bottom));
  console.log(`wrote ${name}.drawio`);
  const drawio = process.env.DRAWIO || '/Applications/draw.io.app/Contents/MacOS/draw.io';
  const tmp = join(process.env.TMPDIR || '/tmp', 'paddock-int');
  execFileSync(join(here, '../../../diagrams/scripts/export-diagrams.sh'), [src, tmp, name], { env: { ...process.env, DRAWIO: drawio }, stdio: 'inherit' });
  copyFileSync(join(tmp, 'svg', `${name}.svg`), join(out, `${name}.svg`));
  if (process.argv.includes('--png')) {
    const m = readFileSync(join(out, `${name}.svg`), 'utf8').match(/width="(\d+)px" height="(\d+)px"/);
    for (const theme of ['light', 'dark']) {
      screenshotSvg(join(out, `${name}.svg`), join(out, `${name}-${theme}.png`), Number(m[1]), Number(m[2]), theme);
      console.log(`wrote ${name}-${theme}.png`);
    }
  }
}
versionPaddockDocs(join(out, '../..'));
