import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const adrs = (...names) => Object.fromEntries(names.map((n) => [`docs/adr/${n}`, '# ADR\n']));

test('C31 good: unique numbers, README and other folders are ignored', async () => {
  assert.deepEqual(await findingsOf('C31', makeApp({ ...adrs('0001-a.md', '0002-b.md', 'README.md'), 'docs/other/0001-x.md': 'x', 'docs/adr/0001-notes/y.txt': 'y' })), []);
  assert.deepEqual(await findingsOf('C31', makeApp({ 'README.md': 'x' })), []);
});

test('C31 bad: a shared number is reported on every file that has it', async () => {
  const f = await findingsOf('C31', makeApp(adrs('0007-queue.md', '0007-notifier.md', '0008-ok.md')));
  assert.deepEqual(f.map((x) => x.file), ['docs/adr/0007-notifier.md', 'docs/adr/0007-queue.md']);
  assert.ok(f.every((x) => x.severity === 'error'));
  assert.match(messages(f), /ADR number 0007 is used by 2 files/);
});

test('C31 bad: 7 and 0007 are the same number, and a short number is reported', async () => {
  const f = await findingsOf('C31', makeApp(adrs('0007-a.md', '7-b.md')));
  assert.equal(f.filter((x) => /not four digits/.test(x.message)).length, 1);
  assert.equal(f.filter((x) => /used by 2 files/.test(x.message)).length, 2);
});
