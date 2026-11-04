import assert from 'node:assert/strict';
import { test } from 'node:test';
import { classifyFailure, controlledStep, renderResult, verdict, withReaderControl } from './judge.mjs';
import { LIFECYCLE_IDS, defineCheck, defineSuite, pageHandleControl, pageReaderControl, runCheck, runValueControl } from './lifecycle.mjs';

const run = (extra = {}) => defineCheck({
  id: 'L1', title: 'Run value', pass: 'registered',
  probe: async (io) => ({ ran: true, raw: String(await io.readRunValue('Dyno')), value: await io.readRunValue('Dyno') }),
  judge: (v) => ({ ok: v === 'C:\\Dyno.exe', checks: [{ ok: v === 'C:\\Dyno.exe', detail: `Run value is ${JSON.stringify(v)}` }] }),
  control: runValueControl(),
  ...extra,
});
const guest = (values = {}) => ({
  writeRunValue: async (n, v) => { values[n] = v; },
  removeRunValue: async (n) => { delete values[n]; },
  readRunValue: async (n) => values[n],
});

test('a check without a positive control is refused at definition time', () => {
  assert.throws(() => defineCheck({ id: 'L2', title: 't', probe() {}, judge() {} }), /positive control/);
  assert.throws(() => defineCheck({ id: 'L2', title: 't', probe() {}, judge() {}, control: { describe: 'd' } }), /positive control/);
  assert.throws(() => defineSuite([run(), run()]), /twice/);
  assert.equal(LIFECYCLE_IDS.length, 8);
});

test('control passes and the probe reads the declared state: PASS', async () => {
  const s = await runCheck(run(), guest({ Dyno: 'C:\\Dyno.exe' }));
  assert.equal(s.status, 'pass');
  assert.equal(s.control.ok, true);
});

test('control passes and the probe reads another state: a product FAIL with the raw output as evidence', async () => {
  const s = await runCheck(run(), guest({ Dyno: 'D:\\Old.exe' }));
  assert.equal([s.status, s.class].join('/'), 'fail/product');
  assert.match(s.evidence.join('\n'), /probe: "?D:\\Old\.exe|probe: D:\\Old\.exe/);
  assert.match(s.evidence.join('\n'), /control: passed/);
});

test('the control itself fails: CHECK BROKEN (class check), never a product FAIL', async () => {
  const io = { ...guest({ Dyno: 'D:\\Old.exe' }), writeRunValue: async () => {} }; // the runner cannot write its own value
  const s = await runCheck(run(), io);
  assert.equal([s.status, s.class].join('/'), 'broken/check');
  assert.match(s.summary, /^CHECK BROKEN: the positive control failed/);
  assert.ok(s.fix);
  assert.equal(verdict([s]).result, 'CHECK BROKEN');
  assert.equal(verdict([s]).blocksTag, true);
});

test('the probe cannot run: check class, with the error in the summary', async () => {
  const s = await runCheck(run({ probe: async () => { throw new Error('ssh: connection refused'); } , control: { ...runValueControl(), probe: undefined } }), guest());
  assert.equal(s.class, 'check');
  assert.match(s.summary, /ssh: connection refused/);
});

test('the reader found no element of the expected kind: check class (the Settings window read as the page)', async () => {
  const d = defineCheck({
    id: 'L3', title: 'Card', probe: async (io) => ({ ran: true, raw: await io.readPage(), foundElement: (await io.readPage()).includes('Dyno'), value: await io.readPage() }),
    judge: (t) => ({ ok: t.includes('On'), checks: [{ ok: t.includes('On'), detail: 'card says On' }] }),
    control: pageReaderControl({ label: 'Dyno' }),
  });
  const s = await runCheck(d, { showKnownPage: async () => {}, readPage: async () => 'Settings' });
  assert.equal(s.class, 'check');
});

test('page handle control: a lost handle after a reboot is a check error', async () => {
  const d = defineCheck({
    id: 'L4', title: 'Restart', probe: async (io) => { const h = await io.acquirePage(); return { ran: true, raw: String(Boolean(h)), foundElement: Boolean(h), value: h ? await h.evaluate('1 + 1') : null }; },
    judge: (a) => ({ ok: a === 2, checks: [{ ok: a === 2, detail: `answer ${a}` }] }), control: pageHandleControl(),
  });
  const up = { ensureAppUp: async () => {}, acquirePage: async () => ({ evaluate: async () => 2 }) };
  assert.equal((await runCheck(d, up)).status, 'pass');
  assert.equal((await runCheck(d, { ...up, acquirePage: async () => null })).status, 'broken');
});

test('older steps without a control: a fail stays a product FAIL when the reader control passed, and breaks when it failed', () => {
  const failed = { title: 'Cold start', status: 'fail', summary: 'two agents', checks: [{ ok: false, detail: 'two agents' }] };
  assert.equal(withReaderControl(failed, { ok: true, detail: 'ps lists itself' }).status, 'fail');
  const b = withReaderControl(failed, { ok: false, detail: 'ps did not list itself' });
  assert.equal([b.status, b.class].join('/'), 'broken/check');
  assert.equal(withReaderControl({ title: 'x', status: 'pass', summary: '' }, null).status, 'pass');
});

test('classifyFailure: no control means check; mismatch with a passing control means product', () => {
  assert.equal(classifyFailure({ control: null, mismatch: 'm' }).class, 'check');
  assert.equal(classifyFailure({ control: { ok: true, detail: 'd' }, mismatch: 'm' }).class, 'product');
});

test('verdict reports product FAILs and check errors apart; both block a tag', () => {
  const steps = [{ title: 'a', status: 'fail' }, { title: 'b', status: 'broken' }, { title: 'c', status: 'pass' }];
  assert.deepEqual(verdict(steps), { result: 'FAIL and CHECK BROKEN', product: ['a'], check: ['b'], blocksTag: true });
  assert.equal(verdict([{ title: 'c', status: 'pass' }]).blocksTag, false);
});

test('the result file carries the lease, the VM, the snapshot, the command, the class and the evidence', () => {
  const step = controlledStep('L1: Run value', { control: { ok: false, detail: 'wrote nothing' }, checks: [{ ok: false, detail: 'x' }] });
  const md = renderResult({ app: 'Dyno', installer: 'i', guest: 'g', started: 't', lease: 'paddock-win11-test.1.2', vm: 'paddock-win11-test', snapshot: 'gui-session-tools', command: 'node run-win.mjs --app dyno', steps: [step] });
  for (const want of ['Result: **CHECK BROKEN**', 'Check errors (1)', 'Broker lease: paddock-win11-test.1.2', 'VM: paddock-win11-test, snapshot gui-session-tools', 'Runner command: `node run-win.mjs --app dyno`', 'Class: **check**', 'Positive control: FAILED']) assert.ok(md.includes(want), want);
});
