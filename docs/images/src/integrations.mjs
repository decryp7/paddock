// Generates the two integration drawings with the Paddock diagram kit and draw.io (+ light/dark 2x PNGs with --png):
//   docs/images/paddock-integrations.svg  apps, Pitwall's processes, MCP servers (what talks to Pitwall and how)
//   docs/images/paddock-handoffs.svg      the hand-offs between apps (files, deep links, MCP), none of which needs Pitwall
//   node docs/images/src/integrations.mjs [--png]      (draw.io desktop: set DRAWIO to override the path)
// Both canvases stay at most 1100 px wide so the 11 px pills and footer are still 9 px at README width (about 900 px).
import { readFileSync, writeFileSync, mkdirSync, copyFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { versionPaddockDocs } from './version-links.mjs';
import { screenshotSvg } from './chrome.mjs';
import { Diagram, link } from '../../../diagrams/tools/lib.mjs';
import { CODEBASE_TOOL } from './family.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, '..');
const date = new Date().toISOString().slice(0, 10);
const W = 1088;

// Planned family apps keep the solid outline of their role and carry only the "planned" pill, so they never look like the
// dashed external systems (Gitea runners, other MCP clients).
function app(d, id, x, y, w, h, opts, planned = false) {
  const r = d.node(id, x, y, w, h, 'process', opts);
  if (planned) d.tag(x + w - 8, y + 8, 'planned', 'process');
  return r;
}

// ---------------------------------------------------------------- integrations: apps, Pitwall, MCP servers
{
  const d = new Diagram({
    title: 'Paddock integrations', kind: 'Deployment · Processes and protocols', app: 'pitwall', date, w: W,
    desc: 'Pitwall is the optional local AI hub. Apps reach it through one shared client; nothing leaves the machine.',
  });
  d.t.app = 'Paddock';

  // Apps on this machine (top), each on the shared paddock-pitwall client.
  const AY = 144, NW = 176, STEP = 192;
  const apps = [
    ['pacenotes', CODEBASE_TOOL, 'Tauri app and service', 'Answers, explanations and embeddings.', true],
    ['debrief', 'Debrief', 'Tauri app', 'AI engine once paired; built-in engine as fallback.', false],
    ['sighter', 'Sighter', 'Tauri app and server', 'Vision, and the agent planner.', true],
    ['spotter', 'Spotter', 'Tauri app', 'What’s-new summaries.', false],
    ['dyno', 'Dyno', 'Tauri app and agent', 'Diagnosis in plain words.', false],
  ];
  const NH = Math.max(...apps.map(([, name, tech, desc]) => d.fit(NW, { name, tech, desc })));
  const LIB = { name: 'paddock-pitwall', tech: 'shared Rust crate in each app', desc: 'One client: discovery, pairing, keychain keys, retries, optional-AI state.' };
  const LY = AY + 48 + NH + 40, LH = d.fit(944, LIB), AGB = LY + LH + 24;

  // Pitwall: one service process (left); MCP servers (right). Geometry first: groups are drawn before (under) their nodes.
  const PY = AGB + 80, API_Y = PY + 48;
  const API = { name: 'Local API', tech: 'HTTP on 127.0.0.1',
    desc: 'OpenAI, Anthropic and Ollama compatible, plus /x/v1. One key per app, job priorities, prompt and response caching, usage and limits per app.' };
  const apiH = d.fit(656, API);
  const RY = API_Y + apiH + 48;
  const CRED = { name: 'OS credential store', tech: 'Keychain, Credential Manager, Secret Service', desc: 'Holds the per-app keys.' };
  const ENG = { name: 'llama.cpp', tech: 'child process, loopback', desc: 'The only engine. Runs the model on your hardware.' };
  const RT = { name: 'Tool router', tech: 'MCP client', desc: 'Offers MCP tools to local models.' };
  const credH = d.fit(200, CRED), engH = d.fit(184, ENG), rtH = d.fit(160, RT);
  const rowH = Math.max(credH, engH + 56, rtH);
  const NOTE = 'Local only: the sole outbound traffic is the model and engine downloads you ask for.';
  const NY = RY + rowH + 32, noteH = 40, PB = NY + noteH + 24;

  d.group('apps', 48, AY, 992, AGB - AY, 'Apps on this machine');
  d.group('pw', 48, PY, 704, PB - PY, 'Pitwall: one service process, loopback only');
  d.group('os', 312, RY, 216, engH + 56, 'Engine process', { os: true });
  apps.forEach(([id, name, tech, desc, planned], i) => app(d, id, 72 + i * STEP, AY + 48, NW, NH, { name, tech, desc }, planned));
  d.node('lib', 72, LY, 944, LH, 'core', LIB);
  apps.forEach(([id]) => link(d, id, 'B', 'lib', 'T'));
  d.node('api', 72, API_Y, 656, apiH, 'accent', API);
  d.node('cred', 72, RY, 200, credH, 'process', CRED);
  d.node('eng', 328, RY + 40, 184, engH, 'process', ENG);
  d.node('router', 568, RY, 160, rtH, 'accent', RT);
  d.note('local', 72, NY, 656, NOTE, noteH);

  link(d, 'lib', 'B', 'api', 'T', { label: 'HTTP, one key per app', at: 0.62 });
  link(d, 'api', 'B', 'cred', 'T');
  d.edge('api', 'eng', [472, API_Y + apiH], [472, RY + 40]); // right of the "Engine process" group label
  link(d, 'api', 'B', 'router', 'T');

  // MCP servers: read-only tools any MCP client may call (ADR 0007); Pitwall's tool router is one such client.
  const MX = 832, MW = 184;
  const servers = [
    ['code', CODEBASE_TOOL, 'MCP server, read-only', 'Code: dependency graph, review, local search.'],
    ['dbmcp', 'Debrief', 'MCP server, read-only', 'Logs and analysis; first, after Debrief 1.0.2.'],
    ['sigmcp', 'Sighter', 'MCP server; this PC or one over RDP', 'observe, locate, click, type, launch.'],
  ];
  const hs = servers.map(([, name, tech, desc]) => d.fit(MW, { name, tech, desc }));
  let y = PY + 48 + hs.reduce((a, h) => a + h + 16, 0);
  d.group('mcp', 808, PY, 232, y - 16 + 24 - PY, 'MCP servers');
  servers.reduce((sy, [id, name, tech, desc], k) => { app(d, id, MX, sy, MW, hs[k], { name, tech, desc }, true); return sy + hs[k] + 16; }, PY + 48);
  const CY = y - 16 + 24 + 64;
  const CL = { name: 'Other MCP clients', tech: 'editors, scripts', desc: 'Same servers, same tools.' };
  d.node('mcpc', MX, CY, MW, d.fit(MW, CL), 'external', CL);
  link(d, 'router', 'R', 'mcp', 'L', { label: 'MCP' });
  link(d, 'mcpc', 'T', 'mcp', 'B', { label: 'MCP' });

  d.legend([{ kind: 'accent', label: 'Pitwall' }, { kind: 'process', label: 'Process' }, { kind: 'core', label: 'Library' },
    { kind: 'external', label: 'Outside Paddock' }, { kind: 'tag', role: 'process', label: 'Not built yet' }, { kind: 'line', label: 'Request' }]);
  write(d, 'paddock-integrations', Math.max(PB, CY + d.rect.mcpc[3]));
}

// ---------------------------------------------------------------- hand-offs between apps
{
  const d = new Diagram({
    title: 'Paddock hand-offs', kind: 'Context · Files, deep links and MCP between apps', app: 'pitwall', date, w: W,
    desc: 'Each app does its own job alone; these are the places where they meet. Pitwall is optional for all of them.',
  });
  d.t.app = 'Paddock';
  const planned = (id) => /^(code|sighter)/.test(id);
  // [from, to, label, later]; left column first, then right. Every one is in the contract table of docs/ecosystem.md.
  const rows = [
    [['sighter1', 'Sighter'], ['debrief1', 'Debrief'], 'Run bundles'],
    [['pitcrew1', 'Pitcrew'], ['debrief2', 'Debrief'], 'Job logs'],
    [['dyno1', 'Dyno'], ['debrief3', 'Debrief'], 'Issues'],
    [['code1', CODEBASE_TOOL], ['debrief4', 'Debrief'], 'Log checks'],
    [['debrief5', 'Debrief'], ['code2', CODEBASE_TOOL], 'Trace → code'],
    [['debrief6', 'Debrief'], ['sighter2', 'Sighter'], 'Video repro (MCP)'],
    [['code3', CODEBASE_TOOL], ['sighter3', 'Sighter'], 'Test plans (MCP)'],
    [['sighter4', 'Sighter'], ['code4', CODEBASE_TOOL], 'Results'],
    [['dyno2', 'Dyno'], ['spotter1', 'Spotter'], 'Watch list'],
    [['dyno3', 'Dyno'], ['pitcrew2', 'Pitcrew'], 'Machine health'],
    [['spotter2', 'Spotter'], ['code5', CODEBASE_TOOL], 'Newer version'],
    [['pitcrew3', 'Pitcrew'], ['runners', 'Gitea runners'], 'Supervises'],
    [['spotter3', 'Spotter'], ['sighter5', 'Sighter'], 'Smoke tests', true],
  ];
  const BW = 168, BH = 48, GAP = 128, COL = 2 * BW + GAP + 80, PER = Math.ceil(rows.length / 2);
  rows.forEach(([[ia, na], [ib, nb], label, later], i) => {
    const x = 48 + Math.floor(i / PER) * COL, y = 144 + (i % PER) * (BH + 24);
    app(d, ia, x, y, BW, BH, { name: na }, planned(ia));
    if (ib === 'runners') d.node(ib, x + BW + GAP, y, BW, BH, 'external', { name: nb });
    else app(d, ib, x + BW + GAP, y, BW, BH, { name: nb }, planned(ib));
    link(d, ia, 'R', ib, 'L', { label, dash: !!later });
  });
  d.legend([{ kind: 'process', label: 'App' }, { kind: 'tag', role: 'process', label: 'Not built yet' }, { kind: 'external', label: 'Outside Paddock' },
    { kind: 'line', label: 'Hand-off, arrow shows the data' }, { kind: 'dashline', label: 'Later' }]);
  write(d, 'paddock-handoffs', 144 + PER * (BH + 24) - 24);
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
