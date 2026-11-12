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

  test('a repository where no source has an export checked nothing, which is a failure', () => {
    const r = repo('nothing');
    spawnSync('git', ['rm', '-rq', 'apps/x/docs/diagrams/svg'], { cwd: r.root });
    const { code, out } = check(r);
    assert.equal(code, 1);
    assert.match(out, /saw nothing/);
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

  test('numbers under 8 (stroke width, opacity) must be equal, larger ones may differ by the tolerance', () => {
    assert.equal(differs('<rect width="120" stroke-width="1"/>', '<rect width="130" stroke-width="1"/>'), null);
    assert.match(differs('<rect width="120" stroke-width="1"/>', '<rect width="130" stroke-width="2"/>'), /number 2/);
    assert.match(differs('<rect width="120"/>', '<rect width="150"/>'), /number 1/);
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

  test('a source is paired with <dir>/svg/<name>.svg, the family drawings with docs/images, templates with nothing', () => {
    assert.equal(committedSvg('apps/x/docs/diagrams/a.drawio', files), 'apps/x/docs/diagrams/svg/a.svg');
    assert.equal(committedSvg('docs/images/src/drawio/paddock-handoffs.drawio', files), 'docs/images/paddock-handoffs.svg');
    assert.equal(committedSvg('diagrams/templates/t.drawio', files), null);
  });

  test('the regeneration command is the app copy of the export script when there is one', () => {
    assert.equal(regenerate('apps/x/docs/diagrams/a.drawio', files), '(cd apps/x && scripts/export-diagrams.sh docs/diagrams docs/diagrams a)');
    assert.equal(regenerate('docs/images/src/drawio/p.drawio', files), 'node docs/images/src/integrations.mjs');
  });
});

