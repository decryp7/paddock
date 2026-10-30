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
    comp('`code { font-family: var(--pd-font-mono); } kbd { font-family: var(--pd-font-sans, system-ui); }`'),
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

html().run('dialog-frame-footer (ADR 0026)', rules['dialog-frame-footer'], {
  valid: ['<p>Remove the channel?</p>', '<form><input data-autofocus /></form>', '<button type="button">Copy</button>', '<div class="pd-key__field"></div>'],
  invalid: [
    { code: '<div class="pd-rm__actions"><span></span></div>', errors: [{ messageId: 'footer' }] },
    { code: '<div class="row foot"></div>', errors: [{ messageId: 'footer' }] },
    { code: '<footer></footer>', errors: [{ messageId: 'footer' }] },
    { code: '<button type="button" (click)="cancel()"> Cancel </button>', errors: [{ messageId: 'wayOut' }] },
    { code: '<button type="button">OK</button><button>Close</button>', errors: [{ messageId: 'wayOut' }, { messageId: 'wayOut' }] },
  ],
});

ts().run('no-control-or-meta (e2e keys follow the page)', rules['no-control-or-meta'], {
  valid: [
    "await pressMod(page, 'Mod+k');",
    "await rows.nth(1).click({ modifiers: [await pageModKey(page)] });",
    "await page.keyboard.press('Control+k');",
    '// ControlOrMeta in a comment is fine\nconst a = 1;',
  ],
  invalid: [
    { code: "await page.keyboard.press('ControlOrMeta+k');", errors: [{ message: /Use `pressMod/ }] },
    { code: "await input.press('ControlOrMeta+Enter');", errors: 1 },
    { code: "await row.click({ modifiers: ['ControlOrMeta'] });", errors: 1 },
    { code: 'await page.keyboard.press(`ControlOrMeta+${key}`);', errors: 1 },
  ],
});
