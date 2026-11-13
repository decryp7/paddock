import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { test } from 'node:test';
import { COMMANDS, listChecks, main } from '../src/cli.mjs';
import { checks } from '../src/checks/index.mjs';

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

test('the report-only checks are the ones that only warn', () => {
  assert.deepEqual(
    listChecks().filter((c) => c.mode === 'report-only').map((c) => c.id),
    ['C14', 'C29', 'C36', 'C51', 'C61', 'C62', 'C85', 'C119'],
  );
});

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
