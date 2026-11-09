// C06: every workflow step that hands TAURI_SIGNING_PRIVATE_KEY or TAURI_UPDATER_PUBKEY to a build strips whitespace first
// (or calls a script / shared helper that does). Catches B09 (Pitwall's base64 "Invalid symbol 10").
import { error } from '../finding.mjs';
import { referencedScripts } from './C04-macos-signing.mjs';

export const KEYS = ['TAURI_SIGNING_PRIVATE_KEY', 'TAURI_UPDATER_PUBKEY'];

/** True when `text` strips `key`: tr -d / sed / .replace(/\s/) on a line that names it, or the shared strip-keys helper. */
export function stripsKey(text, key) {
  if (/strip-keys|strip-signing-keys/.test(text)) return true;
  return text.split('\n').some((l) => l.includes(key) && /tr\s+-d\s+['"][^'"]*(?:\\n|\\r|\s)|\.replace\(\s*\/\\s|sed\s+[^\n]*\\s|gsub\(/.test(l));
}

export default {
  id: 'C06',
  scope: 'both',
  mode: 'gate',
  title: 'Signing keys are stripped of whitespace before the bundler reads them',
  rule: 25,
  run(ctx) {
    const out = [];
    const wf = ctx.workflows();
    if (!wf.length) return [error('C06', 25, ctx.config.workflowsDir, 1, 'no workflow files found', 'Point workflowsDir at the folder with the build workflow.')];
    for (const w of wf) {
      for (const step of w.steps) {
        const keys = KEYS.filter((k) => k in step.env);
        if (!keys.length) continue;
        const scripts = referencedScripts(ctx, step.run ?? '').map((p) => ctx.read(p) ?? '');
        const text = [step.run ?? '', step.uses ?? '', ...scripts].join('\n');
        for (const key of keys) {
          if (!stripsKey(text, key)) {
            out.push(error('C06', 25, w.file, step.line, `step "${step.name ?? step.uses ?? 'unnamed'}" passes ${key} to the build without stripping whitespace`, `Run \`eval "$(node conformance/release/strip-keys.mjs)"\` (or tr -d ' \\t\\r\\n') before the bundler.`));
          }
        }
      }
    }
    return out;
  },
};
