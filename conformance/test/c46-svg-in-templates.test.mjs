import assert from 'node:assert/strict';
import { test } from 'node:test';
import { inlineTemplates, svgElements } from '../src/checks/C46-svg-in-templates.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const ICON = '<svg viewBox="0 0 24 24"><path d="M1 1h2"/></svg>';
const component = (template, head = '') => `${head}@Component({\n  selector: 'x-y',\n  template: \`${template}\`,\n})\nexport class Y {}\n`;

test('C46 bad: an <svg> in an html template or a component template, with attributes or bound ones', () => {
  assert.deepEqual(svgElements('a.html', `<button>\n  ${ICON}\n</button>\n`).map((s) => [s.line, s.waived]), [[2, false]]);
  assert.deepEqual(svgElements('a.ts', component(`<button>${ICON}</button>`)).map((s) => [s.line, s.waived]), [[3, false]]);
  assert.equal(svgElements('a.ts', component('<svg [attr.viewBox]="v()"></svg>')).length, 1);
  assert.equal(svgElements('a.ts', component('<svg\n class="i"></svg>')).length, 1);
  assert.equal(svgElements('a.ts', "@Component({ template: '<svg></svg>' }) class A {}").length, 1);
});

test('C46 good: no svg, svg-like names, and svg strings outside a template', () => {
  assert.deepEqual(svgElements('a.ts', component('<mat-icon svgIcon="pd:refresh" /><svg-chart></svg-chart>')), []);
  // An icon literal builder is not a template.
  assert.deepEqual(svgElements('outline.ts', `export const s = \`${ICON}\`;\nconst t = '<svg>';\n`), []);
  assert.deepEqual(svgElements('a.html', '<p>svg is a word</p>'), []);
});

test('C46 waivers: a marker on the line or the line above, in the decorator head, with a reason', () => {
  assert.equal(svgElements('a.html', `<!-- conformance: svg-ok — a sparkline chart -->\n${ICON}`)[0].waived, 'reason');
  assert.equal(svgElements('a.html', `<!-- conformance: svg-ok — a sparkline chart -->${ICON}`)[0].waived, 'reason');
  assert.equal(svgElements('a.ts', component(`<!-- conformance: svg-ok — an illustration -->${ICON}`))[0].waived, 'reason');
  assert.equal(svgElements('a.ts', component(`<p></p>\n<!-- conformance: svg-ok — a diagram -->\n${ICON}`))[0].waived, 'reason');
  assert.equal(svgElements('a.ts', component(ICON, '// conformance: svg-ok — the whole template is a chart\n'))[0].waived, 'reason');
  assert.equal(svgElements('a.ts', component(`<!-- conformance: svg-ok -->${ICON}`))[0].waived, 'no-reason');
  // Two lines away does not count.
  assert.equal(svgElements('a.html', `<!-- conformance: svg-ok — a chart -->\n<p></p>\n${ICON}`)[0].waived, false);
});

test('inlineTemplates: reads the template of each component and honours escapes', () => {
  const src = component('<a>`x`</a>').replace('`<a>`x`</a>`', '`<a>\\`x\\`</a>`') + component('<b></b>');
  assert.equal(inlineTemplates(src).length, 2);
  assert.match(inlineTemplates(src)[0].text, /<a>/);
});

test('C46 through the runner: errors in ui/projects and apps/*/app/src, none for charts, specs, other folders or waived paths', async () => {
  const root = makeApp({
    'ui/projects/lib/src/bad.ts': component(`<button>${ICON}</button>`),
    'ui/projects/lib/src/chart.ts': component(`<!-- conformance: svg-ok — a bar chart -->${ICON}`),
    'ui/projects/lib/src/bad.spec.ts': component(ICON),
    'ui/projects/lib/src/good.ts': component('<mat-icon svgIcon="pd:refresh" />'),
    'ui/projects/lib/public/ghost.html': ICON,
    'apps/one/app/src/app/bad.html': ICON,
    'apps/one/app/src/app/bare.ts': component(`<!-- conformance: svg-ok -->${ICON}`),
    'apps/one/crates/x/templates/ok.html': ICON,
    'docs/page.html': ICON,
    'ui/e2e/x.ts': component(ICON),
  });
  const f = await findingsOf('C46', root, { app: 'paddock' });
  assert.deepEqual(
    f.map((x) => `${x.file}:${x.line}`).sort(),
    ['apps/one/app/src/app/bad.html:1', 'apps/one/app/src/app/bare.ts:3', 'ui/projects/lib/src/bad.ts:3'],
  );
  assert.match(f.find((x) => x.file.endsWith('bare.ts')).message, /no reason/);
  assert.ok(f.every((x) => x.severity === 'error' && x.rule === 47));
});

test('C46 dated waiver: a waived app passes until the date, then fails', async () => {
  const files = {
    'apps/old/app/src/a.html': ICON,
    'paddock.waivers.toml': '[[waiver]]\ncheck = "C46"\npath = "apps/old/*"\nreason = "migrates in its own branch"\nexpires = "2026-12-31"\n',
  };
  const root = makeApp(files);
  const before = await findingsOf('C46', root, { app: 'paddock', options: { now: new Date('2026-11-01') } });
  assert.deepEqual(before, []);
  const after = await findingsOf('C46', root, { app: 'paddock', options: { now: new Date('2027-01-02') } });
  assert.ok(after.length >= 1);
});

test('C46 bad: an svg built in code with createElementNS; a marker above it waives', () => {
  const code = "const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');\n";
  assert.deepEqual(svgElements('dock.ts', code).map((s) => [s.line, s.waived]), [[1, false]]);
  assert.equal(svgElements('dock.ts', `// conformance: svg-ok — a diagram\n${code}`)[0].waived, 'reason');
  assert.deepEqual(svgElements('dock.ts', "document.createElementNS(ns, 'path');"), []);
});

test('C46 tightened: commented svg ignored, <svg:svg> caught, decorator marker covers one svg only, src/ only', async () => {
  assert.deepEqual(svgElements('a.html', `<!-- ${ICON} -->\n<p></p>`), []);
  assert.equal(svgElements('a.html', '<svg:svg viewBox="0 0 1 1"></svg:svg>').length, 1);
  const head = '// conformance: svg-ok — one chart\n';
  assert.equal(svgElements('a.ts', component(ICON, head))[0].waived, 'reason');
  assert.deepEqual(svgElements('a.ts', component(`${ICON}\n${ICON}`, head)).map((s) => s.waived), [false, false]);
  const root = makeApp({ 'ui/projects/lib/public/ghost.html': ICON, 'ui/projects/lib/src/in.html': ICON });
  const f = await findingsOf('C46', root, { app: 'paddock' });
  assert.deepEqual(f.map((x) => x.file), ['ui/projects/lib/src/in.html']);
});
