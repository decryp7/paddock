import assert from 'node:assert/strict';
import { test } from 'node:test';
import { MCP_TOOLS, callPayload, judgeAfterAgentKill, judgeAfterQuit, judgeQuitStop, judgeSystemSummary, judgeToolsList, mcpInput, mcpStep, parseResponses, quitStopStep } from './mcp-judges.mjs';
import { requiredSteps } from './manifest.mjs';

const bad = (c) => c.filter((x) => !x.ok);
const list = (names) => ({ id: 2, result: { tools: names.map((name) => ({ name })) } });
const summary = (o = {}) => ({ id: 3, result: { content: [{ type: 'text', text: JSON.stringify({ uptimeSeconds: 900, cpu: { percent: 12.5 }, memory: { usedBytes: 4e9, totalBytes: 8e9 }, ...o }) }] } });

test('dead state: no tools, a missing tool or an unallowed action fails', () => {
  assert.ok(bad(judgeToolsList(undefined)).length);
  assert.ok(bad(judgeToolsList(list(MCP_TOOLS.slice(1)))).length);
  assert.ok(bad(judgeToolsList(list([...MCP_TOOLS, 'keep_awake']))).length);
  assert.equal(bad(judgeToolsList(list([...MCP_TOOLS, 'keep_awake']), { actionsAllowed: true })).length, 0);
  assert.equal(bad(judgeToolsList(list(MCP_TOOLS))).length, 0);
});
test('dead state: system_summary with an error, cpu 140, memory used above total, or uptime 0 fails', () => {
  assert.ok(bad(judgeSystemSummary({ id: 3, error: { message: 'x' } })).length);
  assert.ok(bad(judgeSystemSummary(summary({ cpu: { percent: 140 } }))).length);
  assert.ok(bad(judgeSystemSummary(summary({ memory: { usedBytes: 9e9, totalBytes: 8e9 } }))).length);
  assert.ok(bad(judgeSystemSummary(summary({ uptimeSeconds: 0 }))).length);
  assert.equal(bad(judgeSystemSummary(summary())).length, 0);
});
test('dead state: with the agent dead, old numbers or a silence fail; an error naming the agent passes; a live agent is a broken check', () => {
  assert.ok(bad(judgeAfterAgentKill(summary(), { agentDead: true })).length);
  assert.ok(bad(judgeAfterAgentKill(undefined, { agentDead: true })).length);
  assert.equal(bad(judgeAfterAgentKill({ id: 3, error: { message: 'The Dyno agent is not running' } }, { agentDead: true })).length, 0);
  assert.equal(judgeAfterAgentKill(summary(), { agentDead: false })[0].kind, 'check');
});
test('dead state: after Quit a live listener file, exit 0 or a missing message fail', () => {
  assert.ok(bad(judgeAfterQuit({ code: 0, out: 'ok', discovery: 'live' })).length === 3);
  assert.equal(bad(judgeAfterQuit({ code: 1, out: "Dyno isn't open", discovery: 'gone' })).length, 0);
  assert.equal(bad(judgeAfterQuit({ code: 1, out: "Dyno isn't open", discovery: 'pid-fails' })).length, 0);
});
test('dead state: Quit and stop with a process alive, too short a window, or nothing back after reopening or a reboot fails', () => {
  const s = (c) => [0, 10, 20, 30, 40, 50, 60].map((t) => ({ t, count: c(t) }));
  assert.equal(bad(judgeQuitStop({ samples: s(() => 0), reopened: 1, rebooted: 1 })).length, 0);
  assert.ok(bad(judgeQuitStop({ samples: s((t) => (t === 30 ? 1 : 0)), reopened: 1, rebooted: 1 })).length);
  assert.equal(bad(judgeQuitStop({ samples: s(() => 0).slice(0, 3), reopened: 1, rebooted: 1 })).length, 1);
  assert.ok(bad(judgeQuitStop({ samples: s(() => 0), reopened: 0, rebooted: 1 })).length);
  assert.ok(bad(judgeQuitStop({ samples: s(() => 0), reopened: 1, rebooted: 0 })).length);
  assert.ok(bad(judgeQuitStop({ samples: s(() => 0), reopened: 1, rebooted: null })).length);
});
test('the step runs the four calls in order against a fake guest; an unseeded key is a broken check', async () => {
  const order = [];
  const out = JSON.stringify(list(MCP_TOOLS)) + '\n' + JSON.stringify(summary()) + '\n';
  let dead = false; let n = 0;
  const g = { enable: async () => 'pmk_x', mcp: async (key) => { order.push('mcp'); if (++n === 3) return { code: 1, out: "Dyno isn't open" }; return { code: 0, out: dead ? JSON.stringify({ id: 3, error: { message: 'agent is not running' } }) : out }; }, killAgent: async () => { order.push('kill'); dead = true; }, agentDead: async () => true, quitApp: async () => order.push('quit'), discovery: async () => 'gone' };
  const r = await mcpStep(g);
  assert.equal(r.status, 'pass');
  assert.deepEqual(order, ['mcp', 'kill', 'mcp', 'quit', 'mcp']);
  assert.equal((await mcpStep({ enable: async () => null })).status, 'check-broken');
  assert.equal((await quitStopStep({ trigger: async () => { throw new Error('no menu'); } })).status, 'check-broken');
  assert.match(mcpInput(), /"tools\/list"/);
  assert.equal(parseResponses('garbage\n{"id":2}\n').size, 1);
  assert.equal(callPayload({ result: { isError: true } }), null);
});
test('both rows are required steps of every OS release run', () => {
  for (const os of ['windows', 'linux', 'macos']) {
    const ids = requiredSteps({ os, suite: 'power', ui: true, reboot: false, release: true }).map((r) => r.id);
    assert.ok(ids.includes('mcp') && ids.includes('quit-stop'), os);
  }
});
