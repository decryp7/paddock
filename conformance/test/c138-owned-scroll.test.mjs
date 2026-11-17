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
  // The one reasoned waiver (helper.ts) is listed in a single warning; nothing else is found.
  const f = await findingsOf('C138', root);
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'warn');
  assert.match(f[0].message, /^1 site waived by an owned-scroll comment: apps\/one\/app\/src\/app\/helper\.ts:3$/);
});

test('C138 lists every reasoned waiver in one warning, and a site without a reason is not in it', async () => {
  const waive = (n) => `export function f${n}(el: HTMLElement) {\n  // conformance: owned-scroll — the caller owns this scroller\n  el.scrollTop = ${n};\n}\n`;
  const f = await findingsOf('C138', makeApp({
    'ui/projects/lib/a/src/one.ts': waive(1) + waive(2),
    'ui/projects/lib/a/src/two.ts': waive(3),
    'ui/projects/lib/a/src/bare.ts': 'export function g(el: HTMLElement) {\n  el.scrollTop = 1; // conformance: owned-scroll\n}\n',
  }));
  const list = f.filter((x) => /waived by an owned-scroll comment/.test(x.message));
  assert.equal(list.length, 1);
  assert.match(list[0].message, /^3 sites waived by an owned-scroll comment: /);
  assert.match(list[0].message, /one\.ts:3, .*one\.ts:7/);
  assert.match(list[0].message, /two\.ts:3/);
  assert.doesNotMatch(list[0].message, /bare\.ts/);
  assert.equal(f.filter((x) => /has no reason/.test(x.message)).length, 1);
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

test('C138 precision (review of 8e06ec809): receivers, bracket and logical assignments, multi-line assigns, comments and strings', () => {
  const one = (body, tpl = '<div></div>') => ownedScrollProblems(component(tpl, body), () => null).length;
  // Writes the first version missed.
  assert.equal(one('  f(el?: HTMLElement) { el?.scrollTo(0, 0); }'), 1, 'optional call');
  assert.equal(one('  f(el: Element) { (el as HTMLElement).scrollTo(0, 0); }'), 1, 'cast receiver');
  assert.equal(one('  f(els: HTMLElement[]) { els[0].scrollTo(0, 0); }'), 1, 'indexed receiver');
  assert.equal(one("  f(el: HTMLElement) { el['scrollTop'] = 3; }"), 1, 'bracket write');
  assert.equal(one('  f(el: HTMLElement) { el.scrollTop ||= 3; }'), 1, 'logical assignment');
  assert.equal(one('  f(el: HTMLElement) { el.scrollLeft ??= 3; }'), 1, 'nullish assignment');
  assert.equal(one('  f(el: HTMLElement) { Object.assign(el, {\n    scrollTop: 5,\n  }); }'), 1, 'multi-line Object.assign');
  // A marker in a comment is no marker; a `//` inside a string is no comment.
  assert.equal(one('  // TODO: add pdOwnedScroll\n  f(el: HTMLElement) { el.scrollTop = 3; }'), 1, 'marker in a TS comment');
  assert.equal(one('  f(el: HTMLElement) { el.scrollTop = 3; }', '<!-- pdOwnedScroll --><div></div>'), 1, 'marker in an HTML comment');
  assert.equal(one("  f(el: HTMLElement) { const u = 'a//b'; el.scrollTop = 3; }"), 1, 'write after a string with //');
  // Not writes: reads and the page.
  assert.equal(one('  f(el: HTMLElement) { return el.scrollTop >= 3 && el.scrollLeft == 0; }'), 0, 'comparisons');
  assert.equal(one('  f() { window.scrollTo(0, 0); globalThis.scrollBy(0, 1); document.scrollingElement; }'), 0, 'the page');
  // Accepted gaps, documented in the header and the README: scrollIntoView, Renderer2.setProperty, per file not per element.
  assert.equal(one("  f(el: HTMLElement) { el.scrollIntoView({ block: 'nearest' }); }"), 0, 'scrollIntoView (gap)');
  assert.equal(one("  f(el: HTMLElement) { this.r.setProperty(el, 'scrollTop', 3); }"), 0, 'setProperty (gap)');
  assert.equal(one('  f(el: HTMLElement) { el.scrollTop = 3; }', '<div pdOwnedScroll></div><div #b></div>'), 0, 'per file (gap)');
  // Outside a component, every write needs a waiver.
  assert.equal(ownedScrollProblems("@Directive({ selector: '[x]' })\nexport class D { f() { this.el.nativeElement.scrollTop = 3; } }\n", () => null).length, 1, 'directive host');
  assert.equal(ownedScrollProblems('@Injectable()\nexport class S { pin(el: HTMLElement) { el.scrollTop = el.scrollHeight; } }\n', () => null).length, 1, 'service');
});
