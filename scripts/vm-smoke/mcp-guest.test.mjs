import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { test } from 'node:test';
import { discoveryState, keysFileJson, makeKey, makeMcpGuest, mcpCommand, settingsJson, splitExit, writeKeysCommand, writeSettingsCommand } from './mcp-guest.mjs';
import { mcpStep, MCP_TOOLS } from './mcp-judges.mjs';

test('the key is the product format and its entry holds only the SHA-256, never the key', () => {
  const { key, entries } = makeKey(Buffer.alloc(32, 0xab), 1);
  assert.match(key, /^pmk_(ab){32}$/);
  const e = JSON.parse(entries)[0];
  assert.equal(e.sha256, createHash('sha256').update(key).digest('hex'));
  assert.ok(!entries.includes(key));
  assert.deepEqual(Object.keys(e).sort(), ['client', 'created_ms', 'id', 'revoked', 'sha256']);
  assert.deepEqual(JSON.parse(settingsJson()), { version: 1, enabled: true, resources: true, disabledTools: [], allowedActions: [] });
});
test('the key list is the owner-only mcp-keys.json (versioned, mode 600), never the credential store; the key goes in the environment, not an argument', () => {
  const { entries, key } = makeKey(Buffer.alloc(32, 1), 1);
  const file = JSON.parse(keysFileJson(entries));
  assert.equal(file.version, 1);
  assert.equal(file.keys.length, 1);
  for (const os of ['macos', 'linux']) {
    const c = writeKeysCommand(os, '/home/a b/.config/dyno', entries);
    assert.match(c, /umask 077/);
    assert.match(c, /chmod 600 '\/home\/a b\/\.config\/dyno\/mcp-keys\.json'/);
    assert.doesNotMatch(c, /security|secret-tool|cmdkey/);
  }
  const w = writeKeysCommand('windows', 'C:\\Users\\u\\AppData\\Roaming\\dyno', entries);
  assert.match(w, /mcp-keys\.json/);
  assert.doesNotMatch(w, /cmdkey|schtasks/);
  assert.doesNotMatch(w, /pmk_/);
  assert.match(mcpCommand('linux', key, '{}', '/usr/bin/dyno'), /DYNO_MCP_KEY='pmk_/);
  assert.doesNotMatch(mcpCommand('macos', key, '{}', '/x/dyno'), /\btimeout\b/); // macOS has no timeout(1)
  assert.doesNotMatch(mcpCommand('linux', key, '{}', '/usr/bin/dyno'), /dyno' mcp pmk_/);
  assert.match(writeSettingsCommand('macos', "/Users/a b/Library", '{}'), /> '\/Users\/a b\/Library\/mcp\.json'/);
  assert.deepEqual(splitExit('hello\nEXIT=1\n'), { code: 1, out: 'hello' });
  assert.equal(splitExit('nothing').code, null);
});
test('dead state: a leftover discovery file naming a live pid is live, a dead pid fails verification, no file is gone', () => {
  assert.equal(discoveryState('', () => true), 'gone');
  assert.equal(discoveryState('{"pid":5}', () => true), 'live');
  assert.equal(discoveryState('{"pid":5}', () => false), 'pid-fails');
  assert.equal(discoveryState('garbage', () => true), 'pid-fails');
});
test('enable stops the app, writes the switch, stores the key, THEN starts it; a failing store is a broken check, not a pass', async () => {
  const log = [];
  const p = { os: 'linux', dyno: '/usr/bin/dyno', configDir: async () => '/home/a/.config/dyno', stopApp: async () => log.push('stop'), startApp: async () => log.push('start'), killAgent: async () => {}, agentAlive: async () => false, quitApp: async () => {}, pidAlive: () => false,
    sh: async (c) => { log.push(/mcp-keys\.json/.test(c) ? 'store' : /mcp\.json/.test(c) ? 'settings' : 'other'); return { code: /mcp-keys\.json/.test(c) && p.failStore ? 1 : 0, stdout: '' }; } };
  const g = makeMcpGuest(p);
  assert.match(await g.enable(), /^pmk_/);
  assert.deepEqual(log, ['stop', 'settings', 'store', 'start']);
  p.failStore = true;
  const r = await mcpStep(g);
  assert.equal(r.status, 'check-broken');
  assert.match(r.summary, /key list file/);
  assert.ok(MCP_TOOLS.length === 10);
});
