import assert from 'node:assert/strict';
import { test } from 'node:test';
import { dynoValues, problems } from './check-logon-entry.mjs';

const head = 'HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n';
const onedrive = '    OneDrive    REG_SZ    "C:\\Users\\a\\OneDrive.exe" /background\r\n';

test('the app entry alone is right', () => {
  assert.deepEqual(problems(head + onedrive + '    Dyno    REG_SZ    C:\\Users\\a\\AppData\\Local\\Dyno\\dyno-app.exe --hidden\r\n'), []);
  assert.deepEqual(problems(head + '    Dyno    REG_SZ    "C:\\Dyno\\dyno-app.exe" --hidden\r\n'), []);
});

test('the 1.0.0 state (only the agent starts) is reported twice over', () => {
  const found = problems(head + onedrive + '    Dyno    REG_SZ    "C:\\Users\\a\\AppData\\Local\\Dyno\\dyno-agent.exe" run\r\n');
  assert.equal(found.length, 2);
  assert.match(found[0], /starts only dyno-agent/);
  assert.match(found[1], /no logon entry starts dyno-app.exe --hidden/);
});

test('an agent entry beside the app entry is not wrong, an empty list is', () => {
  const both = head + '    Dyno    REG_SZ    C:\\Dyno\\dyno-app.exe --hidden\r\n    Dyno agent    REG_SZ    "C:\\Dyno\\dyno-agent.exe" run\r\n';
  assert.equal(problems(both).length, 1, 'the separate agent entry still starts only the agent');
  assert.deepEqual(dynoValues(head + onedrive), []);
  assert.equal(problems(head + onedrive).length, 1);
});
