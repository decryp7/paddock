import assert from 'node:assert/strict';
import { test } from 'node:test';
import { busyReads, kindOf } from '../src/checks/C60-busy-wait-read.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const sh = (text) => busyReads(text, 'shell').map((p) => p.line);
const code = (text) => busyReads(text, 'code').map((p) => p.line);

test('C60 bad: read -t on /dev/zero, /dev/null or an ended pipe', () => {
  assert.deepEqual(sh('while true; do read -t 20 < /dev/zero; done'), [1]);
  assert.deepEqual(sh('until check; do\n  read -t 1 _ </dev/null\ndone'), [2]);
  assert.deepEqual(sh('read -r -t 5 x 0</dev/null'), [1]);
  assert.deepEqual(sh('read -t1 x < "/dev/null"'), [1]);
  assert.deepEqual(sh('read -rt 0.5 x </dev/zero'), [1]);
  assert.deepEqual(sh(': | read -t 3 x'), [1]);
  assert.deepEqual(sh(':|read -t 3 x'), [1]);
  assert.deepEqual(sh('echo | read -t 3 x'), [1]);
  assert.deepEqual(sh("printf 'x\\n' | read -t 3 x"), [1]);
  assert.deepEqual(sh('yes | read -t 3 x'), [1]);
  assert.deepEqual(sh('cat /dev/zero | read -t 3 x'), [1]);
});

test('C60 bad: an option with an argument before -t does not hide the read', () => {
  assert.deepEqual(sh('read -s -n 1 -t 20 key </dev/null'), [1]);
  assert.deepEqual(sh('read -p "x" -t 5 x </dev/null'), [1]);
  assert.deepEqual(sh('read -d "" -t 5 x </dev/null'), [1]);
  assert.deepEqual(sh('read -n1 -t 5 x </dev/null'), [1]);
});

test('C60 bad: no space around the redirect', () => {
  assert.deepEqual(sh('read -t 5 x</dev/zero'), [1]);
  assert.deepEqual(sh('read -t5</dev/zero'), [1]);
});

test('C60 bad: a variable timeout is a wait', () => {
  assert.deepEqual(sh('read -t "$T" x </dev/null'), [1]);
  assert.deepEqual(sh('read -t $N x </dev/null'), [1]);
  assert.deepEqual(sh('read -t ${DELAY:-5} x </dev/null'), [1]);
});

test('C60 bad: here-strings, heredocs, a closed stdin and a trivial process substitution', () => {
  assert.deepEqual(sh('read -t 5 x <<< ""'), [1]);
  assert.deepEqual(sh('read -t 5 x <<EOF'), [1]);
  assert.deepEqual(sh('read -t 5 x <&-'), [1]);
  assert.deepEqual(sh('read -t 5 x < <(:)'), [1]);
});

test('C60 bad: a case arm is not a comment', () => {
  assert.deepEqual(sh('case $x in\n  *) read -t 1 _ </dev/null ;;\nesac'), [2]);
});

test('C60 bad: the embedded shell of a Rust or TS test helper', () => {
  assert.deepEqual(code('    let script = "while true; do read -t 1 _ </dev/null; done";'), [1]);
  assert.deepEqual(code('const sh = `i=0\\nwhile [ $i -lt 9 ]; do read -t 1 _ < /dev/zero\\ni=$((i+1))\\ndone`;'), [1]);
  assert.deepEqual(code('const sh = "echo\\n: | read -t 3 x";'), [1]);
});

test('C60 good: a fifo, a terminal prompt, a file, an inherited stdin, a poll, a real sleep', () => {
  assert.deepEqual(sh('read -t 5 line < "$FIFO"'), []);
  assert.deepEqual(sh('read -t 30 -p "Continue? " answer'), []);
  assert.deepEqual(sh('read -t 5 x < /dev/tty'), []);
  assert.deepEqual(sh('read -t 5 x < input.txt'), []);
  assert.deepEqual(sh('tail -f log | read -t 5 x'), []);
  assert.deepEqual(sh('read -t 0 x < /dev/null'), []);
  assert.deepEqual(sh('read x < /dev/null'), []);
  assert.deepEqual(sh('sleep 20'), []);
  assert.deepEqual(sh("perl -e 'select(undef,undef,undef,20)'"), []);
  assert.deepEqual(sh('# read -t 20 < /dev/zero is the bug'), []);
  assert.deepEqual(code('// read -t 1 _ </dev/null\n * read -t 1 _ </dev/null'), []);
});

test('C60 good: a redirect that is not the read\'s stdin', () => {
  assert.deepEqual(sh('read -t 5 x 2</dev/null'), []);
  assert.deepEqual(sh('read -t 5 x < "$f" & foo </dev/null'), []);
  assert.deepEqual(sh('read -t 5 x < "$f" | foo </dev/null'), []);
  assert.deepEqual(sh('read -t 5 x < "$f" # then </dev/null'), []);
  assert.deepEqual(sh('read -t 5 x < "$f" 2>&1'), []);
  // Needs the literal \n split of a Rust/TS/JS string: the second command is not the read's.
  assert.deepEqual(code('"read -t 5 x < \\"$f\\"\\nfoo </dev/null"'), []);
});

test('C60 waivers: with a reason the site passes, without one it is reported', () => {
  assert.equal(busyReads('# conformance: busy-wait-read — the stream is closed on purpose in this fixture\nread -t 1 x </dev/null')[0].waived, 'reason');
  assert.equal(busyReads('read -t 1 x </dev/null # conformance: busy-wait-read')[0].waived, 'no-reason');
  assert.equal(busyReads('// conformance: busy-wait-read — a fixture whose stdin must be at EOF\nrun("read -t 1 x </dev/null")', 'code')[0].waived, 'reason');
});

test('C60 kindOf: extensions, shebang files without one, package.json', () => {
  assert.equal(kindOf('a.bash', ''), 'shell');
  assert.equal(kindOf('dev', '#!/usr/bin/env bash\nx'), 'shell');
  assert.equal(kindOf('dev', 'plain'), null);
  assert.equal(kindOf('package.json', '{}'), 'code');
  assert.equal(kindOf('notes.md', ''), null);
});

test('C60 through the runner: shell, workflow, test-helper and extension-less files, error per site', async () => {
  const bad = 'while :; do read -t 20 < /dev/zero; done\n';
  const f = await findingsOf(
    'C60',
    makeApp({
      'scripts/wait.sh': `#!/bin/bash\n${bad}`,
      'scripts/a.bash': bad,
      'scripts/b.zsh': bad,
      'dev': `#!/usr/bin/env bash\n${bad}`,
      'tools/x.mjs': 'const sh = "while :; do read -t 1 _ </dev/null; done";\n',
      'ui/y.ts': 'export const sh = `read -t 1 _ </dev/null`;\n',
      'package.json': '{ "scripts": { "w": "read -t 1 _ </dev/null" } }\n',
      '.gitea/workflows/x.yml': 'jobs:\n  a:\n    steps:\n      - run: |\n          until ok; do read -t 1 _ </dev/null; done\n',
      'crates/p/src/process.rs': 'const SH: &str = "while true; do read -t 1 _ </dev/null; done";\n',
      'scripts/good.sh': 'read -t 5 line < "$FIFO"\nsleep 1\n',
      'scripts/waived.sh': '# conformance: busy-wait-read — EOF is the point of this fixture\nread -t 1 x </dev/null\n',
      'notes.md': 'read -t 1 </dev/null\n',
      'README': 'read -t 1 </dev/null\n',
    }),
  );
  assert.deepEqual(f.map((x) => [x.file, x.severity]).sort(), [
    ['.gitea/workflows/x.yml', 'error'],
    ['crates/p/src/process.rs', 'error'],
    ['dev', 'error'],
    ['package.json', 'error'],
    ['scripts/a.bash', 'error'],
    ['scripts/b.zsh', 'error'],
    ['scripts/wait.sh', 'error'],
    ['tools/x.mjs', 'error'],
    ['ui/y.ts', 'error'],
  ]);
  assert.match(f[0].fix, /sleep/);
});
