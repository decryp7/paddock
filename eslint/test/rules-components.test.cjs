'use strict';
const { ts } = require('./helpers.cjs');
const rules = require('../src/rules/index.cjs');

const comp = (meta) => `@Component({ selector: 'pc-a', template: '', ${meta} }) class A {}`;

ts().run('onpush-explicit (rule 27)', rules['onpush-explicit'], {
  valid: [
    comp('changeDetection: ChangeDetectionStrategy.OnPush'),
    '// not a component\n@Injectable({ providedIn: "root" }) class S {}',
    '@Component({ selector: "pc-a", // eager: the grid measures after every event\n changeDetection: ChangeDetectionStrategy.Eager }) class A {}',
    '@Component({ selector: "pc-a",\n  // eager: needed for the third-party chart\n  changeDetection: ChangeDetectionStrategy.Eager,\n}) class A {}',
  ],
  invalid: [
    { code: comp(''), errors: [{ messageId: 'missing' }] },
    { code: comp('changeDetection: ChangeDetectionStrategy.Default'), errors: [{ messageId: 'eager' }] },
    { code: comp('changeDetection: ChangeDetectionStrategy.Eager'), errors: [{ messageId: 'eager' }] },
    { code: comp('changeDetection: strategy'), errors: [{ messageId: 'eager', data: { strategy: 'a computed value' } }] },
  ],
});

ts().run('no-zone (rules 27, 38)', rules['no-zone'], {
  valid: [
    "import { provideZonelessChangeDetection } from '@angular/core';",
    "import { TestBed } from '@angular/core/testing';",
    "import 'not-zone';",
  ],
  invalid: [
    { code: "import 'zone.js';", errors: [{ messageId: 'zone' }] },
    { code: "import 'zone.js/testing';", errors: [{ messageId: 'zone' }] },
    { code: "import { provideZoneChangeDetection } from '@angular/core';\nprovideZoneChangeDetection({});", errors: [{ messageId: 'provide' }, { messageId: 'provide' }] },
    { code: "import { fakeAsync, tick, TestBed } from '@angular/core/testing';", errors: [{ messageId: 'testing' }, { messageId: 'testing' }] },
    { code: "import { waitForAsync } from '@angular/core/testing';", errors: [{ messageId: 'testing' }] },
  ],
});

ts().run('signal-read-before-await (rule 29)', rules['signal-read-before-await'], {
  valid: [
    'class A { n = signal(0); async go() { const v = this.n(); await load(v); } }',
    'class A { async go() { await load(); this.n(); } }', // n is not a signal field
    'class A { n = signal(0); go() { this.n(); } }',
    'class A { n = signal(0); async go(d) { await load(); /* after-await: race guard */ if (this.n() === d) done(); } }'.replace('/* after-await: race guard */', '\n// after-await: race guard\n'),
    'class A { n = signal(0); async go() { await load(); this.reload(); } }',
    'class A { n = signal(0); async go(p) { const g = await this.store.reopen(this.n(), p); return g; } }', // read in the operand of the first await
    'class A { n = signal(0); c = effect(async () => { const v = this.n(); await save(v); }); }',
  ],
  invalid: [
    { code: 'class A { n = signal(0); async go() { const v = (await load()) + this.n(); return v; } }', errors: [{ messageId: 'late' }] },
    { code: 'class A { n = signal(0); async go() { await load(); return this.n(); } }', errors: [{ messageId: 'late', data: { name: 'n' } }] },
    { code: 'class A { n = input.required(); go = async () => { await load(); this.n(); } }', errors: [{ messageId: 'late' }] },
    { code: 'class A { n = computed(() => 1); c = effect(async () => { await save(); this.n(); }); }', errors: [{ messageId: 'late' }] },
    { code: 'class A { a = signal(1); b = model(2); async go() { await x(); this.a(); this.b(); } }', errors: [{ messageId: 'late' }, { messageId: 'late' }] },
  ],
});
