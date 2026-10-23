// Generates docs/images/paddock-integrations.svg (+ light/dark 2x PNGs with --png) with the Paddock diagram kit and draw.io.
//   node docs/images/src/integrations.mjs [--png]      (draw.io desktop: set DRAWIO to override the path)
import { readFileSync, writeFileSync, mkdirSync, copyFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { Diagram, link } from '../../../diagrams/tools/lib.mjs';
import { CODEBASE_TOOL } from './family.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, '..');
const date = new Date().toISOString().slice(0, 10);
const d = new Diagram({
  title: 'Paddock integrations', kind: 'Deployment · Processes and protocols', app: 'pitwall', date, w: 1520,
  desc: 'Pitwall is the optional local AI hub; everything else is a file, a deep link or CI. Nothing leaves the machine.',
});
d.t.app = 'Paddock';

// Apps that call Pitwall (left), Pitwall (centre), MCP server (right).
d.group('apps', 48, 144, 312, 608, 'Apps on this machine');
const apps = [['debrief', 'Debrief', 'Tauri app', 'Explain, research, summaries.'], ['spotter', 'Spotter', 'Tauri app', 'What’s-new summaries.'],
  ['dyno', 'Dyno', 'Tauri app and agent', 'Diagnosis in plain words.'], ['sighter', 'Sighter', 'Tauri app and server', 'Vision for finding UI elements.']];
apps.forEach(([id, name, tech, desc], i) => d.node(id, 72, 192 + i * 112, 264, 88, 'process', { name, tech, desc, planned: id === 'sighter' }));
d.node('lib', 72, 640, 264, 88, 'core', { name: 'paddock-pitwall', tech: 'Rust crate in each app', desc: 'Discovery, pairing, keys, retries.', planned: true });

d.group('pw', 472, 144, 648, 608, 'Pitwall: one service process, loopback only');
d.node('api', 496, 192, 272, 424, 'accent', { name: 'Local API', tech: 'HTTP on 127.0.0.1',
  desc: 'OpenAI, Anthropic and Ollama compatible, plus /x/v1. One key per app, job priorities, prompt and response caching, usage and limits per app.' });
d.node('router', 824, 192, 272, 88, 'accent', { name: 'Tool router', tech: 'MCP client', desc: 'Offers MCP tools to local models.' });
d.group('os', 800, 336, 320, 168, 'Engine process', { os: true });
d.node('eng', 824, 384, 272, 88, 'process', { name: 'llama.cpp', tech: 'child process, loopback', desc: 'Runs the model on your hardware.' });
d.node('cred', 824, 536, 272, 88, 'process', { name: 'OS credential store', tech: 'Keychain, Credential Manager, Secret Service', desc: 'Holds the per-app keys.' });
d.note('local', 496, 664, 600, 'Local only: the sole outbound traffic is the model and engine downloads you ask for.');

d.node('code', 1232, 192, 248, null, 'process', { name: `${CODEBASE_TOOL}`, tech: 'MCP server, read-only', desc: 'Symbols, references, repo map, stack-trace explanation.', planned: true });
d.node('mcpc', 1232, 392, 248, 88, 'external', { name: 'Other MCP clients', tech: 'editors, scripts', desc: 'Same server, same tools.' });

link(d, 'debrief', 'R', 'api', 'L', { label: 'HTTP and key' });
['spotter', 'dyno', 'sighter'].forEach((a) => link(d, a, 'R', 'api', 'L'));
link(d, 'api', 'R', 'router', 'L');
link(d, 'api', 'R', 'eng', 'L');
link(d, 'api', 'R', 'cred', 'L');
link(d, 'router', 'R', 'code', 'L', { label: 'MCP' });
link(d, 'mcpc', 'T', 'code', 'B', { label: 'MCP', arrow: 'block' });

// Hand-offs that do not involve Pitwall.
d.group('ho', 48, 808, 1432, 272, 'Hand-offs between apps (none of them needs Pitwall)');
const rows = [
  [['sighter2', 'Sighter'], ['debrief2', 'Debrief'], 'Run bundle: video, steps'],
  [[ 'code2', CODEBASE_TOOL], ['sighter3', 'Sighter'], 'Test plan, results back'],
  [['pitcrew', 'Pitcrew'], ['debrief3', 'Debrief'], 'Deep link: open job log'],
  [['dyno2', 'Dyno'], ['debrief4', 'Debrief'], 'Deep link: open issue'],
  [['dyno3', 'Dyno'], ['spotter2', 'Spotter'], 'Deep link: watch for updates'],
  [['pitcrew2', 'Pitcrew'], ['runners', 'Gitea runners'], 'Supervises, keeps updated'],
];
rows.forEach(([[ia, na], [ib, nb], label], i) => {
  const x = 72 + (i % 2) * 720, y = 856 + Math.floor(i / 2) * 72;
  d.node(ia, x, y, 184, 48, ia.startsWith('code') || ia.startsWith('sighter') ? 'process' : 'process', { name: na, planned: ia.startsWith('code') || ia.startsWith('sighter') });
  d.node(ib, x + 400, y, 184, 48, ib === 'runners' ? 'external' : 'process', { name: nb, });
  link(d, ia, 'R', ib, 'L', { label, dash: true });
});
d.legend([{ kind: 'accent', label: 'Pitwall' }, { kind: 'process', label: 'App or process' }, { kind: 'core', label: 'Shared library' },
  { kind: 'external', label: 'Outside Paddock' }, { kind: 'planned', label: 'Not built yet' }, { kind: 'line', label: 'Request' }, { kind: 'dashline', label: 'Hand-off' }]);
const src = join(here, 'drawio');
mkdirSync(src, { recursive: true });
writeFileSync(join(src, 'paddock-integrations.drawio'), d.finish(1080));
console.log('wrote paddock-integrations.drawio');

const drawio = process.env.DRAWIO || '/Applications/draw.io.app/Contents/MacOS/draw.io';
const tmp = join(process.env.TMPDIR || '/tmp', 'paddock-int');
execFileSync(join(here, '../../../diagrams/scripts/export-diagrams.sh'), [src, tmp, 'paddock-integrations'], { env: { ...process.env, DRAWIO: drawio }, stdio: 'inherit' });
copyFileSync(join(tmp, 'svg', 'paddock-integrations.svg'), join(out, 'paddock-integrations.svg'));
if (process.argv.includes('--png')) {
  const chrome = process.env.CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
  const m = readFileSync(join(out, 'paddock-integrations.svg'), 'utf8').match(/width="(\d+)px" height="(\d+)px"/);
  const [W, H] = [Number(m[1]), Number(m[2])];
  for (const [theme, bg] of [['light', '#ffffff'], ['dark', '#0d1117']]) {
    const html = join(tmp, `${theme}.html`);
    writeFileSync(html, `<!doctype html><meta name="color-scheme" content="${theme}"><style>html,body{margin:0;background:${bg};color-scheme:${theme}}img{display:block}</style><img src="file://${join(out, 'paddock-integrations.svg')}" width="${W}" height="${H}">`);
    execFileSync(chrome, ['--headless=new', '--disable-gpu', '--hide-scrollbars', '--force-device-scale-factor=2', `--force-dark-mode=${theme === 'dark'}`,
      `--screenshot=${join(out, `paddock-integrations-${theme}.png`)}`, `--window-size=${W},${H}`, `file://${html}`], { stdio: 'ignore' });
    console.log(`wrote paddock-integrations-${theme}.png`);
  }
}
