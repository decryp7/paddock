// Builds the draw.io shape library (paddock.xml) and the starting templates (templates/*.drawio) from lib.mjs.
//   node diagrams/tools/build-kit.mjs
import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Diagram, link, g8 } from './lib.mjs';
import { sequence, dot, cls } from './uml.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const DATE = '2026-10-22';
const D = (o) => new Diagram({ app: 'kit', date: DATE, ...o });
const out = [];

// ---------------- templates ----------------
function context() {
  const d = D({ title: 'System name — system context', kind: 'C4 · Level 1 — System context', desc: 'Who uses the system and which outside systems it talks to. One sentence, then the picture.', w: 960 });
  d.node('p', 96, 144, 264, null, 'person', { name: 'Person', tech: 'Person', desc: 'What this person does with the system.', person: true });
  const e1 = { name: 'External system', tech: 'External system', desc: 'What it gives the system.' };
  const eh = d.fit(264, e1);
  const sy = g8(144 + d.rect.p[3] + 80);
  d.node('s', 96, sy, 264, eh * 2 + 40, 'accent', { name: 'System in scope', tech: 'Software system', desc: 'What it does for its users, in one or two lines.' });
  d.node('x1', 600, sy, 264, eh, 'external', e1);
  d.node('x2', 600, sy + eh + 40, 264, eh, 'external', { ...e1, name: 'Another system' });
  link(d, 'p', 'B', 's', 'T', { label: 'Uses' });
  link(d, 's', 'R', 'x1', 'L', { label: 'Calls API', t: [(eh / 2) / (eh * 2 + 40), 0.5] });
  link(d, 's', 'R', 'x2', 'L', { label: 'Reads files', t: [(eh * 1.5 + 40) / (eh * 2 + 40), 0.5] });
  d.legend([{ kind: 'process', label: 'Person' }, { kind: 'accent', label: 'System in scope' }, { kind: 'external', label: 'External system' }, { kind: 'line', label: 'Uses' }]);
  return d.finish(sy + eh * 2 + 40);
}
function containers() {
  const d = D({ title: 'System name — containers', kind: 'C4 · Level 2 — Containers', desc: 'The runnable units and how they talk. Technology goes in the brackets, protocols in the node text.', w: 1280 });
  d.node('p', 48, 144, 264, null, 'person', { name: 'Person', tech: 'Person', desc: 'Uses the window.', person: true });
  const g1 = 144 + d.rect.p[3] + 56;
  const n = { name: 'Window', tech: 'Web UI', desc: 'Shows state, sends commands.' };
  const nh = 88;
  d.group('g1', 48, g1, 728, nh + 80, 'App process');
  d.node('ui', 72, g1 + 48, 264, nh, 'ui', n);
  d.node('sh', 488, g1 + 48, 264, nh, 'accent', { name: 'Shell', tech: 'Rust', desc: 'Validates every argument, then calls the service.' });
  const g2 = g1 + nh + 80 + 80;
  d.group('g2', 48, g2, 728, nh + 80, 'Service process', { os: true });
  d.node('co', 72, g2 + 48, 264, nh, 'core', { name: 'Core library', tech: 'Rust, no I/O', desc: 'Rules as pure functions.' });
  d.node('sv', 488, g2 + 48, 264, nh, 'accent', { name: 'Service', tech: 'Rust', desc: 'Owns the work; one thread per runner.' });
  d.node('x1', 936, g2 + 48, 296, nh, 'external', { name: 'External system', tech: 'HTTPS', desc: 'Who or what the service calls.' });
  link(d, 'p', 'B', 'ui', 'T', { label: 'Uses', at: 0.35 });
  link(d, 'ui', 'R', 'sh', 'L', { label: 'Commands' });
  link(d, 'sh', 'B', 'sv', 'T', { label: 'Requests' });
  link(d, 'sv', 'L', 'co', 'R', { label: 'Decisions' });
  link(d, 'sv', 'R', 'x1', 'L', { label: 'Fetches' });
  d.legend([{ kind: 'ui', label: 'Window' }, { kind: 'accent', label: 'Our container' }, { kind: 'core', label: 'Core library' }, { kind: 'external', label: 'External system' }, { kind: 'line', label: 'Uses' }]);
  return d.finish(g2 + nh + 80);
}
function seq() {
  const d = D({ title: 'Interaction name', kind: 'UML · Sequence diagram', desc: 'One scenario, left to right by who starts it. At most 8 participants and 25 messages; split at a natural pause.', w: 960 });
  const s = sequence(d, [
    { id: 'a', name: 'Window', tech: 'UI', role: 'ui' },
    { id: 'b', name: 'Service', tech: 'Rust', role: 'accent' },
    { id: 'c', name: 'Core', tech: 'Library', role: 'core' },
    { id: 'e', name: 'Server', tech: 'External', role: 'external' },
  ]);
  s.msg('a', 'b', 'Request');
  s.msg('b', 'c', 'Decide');
  s.msg('c', 'b', 'Action', { reply: true });
  s.open('opt', '[server needed]');
  s.msg('b', 'e', 'Fetch');
  s.msg('e', 'b', 'Response', { reply: true });
  s.close();
  s.self('b', 'Record the outcome');
  s.msg('b', 'a', 'Reply', { reply: true });
  const y = s.done();
  d.legend([{ kind: 'line', label: 'Call' }, { kind: 'dashline', label: 'Reply' }]);
  return d.finish(y);
}
function state() {
  const d = D({ title: 'Machine name', kind: 'UML · State machine', desc: 'Events and guards on the transitions, entry actions in the state. One path per outcome.', w: 960 });
  dot(d, 's0', 48, 232, 'start');
  d.node('idle', 144, 200, 192, 80, 'accent', { name: 'Idle', desc: 'Waiting for work.' });
  d.node('run', 456, 200, 192, 80, 'accent', { name: 'Running', desc: 'entry: start the work.' });
  dot(d, 'ch', 768, 224, 'choice');
  d.node('ok', 456, 376, 192, 80, 'accent', { name: 'Done', desc: 'The work finished.' });
  dot(d, 'end', 540, 528, 'end');
  link(d, 's0', 'R', 'idle', 'L');
  link(d, 'idle', 'R', 'run', 'L', { label: 'Start' });
  link(d, 'run', 'R', 'ch', 'L', { label: 'exits' });
  d.edge('ch', 'ok', d.port('ch', 'B'), d.port('ok', 'R'), { via: [[784, 416]] });
  d.lbl('[success]', 784, 336, 'r');
  d.edge('ch', 'idle', d.port('ch', 'T'), d.port('idle', 'T'), { via: [[784, 160], [240, 160]] });
  d.lbl('[failure] retry', 512, 160, 'a');
  link(d, 'ok', 'B', 'end', 'T');
  d.legend([{ kind: 'accent', label: 'State' }, { kind: 'line', label: 'Event [guard]' }]);
  return d.finish(552);
}
function klass() {
  const d = D({ title: 'Domain model', kind: 'UML · Class diagram', desc: 'Types and how they relate. Members in monospace; keep each box to what the reader needs.', w: 960 });
  cls(d, 'a', 48, 144, 280, { name: 'Config', attrs: ['version: u32', 'items: Vec<Item>'], ops: ['load(path): Config'] });
  cls(d, 'b', 48, 392, 280, { name: 'Item', attrs: ['name: Name', 'enabled: bool'] });
  cls(d, 'c', 648, 144, 264, { name: 'Status', stereotype: 'enumeration', attrs: ['Idle', 'Busy', 'Failed'], role: 'core' });
  link(d, 'a', 'B', 'b', 'T', { start: 'diamondThin', arrow: 'none', label: '0..*' });
  link(d, 'a', 'R', 'c', 'L', { dash: true, arrow: 'open', label: 'uses', t: [0.3, 0.3] });
  d.legend([{ kind: 'accent', label: 'Our type' }, { kind: 'core', label: 'Enumeration' }, { kind: 'line', label: 'Association' }, { kind: 'dashline', label: 'Dependency' }]);
  return d.finish(d.rect.b[1] + d.rect.b[3]);
}
function deployment() {
  const d = D({ title: 'Deployment — machine type', kind: 'UML · Deployment diagram', desc: 'Where each part runs on one machine, and what it talks to outside. One diagram per operating system.', w: 1280 });
  const nh = 88;
  d.group('m', 48, 144, 816, 400, 'Build machine [OS name]');
  d.node('app', 72, 192, 280, nh, 'accent', { name: 'App', tech: 'binary: runs as the user', desc: 'Window and tray.' });
  d.node('cred', 480, 192, 360, nh, 'process', { name: 'Credential store', tech: 'OS', desc: 'Holds tokens.' });
  d.group('os', 72, 352, 768, nh + 80, 'Service process', { os: true });
  d.node('svc', 96, 400, 280, nh, 'accent', { name: 'Service', tech: 'binary: starts at boot', desc: 'Owns the work.' });
  d.node('data', 504, 400, 312, nh, 'process', { name: 'Data folder', tech: 'path', desc: 'Config, state, logs.' });
  d.node('x', 984, 400, 248, nh, 'external', { name: 'External system', tech: 'HTTPS', desc: 'Who the service calls.' });
  link(d, 'app', 'B', 'svc', 'T', { label: 'Local socket', at: 0.3 });
  link(d, 'app', 'R', 'cred', 'L', { label: 'Stores' });
  link(d, 'svc', 'R', 'data', 'L', { label: 'Reads, writes' });
  link(d, 'data', 'R', 'x', 'L', { label: 'Fetches' });
  d.legend([{ kind: 'accent', label: 'Our artefact' }, { kind: 'process', label: 'OS or data' }, { kind: 'external', label: 'External' }, { kind: 'line', label: 'Communication' }]);
  return d.finish(544);
}
function flow() {
  const d = D({ title: 'Process name', kind: 'Flow · Flowchart', desc: 'Decisions as diamonds, one outcome per exit; keep to one direction.', w: 960 });
  dot(d, 's', 232, 160, 'start');
  d.node('a', 144, 224, 192, 80, 'accent', { name: 'Step one', desc: 'What happens.' });
  dot(d, 'q', 224, 344, 'choice');
  d.node('b', 464, 320, 192, 80, 'accent', { name: 'Step two', desc: 'When yes.' });
  d.node('c', 144, 456, 192, 80, 'accent', { name: 'Step three', desc: 'When no.' });
  dot(d, 'e', 784, 348, 'end');
  link(d, 's', 'B', 'a', 'T');
  link(d, 'a', 'B', 'q', 'T');
  link(d, 'q', 'R', 'b', 'L', { label: 'yes' });
  link(d, 'q', 'B', 'c', 'T', { label: 'no' });
  link(d, 'b', 'R', 'e', 'L');
  d.legend([{ kind: 'accent', label: 'Step' }, { kind: 'line', label: 'Flow' }]);
  return d.finish(536);
}
const templates = { 'c4-context': context, 'c4-containers': containers, sequence: seq, state, class: klass, deployment, flow };
mkdirSync(join(root, 'templates'), { recursive: true });
for (const [n, f] of Object.entries(templates)) writeFileSync(join(root, 'templates', `${n}.drawio`), f());

// ---------------- shape library ----------------
const lib = [];
const add = (title, w, h, build) => {
  const d = D({ bare: true });
  build(d);
  lib.push({ xml: d.fragment(), w, h, aspect: 'fixed', title });
};
const sample = { name: 'Name', tech: 'Technology', desc: 'What it does, in one line.' };
add('Person', 240, 96, (d) => d.node('x', 0, 0, 240, 96, 'process', { ...sample, tech: 'Person', person: true }));
add('Own container (accent)', 240, 96, (d) => d.node('x', 0, 0, 240, 96, 'accent', sample));
add('Core library', 240, 96, (d) => d.node('x', 0, 0, 240, 96, 'core', sample));
add('UI (window, webview)', 240, 96, (d) => d.node('x', 0, 0, 240, 96, 'ui', sample));
add('Plugin or extension', 240, 96, (d) => d.node('x', 0, 0, 240, 96, 'plugin', sample));
add('Separate process', 240, 96, (d) => d.node('x', 0, 0, 240, 96, 'process', sample));
add('External system (dashed)', 240, 96, (d) => d.node('x', 0, 0, 240, 96, 'external', sample));
add('Planned container', 240, 96, (d) => d.node('x', 0, 0, 240, 96, 'accent', { ...sample, planned: true }));
add('Group or boundary', 480, 240, (d) => d.group('x', 0, 0, 480, 240, 'Boundary'));
add('OS process boundary (dashed)', 480, 240, (d) => d.group('x', 0, 0, 480, 240, 'Process', { os: true }));
add('Note', 240, 56, (d) => d.note('x', 0, 0, 240, 'A note that explains one thing.'));
add('Planned tag', 64, 18, (d) => d.tag(64, 0, 'planned'));
add('Legend and footer', 640, 40, (d) => { d.legend([{ kind: 'accent', label: 'Own container' }, { kind: 'external', label: 'External' }, { kind: 'line', label: 'Uses' }]); d.finish(-40); });
add('Title block', 640, 80, (d) => {
  d.text('t1', [0, 0, 640, 32], 'Diagram title', { size: 24, weight: 600 });
  d.text('t2', [0, 32, 640, 24], 'C4 · Level 2 — Containers', { size: 14, color: 'muted' });
  d.text('t3', [0, 56, 640, 20], 'One sentence about what this diagram shows.', { size: 12, color: 'muted' });
});
add('Sequence participant', 160, 56, (d) => d.node('x', 0, 0, 160, 56, 'accent', { name: 'Participant', tech: 'Tech' }));
add('Lifeline', 8, 240, (d) => d.line([4, 0], [4, 240], { dash: true, arrow: 'none', width: 1 }));
add('Activation', 12, 64, (d) => d.raw(`<mxCell id="act" value="" style="rounded=0;strokeWidth=1.5;strokeColor=light-dark(${d.t.accent.s[0]},${d.t.accent.s[1]});fillColor=light-dark(${d.t.accent.f[0]},${d.t.accent.f[1]});" vertex="1" parent="1"><mxGeometry width="12" height="64" as="geometry"/></mxCell>`));
add('State', 200, 80, (d) => d.node('x', 0, 0, 200, 80, 'accent', { name: 'State', desc: 'entry: action' }));
add('Start', 16, 16, (d) => dot(d, 'x', 0, 0, 'start'));
add('End', 24, 24, (d) => dot(d, 'x', 0, 0, 'end'));
add('Choice', 32, 32, (d) => dot(d, 'x', 0, 0, 'choice'));
add('Class box', 280, 128, (d) => cls(d, 'x', 0, 0, 280, { name: 'ClassName', attrs: ['field: Type', 'other: Type'], ops: ['method(arg): Type'] }));
add('Edge (orthogonal, rounded)', 120, 8, (d) => d.line([0, 4], [120, 4]));
add('Edge (reply, dashed)', 120, 8, (d) => d.line([0, 4], [120, 4], { dash: true, arrow: 'open' }));
writeFileSync(join(root, 'paddock.xml'), `<mxlibrary>${JSON.stringify(lib, null, 1).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')}</mxlibrary>\n`);
console.log(`wrote ${Object.keys(templates).length} templates and ${lib.length} library shapes`);
