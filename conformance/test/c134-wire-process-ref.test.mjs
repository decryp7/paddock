import assert from 'node:assert/strict';
import { test } from 'node:test';
import { pidFields } from '../src/checks/C134-wire-process-ref.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const S = '#[derive(Debug, Clone, Serialize, Deserialize)]\n';
const fields = (text) => pidFields(text).map((f) => [f.item, f.field, f.line, f.waived]);

test('C134 bad: a pid, pids or *_pid field of a serializing struct, enum variant or multi-line derive', () => {
  assert.deepEqual(fields(`${S}pub struct A {\n    pub pid: u32,\n    pub name: String,\n}\n`), [['A', 'pid', 3, null]]);
  assert.deepEqual(fields(`${S}pub struct G {\n    /// Members.\n    pub pids: Vec<u32>,\n    pub via_pid: Option<u32>,\n}\n`), [['G', 'pids', 4, null], ['G', 'via_pid', 5, null]]);
  assert.deepEqual(fields(`#[derive(\n    Debug,\n    Serialize,\n)]\npub enum O {\n    Process { pid: u32, name: String },\n    Unknown { reason: String },\n}\n`), [['O', 'pid', 6, null]]);
});

test('C134 good: ProcessRef itself, a type that does not serialize, a function argument, a comment and the test module', () => {
  assert.deepEqual(fields(`${S}pub struct ProcessRef {\n    pub pid: u32,\n    pub started: Option<i64>,\n}\n`), []);
  assert.deepEqual(fields('#[derive(Debug, Clone)]\npub struct Raw {\n    pub pid: u32,\n}\n'), []);
  assert.deepEqual(fields(`${S}pub struct A {\n    // the pid: u32 is in process\n    pub process: ProcessRef,\n}\nimpl A {\n    pub fn find(pid: u32) {}\n}\n`), []);
  assert.deepEqual(fields(`${S}pub struct A {\n    pub process: ProcessRef,\n}\n#[cfg(test)]\nmod tests {\n    #[derive(Serialize)]\n    struct B { pid: u32 }\n}\n`), []);
});

test('C134: a waiver needs a reason (same line or the comment line above); a state after an item does not leak into the next', () => {
  const t = (c) => `${S}pub struct A {\n    // ${c}\n    pub pid: u32,\n}\n`;
  assert.deepEqual(fields(t('conformance: wire-pid — the sample whose reference() is a ProcessRef')).map((f) => f[3]), ['the sample whose reference() is a ProcessRef']);
  assert.deepEqual(fields(t('conformance: wire-pid')).map((f) => f[3]), ['no-reason']);
  assert.deepEqual(fields(`${S}pub struct A {\n    pub process: ProcessRef,\n}\npub struct B {\n    pub pid: u32,\n}\n`), []);
});

test('C134: report-only, a finding names the type and the field, a waived one is silent, no wire crate is check broken', async () => {
  const dir = 'apps/dyno/crates/dyno-core/src';
  const root = makeApp({
    [`${dir}/a.rs`]: `${S}pub struct A {\n    pub pid: u32,\n}\n`,
    [`${dir}/b.rs`]: `${S}pub struct B {\n    pub pid: u32, // conformance: wire-pid — the helper's table carries a start time next to it\n}\n`,
  });
  const f = await findingsOf('C134', root, { app: 'paddock' });
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'warn');
  assert.equal(f[0].file, `${dir}/a.rs`);
  assert.match(f[0].message, /`A` has a wire field `pid`/);
  const none = await findingsOf('C134', makeApp({ 'x.txt': 'x' }), { app: 'paddock' });
  assert.equal(none.length, 1);
  assert.equal(none[0].unwaivable, true);
  assert.match(none[0].message, /check broken/);
});
