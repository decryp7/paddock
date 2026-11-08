// C48: an app on the shared shell grants its dialog windows the shell's dialog set (ADR 0026 §3). Every `src-tauri` whose
// `Cargo.toml` depends on `paddock-shell` needs capabilities (`capabilities/*.json`) that name the dialog windows `d-*`
// (as `windows` or `webviews`) and together grant `core:event:default` (the title-bar close request is an event) and
// `paddock-shell:dialog`; one that also depends on `paddock-workbench` also `paddock-workbench:dialog`. Without them Tauri
// refuses `plugin:paddock-shell|dialog_take_input` and the rest from the dialog window and the dialog never opens (the
// real-window probe's 2026-10-29 failure, which was only its harness lacking the same grant). The check starts from the
// Cargo.toml, so an app with no dialog capability at all is caught too. Apps that do not depend on `paddock-shell` are
// not checked.
import { error } from '../finding.mjs';

const CARGO = /^(.*\/)?src-tauri\/Cargo\.toml$/;
const dependsOn = (cargo, name) =>
  new RegExp(
    [
      `^\\s*${name}\\s*[.=]`, // name = ..., name.workspace = true, name.path = ...
      `^\\s*\\[[^\\]\\n]*dependencies\\.${name}\\]`, // [dependencies.name], [target.x.dependencies.name]
      `\\bpackage\\s*=\\s*"${name}"`, // renamed: alias = { package = "name" }
    ].join('|'),
    'm',
  ).test(cargo);

const idOf = (p) => (typeof p === 'string' ? p : typeof p?.identifier === 'string' ? p.identifier : '');

/**
 * What the capabilities (JSON texts) fail to give the dialog windows: messages, empty when it is right.
 * Capabilities that are not valid JSON are reported, and the ones that do not name `d-*` are ignored.
 */
export function capabilityProblems(texts, { workbench }) {
  const out = [];
  const granted = new Set();
  let named = false;
  for (const { file, text } of texts) {
    let cap;
    try {
      cap = JSON.parse(text);
    } catch (e) {
      out.push(`${file} is not valid JSON (${e.message})`);
      continue;
    }
    const labels = [...(Array.isArray(cap.windows) ? cap.windows : []), ...(Array.isArray(cap.webviews) ? cap.webviews : [])];
    if (!labels.includes('d-*')) continue;
    named = true;
    for (const p of Array.isArray(cap.permissions) ? cap.permissions : []) granted.add(idOf(p));
  }
  if (!named) {
    out.push('no capability names the dialog windows `d-*` (in `windows` or `webviews`)');
    return out;
  }
  const need = ['core:event:default', 'paddock-shell:dialog', ...(workbench ? ['paddock-workbench:dialog'] : [])];
  for (const p of need) if (!granted.has(p)) out.push(`no capability for \`d-*\` grants \`${p}\``);
  return out;
}

export default {
  id: 'C48',
  scope: 'both',
  mode: 'gate',
  title: 'Apps on the shared shell grant their dialog windows paddock-shell:dialog',
  rule: 40,
  run(ctx) {
    const out = [];
    const files = ctx.files();
    for (const f of files) {
      const m = CARGO.exec(f);
      if (!m) continue;
      const cargo = ctx.read(f);
      if (!cargo || !dependsOn(cargo, 'paddock-shell')) continue;
      const dir = `${m[1] ?? ''}src-tauri/capabilities/`;
      const texts = files
        .filter((x) => x.startsWith(dir) && x.endsWith('.json') && !x.slice(dir.length).includes('/'))
        .map((file) => ({ file, text: ctx.read(file) ?? '' }));
      for (const problem of capabilityProblems(texts, { workbench: dependsOn(cargo, 'paddock-workbench') })) {
        out.push(error('C48', 40, f, 1, `the dialog capability of an app on the shared shell: ${problem}`, 'Add `capabilities/dialog.json` for `d-*` with `core:event:default` and `paddock-shell:dialog` (and `paddock-workbench:dialog` in a workbench app), as apps/shell-demo/src-tauri/capabilities/dialog.json does.'));
      }
    }
    return out;
  },
};
