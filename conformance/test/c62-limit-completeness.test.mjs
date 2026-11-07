import assert from 'node:assert/strict';
import { test } from 'node:test';
import { caseIds, limitConstants, rowIds } from '../src/checks/C62-limit-completeness.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const ROWS = 'pub const A: LimitRow = LimitRow::stop("net.body", Quantity::Bytes(1), Quantity::Bytes(1), Quantity::Bytes(2), RowValue::Hard, Remedy::None);\nconst B: LimitRow = LimitRow { id: LimitId::new("net.redirects"), unit: Unit::Count };\n';
const CASES = 'limit_cases! { REGISTRY;\n  body: "net.body" => stop(got = x, limit = y, setting = None),\n  redirects: "net.redirects" => cut(at = a, over = b, of = "rows", shown = 1, total = t, selection = s),\n}\n';
const crate = (name, files) => Object.fromEntries(Object.entries({ 'Cargo.toml': `[package]\nname = "${name}"\n`, ...files }).map(([k, v]) => [`crates/${name}/${k}`, v]));

test('the pieces: row ids, case ids and limit-shaped constants are read from source', () => {
  assert.deepEqual(rowIds(ROWS), [{ id: 'net.body', line: 1 }, { id: 'net.redirects', line: 2 }]);
  assert.deepEqual(caseIds(CASES), ['net.body', 'net.redirects']);
  assert.deepEqual(caseIds('limit_case!(stop, REGISTRY, "ipc.frame", got = x)'), ['ipc.frame']);
  assert.deepEqual(limitConstants('pub const MAX_ENTRIES: u64 = 1;\nconst CALL_TIMEOUT: Duration = D;\nconst NAME: u8 = 1;\nstatic MAX_X: u8 = 1;'), ['MAX_ENTRIES', 'CALL_TIMEOUT']);
});

test('C62 good: every row has a case', async () => {
  const app = makeApp(crate('paddock-net', { 'src/limits.rs': ROWS + `#[cfg(test)] mod tests { ${CASES} }` }));
  assert.deepEqual(await findingsOf('C62', app), []);
});

test('C62 bad: a row without a case is a warning naming the row and the file', async () => {
  const app = makeApp(crate('paddock-net', { 'src/limits.rs': ROWS + 'limit_cases! { REGISTRY;\n body: "net.body" => stop(got = x),\n}\n' }));
  const f = await findingsOf('C62', app);
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'warn');
  assert.equal(f[0].file, 'crates/paddock-net/src/limits.rs');
  assert.match(messages(f), /limit row "net\.redirects" has no limit_cases! case in crates\/paddock-net/);
});

test('C62: a case in another file of the crate counts; a case in another crate does not', async () => {
  const inCrate = makeApp(crate('paddock-net', { 'src/limits.rs': ROWS, 'tests/it/limits.rs': CASES }));
  assert.deepEqual(await findingsOf('C62', inCrate), []);
  const elsewhere = makeApp({ ...crate('paddock-net', { 'src/limits.rs': ROWS }), ...crate('paddock-other', { 'tests/it/limits.rs': CASES }) });
  assert.equal((await findingsOf('C62', elsewhere)).length, 2);
});

test('C62: limit-shaped constants no registry file mentions are one warning per crate; waivers and mentions silence them', async () => {
  const files = { 'src/limits.rs': ROWS + CASES, 'src/lib.rs': 'pub const MAX_NAME: usize = 1;\npub const CALL_TIMEOUT: u64 = 2;\npub const MAX_BODY: u64 = 3;\n' };
  const f = await findingsOf('C62', makeApp(crate('paddock-net', files)));
  assert.equal(f.length, 1);
  assert.match(f[0].message, /3 limit-shaped constant\(s\) no registry file mentions \(first: MAX_NAME, CALL_TIMEOUT, MAX_BODY\)/);
  const waived = { ...crate('paddock-net', files), 'paddock.conformance.json': JSON.stringify({ app: 'demo', limitWaivers: ['MAX_NAME', 'CALL_TIMEOUT'] }) };
  assert.match(messages(await findingsOf('C62', makeApp(waived))), /1 limit-shaped constant\(s\).*first: MAX_BODY/);
  const mentioned = crate('paddock-net', { ...files, 'src/limits.rs': ROWS + CASES + '// MAX_NAME CALL_TIMEOUT MAX_BODY\n' });
  assert.deepEqual(await findingsOf('C62', makeApp(mentioned)), []);
});

test('C62: a crate with no registry, a skipped crate and fuzz folders are ignored', async () => {
  assert.deepEqual(await findingsOf('C62', makeApp(crate('plain', { 'src/lib.rs': 'pub const MAX_X: u64 = 1;\n' }))), []);
  const skipped = { ...crate('paddock-net', { 'src/limits.rs': ROWS }), 'paddock.conformance.json': JSON.stringify({ app: 'demo', limitSkip: ['paddock-net'] }) };
  assert.deepEqual(await findingsOf('C62', makeApp(skipped)), []);
});

test('C62: a row declared in a test module or under tests/ is a fixture', async () => {
  const app = makeApp(crate('paddock-net', { 'src/lib.rs': `#[cfg(test)]\nmod tests {\n${ROWS}\n}\n`, 'tests/it/fixtures.rs': ROWS, 'src/tests.rs': ROWS }));
  assert.deepEqual(await findingsOf('C62', app), []);
});
