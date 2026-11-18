'use strict';
// `paddock/no-backend-call-in-loop` (PRINCIPLES, Bulk APIs; WP-0 G5): one case per shape it flags, the shapes it leaves alone, and
// the opt-out. The positive control (ER-33) is the last block: the same source with the loop taken away gives no finding, and with
// the backend name hidden gives none, so a finding comes from the loop and the receiver, not from the call alone.
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { Linter } = require('eslint');
const tseslint = require('typescript-eslint');
const { ts } = require('./helpers.cjs');
const rules = require('../src/rules/index.cjs');

const rule = rules['no-backend-call-in-loop'];
const loop = { messageId: 'loop' };
const parallel = { messageId: 'parallel' };

ts().run('no-backend-call-in-loop (Bulk APIs)', rule, {
  valid: [
    // One bulk call for every item.
    'async function f(ids) { await backend.deleteModels(ids); }',
    // A loop over local work only.
    'for (const m of models) total += m.size;',
    // The loop reads the backend's answer; the call is outside it.
    'const rows = await backend.list(); for (const r of rows) show(r);',
    // `for (init; ...)` and `for (... of <right>)` run once.
    'for (let page = await backend.first(); page; page = page.next) show(page);',
    'for (const r of await backend.list()) show(r);',
    // A function made in a loop is not a call in it.
    'for (const x of xs) handlers.push(() => backend.open(x));',
    // `.map` whose promises are not started together is not this rule's shape.
    'const labels = ids.map((id) => names.get(id));',
    'await Promise.all(ids.map((id) => cache.get(id)));',
    // A callback handed to something that keeps it for later runs later, not once per item now.
    'for (const x of xs) setTimeout(() => backend.open(x), 10);',
    'for (const x of xs) effect(() => backend.watch(x()));',
    'for (const x of xs) emitter.on(x, () => backend.open(x));',
    'for (const x of xs) new Promise(() => backend.open(x));',
    // Loops over no collection: a retry or paging loop is one call per attempt or page, not per item (P2-1).
    'for (let attempt = 1; ; attempt++) { const a = await backend.queryPage(q); if (a.ok) break; }',
    'for (let i = 0; i < 3; i++) await backend.retry();',
    'while (true) { const p = await backend.next(); if (!p) break; }',
    'do { page = await backend.next(page); } while (page);',
    'while (await backend.more()) step();',
    // A loop over a list written out in the code is a fixed handful, not N items.
    'for (const id of [survey, ai]) if (id !== null) void jobs.unwait(id);',
    '[a, b].forEach((x) => backend.close(x));',
    // Names that only end like a port.
    'for (const x of xs) report.add(x); for (const x of xs) viewport.scroll(x); for (const x of xs) support.ask(x);',
    // The opt-out with a reason, on the call or on the loop.
    'for (const r of runners) {\n  // per-item: each runner has its own pipe and answers on its own\n  await backend.ping(r);\n}',
    '// per-item: the helper has no bulk form on old versions\nfor (const c of cards) await backend.wolState(c);',
    '// per-item: one tile at a time on purpose\nawait Promise.all(tiles.map((t) => backend.tile(t)));',
  ],
  invalid: [
    // for-of with await (PW-1's shape).
    { code: 'for (const m of models) await backend.deleteModel(m.id);', errors: [loop] },
    // Classic for, for-in, while, do-while, for-await.
    { code: 'for (let i = 0; i < items.length; i++) { await this.backend.save(items[i]); }', errors: [loop] },
    { code: 'for (const k in byId) backend.touch(k);', errors: [loop] },
    { code: 'while (queue.length) await backend.send(queue.shift());', errors: [loop] },
    { code: 'for await (const x of stream) await backend.put(x);', errors: [loop] },
    // A loop's test and update run every iteration, over the collection it drains.
    { code: 'while (queue.size > 0) await backend.send(take(queue));', errors: [loop] },
    { code: 'do { await backend.send(queue.pop()); } while (queue.length);', errors: [loop] },
    // `invoke` itself, bare or on the IPC object.
    { code: "for (const id of ids) await invoke('delete_watch', { id });", errors: [loop] },
    { code: "for (const id of ids) await ipc.invoke('delete_watch', { id });", errors: [loop] },
    // A field or variable made by `inject(<backend or port token>)`, whatever its name.
    { code: 'class S { private ws = inject(DebriefBackend); async f(ids) { for (const id of ids) await this.ws.drop(id); } }', errors: [loop] },
    { code: 'const menu = inject(NATIVE_MENU_PORT); for (const i of items) menu.add(i);', errors: [loop] },
    { code: 'for (const i of items) inject(BACKEND).add(i);', errors: [loop] },
    // Port names.
    { code: 'for (const r of rows) await this.storePort.write(r);', errors: [loop] },
    // `.forEach` is a loop.
    { code: 'ids.forEach((id) => backend.remove(id));', errors: [loop] },
    { code: 'ids.forEach(async function (id) { await this.backend.remove(id); });', errors: [loop] },
    // N calls started together (SP-2, DB-4's shape): Promise.all/allSettled and forkJoin over a `.map`.
    { code: 'await Promise.all(ids.map((id) => backend.get(id)));', errors: [parallel] },
    { code: 'await Promise.allSettled(ids.map(async (id) => { await backend.get(id); }));', errors: [parallel] },
    { code: 'forkJoin(ids.map((id) => this.backend.get$(id)));', errors: [parallel] },
    { code: 'await Promise.all([...ids.map((id) => backend.get(id)), backend.summary()]);', errors: [parallel] },
    { code: 'const pending = ids.map((id) => backend.get(id));\nconst rows = await Promise.all(pending);', errors: [parallel] },
    // A loop around Promise.all is still one finding per call.
    { code: 'for (const g of groups) await Promise.all(g.map((id) => backend.get(id)));', errors: [parallel] },
    // A callback handed to a wrapper (`act`, `apply`, `withRequestStatus`) runs inside the call (SP-2, DB-20's watched folders).
    { code: "await Promise.all(unseen.map((w) => act(() => backend.acknowledge(w.id, null), null)));", errors: [parallel] },
    { code: 'for (const f of folders) await apply(() => backend.removeWatchedFolder(f.path));', errors: [loop] },
    { code: 'for (const r of rows) await guard(async () => { await backend.save(r); }).then(() => backend.touch(r));', errors: [loop, loop] },
    // A parameter typed as `invoke` (DB-21's `call: typeof invoke`).
    { code: "async function cancel(call: typeof invoke, ids: number[]) { for (const id of ids) await call<void>('jobs_cancel', { id }); }", errors: [loop] },
    // An ordinary comment is not the opt-out, nor is the tag without a reason.
    { code: '// slow, fix later\nfor (const m of models) await backend.deleteModel(m.id);', errors: [loop] },
    { code: '// per-item:\nfor (const m of models) await backend.deleteModel(m.id);', errors: [loop] },
    // A reason needs at least three words (P2-2).
    { code: '// per-item: legacy\nfor (const m of models) await backend.deleteModel(m.id);', errors: [loop] },
    { code: '// per-item: legacy code\nfor (const m of models) await backend.deleteModel(m.id);', errors: [loop] },
  ],
});

/** The findings of the rule on `code`, as message ids. */
function findings(code) {
  const linter = new Linter({ configType: 'flat' });
  const messages = linter.verify(code, [
    { files: ['**/*.ts'], languageOptions: { parser: tseslint.parser, ecmaVersion: 2023, sourceType: 'module' }, plugins: { paddock: { rules: { 'no-backend-call-in-loop': rule } } }, rules: { 'paddock/no-backend-call-in-loop': 'error' } },
  ], 'a.ts');
  return messages.map((m) => m.messageId ?? m.message);
}

test('positive control (ER-33): the finding comes from the loop and the backend receiver, not from the call alone', () => {
  assert.deepEqual(findings('for (const m of models) await backend.deleteModel(m.id);'), ['loop']);
  // Loop taken away: the same call, no finding.
  assert.deepEqual(findings('await backend.deleteModel(models[0].id);'), []);
  // Receiver hidden behind a name the rule does not know: no finding.
  assert.deepEqual(findings('for (const m of models) await thing.deleteModel(m.id);'), []);
  // Promise.all taken away from the map: no finding; put back: one.
  assert.deepEqual(findings('const ps = ids.map((id) => backend.get(id));'), []);
  assert.deepEqual(findings('await Promise.all(ids.map((id) => backend.get(id)));'), ['parallel']);
});
