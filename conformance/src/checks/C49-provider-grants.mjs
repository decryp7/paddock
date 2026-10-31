// C49: a dialog window's providers only use commands its contract grants (ADR 0026 §3; ER-21). A window that loads a shared
// provider (`provideShellJobs` calls `jobs_snapshot` and `job_cancel`, the settings window's providers call `settings_*`) needs
// those commands in its `dialogs!` contract; Tauri's guard refuses an ungranted command, and the browser mock never does, so
// the gap only shows in the installed app (Spotter's Settings window, 2026-10-29). The check reads, per app on the shared shell:
// `generated/dialog-contracts.ts` (each dialog's commands), the app's TypeScript sources (`defineDialog(DIALOG_CONTRACTS['id'], {
// providers: () => import('./file') })` names a dialog's providers file) and, for each shared provider call found in that file,
// the commands it needs, read from the provider's own generated command table (`shell/<x>/generated/<x>-commands.ts`). A
// checkout without those tables (an app outside the monorepo) is not checked. Add a provider to PROVIDERS to cover it.
import { error } from '../finding.mjs';

/** Shared provider call -> the generated table (a path suffix) and the exported const that lists its commands. */
export const PROVIDERS = {
  provideShellJobs: { file: 'shell/jobs/generated/jobs-commands.ts', name: 'JOBS_COMMANDS' },
  settingsWindowProviders: { file: 'shell/settings/generated/settings-commands.ts', name: 'SETTINGS_COMMANDS' },
};
const BASE = ['window_pong', 'window_ready'];

/** The command names of `export const NAME = { cmd: {...}, ... }`. */
export function commandsOf(text, name) {
  const m = new RegExp(`export const ${name}\\s*=\\s*\\{([\\s\\S]*?)\\n\\}\\s*as const`).exec(text);
  return m ? [...m[1].matchAll(/^\s{2}([a-z0-9_]+)\s*:/gm)].map((x) => x[1]) : [];
}

const granted = (list, cmd) => BASE.includes(cmd) || list.some((g) => g === cmd || (g.endsWith('_*') && cmd.startsWith(g.slice(0, -1))));

/**
 * What each dialog's providers need that its contract lacks: messages. `contracts` is the generated contracts file's text,
 * `sources` the app's TypeScript ({ path: text }), `required` { providerCall: [commands] }.
 */
export function grantProblems({ contracts, sources, required }) {
  const out = [];
  for (const m of contracts.matchAll(/'([a-z0-9.-]+)':\s*\{[\s\S]*?commands:\s*\[([^\]]*)\]/g)) {
    const [, id, list] = m;
    const grants = [...list.matchAll(/'([^']+)'/g)].map((x) => x[1]);
    const files = new Set();
    for (const [path, text] of Object.entries(sources)) {
      const at = text.indexOf(`DIALOG_CONTRACTS['${id}']`);
      if (at < 0) continue;
      const next = text.indexOf('defineDialog(', at + 1);
      const call = text.slice(at, next < 0 ? undefined : next);
      for (const i of call.matchAll(/import\(\s*'(\.[^']+)'\s*\)/g)) {
        const base = path.split('/').slice(0, -1);
        for (const part of i[1].split('/')) part === '..' ? base.pop() : part !== '.' && base.push(part);
        files.add(`${base.join('/')}.ts`);
      }
    }
    for (const file of files) {
      const text = sources[file] ?? '';
      for (const [call, commands] of Object.entries(required)) {
        if (!new RegExp(`\\b${call}\\s*\\(`).test(text)) continue;
        const missing = commands.filter((c) => !granted(grants, c));
        if (missing.length) out.push(`dialog \`${id}\`: ${file} calls ${call}(), which uses ${missing.sort().join(', ')}, but the contract does not grant ${missing.length > 1 ? 'them' : 'it'}`);
      }
    }
  }
  return out;
}

export default {
  id: 'C49',
  title: 'A dialog window\'s providers only use commands its contract grants',
  rule: 40,
  run(ctx) {
    const files = ctx.files();
    const required = {};
    for (const [call, { file, name }] of Object.entries(PROVIDERS)) {
      const f = files.find((x) => x === file || x.endsWith(`/${file}`));
      const text = f && ctx.read(f);
      if (text) required[call] = commandsOf(text, name);
    }
    if (!Object.keys(required).length) return [];
    const out = [];
    for (const cargoPath of files.filter((f) => /(^|\/)src-tauri\/Cargo\.toml$/.test(f))) {
      if (!/^\s*paddock-shell\s*[.=]/m.test(ctx.read(cargoPath) ?? '')) continue;
      const appDir = cargoPath.replace(/src-tauri\/Cargo\.toml$/, '');
      const own = files.filter((f) => f.startsWith(appDir) && f.endsWith('.ts') && f.includes('/src/') && !f.endsWith('.spec.ts'));
      const contractsFile = own.find((f) => f.endsWith('generated/dialog-contracts.ts'));
      if (!contractsFile) continue;
      const sources = Object.fromEntries(own.map((f) => [f, ctx.read(f) ?? '']));
      for (const message of grantProblems({ contracts: ctx.read(contractsFile) ?? '', sources, required })) {
        out.push(error('C49', 40, contractsFile, 1, message, 'Add the commands to the dialog\'s `commands` in the app\'s `dialogs!` table (`shell::jobs::jobs_snapshot`, `shell::jobs::job_cancel`, `prefix(shell::settings::ALL, "settings_")`), then regenerate the contracts and the manifest.'));
      }
    }
    return out;
  },
};
