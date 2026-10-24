import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseFunctions } from '../src/checks/C35-parser-hardening.mjs';
import { findingsOf, makeApp, messages } from './helpers.mjs';

const crate = (name, lib, extra = {}) => ({ [`crates/${name}/Cargo.toml`]: `[package]\nname = "${name}"\n`, [`crates/${name}/src/lib.rs`]: lib, ...extra });
const PARSER = 'pub fn parse_line(input: &str) -> Option<Line> { None }\n';

test('parseFunctions finds public parsers over text, bytes and readers', () => {
  for (const sig of ['pub fn parse(s: &str)', 'pub fn decode_frame(bytes: &[u8])', 'pub fn from_str(text: String)', 'pub fn read_header<R: Read>(r: impl std::io::Read)', 'pub fn parse_file(path: &Path)', 'pub fn from_bytes(b: Vec<u8>)']) {
    assert.equal(parseFunctions(`${sig} -> X {}`).length, 1, sig);
  }
  for (const sig of ['fn parse(s: &str)', 'pub fn parse(n: u32)', 'pub fn render(s: &str)', 'pub fn parsed(x: &Foo)']) assert.equal(parseFunctions(`${sig} {}`).length, 0, sig);
  assert.deepEqual(parseFunctions(`\n\n${PARSER}`), [{ line: 3, name: 'parse_line' }]);
});

test('C35 bad: a parser crate with nothing is a warning naming the first parser', async () => {
  const f = await findingsOf('C35', makeApp(crate('log-parse', PARSER)));
  assert.equal(f.length, 1);
  assert.equal(f[0].severity, 'warn');
  assert.equal(f[0].file, 'crates/log-parse/src/lib.rs');
  assert.match(f[0].message, /log-parse parses untrusted input \(1 function\(s\), first parse_line\)/);
});

test('C35 good: a fuzz target, a never-panics test in the crate, a test elsewhere that uses the crate, proptest', async () => {
  const fuzz = { 'fuzz/Cargo.toml': '[package]\nname = "fuzz"\n', 'fuzz/fuzz_targets/fuzz_line.rs': 'fuzz_target!(|d: &[u8]| { let _ = log_parse::parse_line(std::str::from_utf8(d).unwrap_or("")); });' };
  assert.deepEqual(await findingsOf('C35', makeApp({ ...crate('log-parse', PARSER), ...fuzz })), []);
  const inside = crate('log-parse', PARSER, { 'crates/log-parse/tests/hostile.rs': 'assert!(paddock_testkit::never_panics(&corpus(SEED), |b| parse_line(b)).is_empty());' });
  assert.deepEqual(await findingsOf('C35', makeApp(inside)), []);
  const elsewhere = { ...crate('log-parse', PARSER), 'crates/app/tests/robust.rs': 'use log_parse::parse_line;\nproptest! { fn p(s in ".*") { parse_line(&s); } }' };
  assert.deepEqual(await findingsOf('C35', makeApp(elsewhere)), []);
  const unrelated = { ...crate('log-parse', PARSER), 'crates/other/tests/robust.rs': 'never_panics(&c, other::parse)' };
  assert.equal((await findingsOf('C35', makeApp(unrelated))).length, 1, 'a test of another crate does not cover this one');
});

test('C35: config names parsers and skips non-parsers; crates without parse functions are ignored; test code is not a parser', async () => {
  assert.deepEqual(await findingsOf('C35', makeApp({ ...crate('log-parse', PARSER), 'paddock.conformance.json': JSON.stringify({ app: 'demo', parserSkip: ['log-parse'] }) })), []);
  const named = await findingsOf('C35', makeApp({ ...crate('wire', 'pub fn handle(x: u8) {}\n'), 'paddock.conformance.json': JSON.stringify({ app: 'demo', parserCrates: ['wire'] }) }));
  assert.match(messages(named), /wire parses untrusted input but has no fuzz target/);
  assert.equal(named[0].file, 'crates/wire/Cargo.toml');
  assert.deepEqual(await findingsOf('C35', makeApp(crate('plain', 'pub fn add(a: u8) {}\n'))), []);
  assert.deepEqual(await findingsOf('C35', makeApp(crate('t', 'fn x() {}\n', { 'crates/t/tests/helper.rs': PARSER, 'crates/t/src/tests.rs': PARSER }))), []);
});

test('C35 handles a crate at the repository root', async () => {
  const f = await findingsOf('C35', makeApp({ 'Cargo.toml': '[package]\nname = "root-parse"\n', 'src/lib.rs': PARSER }));
  assert.equal(f.length, 1);
  assert.equal(f[0].file, 'src/lib.rs');
});
