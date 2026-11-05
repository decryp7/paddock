import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findingsOf, makeApp } from './helpers.mjs';

const HOOKS_OK = `!include "\${__FILEDIR__}\\paddock-per-machine.generated.nsh"
!include "\${__FILEDIR__}\\paddock-per-machine.nsh"
!macro NSIS_HOOK_PREINSTALL
  !insertmacro PADDOCK_INSTALLING
  !insertmacro PADDOCK_CHECK_DIR
  !insertmacro PADDOCK_GUARD_PER_USER
  !insertmacro PADDOCK_STOP_ALL
!macroend
!macro NSIS_HOOK_POSTINSTALL
  !insertmacro PADDOCK_INSTALL_SETUP
  !insertmacro PADDOCK_CLEAN_RUN_VALUES
  !insertmacro PADDOCK_FINISH_INSTALL
!macroend
!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro PADDOCK_INSTALLING
  !insertmacro PADDOCK_REINSTALL_AS_UPDATE
  !insertmacro PADDOCK_STOP_ALL
  !insertmacro PADDOCK_CLEAN_RUN_VALUES
  !insertmacro PADDOCK_REMOVE_PROGRAM_FILES
!macroend
!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro PADDOCK_FINISH_UNINSTALL
!macroend
`;

const conf = (mode, hooks = './windows/installer-hooks.nsh') =>
  JSON.stringify({ bundle: { windows: { nsis: { installMode: mode, ...(hooks === null ? {} : { installerHooks: hooks }) } } } });
const base = JSON.stringify({ plugins: { updater: { windows: { installMode: 'passive' } } } });

const app = (over = {}) =>
  makeApp(
    Object.fromEntries(
      Object.entries({
    'app/src-tauri/tauri.conf.json': base,
    'app/src-tauri/tauri.windows.conf.json': conf('perMachine'),
    'app/src-tauri/windows/installer-hooks.nsh': HOOKS_OK,
    'app/src-tauri/windows/paddock-per-machine.nsh': '; include\n',
    'app/src-tauri/windows/paddock-per-machine.generated.nsh': '!define PADDOCK_STOP_IMAGES "a.exe"\n',
    'app/src-tauri/windows/per-machine.json': '{}',
    '.gitea/workflows/demo-release.yml': 'jobs:\n  b:\n    with:\n      windows-pre-bundle: node scripts/stage-paddock-setup.mjs --app demo\n',
    ...over,
      }).filter(([, v]) => v !== undefined),
    ),
  );

test('C91: a per-machine app that follows the contract is clean', async () => {
  assert.deepEqual(await findingsOf('C91', app()), []);
});

test('C91: a per-user app (or none) is not checked', async () => {
  assert.deepEqual(await findingsOf('C91', app({ 'app/src-tauri/tauri.windows.conf.json': conf('currentUser'), 'app/src-tauri/windows/installer-hooks.nsh': '; own hooks\n' })), []);
  assert.deepEqual(await findingsOf('C91', makeApp({ 'app/src-tauri/tauri.conf.json': '{}' })), []);
});

test('C91: a per-machine app must use the shared include, its macros in the right hooks, the passive updater and the staging', async () => {
  const msg = async (over) => (await findingsOf('C91', app(over))).map((f) => f.message).join('\n');
  assert.match(await msg({ 'app/src-tauri/windows/installer-hooks.nsh': '!macro NSIS_HOOK_PREINSTALL\n!macroend\n' }), /does not include paddock-per-machine.nsh/);
  assert.match(await msg({ 'app/src-tauri/windows/installer-hooks.nsh': HOOKS_OK.replace('  !insertmacro PADDOCK_STOP_ALL\n!macroend\n!macro NSIS_HOOK_POSTINSTALL', '!macroend\n!macro NSIS_HOOK_POSTINSTALL') }), /NSIS_HOOK_PREINSTALL.*PADDOCK_STOP_ALL/);
  assert.match(await msg({ 'app/src-tauri/windows/installer-hooks.nsh': HOOKS_OK.replace('PADDOCK_REINSTALL_AS_UPDATE', 'PADDOCK_SOMETHING') }), /NSIS_HOOK_PREUNINSTALL.*PADDOCK_REINSTALL_AS_UPDATE/);
  assert.match(await msg({ 'app/src-tauri/tauri.conf.json': JSON.stringify({ plugins: { updater: { windows: { installMode: 'quiet' } } } }) }), /updater.*passive/);
  assert.match(await msg({ 'app/src-tauri/tauri.windows.conf.json': conf('perMachine', null) }), /installerHooks/);
  assert.match(await msg({ 'app/src-tauri/windows/paddock-per-machine.generated.nsh': undefined }), /paddock-per-machine.generated.nsh/);
  assert.match(await msg({ 'app/src-tauri/windows/per-machine.json': undefined }), /per-machine.json/);
  assert.match(await msg({ '.gitea/workflows/demo-release.yml': 'jobs: {}\n' }), /stage-paddock-setup/);
});
