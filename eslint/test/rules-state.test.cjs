'use strict';
const { ts } = require('./helpers.cjs');
const rules = require('../src/rules/index.cjs');

ts().run('effect-state (rule 28)', rules['effect-state'], {
  valid: [
    'effect(() => { console.log(a()); });',
    '// effect-state: mirrors the router into the store\neffect(() => { x.set(1); });',
    '// effect-state: reason one\n// more words\neffect(() => { patchState(s, { a: 1 }); });',
    'foo.effect(() => x.set(1));',
    "effect(() => { log('x.set(1)'); /* y.set(2) */ });",
    'class A { // effect-state: mirrors the input\n  e = effect(() => { this.x.update((v) => v + 1); }); }',
  ],
  invalid: [
    { code: 'effect(() => { x.set(1); });', errors: [{ messageId: 'unjustified' }] },
    { code: 'effect(() => { patchState(s, { a: 1 }); });', errors: [{ messageId: 'unjustified' }] },
    { code: 'effect(() => { x.update((v) => v + 1); });', errors: [{ messageId: 'unjustified' }] },
    { code: 'effect(() => { if (a) { untracked(() => { x.set(1); }); } });', errors: [{ messageId: 'unjustified' }] },
    { code: '// an ordinary comment\neffect(() => { x.set(1); });', errors: [{ messageId: 'unjustified' }] },
    { code: 'class A { e = effect(() => { this.x.set(1); }); }', errors: [{ messageId: 'unjustified' }] },
  ],
});

const store = (body) => `export const S = signalStore(${body});`;
ts().run('store-rules (rules 30 to 32)', rules['store-rules'], {
  valid: [
    { code: store('withState({ items: [], status: "idle" })'), filename: 'a.store.ts' },
    { code: store('withState({ items: [] })'), filename: 'a.store.spec.ts' },
    { code: '// app-wide: the one clock\n' + store('{ providedIn: "root" }, withState({})'), filename: 'clock.store.ts' },
    { code: store('withState({ loading: true })'), filename: 'request-status.store.ts', options: [{ allowFlagsIn: 'request-status' }] },
    { code: store('withProps(() => ({ a: inject(AStore), b: inject(BStore) }))'), filename: 'a.store.ts' },
    { code: 'const s = {}; withState({ items: 1 });', filename: 'a.ts' },
  ],
  invalid: [
    { code: store('withState({ loading: false, error: null })'), filename: 'a.store.ts', errors: [{ messageId: 'flag' }, { messageId: 'flag' }] },
    { code: store('withState({ isLoading: false })'), filename: 'a.store.ts', errors: [{ messageId: 'flag' }] },
    { code: store('withState({})'), filename: 'a.service.ts', errors: [{ messageId: 'file' }] },
    { code: store('{ protectedState: false }, withState({})'), filename: 'a.store.ts', errors: [{ messageId: 'protectedState' }] },
    { code: store('{ providedIn: "root" }, withState({})'), filename: 'a.store.ts', errors: [{ messageId: 'root' }] },
    { code: store('withProps(() => ({ a: inject(AStore), b: inject(BStore), c: inject(CStore), s: inject(Service) }))'), filename: 'a.store.ts', errors: [{ messageId: 'inject', data: { count: 3 } }] },
  ],
});
