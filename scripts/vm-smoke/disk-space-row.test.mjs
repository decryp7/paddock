import test from 'node:test';
import assert from 'node:assert/strict';
import { judgeDisk, parseDisks, printedFree, diskSpaceStep, TITLE_DISK } from './disk-space-row.mjs';

const GB = 1e9;
const disks = [{ name: '/', freeBytes: 50 * GB, totalBytes: 100 * GB }];
const text = 'CPU 3%\nDisk      /: 50.0 GB free of 100.0 GB\nNetwork ...\n';
const base = { disks, volumeName: '/', osFreeBytes: 50.2 * GB, osSource: 'df', statusText: text, baselineFree: 50 * GB, afterWriteFree: 49 * GB, writeSecs: 6, afterDeleteFree: 50 * GB, deleteSecs: 6 };

test('parseDisks reads the JSON and rejects other text', () => {
  assert.equal(parseDisks(JSON.stringify({ disks })).length, 1);
  assert.equal(parseDisks('not json'), null);
});
test('printedFree reads the decimal line', () => {
  assert.equal(printedFree(text, '/').freeUnit, 'GB');
  assert.equal(printedFree(text, '/nope'), null);
});
test('a matching, decimal, refreshing disk passes', () => assert.ok(judgeDisk(base).every((c) => c.ok)));
test('more than 1 % apart from the OS fails', () => assert.ok(judgeDisk({ ...base, osFreeBytes: 60 * GB }).some((c) => !c.ok && /apart/.test(c.detail))));
test('GiB units fail', () => assert.ok(judgeDisk({ ...base, statusText: text.replace('GB free', 'GiB free') }).some((c) => !c.ok && /binary/.test(c.detail))));
test('no drop after the write fails', () => assert.ok(judgeDisk({ ...base, afterWriteFree: 50 * GB, writeSecs: 60 }).some((c) => !c.ok && /fell by only/.test(c.detail))));
test('an unreadable OS figure is a broken check, not a pass', () => assert.ok(judgeDisk({ ...base, osFreeBytes: NaN }).some((c) => !c.ok && c.kind === 'check')));
test('the step runs against a fake guest', async () => {
  let free = 50 * GB, t = 0;
  const exec = async (c) => {
    if (c === 'json') return { code: 0, stdout: JSON.stringify({ disks: [{ name: '/', freeBytes: free, totalBytes: 100 * GB }] }) };
    if (c === 'text') return { code: 0, stdout: text };
    if (c === 'write') { free -= GB; return { code: 0, stdout: '' }; }
    if (c === 'del') { free += GB; return { code: 0, stdout: '' }; }
    return { code: 0, stdout: '' };
  };
  const adapter = { volumeName: '/', statusJsonCmd: 'json', statusTextCmd: 'text', writeCmd: 'write', deleteCmd: 'del', osFree: async () => ({ bytes: free, source: 'fake' }) };
  const r = await diskSpaceStep({ exec, sleep: async (ms) => { t += ms; }, now: () => t, adapter });
  assert.equal(r.title, TITLE_DISK);
  assert.equal(r.status, 'pass');
});
test('a failing write is not measured', async () => {
  const adapter = { volumeName: '/', statusJsonCmd: 'json', statusTextCmd: 'text', writeCmd: 'write', deleteCmd: 'del', osFree: async () => ({ bytes: 50 * GB, source: 'fake' }) };
  const exec = async (c) => (c === 'json' ? { code: 0, stdout: JSON.stringify({ disks }) } : c === 'write' ? { code: 1, stdout: 'no space' } : { code: 0, stdout: text });
  const r = await diskSpaceStep({ exec, sleep: async () => {}, now: () => 0, adapter });
  assert.equal(r.status, 'not-measured');
});
test('when the OS figure itself did not fall, the refresh checks are broken checks, not a fail of Dyno', () => {
  const cs = judgeDisk({ ...base, afterWriteFree: 49.9 * GB, writeSecs: 60, afterDeleteFree: 49.9 * GB, deleteSecs: 60, osBaseline: 50 * GB, osAfterWrite: 49.95 * GB });
  assert.ok(cs.filter((c) => !c.ok).every((c) => c.kind === 'check'));
  assert.ok(cs.some((c) => !c.ok));
});
