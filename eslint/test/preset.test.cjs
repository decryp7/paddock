'use strict';
const assert = require('node:assert/strict');
const { mkdirSync, mkdtempSync, writeFileSync, rmSync } = require('node:fs');
const { tmpdir } = require('node:os');
const { dirname, join } = require('node:path');
const { test, after } = require('node:test');
const { ESLint } = require('eslint');
const { defineConfig } = require('eslint/config');
const { preset } = require('../src/index.cjs');

const dir = mkdtempSync(join(tmpdir(), 'paddock-eslint-'));
after(() => rmSync(dir, { recursive: true, force: true }));

async function lint(files, options = { prefix: 'pc' }) {
  for (const [p, text] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, p)), { recursive: true });
    writeFileSync(join(dir, p), text);
  }
  const eslint = new ESLint({ cwd: dir, overrideConfigFile: true, overrideConfig: defineConfig(...preset(options)) });
  const results = await eslint.lintFiles(Object.keys(files));
  return results.flatMap((r) => r.messages.map((m) => `${r.filePath.slice(dir.length + 1)}: ${m.ruleId ?? 'parse'}`));
}

const GOOD = `import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
@Component({
  selector: 'pc-good',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: '<p [class.on]="n() > 0">{{ n() }}</p>',
  styles: [':host { color: var(--pd-text); }'],
})
export class GoodComponent {
  readonly n = signal(0);
}
`;

test('a component that follows the rules is clean', async () => {
  assert.deepEqual(await lint({ 'src/app/good.component.ts': GOOD }), []);
});

test('the preset reports the rules it was made for, with the files they belong to', async () => {
  const bad = `import 'zone.js';
import { Component, effect, signal } from '@angular/core';
import { patchState } from '@ngrx/signals';
@Component({ selector: 'other-bad', template: '', styles: ['a { color: #fff; }'] })
export class BadComponent {
  n = signal(0);
  e = effect(() => { this.n.set(1); });
}
`;
  const found = await lint({ 'src/app/bad.component.ts': bad });
  for (const id of ['paddock/no-zone', 'paddock/onpush-explicit', 'paddock/effect-state', 'paddock/no-raw-style-values', 'no-restricted-imports', '@angular-eslint/component-selector']) {
    assert.ok(found.some((f) => f.endsWith(id)), `${id} in ${found.join(', ')}`);
  }
});

test('patchState may be imported in a store file and in a spec, not elsewhere', async () => {
  const code = "import { patchState } from '@ngrx/signals';\nexport const f = patchState;\n";
  assert.deepEqual(await lint({ 'src/app/x.store.ts': code, 'src/app/x.spec.ts': code }), []);
  assert.deepEqual(await lint({ 'src/app/x.ts': code }), ['src/app/x.ts: no-restricted-imports']);
});

test('templates: control flow, class bindings and no innerHTML', async () => {
  const found = await lint({ 'src/app/t.component.html': '<div *ngIf="a" [ngClass]="b" [innerHTML]="c"></div>\n' });
  assert.ok(found.some((f) => f.endsWith('paddock/template-no-ng-class')));
  assert.ok(found.some((f) => f.endsWith('paddock/template-no-inner-html')));
  assert.ok(found.some((f) => f.endsWith('prefer-control-flow')));
});

test('options: prefix is required; signalForms forbids reactive forms; extra rules and ignores apply', async () => {
  assert.throws(() => preset({}), /prefix/);
  const forms = "import { FormBuilder } from '@angular/forms';\nexport const f = FormBuilder;\n";
  assert.deepEqual(await lint({ 'src/app/f.ts': forms }), []);
  assert.deepEqual(await lint({ 'src/app/f.ts': forms }, { prefix: 'pc', signalForms: true }), ['src/app/f.ts: no-restricted-imports']);
  assert.deepEqual(await lint({ 'src/app/g.ts': "export const a = 1;\nvar b = 2;\n" }, { prefix: 'pc', rules: { 'no-var': 'off', '@typescript-eslint/no-unused-vars': 'off' } }), []);
  const ignored = new ESLint({ cwd: dir, overrideConfigFile: true, overrideConfig: defineConfig(...preset({ prefix: 'pc', ignores: ['src/gen/**'] })) });
  assert.equal(await ignored.isPathIgnored(join(dir, 'src/gen/x.ts')), true);
  assert.equal(await ignored.isPathIgnored(join(dir, 'dist/x.ts')), true);
});

test('dialog contents draw no footer of their own (ADR 0026)', async () => {
  const dialog = `import { ChangeDetectionStrategy, Component } from '@angular/core';
@Component({ selector: 'pc-rename-dialog', changeDetection: ChangeDetectionStrategy.OnPush, template: '<p>Name</p><div class="pc-actions"><button type="button">Cancel</button></div>' })
export class RenameDialog {}
`;
  const found = await lint({ 'src/app/rename-dialog.ts': dialog });
  assert.equal(found.filter((f) => f.endsWith('paddock/dialog-frame-footer')).length, 2, found.join('\n'));
  assert.deepEqual(await lint({ 'src/app/plain.component.ts': dialog.replace('pc-rename-dialog', 'pc-plain').replace('RenameDialog', 'Plain') }), []);
});
