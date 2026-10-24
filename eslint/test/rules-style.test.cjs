'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { ts, html } = require('./helpers.cjs');
const rules = require('../src/rules/index.cjs');
const { cssProblems } = require('../src/rules/no-raw-style-values.cjs');

const comp = (styles) => `@Component({ selector: 'pc-a', template: '', styles: ${styles} }) class A {}`;

ts().run('no-raw-style-values (design guide)', rules['no-raw-style-values'], {
  valid: [
    comp('`:host { color: var(--pd-text); font-size: var(--pd-size-2); font-family: var(--font-body); }`'),
    comp('[`a { padding: 4px; }`, `b { background: var(--pd-bg); }`]'),
    comp("'a { font-family: inherit; }'"),
    '@Injectable() class S { styles = `a { color: #fff; }`; }',
    { code: 'const x = `not styles #fff`;', filename: 'a.ts' },
  ],
  invalid: [
    { code: comp('`:host { color: #fff; }`'), errors: [{ message: /Hard-coded hex colour/ }] },
    { code: comp('`a { color: rgba(0, 0, 0, .5); }`'), errors: [{ message: /Hard-coded colour function/ }] },
    { code: comp('`a { color: red; }`'), errors: [{ message: /Hard-coded named colour/ }] },
    { code: comp('`a { font-size: 12px; }`'), errors: [{ message: /Hard-coded font size/ }] },
    { code: comp('`a { font-family: Arial; }`'), errors: [{ message: /Hard-coded font family/ }] },
    { code: comp('`a { font: 12px/1.4 sans-serif; }`'), errors: [{ message: /Hard-coded font shorthand/ }] },
    { code: 'export const s = `a { background: #123456; }`;', filename: 'chart-styles.ts', errors: [{ message: /Hard-coded hex colour/ }] },
  ],
});

test('the line of a problem is the line inside the styles block', () => {
  assert.deepEqual(cssProblems('a { }\n/* #fff */\nb { color: #abc; }').map((p) => p.line), [3]);
});

html().run('template rules (rule 33, security)', { 'ng-class': rules['template-no-ng-class'], 'inner-html': rules['template-no-inner-html'] }['ng-class'], {
  valid: ['<div [class.on]="active()"></div>', '<div class="a"></div>'],
  invalid: [{ code: '<div [ngClass]="{ on: active() }"></div>', errors: [{ messageId: 'bound' }] }],
});
html().run('template-no-inner-html', rules['template-no-inner-html'], {
  valid: ['<p>{{ text() }}</p>', '<p [textContent]="text()"></p>'],
  invalid: [
    { code: '<p [innerHTML]="text()"></p>', errors: [{ messageId: 'bound' }] },
    { code: '<p [outerHTML]="text()"></p>', errors: [{ messageId: 'bound' }] },
  ],
});
