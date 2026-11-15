import assert from 'node:assert/strict';
import { test } from 'node:test';
import C127, { computedRmSites } from '../src/checks/C127-computed-rm.mjs';

const lines = (text, file = 'tools/x.sh') => computedRmSites(text, file).map((s) => s.line);

test('C127: a recursive rm of a computed path in a shell script is a site', () => {
  // The shapes the earlier instances had: a bare variable (the prune folder), a guarded one, one with an owned prefix.
  assert.deepEqual(lines('dir="$1"\nrm -rf "$dir"\n'), [2]);
  assert.deepEqual(lines('rm -rf "${t:?}"\n'), [1], 'a guarded variable is still computed: an unset one fails, a wrong one does not');
  assert.deepEqual(lines('rm -rf "$root/dist/bundle" 2>/dev/null\n'), [1]);
  assert.deepEqual(lines('[ -d "$d" ] && rm -fr $d\n'), [1]);
  assert.deepEqual(lines('rm -r -f -- "$a" "$b"\n'), [1]);
  assert.deepEqual(lines('rm --recursive --force "$(cat list)"\n'), [1]);
  assert.deepEqual(lines('x=1; rm -rf "$x" && echo y\n'), [1]);
});

test('C127: safe_rm, a literal path, a non-recursive rm, a mktemp folder, comments and waivers are fine', () => {
  assert.deepEqual(lines('safe_rm "$root" "$root/dist"\n'), []);
  assert.deepEqual(lines('rm -rf dist/bundle\n'), []);
  assert.deepEqual(lines('rm -f "$lock"\n'), []);
  assert.deepEqual(lines('tmp=$(mktemp -d)\ntrap \'rm -rf "$tmp"\' EXIT\n'), [], "the script's own mktemp folder");
  assert.deepEqual(lines('# rm -rf "$dir" was here\n'), []);
  assert.deepEqual(lines('echo "never rm -rf $x"\n'), [], 'text in a quoted echo is not a command');
  assert.deepEqual(lines('# conformance: computed-rm — the folder this script just made with mkdir\nrm -rf "$work"\n'), []);
  assert.deepEqual(lines('rm -rf "$work" # conformance: computed-rm — made above with mkdir -p\n'), []);
  assert.deepEqual(lines('# conformance: scoped-cleanup — the stale holder\'s own lock folder\nrm -rf "$other"\n'), [], "C115's waiver counts");
  assert.deepEqual(lines('rm -rf "$a" && rm -rf "$b"\n'), [1], 'one finding per line');
});

test('C127 reads tracked shell scripts only, never tests, fixtures or the helper', () => {
  const text = 'rm -rf "$d"\n';
  assert.deepEqual(lines(text, 'scripts/vm-lock.sh'), [1]);
  assert.deepEqual(lines(text, 'tools/heavy/tests/fixtures/plock.sh'), []);
  assert.deepEqual(lines(text, '.claude/hooks/kill-guard.test.sh'), []);
  assert.deepEqual(lines(text, 'conformance/ci/safe-rm.sh'), []);
  assert.deepEqual(lines(text, 'apps/x/node_modules/y/z.sh'), []);
  assert.deepEqual(lines(text, 'docs/research/2026-x/harness/bench.sh'), [], 'a research harness is a record, not a tool that runs');
  assert.deepEqual(lines(text, 'apps/debrief/docs/research/x/bench.sh'), []);
  assert.deepEqual(lines(text, 'tools/x.mjs'), [], 'not a shell script');
});

test('C127 is report-only and warns with the helper as the fix', () => {
  assert.equal(C127.mode, 'report-only');
  const files = { 'tools/x.sh': '#!/bin/sh\nrm -rf "$dir"\n', 'tools/ok.sh': 'safe_rm "$r" "$r/x"\n' };
  const out = C127.run({ files: () => Object.keys(files), read: (f) => files[f] });
  assert.equal(out.length, 1);
  assert.equal(out[0].severity ?? out[0].level, 'warn');
  assert.match(out[0].message, /computed path/);
  assert.match(out[0].fix, /safe_rm <root> <path>/);
});
