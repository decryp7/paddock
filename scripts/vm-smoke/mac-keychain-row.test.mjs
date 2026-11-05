import assert from 'node:assert/strict';
import { test } from 'node:test';
import { macKeychainPrimitives } from './mac-keychain-row.mjs';

function rig({ itemAppears = true } = {}) {
  let t = 0; const calls = []; let item = false;
  const guest = { now: () => t, sleep: async (ms) => { t += ms; }, exec: async (c) => { calls.push(c); if (/delete-generic-password/.test(c)) item = false; if (/find-generic-password/.test(c)) return { code: item ? 0 : 1, stdout: '', stderr: '' }; return { code: 0, stdout: '1\n', stderr: '' }; } };
  const gui = { app: 'Dyno', has: async (a, re) => !/off/.test(re) || true, press: async (a, re) => { calls.push(`press ${re}`); if (/Create key/.test(re) && itemAppears) item = true; return true; },
    waitFor: async (fn) => { const v = await fn(); return { ok: Boolean(v), value: v }; }, activate: async () => {}, openSettingsSection: async () => ({ ok: true }), fieldIndex: async () => 0, setField: async () => true };
  const check = { install: async (dmg) => { calls.push(`install ${dmg}`); return { status: 'pass' }; } };
  return { calls, p: macKeychainPrimitives({ guest, gui, check, share: '/share', bundle: '/Applications/Dyno.app' }) };
}
test('build A: the old key item is deleted before A is installed, the key is made through the UI, and the item must exist afterwards', async () => {
  const { calls, p } = rig();
  await p.createKeyInBuildA();
  const iDel = calls.findIndex((c) => /delete-generic-password/.test(c)), iIns = calls.indexOf('install /share/before.dmg');
  assert.ok(iDel >= 0 && iDel < iIns, 'a key left by the suite is deleted first');
  assert.ok(calls.includes('press ^Create key$'));
  const { p: p2 } = rig({ itemAppears: false });
  await assert.rejects(p2.createKeyInBuildA(), /created no Dyno \/ mcp:keys item/);
});
test('build B is installed from the staged installer, and cleanup removes the key item and the app', async () => {
  const { calls, p } = rig();
  await p.installBuildB();
  assert.ok(calls.includes('install /share/installer.dmg'));
  await p.cleanup();
  assert.ok(calls.some((c) => /delete-generic-password/.test(c)));
});
