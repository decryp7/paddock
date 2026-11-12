// The SVG drift check (scripts/svg-drift.mjs): a real change to a diagram is still seen, a no-op re-export and a different font are
// not. draw.io is replaced by a deterministic fake (a shim script that writes an SVG in draw.io's shape from the .drawio text), so
// this runs anywhere; the real draw.io is exercised by the CI job itself, whose ER-33 probe fails when the tool is missing or off pin.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, describe, test } from 'node:test';
import { committedSvg, differs, main, normalise, regenerate } from './svg-drift.mjs';

const scratch = mkdtempSync(join(tmpdir(), 'svg-drift-test-'));
after(() => rmSync(scratch, { recursive: true, force: true }));

const PIN = '1.2.3';
// The fake: `--version` prints FAKE_VERSION (default the pin); an export writes <name>.svg per .drawio of the input folder.
// FAKE_SHIFT moves the canvas like a wider font does and re-wraps the fallback text; draw.io's PNG label copy is always there.
const SHIM = `#!/usr/bin/env node
const fs = require('node:fs'), path = require('node:path');
const a = process.argv.slice(2);
if (a.includes('--version')) { console.log('[1:0:ERROR:dbus] noise'); console.log(process.env.FAKE_VERSION || '${PIN}'); process.exit(0); }
const out = a[a.indexOf('--output') + 1];
const src = a.filter((x) => !x.startsWith('--') && x !== 'false' && x !== 'svg' && x !== '16' && x !== out).pop();
const shift = Number(process.env.FAKE_SHIFT || 0);
fs.mkdirSync(out, { recursive: true });
for (const f of fs.readdirSync(src).filter((f) => f.endsWith('.drawio'))) {
  const x = fs.readFileSync(path.join(src, f), 'utf8');
  const label = (x.match(/value="([^"]*)"/) || [, ''])[1];
  const fill = (x.match(/fillColor=(#[0-9a-f]{3,6})/) || [, '#ffffff'])[1];
  const w = (x.match(/width="(\\d+)"/) || [, '100'])[1];
  const wrapped = shift ? label.replace(' ', '</tspan><tspan x="16" y="99">') : label;
  fs.writeFileSync(path.join(out, f.replace(/\\.drawio$/, '.svg')),
    '<svg width="' + (200 + shift) + 'px" height="90px" viewBox="0 0 ' + (200 + shift) + ' 90"><g data-cell-id="1"><rect x="' + (16 + shift) + '" y="16" width="' + w +
    '" height="40" fill="' + fill + '" stroke-width="1"/><switch><foreignObject><div>' + label + '</div></foreignObject>' +
    '<image x="0" y="0" xlink:href="data:image/png;base64,AAAA" width="10" height="10"/>' +
    '<text x="20" y="30"><tspan x="16" y="20">' + wrapped + '</tspan></text></switch></g></svg>');
}
`;

/** A tiny repository: one app with a diagram and its committed export, the pin, a regeneration script. */
function repo(name, { label = 'Hello world', fill = '#aabbcc', width = 100 } = {}) {
  const root = join(scratch, name);
  mkdirSync(join(root, 'apps/x/docs/diagrams/svg'), { recursive: true });
  mkdirSync(join(root, 'apps/x/scripts'), { recursive: true });
  mkdirSync(join(root, 'ci-image'), { recursive: true });
  writeFileSync(join(root, 'ci-image/Dockerfile'), `ARG DRAWIO_VERSION=${PIN}\n`);
  writeFileSync(join(root, 'apps/x/scripts/export-diagrams.sh'), '#!/bin/sh\n');
  const shim = join(scratch, 'drawio-shim');
  writeFileSync(shim, SHIM);
  chmodSync(shim, 0o755);
  writeDiagram(root, { label, fill, width });
  const svgDir = join(root, 'apps/x/docs/diagrams');
  const r = spawnSync(shim, ['--export', '--output', join(svgDir, 'svg'), svgDir, '--no-sandbox'], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  // What export-diagrams.sh commits: the same bytes minus the PNG copy of the label.
  const f = join(svgDir, 'svg/a.svg');
  writeFileSync(f, readFileSync(f, 'utf8').replace(/<image [^>]*\/>/, ''));
  for (const c of [['init', '-q'], ['add', '-A'], ['-c', 'user.name=t', '-c', 'user.email=t@t', 'commit', '-qm', 'x']]) {
    assert.equal(spawnSync('git', c, { cwd: root }).status, 0);
  }
  return { root, shim, svgDir };
}
const writeDiagram = (root, { label, fill, width }) =>
  writeFileSync(join(root, 'apps/x/docs/diagrams/a.drawio'), `<mxfile><mxCell value="${label}" style="fillColor=${fill}"/><mxGeometry width="${width}"/></mxfile>\n`);

/** Run the check as the CLI does and return its exit code and the lines it printed. */
function check(r, env = {}) {
  const lines = [];
  const log = console.log;
  console.log = (...a) => lines.push(a.join(' '));
  // The shim reads FAKE_* from the real environment, which the check's child processes inherit.
  const saved = Object.fromEntries(Object.keys(env).map((k) => [k, process.env[k]]));
  Object.assign(process.env, env);
  try {
    const code = main(['--root', r.root, '--drawio', r.shim], { DISPLAY: ':0' });
    return { code, out: lines.join('\n') };
  } finally {
    console.log = log;
    for (const [k, v] of Object.entries(saved)) if (v === undefined) delete process.env[k]; else process.env[k] = v;
  }
}

describe('the check on a repository', () => {
  test('a re-export that changed nothing is not drift', () => {
    const r = repo('noop');
    const { code, out } = check(r);
    assert.equal(code, 0, out);
    assert.match(out, /1 up to date, 0 drifted/);
  });

  test('a different font (canvas shifted, fallback text wrapped elsewhere) is not drift', () => {
    const r = repo('font');
    const { code, out } = check(r, { FAKE_SHIFT: '5' });
    assert.equal(code, 0, out);
  });

  test('a changed label in the .drawio is drift, named with the command that regenerates it', () => {
    const r = repo('label');
    writeDiagram(r.root, { label: 'Hello there', fill: '#aabbcc', width: 100 });
    const { code, out } = check(r);
    assert.equal(code, 1);
    assert.match(out, /::error file=apps\/x\/docs\/diagrams\/svg\/a\.svg::/);
    assert.match(out, /\(cd apps\/x && scripts\/export-diagrams\.sh docs\/diagrams docs\/diagrams a\)/);
    assert.match(out, /ER-40/);
  });

  test('a hand-patched colour in the committed SVG is drift', () => {
    const r = repo('colour');
    const f = join(r.svgDir, 'svg/a.svg');
    writeFileSync(f, readFileSync(f, 'utf8').replace('#aabbcc', '#aabbcd'));
    assert.equal(check(r).code, 1);
  });

  test('a hand-patched coordinate far beyond the font tolerance is drift', () => {
    const r = repo('coordinate');
    const f = join(r.svgDir, 'svg/a.svg');
    writeFileSync(f, readFileSync(f, 'utf8').replace('width="100"', 'width="140"'));
    assert.equal(check(r).code, 1);
  });

  test('a diagram whose export would be wider than the committed one is drift', () => {
    const r = repo('wider');
    writeDiagram(r.root, { label: 'Hello world', fill: '#aabbcc', width: 400 });
    assert.equal(check(r).code, 1);
  });

  test('a .drawio with no export, in a folder that keeps exports, is drift', () => {
    const r = repo('missing');
    writeFileSync(join(r.svgDir, 'b.drawio'), '<mxfile/>');
    spawnSync('git', ['add', '-A'], { cwd: r.root });
    const { code, out } = check(r);
    assert.equal(code, 1);
    assert.match(out, /::error file=apps\/x\/docs\/diagrams\/b\.drawio::.*no committed/);
  });

  test('ER-33: a tool on the wrong version, or no tool, never passes', () => {
    const r = repo('version');
    const wrong = check(r, { FAKE_VERSION: '9.9.9' });
    assert.equal(wrong.code, 1);
    assert.match(wrong.out, /is '9\.9\.9'.*the pin is 1\.2\.3/);
    const none = main(['--root', r.root, '--drawio', join(scratch, 'no-such-drawio')], { DISPLAY: ':0' });
    assert.equal(none, 1);
  });

  test('a source whose SVGs are nowhere is an error, "not measured", never a note', () => {
    const r = repo('nothing');
    spawnSync('git', ['rm', '-rq', 'apps/x/docs/diagrams/svg'], { cwd: r.root });
    const { code, out } = check(r);
    assert.equal(code, 1);
    assert.match(out, /::error file=apps\/x\/docs\/diagrams\/a\.drawio::.*not measured/);
    assert.match(out, /saw nothing/);
  });

  test('a folder that keeps its SVGs elsewhere is read from the declared table (the Sighter layout)', () => {
    const r = repo('sighter');
    const from = join(r.root, 'apps/x/docs/diagrams'), to = join(r.root, 'apps/sighter/docs');
    mkdirSync(join(to, 'images/diagrams'), { recursive: true });
    spawnSync('git', ['mv', from, join(to, 'diagrams')], { cwd: r.root });
    mkdirSync(join(to, 'images/diagrams'), { recursive: true });
    spawnSync('git', ['mv', join(to, 'diagrams/svg'), join(to, 'images/diagrams/svg')], { cwd: r.root });
    assert.equal(check(r).code, 0);
    writeFileSync(join(to, 'images/diagrams/svg/a.svg'), readFileSync(join(to, 'images/diagrams/svg/a.svg'), 'utf8').replace('#aabbcc', '#aabbcd'));
    assert.equal(check(r).code, 1);
  });

  test('the scratch export is removed', () => {
    const before = readdirSync(tmpdir()).filter((n) => n.startsWith('svg-drift-') && !n.startsWith('svg-drift-test-')).length;
    check(repo('clean'));
    const after = readdirSync(tmpdir()).filter((n) => n.startsWith('svg-drift-') && !n.startsWith('svg-drift-test-')).length;
    assert.equal(after, before);
  });
});

describe('normalising and comparing', () => {
  const svg = (inner) => `<svg width="100px"><switch><foreignObject>x</foreignObject>${inner}</switch></svg>`;

  test('the PNG copy is dropped and the fallback text keeps its words but not its wrapping', () => {
    const a = svg('<image xlink:href="data:image/png;base64,AAA" width="1"/><text x="1"><tspan>a b</tspan></text>');
    assert.equal(normalise(a), svg('<text>ab</text>'));
    const wrapped = svg('<text x="9" fill="#000"><tspan x="1" y="2">a</tspan><tspan x="1" y="14">b</tspan></text>');
    assert.equal(differs(normalise(a), normalise(wrapped)), null);
    assert.match(differs(normalise(a), normalise(svg('<text x="1"><tspan>a c</tspan></text>'))), /markup differs/);
  });

  test('the random root id of a new export is not a difference, a different id shape is', () => {
    const a = '<svg id="ge-svg-aB3_x-9"><style>#ge-svg-aB3_x-9 { a: b }</style></svg>';
    const b = '<svg id="ge-svg-Zz0-q_1"><style>#ge-svg-Zz0-q_1 { a: b }</style></svg>';
    assert.equal(differs(normalise(a), normalise(b)), null);
    assert.match(differs(normalise(a), normalise(b.replace('a: b', 'a: c'))), /markup differs/);
  });

  const box = (x, w, extra = '') => `<svg width="300px" height="90px" viewBox="0 0 300 90"><rect x="${x}" y="20" width="${w}" height="40" ${extra}/><rect x="${x + 150}" y="20" width="60" height="40"/></svg>`;
  const label = (size) => `<svg width="300px"><span style="font-size:${size}px">Save</span></svg>`;

  test('a font size is compared exactly: 12 to 24, and 12 to 13', () => {
    assert.match(differs(label(24), label(12)), /not an x coordinate/);
    assert.match(differs(label(13), label(12)), /not an x coordinate/);
    assert.equal(differs(label(12), label(12)), null);
  });

  test('a box resized by a few pixels is drift, even where a sideways shift of the whole drawing is allowed', () => {
    assert.match(differs(box(16, 114), box(16, 100)), /not an x coordinate/);
    assert.match(differs(box(16, 101), box(16, 100)), /not an x coordinate/);
    // the same drawing shifted by 5, with one box also 5 wider: still drift
    assert.match(differs(box(21, 105), box(16, 100)), /not an x coordinate/);
  });

  test('one box moved is drift: by 14 (over the tolerance), and by 5 while the others stay put', () => {
    assert.match(differs(box(30, 100), box(16, 100)), /more than 12|moved on its own/);
    const moved = '<svg width="300px"><rect x="21" y="1" width="9"/><rect x="100" y="1" width="9"/></svg>';
    const still = '<svg width="300px"><rect x="16" y="1" width="9"/><rect x="100" y="1" width="9"/></svg>';
    assert.match(differs(moved, still), /is 21 in the export and 16 .*moved on its own/);
  });

  test('the message names the coordinate that broke the shared shift, not one of the others', () => {
    const rects = (xs) => `<svg width="9px">${xs.map((x, i) => `<rect x="${x}" y="${i}"/>`).join('')}</svg>`;
    // a shifted drawing (every x +5) where the third rect did not move: name the third (x = 300)
    const m = differs(rects([21, 121, 300, 221]), rects([16, 116, 300, 216]));
    assert.match(m, /is 300 in the export and 300 in the committed.*differ by 5 px/);
    assert.doesNotMatch(m, /is 21 in the export|is 121 in the export/);
    // an unshifted drawing where only the third moved: name the third as well
    assert.match(differs(rects([16, 116, 305, 216]), rects([16, 116, 300, 216])), /is 305 in the export and 300 in the committed.*differ by 0 px/);
  });

  test('a y coordinate moved is drift, also in a shifted drawing', () => {
    const a = '<svg width="3px"><rect x="21" y="25" width="9"/></svg>';
    const b = '<svg width="3px"><rect x="16" y="20" width="9"/></svg>';
    assert.match(differs(a, b), /not an x coordinate/);
  });

  test('the host shift is allowed: every x coordinate by the same few pixels, root size within 12, nothing else', () => {
    const shifted = '<svg width="312px" height="90px" viewBox="0 0 312 90"><rect x="21" y="20" width="60"/><rect x="171" y="20" width="60"/></svg>';
    const base = '<svg width="300px" height="90px" viewBox="0 0 300 90"><rect x="16" y="20" width="60"/><rect x="166" y="20" width="60"/></svg>';
    assert.equal(differs(shifted, base), null);
    assert.match(differs(shifted.replace('312px', '313px'), base), /root size/);
    const far = '<svg width="300px" height="90px" viewBox="0 0 300 90"><rect x="29" y="20" width="60"/><rect x="179" y="20" width="60"/></svg>';
    assert.match(differs(far, base), /more than 12/);
  });

  test('path coordinates: x follows the shift, y and arc radii do not', () => {
    const path = (x, y) => `<svg width="9px"><path d="M ${x} ${y} L ${x + 40} ${y} Q ${x + 50} ${y} ${x + 50} ${y + 8} A 5 5 0 0 1 ${x + 60} ${y + 20}"/><rect x="${x}" y="1"/></svg>`;
    assert.equal(differs(path(21, 30), path(16, 30)), null);
    assert.match(differs(path(21, 31), path(16, 30)), /not an x coordinate/);
    assert.match(differs(path(16, 30).replace('A 5 5', 'A 6 5'), path(16, 30)), /not an x coordinate/);
  });

  test('a percentage position is not a coordinate', () => {
    assert.match(differs('<svg width="9px"><text x="50%"/><rect x="21"/></svg>', '<svg width="9px"><text x="40%"/><rect x="16"/></svg>'), /not an x coordinate/);
    assert.equal(differs('<svg width="9px"><text x="50%"/><rect x="21"/></svg>', '<svg width="9px"><text x="50%"/><rect x="16"/></svg>'), null);
  });

  test('colours are text, never numbers', () => {
    assert.match(differs('<rect fill="#112233"/>', '<rect fill="#112244"/>'), /markup differs/);
    assert.match(differs('<rect fill="rgb(10, 20, 30)"/>', '<rect fill="rgb(10, 20, 31)"/>'), /markup differs/);
  });

  test('a different element, text or order is a difference', () => {
    assert.match(differs('<g><rect/><path/></g>', '<g><path/><rect/></g>'), /markup differs/);
    assert.match(differs('<g>Save</g>', '<g>Safe</g>'), /markup differs/);
  });
});

describe('layout', () => {
  const files = new Set(['apps/x/docs/diagrams/a.drawio', 'apps/x/docs/diagrams/svg/a.svg', 'apps/x/scripts/export-diagrams.sh', 'diagrams/templates/t.drawio']);

  test('a source is paired with <dir>/svg/<name>.svg, a declared folder with its table entry, templates with nothing', () => {
    assert.equal(committedSvg('apps/x/docs/diagrams/a.drawio'), 'apps/x/docs/diagrams/svg/a.svg');
    assert.equal(committedSvg('docs/images/src/drawio/paddock-handoffs.drawio'), 'docs/images/paddock-handoffs.svg');
    assert.equal(committedSvg('apps/sighter/docs/diagrams/uml-seq-locate.drawio'), 'apps/sighter/docs/images/diagrams/svg/uml-seq-locate.svg');
    assert.equal(committedSvg('diagrams/templates/t.drawio'), null);
  });

  test('the regeneration command is the app copy of the export script when there is one', () => {
    assert.equal(regenerate('apps/x/docs/diagrams/a.drawio', files), '(cd apps/x && scripts/export-diagrams.sh docs/diagrams docs/diagrams a)');
    assert.equal(regenerate('docs/images/src/drawio/p.drawio', files), 'node docs/images/src/integrations.mjs');
    const sighter = new Set([...files, 'apps/sighter/scripts/export-diagrams.sh']);
    assert.equal(regenerate('apps/sighter/docs/diagrams/q.drawio', sighter), '(cd apps/sighter && scripts/export-diagrams.sh docs/diagrams docs/images/diagrams q)');
  });
});

