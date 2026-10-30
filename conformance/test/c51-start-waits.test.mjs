import assert from 'node:assert/strict';
import { test } from 'node:test';
import { startWaits } from '../src/checks/C51-start-waits.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const lines = (t) => startWaits(t).map((s) => s.line);

test('C51 flags a fixed start constant, field and a wait inside a start function', () => {
  assert.deepEqual(lines('pub const START_LIMIT: Duration = Duration::from_secs(20);\n'), [1]);
  assert.deepEqual(lines('struct O {\n    pub spawn_timeout: Duration,\n    pub ready_deadline: Option<Duration>,\n}\n'), [2, 3]);
  assert.deepEqual(lines('fn start_agent() {\n    let end = Deadline::after(clock, d);\n    rx.recv_timeout(d);\n}\nfn other() {\n    rx.recv_timeout(d);\n}\n'), [2, 3]);
  assert.deepEqual(lines('fn launch() {\n    let t = Instant::now() + d;\n}\n'), [2]);
  assert.deepEqual(lines('static SPAWN_WAIT: std::time::Duration = D;\n'), [1]);
});

test('C51 passes waived sites, the judge, other names and test modules', () => {
  assert.deepEqual(lines('// cap: the render allowance a user sets in Settings\npub const START_LIMIT: Duration = D;\n'), []);
  assert.deepEqual(lines('pub const START_LIMIT: Duration = D; // external: the OS decides\n'), []);
  assert.deepEqual(lines('fn start() {\n    // pace: a poll interval, bounds nothing\n    rx.recv_timeout(POLL);\n}\n'), []);
  assert.deepEqual(lines('let p: Patience = Patience::default(); // START_LIMIT: Duration\n'), []);
  assert.deepEqual(lines('const POLL: Duration = D;\nconst CLOSE_GRACE: Duration = D;\npub start_page: String,\n'), []);
  assert.deepEqual(lines('// const START_LIMIT: Duration = D;\nfn a() { let s = "START_LIMIT: Duration"; }\n'), []);
  assert.deepEqual(lines('fn a() {}\n#[cfg(test)]\nmod tests {\n    const START_LIMIT: Duration = D;\n    fn start() { rx.recv_timeout(d); }\n}\n'), []);
  assert.deepEqual(lines('fn start() {\n}\nfn later() {\n    rx.recv_timeout(d);\n}\n'), []);
});

test('C51 through the runner: warnings only, for source in scope only', async () => {
  const bad = 'pub const START_LIMIT: Duration = D;\n';
  const f = await findingsOf(
    'C51',
    makeApp({
      'crates/a/src/lib.rs': bad,
      'apps/x/crates/y/src/lib.rs': bad,
      'apps/x/app/src-tauri/src/l.rs': bad,
      'tools/t/src/main.rs': bad,
      'crates/a/tests/t.rs': bad,
      'crates/a/examples/e.rs': bad,
      'ui/other.rs': bad,
      'crates/a/src/ok.rs': '// cap: product rule\nconst START_LIMIT: Duration = D;\n',
    }),
  );
  assert.deepEqual(f.map((x) => x.file).sort(), ['apps/x/app/src-tauri/src/l.rs', 'apps/x/crates/y/src/lib.rs', 'crates/a/src/lib.rs', 'tools/t/src/main.rs']);
  assert.ok(f.every((x) => x.severity === 'warn' && /StartJudge/.test(x.fix)));
});
