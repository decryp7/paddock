// C48: an app on the shared shell grants its dialog windows the shell's dialog set (ADR 0026 §3). Its
// `src-tauri/capabilities/dialog.json` must grant `paddock-shell:dialog` to `d-*` (a workbench app, one that depends on
// `paddock-workbench`, also `paddock-workbench:dialog`). Without it Tauri refuses `plugin:paddock-shell|dialog_take_input`
// and the rest from the dialog window, and the dialog never opens (the real-window probe's 2026-10-29 failure, which was
// only its harness lacking the same grant). Apps that do not depend on `paddock-shell` are not checked.
import { error } from '../finding.mjs';

const CAPABILITY = /^(.*\/)?src-tauri\/capabilities\/dialog\.json$/;
const DEPENDS = (cargo, name) => new RegExp(`^\\s*${name}\\s*=`, 'm').test(cargo);

/** What is wrong with a dialog capability (JSON text) for an app with these dependencies: messages, empty when it is right. */
export function capabilityProblems(text, { workbench }) {
  let cap;
  try {
    cap = JSON.parse(text);
  } catch (e) {
    return [`it is not valid JSON (${e.message})`];
  }
  const out = [];
  const windows = Array.isArray(cap.windows) ? cap.windows : [];
  const permissions = Array.isArray(cap.permissions) ? cap.permissions : [];
  if (!windows.includes('d-*')) out.push('it does not name the windows `d-*` (the dialog windows)');
  const need = workbench ? ['paddock-shell:dialog', 'paddock-workbench:dialog'] : ['paddock-shell:dialog'];
  for (const p of need) if (!permissions.includes(p)) out.push(`it does not grant \`${p}\``);
  return out;
}

export default {
  id: 'C48',
  title: 'Apps on the shared shell grant their dialog windows paddock-shell:dialog',
  rule: 40,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      const m = CAPABILITY.exec(f);
      if (!m) continue;
      const cargo = ctx.read(`${m[1] ?? ''}src-tauri/Cargo.toml`);
      if (!cargo || !DEPENDS(cargo, 'paddock-shell')) continue;
      for (const problem of capabilityProblems(ctx.read(f) ?? '', { workbench: DEPENDS(cargo, 'paddock-workbench') })) {
        out.push(error('C48', 40, f, 1, `the dialog capability of an app on the shared shell: ${problem}`, 'Grant `d-*` windows `core:event:default` and `paddock-shell:dialog` (and `paddock-workbench:dialog` in a workbench app), as apps/shell-demo/src-tauri/capabilities/dialog.json does.'));
      }
    }
    return out;
  },
};
