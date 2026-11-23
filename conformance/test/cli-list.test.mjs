import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { test } from 'node:test';
import { COMMANDS, listChecks, main } from '../src/cli.mjs';
import { checks } from '../src/checks/index.mjs';
import { makeApp } from './helpers.mjs';

const SCOPES = ['root', 'apps', 'both'];
const MODES = ['gate', 'report-only'];
const NEEDS = ['all', 'rust', 'ui'];

test('list --json: every check declares where it runs, whether it can fail a run, and what an app needs', () => {
  const list = listChecks();
  assert.equal(list.length, checks.length);
  for (const c of list) {
    assert.ok(SCOPES.includes(c.scope), `${c.id}: scope ${c.scope}`);
    assert.ok(MODES.includes(c.mode), `${c.id}: mode ${c.mode}`);
    assert.ok(NEEDS.includes(c.needs), `${c.id}: needs ${c.needs}`);
    assert.ok(existsSync(resolve(import.meta.dirname, '..', '..', c.file)), `${c.id}: ${c.file} does not exist`);
  }
});

test('list --json bad: a check without scope or mode is listed as undefined, which the consumer rejects', () => {
  const list = listChecks([{ id: 'C99', title: 't', rule: 1 }]);
  assert.equal(list[0].scope, undefined);
  assert.equal(list[0].mode, undefined);
});

// Which checks are report-only is each module's `mode`, read by `./dev gates check` (docs/gates.toml registers each one); no
// copy of that list here (ER-42).

test('main: list --json and commands --json print JSON', async () => {
  const out = [];
  assert.equal(await main(['list', '--json'], { log: (x) => out.push(x), err: () => {} }), 0);
  assert.equal(JSON.parse(out[0])[0].id, checks[0].id);
  const cmds = [];
  assert.equal(await main(['commands', '--json'], { log: (x) => cmds.push(x), err: () => {} }), 0);
  assert.deepEqual(JSON.parse(cmds[0]), COMMANDS);
  assert.ok(COMMANDS.includes('commit-lint'));
  assert.ok(!COMMANDS.includes('copies'), 'copies --check-register is planned (ADR 0034 WP-1), not built');
});

test('the printed report keeps its documented shape: the outcomes (with their times) decide the exit and are not printed', async () => {
  const root = makeApp({ 'paddock.waivers.toml': '' });
  const lines = [];
  const code = await main(['--app', 'paddock', '--root', root, '--only', 'C57', '--json'], { log: (s) => lines.push(s), err() {}, env: {} });
  assert.equal(code, 0, lines.join('\n'));
  const report = JSON.parse(lines.join('\n'));
  assert.deepEqual(Object.keys(report), ['app', 'checks', 'findings', 'waived', 'errors', 'warnings']);
});

// The push gate counts progress by output lines; a row that prints only at its end has none (watchdog audit 2026-11-23, cause A).
// `PADDOCK_PROGRESS_LINES=1` (set by the gate for every row; tools/paddock-dev/src/progress_lines.rs) asks for one line per check finished.
test('PADDOCK_PROGRESS_LINES=1 prints one line per check finished on stderr, and nothing without it', async () => {
  const root = makeApp({ 'paddock.waivers.toml': '' });
  const run = async (env) => {
    const err = [];
    // the exit status is not the point here (a check of the fixture app may find something): the lines are
    await main(['--app', 'paddock', '--root', root, '--only', 'C57,C06', '--json'], { log() {}, err: (s) => err.push(s), env });
    return err.filter((l) => l.startsWith('progress: '));
  };
  const asked = await run({ PADDOCK_PROGRESS_LINES: '1' });
  assert.deepEqual(asked, ['progress: C06 finished', 'progress: C57 finished']);
  // controls: unset and `0` print no line (the output of CI and of a person is unchanged)
  assert.deepEqual(await run({}), []);
  assert.deepEqual(await run({ PADDOCK_PROGRESS_LINES: '0' }), []);
});
