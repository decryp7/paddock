import assert from 'node:assert/strict';
import { test } from 'node:test';
import { busyReads } from '../src/checks/C60-busy-wait-read.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const lines = (text) => busyReads(text).map((p) => p.line);

test('C60 bad: read -t on /dev/zero, /dev/null or an ended pipe', () => {
  assert.deepEqual(lines('while true; do read -t 20 < /dev/zero; done'), [1]);
  assert.deepEqual(lines('until check; do\n  read -t 1 _ </dev/null\ndone'), [2]);
  assert.deepEqual(lines('read -r -t 5 x 0</dev/null'), [1]);
  assert.deepEqual(lines('read -t1 x < "/dev/null"'), [1]);
  assert.deepEqual(lines('read -rt 0.5 x </dev/zero'), [1]);
  assert.deepEqual(lines(': | read -t 3 x'), [1]);
  assert.deepEqual(lines('echo | read -t 3 x'), [1]);
  assert.deepEqual(lines('yes | read -t 3 x'), [1]);
  assert.deepEqual(lines('cat /dev/zero | read -t 3 x'), [1]);
});

test('C60 bad: the embedded shell of a Rust or TS test helper', () => {
  assert.deepEqual(lines('    let script = "while true; do read -t 1 _ </dev/null; done";'), [1]);
  assert.deepEqual(lines('const sh = `i=0\\nwhile [ $i -lt 9 ]; do read -t 1 _ < /dev/zero\\ni=$((i+1))\\ndone`;'), [1]);
});

test('C60 good: a fifo, a terminal prompt, a file, an inherited stdin, a poll, a real sleep', () => {
  assert.deepEqual(lines('read -t 5 line < "$FIFO"'), []);
  assert.deepEqual(lines('read -t 30 -p "Continue? " answer'), []);
  assert.deepEqual(lines('read -t 5 x < /dev/tty'), []);
  assert.deepEqual(lines('read -t 5 x < input.txt'), []);
  assert.deepEqual(lines('tail -f log | read -t 5 x'), []);
  assert.deepEqual(lines('read -t 0 x < /dev/null'), []);
  assert.deepEqual(lines('read x < /dev/null'), []);
  assert.deepEqual(lines('sleep 20'), []);
  assert.deepEqual(lines("perl -e 'select(undef,undef,undef,20)'"), []);
  assert.deepEqual(lines('# read -t 20 < /dev/zero is the bug\n// read -t 1 _ </dev/null'), []);
});

test('C60 waivers: with a reason the site passes, without one it is reported', () => {
  assert.equal(busyReads('# conformance: busy-wait-read — the stream is closed on purpose in this fixture\nread -t 1 x </dev/null')[0].waived, 'reason');
  assert.equal(busyReads('read -t 1 x </dev/null # conformance: busy-wait-read')[0].waived, 'no-reason');
});

test('C60 through the runner: shell, workflow and test-helper files, error per site', async () => {
  const f = await findingsOf(
    'C60',
    makeApp({
      'scripts/wait.sh': '#!/bin/bash\nwhile :; do read -t 20 < /dev/zero; done\n',
      '.gitea/workflows/x.yml': 'jobs:\n  a:\n    steps:\n      - run: |\n          until ok; do read -t 1 _ </dev/null; done\n',
      'crates/p/src/process.rs': 'const SH: &str = "while true; do read -t 1 _ </dev/null; done";\n',
      'scripts/good.sh': 'read -t 5 line < "$FIFO"\nsleep 1\n',
      'scripts/waived.sh': '# conformance: busy-wait-read — EOF is the point of this fixture\nread -t 1 x </dev/null\n',
      'notes.md': 'read -t 1 </dev/null\n',
    }),
  );
  assert.deepEqual(f.map((x) => [x.file, x.severity]).sort(), [
    ['.gitea/workflows/x.yml', 'error'],
    ['crates/p/src/process.rs', 'error'],
    ['scripts/wait.sh', 'error'],
  ]);
  assert.match(f[0].fix, /sleep/);
});
