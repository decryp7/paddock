import assert from 'node:assert/strict';
import { test } from 'node:test';
import { baseWaivers, pidFields, waiverKeys } from '../src/checks/C134-wire-process-ref.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const S = '#[derive(Debug, Clone, Serialize, Deserialize)]\n';
const fields = (text) => pidFields(text).map((f) => [f.item, f.field, f.line, f.waived]);
const names = (text) => pidFields(text).map((f) => f.field);

test('C134 bad: a pid, pids or *_pid field of a serializing struct, enum variant or multi-line derive', () => {
  assert.deepEqual(fields(`${S}pub struct A {\n    pub pid: u32,\n    pub name: String,\n}\n`), [['A', 'pid', 3, null]]);
  assert.deepEqual(fields(`${S}pub struct G {\n    /// Members.\n    pub pids: Vec<u32>,\n    pub via_pid: Option<u32>,\n}\n`), [['G', 'pids', 4, null], ['G', 'via_pid', 5, null]]);
  assert.deepEqual(fields(`#[derive(\n    Debug,\n    Serialize,\n)]\npub enum O {\n    Process { pid: u32, name: String },\n    Unknown { reason: String },\n}\n`), [['O', 'pid', 6, null]]);
});

test('C134 bad: the names parent, ppid and process_id, a camel or upper case pid, and a serde rename to one of them', () => {
  assert.deepEqual(names(`${S}pub struct A {\n    pub parent: Option<u32>,\n    pub ppid: u32,\n    pub process_id: u32,\n}\n`), ['parent', 'ppid', 'process_id']);
  assert.deepEqual(names(`${S}pub struct A {\n    pub parentPid: u32,\n    pub PID: u32,\n}\n`), ['parentPid', 'PID']);
  assert.deepEqual(names(`${S}pub struct A {\n    #[serde(rename = "pid")]\n    pub who: u32,\n    pub other: u32,\n}\n`), ['who']);
});

test('C134 bad: a field of the Pid type under any name, in a tuple struct or variant, and a serialisation by cfg_attr or by hand', () => {
  assert.deepEqual(names(`${S}pub struct A {\n    pub who: Pid,\n    pub maybe: Option<Pid>,\n    pub all: Vec<Pid>,\n}\n`), ['who', 'maybe', 'all']);
  assert.deepEqual(names(`${S}pub struct T(pub Pid, pub String);\n`), ['(tuple field)']);
  assert.deepEqual(names(`${S}pub enum E {\n    Proc(Pid),\n    Other(String),\n}\n`), ['(tuple field)']);
  assert.deepEqual(names('#[cfg_attr(feature = "x", derive(Serialize))]\npub struct A {\n    pub pid: u32,\n}\n'), ['pid']);
  assert.deepEqual(names('pub struct A {\n    pub pid: u32,\n}\nimpl Serialize for A { }\n'), ['pid']);
  assert.deepEqual(names('#[derive(Debug, serde::Serialize)]\npub struct A {\n    pub pid: u32,\n}\n'), ['pid']);
});

test('C134 bad: a Pid or numeric parent whose type wraps onto the next line, in any layout rustfmt makes', () => {
  assert.deepEqual(names(`${S}pub struct A {\n    pub who:\n        Pid,\n}\n`), ['who']);
  assert.deepEqual(names(`${S}pub struct A {\n    pub all: Vec<\n        Pid,\n    >,\n}\n`), ['all']);
  assert.deepEqual(names(`${S}pub struct A {\n    pub parent:\n        Option<u32>,\n}\n`), ['parent']);
});

test('C134 good: names that only end in the letters pid (rapid, vapid, cupid) and a parent that is not a number', () => {
  assert.deepEqual(names(`${S}pub struct A {\n    pub rapid: bool,\n    pub vapid: u32,\n    pub cupid: String,\n    pub Cupid: u8,\n}\n`), []);
  assert.deepEqual(names(`${S}pub struct A {\n    pub parent: Option<String>,\n    pub parent_folder: PathBuf,\n}\n`), []);
  assert.deepEqual(names(`${S}pub struct A {\n    pub parent:\n        Option<PathBuf>,\n}\n`), []);
  // the real shapes still count: snake and camel suffixes, and a numeric parent
  assert.deepEqual(names(`${S}pub struct A {\n    pub via_pid: u32,\n    pub parentPid: u32,\n    pub parent: Option<u32>,\n}\n`), ['via_pid', 'parentPid', 'parent']);
});

test('C134 good: ProcessRef holds its pid, a type that does not serialize, a function argument, a comment and the test module', () => {
  assert.deepEqual(fields(`${S}pub struct ProcessRef {\n    pub pid: u32,\n    pub started: Option<i64>,\n}\n`), []);
  assert.deepEqual(names(`${S}pub struct ProcessRef {\n    pub pid: u32,\n    pub extra_pid: u32,\n}\n`), ['extra_pid']);
  assert.deepEqual(fields('#[derive(Debug, Clone)]\npub struct Raw {\n    pub pid: u32,\n}\n'), []);
  assert.deepEqual(fields(`${S}pub struct A {\n    // the pid: u32 is in process\n    pub process: ProcessRef,\n}\nimpl A {\n    pub fn find(pid: u32) {}\n}\n`), []);
  assert.deepEqual(fields(`${S}pub struct A {\n    pub process: ProcessRef,\n}\n#[cfg(test)]\nmod tests {\n    #[derive(Serialize)]\n    struct B { pid: u32 }\n}\n`), []);
});

test('C134: a #[cfg(test)] item before the wire types does not hide them, and a string with slashes is not a comment', () => {
  assert.deepEqual(names(`#[cfg(test)]\nuse x;\n${S}pub struct A {\n    pub pid: u32,\n}\n`), ['pid']);
  assert.deepEqual(names(`${S}pub struct A {\n    #[serde(default = "x//y")] pub pid: u32,\n}\n`), ['pid']);
});

test('C134: a waiver is a plain // comment with a reason (same line or the comment line above), never a doc comment', () => {
  const t = (c) => `${S}pub struct A {\n    ${c}\n    pub pid: u32,\n}\n`;
  assert.deepEqual(fields(t('// conformance: wire-pid — the sample whose reference() is a ProcessRef')).map((f) => f[3]), ['the sample whose reference() is a ProcessRef']);
  assert.deepEqual(fields(t('// conformance: wire-pid')).map((f) => f[3]), ['no-reason']);
  assert.deepEqual(fields(t('/// conformance: wire-pid — a doc comment is no waiver')).map((f) => f[3]), [null]);
  assert.deepEqual(fields(`${S}pub struct A {\n    // conformance: wire-pid — reason one\n    pub process: ProcessRef,\n    pub pid: u32,\n}\n`).map((f) => f[3]), [null]);
  assert.deepEqual(fields(`${S}pub struct A {\n    pub process: ProcessRef,\n}\npub struct B {\n    pub pid: u32,\n}\n`), []);
});

test('C134: the waivers may only shrink against the merge base (ER-46)', () => {
  const file = 'apps/dyno/crates/dyno-core/src/x.rs';
  const waived = (list) => list.map((n) => `    // conformance: wire-pid — a reason\n    pub ${n}: u32,\n`).join('');
  const src = (list) => `${S}pub struct A {\n${waived(list)}}\n`;
  const git = (base) => (args) => {
    if (args[0] === 'rev-parse') return 'head1\n';
    if (args[0] === 'merge-base') return 'base1\n';
    if (args[0] === 'ls-tree') return `${file}\n`;
    if (args[0] === 'show') return base;
    throw new Error(`unexpected ${args.join(' ')}`);
  };
  assert.deepEqual(baseWaivers('/x', git(src(['pid'])), 'origin/main').keys, [`${file}::A::pid`]);
  assert.equal(baseWaivers('/x', git(`${S}pub struct A {\n    pub process: ProcessRef,\n}\n`), 'origin/main').first, true);
  assert.deepEqual(waiverKeys(file, src(['pid', 'ppid'])), [`${file}::A::pid`, `${file}::A::ppid`]);
  assert.match(baseWaivers('/x', () => { throw new Error('no git'); }).reason, /no merge base/);
});

test('C134: report-only, a finding names the type and the field, a waived one is silent, no wire crate is check broken', async () => {
  const dir = 'apps/dyno/crates/dyno-core/src';
  const root = makeApp({
    [`${dir}/a.rs`]: `${S}pub struct A {\n    pub pid: u32,\n}\n`,
    [`${dir}/b.rs`]: `${S}pub struct B {\n    // conformance: wire-pid — the helper's table carries a start time next to it\n    pub pid: u32,\n}\n`,
  });
  const f = (await findingsOf('C134', root, { app: 'paddock' })).filter((x) => !/shrink-only rule is not enforced/.test(x.message));
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'warn');
  assert.equal(f[0].file, `${dir}/a.rs`);
  assert.match(f[0].message, /`A` has a wire field `pid`/);
  const none = await findingsOf('C134', makeApp({ 'x.txt': 'x' }), { app: 'paddock' });
  assert.equal(none.length, 1);
  assert.equal(none[0].unwaivable, true);
  assert.match(none[0].message, /check broken/);
});
