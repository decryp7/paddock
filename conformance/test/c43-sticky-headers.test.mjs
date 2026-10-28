import assert from 'node:assert/strict';
import { test } from 'node:test';
import { cssBlocks, stickyKind, stickyProblems, zValue } from '../src/checks/C43-sticky-headers.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const component = (css) => `@Component({ selector: 'x-grid', template: \`<table></table>\`, styles: \`${css}\` })\nexport class Grid {}\n`;

test('C43 good: headers with a z-index, columns below them, waived panels, specs and other files are ignored', async () => {
  const root = makeApp({
    'apps/one/app/src/app/grid.ts': component('th { position: sticky; top: 0; z-index: 3; background: var(--bg); } td.act { position: sticky; right: 0; z-index: 2; }'),
    'apps/one/app/src/app/panel.scss': '.panel {\n  /* conformance: not-a-header */\n  position: sticky;\n  top: 8px;\n}\n',
    'apps/one/app/src/app/footer.ts': component('.bar { position: sticky; bottom: 0; }'),
    'apps/one/app/src/app/grid.spec.ts': component('th { position: sticky; top: 0; }'),
    'apps/one/app/e2e/x.ts': component('th { position: sticky; top: 0; }'),
    'docs/x.css': 'th { position: sticky; top: 0; }',
    // Two entry points of a shared package are styled apart: one's header does not rank the other's columns.
    'ui/projects/lib/explorer/src/head.ts': component('.head { position: sticky; top: 0; z-index: 1; }'),
    'ui/projects/lib/matrix/src/matrix.ts': component('tbody th { position: sticky; inset-inline-start: 0; z-index: 2; }'),
    'ui/projects/lib/src/table.ts': component('.h { position: sticky; top: 0; z-index: var(--pd-z-table-header); } .c { position: sticky; inset-inline-start: 0; z-index: var(--pd-z-table-sticky-column); }'),
  });
  assert.deepEqual(await findingsOf('C43', root), []);
});

test('C43 bad: a sticky header without a z-index is an error on its line', async () => {
  const f = await findingsOf('C43', makeApp({ 'apps/one/app/src/app/alerts.ts': component('\n  th { position: sticky; top: 0; background: var(--bg-2); }\n  .muted td { opacity: 0.75; }') }));
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'error');
  assert.equal(f[0].file, 'apps/one/app/src/app/alerts.ts');
  assert.equal(f[0].line, 2);
  assert.match(messages(f), /`th` sticks to the top without a z-index/);
});

test('C43 bad: a sticky column at the header\'s z-index paints over it, also when the header is styled in another file of the app', async () => {
  const f = await findingsOf('C43', makeApp({
    'apps/one/app/src/styles.scss': '.table {\n  th {\n    position: sticky;\n    top: 0;\n    z-index: 1;\n  }\n}\n',
    'apps/one/app/src/app/library.ts': component('.table td.act { position: sticky; right: 0; z-index: 1; }'),
    // Another app's columns are compared with its own headers only.
    'apps/two/app/src/app/x.ts': component('td { position: sticky; left: 0; z-index: 5; }'),
  }));
  assert.equal(f.length, 1);
  assert.equal(f[0].file, 'apps/one/app/src/app/library.ts');
  assert.match(messages(f), /sticky column `\.table td\.act` has z-index 1, not below the lowest header's 1/);
});

test('cssBlocks: nested SCSS blocks keep their own declarations, interpolations and comments do not break the parse', () => {
  const blocks = cssBlocks('.t {\n  color: red;\n  th { position: sticky; top: ${HEAD}px; }\n  /* x { */ td { z-index: 1; }\n}');
  const th = blocks.find((b) => b.selector === 'th');
  assert.ok(th);
  assert.equal(th.line, 3);
  assert.equal(stickyKind(th.body), 'top');
  const outer = blocks.find((b) => b.selector === '.t');
  assert.ok(outer && !/sticky/.test(outer.body), 'the parent does not inherit the nested block');
});

test('stickyKind and zValue: what sticks where, and which z-indexes compare', () => {
  assert.equal(stickyKind('position: sticky; inset-block-start: 0'), 'top');
  assert.equal(stickyKind('position: sticky; inset-inline-end: 0'), 'side');
  assert.equal(stickyKind('position: sticky; bottom: 0'), null);
  assert.equal(stickyKind('position: relative; top: 0'), null);
  assert.equal(zValue('3'), 3);
  assert.equal(zValue('var(--pd-z-table-header)'), 3);
  assert.equal(zValue('var(--pd-z-table-sticky-column)'), 2);
  assert.equal(zValue('var(--app-z)'), null);
  assert.equal(zValue(null), null);
  assert.deepEqual(stickyProblems([{ file: 'a.ts', text: 'th { position: sticky; top: 0; z-index: var(--app-z); } td { position: sticky; left: 0; z-index: 9; }' }]), []);
});
