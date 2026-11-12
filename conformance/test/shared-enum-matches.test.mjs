// A shared enum's variants grow (Dyno WP-8 adds `Interrupted` to the job states); every app's non-test `match` on it must then fail to
// compile instead of falling into a catch-all arm. The scanner is tested on fixtures, the repository against a shrink-only baseline.
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { RUST_TEST_FILE } from '../src/checks/C71-test-timing.mjs';
import { enumMatches } from '../src/util/shared-enum-matches.mjs';
import { enforceShrinkOnly } from '../src/util/baseline.mjs';
import { openTree } from '../src/util/fs.mjs';
import { PADDOCK } from './helpers.mjs';

const NAMES = ['JobState', 'WireState'];
const found = (src) => enumMatches(src, NAMES).map((m) => [m.line, m.catchAllLine]);

// Dyno's popover, as it matches the states today (a catch-all arm) and as WP-8 needs it once `Interrupted` exists.
const TODAY = `fn label(s: &WireState) -> &'static str {
    match s {
        WireState::Queued => "Waiting",
        WireState::Running(_) => "Working",
        WireState::Done => "Done",
        _ => "Stopped",
    }
}
`;
const WP8_BAD = `fn label(s: &WireState) -> &'static str {
    match s {
        WireState::Queued => "Waiting",
        WireState::Running(_) => "Working",
        WireState::Done => "Done",
        WireState::Interrupted => "Interrupted",
        other => describe(other),
    }
}
`;
const WP8_GOOD = `fn label(s: &WireState) -> &'static str {
    match s {
        WireState::Queued => "Waiting",
        WireState::Running(_) => "Working",
        WireState::Done => "Done",
        WireState::Failed { .. } | WireState::Cancelled(_) => "Stopped",
        WireState::Interrupted => "Interrupted",
    }
}
`;

test('a catch-all arm (`_` or a bare binding) on a shared enum is reported at its line, with the enum and its variants', () => {
  assert.deepEqual(found(TODAY), [[2, 6]]);
  assert.deepEqual(found(WP8_BAD), [[2, 7]]);
  const m = enumMatches(WP8_BAD, NAMES)[0];
  assert.equal(m.enumName, 'WireState');
  assert.deepEqual(m.variants, ['Queued', 'Running', 'Done', 'Interrupted']);
});

test('an exhaustive match (Interrupted handled by name) is listed and not reported', () => {
  assert.deepEqual(found(WP8_GOOD), [[2, 0]]);
});

test('guarded wildcards, nested matches, other enums, strings, comments and test code are read correctly', () => {
  assert.deepEqual(found('fn a(s: JobState) {\n    match s {\n        JobState::Done => 1,\n        x if x.is_final() => 2,\n        JobState::Queued | JobState::Running { .. } => 3,\n        JobState::Failed { .. } | JobState::Cancelled { .. } => 4,\n    }\n}\n'), [[2, 0]], 'a guarded binding is not a catch-all');
  assert.deepEqual(found('fn a(o: Option<WireState>) {\n    match o {\n        Some(WireState::Done) => {}\n        Some(_) => {}\n        None => {}\n    }\n}\n'), [[2, 4]], 'Some(_) takes every state inside Some: a catch-all of the enum');
  assert.deepEqual(found('fn a(o: Option<WireState>) {\n    match o {\n        Some(WireState::Done) => {}\n        _ => {}\n    }\n}\n'), [[2, 4]]);
  // The two real ones the review found: Dyno's cli.rs (`Some(_) => {}` on WireState) and the link's `Err(e)` on IpcError.
  assert.deepEqual(found('fn a(s: Option<&WireState>) {\n    match s {\n        Some(WireState::Done) => break,\n        Some(WireState::Failed { message, .. }) => return,\n        Some(_) => {}\n        None => return,\n    }\n}\n'), [[2, 5]]);
  assert.deepEqual(enumMatches('fn a(r: Result<M, IpcError>) {\n    match r {\n        Ok(Message::Reply(x)) => 1,\n        Ok(_) => 2,\n        Err(IpcError::Timeout) => 3,\n        Err(IpcError::Closed(_)) => 4,\n        Err(e) => 5,\n    }\n}\n', ['IpcError']).map((m) => [m.line, m.catchAllLine]), [[2, 7]], 'Err(e) is flagged; Ok(_) is not (Ok names no variant of the enum)');
  assert.deepEqual(found('fn a(k: Kind) {\n    match k {\n        Kind::A => {}\n        _ => {}\n    }\n}\n'), [], 'another enum');
  assert.deepEqual(found('// match s { WireState::Done => 1, _ => 2 }\nfn a() { let t = "match s { WireState::Done => 1, _ => 2 }"; }\n'), []);
  assert.deepEqual(found('fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t(s: WireState) {\n        match s {\n            WireState::Done => {}\n            _ => {}\n        }\n    }\n}\n'), [], 'test code');
  assert.deepEqual(found('fn a(s: JobState, t: JobState) {\n    match s {\n        JobState::Done => match t {\n            JobState::Done => 1,\n            _ => 2,\n        },\n        JobState::Queued => 3,\n        _ => 4,\n    }\n}\n').map(([l, c]) => [l, c]).sort(), [[2, 8], [3, 5]], 'inner and outer are two matches');
  assert.deepEqual(enumMatches(TODAY, NAMES, { wholeFileIsTest: true }), []);
});

// The shared enums and the apps' matches against the list in shared-enums.json.
const config = JSON.parse(readFileSync(join(PADDOCK, 'conformance', 'shared-enums.json'), 'utf8'));
const BASELINE = join(PADDOCK, 'conformance', 'baselines', 'shared-enum-matches.json');

test('every listed shared enum is defined where the list says (the probe sees what it looks for)', () => {
  for (const e of config.enums) {
    const text = readFileSync(join(PADDOCK, e.definedIn), 'utf8');
    assert.match(text, new RegExp(`pub enum ${e.name}\\b`), `${e.name} is not defined in ${e.definedIn}`);
  }
});

test('no app match on a shared enum has a catch-all arm, except the listed ones that only shrink', () => {
  const tree = openTree(PADDOCK);
  const crates = [...new Set(config.enums.map((e) => e.crate))];
  const names = config.enums.map((e) => e.name);
  const all = [];
  for (const file of tree.files().filter((f) => /^apps\/.+\.rs$/.test(f) && !/(?:^|\/)(?:vendor|target)\//.test(f) && !RUST_TEST_FILE.test(f))) {
    const text = tree.read(file);
    if (!text || !crates.some((c) => text.includes(c))) continue;
    for (const m of enumMatches(text, names)) all.push({ file, ...m, key: `${file}:${text.split('\n')[m.catchAllLine - 1]?.trim() ?? ''}` });
  }
  assert.ok(all.length > 0, 'check broken: no app match on a shared enum was found at all');
  const bad = all.filter((m) => m.catchAllLine);
  const listed = existsSync(BASELINE) ? JSON.parse(readFileSync(BASELINE, 'utf8')).sites : [];
  const left = new Map();
  for (const k of listed) left.set(k, (left.get(k) ?? 0) + 1);
  const fresh = [];
  for (const m of bad) {
    if ((left.get(m.key) ?? 0) > 0) left.set(m.key, left.get(m.key) - 1);
    else fresh.push(`${m.file}:${m.catchAllLine}  match on ${m.enumName} (${m.variants.join(', ')}) ends in a catch-all arm`);
  }
  assert.deepEqual(fresh, [], 'Next step: list the variants (a new one must break this match at compile time), or if the default is meant, say so in a variant-by-variant arm.');
  const gone = [...left].filter(([, n]) => n > 0).map(([k]) => k);
  // Fixed entries only warn (remove them); entries the merge base's list lacks fail (conformance/src/util/baseline.mjs).
  enforceShrinkOnly({ root: PADDOCK, path: 'conformance/baselines/shared-enum-matches.json', current: listed, gone, keyOf: (k) => k, entriesOf: (j) => j.sites });
});
