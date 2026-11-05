import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { test } from 'node:test';
import { discoveryState, makeKey, makeMcpGuest, mcpCommand, settingsJson, splitExit, storeEntryCommand, writeSettingsCommand } from './mcp-guest.mjs';
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
test('credential-store commands name service Dyno and account mcp:keys on every OS; the key goes in the environment, not an argument', () => {
  const { entries, key } = makeKey(Buffer.alloc(32, 1), 1);
  assert.match(storeEntryCommand('macos', entries), /security add-generic-password -U -T '[^']*' -s Dyno -a 'mcp:keys'/);
  assert.match(storeEntryCommand('linux', entries), /secret-tool store --label=Dyno service Dyno username 'mcp:keys'/);
  assert.match(storeEntryCommand('windows', entries), /schtasks \/create \/tn DynoStoreKey[^;]*\/it /);
  assert.match(storeEntryCommand('windows', entries), /FromBase64String/);
  assert.doesNotMatch(storeEntryCommand('windows', entries), /entries|\[\{"id"/);
  assert.match(mcpCommand('linux', key, '{}', '/usr/bin/dyno'), /DYNO_MCP_KEY='pmk_/);
  assert.doesNotMatch(mcpCommand('macos', key, '{}', '/x/dyno'), /\btimeout\b/); // macOS has no timeout(1)
  assert.doesNotMatch(mcpCommand('linux', key, '{}', '/usr/bin/dyno'), /dyno' mcp pmk_/);
  assert.match(writeSettingsCommand('macos', "/Users/a b/Library", '{}'), /> '\/Users\/a b\/Library\/mcp\.json'/);
  assert.deepEqual(splitExit('hello\nEXIT=1\n'), { code: 1, out: 'hello' });
  assert.equal(splitExit('nothing').code, null);
});
test('macOS: the seeded key entry has its partition list opened right after it is made, so the app does not block on the keychain', () => {
  const { entries } = makeKey(Buffer.alloc(32, 2), 1);
  const cmd = storeEntryCommand('macos', entries, 'pw with space');
  const add = cmd.indexOf('security add-generic-password');
  const open = cmd.indexOf('security set-generic-password-partition-list');
  assert.ok(add >= 0 && open > add, 'the partition list is set after the entry exists');
  assert.match(cmd, /^H=\$\(codesign -dvvv '\/Applications\/Dyno\.app' 2>&1 \| sed -n 's\/\^CDHash=\/\/p'\); \[ -n "\$H" \] && security add-generic-password -U -T '\/Applications\/Dyno\.app' /, 'the app\'s own CDHash is read from the bundle; an unreadable one stops the seed');
  assert.match(cmd, /&& security set-generic-password-partition-list -S "apple-tool:,apple:,cdhash:\$H" -s Dyno -a 'mcp:keys' -k 'pw with space'/, 'chained: a failed add skips it, and a failed partition call fails the whole store (a broken seed is not a pass); apple-tool: alone, apple: and an empty list were measured to still block the app');
  assert.match(storeEntryCommand('macos', entries), /-k 'admin'/, 'the image\'s admin password is the default');
  assert.doesNotMatch(storeEntryCommand('linux', entries), /partition/);
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
    sh: async (c) => { log.push(/secret-tool/.test(c) ? 'store' : /mcp\.json/.test(c) ? 'settings' : 'other'); return { code: /secret-tool/.test(c) && p.failStore ? 1 : 0, stdout: '' }; } };
  const g = makeMcpGuest(p);
  assert.match(await g.enable(), /^pmk_/);
  assert.deepEqual(log, ['stop', 'settings', 'store', 'start']);
  p.failStore = true;
  const r = await mcpStep(g);
  assert.equal(r.status, 'check-broken');
  assert.match(r.summary, /credential store/);
  assert.ok(MCP_TOOLS.length === 10);
});
