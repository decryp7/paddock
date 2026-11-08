// C91: the per-machine installer contract (family ADR 0053 section 8). An app whose Windows installer is `perMachine`
// (`bundle.windows.nsis.installMode`, read from tauri.windows.conf.json over tauri.conf.json) must use the shared include:
// its installer hooks include paddock-per-machine.nsh and call the include's macros from the hooks the ADR names; its updater
// runs `passive` (`quiet` cannot elevate); the generated stop list, the include copy and the exclusions file are there (their
// content is checked by paddock-service's `per_machine` test, which runs with this check: scripts/affected.mjs
// PER_MACHINE_INPUT); and the app's release workflow stages paddock-setup.exe before the bundle (scripts/stage-paddock-setup.mjs),
// without which the include's `File` has nothing to embed.
// Not checked yet, because they land with their own work packages: the app declares InstallScope::PerMachine (S6), the
// compiled update key equals plugins.updater.pubkey (S7) and no tauri-plugin-autostart on the Windows target (S5, PM-b).
import { error } from '../finding.mjs';

const INCLUDE = 'paddock-per-machine.nsh';

/** The macros each hook must call (ADR 0053 section 2). */
export const REQUIRED = {
  NSIS_HOOK_PREINSTALL: ['PADDOCK_INSTALLING', 'PADDOCK_CHECK_DIR', 'PADDOCK_GUARD_PER_USER', 'PADDOCK_STOP_ALL'],
  NSIS_HOOK_POSTINSTALL: ['PADDOCK_INSTALL_SETUP', 'PADDOCK_CLEAN_RUN_VALUES', 'PADDOCK_FINISH_INSTALL'],
  NSIS_HOOK_PREUNINSTALL: ['PADDOCK_INSTALLING', 'PADDOCK_REINSTALL_AS_UPDATE', 'PADDOCK_STOP_ALL', 'PADDOCK_CLEAN_RUN_VALUES', 'PADDOCK_REMOVE_PROGRAM_FILES'],
  NSIS_HOOK_POSTUNINSTALL: ['PADDOCK_FINISH_UNINSTALL'],
};

/** The text of one hook macro, or null when the hooks file does not define it. */
export function hookBody(hooks, name) {
  const start = hooks.indexOf(`!macro ${name}`);
  if (start < 0) return null;
  const end = hooks.indexOf('!macroend', start);
  return hooks.slice(start, end < 0 ? undefined : end);
}

const json = (ctx, path) => {
  const text = ctx.read(path);
  if (text === null || text === undefined) return null;
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
};

export default {
  id: 'C91',
  title: 'Per-machine installers use the shared include',
  rule: 42,
  run(ctx) {
    const dir = ctx.config.tauriDir;
    const base = json(ctx, `${dir}/tauri.conf.json`);
    const win = json(ctx, `${dir}/tauri.windows.conf.json`);
    const nsis = { ...base?.bundle?.windows?.nsis, ...win?.bundle?.windows?.nsis };
    if (nsis.installMode !== 'perMachine') return [];
    const f = [];
    const bad = (file, message, fix) => f.push(error('C91', 42, file, 1, message, fix));
    const confFile = ctx.exists(`${dir}/tauri.windows.conf.json`) ? `${dir}/tauri.windows.conf.json` : `${dir}/tauri.conf.json`;

    const updater = { ...base?.plugins?.updater?.windows, ...win?.plugins?.updater?.windows };
    if (updater.installMode !== 'passive') {
      bad(confFile, `the updater's windows.installMode is "${updater.installMode ?? 'unset'}", a per-machine app needs "passive" ("quiet" cannot elevate)`, 'Set plugins.updater.windows.installMode to "passive".');
    }
    if (!nsis.installerHooks) {
      bad(confFile, 'a perMachine app has no bundle.windows.nsis.installerHooks, so nothing calls the shared installer include', `Set installerHooks to a hooks file that includes ${INCLUDE}.`);
    } else {
      const hooksPath = `${dir}/${nsis.installerHooks.replace(/^\.\//, '')}`;
      const hooks = ctx.read(hooksPath);
      if (hooks === null || hooks === undefined) bad(confFile, `${hooksPath} (installerHooks) does not exist`, 'Create the hooks file.');
      else if (!hooks.includes(INCLUDE)) bad(hooksPath, `${hooksPath} does not include ${INCLUDE}`, `!include "\${__FILEDIR__}\\${INCLUDE}" (and the generated stop list before it).`);
      else {
        for (const [hook, macros] of Object.entries(REQUIRED)) {
          const body = hookBody(hooks, hook);
          for (const m of macros) {
            if (body === null || !body.includes(m)) bad(hooksPath, `${hook} does not call ${m} (ADR 0053 section 2)`, `Add !insertmacro ${m} to ${hook}.`);
          }
        }
      }
    }
    const win_dir = nsis.installerHooks ? `${dir}/${nsis.installerHooks.replace(/^\.\//, '').replace(/\/[^/]*$/, '')}` : `${dir}/windows`;
    for (const name of ['paddock-per-machine.nsh', 'paddock-per-machine.generated.nsh', 'per-machine.json']) {
      if (!ctx.exists(`${win_dir}/${name}`)) {
        bad(`${win_dir}/${name}`, `${win_dir}/${name} is missing`, 'Run `PADDOCK_WRITE_TS=1 cargo test -p paddock-service --lib per_machine` (the include copy and the stop list); per-machine.json holds the exclusions and the forced-end statements.');
      }
    }
    const wf = ctx.workflows().find((w) => w.file.endsWith(`${ctx.app}-release.yml`));
    if (!wf || !wf.text.includes('stage-paddock-setup')) {
      bad(wf?.file ?? `${ctx.config.workflowsDir}/${ctx.app}-release.yml`, `the ${ctx.app} release workflow does not stage paddock-setup.exe before the bundle (stage-paddock-setup)`, 'Pass windows-pre-bundle: node scripts/stage-paddock-setup.mjs --app <app> to app-release.yml.');
    }
    return f;
  },
};
