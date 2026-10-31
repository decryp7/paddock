import assert from 'node:assert/strict';
import { readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { codeOf, pipesIntoQuietGrep, quietGrepLines, setsPipefail, workflowSites } from '../src/checks/C62-pipe-grep.mjs';
import { findingsOf, makeApp, PADDOCK } from './helpers.mjs';

test('C62 sees every pipe-fed quiet grep form and nothing else', () => {
  for (const l of [
    'find "$DIR" -name "*.sig" | grep -q .',
    'printf "%s\\n" "$x" | grep -Eq -- "$re"',
    'rustup component list | grep -Fxq clippy',
    'if ! echo "$s" | grep -qx active; then',
    'out=$(cmd | grep -i -q foo)',
    'a | LC_ALL=C grep --quiet x',
    'a | egrep -s -q x && b',
    'a|grep -q x',
    'x && a | fgrep --silent y',
  ]) assert.ok(pipesIntoQuietGrep(l), l);
  for (const l of [
    'grep -q x <<<"$out"',
    'grep -q x file | wc -l',
    'a || grep -q x file',
    'a | grep x',
    'a | grep -c x',
    'a | grep -e quux',
    '# a | grep -q x',
    'echo "a | grep -q x"',
    "bash -c 'a | grep -q x'",
    'a # then | grep -q x',
    'a |& tee log',
  ]) assert.ok(!pipesIntoQuietGrep(l), l);
});

test('C62 reads quotes and comments like the shell', () => {
  assert.equal(codeOf('echo "a # b" # c'), 'echo "     " ');
  assert.equal(codeOf("x='it''s' # c"), "x='  '' ' ", "'it''s' is two quoted words");
  assert.equal(codeOf('url=a#b'), 'url=a#b', 'a # inside a word is not a comment');
});

test('C62 knows when pipefail is on', () => {
  for (const s of ['set -o pipefail', 'set -euo pipefail', '  set -eo pipefail', 'set -e -o pipefail', 'set -Eeuo pipefail']) assert.ok(setsPipefail(`#!/bin/bash\n${s}\n`), s);
  for (const s of ['set -e', '# set -o pipefail', 'echo set -o pipefail', 'set +o pipefail']) assert.ok(!setsPipefail(`#!/bin/bash\n${s}\n`), s);
});

test('C62 takes a reasoned waiver on the line or the line above, never a bare one', () => {
  assert.deepEqual(quietGrepLines(['a | grep -q x  # conformance: pipe-grep — the writer is a fixed one-byte printf']), []);
  assert.deepEqual(quietGrepLines(['# conformance: pipe-grep — reason given here', 'a | grep -q x']), []);
  assert.deepEqual(quietGrepLines(['# conformance: pipe-grep', 'a | grep -q x']), [2]);
  assert.deepEqual(quietGrepLines(['ok', 'a | grep -q x', 'b | grep -q y'], 10), [11, 12]);
});

test('C62 in workflows: bash steps (explicit or by defaults) and steps that set pipefail; a step without a shell is not', () => {
  const wf = [
    'on: push',
    'jobs:',
    '  a:',
    '    steps:',
    '      - name: explicit bash',
    '        shell: bash',
    '        run: |',
    '          echo hi',
    "          if find out -name '*.sig' | grep -q .; then echo y; fi",
    '      - name: no shell (bash -e, no pipefail)',
    '        run: |',
    '          a | grep -q x',
    '      - name: sets it',
    '        run: |',
    '          set -euo pipefail',
    '          a | grep -q x',
    '      - name: one line',
    '        shell: bash',
    '        run: a | grep -qx y',
    '      - name: pwsh',
    '        shell: pwsh',
    '        run: a | grep -q x',
    '',
  ].join('\n');
  assert.deepEqual(workflowSites(wf), [9, 16, 19]);
  const defaults = ['on: push', 'defaults:', '  run:', '    shell: bash', 'jobs:', '  a:', '    steps:', '      - run: |', '          a | grep -q x', ''].join('\n');
  assert.deepEqual(workflowSites(defaults), [9]);
});

test('C62 through the runner: warnings in pipefail scripts and bash steps of workflows and actions only', async (t) => {
  const root = makeApp({
    'scripts/bad.sh': '#!/usr/bin/env bash\nset -euo pipefail\nif codesign -dv x 2>&1 | grep -q adhoc; then :; fi\n',
    'scripts/no-pipefail.sh': '#!/bin/sh\nset -e\na | grep -q x\n',
    'scripts/good.sh': '#!/usr/bin/env bash\nset -euo pipefail\nout=$(codesign -dv x 2>&1)\ngrep -q adhoc <<<"$out"\n',
    '.gitea/actions/x/action.yml': 'name: x\nruns:\n  using: composite\n  steps:\n    - shell: bash\n      run: |\n        a | grep -Fxq y\n',
    '.github/workflows/w.yml': 'on: push\njobs:\n  a:\n    steps:\n      - shell: bash\n        run: a | grep --quiet y\n',
    'docs/note.md': 'set -o pipefail\na | grep -q x\n',
  });
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const f = await findingsOf('C62', root);
  assert.deepEqual(
    f.map((x) => [x.file, x.line, x.severity]).sort(),
    [
      ['.gitea/actions/x/action.yml', 7, 'warn'],
      ['.github/workflows/w.yml', 6, 'warn'],
      ['scripts/bad.sh', 3, 'warn'],
    ],
  );
  assert.match(f[0].fix, /<<<"\$out"/);
});

// The shared release path and CI setup carry none (fixed when C62 was added): publish-release decided whether to publish
// latest.json with `find | grep -q .`, and setup-rust read installed components through `printf | grep -q`.
test('C62 finds nothing in the shared release path and CI setup of this repository', async () => {
  const files = [
    '.gitea/actions/publish-release/action.yml',
    '.gitea/actions/setup-rust/action.yml',
    '.gitea/workflows/app-release.yml',
    '.gitea/workflows/debrief.yml',
    'conformance/ci/first-release-notes.sh',
    'conformance/ci/job-isolation.sh',
    'conformance/release/macos-verify-bundle.sh',
  ];
  const sites = [];
  for (const f of files) {
    const text = readFileSync(join(PADDOCK, f), 'utf8');
    const lines = f.endsWith('.sh') ? (setsPipefail(text) ? quietGrepLines(text.split('\n')) : []) : workflowSites(text);
    for (const l of lines) sites.push(`${f}:${l}`);
  }
  assert.deepEqual(sites, []);
});
