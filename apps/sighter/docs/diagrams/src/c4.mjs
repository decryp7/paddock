import { P, save, link } from './common.mjs';

// ---------- C4 level 2: containers (planned) ----------
const d = P({ title: 'Sighter — containers', kind: 'C4 · Level 2 — Containers · planned', w: 1584,
  desc: 'Planned design, nothing is built yet. The studio and the recorder are for people; the locator server answers tests on loopback; training runs in a sandbox.' });
const NW = 208, NH = 128, EW = 216;
const lx = 48, gx = 360, cx = [384, 688, 992], rx = 1320;
const r0 = 144, g1 = r0 + NH + 64, row = (i) => g1 + 48 + i * (NH + 56), gh = 48 + NH * 2 + 56 + 24, gw = 24 + 3 * NW + 2 * 96 + 24;

d.node('cli', cx[2], r0, NW, NH, 'accent', { name: 'Client libraries', tech: 'C#, Java, C++, Python, TypeScript', desc: 'Playwright-style locators over the HTTP API; used by engineers\' tests.' });
d.node('pit', rx, r0, EW, NH, 'external', { name: 'Pitcrew runners', tech: 'CI runners', desc: 'Run the tests and `sighter run` on a machine with a desktop session.' });

d.group('g1', gx, g1, gw, gh, 'Sighter machine [one user session, loopback only]');
d.node('studio', cx[0], row(0), NW, NH, 'ui', { name: 'Studio app', tech: 'Tauri 2, Angular', desc: 'Label, review queue, test authoring, models. Pitwall may help label.' });
d.node('train', cx[1], row(0), NW, NH, 'accent', { name: 'Training runtime', tech: 'sandboxed process, downloaded on request', desc: 'Trains the permissive YOLO-style detector from reviewed labels.' });
d.node('srv', cx[2], row(0), NW, NH, 'accent', { name: 'Locator server', tech: 'Rust, HTTP API on loopback', desc: 'Layered resolver and guarded input driver; `sighter run`.' });
d.node('rec', cx[0], row(1), NW, NH, 'accent', { name: 'Recorder', tech: 'studio and headless CLI', desc: 'Screen and input events; accessibility optional; redacts before storing.' });
d.node('store', cx[1], row(1), 2 * NW + 96, NH, 'process', { name: 'Model store and data folder', tech: 'versioned, immutable', desc: 'Recordings, labels, ONNX models (app@N) and run bundles. Local files only.' });

d.node('pn', lx, row(0), EW, NH, 'external', { name: 'Pacenotes', tech: 'sibling app', desc: 'Test plans in, results and requirement traceability out.' });
d.node('app', lx, row(1), EW, NH, 'external', { name: 'App under test', tech: 'any UI', desc: 'Captured by sight, driven only through the guarded input driver.' });
d.node('pw', rx, row(0), EW, NH, 'external', { name: 'Pitwall', tech: 'local AI hub, optional', desc: 'Vision model for grounding, labelling help, recovery diagnosis.' });
d.node('dbf', rx, row(1), EW, NH, 'external', { name: 'Debrief', tech: 'sibling app', desc: 'Opens run bundles: video, steps, screenshots, recovery transcript.' });

link(d, 'pit', 'L', 'cli', 'R', { label: 'Runs tests' });
link(d, 'cli', 'B', 'srv', 'T', { label: 'HTTP calls', at: 0.2 });
link(d, 'pn', 'R', 'studio', 'L', { label: 'Test plans' });
link(d, 'studio', 'R', 'train', 'L', { label: 'Starts' });
link(d, 'studio', 'B', 'rec', 'T', { label: 'Controls' });
link(d, 'rec', 'L', 'app', 'R', { label: 'Captures', rev: false });
link(d, 'rec', 'R', 'store', 'L', { label: 'Writes' });
link(d, 'train', 'B', 'store', 'T', { label: 'Reads, writes', t: [0.5, 0.2031] });
link(d, 'srv', 'B', 'store', 'T', { label: 'Loads models', t: [0.5, 0.7969] });
link(d, 'srv', 'R', 'pw', 'L', { label: 'Grounding', dash: true });
link(d, 'store', 'R', 'dbf', 'L', { label: 'Run bundles' });
d.legend([{ kind: 'ui', label: 'Studio' }, { kind: 'accent', label: 'Sighter container' }, { kind: 'process', label: 'Data' }, { kind: 'external', label: 'External system' }, { kind: 'line', label: 'Uses' }, { kind: 'dashline', label: 'Optional' }]);
save('c4-2-containers', d, g1 + gh);
