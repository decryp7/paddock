import assert from 'node:assert/strict';
import { test } from 'node:test';
import check, { ownedScrollProblems, positiveControl, scrollWrites } from '../src/checks/C138-owned-scroll.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const component = (template, body) => `@Component({ selector: 'x-a', template: \`${template}\` })\nexport class A {\n${body}\n}\n`;

test('C138 good: a scroller with the directive, the class, the mixin, a CDK viewport, a templateUrl, a reasoned waiver; reads and specs are ignored', async () => {
  const root = makeApp({
    'apps/one/app/src/app/log.ts': component('<div #log pdOwnedScroll></div>', '  end(el: HTMLElement) { el.scrollTop = el.scrollHeight; }'),
    'apps/one/app/src/app/cls.ts': component('<div class="list pd-owned-scroll"></div>', '  f(el: HTMLElement) { el.scrollBy(0, 4); }'),
    'apps/one/app/src/app/cdk.ts': component('<cdk-virtual-scroll-viewport itemSize="20"></cdk-virtual-scroll-viewport>', '  f() { this.v.scrollToOffset(0); }'),
    'apps/one/app/src/app/page.ts': "@Component({ selector: 'x-p', templateUrl: './page.html' })\nexport class P { f(el: HTMLElement) { el.scrollLeft = 0; } }\n",
    'apps/one/app/src/app/page.html': '<div class="strip" pdOwnedScroll></div>\n',
    'apps/one/app/src/app/styled.ts': "@Component({ selector: 'x-s', template: '<div></div>', styleUrl: '../styles/s.scss' })\nexport class S { f(el: HTMLElement) { el.scrollTop += 2; } }\n",
    'apps/one/app/src/styles/s.scss': '.box { @include base.owned-scroll; }\n',
    'apps/one/app/src/app/helper.ts': 'export function nudge(el: HTMLElement) {\n  // conformance: owned-scroll — the caller owns the scroller and reads it back at once\n  el.scrollTop = el.scrollTop + 1;\n}\n',
    'apps/one/app/src/app/read.ts': component('<div></div>', '  f(el: HTMLElement) { return el.scrollTop === 0 || el.scrollLeft !== 3; }'),
    'apps/one/app/src/app/page-scroll.ts': component('<div></div>', '  f() { window.scrollTo(0, 0); document.scrollingElement; }'),
    'apps/one/app/src/app/log.spec.ts': component('<div></div>', '  f(el: HTMLElement) { el.scrollTop = 9; }'),
    'apps/one/app/e2e/x.ts': 'export const f = (el: HTMLElement) => { el.scrollTop = 9; };\n',
    'ui/projects/paddock-e2e/src/grid.ts': 'export const f = (el: HTMLElement) => { el.scrollTop = 9; };\n',
    'apps/one/app/src/app/commented.ts': component('<div></div>', '  // el.scrollTop = 3 was the old way\n  f() {}'),
  });
  assert.deepEqual(await findingsOf('C138', root), []);
});

test('C138 bad: each write shape in a component without the style is a warning on its line (report-only, ER-41)', async () => {
  const body = [
    '  a(el: HTMLElement) { el.scrollTop = 1; }',
    '  b(el: HTMLElement) { el.scrollLeft -= 1; }',
    '  c(el: HTMLElement) { el.scrollTo({ top: 0 }); }',
    '  d(el: HTMLElement) { this.box().nativeElement.scrollBy(0, 1); }',
    '  e() { this.viewport.scrollToIndex(4); }',
    '  f(el: HTMLElement) { Object.assign(el, { scrollTop: 5 }); }',
  ].join('\n');
  const f = await findingsOf('C138', makeApp({ 'apps/one/app/src/app/grid.ts': component('<div class="viewport"></div>', body) }));
  assert.equal(f.length, 6);
  assert.ok(f.every((x) => x.severity === 'warn'));
  assert.deepEqual(f.map((x) => x.line), [3, 4, 5, 6, 7, 8]);
  assert.match(messages(f), /writes `scrollTop` but no scroller in this component carries the shared style/);
  assert.match(messages(f), /writes `scrollToIndex`/);
});

test('C138 bad: a helper outside a component needs a waiver, and a waiver needs a reason', async () => {
  const f = await findingsOf('C138', makeApp({
    'ui/projects/lib/widgets/src/gesture.ts': 'export function nudge(el: HTMLElement) {\n  el.scrollTop = 1;\n}\n',
    'ui/projects/lib/widgets/src/bare.ts': 'export function nudge(el: HTMLElement) {\n  el.scrollTop = 1; // conformance: owned-scroll\n}\n',
  }));
  assert.equal(f.length, 2);
  assert.match(messages(f), /writes `scrollTop` outside a component/);
  assert.match(messages(f), /the `owned-scroll` waiver for this `scrollTop` has no reason/);
});

test('C138 the marker is looked up in the templateUrl the component names, not any html beside it', async () => {
  const f = await findingsOf('C138', makeApp({
    'apps/one/app/src/app/page.ts': "@Component({ selector: 'x-p', templateUrl: './page.html' })\nexport class P { f(el: HTMLElement) { el.scrollLeft = 0; } }\n",
    'apps/one/app/src/app/page.html': '<div class="strip"></div>\n',
    'apps/one/app/src/app/other.html': '<div pdOwnedScroll></div>\n',
  }));
  assert.equal(f.length, 1);
});

test('C138 positive control (ER-33): the probe sees a known write, a marker and a waiver, and a read is not a write', () => {
  assert.equal(positiveControl(), true);
  assert.equal(scrollWrites('el.scrollTop = 1;\nconst x = el.scrollTop;\n').length, 1);
  assert.deepEqual(ownedScrollProblems(component('<div></div>', '  f(el: HTMLElement) { el.scrollTop = 1; }'), () => null).length, 1);
  assert.equal(check.mode, 'report-only');
});
