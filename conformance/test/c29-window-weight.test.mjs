import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { findingsOf, makeApp, messages } from './helpers.mjs';
import { main } from '../src/cli.mjs';
import { budgetKb, chunkOf, indexOutputs, measure, packageOf, report, staticClosure, windowEntries } from '../src/window-weight/measure.mjs';

const out = (entryPoint, bytes, imports = [], inputs = {}) => ({ entryPoint, bytes, imports, inputs });
const imp = (path, kind = 'import-statement') => ({ path, kind });
const KB = 1024;
const outputs = {
  'dist/main.js': out('src/main.ts', 100 * KB, [imp('chunk-a.js'), imp('bootstrap.js', 'dynamic-import')], { 'src/main.ts': { bytesInOutput: 100 * KB } }),
  'dist/bootstrap.js': out('src/bootstrap.ts', 200 * KB, [imp('chunk-a.js')], { 'src/bootstrap.ts': { bytesInOutput: 200 * KB } }),
  'dist/app.js': out('src/app/app.ts', 300 * KB, [imp('chunk-b.js'), imp('lazy.js', 'dynamic-import')]),
  'dist/config.js': out('src/app/app.config.ts', 400 * KB),
  'dist/popout.js': out('src/app/popout/popout-shell.ts', 50 * KB),
  'dist/shell.js': out('src/app/dialog-windows/dialog-shell.ts', 10 * KB),
  'dist/dbs.js': out('src/app/dialog-windows/dialog-bootstrap.ts', 10 * KB),
  'dist/about.js': out('src/app/about/about-dialog.ts', 20 * KB),
  'dist/chunk-a.js': { bytes: 10 * KB, imports: [], inputs: { 'node_modules/@angular/core/x.mjs': { bytesInOutput: 10 * KB } } },
  'dist/chunk-b.js': { bytes: 20 * KB, imports: [], inputs: { 'src/app/dock/dock.ts': { bytesInOutput: 20 * KB } } },
  'dist/lazy.js': { bytes: 5000 * KB, imports: [], inputs: {} },
};
const MAIN_KB = 100 + 200 + 300 + 400 + 10 + 20; // 1030

const app = (windows, extra = {}) =>
  makeApp({
    'paddock.conformance.json': JSON.stringify({ windowWeight: { windows } }),
    'app/dist/app/browser-stats.json': JSON.stringify({ outputs }),
    ...extra,
  });

test('measure: static imports count, dynamic ones do not; popout and dialogs have their own entries', () => {
  const m = measure(outputs, 'main');
  assert.equal(m.total, MAIN_KB * KB);
  assert.ok(!m.files.has('lazy.js'));
  assert.equal(measure(outputs, 'popout').total, (100 + 200 + 50 + 400 + 10) * KB);
  assert.equal(measure(outputs, 'about').total, (100 + 200 + 10 + 10 + 20 + 10) * KB);
  assert.equal(measure(outputs, 'missing'), null, 'a window with no chunk is not silently zero');
  assert.deepEqual(windowEntries('x', { entries: ['a.ts'] }), ['a.ts']);
  assert.equal(measure(outputs, 'about', { entries: ['src/main.ts', 'src/nope.ts'], optional: 1 }).total, (100 + 10) * KB, 'optional allows a missing entry');
});

test('helpers: packages, closure with cycles and unknown files, chunk lookup, budget', () => {
  assert.equal(packageOf('node_modules/@angular/core/fesm2022/x.mjs'), '@angular/core');
  assert.equal(packageOf('src/app/dock/dock.ts'), 'src/app/dock');
  assert.equal(packageOf('src/main.ts'), 'src');
  assert.equal(packageOf('other/dir/x.js'), 'other/dir');
  const by = indexOutputs({ 'd/a.js': { bytes: 1, imports: [imp('b.js'), imp('missing.js')] }, 'd/b.js': { bytes: 1, imports: [imp('a.js')] }, 'd/c.css': { bytes: 9 } });
  assert.deepEqual([...staticClosure(by, ['a.js'])].sort(), ['a.js', 'b.js']);
  assert.equal(chunkOf(indexOutputs(outputs), 'src/main.ts'), 'main.js');
  assert.equal(budgetKb(1000 * KB), 1100);
  assert.equal(budgetKb(1), 1);
  assert.match(report(measure(outputs, 'main')), /1030\.0 kB in 6 chunks[\s\S]*heaviest packages:/);
});

test('C29 good: within budget and tight', async () => {
  assert.deepEqual(await findingsOf('C29', app({ main: { maxKb: budgetKb(MAIN_KB * KB) } })), []);
});

test('C29 bad: over budget is an error; a loose budget and a missing budget warn', async () => {
  const over = await findingsOf('C29', app({ main: { maxKb: 1000 } }));
  assert.equal(over[0].severity, 'error');
  assert.match(over[0].message, /loads 1030\.0 kB .* over its 1000 kB budget/);
  const loose = await findingsOf('C29', app({ main: { maxKb: 5000 } }));
  assert.deepEqual(loose.map((f) => f.severity), ['warn']);
  assert.match(loose[0].fix, /Tighten it to 1133 kB/);
  assert.match(messages(await findingsOf('C29', app({ main: {} }))), /no maxKb/);
});

test('C29 bad: a window that is not in the build, a broken metafile, no stats, no configuration', async () => {
  assert.match(messages(await findingsOf('C29', app({ ghost: { maxKb: 1 } }))), /"ghost" window's entry chunks are not in the build stats/);
  assert.match(messages(await findingsOf('C29', app({ main: { maxKb: 1 } }, { 'app/dist/app/browser-stats.json': '{"x":1}' }))), /not an esbuild metafile/);
  assert.match(messages(await findingsOf('C29', app({ main: { maxKb: 1 } }, { 'app/dist/app/browser-stats.json': 'nope' }))), /not an esbuild metafile/);
  const noStats = await findingsOf('C29', makeApp({ 'paddock.conformance.json': JSON.stringify({ windowWeight: { windows: { main: { maxKb: 1 } } } }) }));
  assert.deepEqual(noStats.map((f) => f.severity), ['warn']);
  assert.deepEqual((await findingsOf('C29', makeApp({ 'a.txt': 'x' }))).map((f) => f.severity), ['warn']);
  const custom = makeApp({ 'paddock.conformance.json': JSON.stringify({ windowWeight: { stats: 'out/stats.json', windows: { main: { maxKb: budgetKb(MAIN_KB * KB) } } } }), 'out/stats.json': JSON.stringify({ outputs }) });
  assert.deepEqual(await findingsOf('C29', custom), []);
});

test('window-weight CLI: report, --max, configured windows, --record writes measured + 10% and keeps other keys', async () => {
  const root = app({ main: { maxKb: 2000 } }, { 'paddock.conformance.json': JSON.stringify({ app: 'demo', windowWeight: { windows: { main: { maxKb: 2000 } } }, sidecars: [] }) });
  const lines = [];
  const io = { log: (s) => lines.push(s), err: (s) => lines.push(s) };
  assert.equal(await main(['window-weight', '--root', root], io), 0);
  assert.match(lines.join('\n'), /"main" window loads before first paint/);
  assert.equal(await main(['window-weight', 'main', '--root', root, '--max', '1000'], io), 1);
  assert.match(lines.join('\n'), /FAIL: 1030\.0 kB exceeds the 1000 kB budget/);
  assert.equal(await main(['window-weight', 'main', 'about', '--root', root, '--record'], io), 0);
  const saved = JSON.parse(readFileSync(join(root, 'paddock.conformance.json'), 'utf8'));
  assert.equal(saved.windowWeight.windows.main.maxKb, 1133);
  assert.equal(saved.windowWeight.windows.about.maxKb, 386);
  assert.deepEqual(saved.sidecars, []);
  assert.equal(saved.app, 'demo');
  assert.deepEqual(await findingsOf('C29', root), [], 'recorded budgets satisfy the check');
});

test('window-weight CLI: unreadable stats, unknown window and no windows are usage errors', async () => {
  const errs = [];
  const io = { log() {}, err: (s) => errs.push(s) };
  assert.equal(await main(['window-weight', 'main', '--root', makeApp({ 'a.txt': 'x' })], io), 2);
  assert.match(errs.join('\n'), /cannot read the build stats/);
  assert.equal(await main(['window-weight', 'ghost', '--root', app({})], io), 2);
  assert.match(errs.join('\n'), /could not find the "ghost" window's entry chunks/);
  assert.equal(await main(['window-weight', '--root', app({})], io), 2);
  assert.match(errs.join('\n'), /no window given and none configured/);
});
